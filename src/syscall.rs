use crate::dev::DEV_TREE;
use crate::usr::SStatusBits;
use crate::{arch, debug, kernel_do_no_thing, numeric, print};

numeric! {
    pub enum ErrorCode : u64 {
        EINVAL = !22 + 1,
        EIO = !5 + 1,
        ENOSYS = !38 + 1,
    }
}

numeric! {
    pub enum Syscall: u64 {
        READ = 0,
        WRITE = 1,
        EXIT = 60,
    }
}

fn read(_fd: u64, buf: &mut [u8]) -> u64 {
    let uart = match DEV_TREE.force().ns16550a.as_ref() {
        Some(u) => u,
        None => return ErrorCode::EIO.into(),
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
            (args[0], kernel_do_no_thing as *const () as u64)
        }
        _ => (ErrorCode::ENOSYS.into(), sepc + 4),
    }
}
