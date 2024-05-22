use std::{
    marker::PhantomData,
    os::windows::io::{AsHandle, AsRawHandle},
    path::Path,
    sync::Arc,
};

use futures::{io::Cursor, stream, Stream, StreamExt};

use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    fs::{File, OpenOptions},
    io::{self, AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::oneshot,
    task,
    time::{Duration, Instant},
};

use crate::model::{database::DbResult, page::Page};
use crate::{
    lock::LockHandle,
    model::{builder::Builder, page::PAGE_SIZE},
};

pub struct Db<T, const ROW_SIZE: usize> {
    current_page: Page<ROW_SIZE>,
    reader: File,
    writer: File,
    last_sync: Instant,
    pub(crate) sync_writes: Option<Duration>,
    data: PhantomData<T>,
}

impl<const ROW_SIZE: usize, T: Serialize + DeserializeOwned> Db<T, ROW_SIZE> {
    pub fn builder() -> Builder {
        Builder::default()
    }

    pub async fn from_path(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .await?;

        let current_page = if file
            .seek(io::SeekFrom::End(-(PAGE_SIZE as i64)))
            .await
            .is_ok()
        {
            let mut buf = vec![0; PAGE_SIZE];
            file.read_exact(&mut buf).await?;
            file.seek(io::SeekFrom::End(-(PAGE_SIZE as i64))).await?;
            Page::from_bytes(buf)
        } else {
            file.seek(io::SeekFrom::End(0)).await?;
            Page::new()
        };

        Ok(Self {
            current_page,
            reader: File::open(&path).await?,
            writer: file,
            last_sync: Instant::now(),
            sync_writes: Some(Duration::from_secs(0)),
            data: PhantomData,
        })
    }

    pub async fn insert(&mut self, row: T) -> DbResult<()> {
        self.current_page.insert(row)?;

        self.writer
            .write_all(
                &[
                    self.current_page.as_ref(),
                    &vec![0; PAGE_SIZE - self.current_page.len()],
                ]
                .concat(),
            )
            .await?;

        match self.sync_writes {
            Some(interval) if self.last_sync.elapsed() > interval => {
                self.writer.sync_data().await?;
                self.last_sync = Instant::now();
            }
            _ => {}
        }

        if self.current_page.available_rows() == 0 {
            self.current_page = Page::new();
        } else {
            self.writer
                .seek(io::SeekFrom::End(-(PAGE_SIZE as i64)))
                .await?;
        }

        Ok(())
    }

    fn pages(&mut self) -> impl Stream<Item = Page<ROW_SIZE>> + '_ {
        let mut cursor = 0;

        stream! {
            loop {
                let offset = (cursor * PAGE_SIZE) as u64;

                if self.reader.seek(io::SeekFrom::Start(offset)).await.is_err() {
                    break;
                }

                let mut buf = vec![0; PAGE_SIZE];
                cursor += 1;

                match self.reader.read_exact(&mut buf).await {
                    Ok(n) if n > 0 => yield Page::<ROW_SIZE>::from_bytes(buf),
                    _ => break,
                }
            }
        }
    }

    fn pages_reverse(&mut self) -> impl Stream<Item = Page<ROW_SIZE>> + '_ {
        let mut cursor = 1;

        stream! {
            loop {
                let offset = (cursor * PAGE_SIZE) as u64;

                if self.reader.seek(io::SeekFrom::End(-offset)).await.is_err() {
                    break;
                }

                let mut buf = vec![0; PAGE_SIZE];
                cursor += 1;

                match self.reader.read_exact(&mut buf).await {
                    Ok(n) if n > 0 => yield Page::<ROW_SIZE>::from_bytes(buf),
                    _ => break,
                }
            }
        }
    }

    pub fn rows(&mut self) -> impl Stream<Item = DbResult<t>> + '_ {
        self.pages().flat_map(|page| {
            stream::iter(
                page.rows()
                    .map(|row| bitcode::deserialize(row).map_err(|err| err, into()))
                    .collect::<Vec<_>>(),
            )
        })
    }

    //TODO: REFACT ALL TO LIBC

    pub async fn lock_writes(&mut self) -> DbResult<LockHandle> {
        let (tx, rx) = oneshot::channel();
        let fd = self.writer.as_raw_fd();
        task::spawn_blocking(move || match unsafe { libc::flock(fd, libc::LOCK_EX) } {
            0 => tx.send(Ok(LockHandle { fd })),
            _ => tx.send(Err(io::Error::new(
                io::ErrorKind::Other,
                "couldn't acquire lock",
            ))),
        });
        Ok(rx.await.unwrap()?)
    }
}
