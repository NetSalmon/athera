#[derive(Debug, Clone)]
#[repr(C)]
pub struct RecordString(pub [u8; 16]);

impl RecordString {
    /// 把定长 16 字节的 `RecordString` 转为 `&str`。
    ///
    /// 从开头起遇到第一个 NUL 即截断（未写满的尾部以 0 填充），
    /// 若剩余字节不是合法 UTF-8 则返回空串。
    pub fn as_str(&self) -> &str {
        let end = self.0.iter().position(|&b| b == 0).unwrap_or(self.0.len());
        core::str::from_utf8(&self.0[..end]).unwrap_or("")
    }
}

impl core::fmt::Display for RecordString {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for RecordString {
    fn from(s: &str) -> Self {
        let bytes = s.as_bytes();
        assert!(bytes.len() < 16, "RecordString: file name too long");
        let mut buf = [0u8; 16];
        buf[..bytes.len()].copy_from_slice(bytes);
        RecordString(buf)
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct File {
    // block id
    pub start: u32,
    // bytes
    pub size: u32,
    pub file_name: RecordString,
}

impl File {
    pub fn is_empty(&self) -> bool {
        self.start == 0 && self.size == 0
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct Index {
    pub files: [File; 21],
    // block id
    pub next_index: u64,
}

impl Index {
    pub fn as_slice(&self) -> &[u8; 512] {
        unsafe { &*(self as *const _ as *const [u8; 512]) }
    }

    /// 从磁盘读出的 512 字节构造 `Index`。
    ///
    /// 用 `read_unaligned` 按字节拷贝，不要求缓冲区 8 字节对齐。
    pub fn from_slice(slice: &[u8; 512]) -> Self {
        unsafe { core::ptr::read_unaligned(slice.as_ptr() as *const Self) }
    }
}

// 磁盘格式固定：File = 24B，Index = 21 * 24 + 8 = 512B
const _: () = assert!(size_of::<File>() == 24);
const _: () = assert!(size_of::<Index>() == 512);
