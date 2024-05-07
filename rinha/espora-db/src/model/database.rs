use std::{
    fs::File,
    marker::PhantomData,
    time::{Duration, Instant},
};

use crate::error::Error;

pub struct Db<T, const ROM_SIZE: usize> {
    //    current_page: Page<ROM_SIZE>,
    reader: File,
    writer: File,
    last_sync: Instant,
    pub(crate) sync_writes: Option<Duration>,
    data: PhantomData<T>,
}

pub(crate) type DbResult<T> = Result<T, Error>;
