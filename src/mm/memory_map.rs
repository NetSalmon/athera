//! 匿名内存映射：`mmap` / `munmap` / `mremap` 的实现。
//!
//! 调用约定对齐 Linux asm-generic（riscv64）ABI：
//!
//! - `mmap(addr, length, prot, flags, fd, offset)`，当前只支持
//!   `MAP_ANONYMOUS`（`fd` 必须为 `-1`、`offset` 必须为 `0`）；
//! - `munmap(addr, length)`：解除 `[addr, addr + length)` 的映射；
//! - `mremap(old_address, old_size, new_size, flags, new_address)`：原地
//!   收缩/扩张，无法原地扩张时按 `MREMAP_MAYMOVE` / `MREMAP_FIXED` 移动。
//!
//! 与 `MemorySet` 的约定：每个 [`UserMapping`] 表示一段连续的虚拟区间，
//! 由一块连续物理帧支撑，且 `mapping.va` 与 `mapping.frame.start` 严格
//! 一一对应（页表里 `va + k*PAGE` 恒映射到 `frame.start + k*PAGE`）。本
//! 模块的所有操作（按页解除 / 移动 / 重建）都维护这一不变量；物理帧只在
//! 整段不再被映射时才归还。部分解除映射时先为保留部分预分配新帧并拷贝
//! 内容（事务式失败回滚），再拆除原映射，因此不会出现把仍被页表引用的
//! 物理帧归还给伙伴系统的悬垂情况。
//!
//! 已知限制：用户地址空间的低半区与内核共享（`copy_low_half`），因此
//! `MAP_FIXED` 落到内核已恒等映射的物理区域（如 UART / virtio 所在的
//! `vpn2 = 0`、RAM 所在的 `vpn2 = 2`）时，`AddressSpace::map` 找不到属于
//! 该地址空间的中间页表会返回 EINVAL。默认的非固定分配从用户栈（约
//! `0xffff8000`）下方展开，永不触及这些区域。

use alloc::vec::Vec;
use core::ptr;

use crate::{
    constants::{PAGE_SIZE, USER_STACK_LOWER_BOUND},
    error::MemError,
    mm::{
        address::VirtualAddr,
        allocator::alloc_frame,
        frame::Frame,
        page_table::{ADDRESS_SPACE_MANAGER, AddressSpaceManager, PageTableEntryFlags},
    },
    syscall::abi::{ErrorCode, MmapFlags, MmapProt, MremapFlags},
    task::{MemorySet, TASKS, Tid, UserMapping},
};

/// Sv39 用户低半区上界（2^38，不含）。
const LOW_HALF_END: usize = 0x0000_0040_0000_0000;
/// Sv39 用户高半区起始。
const HIGH_HALF_START: usize = 0xffff_ffc0_0000_0000;

/// `prot` 参数允许的位（PROT_READ | PROT_WRITE | PROT_EXEC）。
const PROT_MASK: usize = MmapProt::READ.0 | MmapProt::WRITE.0 | MmapProt::EXEC.0;

#[inline]
fn align_up(addr: usize) -> usize {
    (addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

/// 把 [`MemError`] 映射为 Linux errno。
fn mem_errno(err: MemError) -> ErrorCode {
    match err {
        MemError::OutOfMemory => ErrorCode::ENOMEM,
        _ => ErrorCode::EINVAL,
    }
}

/// 一个映射覆盖的（页对齐）虚拟区间 `[start, end)`。
fn mapping_range(m: &UserMapping) -> (usize, usize) {
    let start = usize::from(m.va) & !(PAGE_SIZE - 1);
    let size = align_up(m.frame.size);
    (start, start + size)
}

fn overlaps(m: &UserMapping, start: usize, end: usize) -> bool {
    let (s, e) = mapping_range(m);
    s < end && start < e
}

fn overlapping_indices(ms: &MemorySet, start: usize, end: usize) -> Vec<usize> {
    ms.mappings
        .iter()
        .enumerate()
        .filter(|(_, m)| overlaps(m, start, end))
        .map(|(i, _)| i)
        .collect()
}

/// `[start, end)`（页对齐）是否完全空闲。
fn range_is_free(ms: &MemorySet, start: usize, end: usize) -> bool {
    !ms.mappings.iter().any(|m| overlaps(m, start, end))
}

/// `[start, end)`（页对齐）是否被映射完全覆盖。
fn range_fully_mapped(ms: &MemorySet, start: usize, end: usize) -> bool {
    (start..end)
        .step_by(PAGE_SIZE)
        .all(|va| page_phys(ms, va).is_some())
}

/// 页对齐虚拟地址 `va` 对应的物理页地址（依据映射表推算，不依赖页表）。
fn page_phys(ms: &MemorySet, va: usize) -> Option<usize> {
    for m in &ms.mappings {
        let (s, e) = mapping_range(m);
        if va >= s && va < e {
            return Some(m.frame.start + (va - s));
        }
    }
    None
}

/// 覆盖页对齐地址 `va` 的映射标志。
fn flags_of_first(ms: &MemorySet, va: usize) -> Option<PageTableEntryFlags> {
    ms.mappings
        .iter()
        .find(|m| overlaps(m, va, va + 1))
        .map(|m| m.flags)
}

/// 在用户栈下方查找一块 `length` 字节的空闲连续区域（Linux 风格：匿名
/// 映射从栈下方向低地址生长）。
fn find_free_region(ms: &MemorySet, length: usize) -> Option<usize> {
    let mut ranges: Vec<(usize, usize)> = ms
        .mappings
        .iter()
        .map(mapping_range)
        .filter(|(_, e)| *e <= USER_STACK_LOWER_BOUND)
        .collect();
    ranges.sort_unstable_by_key(|(s, _)| *s);

    let mut top = USER_STACK_LOWER_BOUND;
    for &(s, e) in ranges.iter().rev() {
        if top > e && top - e >= length {
            return Some(top - length);
        }
        top = s;
    }
    (top >= length).then_some(top - length)
}

/// 校验虚拟区间位于 Sv39 用户区（低半区 `[0, 2^38)` 或高半区 `[2^38 边界, 2^64)`）。
fn is_canonical_user_range(start: usize, end: usize) -> bool {
    end <= LOW_HALF_END || start >= HIGH_HALF_START
}

/// 根据 `mmap` 的 `prot` 构造页表项标志。
///
/// RISC-V 要求叶页表项满足 `R=1` 或 `X=1`，`R=W=X=0` 是指向下一级页表的
/// 非叶条目 / 保留条目，因此任何非 `PROT_NONE` 的映射都置读位（与 Linux
/// riscv 的 `VM_READ` 处理一致）；`PROT_NONE` 不映射页表项（访问时按缺页
/// 异常处理），仅登记在映射表中供解除/移动/克隆时释放物理帧。
fn prot_to_page_flags(prot: usize) -> PageTableEntryFlags {
    let mut flags = PageTableEntryFlags::new();
    flags.set_u(true);
    flags.set_a(true);
    flags.set_d(true);
    if prot & PROT_MASK != 0 {
        flags.set_r(true);
    }
    if prot & usize::from(MmapProt::WRITE) != 0 {
        flags.set_w(true);
    }
    if prot & usize::from(MmapProt::EXEC) != 0 {
        flags.set_x(true);
    }
    flags
}

/// 把一块已分配、已清零的物理帧映射到 `addr`（页对齐）并登记进映射表。
///
/// `PROT_NONE`（`rwx` 全 0）不写页表项，仅登记映射。失败时回滚已建立的
/// 页表项并归还该帧，不留下半成品。
fn map_frame(
    ms: &mut MemorySet,
    tid: Tid,
    addr: usize,
    frame: Frame,
    flags: PageTableEntryFlags,
) -> Result<(), ErrorCode> {
    if flags.r() || flags.w() || flags.x() {
        let mut manager = ADDRESS_SPACE_MANAGER.force().lock();
        for step in (0..frame.size).step_by(PAGE_SIZE) {
            if let Err(err) = manager.user_map(
                tid,
                (addr + step).into(),
                (frame.start + step).into(),
                flags,
                false,
            ) {
                for rollback in (0..step).step_by(PAGE_SIZE) {
                    let _ = manager.user_unmap(tid, (addr + rollback).into(), false);
                }
                AddressSpaceManager::flush();
                return Err(mem_errno(err));
            }
        }
        AddressSpaceManager::flush();
    }
    ms.mappings.push(UserMapping {
        va: VirtualAddr::from(addr),
        frame,
        flags,
    });
    Ok(())
}

/// 解除 `[start, end)`（页对齐）的映射。
///
/// 与映射表重叠的部分被拆除；部分重叠时保留区间外的部分（预分配新物理帧
/// 并拷贝内容，全部成功后才动原有映射，失败则整体回滚），保证
/// `va -> frame.start` 一一对应且不会把仍在使用的物理帧归还。
fn unmap_range(ms: &mut MemorySet, tid: Tid, start: usize, end: usize) -> Result<(), ErrorCode> {
    struct Keep {
        va: usize,
        pa: usize,
        size: usize,
        flags: PageTableEntryFlags,
    }

    // 1) 收集需要保留的片段规格：每个被 [start, end) 部分覆盖的映射至多
    //    产生两个保留片段（区间左侧 / 右侧）。
    let mut keeps: Vec<Keep> = Vec::new();
    for m in &ms.mappings {
        if !overlaps(m, start, end) {
            continue;
        }
        let (s, e) = mapping_range(m);
        let o_start = s.max(start);
        let o_end = e.min(end);
        if s < o_start {
            keeps.push(Keep {
                va: s,
                pa: m.frame.start,
                size: o_start - s,
                flags: m.flags,
            });
        }
        if o_end < e {
            keeps.push(Keep {
                va: o_end,
                pa: m.frame.start + (o_end - s),
                size: e - o_end,
                flags: m.flags,
            });
        }
    }

    // 2) 预分配新帧并拷贝内容（事务式：任一步失败则整体回滚，映射表与页
    //    表都保持原状）。
    let mut buffers: Vec<(Frame, Keep)> = Vec::with_capacity(keeps.len());
    for k in keeps {
        let Some(frame) = alloc_frame(Some(k.size)) else {
            return Err(ErrorCode::ENOMEM);
        };
        // Safety: `k.pa..k.pa + k.size` 位于原映射对应物理帧的已分配块内。
        unsafe {
            ptr::copy(k.pa as *const u8, frame.start as *mut u8, k.size);
        }
        buffers.push((frame, k));
    }

    // 3) 拆除重叠映射的页表项并移除映射项（内容已备份，物理帧可安全归还）。
    {
        let mut manager = ADDRESS_SPACE_MANAGER.force().lock();
        for idx in overlapping_indices(ms, start, end).iter().rev() {
            let mapping = ms.mappings.remove(*idx);
            let (s, e) = mapping_range(&mapping);
            for page in (s..e).step_by(PAGE_SIZE) {
                let _ = manager.user_unmap(tid, page.into(), false);
            }
            // mapping.frame 在此 drop，归还物理帧。
        }
        AddressSpaceManager::flush();
    }

    // 4) 重新映射保留片段。
    for (frame, k) in buffers {
        map_frame(ms, tid, k.va, frame, k.flags)?;
    }
    Ok(())
}

/// 把 `[old_start, old_end)` 的内容重建为 `[dest, dest + new_size)` 的
/// 单段匿名映射：分配新帧、按页拷贝内容、（可选地）在 `dest` 处替换既有
/// 映射，最后拆除旧映射。各步骤失败都会回滚，不会留下悬垂指针。
fn rebuild(
    ms: &mut MemorySet,
    tid: Tid,
    old_start: usize,
    old_end: usize,
    dest: usize,
    new_size: usize,
    flags: PageTableEntryFlags,
) -> Result<(), ErrorCode> {
    let old_size = old_end - old_start;
    let copy_len = old_size.min(new_size);

    let Some(frame) = alloc_frame(Some(new_size)) else {
        return Err(ErrorCode::ENOMEM);
    };
    // Safety: 帧内 `new_size` 字节全部可写。
    unsafe {
        ptr::write_bytes(frame.start as *mut u8, 0, new_size);
    }

    for off in (0..copy_len).step_by(PAGE_SIZE) {
        let Some(src) = page_phys(ms, old_start + off) else {
            return Err(ErrorCode::EFAULT);
        };
        // Safety: 源/目标均是页对齐的已分配物理区间。
        unsafe {
            ptr::copy(src as *const u8, (frame.start + off) as *mut u8, PAGE_SIZE);
        }
    }

    unmap_range(ms, tid, old_start, old_end)?;
    map_frame(ms, tid, dest, frame, flags)
}

/// `mmap(addr, length, prot, flags, fd, offset)`。
///
/// 仅支持匿名映射（`MAP_ANONYMOUS`）：`fd` 必须为 `-1`、`offset` 必须为
/// `0`。返回映射起始虚拟地址。`MAP_FIXED` 要求 `addr` 页对齐并替换重叠
/// 映射；`MAP_FIXED_NOREPLACE` 在目标区间非空闲时返回 `EEXIST`；其余情况
/// 优先使用页对齐的 `addr` 提示，否则在用户栈下方寻找空闲区间。
pub fn mmap(
    tid: Tid,
    addr: usize,
    length: usize,
    prot: usize,
    flags: usize,
    fd: isize,
    offset: usize,
) -> Result<usize, ErrorCode> {
    if flags & usize::from(MmapFlags::ANONYMOUS) == 0 {
        return Err(ErrorCode::ENOSYS);
    }
    if fd != -1 || offset != 0 {
        return Err(ErrorCode::EINVAL);
    }
    if prot & !PROT_MASK != 0 {
        return Err(ErrorCode::EINVAL);
    }
    if length == 0 || length >= usize::MAX - PAGE_SIZE {
        return Err(ErrorCode::EINVAL);
    }
    let length = align_up(length);

    let noreplace = flags & usize::from(MmapFlags::FIXED_NOREPLACE) != 0;
    let fixed =
        flags & (usize::from(MmapFlags::FIXED) | usize::from(MmapFlags::FIXED_NOREPLACE)) != 0;

    let mut tasks = TASKS.force().lock();
    let tcb = tasks.get_mut(&tid).ok_or(ErrorCode::ESRCH)?;
    let ms = &mut tcb.memory_set;

    let target = if fixed {
        if !addr.is_multiple_of(PAGE_SIZE) {
            return Err(ErrorCode::EINVAL);
        }
        addr
    } else if addr != 0
        && addr.is_multiple_of(PAGE_SIZE)
        && let Some(hint_end) = addr.checked_add(length)
        && range_is_free(ms, addr, hint_end)
    {
        addr
    } else {
        find_free_region(ms, length).ok_or(ErrorCode::ENOMEM)?
    };

    let end = target.checked_add(length).ok_or(ErrorCode::ENOMEM)?;
    if !is_canonical_user_range(target, end) {
        return Err(ErrorCode::ENOMEM);
    }

    if noreplace && !range_is_free(ms, target, end) {
        return Err(ErrorCode::EEXIST);
    }
    if fixed && !noreplace {
        unmap_range(ms, tid, target, end)?;
    }

    let Some(frame) = alloc_frame(Some(length)) else {
        return Err(ErrorCode::ENOMEM);
    };
    // Safety: 帧内 `length` 字节全部可写。
    unsafe {
        ptr::write_bytes(frame.start as *mut u8, 0, length);
    }
    map_frame(ms, tid, target, frame, prot_to_page_flags(prot))?;
    Ok(target)
}

/// `munmap(addr, length)`：解除 `[addr, addr + length)` 的映射。
///
/// `addr` 必须页对齐、`length` 非零；对未映射区间解除映射是成功无操作。
pub fn munmap(tid: Tid, addr: usize, length: usize) -> Result<(), ErrorCode> {
    if !addr.is_multiple_of(PAGE_SIZE) || length == 0 {
        return Err(ErrorCode::EINVAL);
    }
    let end = addr.checked_add(length).ok_or(ErrorCode::EINVAL)?;

    let mut tasks = TASKS.force().lock();
    let tcb = tasks.get_mut(&tid).ok_or(ErrorCode::ESRCH)?;
    let ms = &mut tcb.memory_set;

    unmap_range(ms, tid, addr, end)?;
    Ok(())
}

/// `mremap(old_address, old_size, new_size, flags, new_address)`。
///
/// 要求 `old_address` / `old_size` / `new_size` 均页对齐且非零，旧区间完全
/// 被映射。收缩或可原地扩张时保持起始地址不变；无法原地扩张时，
/// `MREMAP_MAYMOVE` 在栈下寻找空闲区间，`MREMAP_FIXED` 使用 `new_address`
/// （页对齐，目标区间须空闲且与原区间不重叠）。返回新的起始地址。
pub fn mremap(
    tid: Tid,
    old_address: usize,
    old_size: usize,
    new_size: usize,
    flags: usize,
    new_address: usize,
) -> Result<usize, ErrorCode> {
    if !old_address.is_multiple_of(PAGE_SIZE)
        || old_size == 0
        || new_size == 0
        || !old_size.is_multiple_of(PAGE_SIZE)
        || !new_size.is_multiple_of(PAGE_SIZE)
        || new_size >= usize::MAX - PAGE_SIZE
    {
        return Err(ErrorCode::EINVAL);
    }
    let old_end = old_address.checked_add(old_size).ok_or(ErrorCode::EINVAL)?;

    let fixed = flags & usize::from(MremapFlags::FIXED) != 0;
    if fixed && !new_address.is_multiple_of(PAGE_SIZE) {
        return Err(ErrorCode::EINVAL);
    }

    let mut tasks = TASKS.force().lock();
    let tcb = tasks.get_mut(&tid).ok_or(ErrorCode::ESRCH)?;
    let ms = &mut tcb.memory_set;

    if new_size == old_size {
        if fixed && new_address != old_address {
            return move_to(ms, tid, old_address, old_end, new_address, old_size, true);
        }
        return Ok(old_address);
    }

    if !range_fully_mapped(ms, old_address, old_end) {
        return Err(ErrorCode::EFAULT);
    }

    if new_size < old_size {
        // 收缩：原地重建为更小映射。
        let prot_flags = flags_of_first(ms, old_address).ok_or(ErrorCode::EFAULT)?;
        rebuild(
            ms,
            tid,
            old_address,
            old_end,
            old_address,
            new_size,
            prot_flags,
        )?;
        return Ok(old_address);
    }

    // 扩张：优先原地扩张（旧映射结束位置之后须空闲）。
    let growth = new_size - old_size;
    let grow_in_place = old_end
        .checked_add(growth)
        .is_some_and(|end| range_is_free(ms, old_end, end))
        && is_canonical_user_range(old_address, old_end + growth);
    if grow_in_place {
        let prot_flags = flags_of_first(ms, old_address).ok_or(ErrorCode::EFAULT)?;
        rebuild(
            ms,
            tid,
            old_address,
            old_end,
            old_address,
            new_size,
            prot_flags,
        )?;
        return Ok(old_address);
    }

    if fixed {
        return move_to(ms, tid, old_address, old_end, new_address, new_size, true);
    }
    if flags & usize::from(MremapFlags::MAYMOVE) != 0 {
        let dest = find_free_region(ms, new_size).ok_or(ErrorCode::ENOMEM)?;
        return move_to(ms, tid, old_address, old_end, dest, new_size, false);
    }
    Err(ErrorCode::ENOMEM)
}

/// 把 `[old_start, old_end)` 的内容移动到 `dest`（页对齐），尺寸为
/// `new_size`。
///
/// `replace` 为真（`MREMAP_FIXED`）时先解除 `dest` 处既有映射并替换之；
/// 为假（`MREMAP_MAYMOVE`）时要求 `dest` 区间空闲。目标区间不得与旧区间
/// 重叠（否则内容来源会被破坏）。
fn move_to(
    ms: &mut MemorySet,
    tid: Tid,
    old_start: usize,
    old_end: usize,
    dest: usize,
    new_size: usize,
    replace: bool,
) -> Result<usize, ErrorCode> {
    let dest_end = dest.checked_add(new_size).ok_or(ErrorCode::ENOMEM)?;
    if !is_canonical_user_range(dest, dest_end) {
        return Err(ErrorCode::ENOMEM);
    }
    if dest < old_end && old_start < dest_end {
        return Err(ErrorCode::ENOMEM);
    }
    if replace {
        unmap_range(ms, tid, dest, dest_end)?;
    } else if !range_is_free(ms, dest, dest_end) {
        return Err(ErrorCode::ENOMEM);
    }
    let prot_flags = flags_of_first(ms, old_start).ok_or(ErrorCode::EFAULT)?;
    rebuild(ms, tid, old_start, old_end, dest, new_size, prot_flags)?;
    Ok(dest)
}
