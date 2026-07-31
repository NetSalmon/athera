#![no_std]
#![no_main]

const TEXT: &str = "Hello world!!!\n";
#[unsafe(no_mangle)]
fn main() {
    let data = TEXT.as_bytes();

    novus_userland::syscall::write(1, data);
}
