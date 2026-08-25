#![no_std]
//! 通用前缀树 [`Trie`]。
//!
//! 以元素序列（`&[K]`）为键的有序前缀树（trie）：键逐元素沿树下行，
//! 在各级节点间共享存储，值挂在键路径终点的节点上；空键 `&[]` 对应
//! 根节点自身的值。子节点按 `K` 的 [`Ord`] 序（B 树）排列，因此
//! [`iter`](Trie::iter) 以字典序产出条目。
//!
//! 键不需要定长，也不需要在别处完整保存；`K = u8` 时即可直接用字节串
//! （路径名、协议串等）作为键，典型用途包括路径路由表、最长前缀匹配
//! 与字符串表。
//!
//! # 示例
//!
//! ```
//! use athera_trie::Trie;
//!
//! let mut t = Trie::new();
//! t.insert(b"/proc", 1);
//! t.insert(b"/proc/self", 2);
//!
//! assert_eq!(t.get(b"/proc"), Some(&1));
//!
//! // 最长前缀匹配：返回查询键切片中最长的已存前缀及其值。
//! assert_eq!(
//!     t.longest_prefix(b"/proc/self/cmdline"),
//!     Some((&b"/proc/self"[..], &2))
//! );
//!
//! // 按字典序遍历某前缀下的所有条目。
//! let keys: Vec<Vec<u8>> = t.iter_prefix(b"/proc").map(|(k, _)| k).collect();
//! assert_eq!(keys, vec![b"/proc".to_vec(), b"/proc/self".to_vec()]);
//!
//! assert_eq!(t.remove(b"/proc"), Some(1));   // 后缀条目不受影响
//! assert_eq!(t.get(b"/proc/self"), Some(&2));
//! ```
//!
//! # 复杂度
//!
//! 设 `m` 为键长度、`σ` 为单节点子节点数（B 树有序表）：
//!
//! | 操作 | 复杂度 |
//! |------|--------|
//! | [`insert`](Trie::insert) / [`get`](Trie::get) / [`remove`](Trie::remove) | `O(m log σ)` |
//! | [`longest_prefix`](Trie::longest_prefix) / [`contains_prefix`](Trie::contains_prefix) | `O(m log σ)` |
//!
//! [`remove`](Trie::remove) 会顺手剪掉因此变空的分支；迭代器逐条目
//! 把完整键物化成 `Vec<K>`，值始终以引用返回。

extern crate alloc;

use alloc::{
    boxed::Box,
    collections::{BTreeMap, btree_map},
    vec,
    vec::Vec,
};
use core::{cmp::Ord, fmt};

// ---------------------------------------------------------------------------
// 内部节点
// ---------------------------------------------------------------------------

/// 前缀树内部节点：可选值 + 按 `K` 排序的子节点表。
#[derive(Clone, PartialEq, Eq)]
struct Node<K, V> {
    children: BTreeMap<K, Box<Node<K, V>>>,
    value: Option<V>,
}

impl<K, V> Node<K, V> {
    /// 是否既无值也无子节点（可被 [`Trie::remove`] 剪枝）。
    fn is_prunable(&self) -> bool {
        self.value.is_none() && self.children.is_empty()
    }

    /// 以 `node` 为根的子树内存储的条目数（含 `node` 自身的值）。
    fn count(node: &Self) -> usize {
        let mut n = usize::from(node.value.is_some());
        for child in node.children.values() {
            n += Self::count(child);
        }
        n
    }

    /// 以 `node` 为根的子树内是否存在任何条目。
    fn has_entry(node: &Self) -> bool {
        node.value.is_some() || node.children.values().any(|c| Self::has_entry(c))
    }
}

impl<K, V> Default for Node<K, V> {
    fn default() -> Self {
        Self {
            children: BTreeMap::new(),
            value: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 前缀树
// ---------------------------------------------------------------------------

/// 以元素序列（`&[K]`）为键的前缀树。
///
/// 见[模块文档](self)的示例与复杂度说明。插入相同键会替换旧值并
/// 返回之（[`insert`](Self::insert)）。
#[derive(Clone, PartialEq, Eq)]
pub struct Trie<K, V> {
    root: Node<K, V>,
    len: usize,
}

impl<K, V> Default for Trie<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> Trie<K, V> {
    /// 创建空前缀树。
    pub const fn new() -> Self {
        Self {
            root: Node {
                children: BTreeMap::new(),
                value: None,
            },
            len: 0,
        }
    }

    /// 已存储的条目数。
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否没有任何条目。
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 丢弃所有条目，回到空树状态。
    pub fn clear(&mut self) {
        self.root.children.clear();
        self.root.value = None;
        self.len = 0;
    }
}

impl<K: Ord, V> Trie<K, V> {
    /// 查找 `key` 对应的值。
    #[must_use]
    pub fn get(&self, key: &[K]) -> Option<&V> {
        self.node_of(key).and_then(|n| n.value.as_ref())
    }

    /// 查找 `key` 对应的值（可变引用）。
    pub fn get_mut(&mut self, key: &[K]) -> Option<&mut V> {
        self.node_of_mut(key).and_then(|n| n.value.as_mut())
    }

    /// 是否存在键 `key`。
    #[must_use]
    pub fn contains_key(&self, key: &[K]) -> bool {
        self.get(key).is_some()
    }

    /// 移除并返回 `key` 对应的值；键不存在时返回 `None`。
    ///
    /// 随之变空的中间节点会被剪掉，只保留仍承载值或子树的前缀；
    /// 键是其他已存键前缀的条目被移除时，后缀条目不受影响。
    pub fn remove(&mut self, key: &[K]) -> Option<V> {
        let old = Self::remove_at(&mut self.root, key);
        if old.is_some() {
            self.len -= 1;
        }
        old
    }

    /// 在 `node` 下按 `key` 递归移除，返回被移除的值（未找到返回
    /// `None`，且不做任何修改）。
    fn remove_at(node: &mut Node<K, V>, key: &[K]) -> Option<V> {
        let Some((first, rest)) = key.split_first() else {
            return node.value.take();
        };
        let old = Self::remove_at(node.children.get_mut(first)?, rest)?;
        if node.children.get(first).is_some_and(|c| c.is_prunable()) {
            node.children.remove(first);
        }
        Some(old)
    }

    /// 在已存键中找出 `key` 的（真或假）前缀里最长的一个，返回
    /// `(&key[..depth], &value)`——键切片借用查询键本身，零拷贝。
    ///
    /// 空键若作为条目存在，会匹配任何查询（深度 0）。
    #[must_use]
    pub fn longest_prefix<'k>(&self, key: &'k [K]) -> Option<(&'k [K], &V)> {
        let mut node = &self.root;
        let mut best: Option<(usize, &V)> = node.value.as_ref().map(|v| (0, v));
        for (i, k) in key.iter().enumerate() {
            let Some(child) = node.children.get(k) else {
                break;
            };
            node = child;
            if let Some(v) = node.value.as_ref() {
                best = Some((i + 1, v));
            }
        }
        best.map(|(depth, v)| (&key[..depth], v))
    }

    /// 是否存在以 `prefix` 开头的条目（`prefix` 本身不必是已存键）。
    #[must_use]
    pub fn contains_prefix(&self, prefix: &[K]) -> bool {
        self.node_of(prefix).is_some_and(Node::has_entry)
    }

    /// 沿 `key` 下行到对应节点。
    fn node_of(&self, key: &[K]) -> Option<&Node<K, V>> {
        let mut node = &self.root;
        for k in key {
            node = node.children.get(k)?;
        }
        Some(node)
    }

    /// 沿 `key` 下行到对应节点（可变）。
    fn node_of_mut(&mut self, key: &[K]) -> Option<&mut Node<K, V>> {
        let mut node = &mut self.root;
        for k in key {
            node = node.children.get_mut(k)?;
        }
        Some(node)
    }
}

impl<K: Ord + Clone, V> Trie<K, V> {
    /// 插入 `key -> value`；键已存在时替换旧值并返回之。
    pub fn insert(&mut self, key: &[K], value: V) -> Option<V> {
        let mut node = &mut self.root;
        for k in key {
            node = &mut *node.children.entry(k.clone()).or_default();
        }
        let old = node.value.replace(value);
        if old.is_none() {
            self.len += 1;
        }
        old
    }

    /// 按字典序遍历所有条目，产出 `(完整键, &值)`。
    ///
    /// 完整键在树中逐级分片存储，因此每个条目的键在产出时才物化为
    /// `Vec<K>`；值始终以引用返回。迭代器是携带路径缓冲的深度优先
    /// 遍历，`len`（[`size_hint`](Iter::size_hint)）精确。
    #[must_use]
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter::new(&self.root, Vec::new(), self.len)
    }

    /// 按字典序遍历所有以 `prefix` 开头的条目。
    ///
    /// `prefix` 对应的路径不存在时产出为空。等价于
    /// `iter().filter(|(k, _)| k.starts_with(prefix))`，但只走前缀
    /// 子树。
    #[must_use]
    pub fn iter_prefix(&self, prefix: &[K]) -> Iter<'_, K, V> {
        match self.node_of(prefix) {
            Some(node) => Iter::new(node, prefix.to_vec(), Node::count(node)),
            None => Iter::empty(),
        }
    }
}

impl<K, V> fmt::Debug for Trie<K, V>
where
    K: Ord + Clone + fmt::Debug,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

// ---------------------------------------------------------------------------
// 迭代器
// ---------------------------------------------------------------------------

/// 条目迭代器，由 [`Trie::iter`] / [`Trie::iter_prefix`] 产生。
///
/// 以字典序（键逐元素按 `K` 的 [`Ord`] 序比较）产出 `(Vec<K>, &V)`。
pub struct Iter<'a, K, V> {
    /// 深度优先遍历的节点栈（每帧记录自身值是否已产出、子节点迭代进度）。
    frames: Vec<Frame<'a, K, V>>,
    /// 当前栈顶节点的完整键（起始前缀 + 逐级 clone 的键元素）。
    path: Vec<K>,
    /// 尚未产出的条目数（精确）。
    remaining: usize,
}

/// 迭代栈帧。
struct Frame<'a, K, V> {
    node: &'a Node<K, V>,
    children: btree_map::Iter<'a, K, Box<Node<K, V>>>,
    emitted: bool,
}

impl<'a, K, V> Iter<'a, K, V> {
    /// 从 `root`（初始路径 `path`）开始遍历，共 `remaining` 个条目。
    fn new(root: &'a Node<K, V>, path: Vec<K>, remaining: usize) -> Self {
        let frames = vec![Frame {
            node: root,
            children: root.children.iter(),
            emitted: false,
        }];
        Self {
            frames,
            path,
            remaining,
        }
    }

    /// 空迭代器。
    fn empty() -> Self {
        Self {
            frames: Vec::new(),
            path: Vec::new(),
            remaining: 0,
        }
    }
}

impl<'a, K: Clone, V> Iterator for Iter<'a, K, V> {
    type Item = (Vec<K>, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let frame = self.frames.last_mut()?;

            // 先输出栈顶节点自身的值（若尚未输出）。
            if !frame.emitted {
                frame.emitted = true;
                let node = frame.node;
                if let Some(v) = node.value.as_ref() {
                    self.remaining -= 1;
                    return Some((self.path.clone(), v));
                }
            }

            // 再深入下一个子节点，或回溯到父节点。
            if let Some((k, child)) = frame.children.next() {
                let node: &'a Node<K, V> = child;
                self.path.push(k.clone());
                self.frames.push(Frame {
                    node,
                    children: node.children.iter(),
                    emitted: false,
                });
            } else {
                self.frames.pop();
                self.path.pop();
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a, K: Clone, V> ExactSizeIterator for Iter<'a, K, V> {
    fn len(&self) -> usize {
        self.remaining
    }
}
