use crate::arch::sbi;
use crate::arch::sbi::srst::{ResetReason, ResetType, system_reset};
use crate::dev::DEV_TREE;
use crate::usr::SStatusBits;
use crate::{arch, debug, kernel_halt, numeric, print};

numeric! {
    pub enum ErrorCode : isize {
        EINVAL = -22,
        EIO = -5,
        ENOSYS = -38,
    }
}

numeric! {
    pub enum Syscall: u64 {
        READ = 63,
        WRITE = 64,
        EXIT = 93,
        REBOOT = 142,
        FORK = 220,
        WAITPID = 95,
        EXEC = 221,
    }
}

numeric! {
    pub enum RebootCmd : u64 {
        RESTART = 0x1234567,
        POWER_OFF = 0x4321fedc,
        HALT = 0xcdef0123,
    }
}

fn read(_fd: u64, buf: &mut [u8]) -> u64 {
    let uart = match DEV_TREE.force().ns16550a.as_ref() {
        Some(u) => u,
        None => return ErrorCode::EIO.0 as u64,
    };

    let mut bytes_read = 0;
    for i in buf.iter_mut() {
        if let Some(ch) = uart.lock().getchar() {
            *i = ch;
            bytes_read += 1;
        } else {
            break;
        }
    }
    bytes_read
}

fn write(_fd: u64, buf: &[u8]) -> u64 {
    for i in buf.iter() {
        print!("{}", *i as char);
    }
    buf.len() as u64
}

fn reboot(magic: u64, magic2: u64, cmd: u64) -> isize {
    if magic != 0xfee1dead {
        return -1;
    }
    if magic2 != 0x28121969 {
        return -1;
    }

    match RebootCmd::from(cmd) {
        RebootCmd::POWER_OFF => {
            system_reset(ResetType::Shutdown, ResetReason::None);
        }
        RebootCmd::RESTART => {
            system_reset(ResetType::ColdReboot, ResetReason::None);
        }
        RebootCmd::HALT => {
            sbi::hsm::hart_stop();
        }
        _ => return -1,
    }
    -1
}

pub fn handle(args: &[u64; 8], sepc: u64) -> (u64, u64) {
    match Syscall::from(args[7]) {
        Syscall::READ => {
            let ptr = args[1] as *mut u8;
            let buf = core::ptr::slice_from_raw_parts_mut(ptr, args[2] as usize);
            let ret = read(args[0], unsafe { &mut *buf });
            (ret, sepc + 4)
        }
        Syscall::WRITE => {
            let ptr = args[1] as *mut u8;
            let buf = core::ptr::slice_from_raw_parts_mut(ptr, args[2] as usize);
            let buf = unsafe { &*buf };
            let ret = write(args[0], buf);
            (ret, sepc + 4)
        }
        Syscall::EXIT => {
            debug!("user program exit, code: {}", args[0] as i32);
            let mut s: SStatusBits = arch::registers::csr::Sstatus::read().into();
            s.set_spp(true);
            arch::registers::csr::Sstatus::write(s.into());
            (args[0], kernel_halt as *const () as u64)
        }
        Syscall::REBOOT => {
            let ret = reboot(args[0], args[1], args[2]);

            (ret as u64, sepc + 4)
        }
        _ => (ErrorCode::ENOSYS.0 as u64, sepc + 4),
    }
}
