#![no_std]
#![no_main]

use core::ffi::CStr;

use athera_userland::println;

fn cstr(ptr: *const u8) -> &'static str {
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .unwrap_or("<invalid utf8>")
}

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8, envp: *const *const u8) {
    println!("argc = {argc}");

    println!("--- argv ---");
    for i in 0..argc {
        let arg = unsafe { *argv.add(i) };
        println!("argv[{i}] = {}", cstr(arg));
    }

    println!("--- envp ---");
    let mut i = 0;
    while unsafe { !(*envp.add(i)).is_null() } {
        let env = unsafe { *envp.add(i) };
        println!("envp[{i}] = {}", cstr(env));
        i += 1;
    }
    println!("envp count = {i}");
}
