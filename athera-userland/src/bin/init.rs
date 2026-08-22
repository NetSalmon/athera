#![no_std]
#![no_main]

#[unsafe(no_mangle)]
fn main() {
    athera_userland::println!("Hello, world from init task!!!");

    loop {
        core::hint::spin_loop();
    }
}
