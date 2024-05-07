#[cfg(test)]
mod test {
    use crate::{
        model::page::{Page, PAGE_SIZE},
        test,
    };

    #[test]
    fn initialize() {
        let page = Page::<1024>::new();

        assert_eq!(0, page.len());
        assert_eq!(PAGE_SIZE, page.available_space());
    }

    #[test]
    fn initialize_with_empty_bytes() {
        let page = Page::<1024>::from_bytes(vec![]);

        assert_eq!(0, page.len());
        assert_eq!(PAGE_SIZE, page.available_space());
    }

    #[test]
    fn initialize_from_bytes() {
        let mut page = Page::<1024>::from_bytes(vec![]);

        page.insert(1).unwrap();
        page.insert(2).unwrap();

        let new_page = Page::<1024>::from_bytes(page.as_ref().to_vec());

        assert_eq!(page.len(), new_page.len());
        assert_eq!(page.available_rows(), new_page.available_rows());
        assert_eq!(page.available_space(), new_page.available_space());
    }

    #[test]
    fn insert_into_page() {
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

    #[test]
    fn update_existing_page() {
        let mut page = Page::<1024>::from_bytes(vec![]);
        page.insert("Rinha").unwrap();
        page.insert("De").unwrap();

        let mut page = Page::<1024>::from_bytes(page.as_ref().to_vec());
        page.insert("Backend").unwrap();
        page.insert("2024").unwrap();

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
            "Backend",
            bitcode::deserialize::<String>(&rows.next().unwrap()).unwrap()
        );
        assert_eq!(
            "2024",
            bitcode::deserialize::<String>(&rows.next().unwrap()).unwrap()
        );
        assert!(rows.next().is_none());
    }
}
