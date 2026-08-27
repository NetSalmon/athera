#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use core::iter::Iterator;

use athera_userland::{
    alloc::{allocate, deallocate, stats},
    println,
    syscall::{exit, fork},
};

#[unsafe(no_mangle)]
fn main() {
    // 1. 基本堆分配：Vec 正常分配与读取。
    let mut numbers = Vec::with_capacity(8);
    for index in 0..8 {
        numbers.push((index * index) as u64);
    }

    let sum: u64 = numbers.iter().sum();
    println!("heap array: {numbers:?}");
    println!("heap array sum: {sum}");
    assert_eq!(sum, 140);

    // 2. 堆按需增长：超过旧静态堆（64 KiB）上限的分配应当成功且内容可读写。
    let big = allocate(64 * 1024 + 1);
    assert!(
        !big.is_null(),
        "allocation larger than the old static heap should succeed"
    );
    unsafe {
        big.write_bytes(0x5a, 64 * 1024 + 1);
        assert_eq!(big.read(), 0x5a);
    }
    unsafe { deallocate(big, 64 * 1024 + 1) };
    println!("heap grows beyond 64 KiB: ok");

    // 3. 超大分配失败应返回空指针（mmap 失败），而不是 panic。
    let too_large = allocate(1 << 30);
    assert!(too_large.is_null(), "1 GiB allocation should fail");
    println!("oversized allocation is null: {}", too_large.is_null());

    // 4. 块级分配/释放循环：反复申请与归还整块区域，触发 munmap 整块归还
    //    与重新 mmap。
    for round in 0..3 {
        let mut chunk: Vec<u64> = Vec::with_capacity(384 * 1024); // 3 MiB
        chunk.push(0x1234);
        chunk.push(0x5678);
        assert_eq!(chunk[0], 0x1234);
        assert_eq!(chunk[1], 0x5678);
        drop(chunk);
        println!("churn round {round}: ok");
    }

    // 5. 超过内核伙伴系统单段上限（4 MiB）的大分配：内核按多段拼接，应
    //    成功且整段（含段边界、尾部）内容可读写。
    let huge = allocate(8 * 1024 * 1024); // 8 MiB，跨两个 4 MiB 段
    assert!(
        !huge.is_null(),
        "8 MiB allocation should succeed (multi-frame)"
    );
    unsafe {
        huge.write_bytes(0xa5, 8 * 1024 * 1024);
        assert_eq!(huge.read(), 0xa5);
        assert_eq!(huge.add(4 * 1024 * 1024).read(), 0xa5, "跨段边界内容损坏");
        assert_eq!(
            huge.add(8 * 1024 * 1024 - 1).read(),
            0xa5,
            "映射尾部内容损坏"
        );
    }
    unsafe { deallocate(huge, 8 * 1024 * 1024) };
    println!("multi-frame 8 MiB allocation: ok");

    // 6. fork 深拷贝多段映射：子进程应能读到父进程写入的跨段内容。
    let keep = allocate(8 * 1024 * 1024);
    assert!(!keep.is_null(), "fork 前 8 MiB 分配失败");
    unsafe {
        keep.write_bytes(0x5c, 8 * 1024 * 1024);
        keep.add(4 * 1024 * 1024).write(0x99);
    }
    if fork() == 0 {
        // 子进程：验证多段映射被深拷贝，然后退出。
        assert_eq!(unsafe { keep.read() }, 0x5c, "子进程首段内容不一致");
        assert_eq!(
            unsafe { keep.add(4 * 1024 * 1024).read() },
            0x99,
            "子进程跨段内容不一致"
        );
        println!("fork multi-frame copy: ok");
        exit(0);
    }
    unsafe { deallocate(keep, 8 * 1024 * 1024) };

    // 7. 区域尾部释放：小块保留、释放其上方的相邻大块，触发 mremap 收缩；
    //    收缩后分配器仍应正常工作。
    let small = allocate(128);
    let big = allocate(512 * 1024);
    assert!(!small.is_null() && !big.is_null());
    unsafe {
        big.write_bytes(0x7e, 512 * 1024);
        assert_eq!(big.read(), 0x7e);
    }
    unsafe { deallocate(big, 512 * 1024) };
    let after = allocate(256);
    assert!(!after.is_null(), "allocator must work after mremap shrink");
    unsafe { after.write_bytes(0x3c, 256) };
    unsafe { deallocate(after, 256) };
    unsafe { deallocate(small, 128) };
    println!("mremap shrink: ok");

    // 8. 上方区域整块归还后，最近区域应能 mremap 原地扩大复用其虚拟区间。
    //    三块各占一个区域（1 MiB→2 MiB、3 MiB→4 MiB、1 MiB→2 MiB 映射），
    //    释放中间的整块后，最下方区域的上方虚拟区间即空闲，下一次扩容应
    //    原地扩大而不是新建区域。
    let upper = allocate(1 << 20); // 区域 A（2 MiB 映射）
    let middle = allocate(3 << 20); // 区域 B（4 MiB 映射，填满）
    let lower = allocate(1 << 20); // 区域 C（2 MiB 映射，链表头）
    assert!(!upper.is_null() && !middle.is_null() && !lower.is_null());
    unsafe {
        middle.write_bytes(0x6d, 4096);
        assert_eq!(middle.read(), 0x6d);
    }
    unsafe { deallocate(middle, 3 << 20) }; // 整块归还 B → C 上方虚拟区间空闲
    let grown = allocate(1 << 20); // 应触发 mremap 原地扩大 C
    assert!(
        !grown.is_null(),
        "allocation after freeing the region above should succeed"
    );
    unsafe {
        grown.write_bytes(0x11, 4096);
        assert_eq!(grown.read(), 0x11);
    }
    unsafe { deallocate(lower, 1 << 20) };
    unsafe { deallocate(grown, 1 << 20) };
    unsafe { deallocate(upper, 1 << 20) };
    println!("mremap grow: ok");

    // 9. 统计：四个系统调用路径都应当被实际使用过。
    let s = stats();
    println!("allocator stats: {s:?}");
    assert!(s.mmaps > 0, "mmap must be used to acquire heap memory");
    assert!(s.munmaps > 0, "munmap must be used to release heap blocks");
    assert!(s.mremap_shrinks > 0, "mremap shrink must be exercised");
    assert!(s.mremap_grows > 0, "mremap grow must be exercised");

    println!("heap test passed");
}
