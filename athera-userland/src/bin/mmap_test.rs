#![no_std]
#![no_main]

use core::ptr;

use athera_userland::{println, syscall, syscall::exit};

fn errno(ret: isize) -> isize {
    -ret
}

#[unsafe(no_mangle)]
fn main() {
    // 1. 基本匿名映射：写读回。
    let len = 4096usize;
    let addr = syscall::mmap(
        0,
        len,
        syscall::PROT_READ | syscall::PROT_WRITE,
        syscall::MAP_PRIVATE | syscall::MAP_ANONYMOUS,
        -1,
        0,
    );
    assert!(addr > 0, "mmap failed: ret = {addr}");
    println!("mmap -> {addr:#x}");

    unsafe { ptr::write(addr as *mut u8, 0xAB) };
    assert!(
        unsafe { ptr::read(addr as *const u8) } == 0xAB,
        "mmap readback failed"
    );

    // 2. 未按页对齐访问应越界测试：跨页写入（4 页映射，往最后一页写）。
    let addr4 = syscall::mmap(
        0,
        4 * 4096,
        syscall::PROT_READ | syscall::PROT_WRITE,
        syscall::MAP_PRIVATE | syscall::MAP_ANONYMOUS,
        -1,
        0,
    );
    assert!(addr4 > 0, "second mmap failed: ret = {addr4}");
    unsafe {
        ptr::write((addr4 as usize + 3 * 4096) as *mut u8, 0x55);
    }
    assert!(
        unsafe { ptr::read((addr4 as usize + 3 * 4096) as *const u8) } == 0x55,
        "cross-page write failed"
    );
    // 往首字节写一个标志，供 mremap 内容保留检查使用。
    unsafe { ptr::write(addr4 as *mut u8, 0xAB) };

    // 3. mremap 扩张（MREMAP_MAYMOVE），内容应保留。
    let moved = syscall::mremap(
        addr4 as usize,
        4 * 4096,
        8 * 4096,
        syscall::MREMAP_MAYMOVE,
        0,
    );
    assert!(moved > 0, "mremap expand failed: ret = {moved}");
    assert!(
        unsafe { ptr::read(moved as *const u8) } == 0xAB,
        "mremap expand lost content"
    );
    assert!(
        unsafe { ptr::read((moved as usize + 3 * 4096) as *const u8) } == 0x55,
        "mremap expand lost tail content"
    );
    println!("mremap expand {addr4:#x} -> {moved:#x}");

    // 4. mremap 收缩（原地），地址不变且内容保留。
    let shrunk = syscall::mremap(
        moved as usize,
        8 * 4096,
        2 * 4096,
        syscall::MREMAP_MAYMOVE,
        0,
    );
    assert!(shrunk > 0, "mremap shrink failed: ret = {shrunk}");
    assert_eq!(
        shrunk as usize, moved as usize,
        "mremap shrink should keep address"
    );
    assert!(
        unsafe { ptr::read(shrunk as *const u8) } == 0xAB,
        "mremap shrink lost content"
    );
    println!("mremap shrink {moved:#x} -> {shrunk:#x}");

    // 5. munmap 解除映射。
    let r = syscall::munmap(shrunk as usize, 2 * 4096);
    assert_eq!(r, 0, "munmap failed: ret = {r}");

    let r = syscall::munmap(addr as usize, len);
    assert_eq!(r, 0, "munmap(2) failed: ret = {r}");

    // 6. 错误路径。
    // 非匿名映射返回 -ENOSYS。
    let r = syscall::mmap(0, 4096, syscall::PROT_READ, syscall::MAP_PRIVATE, -1, 0);
    assert_eq!(
        r,
        -38,
        "non-anonymous mmap should be ENOSYS, got {r} ({})",
        errno(r)
    );

    // MAP_FIXED 且地址未页对齐返回 -EINVAL。
    let r = syscall::mmap(
        0x1234,
        4096,
        syscall::PROT_READ | syscall::PROT_WRITE,
        syscall::MAP_PRIVATE | syscall::MAP_ANONYMOUS | syscall::MAP_FIXED,
        -1,
        0,
    );
    assert_eq!(r, -22, "unaligned MAP_FIXED should be EINVAL, got {r}");

    // munmap 未页对齐返回 -EINVAL。
    let r = syscall::munmap(0x1234, 4096);
    assert_eq!(r, -22, "unaligned munmap should be EINVAL, got {r}");

    // 对未映射区间 munmap 是成功无操作。
    let r = syscall::munmap(0x0000_2000_0000, 4096);
    assert_eq!(r, 0, "munmap of unmapped region should succeed, got {r}");

    // 7. mremap 对未映射区间应返回 -EFAULT。
    let r = syscall::mremap(0x0000_3000_0000, 4096, 8192, syscall::MREMAP_MAYMOVE, 0);
    assert_eq!(
        r, -14,
        "mremap of unmapped region should be EFAULT, got {r}"
    );

    // 8. 部分 munmap（拆分为两块），两端内容仍可读写、中间不可访问。
    let big = syscall::mmap(
        0,
        8 * 4096,
        syscall::PROT_READ | syscall::PROT_WRITE,
        syscall::MAP_PRIVATE | syscall::MAP_ANONYMOUS,
        -1,
        0,
    );
    assert!(big > 0, "big mmap failed: ret = {big}");
    unsafe {
        ptr::write(big as *mut u8, 0x41);
        ptr::write((big as usize + 7 * 4096) as *mut u8, 0x42);
    }
    let r = syscall::munmap(big as usize + 2 * 4096, 3 * 4096);
    assert_eq!(r, 0, "partial munmap failed: ret = {r}");
    assert!(
        unsafe { ptr::read(big as *const u8) } == 0x41,
        "left part after partial munmap lost content"
    );
    assert!(
        unsafe { ptr::read((big as usize + 7 * 4096) as *const u8) } == 0x42,
        "right part after partial munmap lost content"
    );
    println!("partial munmap ok");

    // 9. 原地扩张：映射 6 页、释放顶部 2 页后，mremap 应原地扩张保持地址。
    let g = syscall::mmap(
        0,
        6 * 4096,
        syscall::PROT_READ | syscall::PROT_WRITE,
        syscall::MAP_PRIVATE | syscall::MAP_ANONYMOUS,
        -1,
        0,
    );
    assert!(g > 0, "growth mmap failed: ret = {g}");
    unsafe { ptr::write(g as *mut u8, 0x43) };
    let r = syscall::munmap(g as usize + 4 * 4096, 2 * 4096);
    assert_eq!(r, 0, "growth munmap failed: ret = {r}");
    let grown = syscall::mremap(g as usize, 4 * 4096, 6 * 4096, syscall::MREMAP_MAYMOVE, 0);
    assert_eq!(
        grown as usize, g as usize,
        "in-place growth should keep address, got {grown}"
    );
    assert!(
        unsafe { ptr::read(grown as *const u8) } == 0x43,
        "in-place growth lost content"
    );
    println!("in-place growth ok");

    // 10. MAP_FIXED 与 MAP_FIXED_NOREPLACE。
    let fixed_addr = 0x0000_4000_0000usize;
    let r = syscall::mmap(
        fixed_addr,
        4096,
        syscall::PROT_READ | syscall::PROT_WRITE,
        syscall::MAP_PRIVATE | syscall::MAP_ANONYMOUS | syscall::MAP_FIXED,
        -1,
        0,
    );
    assert_eq!(
        r as usize, fixed_addr,
        "MAP_FIXED should use the requested address, got {r}"
    );
    unsafe { ptr::write(fixed_addr as *mut u8, 0x77) };
    // 覆盖映射：再次 MAP_FIXED 同一地址，应替换旧映射。
    let r = syscall::mmap(
        fixed_addr,
        4096,
        syscall::PROT_READ,
        syscall::MAP_PRIVATE | syscall::MAP_ANONYMOUS | syscall::MAP_FIXED,
        -1,
        0,
    );
    assert_eq!(r as usize, fixed_addr, "MAP_FIXED replace failed, got {r}");
    assert!(
        unsafe { ptr::read(fixed_addr as *const u8) } == 0,
        "MAP_FIXED replace should zero new pages"
    );
    // NOREPLACE：目标已占用应返回 -EEXIST。
    let r = syscall::mmap(
        fixed_addr,
        4096,
        syscall::PROT_READ,
        syscall::MAP_PRIVATE | syscall::MAP_ANONYMOUS | syscall::MAP_FIXED_NOREPLACE,
        -1,
        0,
    );
    assert_eq!(
        r, -17,
        "MAP_FIXED_NOREPLACE conflict should be EEXIST, got {r}"
    );
    let r = syscall::munmap(fixed_addr, 4096);
    assert_eq!(r, 0, "munmap fixed region failed: ret = {r}");
    println!("MAP_FIXED / NOREPLACE ok");

    println!("mmap test passed");
    exit(0);
}
