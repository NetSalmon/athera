#![no_std]
#![no_main]

#[unsafe(no_mangle)]
fn main() {
    let a = 1;
    let b = 2;

    let c = add(a, b);

    novus_userland::syscall::write(
        0,
        &[
            num_to_char(a),
            b' ',
            b'+',
            b' ',
            num_to_char(b),
            b' ',
            b'=',
            b' ',
            num_to_char(c),
            b'\n',
        ],
    );
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn num_to_char(num: i32) -> u8 {
    if !(0..10).contains(&num) {
        panic!("num out of range");
    }

    num as u8 + b'0'
}
