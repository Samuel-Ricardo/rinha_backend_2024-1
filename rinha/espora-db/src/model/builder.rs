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

impl Builder {
    pub fn sync_writes(self, sync_writes: bool) -> Self {
        Self {
            sync_writes: if sync_writes {
                Some(Duration::from_secs(0))
            } else {
                None
            },
        }
    }

    pub fn sync_write_interval(self, interval: Duration) -> Self {
        Self {
            sync_writes: Some(interval),
        }
    }
}
