use std::io::Cursor;
use std::iter;

pub struct Page<const ROW_SIZE: usize> {
    data: Vec<u8>,
    free: usize,
}

pub const PAGE_SIZE: usize = 4096;

impl<const ROW_SIZE: usize> Page<ROW_SIZE> {
    pub fn new() -> Self {
        Self {
            data: Vec::with_capacity(PAGE_SIZE),
            free: PAGE_SIZE,
        }
    }

    pub fn from_bytes(data: Vec<u8>) -> Self {
        let free = {
            let mut cursor = 0;

            let last_page_offset = iter::from_fn(|| {
                let offset = cursor * ROW_SIZE;

                if offset + ROW_SIZE > data.len() {
                    return None;
                }
                cursor += 1;

                if data[offset..offset + 8] != [0; 8] {
                    Some(offset + ROW_SIZE)
                } else {
                    None
                }
            })
            .last()
            .unwrap_or(0);
            PAGE_SIZE - last_page_offset
        };

        Self { data, free }
    }
}
