use crate::dev::DEV_TREE;
use crate::{numeric, print, println};

numeric! {
    pub enum Syscall: u64 {
        READ = 0,
        WRITE = 1,
        EXIT = 60,
    }
}

fn read(_fd: u64, buf: &mut [u8]) -> isize {
    let uart = match DEV_TREE.force().ns16550a.as_ref() {
        Some(u) => u,
        None => return -1,
    };
    for i in buf.iter_mut() {
        let ch = if let Some(ch) = uart.lock().getchar() {
            ch
        } else {
            return -1;
        };
        *i = ch;
    }
    0
}

fn write(_fd: u64, buf: &[u8]) -> isize {
    for i in buf.iter() {
        print!("{}", *i as char);
    }
    0
}

pub fn handle(args: [u64; 8]) -> u64 {
    match args[7].into() {
        Syscall::READ => {
            let ptr = args[1] as *mut u8;
            let buf = core::ptr::slice_from_raw_parts_mut(ptr, args[2] as usize);
            read(args[0], unsafe { &mut *buf }) as u64
        }
        Syscall::WRITE => {
            let ptr = args[1] as *mut u8;
            let buf = core::ptr::slice_from_raw_parts_mut(ptr, args[2] as usize);
            let buf = unsafe { &*buf };
            write(args[0], buf) as u64
        }
        Syscall::EXIT => {
            println!("[Kernel] user program exit, code: {}", args[0] as i32);
            args[0]
        }
        _ => panic!("Unsupported syscall"),
    }
}
