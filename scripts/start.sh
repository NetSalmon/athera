#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"

QEMU_BIN="qemu-system-riscv64"
MACHINE="virt"
CPUS=1
KERNEL="$ROOT_DIR/target/riscv64gc-unknown-none-elf/release/athera"
DISK_FORMAT="qcow2"
BLK_IMAGE=""
PCI_BLK_IMAGE=""
DISPLAY_MODE="nographic"
GDB_WAIT=0
RANDOM_DEVICE=0
LOG_FLAGS=()
TRACE_EVENTS=(
    virtio_blk_handle_read
    virtio_blk_handle_write
    virtio_blk_submit_multireq
    virtio_blk_rw_complete
    virtio_blk_req_complete
)
QEMU_EXTRA_ARGS=()

usage() {
    cat <<EOF
用法: $(basename "$0") [选项] [-- QEMU参数...]

启动选项:
  -c, --cpus NUM          设置 CPU 核数（默认: 1，支持例如 2）
  -k, --kernel FILE       指定内核 ELF（默认: $KERNEL）
  -M, --machine NAME      指定 QEMU machine（默认: $MACHINE）
  -q, --qemu FILE         指定 QEMU 程序（默认: $QEMU_BIN）
      --disk-format FMT   磁盘格式（默认: $DISK_FORMAT）
  -d, --blk FILE           挂载 virtio-blk MMIO 磁盘
  -p, --pci-blk FILE       挂载 virtio-blk PCI 磁盘
  -r, --random             添加 virtio-rng MMIO 设备
  -b, --gui                使用 GTK 显示和 ramfb
  -s, --gdb                启用 GDB server 并暂停等待连接（-s -S）
  -m, --mmu-debug          输出 MMU 调试日志
  -i, --int-debug          输出中断调试日志
  -t, --trace EVENT        添加 QEMU trace 事件（可重复）
      --no-trace           禁用默认 QEMU trace 事件
  -h, --help               显示此帮助

示例:
  $(basename "$0") --cpus 2 --blk resources/minix.qcow2 --random
  $(basename "$0") -c 2 -d resources/minix.qcow2 -m -i
  $(basename "$0") --kernel target/.../athera -- --serial mon:stdio
EOF
}

die() {
    printf '错误: %s\n' "$1" >&2
    printf '使用 --help 查看用法。\n' >&2
    exit 2
}

require_value() {
    local option="$1"
    if (($# < 2)) || [[ -z "${2:-}" ]]; then
        die "$option 需要一个参数"
    fi
}

parse_positive_integer() {
    local option="$1"
    local value="$2"
    [[ "$value" =~ ^[1-9][0-9]*$ ]] || die "$option 必须是正整数: $value"
}

while (($# > 0)); do
    case "$1" in
        -c|--cpus)
            require_value "$1" "${2:-}"
            parse_positive_integer "$1" "$2"
            CPUS="$2"
            shift 2
            ;;
        -k|--kernel)
            require_value "$1" "${2:-}"
            KERNEL="$2"
            shift 2
            ;;
        -M|--machine)
            require_value "$1" "${2:-}"
            MACHINE="$2"
            shift 2
            ;;
        -q|--qemu)
            require_value "$1" "${2:-}"
            QEMU_BIN="$2"
            shift 2
            ;;
        --disk-format)
            require_value "$1" "${2:-}"
            DISK_FORMAT="$2"
            shift 2
            ;;
        -d|--blk)
            require_value "$1" "${2:-}"
            [[ -z "$BLK_IMAGE" ]] || die "不能重复指定 MMIO 磁盘"
            [[ -z "$PCI_BLK_IMAGE" ]] || die "不能同时指定 --blk 和 --pci-blk"
            BLK_IMAGE="$2"
            shift 2
            ;;
        -p|--pci-blk)
            require_value "$1" "${2:-}"
            [[ -z "$PCI_BLK_IMAGE" ]] || die "不能重复指定 PCI 磁盘"
            [[ -z "$BLK_IMAGE" ]] || die "不能同时指定 --pci-blk 和 --blk"
            PCI_BLK_IMAGE="$2"
            shift 2
            ;;
        -r|--random)
            RANDOM_DEVICE=1
            shift
            ;;
        -b|--gui)
            DISPLAY_MODE="gui"
            shift
            ;;
        -s|--gdb)
            GDB_WAIT=1
            shift
            ;;
        -m|--mmu-debug)
            LOG_FLAGS+=(mmu)
            shift
            ;;
        -i|--int-debug)
            LOG_FLAGS+=(int)
            shift
            ;;
        -t|--trace)
            require_value "$1" "${2:-}"
            if ((${#TRACE_EVENTS[@]} == 5)) && [[ "${TRACE_EVENTS[0]}" == virtio_blk_handle_read ]]; then
                TRACE_EVENTS=()
            fi
            TRACE_EVENTS+=("$2")
            shift 2
            ;;
        --no-trace)
            TRACE_EVENTS=()
            shift
            ;;
        --)
            shift
            QEMU_EXTRA_ARGS+=("$@")
            break
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -* )
            die "未知选项: $1"
            ;;
        *)
            die "不支持的位置参数: $1"
            ;;
    esac
done

[[ -f "$KERNEL" ]] || die "找不到内核 ELF: $KERNEL"
command -v "$QEMU_BIN" >/dev/null 2>&1 || die "找不到 QEMU: $QEMU_BIN"

if ((RANDOM_DEVICE)) && [[ -z "$BLK_IMAGE" && -z "$PCI_BLK_IMAGE" ]]; then
    die "--random 需要同时指定 --blk 或 --pci-blk"
fi

QEMU_ARGS=(
    -machine "$MACHINE"
    -smp "$CPUS"
    -kernel "$KERNEL"
)

for event in "${TRACE_EVENTS[@]}"; do
    QEMU_ARGS+=(-trace "$event")
done

if ((${#LOG_FLAGS[@]} > 0)); then
    QEMU_LOG_FLAGS=$(IFS=,; printf '%s' "${LOG_FLAGS[*]}")
    QEMU_ARGS+=(-d "$QEMU_LOG_FLAGS")
fi

if [[ "$DISPLAY_MODE" == gui ]]; then
    QEMU_ARGS+=(-display gtk -device ramfb)
else
    QEMU_ARGS+=(-nographic)
fi

if [[ -n "$BLK_IMAGE" ]]; then
    QEMU_ARGS+=(
        -drive "file=$BLK_IMAGE,format=$DISK_FORMAT,id=hd0,if=none"
        -device virtio-blk-device,drive=hd0
    )
elif [[ -n "$PCI_BLK_IMAGE" ]]; then
    QEMU_ARGS+=(
        -drive "file=$PCI_BLK_IMAGE,format=$DISK_FORMAT,id=hd0,if=none"
        -device virtio-blk-pci,drive=hd0,disable-legacy=on
    )
fi

if ((RANDOM_DEVICE)); then
    QEMU_ARGS+=(-device virtio-rng-device)
fi

if ((GDB_WAIT)); then
    QEMU_ARGS+=(-s -S)
fi

exec "$QEMU_BIN" "${QEMU_ARGS[@]}" "${QEMU_EXTRA_ARGS[@]}"
