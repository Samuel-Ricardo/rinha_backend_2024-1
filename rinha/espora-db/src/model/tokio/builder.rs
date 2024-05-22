use std::path::Path;

use serde::{de::DeserializeOwned, Serialize};
use tokio::io;

use crate::model::builder::Builder;

use super::database::Db;

impl Builder {
    pub async fn build_tokio<T: Serialize + DeserializeOwned, const ROW_SIZE: usize>(
        self,
        path: impl AsRef<Path>,
    ) -> io::Result<Db<T, ROW_SIZE>> {
        let mut db = Db::from_path(path).await?;
        db.sync_writes = self.sync_writes;
        Ok(db)
    }
}
