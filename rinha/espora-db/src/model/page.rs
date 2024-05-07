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
}
