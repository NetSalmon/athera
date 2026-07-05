use crate::arch::sbi::srst::{ResetReason, ResetType, system_reset};
use crate::{arch, debug, numeric, syscall};

const INTERRUPT_MASK: i64 = 1 << 63;

numeric! {
    pub enum Interrupt: i64 {
        U_MODE_SOFTWARE = INTERRUPT_MASK,
        S_MODE_SOFTWARE = INTERRUPT_MASK | 1,
        M_MODE_SOFTWARE = INTERRUPT_MASK | 3,
        USER_TIMER = INTERRUPT_MASK | 4,
        SUPERVISOR_TIMER = INTERRUPT_MASK | 5,
        MACHINE_TIMER = INTERRUPT_MASK | 7,
        USER_EXTERNAL = INTERRUPT_MASK | 8,
        SUPERVISOR_EXTERNAL = INTERRUPT_MASK | 9,
        MACHINE_EXTERNAL = INTERRUPT_MASK | 11,
    }
}

numeric! {
    pub enum Exception: i64 {
        INSTRUCTION_ADDRESS_MISALIGNED = 0,
        INSTRUCTION_ACCESS_FAULT = 1,
        ILLEGAL_INSTRUCTION = 2,
        BREAKPOINT = 3,
        LOAD_ADDRESS_MISALIGNED = 4,
        LOAD_ACCESS_FAULT = 5,
        STORE_ADDRESS_MISALIGNED = 6,
        STORE_ACCESS_FAULT = 7,
        U_MODE_ECALL = 8,
        S_MODE_ECALL = 9,
        M_MODE_ECALL = 11,
        INSTRUCTION_PAGE_FAULT = 12,
        LOAD_PAGE_FAULT = 13,
        STORE_PAGE_FAULT = 15,
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
        Trap::Interrupt(Interrupt::SUPERVISOR_TIMER) => {
            set_time();
        }
        Trap::Exception(Exception::U_MODE_ECALL) => {
            let args = unsafe { &*((trap_frame_sp + 80) as *const [u64; 8]) };

            debug!("args: {:?}", args);

            let (ret, next) = syscall::handle(args, sepc);

            unsafe { (trap_frame_sp as *mut u64).add(10).write(ret) };

            arch::registers::csr::Sepc::write(next);
        }
        Trap::Exception(Exception::BREAKPOINT) => {
            arch::registers::csr::Sepc::write(sepc + 4);
        }
        Trap::Exception(Exception::S_MODE_ECALL | Exception::M_MODE_ECALL) => {
            arch::registers::csr::Sepc::write(sepc + 4);
        }
        Trap::Exception(Exception::ILLEGAL_INSTRUCTION) => {
            system_reset(ResetType::Shutdown, ResetReason::None);
        }
        Trap::Interrupt(Interrupt::SUPERVISOR_EXTERNAL) => {}
        Trap::Exception(
            Exception::LOAD_ACCESS_FAULT
            | Exception::LOAD_PAGE_FAULT
            | Exception::STORE_ACCESS_FAULT
            | Exception::INSTRUCTION_ACCESS_FAULT,
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
