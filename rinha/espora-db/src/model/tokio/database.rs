use std::marker::PhantomData;

use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    fs::File,
    time::{Duration, Instant},
};

use crate::model::builder::Builder;
use crate::model::page::Page;

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
}
