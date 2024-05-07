use std::{
    fs::File,
    marker::PhantomData,
    time::{Duration, Instant},
};

pub struct Db<T, const ROM_SIZE: usize> {
    //    current_page: Page<ROM_SIZE>,
    reader: File,
    writer: File,
    last_sync: Instant,
    pub(crate) sync_writes: Option<Duration>,
    data: PhantomData<T>,
}
