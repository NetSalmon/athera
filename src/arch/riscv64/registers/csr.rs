#![allow(dead_code)]
//! Control and Status Register (CSR) accessors.
//!
//! The table below records the standard RISC-V CSR assignments used by the
//! kernel. Accessors are generated from it with the local `csr!` macro. A
//! read-only CSR only exposes `read()`, while a read/write CSR additionally
//! exposes `write()`, `modify()`, `set_bits()`, and `clear_bits()`.
// | Address | Name       | Mode | Access | Description                          |
// | ------- | ---------- | ---- | ------ | ------------------------------------ |
// | 0x001   | fflags     | U    | rw     | Floating-point accrued exceptions    |
// | 0x002   | frm        | U    | rw     | Floating-point dynamic rounding mode |
// | 0x003   | fcsr       | U    | rw     | Floating-point control and status    |
// | 0x000   | ustatus    | U    | rw     | User status register                 |
// | 0x004   | uie        | U    | rw     | User interrupt-enable register       |
// | 0x005   | utvec      | U    | rw     | User trap handler base address       |
// | 0x040   | uscratch   | U    | rw     | User scratch register                |
// | 0x041   | uepc       | U    | rw     | User exception program counter       |
// | 0x042   | ucause     | U    | rw     | User trap cause                      |
// | 0x043   | utval      | U    | rw     | User bad address or instruction      |
// | 0x044   | uip        | U    | rw     | User interrupt pending               |
// | 0xC00   | cycle      | U    | r      | Cycle counter                        |
// | 0xC01   | time       | U    | r      | Timer                                |
// | 0xC02   | instret    | U    | r      | Instructions-retired counter         |
// | 0x100   | sstatus    | S    | rw     | Supervisor status register           |
// | 0x102   | sedeleg    | S    | rw     | Supervisor exception delegation      |
// | 0x103   | sideleg    | S    | rw     | Supervisor interrupt delegation      |
// | 0x104   | sie        | S    | rw     | Supervisor interrupt-enable register |
// | 0x105   | stvec      | S    | rw     | Supervisor trap handler base address |
// | 0x106   | scounteren | S    | rw     | Supervisor counter enable            |
// | 0x140   | sscratch   | S    | rw     | Supervisor scratch register          |
// | 0x141   | sepc       | S    | rw     | Supervisor exception program counter |
// | 0x142   | scause     | S    | rw     | Supervisor trap cause                |
// | 0x143   | stval      | S    | rw     | Supervisor bad address or instruction|
// | 0x144   | sip        | S    | rw     | Supervisor interrupt pending         |
// | 0x180   | satp       | S    | rw     | Supervisor address translation       |
// | 0xDA0   | scountovf  | S    | r      | Counter overflow                     |
// | 0x14D   | stimecmp   | S    | rw     | Supervisor timer compare (Sstc)      |
// | 0xF11   | mvendorid  | M    | r      | Vendor ID                            |
// | 0xF12   | marchid    | M    | r      | Architecture ID                      |
// | 0xF13   | mimpid     | M    | r      | Implementation ID                    |
// | 0xF14   | mhartid    | M    | r      | Hardware thread ID                   |
// | 0xF15   | mconfigptr | M    | r      | Pointer to config structure          |
// | 0x300   | mstatus    | M    | rw     | Machine status register              |
// | 0x301   | misa       | M    | rw     | ISA and extensions                   |
// | 0x302   | medeleg    | M    | rw     | Machine exception delegation         |
// | 0x303   | mideleg    | M    | rw     | Machine interrupt delegation         |
// | 0x304   | mie        | M    | rw     | Machine interrupt-enable register    |
// | 0x305   | mtvec      | M    | rw     | Machine trap handler base address    |
// | 0x306   | mcounteren | M    | rw     | Machine counter enable               |
// | 0x310   | mstatush   | M    | rw     | Machine status (high bits, RV32)     |
// | 0x320   | mcountinhibit| M  | rw     | Machine counter inhibit              |
// | 0x340   | mscratch   | M    | rw     | Machine scratch register             |
// | 0x341   | mepc       | M    | rw     | Machine exception program counter    |
// | 0x342   | mcause     | M    | rw     | Machine trap cause                   |
// | 0x343   | mtval      | M    | rw     | Machine bad address or instruction   |
// | 0x344   | mip        | M    | rw     | Machine interrupt pending            |
// | 0x34A   | mtinst     | M    | rw     | Machine trap instruction (transformed)|
// | 0x34B   | mtval2     | M    | rw     | Machine bad guest physical address   |
// | 0x30A   | menvcfg    | M    | rw     | Machine environment configuration    |
// | 0x747   | mseccfg    | M    | rw     | Machine security configuration       |
// | 0x7A0   | tselect    | M    | rw     | Trigger select                       |
// | 0x7A1   | tdata1     | M    | rw     | Trigger data register 1              |
// | 0x7A2   | tdata2     | M    | rw     | Trigger data register 2              |
// | 0x7A3   | tdata3     | M    | rw     | Trigger data register 3              |
// | 0x7A8   | mcontext   | M    | rw     | Machine context register             |
// | 0xB00   | mcycle     | M    | rw     | Machine cycle counter                |
// | 0xB02   | minstret   | M    | rw     | Machine instructions-retired counter |
// | 0x323   | mhpmevent3 | M    | rw     | Machine event selector 3             |
// | 0x324   | mhpmevent4 | M    | rw     | Machine event selector 4             |
// | 0x325   | mhpmevent5 | M    | rw     | Machine event selector 5             |
// | 0x326   | mhpmevent6 | M    | rw     | Machine event selector 6             |
// | 0x327   | mhpmevent7 | M    | rw     | Machine event selector 7             |
// | 0x328   | mhpmevent8 | M    | rw     | Machine event selector 8             |
// | 0x329   | mhpmevent9 | M    | rw     | Machine event selector 9             |
// | 0x32A   | mhpmevent10| M    | rw     | Machine event selector 10            |
// | 0x32B   | mhpmevent11| M    | rw     | Machine event selector 11            |
// | 0x32C   | mhpmevent12| M    | rw     | Machine event selector 12            |
// | 0x32D   | mhpmevent13| M    | rw     | Machine event selector 13            |
// | 0x32E   | mhpmevent14| M    | rw     | Machine event selector 14            |
// | 0x32F   | mhpmevent15| M    | rw     | Machine event selector 15            |
// | 0x330   | mhpmevent16| M    | rw     | Machine event selector 16            |
// | 0x331   | mhpmevent17| M    | rw     | Machine event selector 17            |
// | 0x332   | mhpmevent18| M    | rw     | Machine event selector 18            |
// | 0x333   | mhpmevent19| M    | rw     | Machine event selector 19            |
// | 0x334   | mhpmevent20| M    | rw     | Machine event selector 20            |
// | 0x335   | mhpmevent21| M    | rw     | Machine event selector 21            |
// | 0x336   | mhpmevent22| M    | rw     | Machine event selector 22            |
// | 0x337   | mhpmevent23| M    | rw     | Machine event selector 23            |
// | 0x338   | mhpmevent24| M    | rw     | Machine event selector 24            |
// | 0x339   | mhpmevent25| M    | rw     | Machine event selector 25            |
// | 0x33A   | mhpmevent26| M    | rw     | Machine event selector 26            |
// | 0x33B   | mhpmevent27| M    | rw     | Machine event selector 27            |
// | 0x33C   | mhpmevent28| M    | rw     | Machine event selector 28            |
// | 0x33D   | mhpmevent29| M    | rw     | Machine event selector 29            |
// | 0x33E   | mhpmevent30| M    | rw     | Machine event selector 30            |
// | 0x33F   | mhpmevent31| M    | rw     | Machine event selector 31            |
// | 0x3A0   | pmpcfg0    | M    | rw     | PMP configuration (pmp0..pmp7)       |
// | 0x3A2   | pmpcfg2    | M    | rw     | PMP configuration (pmp8..pmp15)      |
// | 0x3B0   | pmpaddr0   | M    | rw     | PMP address 0                        |
// | 0x3B1   | pmpaddr1   | M    | rw     | PMP address 1                        |
// | 0x3B2   | pmpaddr2   | M    | rw     | PMP address 2                        |
// | 0x3B3   | pmpaddr3   | M    | rw     | PMP address 3                        |
// | 0x3B4   | pmpaddr4   | M    | rw     | PMP address 4                        |
// | 0x3B5   | pmpaddr5   | M    | rw     | PMP address 5                        |
// | 0x3B6   | pmpaddr6   | M    | rw     | PMP address 6                        |
// | 0x3B7   | pmpaddr7   | M    | rw     | PMP address 7                        |
// | 0x3B8   | pmpaddr8   | M    | rw     | PMP address 8                        |
// | 0x3B9   | pmpaddr9   | M    | rw     | PMP address 9                        |
// | 0x3BA   | pmpaddr10  | M    | rw     | PMP address 10                       |
// | 0x3BB   | pmpaddr11  | M    | rw     | PMP address 11                       |
// | 0x3BC   | pmpaddr12  | M    | rw     | PMP address 12                       |
// | 0x3BD   | pmpaddr13  | M    | rw     | PMP address 13                       |
// | 0x3BE   | pmpaddr14  | M    | rw     | PMP address 14                       |
// | 0x3BF   | pmpaddr15  | M    | rw     | PMP address 15                       |

use core::arch::asm;

/// CSR 的类型级描述。
pub trait CsrRegister {
    /// 返回该 CSR 的特权级。
    fn mode() -> Mode;
    /// 返回该 CSR 的架构编号。
    fn number() -> u16;
}

/// CSR 所属的 RISC-V 特权级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Machine,
    Supervisor,
    User,
}

macro_rules! csr {
    (@core $name:ident, $number:expr, $mode:expr) => {
        pub struct $name;

        impl CsrRegister for $name {
            fn mode() -> Mode {
                $mode
            }

            fn number() -> u16 {
                $number
            }
        }
    };
    (@read $name:ident) => {
        #[inline]
        pub fn read() -> u64 {
            let value: u64;
            unsafe {
                asm!(
                concat!("csrr {}, ", stringify!($name)),
                out(reg) value,
                );
            }

            value
        }
    };
    (@write $name:ident) => {
        #[inline]
        pub fn write(value: u64) {
            unsafe {
                asm!(
                concat!("csrw ", stringify!($name), ", {}"),
                in(reg) value,
                );
            }
        }
    };
    (# $number:expr, $name:ident - $mode:expr => r) => {
        paste::paste! {
            csr!(@core [<$name:camel>], $number, $mode);

            impl [<$name:camel>] {
                csr!(@read $name);
            }
        }
    };
    (# $number:expr, $name:ident - $mode:expr => rw) => {
        paste::paste! {
            csr!(@core [<$name:camel>], $number, $mode);

            impl [<$name:camel>] {
                csr!(@write $name);
                csr!(@read $name);

                #[inline]
                pub fn modify<T>(func: T) where T: FnOnce(u64) -> u64 {
                    [<$name:camel>]::write(func([<$name:camel>]::read()))
                }

                /// 设置指定的位，其他位保持不变。
                #[inline]
                pub fn set_bits(mask: u64) {
                    [<$name:camel>]::modify(|value| value | mask)
                }

                /// 清除指定的位，其他位保持不变。
                #[inline]
                pub fn clear_bits(mask: u64) {
                    [<$name:camel>]::modify(|value| value & !mask)
                }
            }
        }
    };
}

// ─── Floating-Point CSRs (U-mode accessible) ────────────────────────
csr!(#0x001, fflags - Mode::User => rw);
csr!(#0x002, frm - Mode::User => rw);
csr!(#0x003, fcsr - Mode::User => rw);

// ─── User Trap Setup ─────────────────────────────────────────────────
csr!(#0x000, ustatus - Mode::User => rw);
csr!(#0x004, uie - Mode::User => rw);
csr!(#0x005, utvec - Mode::User => rw);

// ─── User Trap Handling ──────────────────────────────────────────────
csr!(#0x040, uscratch - Mode::User => rw);
csr!(#0x041, uepc - Mode::User => rw);
csr!(#0x042, ucause - Mode::User => rw);
csr!(#0x043, utval - Mode::User => rw);
csr!(#0x044, uip - Mode::User => rw);

// ─── User Counter/Timers ─────────────────────────────────────────────
csr!(#0xC00, cycle - Mode::User => r);
csr!(#0xC01, time - Mode::User => r);
csr!(#0xC02, instret - Mode::User => r);

// ─── Supervisor Trap Setup ───────────────────────────────────────────
csr!(#0x100, sstatus - Mode::Supervisor => rw);
csr!(#0x102, sedeleg - Mode::Supervisor => rw);
csr!(#0x103, sideleg - Mode::Supervisor => rw);
csr!(#0x104, sie - Mode::Supervisor => rw);
csr!(#0x105, stvec - Mode::Supervisor => rw);
csr!(#0x106, scounteren - Mode::Supervisor => rw);

// ─── Supervisor Trap Handling ────────────────────────────────────────
csr!(#0x140, sscratch - Mode::Supervisor => rw);
csr!(#0x141, sepc - Mode::Supervisor => rw);
csr!(#0x142, scause - Mode::Supervisor => rw);
csr!(#0x143, stval - Mode::Supervisor => rw);
csr!(#0x144, sip - Mode::Supervisor => rw);

// ─── Supervisor Protection and Translation ───────────────────────────
csr!(#0x180, satp - Mode::Supervisor => rw);

// ─── Supervisor Counter Overflow ─────────────────────────────────────
csr!(#0xDA0, scountovf - Mode::Supervisor => r);

// ─── Supervisor Timer (Sstc) ─────────────────────────────────────────
csr!(#0x14D, stimecmp - Mode::Supervisor => rw);

// ─── Machine Information ─────────────────────────────────────────────
csr!(#0xF11, mvendorid - Mode::Machine => r);
csr!(#0xF12, marchid - Mode::Machine => r);
csr!(#0xF13, mimpid - Mode::Machine => r);
csr!(#0xF14, mhartid - Mode::Machine => r);
csr!(#0xF15, mconfigptr - Mode::Machine => r);

// ─── Machine Trap Setup ──────────────────────────────────────────────
csr!(#0x300, mstatus - Mode::Machine => rw);
csr!(#0x301, misa - Mode::Machine => rw);
csr!(#0x302, medeleg - Mode::Machine => rw);
csr!(#0x303, mideleg - Mode::Machine => rw);
csr!(#0x304, mie - Mode::Machine => rw);
csr!(#0x305, mtvec - Mode::Machine => rw);
csr!(#0x306, mcounteren - Mode::Machine => rw);
csr!(#0x310, mstatush - Mode::Machine => rw);
csr!(#0x320, mcountinhibit - Mode::Machine => rw);

// ─── Machine Trap Handling ───────────────────────────────────────────
csr!(#0x340, mscratch - Mode::Machine => rw);
csr!(#0x341, mepc - Mode::Machine => rw);
csr!(#0x342, mcause - Mode::Machine => rw);
csr!(#0x343, mtval - Mode::Machine => rw);
csr!(#0x344, mip - Mode::Machine => rw);
csr!(#0x34A, mtinst - Mode::Machine => rw);
csr!(#0x34B, mtval2 - Mode::Machine => rw);

// ─── Machine Configuration ───────────────────────────────────────────
csr!(#0x30A, menvcfg - Mode::Machine => rw);
csr!(#0x747, mseccfg - Mode::Machine => rw);

// ─── Machine Debug / Trigger ─────────────────────────────────────────
csr!(#0x7A0, tselect - Mode::Machine => rw);
csr!(#0x7A1, tdata1 - Mode::Machine => rw);
csr!(#0x7A2, tdata2 - Mode::Machine => rw);
csr!(#0x7A3, tdata3 - Mode::Machine => rw);
csr!(#0x7A8, mcontext - Mode::Machine => rw);

// ─── Machine Counter/Timers ──────────────────────────────────────────
csr!(#0xB00, mcycle - Mode::Machine => rw);
csr!(#0xB02, minstret - Mode::Machine => rw);

// ─── Machine Performance Monitors ────────────────────────────────────
csr!(#0x323, mhpmevent3 - Mode::Machine => rw);
csr!(#0x324, mhpmevent4 - Mode::Machine => rw);
csr!(#0x325, mhpmevent5 - Mode::Machine => rw);
csr!(#0x326, mhpmevent6 - Mode::Machine => rw);
csr!(#0x327, mhpmevent7 - Mode::Machine => rw);
csr!(#0x328, mhpmevent8 - Mode::Machine => rw);
csr!(#0x329, mhpmevent9 - Mode::Machine => rw);
csr!(#0x32A, mhpmevent10 - Mode::Machine => rw);
csr!(#0x32B, mhpmevent11 - Mode::Machine => rw);
csr!(#0x32C, mhpmevent12 - Mode::Machine => rw);
csr!(#0x32D, mhpmevent13 - Mode::Machine => rw);
csr!(#0x32E, mhpmevent14 - Mode::Machine => rw);
csr!(#0x32F, mhpmevent15 - Mode::Machine => rw);
csr!(#0x330, mhpmevent16 - Mode::Machine => rw);
csr!(#0x331, mhpmevent17 - Mode::Machine => rw);
csr!(#0x332, mhpmevent18 - Mode::Machine => rw);
csr!(#0x333, mhpmevent19 - Mode::Machine => rw);
csr!(#0x334, mhpmevent20 - Mode::Machine => rw);
csr!(#0x335, mhpmevent21 - Mode::Machine => rw);
csr!(#0x336, mhpmevent22 - Mode::Machine => rw);
csr!(#0x337, mhpmevent23 - Mode::Machine => rw);
csr!(#0x338, mhpmevent24 - Mode::Machine => rw);
csr!(#0x339, mhpmevent25 - Mode::Machine => rw);
csr!(#0x33A, mhpmevent26 - Mode::Machine => rw);
csr!(#0x33B, mhpmevent27 - Mode::Machine => rw);
csr!(#0x33C, mhpmevent28 - Mode::Machine => rw);
csr!(#0x33D, mhpmevent29 - Mode::Machine => rw);
csr!(#0x33E, mhpmevent30 - Mode::Machine => rw);
csr!(#0x33F, mhpmevent31 - Mode::Machine => rw);

// ─── Physical Memory Protection ──────────────────────────────────────
csr!(#0x3A0, pmpcfg0 - Mode::Machine => rw);
csr!(#0x3A2, pmpcfg2 - Mode::Machine => rw);
csr!(#0x3B0, pmpaddr0 - Mode::Machine => rw);
csr!(#0x3B1, pmpaddr1 - Mode::Machine => rw);
csr!(#0x3B2, pmpaddr2 - Mode::Machine => rw);
csr!(#0x3B3, pmpaddr3 - Mode::Machine => rw);
csr!(#0x3B4, pmpaddr4 - Mode::Machine => rw);
csr!(#0x3B5, pmpaddr5 - Mode::Machine => rw);
csr!(#0x3B6, pmpaddr6 - Mode::Machine => rw);
csr!(#0x3B7, pmpaddr7 - Mode::Machine => rw);
csr!(#0x3B8, pmpaddr8 - Mode::Machine => rw);
csr!(#0x3B9, pmpaddr9 - Mode::Machine => rw);
csr!(#0x3BA, pmpaddr10 - Mode::Machine => rw);
csr!(#0x3BB, pmpaddr11 - Mode::Machine => rw);
csr!(#0x3BC, pmpaddr12 - Mode::Machine => rw);
csr!(#0x3BD, pmpaddr13 - Mode::Machine => rw);
csr!(#0x3BE, pmpaddr14 - Mode::Machine => rw);
csr!(#0x3BF, pmpaddr15 - Mode::Machine => rw);
