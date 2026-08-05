// not a file system

pub struct File {
    // block id
    pub start: u32,
    // bytes
    pub size: u32,
}

impl File {
    pub fn is_empty(&self) -> bool {
        self.start == 0 && self.size == 0
    }
}

pub struct Index {
    pub files: [File; 63],
    // block id
    pub next_index: u64,
}

impl Index {
    pub fn as_slice(&self) -> &[u8; 512] {
        unsafe { &*(self as *const _ as *const [u8; 512]) }
    }

    pub fn from_slice(slice: &[u8; 512]) -> &Self {
        unsafe { &*(slice.as_ptr() as *const Self) }
    }
}

const _: () = assert!(size_of::<Index>() == 512);
