//! 可执行格式（binfmt）路由。
//!
//! 魔数注册表 [`BINFMTS`] 用前缀树把文件头魔数映射到处理器：路由时
//! 以文件前 128 字节查询 [`Trie::longest_prefix`]，命中最长的已注册
//! 魔数（更具体的魔数优先）。内置注册 ELF（`\x7fELF`）与 shebang
//! （`#!`）两种格式；其他模块可通过 [`register`] / [`unregister`]
//! 在运行时增删。

use alloc::{string::String, vec::Vec};

use athera_trie::Trie;

use crate::{
    fs::{
        Mode, Path, VFS,
        fs_error::FsError,
        vfs::{FileSystem, OpenFlags},
    },
    sync::{lazy::LazyLock, spin::SpinLock},
    task::{Tid, exec::kernel_execve, process::execve_into},
};

pub mod elf;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("no access")]
    NoAccess,
    #[error(transparent)]
    FsError(FsError),
    #[error("unsupported binfmt")]
    UnsupportedBinfmt,
    #[error("exec failed")]
    ExecFailed,
}

/// 魔数处理器。
///
/// `head` 是文件头读出的字节（含匹配到的魔数及其后内容），因此
/// `&head[magic.len()..]` 可用于提取魔数后的附加信息（如 shebang 的
/// 解释器行）；`path` / `argv` / `envp` 为本次 exec 的参数；`tid` 为
/// `None` 时创建新任务执行，为 `Some(tid)` 时把程序加载进该既有任务
/// （替换其 `memory_set` / `trap_context`，execve 语义）。
pub type Handler = fn(
    head: &[u8],
    path: &str,
    argv: &[&str],
    envp: &[&str],
    tid: Option<Tid>,
) -> Result<(), Error>;

/// 魔数注册表：魔数字节序列 -> 处理器。
///
/// 首次访问时注册内置格式；`SpinLock` 保护运行期增删。
static BINFMTS: LazyLock<SpinLock<Trie<u8, Handler>>> = LazyLock::new(|| {
    let mut trie: Trie<u8, Handler> = Trie::new();
    trie.insert(b"\x7fELF", exec_elf);
    trie.insert(b"#!", exec_shebang);
    SpinLock::new(trie)
});

/// 注册（或替换）魔数处理器。
///
/// 同一魔数重复注册时替换旧处理器并返回 `false`；空魔数会被拒绝
/// （返回 `false`，因为它会匹配一切文件）。
pub fn register(magic: &[u8], handler: Handler) -> bool {
    if magic.is_empty() {
        return false;
    }
    BINFMTS.force().lock().insert(magic, handler).is_none()
}

/// 注销魔数，返回其处理器；未注册时返回 `None`。
pub fn unregister(magic: &[u8]) -> Option<Handler> {
    BINFMTS.force().lock().remove(magic)
}

/// 按魔数路由执行 `path`：创建新任务加载运行命中的可执行格式。
pub fn route(path: &str, argv: &[&str], envp: &[&str]) -> Result<(), Error> {
    route_inner(None, path, argv, envp)
}

/// 与 [`route`] 相同的魔数路由，但把程序加载进 `tid` 指定的既有任务：
/// 替换其 `memory_set` / `trap_context` 并标记可运行（execve 语义），
/// 而不是创建新任务。目标不存在或加载失败时返回 [`Error`]。
pub fn route_at(tid: Tid, path: &str, argv: &[&str], envp: &[&str]) -> Result<(), Error> {
    route_inner(Some(tid), path, argv, envp)
}

fn route_inner(tid: Option<Tid>, path: &str, argv: &[&str], envp: &[&str]) -> Result<(), Error> {
    let file = VFS
        .force()
        .open(&Path::new(path), OpenFlags::read_only(), Mode::new())
        .map_err(Error::FsError)?;

    if !file.dentry.inode.mode.user_execute() {
        return Err(Error::NoAccess);
    }

    let mut head = [0; 128];

    let n = file.read(&mut head).map_err(Error::FsError)?;

    // 先在块作用域内查表并释放锁，再调用处理器：处理器（如 shebang）
    // 可能重入 binfmt_router 再次抢锁。
    let handler = {
        let trie = BINFMTS.force().lock();
        match trie.longest_prefix(&head[..n]) {
            // Handler 是 fn 指针（Copy），拷贝出来以免借用锁守卫。
            Some((_, handler)) => *handler,
            None => return Err(Error::UnsupportedBinfmt),
        }
    };

    handler(&head[..n], path, argv, envp, tid)
}

/// ELF 处理器：直接按 ELF 加载执行。
fn exec_elf(
    _head: &[u8],
    path: &str,
    argv: &[&str],
    envp: &[&str],
    tid: Option<Tid>,
) -> Result<(), Error> {
    match tid {
        Some(tid) => execve_into(tid, path, argv, envp).ok_or(Error::ExecFailed),
        None => {
            kernel_execve(path, argv, envp);
            Ok(())
        }
    }
}

/// shebang 处理器：解析 `#!` 后的解释器行，重组 argv 后重新路由。
fn exec_shebang(
    head: &[u8],
    path: &str,
    argv: &[&str],
    envp: &[&str],
    tid: Option<Tid>,
) -> Result<(), Error> {
    let raw_path = &head[b"#!".len()..];
    let raw_path = &raw_path[..raw_path
        .iter()
        .position(|&c| c == b'\n' || c == b'\r')
        .unwrap_or(raw_path.len())];

    let shebang_args = shebang_split(raw_path);

    let mut new_argv: Vec<String> = Vec::with_capacity(shebang_args.len() + argv.len());
    new_argv.extend(shebang_args);
    new_argv.push(String::from(path));
    new_argv.extend(argv.iter().skip(1).map(|s| String::from(*s)));

    let argv_refs: Vec<&str> = new_argv.iter().map(String::as_str).collect();

    // shebang_args 之后总会 push 原 path，故 new_argv 非空。
    let Some(interpreter) = new_argv.first() else {
        return Err(Error::ExecFailed);
    };

    route_inner(tid, interpreter.as_str(), argv_refs.as_slice(), envp)
}

/// 把 `#!/usr/bin/env foo -bar "baz qux"` 形式的 shebang 续行切成参数列表。
///
/// 实现 POSIX shell 的最小子集，足以覆盖常见 shebang：
///
/// - 空白（空格 / 制表符）分隔参数，引号内或被转义的空白除外；
/// - 单引号 `'…'` 内任何字符原样保留，不解释反斜杠；
/// - 双引号 `"…"` 内处理反斜杠转义，其他字符原样保留；
/// - 在引号外，`\` 转义任意下一字符（行尾反斜杠忽略）；
/// - 行尾 `\n` / `\r` 终止解析，尾部内容被忽略；
/// - 非 UTF-8 字节片段会被替换为 `U+FFFD` 替换字符，避免整个 token 被丢；
/// - 中途遇到解析错误（未配对的多字节 UTF-8）时，已收齐的 token 仍会返回。
fn shebang_split(args: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut token_started = false;

    let flush = |buf: &mut Vec<u8>, out: &mut Vec<String>, token_started: &mut bool| {
        if !*token_started {
            return;
        }
        // 把已收集的字节按 UTF-8 解码，非法序列替换为 U+FFFD。
        let s = String::from_utf8_lossy(buf).into_owned();
        if !s.is_empty() {
            out.push(s);
        }
        buf.clear();
        *token_started = false;
    };

    let mut i = 0;
    while i < args.len() {
        let ch = args[i];

        if ch == b'\n' || ch == b'\r' {
            break;
        }

        if !in_single && !in_double {
            match ch {
                b' ' | b'\t' => {
                    flush(&mut buf, &mut out, &mut token_started);
                    i += 1;
                    continue;
                }
                b'\'' => {
                    in_single = true;
                    token_started = true;
                    i += 1;
                    continue;
                }
                b'"' => {
                    in_double = true;
                    token_started = true;
                    i += 1;
                    continue;
                }
                b'\\' => {
                    if i + 1 < args.len() && args[i + 1] != b'\n' {
                        buf.push(args[i + 1]);
                        token_started = true;
                        i += 2;
                        continue;
                    }
                    // 行尾反斜杠：原样忽略
                    i += 1;
                    continue;
                }
                _ => {
                    buf.push(ch);
                    token_started = true;
                    i += 1;
                }
            }
        } else if in_single {
            if ch == b'\'' {
                in_single = false;
                i += 1;
                continue;
            }
            buf.push(ch);
            i += 1;
        } else {
            // in_double
            match ch {
                b'"' => {
                    in_double = false;
                    i += 1;
                    continue;
                }
                b'\\' => {
                    if i + 1 < args.len() && args[i + 1] != b'\n' {
                        buf.push(args[i + 1]);
                        i += 2;
                        continue;
                    }
                    i += 1;
                    continue;
                }
                _ => {
                    buf.push(ch);
                    i += 1;
                }
            }
        }
    }

    flush(&mut buf, &mut out, &mut token_started);
    out
}
