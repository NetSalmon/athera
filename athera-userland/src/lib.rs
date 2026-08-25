#![no_std]
#![no_main]
//! 用户程序运行库。
//!
//! 链接脚本把入口指向 `_start`（汇编）：它按 Linux 初始栈布局从栈上读出
//! `argc` / `argv` / `envp`，经 `a0` / `a1` / `a2` 寄存器传入 `main`，
//! `main` 返回后以退出码 0 调用 `exit` 结束；`syscall` / `panic` 模块
//! 提供用户态系统调用封装与 panic 处理。
//!
//! 各 bin 的 `main` 可按需声明参数：
//! - 不需要参数：`fn main()`（忽略传入的寄存器）；
//! - 需要参数：`fn main(argc: usize, argv: *const *const u8, envp: *const *const u8)`。

use core::arch::global_asm;

pub mod alloc;
pub mod panic;
pub mod stdio;
pub mod syscall;

// 入口时 `sp` 指向初始栈上的 argc（Linux 约定）。必须在函数序言（本项目
// 强制帧指针，会先改写 sp）之前解析，故全部放在汇编里：a0=argc、
// a1=argv、a2=envp，然后 `call main`；返回后再以 0 状态码 `call exit`。
global_asm!(
    r#"
    .section .text.entry
    .globl _start
_start:
    ld a0, 0(sp)        # argc
    addi a1, sp, 8      # argv
    slli t0, a0, 3      # argc * 8
    add a2, a1, t0      # argv + argc*8 指向 argv 终止 NULL
    addi a2, a2, 8      # envp = &argv[argc+1] = &envp[0]
    call main
    li a0, 0            # 退出码 0
    call exit
    "#
);
