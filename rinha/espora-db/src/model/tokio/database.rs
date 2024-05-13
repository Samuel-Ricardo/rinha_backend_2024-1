use std::marker::PhantomData;

use tokio::{
    fs::File,
    time::{Duration, Instant},
};

use crate::model::page::Page;

pub struct Db<T, const ROW_SIZE: usize> {
    current_page: Page<ROW_SIZE>,
    reader: File,
    write: File,
    last_sync: Instant,
    pub(crate) sync_writes: Option<Duration>,
    data: PhantomData<T>,
}
