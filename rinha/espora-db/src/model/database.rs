use std::{
    fs::File,
    marker::PhantomData,
    time::{Duration, Instant},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::error::Error;

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
}
