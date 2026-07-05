// | Register| ABI Name | Description                      | Access | Saver  |
// | ------- | -------- | -------------------------------- | ------ | ------ |
// | x0      | zero     | Hard-wired zero                  | r      | —      |
// | x1      | ra       | Return address                   | rw     | Caller |
// | x2      | sp       | Stack pointer                    | rw     | Callee |
// | x3      | gp       | Global pointer                   | rw     | —      |
// | x4      | tp       | Thread pointer                   | rw     | —      |
// | x5–7    | t0–2     | Temporaries                      | rw     | Caller |
// | x8      | s0/fp    | Saved register/frame pointer     | rw     | Callee |
// | x9      | s1       | Saved register                   | rw     | Callee |
// | x10–11  | a0–1     | Function arguments/return values | rw     | Caller |
// | x12–17  | a2–7     | Function arguments               | rw     | Caller |
// | x18–27  | s2–11    | Saved registers                  | rw     | Callee |
// | x28–31  | t3–6     | Temporaries                      | rw     | Caller |
// | f0–7    | ft0–7    | FP temporaries                   | rw     | Caller |
// | f8–9    | fs0–1    | FP saved registers               | rw     | Callee |
// | f10–11  | fa0–1    | FP arguments/return values       | rw     | Caller |
// | f12–17  | fa2–7    | FP arguments                     | rw     | Caller |
// | f18–27  | fs2–11   | FP saved registers               | rw     | Callee |
// | f28–31  | ft8–11   | FP temporaries                   | rw     | Caller |
use crate::arch::registers::*;

macro_rules! gpr {
    (@read $name:ident) => {
        #[inline]
        pub fn write(value: u64) {
            unsafe { core::arch::asm!( concat!("mv ", stringify!($name), ", {}"), in(reg) value); }
        }
    };
    (@write $name:ident) => {
        #[inline]
        pub fn read() -> u64 {
            let value: u64;
            unsafe { core::arch::asm!( concat!("mv {}, ", stringify!($name)), out(reg) value); }
            value
        }
    };
    ($name:ident $(-)? $($alias:ident),* => r) => {
        paste::paste! {
            pub struct [<$name:camel>];
            $(pub type [<$alias:camel>] = [<$name:camel>];)*
            impl [<$name:camel>] {
                gpr!(@read $name);
            }
        }
    };
    ($name:ident $(-)? $($alias:ident),* => rw) => {
        paste::paste! {
            pub struct [<$name:camel>];
            $(pub type [<$alias:camel>] = [<$name:camel>];)*

            impl [<$name:camel>] {
                gpr!(@write $name);
                gpr!(@read $name);

                pub fn modify<T>(func: T) where T: FnOnce(u64) -> u64 {
                    [<$name:camel>]::write(func([<$name:camel>]::read()))
                }
            }
        }
    };
}

gpr!(x0 - zero => r);
gpr!(x1 - ra=> rw);
gpr!(x2 - sp=> rw);
gpr!(x3 - gp=> rw);
gpr!(x4 - tp=> rw);
gpr!(x5 - t0=> rw);
gpr!(x6 - t1=> rw);
gpr!(x7 - t2=> rw);
gpr!(x8 - s0,fp=> rw);
gpr!(x9 - s1=> rw);
gpr!(x10 - a0=> rw);
gpr!(x11 - a1=> rw);
gpr!(x12 - a2=> rw);
gpr!(x13 - a3=> rw);
gpr!(x14 - a4=> rw);
gpr!(x15 - a5=> rw);
gpr!(x16 - a6=> rw);
gpr!(x17 - a7=> rw);
gpr!(x18 - s2=> rw);
gpr!(x19 - s3=> rw);
gpr!(x20 - s4=> rw);
gpr!(x21 - s5=> rw);
gpr!(x22 - s6=> rw);
gpr!(x23 - s7=> rw);
gpr!(x24 - s8=> rw);
gpr!(x25 - s9=> rw);
gpr!(x26 - s10=> rw);
gpr!(x27 - s11=> rw);
gpr!(x28 - t3=> rw);
gpr!(x29 - t4=> rw);
gpr!(x30 - t5=> rw);
gpr!(x31 - t6=> rw);
