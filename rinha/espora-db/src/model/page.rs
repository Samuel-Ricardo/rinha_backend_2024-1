pub struct Page<const ROW_SIZE: usize> {
    data: Vec<u8>,
    free: usize,
}
