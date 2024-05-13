use std::{marker::PhantomData, path::Path};

use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    fs::{File, OpenOptions},
    io::{self, AsyncReadExt, AsyncSeekExt},
    time::{Duration, Instant},
};

use crate::model::page::Page;
use crate::model::{builder::Builder, page::PAGE_SIZE};

pub struct Db<T, const ROW_SIZE: usize> {
    current_page: Page<ROW_SIZE>,
    reader: File,
    write: File,
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
            write: file,
            last_sync: Instant::now(),
            sync_writes: Some(Duration::from_secs(0)),
            data: PhantomData,
        })
    }
}
