#![no_std]
#![no_main]

use athera_userland::{
    println,
    syscall::{exit, fork},
};

#[unsafe(no_mangle)]
fn main() {
    println!("start fork");

    let tid = fork();

    if tid == 0 {
        println!("[child]");
        exit(0);
    } else {
        println!("[parent] child task tid: {tid}");
        exit(0);
    }
}
