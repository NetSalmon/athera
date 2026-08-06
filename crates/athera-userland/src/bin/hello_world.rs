#![no_std]
#![no_main]

const TEXT: &str = "Hello world!!!\n";
#[unsafe(no_mangle)]
fn main() {
    let data = TEXT.as_bytes();

    athera_userland::syscall::write(1, data);
}
