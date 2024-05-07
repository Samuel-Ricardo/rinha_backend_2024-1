use std::io::{Cursor, Seek, SeekFrom, Write};
use std::iter;

use serde::Serialize;

use super::database::DbResult;

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

    pub fn insert<S: Serialize>(&mut self, row: S) -> DbResult<()> {
        let serialized = bitcode::serialize(&row)?;
        let size = (serialized.len() as u64).to_be_bytes();

        let mut cursor = Cursor::new(&mut self.data);
        cursor.seek(SeekFrom::Start((PAGE_SIZE - self.free) as u64))?;

        self.free -= cursor.write(&size)?;
        self.free -= cursor.write(&serialized)?;
        self.free -= cursor.write(&vec![0; ROW_SIZE - (serialized.len() + size.len())])?;

        Ok(())
    }

    pub fn rows(&self) -> impl Iterator<Item = &[u8]> {
        let mut cursor = 0;

        iter::from_fn(move || {
            let offset = cursor * ROW_SIZE;
            if offset + ROW_SIZE > self.data.len() {
                return None;
            }

            let row = &self.data[offset..offset + ROW_SIZE];
            let size = {
                let mut buf = [0; 8];
                buf.copy_from_slice(&row[0..8]);
                u64::from_be_bytes(buf) as usize
            };

            if size == 0 {
                return None;
            }

            cursor += 1;
            Some(&row[8..8 + size])
        })
    }
}
