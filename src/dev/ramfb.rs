#![allow(dead_code)]
//! ramfb 显示驱动（QEMU RamFB）。
//!
//! QEMU 11 的 ramfb 不再暴露 MMIO 寄存器，而是通过 fw_cfg 的 `etc/ramfb`
//! 文件配置：把 [`RamfbCfg`]（大端）写入该文件后，QEMU 会把帧缓冲
//! （guest RAM）扫描输出。帧缓冲使用 `DRM_FORMAT_XRGB8888`（32bpp，
//! 内存字节序 B,G,R,X）。探测/初始化/像素绘制均在物理内存上完成，
//! 本内核恒等映射下物理地址即虚拟地址。
use fdt::Fdt;

use crate::{
    constants::align,
    dev::{fw_cfg::FwCfg, traits::Device},
    mem::{allocators::alloc_frame, frame::Frame},
};

/// 帧缓冲宽度（像素）。
pub const WIDTH: u32 = 1024;
/// 帧缓冲高度（像素）。
pub const HEIGHT: u32 = 768;
/// 每像素字节数（XRGB8888）。
pub const BPP: u32 = 4;
/// 每行字节数。
pub const STRIDE: u32 = WIDTH * BPP;

/// 标准色卡占用的顶部高度（像素），下方留给图片轮播。
pub const CARD_HEIGHT: u32 = 128;
/// 图片轮播区域：位于 `(0, CARD_HEIGHT)`，尺寸 `WIDTH x IMAGE_HEIGHT`。
pub const IMAGE_HEIGHT: u32 = HEIGHT - CARD_HEIGHT;

/// `DRM_FORMAT_XRGB8888`（fourcc "XR24"）。
const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;

/// QEMU `struct RAMFBCfg` 的字节数（`QEMU_PACKED`，无对齐填充）。
const RAMFB_CFG_SIZE: u32 = 28;

/// ramfb 配置结构（与 QEMU `struct RAMFBCfg` 一致，全部大端）。
#[repr(C)]
struct RamfbCfg {
    addr: u64,
    fourcc: u32,
    flags: u32,
    width: u32,
    height: u32,
    stride: u32,
}

impl RamfbCfg {
    fn to_be_bytes(&self) -> [u8; 28] {
        let mut out = [0u8; 28];
        out[0..8].copy_from_slice(&self.addr.to_be_bytes());
        out[8..12].copy_from_slice(&self.fourcc.to_be_bytes());
        out[12..16].copy_from_slice(&self.flags.to_be_bytes());
        out[16..20].copy_from_slice(&self.width.to_be_bytes());
        out[20..24].copy_from_slice(&self.height.to_be_bytes());
        out[24..28].copy_from_slice(&self.stride.to_be_bytes());
        out
    }
}

pub struct Ramfb {
    fw: FwCfg,
    frame: Frame,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

impl Device for Ramfb {
    fn name(&self) -> &'static str {
        "ramfb"
    }

    fn irq(&self) -> Option<usize> {
        None
    }
}

impl Ramfb {
    /// 探测 fw_cfg 并定位 `etc/ramfb`，分配帧缓冲并下发配置。
    pub fn probe(fdt: &Fdt) -> Option<Self> {
        let fw = FwCfg::probe(fdt)?;
        crate::info!("ramfb: fw_cfg @ {:#x}", fw.device.mmio.start);
        let (selector, size) = fw.find_file("etc/ramfb")?;
        if size != RAMFB_CFG_SIZE {
            crate::warn!("ramfb: unexpected config size {size}");
            return None;
        }

        let width = WIDTH;
        let height = HEIGHT;
        let stride = STRIDE;
        let fb_bytes = (width * height * BPP) as usize;

        // 分配一页对齐的物理帧缓冲（伙伴系统按 2 的幂放大）。
        let frame = match alloc_frame(Some(fb_bytes)) {
            Some(frame) => frame,
            None => {
                crate::warn!("ramfb: failed to allocate {fb_bytes} bytes frame");
                return None;
            }
        };

        let cfg = RamfbCfg {
            addr: frame.start as u64,
            fourcc: DRM_FORMAT_XRGB8888,
            flags: 0,
            width,
            height,
            stride,
        };
        if fw.write(selector, &cfg.to_be_bytes()).is_err() {
            crate::warn!("ramfb: failed to write config to fw_cfg");
            return None;
        }

        Some(Self {
            fw,
            frame,
            width,
            height,
            stride,
        })
    }

    /// fw_cfg 访问器（供后续查询/调试）。
    pub fn fw_cfg(&self) -> &FwCfg {
        &self.fw
    }

    /// 帧缓冲起始物理地址（也即虚拟地址，恒等映射）。
    pub fn base(&self) -> usize {
        self.frame.start
    }

    /// 帧缓冲字节切片。
    fn fb_bytes(&mut self) -> &mut [u8] {
        self.frame.as_bytes_mut()
    }

    /// 整屏填充单一颜色（`0x00RRGGBB`）。
    pub fn clear(&mut self, color: u32) {
        let bytes = self.fb_bytes();
        let pixels = unsafe {
            core::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut u32, bytes.len() / 4)
        };
        for p in pixels.iter_mut() {
            *p = color;
        }
    }

    /// 填充矩形区域。
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        let width = self.width;
        let x2 = (x + w).min(width);
        let y2 = (y + h).min(self.height);
        let mut row = y;
        while row < y2 {
            let base = (row * width + x) as usize;
            let count = (x2 - x) as usize;
            let bytes = self.fb_bytes();
            let pixels = unsafe {
                core::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut u32, bytes.len() / 4)
            };
            for p in pixels.iter_mut().skip(base).take(count) {
                *p = color;
            }
            row += 1;
        }
    }

    /// 把一段 `XRGB8888` 原始像素（行优先，宽 `w` 高 `h`）拷贝到
    /// 帧缓冲 `(x, y)` 处（逐行 memcpy）。
    pub fn blit(&mut self, src: &[u8], x: u32, y: u32, w: u32, h: u32) {
        let width = self.width;
        let need = (w * h * BPP) as usize;
        if src.len() < need {
            return;
        }

        let fb = self.fb_bytes();
        let row_bytes = (w * BPP) as usize;
        for row in 0..h {
            let dst_off = ((y + row) * width + x) as usize * BPP as usize;
            let src_off = row as usize * row_bytes;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    src.as_ptr().add(src_off),
                    fb.as_mut_ptr().add(dst_off),
                    row_bytes,
                );
            }
        }
    }

    /// 绘制标准色卡：SMPTE 75% 彩条 + 灰阶渐变 + 色相渐变。
    pub fn draw_color_card(&mut self) {
        let w = self.width;
        let bar_h = CARD_HEIGHT * 3 / 8; // 48
        let gray_h = CARD_HEIGHT / 4; // 32
        let hue_h = CARD_HEIGHT - bar_h - gray_h; // 48

        // SMPTE 75% 彩条（白/黄/青/绿/品红/红/蓝）。
        const BARS: [(u32, u32, u32); 7] = [
            (0xFF, 0xFF, 0xFF),
            (0xFF, 0xFF, 0x00),
            (0x00, 0xFF, 0xFF),
            (0x00, 0xFF, 0x00),
            (0xFF, 0x00, 0xFF),
            (0xFF, 0x00, 0x00),
            (0x00, 0x00, 0xFF),
        ];
        let bar_w = w / 7;
        for (i, (r, g, b)) in BARS.iter().enumerate() {
            self.fill_rect(i as u32 * bar_w, 0, bar_w, bar_h, rgb(*r, *g, *b));
        }

        // 灰阶渐变：左黑右白。
        for x in 0..w {
            let v = (x * 255) / w.max(1);
            self.fill_rect(x, bar_h, 1, gray_h, rgb(v, v, v));
        }

        // 色相渐变：红→黄→绿→青→蓝→品红→红。
        for x in 0..w {
            let t = x as f32 / w.max(1) as f32;
            let (r, g, b) = hue_rgb(t);
            self.fill_rect(x, bar_h + gray_h, 1, hue_h, rgb(r, g, b));
        }

        // 四周白色边框，便于确认帧缓冲范围。
        self.fill_rect(0, 0, w, 1, rgb(0xFF, 0xFF, 0xFF));
        self.fill_rect(0, CARD_HEIGHT - 1, w, 1, rgb(0xFF, 0xFF, 0xFF));
    }
}

/// 把帧缓冲地址向上对齐到 `align` 字节（保留，供外部复用）。
pub(crate) fn align_up(addr: usize, align_to: usize) -> usize {
    align(addr, align_to)
}

#[inline]
fn rgb(r: u32, g: u32, b: u32) -> u32 {
    (r & 0xFF) << 16 | (g & 0xFF) << 8 | (b & 0xFF)
}

/// HSV 色相（0..1）→ RGB。
fn hue_rgb(t: f32) -> (u32, u32, u32) {
    let h = t * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    (
        (r * 255.0) as u32,
        (g * 255.0) as u32,
        (b * 255.0) as u32,
    )
}
