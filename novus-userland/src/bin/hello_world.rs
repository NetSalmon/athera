#![no_std]
#![no_main]

const TEXT: &str = "Hello world!!!";
#[unsafe(no_mangle)]
fn main() {
    let data = TEXT.as_bytes();

    novus_userland::syscall::write(0, data);
}
