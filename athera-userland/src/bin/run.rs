#![no_std]
#![no_main]

use athera_userland::*;

#[unsafe(no_mangle)]
fn main(_argc: usize, argv: *const *const u8) {
    let filepath = unsafe { argv.add(1).read() };

    let mut i = 0;

    loop {
        let ch = unsafe { filepath.add(i).read() };

        if ch == 0 {
            break;
        }

        print!("{}", ch as char);

        i += 1;
    }

    println!();
}
