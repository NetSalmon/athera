#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把文件写入 MINIX V1 文件系统镜像（qcow2 / raw，复用 mkdisk.py 的 qcow2 读写）。

磁盘格式与 src/fs/minix_fs.rs 对应：
  * 超级块位于第 1 块（偏移 1024），zone 大小 = 1024 << log_zone_size；
  * inode 位图 / zone 位图 / inode 表依次紧随其后，数据区从 first_data_zone 开始；
  * MINIX V1 inode 32 字节（2+2+4+4+1+1+18），目录项 2+NAME_LEN 字节
    （魔数 0x137F -> NAME_LEN 14，0x138F -> NAME_LEN 30）。

支持 7 个直接 zone + 1 个一级间接 zone（约 530 KiB 以下的文件）。
"""
import argparse
import os
import struct
import sys
import time

from mkdisk import atomic_replace, detect_format, make_temp, qcow2_to_raw, raw_to_qcow2

BLOCK = 1024          # 默认 zone 大小（log_zone_size = 0）
INODE_SIZE = 32
MAX_DIRECT_ZONES = 7
INDIRECT_CAPACITY = BLOCK // 2   # 一级间接 zone 可容纳的 u16 zone 号个数
NAME_LENS = {0x137F: 14, 0x138F: 30}


class MinixV1:
    def __init__(self, img: bytes):
        if len(img) < 2048:
            raise ValueError("镜像太小，不是 MINIX V1 文件系统")
        self.img = img
        sb = img[1024:1024 + 20]
        (self.ninodes, self.nzones, self.imap_blocks, self.zmap_blocks,
         self.first_data_zone, self.log_zone_size) = struct.unpack("<6H", sb[:12])
        self.max_size, self.magic, self.state = struct.unpack("<IHH", sb[12:20])
        self.zone_size = 1024 << self.log_zone_size
        self.name_len = NAME_LENS.get(self.magic)
        if self.name_len is None:
            raise ValueError(f"未知 MINIX 魔数: {self.magic:#06x}")
        self.entry_size = 2 + self.name_len
        self.imap_off = 2 * self.zone_size
        self.zmap_off = (2 + self.imap_blocks) * self.zone_size
        self.inode_off = (2 + self.imap_blocks + self.zmap_blocks) * self.zone_size

    # ---------- 基础读写 ----------

    def zone(self, z: int) -> bytes:
        off = z * self.zone_size
        return self.img[off:off + self.zone_size]

    def inode(self, ino: int) -> tuple:
        off = self.inode_off + (ino - 1) * INODE_SIZE
        mode, uid, size, mtime, gid, nlinks = struct.unpack_from("<HHIIBB", self.img, off)
        zones = list(struct.unpack_from("<9H", self.img, off + 14))
        return off, dict(mode=mode, uid=uid, size=size, mtime=mtime,
                         gid=gid, nlinks=nlinks, zones=zones)

    def read_data(self, ino: dict) -> bytes:
        """按直接 + 一级间接 zone 读取 inode 内容（长度取 size）。"""
        out = bytearray()
        zones = list(ino["zones"][:MAX_DIRECT_ZONES])
        if ino["zones"][7]:
            ind = self.zone(ino["zones"][7])
            zones += list(struct.unpack_from(f"<{INDIRECT_CAPACITY}H", ind, 0))
            zones = [z for z in zones if z]
        for z in zones:
            out += self.zone(z)
            if len(out) >= ino["size"]:
                break
        return bytes(out[:ino["size"]])

    def bitmap_get(self, off: int, n: int) -> bool:
        byte = self.img[off + n // 8]
        return bool(byte & (1 << (n % 8)))

    def bitmap_set(self, off: int, n: int):
        pos = off + n // 8
        b = self.img[pos]
        self.img = self.img[:pos] + bytes([b | (1 << (n % 8))]) + self.img[pos + 1:]

    def alloc_inode(self) -> int:
        for i in range(1, self.ninodes + 1):
            if not self.bitmap_get(self.imap_off, i - 1):
                self.bitmap_set(self.imap_off, i - 1)
                return i
        raise ValueError("inode 表已满")

    def alloc_zones(self, n: int) -> list:
        if n <= 0:
            return []
        free = []
        for z in range(self.first_data_zone, self.nzones):
            if not self.bitmap_get(self.zmap_off, z):
                free.append(z)
            if len(free) == n:
                break
        if len(free) < n:
            raise ValueError(f"空闲 zone 不足（需要 {n}，找到 {len(free)}）")
        for z in free:
            self.bitmap_set(self.zmap_off, z)
        return free

    def write_inode(self, ino: int, mode: int, size: int, zones: list, nlinks: int = 1):
        off, _ = self.inode(ino)
        z = list(zones) + [0] * (9 - len(zones))
        data = struct.pack("<HHIIBB", mode, 0, size, int(time.time()), 0, nlinks)
        data += struct.pack("<9H", *z)
        self.img = self.img[:off] + data + self.img[off + INODE_SIZE:]

    def put_file(self, dst_path: str, content: bytes):
        """把 content 写入 dst_path（如 /bin/sort），自动分配 inode / zone。"""
        parts = [p for p in dst_path.split("/") if p]
        if not parts:
            raise ValueError("目标路径不能为空")
        name = parts[-1]
        if len(name.encode()) > self.name_len:
            raise ValueError(f"文件名 {name!r} 超过 {self.name_len} 字节")
        parent = self.find_dir(parts[:-1]) if len(parts) > 1 else self.root_dir()

        zones_needed = (len(content) + self.zone_size - 1) // self.zone_size
        if zones_needed > MAX_DIRECT_ZONES + INDIRECT_CAPACITY:
            raise ValueError(f"文件过大（需要 {zones_needed} 个 zone，超出直接+一级间接上限）")

        ino = self.alloc_inode()
        zones = self.alloc_zones(zones_needed)

        # 写数据 zone
        for i, z in enumerate(zones):
            chunk = content[i * self.zone_size:(i + 1) * self.zone_size]
            chunk = chunk + b"\x00" * (self.zone_size - len(chunk))
            pos = z * self.zone_size
            self.img = self.img[:pos] + chunk + self.img[pos + len(chunk):]

        # 写 inode（含一级间接）
        if zones_needed <= MAX_DIRECT_ZONES:
            inode_zones = zones
        else:
            ind = self.alloc_zones(1)[0]
            entries = zones[MAX_DIRECT_ZONES:] + [0] * (INDIRECT_CAPACITY - (zones_needed - MAX_DIRECT_ZONES))
            ind_data = struct.pack(f"<{INDIRECT_CAPACITY}H", *entries)
            pos = ind * self.zone_size
            self.img = self.img[:pos] + ind_data + self.img[pos + len(ind_data):]
            inode_zones = zones[:MAX_DIRECT_ZONES] + [ind]

        self.write_inode(ino, 0o100755, len(content), inode_zones)

        # 在父目录追加目录项
        self.add_dir_entry(parent, ino, name)
        return ino

    # ---------- 目录 ----------

    def root_dir(self) -> int:
        return 1

    def find_dir(self, parts: list) -> int:
        """沿路径逐级查找目录 inode；不存在则报错。"""
        cur = self.root_dir()
        for p in parts:
            _, ino = self.lookup(cur, p)
            if ino is None:
                raise ValueError(f"目录不存在: /{'/'.join(parts)}（缺少 {p!r}）")
            _, d = self.inode(ino)
            if d["mode"] & 0o170000 != 0o040000:
                raise ValueError(f"{p!r} 不是目录")
            cur = ino
        return cur

    def lookup(self, dir_ino: int, name: str) -> tuple:
        _, d = self.inode(dir_ino)
        raw = self.read_data(d)
        for off in range(0, len(raw), self.entry_size):
            e = raw[off:off + self.entry_size]
            if len(e) < self.entry_size:
                break
            ino, n = struct.unpack_from(f"<H{self.name_len}s", e, 0)
            n = n.split(b"\x00")[0].decode("utf-8", "replace")
            if n == name:
                return off, ino
        return None, None

    def add_dir_entry(self, dir_ino: int, new_ino: int, name: str):
        _, d = self.inode(dir_ino)
        if self.lookup(dir_ino, name)[1] is not None:
            raise ValueError(f"{name!r} 已存在于目录 inode {dir_ino}")
        entry = struct.pack(f"<H{self.name_len}s", new_ino, name.encode())
        if len(entry) != self.entry_size:
            raise ValueError("目录项编码错误")
        # 目录项落在已分配 zone 内即可（当前镜像 /bin 一个 zone 足够）
        capacity = 0
        zones = list(d["zones"][:MAX_DIRECT_ZONES])
        if d["zones"][7]:
            ind = self.zone(d["zones"][7])
            zones += [z for z in struct.unpack_from(f"<{INDIRECT_CAPACITY}H", ind, 0) if z]
        capacity = len(zones) * self.zone_size
        if d["size"] + self.entry_size > capacity:
            raise ValueError(f"目录 inode {dir_ino} 已满（需扩容，暂不支持）")
        pos = d["zones"][0] * self.zone_size + d["size"]
        self.img = self.img[:pos] + entry + self.img[pos + len(entry):]
        self.write_inode(dir_ino, d["mode"], d["size"] + self.entry_size,
                         [z for z in d["zones"] if z], d["nlinks"])

    # ---------- 列表 ----------

    def list_dir(self, dir_ino: int, prefix: str):
        _, d = self.inode(dir_ino)
        raw = self.read_data(d)
        for off in range(0, len(raw), self.entry_size):
            e = raw[off:off + self.entry_size]
            if len(e) < self.entry_size:
                break
            ino, n = struct.unpack_from(f"<H{self.name_len}s", e, 0)
            n = n.split(b"\x00")[0].decode("utf-8", "replace")
            if ino == 0:
                continue
            _, ci = self.inode(ino)
            kind = "d" if ci["mode"] & 0o170000 == 0o040000 else "-"
            print(f"  {prefix}{n:<20} ino={ino:<6} mode={ci['mode']:06o} size={ci['size']}")
            if kind == "d" and n not in (".", ".."):
                self.list_dir(ino, prefix + n + "/")


def cmd_list(image):
    raw, tmp = load_raw(image)
    try:
        fs = MinixV1(raw)
        print(f"magic={fs.magic:#06x} name_len={fs.name_len} "
              f"ninodes={fs.ninodes} nzones={fs.nzones} zone_size={fs.zone_size}")
        fs.list_dir(fs.root_dir(), "")
    finally:
        if tmp:
            os.unlink(tmp)
    return 0


def load_raw(image):
    """返回 (raw bytes, 临时文件路径或 None)。"""
    if not os.path.isfile(image):
        raise ValueError(f"no such image: {image}")
    if detect_format(image) == "qcow2":
        tmp = make_temp(".raw")
        qcow2_to_raw(image, tmp)
        with open(tmp, "rb") as f:
            return f.read(), tmp
    with open(image, "rb") as f:
        return f.read(), None


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="把文件写入 MINIX V1 文件系统镜像（qcow2 / raw，复用 mkdisk.py 的 qcow2 读写）。",
        epilog="""示例:
  python3 minix_put.py resources/minix.qcow2 target/riscv64gc-unknown-none-elf/release/sort /bin/sort
  python3 minix_put.py --list resources/minix.qcow2
""",
    )
    ap.add_argument("image", help="MINIX 镜像路径（qcow2 / raw）")
    ap.add_argument("src", nargs="?", help="要写入的文件（与 dst 成对出现）")
    ap.add_argument("dst", nargs="?", help="镜像内目标路径，如 /bin/sort")
    ap.add_argument("--list", action="store_true", help="列出镜像内容并退出")
    args = ap.parse_args(argv)

    if args.list:
        return cmd_list(args.image)

    if not args.src or not args.dst:
        ap.error("需要 src 与 dst，或使用 --list")
    if not os.path.isfile(args.src):
        ap.error(f"no such file: {args.src}")

    with open(args.src, "rb") as f:
        content = f.read()

    raw, tmp_raw = load_raw(args.image)
    tmp_out = None
    try:
        fs = MinixV1(raw)
        ino = fs.put_file(args.dst, content)
        fmt = detect_format(args.image)
        if fmt == "qcow2":
            tmp_raw = tmp_raw or make_temp(".raw")
            with open(tmp_raw, "wb") as f:
                f.write(fs.img)
            tmp_out = make_temp(".qcow2")
            raw_to_qcow2(tmp_raw, tmp_out, len(fs.img))
            atomic_replace(tmp_out, args.image)
        else:
            with open(args.image, "wb") as f:
                f.write(fs.img)
        print(f"ok: wrote {args.src} -> {args.dst} (inode {ino}, {len(content)} bytes)")
    except ValueError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    finally:
        for p in (tmp_out, tmp_raw):
            if p and os.path.exists(p):
                os.unlink(p)
    return 0


if __name__ == "__main__":
    sys.exit(main())
