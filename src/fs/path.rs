#![allow(unused)]
//! 路径类型：仿 `std::path` 的 [`Path`] / [`PathBuf`] 与分量迭代器。
//!
//! 内部以 UTF-8 字符串保存，路径分隔符固定为 `/`。

use alloc::{string::String, vec::Vec};
use core::fmt::{Debug, Display, Formatter, Write};

/// 路径分隔符。
pub const PATH_SEPARATOR: &str = "/";

/// 路径（仿 `std::path::Path`，内部以 UTF-8 字符串保存，为自有类型）。
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Path {
    inner: String,
}

impl Path {
    /// 从字符串构造路径（仿 `std::path::Path::new`）。
    pub fn new<S: AsRef<str>>(s: S) -> Self {
        Path {
            inner: String::from(s.as_ref()),
        }
    }

    /// 兼容旧接口：同 [`new`](Self::new)。
    pub fn from_str(s: &str) -> Self {
        Self::new(s)
    }

    /// 路径的字符串视图。
    pub fn as_str(&self) -> &str {
        &self.inner
    }

    /// 是否绝对路径（以 `/` 开头）。
    pub fn is_absolute(&self) -> bool {
        self.inner.starts_with(PATH_SEPARATOR)
    }

    /// 是否相对路径。
    pub fn is_relative(&self) -> bool {
        !self.is_absolute()
    }

    /// 父路径（仿 `std::path::Path::parent`）：
    /// `"/a/b" -> "/a"`、`"/a" -> "/"`、`"a/b" -> "a"`、`"a" -> ""`、
    /// `"/"` / `""` -> `None`。
    pub fn parent(&self) -> Option<&str> {
        let s = self.inner.trim_end_matches(PATH_SEPARATOR);
        if s.is_empty() {
            return None; // "/" 或 ""
        }
        match s.rfind(PATH_SEPARATOR) {
            Some(0) => Some("/"),
            Some(pos) => Some(&s[..pos]),
            None => Some(""),
        }
    }

    /// 文件名：最后一个分量（仿 `std::path::Path::file_name`）。
    /// `"/a/b" -> Some("b")`、`"/"` / `""` -> `None`。
    pub fn file_name(&self) -> Option<&str> {
        let s = self.inner.trim_end_matches(PATH_SEPARATOR);
        match s.rfind(PATH_SEPARATOR) {
            Some(pos) if pos + 1 < s.len() => Some(&s[pos + 1..]),
            None if !s.is_empty() => Some(s),
            _ => None,
        }
    }

    /// 扩展名：文件名中最后一个 `.` 之后的部分（不含点），没有则为 `None`。
    pub fn extension(&self) -> Option<&str> {
        let name = self.file_name()?;
        let pos = name.rfind('.')?;
        if pos + 1 < name.len() {
            Some(&name[pos + 1..])
        } else {
            None
        }
    }

    /// 是否以 `base` 为前缀（按分量比较，如 `/a/b` 以 `/a` 开头）。
    pub fn starts_with<P: AsRef<Path>>(&self, base: P) -> bool {
        let a: Vec<Component<'_>> = self.components().collect();
        let b: Vec<Component<'_>> = base.as_ref().components().collect();
        b.len() <= a.len() && a[..b.len()] == b[..]
    }

    /// 是否以 `child` 为后缀（按分量比较，如 `/a/b` 以 `b` 结尾）。
    pub fn ends_with<P: AsRef<Path>>(&self, child: P) -> bool {
        let a: Vec<Component<'_>> = self.components().collect();
        let b: Vec<Component<'_>> = child.as_ref().components().collect();
        b.len() <= a.len() && a[a.len() - b.len()..] == b[..]
    }

    /// 拼接路径，返回 [`PathBuf`]（仿 `std::path::Path::join`）。
    pub fn join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        let mut buf = PathBuf::from(self.clone());
        buf.push(path);
        buf
    }

    /// 转成 [`PathBuf`]。
    pub fn to_path_buf(&self) -> PathBuf {
        PathBuf::from(self.clone())
    }

    /// 组件迭代器（仿 `std::path::Path::components`）：识别根目录、`.`、
    /// `..` 与普通分量，跳过连续分隔符。
    pub fn components(&self) -> Components<'_> {
        Components {
            parts: self.inner.split(PATH_SEPARATOR),
            is_absolute: self.is_absolute(),
            seen_root: false,
        }
    }
}

impl Debug for Path {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_char('"')?;
        f.write_str(&self.inner)?;
        f.write_char('"')
    }
}

impl Display for Path {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.inner)
    }
}

impl AsRef<str> for Path {
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl PartialEq<str> for Path {
    fn eq(&self, other: &str) -> bool {
        self.inner == other
    }
}

impl PartialEq<&str> for Path {
    fn eq(&self, other: &&str) -> bool {
        self.inner == *other
    }
}

impl PartialEq<String> for Path {
    fn eq(&self, other: &String) -> bool {
        self.inner == *other
    }
}

/// 可增长的路径缓冲（仿 `std::path::PathBuf`），可 `Deref` 成 [`Path`]。
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PathBuf {
    inner: Path,
}

impl PathBuf {
    /// 新建空路径缓冲。
    pub fn new() -> Self {
        PathBuf {
            inner: Path::default(),
        }
    }

    /// 追加分量（仿 `std::path::PathBuf::push`）；参数为绝对路径时整体替换。
    pub fn push<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref();
        if path.is_absolute() {
            self.inner = path.clone();
            return;
        }
        let path_str = path.as_str();
        if !path_str.is_empty() {
            if !self.inner.inner.is_empty() && !self.inner.inner.ends_with(PATH_SEPARATOR) {
                self.inner.inner.push_str(PATH_SEPARATOR);
            }
            self.inner.inner.push_str(path_str);
        }
    }

    /// 去掉最后一个分量（仿 `std::path::PathBuf::pop`）；无可弹分量时返回
    /// `false`。
    pub fn pop(&mut self) -> bool {
        let s = self.inner.inner.trim_end_matches(PATH_SEPARATOR);
        if s.is_empty() {
            return false; // 根 "/" 或空路径
        }
        match s.rfind(PATH_SEPARATOR) {
            Some(pos) => {
                self.inner.inner.truncate(if pos == 0 { 1 } else { pos });
                true
            }
            None => {
                self.inner.inner.clear();
                true
            }
        }
    }

    /// 设置文件名（仿 `std::path::PathBuf::set_file_name`）。
    pub fn set_file_name<S: AsRef<str>>(&mut self, file_name: S) {
        let name = file_name.as_ref();
        match self.inner.parent() {
            Some(parent) => {
                let mut new = String::from(parent);
                if !new.is_empty() && !new.ends_with(PATH_SEPARATOR) {
                    new.push_str(PATH_SEPARATOR);
                }
                new.push_str(name);
                self.inner.inner = new;
            }
            None => {
                let mut new = if self.inner.is_absolute() {
                    String::from(PATH_SEPARATOR)
                } else {
                    String::new()
                };
                new.push_str(name);
                self.inner.inner = new;
            }
        }
    }

    /// 拼接路径，返回新的 [`PathBuf`]（仿 `std::path::PathBuf::join`）。
    pub fn join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        let mut buf = self.clone();
        buf.push(path);
        buf
    }

    /// 转成 [`Path`] 引用。
    pub fn as_path(&self) -> &Path {
        &self.inner
    }

    /// 消费自身，返回内部字符串。
    pub fn into_string(self) -> String {
        self.inner.inner
    }
}

impl Debug for PathBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        Debug::fmt(&self.inner, f)
    }
}

impl Display for PathBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        Display::fmt(&self.inner, f)
    }
}

impl core::ops::Deref for PathBuf {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.inner
    }
}

impl core::ops::DerefMut for PathBuf {
    fn deref_mut(&mut self) -> &mut Path {
        &mut self.inner
    }
}

impl AsRef<Path> for PathBuf {
    fn as_ref(&self) -> &Path {
        &self.inner
    }
}

impl AsRef<str> for PathBuf {
    fn as_ref(&self) -> &str {
        self.inner.as_str()
    }
}

impl From<&str> for Path {
    fn from(s: &str) -> Self {
        Path::new(s)
    }
}

impl From<String> for Path {
    fn from(s: String) -> Self {
        Path { inner: s }
    }
}

impl From<PathBuf> for Path {
    fn from(buf: PathBuf) -> Self {
        buf.inner
    }
}

impl From<&str> for PathBuf {
    fn from(s: &str) -> Self {
        PathBuf {
            inner: Path::new(s),
        }
    }
}

impl From<String> for PathBuf {
    fn from(s: String) -> Self {
        PathBuf {
            inner: Path { inner: s },
        }
    }
}

impl From<&Path> for PathBuf {
    fn from(path: &Path) -> Self {
        PathBuf {
            inner: path.clone(),
        }
    }
}

impl From<Path> for PathBuf {
    fn from(path: Path) -> Self {
        PathBuf { inner: path }
    }
}

impl core::str::FromStr for PathBuf {
    type Err = core::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PathBuf::from(s))
    }
}

impl PartialEq<str> for PathBuf {
    fn eq(&self, other: &str) -> bool {
        self.inner.inner == other
    }
}

impl PartialEq<&str> for PathBuf {
    fn eq(&self, other: &&str) -> bool {
        self.inner.inner == *other
    }
}

impl PartialEq<String> for PathBuf {
    fn eq(&self, other: &String) -> bool {
        self.inner.inner == *other
    }
}

/// 路径的一个分量（仿 `std::path::Component`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Component<'a> {
    /// 根目录 `/`。
    RootDir,
    /// 当前目录 `.`。
    CurDir,
    /// 父目录 `..`。
    ParentDir,
    /// 普通分量。
    Normal(&'a str),
}

impl Component<'_> {
    /// 分量对应的字符串表示（根目录为 `""`）。
    pub fn as_str(&self) -> &str {
        match self {
            Component::RootDir => "",
            Component::CurDir => ".",
            Component::ParentDir => "..",
            Component::Normal(s) => s,
        }
    }
}

/// [`Path::components`] 返回的路径分量迭代器。
#[derive(Clone)]
pub struct Components<'a> {
    parts: core::str::Split<'a, &'a str>,
    is_absolute: bool,
    seen_root: bool,
}

impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_absolute && !self.seen_root {
            self.seen_root = true;
            return Some(Component::RootDir);
        }
        let part = self.parts.next()?;
        match part {
            "" => self.next(), // 连续分隔符 / 前导、尾随分隔符
            "." => Some(Component::CurDir),
            ".." => Some(Component::ParentDir),
            s => Some(Component::Normal(s)),
        }
    }
}
