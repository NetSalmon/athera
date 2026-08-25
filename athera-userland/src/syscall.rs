//! 用户态系统调用封装。
//!
//! 定义 `ecall!` 宏与 read / write / exit / reboot / fork / execve 的
//! RISC-V 调用约定封装（系统调用号放 `a7`，返回值经 `a0` 传回）。

extern crate alloc;

use alloc::vec::Vec;

#[allow(unused)]
const EINVAL: isize = -22;
#[allow(unused)]
const EIO: isize = -5;
#[allow(unused)]
const ENOSYS: isize = -38;
const READ: u64 = 63;
const WRITE: u64 = 64;
const EXIT: u64 = 93;
const REBOOT: u64 = 142;
const MUNMAP: u64 = 215;
const MREMAP: u64 = 216;
const CLONE: u64 = 220;
const EXECVE: u64 = 221;
const MMAP: u64 = 222;

// mmap 的 prot / flags 与 mremap 的 flags（对齐 Linux asm-generic ABI）。
pub const PROT_READ: usize = 0x1;
pub const PROT_WRITE: usize = 0x2;
pub const PROT_EXEC: usize = 0x4;
pub const MAP_SHARED: usize = 0x01;
pub const MAP_PRIVATE: usize = 0x02;
pub const MAP_FIXED: usize = 0x10;
pub const MAP_ANONYMOUS: usize = 0x20;
pub const MAP_FIXED_NOREPLACE: usize = 0x100000;
pub const MREMAP_MAYMOVE: usize = 1;
pub const MREMAP_FIXED: usize = 2;

#[macro_export]
macro_rules! ecall {
    ($syscall:expr $(=> $($register:tt = $value:expr),*$(,)?)?) => {
        {
            let ret: isize;
            #[allow(clippy::macro_metavars_in_unsafe)]
            unsafe {
                core::arch::asm! (
                    "ecall",
                    $(
                    $(
                    in($register) $value,
                    )*
                    )?
                    in("a7") $syscall,
                    lateout("a0") ret
                )
            }
            ret
        }
    };
}

#[repr(transparent)]
pub struct RebootCmd(u64);

impl RebootCmd {
    pub const HALT: Self = Self(0xcdef0123);
    pub const POWER_OFF: Self = Self(0x4321fedc);
    pub const RESTART: Self = Self(0x1234567);
}

pub fn read(fd: u64, buf: &mut [u8]) -> isize {
    ecall!(READ => "a0" = fd, "a1" = buf.as_ptr(), "a2" = buf.len())
}

pub fn write(fd: u64, buf: &[u8]) -> isize {
    ecall!(WRITE => "a0" = fd, "a1" = buf.as_ptr(), "a2" = buf.len())
}

pub fn reboot(cmd: RebootCmd) -> isize {
    ecall!(REBOOT => "a0" = 0xfee1deadu64, "a1" = 0x28121969, "a2" = cmd.0)
}

/// 创建子进程（fork 语义：内核当前忽略 clone 的 flags / stack 参数）。
pub fn fork() -> isize {
    ecall!(CLONE)
}

/// 建立匿名内存映射，返回起始地址；出错返回负 errno。
///
/// 内核当前只支持 `MAP_ANONYMOUS`：`fd` 必须为 `-1`、`offset` 为 `0`。
pub fn mmap(
    addr: usize,
    length: usize,
    prot: usize,
    flags: usize,
    fd: isize,
    offset: usize,
) -> isize {
    ecall!(MMAP => "a0" = addr, "a1" = length, "a2" = prot, "a3" = flags, "a4" = fd as usize, "a5" = offset)
}

/// 解除 `[addr, addr + length)` 的映射；成功返回 0。
pub fn munmap(addr: usize, length: usize) -> isize {
    ecall!(MUNMAP => "a0" = addr, "a1" = length)
}

/// 扩大/缩小/移动内存映射；返回新的起始地址，出错返回负 errno。
pub fn mremap(
    old_address: usize,
    old_size: usize,
    new_size: usize,
    flags: usize,
    new_address: usize,
) -> isize {
    ecall!(MREMAP => "a0" = old_address, "a1" = old_size, "a2" = new_size, "a3" = flags, "a4" = new_address)
}

/// 把 `s` 拷贝成 NUL 结尾的字节缓冲。
fn cstring(s: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(s.len() + 1);
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
    buf
}

/// 以 `path` 加载并执行新程序，替换当前进程的地址空间与用户上下文。
///
/// 成功时不返回（内核按新程序入口继续执行）；失败返回负的 errno，如
/// `-ENOENT`。`argv[0]` 依惯例为程序名，`envp` 为形如 `KEY=VALUE` 的
/// 环境变量列表。
///
/// # Examples
///
/// ```
/// syscall::execve("/bin/print_args", &["/bin/print_args", "--help"], &[]);
/// ```
pub fn execve(path: &str, argv: &[&str], envp: &[&str]) -> isize {
    // 内核按 C 约定读取参数：每个字符串以 NUL 结尾、argv/envp 数组以
    // NULL 指针结尾，故先拷贝成 NUL 结尾缓冲再收集其指针。各缓冲由
    // `Vec` 持有，在 `ecall` 期间保持有效。
    let path_buf = cstring(path);
    let argv_bufs: Vec<Vec<u8>> = argv.iter().map(|arg| cstring(arg)).collect();
    let envp_bufs: Vec<Vec<u8>> = envp.iter().map(|env| cstring(env)).collect();

    let mut argv_ptrs: Vec<*const u8> = argv_bufs.iter().map(Vec::as_ptr).collect();
    argv_ptrs.push(core::ptr::null());
    let mut envp_ptrs: Vec<*const u8> = envp_bufs.iter().map(Vec::as_ptr).collect();
    envp_ptrs.push(core::ptr::null());

    ecall!(
        EXECVE => "a0" = path_buf.as_ptr(), "a1" = argv_ptrs.as_ptr(), "a2" = envp_ptrs.as_ptr()
    )
}

/// 以状态码 `code` 结束当前进程（供 `_start` 汇编入口调用，故 `no_mangle`）。
#[unsafe(no_mangle)]
pub fn exit(code: u64) -> ! {
    ecall!(EXIT => "a0" = code);

    loop {
        core::hint::spin_loop()
    }
}
