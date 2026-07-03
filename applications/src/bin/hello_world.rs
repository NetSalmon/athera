#![no_std]
#![no_main]

#[unsafe(no_mangle)]
fn _start() -> ! {
    let mut data: [u8;14]= [
        b'H',
        b'e',
        b'l',
        b'l',
        b'o',
        b' ',
        b'W',
        b'o',
        b'r',
        b'l',
        b'd',
        b'!',
        b'!',
        b'!',
    ];

    applications::write(0, &mut data);

    loop { core::hint::spin_loop() }
}
