#![no_std]
#![no_main]

use athera_userland::println;

#[unsafe(no_mangle)]
fn main() {
    println!("about to panic...");
    panic!("goodbye world!!!");
}
