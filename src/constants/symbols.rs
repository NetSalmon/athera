//! 链接器符号。
//!
//! `_end` / `trap_entry` 由链接脚本与 `entry.asm` 提供；`FDT_ADDR` 是
//! `entry.asm` 预留的 8 字节槽位，启动时由 `a1` 写入设备树地址。
unsafe extern "C" {
    pub fn _end();
    pub fn trap_entry();
    pub static FDT_ADDR: *const u8;
}
