#![no_std]
#![no_main]

#[unsafe(no_mangle)]
fn main() {
    athera_userland::println!("Hello world!!!");
}
