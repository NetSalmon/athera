use crate::arch::registers::{ReadableRegister, WritableRegister};
use crate::arch::sbi::srst::{ResetReason, ResetType, system_reset};
use crate::usr::SStatusBits;
use crate::{arch, debug, get_tag_address, numeric, read_as_array, syscall};

const INTERRUPT_MASK: i64 = 1 << 63;

numeric! {
    @fallback
    pub enum Interrupt: i64 {
        UModeSoftware = INTERRUPT_MASK,
        SModeSoftware = INTERRUPT_MASK | 1,
        MModeSoftware = INTERRUPT_MASK | 3,
        UserTimer = INTERRUPT_MASK | 4,
        SupervisorTimer = INTERRUPT_MASK | 5,
        MachineTimer = INTERRUPT_MASK | 7,
        UserExternal = INTERRUPT_MASK | 8,
        SupervisorExternal = INTERRUPT_MASK | 9,
        MachineExternal = INTERRUPT_MASK | 11,
    }
}

numeric! {
    @fallback
    pub enum Exception: i64 {
        InstructionAddressMisaligned = 0,
        InstructionAccessFault = 1,
        IllegalInstruction = 2,
        Breakpoint = 3,
        LoadAddressMisaligned = 4,
        LoadAccessFault = 5,
        StoreAddressMisaligned = 6,
        StoreAccessFault = 7,
        UModeEcall = 8,
        SModeEcall = 9,
        MModeEcall = 11,
        InstructionPageFault = 12,
        LoadPageFault = 13,
        StorePageFault = 15,
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Trap {
    Exception(Exception),
    Interrupt(Interrupt),
}

impl From<i64> for Trap {
    fn from(value: i64) -> Trap {
        if value > 0 {
            Trap::Exception(Exception::from(value))
        } else {
            Trap::Interrupt(Interrupt::from(value))
        }
    }
}

impl From<Exception> for Trap {
    fn from(value: Exception) -> Trap {
        Trap::Exception(value)
    }
}

impl From<Interrupt> for Trap {
    fn from(value: Interrupt) -> Trap {
        Trap::Interrupt(value)
    }
}

impl From<Trap> for i64 {
    fn from(value: Trap) -> i64 {
        match value {
            Trap::Exception(i) => i.into(),
            Trap::Interrupt(i) => i.into(),
        }
    }
}

#[unsafe(no_mangle)]
fn trap_handler(scause: u64, sepc: u64, _stval: u64, _sstatus: u64, trap_frame_sp: u64) {
    let trap = Trap::from(scause as i64);

    match trap {
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            set_time();
        }
        Trap::Exception(Exception::UModeEcall) => {
            read_as_array!(args: u64 => trap_frame_sp, 10 => 8);

            debug!("args: {:?}", args);

            let ret = syscall::handle(args);
            unsafe { (trap_frame_sp as *mut u64).add(10).write(ret) };

            if args[7] == 60 {
                get_tag_address!(addr: u64 = "kernel_do_no_thing");
                arch::registers::csr::Sepc::write(addr);
                let mut s: SStatusBits = arch::registers::csr::Sstatus::read().into();
                s.set_spp(true);
                arch::registers::csr::Sstatus::write(s.into());
            } else {
                arch::registers::csr::Sepc::write(sepc + 4);
            }
        }
        Trap::Exception(Exception::Breakpoint) => {
            arch::registers::csr::Sepc::write(sepc + 4);
        }
        Trap::Exception(Exception::SModeEcall | Exception::MModeEcall) => {
            arch::registers::csr::Sepc::write(sepc + 4);
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            system_reset(ResetType::Shutdown, ResetReason::None);
        }
        Trap::Interrupt(Interrupt::SupervisorExternal) => {}
        Trap::Exception(
            Exception::LoadAccessFault
            | Exception::LoadPageFault
            | Exception::InstructionAccessFault,
        ) => {
            system_reset(ResetType::Shutdown, ResetReason::SysFail);
        }
        _ => core::hint::spin_loop(),
    }
}

#[inline]
pub fn set_time() {
    const GAP: u64 = 1_000_000; // 10 Hz
    let t = arch::registers::csr::Time::read();
    arch::registers::csr::Stimecmp::write(t + GAP);
}
