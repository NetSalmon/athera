#![no_std]
#![no_main]

#[unsafe(no_mangle)]
fn main() {
    athera_userland::println!("bubble sort test:");
    
    let mut array = [9, 8, 7, 6, 5, 4, 3, 2, 1, 0];

    let mut times = 0;
    loop {
        times += 1;
        let mut swap = false;
        for i in 1..array.len() {
            if array[i] < array[i - 1] {
                array.swap(i, i - 1);
                swap = true;
            }
        }

        if !swap {
            break;
        }

        athera_userland::println!("sort times:{times}, array: {array:?}");
    }

    athera_userland::println!("sorted: {array:?}");
}
