#!/usr/bin/bash

QEMU_ARGS=(
    -machine virt
    -trace virtio_blk_handle_read
    -trace virtio_blk_handle_write
    -trace virtio_blk_submit_multireq
    -trace virtio_blk_rw_complete
    -trace virtio_blk_req_complete
    -kernel target/riscv64gc-unknown-none-elf/release/novus
)

QEMU_OPTS=()
LOG_FLAGS=()
DISPLAY_OPTS="-nographic"

while getopts "misdpb" opt; do
  case $opt in
    m)
      LOG_FLAGS+=("mmu")
      ;;
    i)
      LOG_FLAGS+=("int")
      ;;
    s)
      QEMU_OPTS+=("-s" "-S")
      ;;
    p)
      QEMU_OPTS+=(
        -drive "file=./resources/disk.qcow2,format=qcow2,id=hd0,if=none"
        -device "virtio-blk-pci,drive=hd0,disable-legacy=on"
      )
      ;;
d)
      QEM_OPTS+=(
        -drive "file=./resources/disk.qcow2,format=qcow2,id=hd0,if=none"
        -device "virtio-blk-device,drive=hd0"
      )
      ;;
b)
      DISPLAY_OPTS="-display gtk"
      QEMU_OPTS+=("-device" "ramfb")
      ;;
    \?)
      echo "无效的选项: -$OPTARG" >&2
      exit 1
      ;;
  esac
done

if [ ${#LOG_FLAGS[@]} -ne 0 ]; then
    IFS=',' LOG_STR="${LOG_FLAGS[*]}"
    QEMU_OPTS+=("-d" "$LOG_STR")
fi

# 6. 最终执行 QEMU 命令
qemu-system-riscv64 "${QEMU_ARGS[@]}" $DISPLAY_OPTS "${QEMU_OPTS[@]}"