#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use core::iter::Iterator;

use athera_userland::{alloc::allocate, println};

#[unsafe(no_mangle)]
fn main() {
    let mut numbers = Vec::with_capacity(8);
    for index in 0..8 {
        numbers.push((index * index) as u64);
    }

    let sum: u64 = numbers.iter().sum();
    println!("heap array: {numbers:?}");
    println!("heap array sum: {sum}");

    let too_large = allocate(athera_userland::alloc::HEAP_SIZE + 1);
    println!("oversized allocation is null: {}", too_large.is_null());
}
