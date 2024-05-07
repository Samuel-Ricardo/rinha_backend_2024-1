#[cfg(test)]
mod test {
    use crate::model::page::Page;

    #[test]
    fn test_insert_into_page() {
        let mut page = Page::<1024>::new();
        assert_eq!(4, page.available_rows());

        page.insert(String::from("Rinha")).unwrap();
        assert_eq!(3, page.available_rows());

        page.insert(String::from("De")).unwrap();
        assert_eq!(2, page.available_rows());

        page.insert(2024 as u64).unwrap();
        assert_eq!(1, page.available_rows());

        let mut rows = page.rows();

        assert_eq!(
            "Rinha",
            bitcode::deserialize::<String>(&rows.next().unwrap()).unwrap()
        );

        assert_eq!(
            "De",
            bitcode::deserialize::<String>(&rows.next().unwrap()).unwrap()
        );

        assert_eq!(
            2024 as u64,
            bitcode::deserialize::<u64>(&rows.next().unwrap()).unwrap()
        );

        assert!(rows.next().is_none());
    }
}
