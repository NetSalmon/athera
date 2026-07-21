# Novus

一个用 Rust 编写的简易 RISC-V 64 操作系统。

## 特性

- 目标平台：`riscv64gc-unknown-none-elf`，QEMU `virt` 机型
- 通过 SBI 与底层交互（含 srst 系统复位、hsm 停止）
- UART 驱动：ns16550a，基于设备树自动探测
- 块设备驱动：virtio-blk（MMIO 模式，含 virtio-mmio 传输层）
- 内存管理：等值映射页表、伙伴系统物理页帧分配器、SLUB 全局分配器
- 陷阱处理与定时器中断（Stimecmp）
- ELF 加载器（支持 32/64 位、大小端）
- 用户态进程管理与 ecall 系统调用（read/write/exit/reboot）
- 同步原语：`SpinLock` / `OnceLock` / `LazyLock`（懒加载静态）
- 日志宏：`debug!` / `info!` / `error!`
- 构建时配置（`config.toml`）

## 工作区结构

```
novus/                    # 内核（根 crate）
├── src/
│   ├── arch/             RISC-V 寄存器（csr/gpr）与 SBI 封装
│   ├── dev/              设备驱动
│   │   ├── ns16550a.rs   UART 驱动
│   │   ├── memory.rs     内存区域探测
│   │   ├── virtio_blk/   virtio-blk 驱动
│   │   └── virtio_mmio/  virtio-mmio 传输层与虚拟队列
│   ├── mem/              内存管理
│   │   ├── addr.rs       地址抽象（Sv39）
│   │   ├── constants.rs  内存常量
│   │   ├── page_table.rs 页表
│   │   ├── alloc_page.rs 页面分配
│   │   └── allocators/   伙伴系统、SLUB、侵入式链表
│   ├── entry.asm         启动汇编
│   ├── trap.rs           陷阱处理
│   ├── syscall.rs        系统调用
│   ├── usr.rs            用户态 ELF 加载
│   ├── proc.rs           进程控制块与 PID 分配
│   ├── elf.rs            ELF 结构定义
│   ├── constants.rs      全局常量与懒加载静态（内存区间、版本等）
│   ├── io.rs             格式化输出（print!/println!）
│   ├── log.rs            日志宏
│   ├── locks.rs          同步原语（SpinLock/OnceLock/LazyLock）
│   ├── macros.rs         宏定义
│   ├── error.rs          错误类型
│   └── main.rs           内核入口
├── applications/         用户程序（子 crate）
│   ├── src/
│   │   ├── bin/hello_world.rs
│   │   ├── lib.rs        入口 _start
│   │   ├── syscall.rs    用户态 ecall 封装
│   │   ├── panic.rs
│   │   └── linker.ld
│   └── build.rs
├── const-num/            辅助 crate（编译期数字解析）
├── const-val/            辅助 crate（编译期配置常量 proc-macro）
├── linker.ld             内核链接脚本
├── config.toml           构建时配置
├── build.rs
└── start.sh              QEMU 启动脚本
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
| `-b` | 使用 GTK 显示（`-display gtk` + ramfb）|
| `-d` | 挂载 virtio-blk MMIO 磁盘 |

示例：

```bash
./start.sh -p          # 带 PCI 块设备启动
./start.sh -s -i       # 调试 + 中断日志
./start.sh -b -d       # GTK 图形界面 + MMIO 磁盘
```

## 系统调用

| 编号 | 系统调用 | 描述 |
| ---- | -------- | ---- |
| 63   | read     | 从 UART 读取 |
| 64   | write    | 向 UART 写入 |
| 93   | exit     | 退出用户程序 |
| 95   | waitpid  | 等待子进程（未实现）|
| 142  | reboot   | 重启/关机/停机 |
| 220  | fork     | 创建子进程（未实现）|
| 221  | exec     | 执行新程序（未实现）|


## 初始化依赖

内核大量使用 `LazyLock`（经 `const-val` 的 `#[lazy]` / `#[lazy(spin)]` 宏生成）实现懒加载静态。
各静态首次通过 `.force()` 访问时按需初始化，其依赖关系如下：

```mermaid
graph TD
    subgraph 汇编期声明["汇编期声明 (entry.asm)"]
        fdt_addr["FDT_ADDR<br/>启动时由 a1 写入设备树地址"]
        kernel_stack["kernel_stack_top"]
        user_stack["user_stack_top"]
        queue_top["queue_top<br/>virtio 队列区"]
    end

    subgraph 懒加载静态
        fdt_addr --> memory_range["MEMORY_RANGE<br/>(constants.rs)"]
        fdt_addr --> dev_tree["DEV_TREE<br/>(dev.rs)"]
        memory_range --> avail_range["AVAIL_RANGE<br/>(_end..MEMORY_RANGE.end)"]
        avail_range --> frame_allocator["FRAME_ALLOCATOR<br/>伙伴系统 (lazy spin)"]
        frame_allocator --> root_page_table["ROOT_PAGE_TABLE<br/>(page_table.rs)"]
        frame_allocator --> caches["CACHES<br/>SLUB 全局分配器 (lazy spin)"]
    end

    subgraph 功能
        caches --> alloc_structs["Vec / String / BTreeMap ..."]
        alloc_structs --> tasks["TASKS<br/>进程表 (proc.rs)"]
        dev_tree --> device["ns16550a / virtio-blk ..."]
        root_page_table --> identity_map["identity_map()"]
        dev_tree --> identity_map
        root_page_table --> map["map / unmap"]
        caches --> exec["usr::exec（用户程序加载）"]
        device --> syscall["read / write 系统调用"]
    end
```

- **汇编期声明**：`entry.asm` 中 `_start` 在 `.data`/`.bss` 段预留了 `FDT_ADDR`、内核/用户栈及 virtio 队列区；`FDT_ADDR` 在启动时由寄存器 `a1` 写入设备树物理地址，供 Rust 侧作为 `extern` 符号读取。
- `MEMORY_RANGE` 与 `DEV_TREE` 依赖启动时由汇编写入的 `FDT_ADDR`（设备树地址）。
- `FRAME_ALLOCATOR`（伙伴系统）以 `AVAIL_RANGE`（内核末尾 `_end` 到内存末尾）作为可用物理页范围。
- `ROOT_PAGE_TABLE` 与 `CACHES`（SLUB 全局分配器）均需先分配物理页帧，故依赖 `FRAME_ALLOCATOR`。
- 一旦 `CACHES` 就绪，`Vec` / `String` / `BTreeMap` 等 `alloc` 结构以及基于它们的 `TASKS` 进程表方可使用。
## 许可证

GPL-3，详见 [LICENSE](LICENSE)。