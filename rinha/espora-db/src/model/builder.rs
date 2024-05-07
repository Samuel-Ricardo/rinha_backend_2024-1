use std::time::Duration;

#[derive(Debug)]
pub struct Builder {
    sync_writes: Option<Duration>,
}

impl Default for Builder {
    fn default() -> Self {
        Builder {
            sync_writes: Some(Duration::from_secs(0)),
        }
    }
}
