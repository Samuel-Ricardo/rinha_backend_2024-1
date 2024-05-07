use std::time::Duration;

#[derive(Debug)]
pub struct Builder {
    sync_writes: Option<Duration>,
}
