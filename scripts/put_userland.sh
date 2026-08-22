#!/usr/bin/bash
# 把用户程序复制到 MINIX 虚拟磁盘的 /bin/ 目录下。
# 依赖: guestfish（libguestfs）、cargo（构建时）、qemu-img（必要时）。
set -euo pipefail

IMAGE="resources/minix.qcow2"
TARGET_DIR="target/riscv64gc-unknown-none-elf/release"
BUILD=1

usage() {
    echo "用法: $0 [-i IMAGE] [-n]"
    echo "  -i IMAGE   指定磁盘镜像（默认 resources/minix.qcow2）"
    echo "  -n         不重新构建，直接使用现有 ELF"
    echo "  -h         显示帮助"
}

while getopts "i:nh" opt; do
    case $opt in
        i) IMAGE=$OPTARG ;;
        n) BUILD=0 ;;
        h) usage; exit 0 ;;
        \?) usage >&2; exit 1 ;;
    esac
done

# 从 Cargo.toml 提取 [[bin]] 的 name（按顺序）
BINS=$(awk '/^\[\[bin\]\]/{inbin=1; next} inbin && /^name *=/ {gsub(/[ "]/,"",$0); split($0,a,"="); print a[2]; inbin=0}' athera-userland/Cargo.toml)

if [ -z "$BINS" ]; then
    echo "error: 未从 userland/Cargo.toml 解析到任何 bin" >&2
    exit 1
fi

if [ "$BUILD" = 1 ]; then
    echo "==> 构建用户程序"
    cargo build -p athera-athera-userland --release
fi

for bin in $BINS; do
    src="$TARGET_DIR/$bin"
    if [ ! -f "$src" ]; then
        echo "error: 未找到 $src，请先构建（或加 -n 跳过）" >&2
        exit 1
    fi
    echo "==> $bin ($(stat -c %s "$src") bytes)"
done

echo "==> 写入 $IMAGE:/bin/"

GUESTFISH_CMDS=()
for bin in $BINS; do
    GUESTFISH_CMDS+=(upload "$TARGET_DIR/$bin" "/bin/$bin" :)
    GUESTFISH_CMDS+=(chmod 0755 "/bin/$bin" :)
done

guestfish -a "$IMAGE" run : mount /dev/sda / : "${GUESTFISH_CMDS[@]}"

echo "==> 完成，镜像 /bin/ 内容："
guestfish -a "$IMAGE" run : mount /dev/sda / : ls /bin/
