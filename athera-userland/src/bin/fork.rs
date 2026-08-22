#![no_std]
#![no_main]

use athera_userland::{println, syscall::fork};

#[unsafe(no_mangle)]
fn main() {
    println!("start fork");

    let tid = fork();

    if tid == 0 {
        println!("[child]")
    } else {
        println!("[parent] child task tid: {tid}")
    }
}
