#![allow(dead_code)]
use crate::bits;

pub trait AsPAddr {
    fn as_paddr(&self) -> PhysicalAddr;
}

pub trait AsVAddr {
    fn as_vaddr(&self) -> VirtualAddr;
}

bits! {
    pub type VirtualAddr : usize {
        page_offset: 0 => 11,
        vpn0: 12 => 20,
        vpn1: 21 => 29,
        vpn2: 30 => 38,
        vpn: 12 => 38,
    }
}

bits! {
    pub type PhysicalAddr : usize {
        page_offset: 0 => 11,
        ppn0: 12 => 20,
        ppn1: 21 => 29,
        ppn2: 30 => 55,
        ppn: 12 => 55,
    }
}

impl<T> AsPAddr for *mut T {
    fn as_paddr(&self) -> PhysicalAddr {
        PhysicalAddr::from(*self as usize)
    }
}

impl<T> AsPAddr for *const T {
    fn as_paddr(&self) -> PhysicalAddr {
        PhysicalAddr::from(*self as usize)
    }
}

impl<T> AsPAddr for &T {
    fn as_paddr(&self) -> PhysicalAddr {
        PhysicalAddr::from(*self as *const T as usize)
    }
}

impl<T> AsPAddr for &mut T {
    fn as_paddr(&self) -> PhysicalAddr {
        PhysicalAddr::from(*self as *const T as usize)
    }
}

impl<T> AsVAddr for *const T {
    fn as_vaddr(&self) -> VirtualAddr {
        VirtualAddr::from(*self as usize)
    }
}

impl<T> AsVAddr for *mut T {
    fn as_vaddr(&self) -> VirtualAddr {
        VirtualAddr::from(*self as usize)
    }
}

impl<T> AsVAddr for &T {
    fn as_vaddr(&self) -> VirtualAddr {
        VirtualAddr::from(*self as *const T as usize)
    }
}

impl<T> AsVAddr for &mut T {
    fn as_vaddr(&self) -> VirtualAddr {
        VirtualAddr::from(*self as *const T as usize)
    }
}
