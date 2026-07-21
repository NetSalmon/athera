unsafe extern "C" {
    pub fn _end();
    pub fn trap_entry();
    pub static FDT_ADDR: *const u8;
}
