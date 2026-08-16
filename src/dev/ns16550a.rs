#![allow(dead_code)]
//! ns16550a UART 驱动。
//!
//! 通过 `mmio_regs!` 访问 RBR/THR、IER、FCR、LCR、LSR 寄存器，支持
//! FIFO/行控制初始化、字符收发与设备树探测。
use core::fmt;

use fdt::Fdt;

use crate::{
    bits,
    dev::{
        device::{Device, Resource},
        traits::{CharDevice, Dev, IoResult},
    },
    mmio_regs,
};

impl Dev for Ns16550a {
    fn name(&self) -> &'static str {
        "ns16550a"
    }

    fn irq(&self) -> Option<usize> {
        self.device.irq
    }
}

impl CharDevice for Ns16550a {
    fn read(&self, buf: &mut [u8]) -> IoResult<usize> {
        let mut read = 0;
        for byte in buf {
            let Some(value) = self.rbr_thr_if_ready() else {
                break;
            };
            *byte = value;
            read += 1;
        }
        Ok(read)
    }

    fn write(&self, buf: &[u8]) -> IoResult<usize> {
        for &byte in buf {
            while !self.lsr().thre() {}
            self.write_rbr_thr(byte);
        }
        Ok(buf.len())
    }
}

/// 让 UART 满足 `core::fmt::Write`，供 `print!` / `println!` 输出。
impl fmt::Write for Ns16550a {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let _ = CharDevice::write(self, s.as_bytes());
        Ok(())
    }
}

pub struct Ns16550a {
    pub device: Device,
}

mmio_regs! {
    Ns16550a: [
        rbr_thr => 0,
        ier: IerStatus => 1,
        fcr: FcrStatus => 2,
        lcr: LcrStatus => 3,
        lsr: LsrStatus => 5,
    ]
}

bits! {
    pub type LsrStatus: u8 {
        dr: 0,      // Data Ready
        thre: 5,    // Transmit Holding Register Empty
    }
}

bits! {
    pub type FcrStatus: u8 {
        enable: 0,
        clear_rx: 1,
        clear_tx: 2,
    }
}

bits! {
    pub type LcrStatus: u8 {
        word_len_lo: 0,
        word_len_hi: 1,
        stop_bits: 2,
        dlab: 7,
    }
}

bits! {
    pub type IerStatus: u8 { }
}

impl Ns16550a {
    pub fn init(&self) {
        let mut fcr = FcrStatus::new();
        fcr.set_enable(true);
        fcr.set_clear_rx(true);
        fcr.set_clear_tx(true);
        self.write_fcr(fcr);

        let mut lcr = LcrStatus::new();
        lcr.set_word_len_lo(true);
        lcr.set_word_len_hi(true);
        self.write_lcr(lcr);
    }

    fn rbr_thr_if_ready(&self) -> Option<u8> {
        if self.lsr().dr() {
            self.rbr_thr()
        } else {
            None
        }
    }

    pub fn probe(fdt: &Fdt) -> Option<Self> {
        let uart = fdt.find_node("/soc/serial")?;
        let irq = uart.interrupts()?.next().unwrap_or(0);

        let reg = uart.reg()?.next()?;
        let start = reg.starting_address as usize;
        let size = reg.size.unwrap_or(0);

        Some(Self {
            device: Device {
                mmio: Resource { start, size },
                irq: Some(irq),
            },
        })
    }
}
