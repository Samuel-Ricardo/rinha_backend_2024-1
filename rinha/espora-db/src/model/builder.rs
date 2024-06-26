use std::{io, path::Path, time::Duration};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::database::Db;

#[derive(Debug)]
pub struct Builder {
    pub(crate) sync_writes: Option<Duration>,
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

    pub fn build<T: Serialize + DeserializeOwned, const ROM_SIZE: usize>(
        self,
        path: impl AsRef<Path>,
    ) -> io::Result<Db<T, ROM_SIZE>> {
        let mut db = Db::from_path(path)?;
        db.sync_writes = self.sync_writes;
        Ok(db)
    }
    /* TODO: TOKIO ASYNC DB
    #[cfg(feature = "tokio")]
    pub async fn build_tokio<T: Serialize + DeserializeOwned, const ROM_SIZE: usize>(
        self,
        path: impl AsRef<Path>,
    ) -> io::Result<Db<T, ROM_SIZE>> {
    }
    */
}
