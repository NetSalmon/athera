#![no_std]
#![no_main]

use core::hint::spin_loop;
use athera_userland::*;
use athera_userland::syscall::{execve, fork};

#[unsafe(no_mangle)]
fn main() {
    println!("init running");

    if fork() == 0 {
        println!("execve test");

        execve("/bin/sort", &["/bin/sort"], &["PWD=/"]);
    }

    loop {
        spin_loop()
    }
}
