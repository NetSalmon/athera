# Novus

一个用 Rust 编写的简易 RISC-V 64 操作系统。

## 特性

- 目标平台：`riscv64gc-unknown-none-elf`，QEMU `virt` 机型
- 通过 SBI 与底层交互（含 srst 系统复位、hsm 停止）
- UART 驱动：ns16550a，基于设备树自动探测
- 块设备驱动：virtio-blk（MMIO 模式）
- 中断控制器：PLIC 驱动
- 内存管理：等值映射页表、伙伴系统物理页帧分配器、SLUB 分配器
- 陷阱处理与定时器中断（Stimecmp）
- ELF 加载器（支持 32/64 位、大小端）
- 用户态进程管理与 ecall 系统调用（read/write/exit/reboot）
- 日志宏：`debug!` / `info!` / `error!`
- 构建时配置（`config.toml`）

## 工作区结构

```
novus/                  # 内核（根 crate）
├── src/
│   ├── arch/           RISC-V 寄存器与 SBI 封装
│   ├── dev/            设备驱动（ns16550a、virtio-blk、PLIC）
│   ├── mem/            地址抽象、页表、伙伴系统、SLUB、帧分配器
│   ├── entry.asm       启动汇编
│   ├── trap.rs         陷阱处理
│   ├── syscall.rs      系统调用
│   ├── usr.rs          用户态 ELF 加载
│   ├── proc.rs         进程控制块
│   ├── elf.rs          ELF 结构定义
│   ├── io.rs           格式化输出（print!/println!）
│   ├── log.rs          日志宏
│   ├── locks.rs        同步原语
│   ├── marco.rs        宏定义
│   ├── error.rs        错误类型
│   └── main.rs         内核入口
├── applications/       用户程序（子 crate）
│   ├── src/
│   │   ├── bin/hello_world.rs
│   │   ├── lib.rs      入口 _start
│   │   ├── syscall.rs  用户态 ecall 封装
│   │   ├── panic.rs
│   │   └── linker.ld
│   └── build.rs
├── const-num/          辅助 crate
├── const-val/          辅助 crate
├── linker.ld            内核链接脚本
├── config.toml          构建时配置
├── build.rs
└── start.sh             QEMU 启动脚本
```

## 构建与运行

依赖：

- Rust nightly（edition 2024）
- `riscv64-elf-objcopy`
- `qemu-system-riscv64`

构建并启动：

```bash
# 先构建用户程序
cargo build -p applications --release

# 构建内核并启动
cargo build --release
./start.sh
```

`start.sh` 选项：

| 选项 | 作用 |
| ---- | ---- |
| `-s` | 启用 GDB 调试（`-s -S`）|
| `-i` | 输出中断日志 |
| `-m` | 输出 MMU 日志 |
| `-p` | 挂载 virtio-blk PCI 磁盘（`resources/disk.qcow2`）|
| `-d` | 挂载 virtio-blk MMIO 磁盘 |

示例：

```bash
./start.sh -p          # 带 PCI 块设备启动
./start.sh -s -i       # 调试 + 中断日志
```

## 系统调用

| 编号 | 系统调用 | 描述 |
| ---- | -------- | ---- |
| 63   | read     | 从 UART 读取 |
| 64   | write    | 向 UART 写入 |
| 93   | exit     | 退出用户程序 |
| 142  | reboot   | 重启/关机/停机 |

## 许可证

GPL-3，详见 [LICENSE](LICENSE)。