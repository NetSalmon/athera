#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把文件按 src/fs/record.rs 的格式写入虚拟磁盘镜像。

默认读写 qcow2 格式（可直接用于 start.sh 的 resources/disk.qcow2）；
对已有镜像自动识别格式，也可用 --format raw 改用原始镜像。

磁盘格式（与 src/fs/record.rs 对应）：
  * 块大小固定 512 字节，块 0 是第一个索引块（Index）。
  * 每个索引块 512 字节：
        - 21 个 File 条目（每个 24 字节），按序排列；
        - 末尾 8 字节 next_index（u64，小端）指向下一个索引块，0 表示没有。
  * File 条目（24 字节）：
        - start:   u32 小端，文件数据起始块号；
        - size:    u32 小端，文件字节数；
        - file_name: 定长 16 字节，UTF-8，未写满处以 0 填充（最长 15 字节）。

qcow2 由脚本内置的纯 Python 读写器生成/解析（v3、64K 簇、16 位引用计数、
无压缩/快照/加密），不需要 qemu-img。
"""
import argparse
import math
import os
import shutil
import struct
import sys
import tempfile

BLOCK_SIZE = 512
FILES_PER_INDEX = 21          # Index.files 长度
ENTRY_SIZE = 24               # size_of::<File>()
NAME_BYTES = 16               # RecordString 定长
MAX_NAME_BYTES = NAME_BYTES - 1  # record.rs: assert!(bytes.len() < 16)
NEXT_INDEX_OFFSET = FILES_PER_INDEX * ENTRY_SIZE  # 504

# ---------------- qcow2 读写 ----------------

QCOW2_MAGIC = b"QFI\xfb"
QCOW2_OFFSET_MASK = (1 << 56) - (1 << 9)   # 条目中的簇偏移字段：bit 9..55
QCOW2_COPIED = 1 << 63
QCOW2_COMPRESSED = 1 << 62
QCOW2_ZERO = 1 << 0


def read_exact_pad(f, n):
    data = f.read(n)
    if len(data) < n:
        data += b"\x00" * (n - len(data))
    return data


def detect_format(path):
    with open(path, "rb") as f:
        return "qcow2" if f.read(4) == QCOW2_MAGIC else "raw"


def qcow2_read_header(f):
    hdr = read_exact_pad(f, 112)
    if hdr[:4] != QCOW2_MAGIC:
        raise ValueError("不是 qcow2 镜像")
    version = struct.unpack_from(">I", hdr, 4)[0]
    if version not in (2, 3):
        raise ValueError(f"不支持的 qcow2 版本: {version}")
    backing_off = struct.unpack_from(">Q", hdr, 8)[0]
    cluster_bits = struct.unpack_from(">I", hdr, 20)[0]
    virtual_size = struct.unpack_from(">Q", hdr, 24)[0]
    crypt = struct.unpack_from(">I", hdr, 32)[0]
    l1_size = struct.unpack_from(">I", hdr, 36)[0]
    l1_off = struct.unpack_from(">Q", hdr, 40)[0]
    n_snap = struct.unpack_from(">I", hdr, 60)[0]
    snap_off = struct.unpack_from(">Q", hdr, 64)[0]
    incompatible = struct.unpack_from(">Q", hdr, 72)[0] if version >= 3 else 0
    return dict(version=version, cluster_bits=cluster_bits, virtual_size=virtual_size,
                l1_size=l1_size, l1_off=l1_off, backing_off=backing_off, crypt=crypt,
                n_snap=n_snap, snap_off=snap_off, incompatible=incompatible)


def qcow2_to_raw(qcow_path, raw_path):
    """把 qcow2 解码成原始镜像（稀疏文件），返回虚拟大小。"""
    with open(qcow_path, "rb") as f:
        h = qcow2_read_header(f)
        if h["backing_off"] or h["crypt"] or h["n_snap"] or h["snap_off"] or h["incompatible"]:
            raise ValueError("qcow2 含 backing file / 加密 / 快照 / 未知特性，暂不支持")
        cluster_size = 1 << h["cluster_bits"]
        l2_entries = cluster_size // 8
        with open(raw_path, "wb") as out:
            out.truncate(h["virtual_size"])
            f.seek(h["l1_off"])
            l1 = read_exact_pad(f, h["l1_size"] * 8)
            for i in range(h["l1_size"]):
                e = struct.unpack_from(">Q", l1, i * 8)[0]
                if e == 0:
                    continue
                f.seek(e & QCOW2_OFFSET_MASK)
                l2 = read_exact_pad(f, cluster_size)
                for j in range(l2_entries):
                    pos = (i * l2_entries + j) * cluster_size
                    if pos >= h["virtual_size"]:
                        break
                    le = struct.unpack_from(">Q", l2, j * 8)[0]
                    if le == 0:
                        continue
                    if le & QCOW2_COMPRESSED:
                        raise ValueError("qcow2 压缩簇暂不支持读取")
                    if le & QCOW2_ZERO:
                        continue
                    data_off = le & QCOW2_OFFSET_MASK
                    if data_off == 0:
                        continue
                    f.seek(data_off)
                    chunk = read_exact_pad(f, cluster_size)
                    out.seek(pos)
                    out.write(chunk[:h["virtual_size"] - pos])
    return h["virtual_size"]


def raw_to_qcow2(raw_path, qcow_path, virtual_size, cluster_bits=16, refcount_order=4):
    """从原始镜像生成 qcow2（仅分配非零簇，稀疏）。返回文件字节数。"""
    cluster_size = 1 << cluster_bits
    l2_entries = cluster_size // 8
    refcount_entry_bytes = (1 << refcount_order) // 8
    clusters_per_refblock = cluster_size // refcount_entry_bytes
    total_clusters = math.ceil(virtual_size / cluster_size)
    l1_size = math.ceil(total_clusters / l2_entries)
    if l1_size * 8 > cluster_size:
        raise ValueError("L1 表超过一个簇（虚拟磁盘过大，暂不支持）")
    n_refblocks = math.ceil(total_clusters / clusters_per_refblock)
    rt_clusters = math.ceil(n_refblocks / l2_entries)
    if rt_clusters > 1:
        raise ValueError("refcount 表超过一个簇（虚拟磁盘过大，暂不支持）")

    with open(raw_path, "rb") as r:
        data_clusters = [c for c in range(total_clusters)
                         if any(read_exact_pad(r, cluster_size))]

    l2_indexes = sorted({c // l2_entries for c in data_clusters})

    # 顺序分配簇：header(0), L1(1), refcount table, refcount blocks, L2 tables, data
    nxt = 1  # cluster 0 留给 header
    l1_id = nxt; nxt += 1
    rt_ids = list(range(nxt, nxt + rt_clusters)); nxt += rt_clusters
    rb_ids = list(range(nxt, nxt + n_refblocks)); nxt += n_refblocks
    l2_ids = list(range(nxt, nxt + len(l2_indexes))); nxt += len(l2_indexes)
    data_ids = list(range(nxt, nxt + len(data_clusters))); nxt += len(data_clusters)
    allocated = set(range(nxt))
    data_id_map = dict(zip(data_clusters, data_ids))
    l2_id_map = dict(zip(l2_indexes, l2_ids))

    with open(qcow_path, "wb") as q:
        hdr = bytearray(cluster_size)
        hdr[0:4] = QCOW2_MAGIC
        struct.pack_into(">I", hdr, 4, 3)              # version
        struct.pack_into(">Q", hdr, 8, 0)              # backing_file_offset
        struct.pack_into(">I", hdr, 16, 0)             # backing_file_size
        struct.pack_into(">I", hdr, 20, cluster_bits)
        struct.pack_into(">Q", hdr, 24, virtual_size)
        struct.pack_into(">I", hdr, 32, 0)             # crypt_method
        struct.pack_into(">I", hdr, 36, l1_size)
        struct.pack_into(">Q", hdr, 40, l1_id * cluster_size)
        struct.pack_into(">Q", hdr, 48, rt_ids[0] * cluster_size)
        struct.pack_into(">I", hdr, 56, rt_clusters)
        struct.pack_into(">I", hdr, 60, 0)             # nb_snapshots
        struct.pack_into(">Q", hdr, 64, 0)             # snapshots_offset
        struct.pack_into(">Q", hdr, 72, 0)             # incompatible_features
        struct.pack_into(">Q", hdr, 80, 0)             # compatible_features
        struct.pack_into(">Q", hdr, 88, 0)             # autoclear_features
        struct.pack_into(">I", hdr, 96, refcount_order)
        struct.pack_into(">I", hdr, 100, 112)          # header_length
        struct.pack_into(">I", hdr, 104, 0)            # compression_type
        q.write(hdr)

        l1 = bytearray(cluster_size)
        for k, l2i in enumerate(l2_indexes):
            struct.pack_into(">Q", l1, k * 8, (l2_id_map[l2i] * cluster_size) | QCOW2_COPIED)
        q.write(l1)

        rt = bytearray(cluster_size)
        for k, rb in enumerate(rb_ids):
            struct.pack_into(">Q", rt, k * 8, rb * cluster_size)
        q.write(rt)

        for b in range(n_refblocks):
            blk = bytearray(cluster_size)
            base = b * clusters_per_refblock
            for e in range(clusters_per_refblock):
                if base + e in allocated:
                    struct.pack_into(">H", blk, e * refcount_entry_bytes, 1)
            q.write(blk)

        for l2i in l2_indexes:
            l2 = bytearray(cluster_size)
            for j in range(l2_entries):
                c = l2i * l2_entries + j
                if c in data_id_map:
                    struct.pack_into(">Q", l2, j * 8, (data_id_map[c] * cluster_size) | QCOW2_COPIED)
            q.write(l2)

        with open(raw_path, "rb") as r:
            for c in data_clusters:
                r.seek(c * cluster_size)
                q.write(read_exact_pad(r, cluster_size))

    return len(allocated) * cluster_size


def make_temp(suffix):
    fd, path = tempfile.mkstemp(prefix="mkdisk-", suffix=suffix)
    os.close(fd)
    return path


def atomic_replace(src, dst):
    try:
        os.replace(src, dst)
    except OSError:
        shutil.copyfile(src, dst)
        os.unlink(src)


# ---------------- record.rs 索引读写 ----------------

class NeedGrow(Exception):
    pass


def parse_size(s: str) -> int:
    s = s.strip().upper()
    mult = 1
    if s.endswith("G"):
        mult = 1024 ** 3
        s = s[:-1]
    elif s.endswith("M"):
        mult = 1024 ** 2
        s = s[:-1]
    elif s.endswith("K"):
        mult = 1024
        s = s[:-1]
    return int(s) * mult


def encode_name(name: str) -> bytes:
    raw = name.encode("utf-8")
    if len(raw) > MAX_NAME_BYTES:
        raise ValueError(
            f"file name {name!r} is {len(raw)} bytes (> {MAX_NAME_BYTES}), "
            f"RecordString only fits {MAX_NAME_BYTES} bytes plus NUL"
        )
    return raw + b"\x00" * (NAME_BYTES - len(raw))


def pack_index_block(entries) -> bytearray:
    """entries: list of (name, start, size); next_index 由调用方写入。"""
    buf = bytearray(BLOCK_SIZE)
    for i, (name, start, size) in enumerate(entries):
        if i >= FILES_PER_INDEX:
            raise ValueError("too many entries for one index block")
        off = i * ENTRY_SIZE
        struct.pack_into("<II", buf, off, start, size)
        buf[off + 8: off + 8 + NAME_BYTES] = encode_name(name)
    return buf


def unpack_file_entry(block, off):
    start, size = struct.unpack_from("<II", block, off)
    raw = block[off + 8: off + 8 + NAME_BYTES]
    name = raw.split(b"\x00", 1)[0].decode("utf-8", "replace")
    return name, start, size


def unpack_index_block(block):
    files = [unpack_file_entry(block, i * ENTRY_SIZE) for i in range(FILES_PER_INDEX)]
    next_index = struct.unpack_from("<Q", block, NEXT_INDEX_OFFSET)[0]
    return files, next_index


def read_block(f, blk_id):
    f.seek(blk_id * BLOCK_SIZE)
    data = f.read(BLOCK_SIZE)
    if len(data) < BLOCK_SIZE:
        data += b"\x00" * (BLOCK_SIZE - len(data))
    return data


def read_chain(f):
    """沿 next_index 链读取全部索引块，返回 (entries, index_blocks)。"""
    total_blocks = file_size(f) // BLOCK_SIZE
    entries = []
    index_blocks = []
    seen = set()
    cur = 0
    for _ in range(1 << 16):
        if cur >= total_blocks:
            raise ValueError(f"index block {cur} 超出镜像大小，镜像可能不是 Athera 磁盘格式")
        if cur in seen:
            raise ValueError(f"index chain cycle at block {cur}")
        seen.add(cur)
        index_blocks.append(cur)
        files, next_index = unpack_index_block(read_block(f, cur))
        for n, s, z in files:
            if s == 0 and z == 0:
                continue
            if s >= total_blocks:
                raise ValueError(
                    f"索引条目 {n!r} 的起始块 {s} 超出镜像大小，镜像可能不是 Athera 磁盘格式"
                )
            entries.append((n, s, z))
        if next_index == 0:
            break
        if next_index >= total_blocks:
            raise ValueError(f"next_index={next_index} 超出镜像大小，镜像可能不是 Athera 磁盘格式")
        cur = next_index
    else:
        raise ValueError("index chain too long")
    return entries, index_blocks


def compute_used(entries, index_blocks):
    used = set(index_blocks)
    for _, start, size in entries:
        n = (size + BLOCK_SIZE - 1) // BLOCK_SIZE
        used.update(range(start, start + n))
    return used


def alloc_run(used, n, total_blocks):
    """first-fit：找一段连续的 n 个空闲块，返回起始块号或 None。"""
    start = 0
    while start + n <= total_blocks:
        ok = True
        for b in range(start, start + n):
            if b in used:
                ok = False
                start = b + 1
                break
        if ok:
            return start
    return None


def file_size(f):
    return os.fstat(f.fileno()).st_size


def cmd_list(image):
    if not os.path.isfile(image):
        print(f"error: no such image: {image}", file=sys.stderr)
        return 1
    tmp = None
    try:
        if detect_format(image) == "qcow2":
            tmp = make_temp(".raw")
            qcow2_to_raw(image, tmp)
            raw_path = tmp
        else:
            raw_path = image
        with open(raw_path, "rb") as f:
            if file_size(f) < BLOCK_SIZE:
                print("空索引（镜像不足一个块）")
                return 0
            entries, index_blocks = read_chain(f)
    except ValueError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    finally:
        if tmp and os.path.exists(tmp):
            os.unlink(tmp)
    print(f"index blocks ({len(index_blocks)}): " + ", ".join(map(str, index_blocks)))
    print(f"files ({len(entries)}):")
    for name, start, size in entries:
        n = max(1, (size + BLOCK_SIZE - 1) // BLOCK_SIZE)
        print(f"  {name:<16} start={start:<8} size={size:<12} blocks=[{start}..{start + n - 1}]")
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="把文件按 src/fs/record.rs 的格式写入虚拟磁盘镜像（默认 qcow2，块 0 为索引块）。",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""示例:
  python3 mkdisk.py resources/disk.qcow2 hello.txt a.out    # 默认 qcow2：重建索引并写入
  python3 mkdisk.py --append resources/disk.qcow2 more.txt  # 保留原有文件，追加（同名视为替换）
  python3 mkdisk.py --list resources/disk.qcow2             # 查看当前索引
  python3 mkdisk.py --format raw disk.img hello.txt         # 改用原始 raw 镜像
""",
    )
    ap.add_argument("image", help="虚拟磁盘镜像路径（不存在则创建，默认 qcow2）")
    ap.add_argument("files", nargs="*", help="要写入镜像的文件")
    ap.add_argument("--append", action="store_true", help="在已有索引上追加，而不是重建")
    ap.add_argument("--list", action="store_true", help="打印镜像当前索引并退出")
    ap.add_argument("--size", default="128M", help="新建镜像的虚拟大小（默认 128M；必要时自动扩容）")
    ap.add_argument("--format", choices=["auto", "raw", "qcow2"], default="auto",
                    help="镜像格式（默认 auto：已有镜像自动识别，新镜像默认 qcow2）")
    args = ap.parse_args(argv)

    if args.list:
        return cmd_list(args.image)

    if not args.files:
        ap.error("没有指定要写入的文件")

    image = args.image
    exists = os.path.exists(image)
    if exists and os.path.isdir(image):
        ap.error(f"{image} 是目录")

    try:
        requested = parse_size(args.size)
    except ValueError:
        ap.error(f"invalid --size: {args.size!r} (expected e.g. 64M, 1G)")

    target_fmt = args.format
    if target_fmt == "auto":
        target_fmt = detect_format(image) if exists else "qcow2"

    # 预检查输入文件
    new_files = []  # (name, path, size)
    for path in args.files:
        if not os.path.isfile(path):
            print(f"error: 不是普通文件: {path}", file=sys.stderr)
            return 1
        if os.path.abspath(path) == os.path.abspath(image):
            print(f"error: 输入文件与镜像相同: {path}", file=sys.stderr)
            return 1
        size = os.path.getsize(path)
        if size == 0:
            print(f"warn: 跳过空文件 {path}", file=sys.stderr)
            continue
        if size > 0xFFFFFFFF:
            print(f"error: {path} 太大（size 字段是 u32）", file=sys.stderr)
            return 1
        name = os.path.basename(path)
        try:
            encode_name(name)
        except ValueError as e:
            print(f"error: {e}", file=sys.stderr)
            return 1
        new_files.append((name, path, size))

    if not new_files:
        print("warn: 没有可写入的文件", file=sys.stderr)
        return 0

    tmp_raw = None
    try:
        if exists:
            if detect_format(image) == "qcow2":
                tmp_raw = make_temp(".raw")
                qcow2_to_raw(image, tmp_raw)
                raw_path = tmp_raw
            else:
                raw_path = image
        else:
            if target_fmt == "qcow2":
                tmp_raw = make_temp(".raw")
                raw_path = tmp_raw
            else:
                raw_path = image
            open(raw_path, "wb").close()

        with open(raw_path, "r+b") as f:
            cur = file_size(f)
            if not exists and cur < requested:
                f.truncate(requested)

            if args.append and exists and cur >= BLOCK_SIZE:
                try:
                    entries, old_index_blocks = read_chain(f)
                except ValueError as e:
                    print(f"error: {e}", file=sys.stderr)
                    return 1
                print(f"existing index: {len(entries)} file(s), {len(old_index_blocks)} index block(s)")
            else:
                entries, old_index_blocks = [], []

            # 同名文件视为替换：先释放旧条目占用的块
            replaced = {name for name, _, _ in new_files}
            if replaced:
                kept = [e for e in entries if e[0] not in replaced]
                if len(kept) != len(entries):
                    print("replace:", ", ".join(sorted(replaced)))
                entries = kept

            final_count = len(entries) + len(new_files)
            n_index = max(1, math.ceil(final_count / FILES_PER_INDEX))
            data_blocks_needed = sum((s + BLOCK_SIZE - 1) // BLOCK_SIZE for _, _, s in new_files)
            grow_by = max(1024 * 1024, (n_index + data_blocks_needed) * BLOCK_SIZE)

            placements = None
            index_ids = None
            for _ in range(32):
                if args.append:
                    existing_ids = sorted(old_index_blocks)
                    keep = min(len(existing_ids), n_index)
                    index_ids = list(existing_ids[:keep])
                    used = set(index_ids)
                    used |= compute_used(entries, [])
                else:
                    index_ids = []
                    used = set()
                placements = []
                total = file_size(f) // BLOCK_SIZE
                try:
                    while len(index_ids) < n_index:
                        b = alloc_run(used, 1, total)
                        if b is None:
                            raise NeedGrow
                        used.add(b)
                        index_ids.append(b)
                    for name, path, size in new_files:
                        n = (size + BLOCK_SIZE - 1) // BLOCK_SIZE
                        start = alloc_run(used, n, total)
                        if start is None:
                            raise NeedGrow
                        used.update(range(start, start + n))
                        placements.append((name, path, size, start))
                    break
                except NeedGrow:
                    f.truncate(file_size(f) + grow_by)
            else:
                print("error: 镜像空间不足，无法分配连续块", file=sys.stderr)
                return 1

            if not index_ids or index_ids[0] != 0:
                print("error: 内部错误：首个索引块不是块 0", file=sys.stderr)
                return 1

            # 写入文件数据（按块对齐，尾部补零）
            for name, path, size, start in placements:
                with open(path, "rb") as src:
                    f.seek(start * BLOCK_SIZE)
                    remaining = size
                    while remaining > 0:
                        chunk = src.read(min(1 << 20, remaining))
                        f.write(chunk)
                        remaining -= len(chunk)
                pad = (-size) % BLOCK_SIZE
                if pad:
                    f.write(b"\x00" * pad)

            # 写入索引链
            all_entries = list(entries)
            all_entries += [(n, s, z) for (n, _, z, s) in placements]
            for k, blk_id in enumerate(index_ids):
                group = all_entries[k * FILES_PER_INDEX:(k + 1) * FILES_PER_INDEX]
                blk = pack_index_block(group)
                next_id = index_ids[k + 1] if k + 1 < len(index_ids) else 0
                struct.pack_into("<Q", blk, NEXT_INDEX_OFFSET, next_id)
                f.seek(blk_id * BLOCK_SIZE)
                f.write(blk)

            f.flush()
            os.fsync(f.fileno())
            final_size = file_size(f)

        # 按目标格式写出
        if target_fmt == "qcow2":
            qc_tmp = make_temp(".qcow2")
            raw_to_qcow2(raw_path, qc_tmp, final_size)
            atomic_replace(qc_tmp, image)
        elif tmp_raw is not None:
            atomic_replace(tmp_raw, image)
            tmp_raw = None

        print(f"ok: wrote {len(placements)} file(s) into {image}")
        print(f"  format: {target_fmt}, virtual size: {final_size} bytes")
        print(f"  index blocks: {len(index_ids)} -> " + ", ".join(map(str, index_ids)))
        for name, _, size, start in placements:
            print(f"  {name:<16} start={start:<8} size={size}")
        if target_fmt == "qcow2":
            print(f"  on-disk size: {os.path.getsize(image)} bytes")
    except ValueError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    finally:
        if tmp_raw and os.path.exists(tmp_raw):
            os.unlink(tmp_raw)
    return 0


if __name__ == "__main__":
    sys.exit(main())
