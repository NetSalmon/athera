#![no_std]
#![no_main]

#[unsafe(no_mangle)]
fn main() {
    let a = 1;
    let b = 2;

    let c = add(a, b);

    athera_userland::println!("{a} + {b} = {c}");
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}
