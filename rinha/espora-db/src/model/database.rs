use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, Write},
    marker::PhantomData,
    os::windows::fs::FileExt,
    path::Path,
    time::{Duration, Instant},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{error::Error, model::page::PAGE_SIZE};

use super::{builder::Builder, page::Page};

pub struct Db<T, const ROM_SIZE: usize> {
    current_page: Page<ROM_SIZE>,
    reader: File,
    writer: File,
    last_sync: Instant,
    pub(crate) sync_writes: Option<Duration>,
    data: PhantomData<T>,
}

pub(crate) type DbResult<T> = Result<T, Error>;

impl<const ROW_SIZE: usize, T: Serialize + DeserializeOwned> Db<T, ROW_SIZE> {
    pub fn builder() -> Builder {
        Builder::default()
    }

    pub fn from_path(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;

        let current_page = if file.seek(io::SeekFrom::End(-(PAGE_SIZE as i64))).is_ok() {
            let mut buf = vec![0; PAGE_SIZE];

            file.read_exact(&mut buf)?;
            file.seek(io::SeekFrom::End(-(PAGE_SIZE as i64)))?;

            Page::from_bytes(buf)
        } else {
            file.seek(io::SeekFrom::End(0))?;
            Page::new()
        };

        Ok(Self {
            current_page,
            reader: File::open(&path)?,
            writer: file,
            last_sync: Instant::now(),
            sync_writes: Some(Duration::from_secs(0)),
            data: PhantomData,
        })
    }

    pub fn insert(&mut self, row: T) -> DbResult<()> {
        self.current_page.insert(row)?;

        self.writer.write_all(
            &[
                self.current_page.as_ref(),
                &vec![0; PAGE_SIZE - self.current_page.as_ref().len()],
            ]
            .concat(),
        )?;

        match self.sync_writes {
            Some(interval) if self.last_sync.elapsed() > interval => {
                self.writer.sync_data()?;
                self.last_sync = Instant::now();
            }
            _ => {}
        }

        if self.current_page.available_rows() == 0 {
            self.current_page = Page::new();
        } else {
            self.writer.seek(io::SeekFrom::End(-(PAGE_SIZE as i64)))?;
        }

        Ok(())
    }
}
