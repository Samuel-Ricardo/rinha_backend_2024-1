#[cfg(test)]
mod test {
    use std::env::temp_dir;

    use futures::TryStreamExt;

    use crate::model::tokio::database::Db;

    #[tokio::test]
    async fn test_db_rows() {
        let tmp = temp_dir();
        let mut db = Db::<i64, 2048>::from_path(tmp.as_path().join("test.espora"))
            .await
            .unwrap();

        db.insert(1).await.unwrap();
        db.insert(2).await.unwrap();
        db.insert(3).await.unwrap();
        db.insert(4).await.unwrap();
        db.insert(5).await.unwrap();

        let row = db.rows().try_collect::<Vec<_>>().await.unwrap();
        assert_eq!(row.len(), 5);
        assert_eq!(row, vec![1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn test_db_rows_reverse() {
        let tmp = temp_dir();

        let mut db = Db::<i64, 2048>::from_path(tmp.as_path().join("test.espora"))
            .await
            .unwrap();

        db.insert(1).await.unwrap();
        db.insert(2).await.unwrap();
        db.insert(3).await.unwrap();
        db.insert(4).await.unwrap();
        db.insert(5).await.unwrap();

        let rows = db.rows_reverse().try_collect::<Vec<_>>().await.unwrap();
        assert_eq!(rows, vec![5, 4, 3, 2, 1]);
    }
}
