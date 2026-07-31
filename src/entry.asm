.section .text.entry
.global _start
.balign 4
_start:
    la sp, kernel_stack_top

    la t0, FDT_ADDR
    sd a1, 0(t0)

    la t0, trap_entry
    andi t0, t0, ~3
    csrs stvec, t0

    li   t0, (1 << 1) | (1 << 5) | (1 << 9)
    csrw sie, t0

    li   t0, (1 << 1) | (1 << 18)
    csrs sstatus, t0

    call main

.section .text
.balign 4
.global trap_entry
trap_entry:
    addi sp, sp, -256
    sd x0, 0(sp)
    sd x1, 8(sp)
    sd x2, 16(sp)
    sd x3, 24(sp)
    sd x4, 32(sp)
    sd x5, 40(sp)
    sd x6, 48(sp)
    sd x7, 56(sp)
    sd x8, 64(sp)
    sd x9, 72(sp)
    sd x10, 80(sp)
    sd x11, 88(sp)
    sd x12, 96(sp)
    sd x13, 104(sp)
    sd x14, 112(sp)
    sd x15, 120(sp)
    sd x16, 128(sp)
    sd x17, 136(sp)
    sd x18, 144(sp)
    sd x19, 152(sp)
    sd x20, 160(sp)
    sd x21, 168(sp)
    sd x22, 176(sp)
    sd x23, 184(sp)
    sd x24, 192(sp)
    sd x25, 200(sp)
    sd x26, 208(sp)
    sd x27, 216(sp)
    sd x28, 224(sp)
    sd x29, 232(sp)
    sd x30, 240(sp)
    sd x31, 248(sp)

    csrr a0, scause
    csrr a1, sepc
    csrr a2, stval
    csrr a3, sstatus
    csrr a4, satp
    mv   a5, sp

    call trap_handler

    ld x0, 0(sp)
    ld x1, 8(sp)
    ld x2, 16(sp)
    ld x3, 24(sp)
    ld x4, 32(sp)
    ld x5, 40(sp)
    ld x6, 48(sp)
    ld x7, 56(sp)
    ld x8, 64(sp)
    ld x9, 72(sp)
    ld x10, 80(sp)
    ld x11, 88(sp)
    ld x12, 96(sp)
    ld x13, 104(sp)
    ld x14, 112(sp)
    ld x15, 120(sp)
    ld x16, 128(sp)
    ld x17, 136(sp)
    ld x18, 144(sp)
    ld x19, 152(sp)
    ld x20, 160(sp)
    ld x21, 168(sp)
    ld x22, 176(sp)
    ld x23, 184(sp)
    ld x24, 192(sp)
    ld x25, 200(sp)
    ld x26, 208(sp)
    ld x27, 216(sp)
    ld x28, 224(sp)
    ld x29, 232(sp)
    ld x30, 240(sp)
    ld x31, 248(sp)

    addi sp, sp, 256

    sret

.section .data
.global FDT_ADDR
.align 3
FDT_ADDR:
    .quad 0

.section .bss.stack
.global boot_stack_lower_bound
.global user_stack_lower_bound
kernel_stack_lower_bound:
    .space 4096 * 16
    .global kernel_stack_top
kernel_stack_top:
