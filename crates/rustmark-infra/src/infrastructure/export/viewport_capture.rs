//! 视口截图 → PNG（图片导出的像素层）。
//!
//! # 为什么用 egui 官方截图 API 而不自己栅格化
//!
//! 最早的实现方向是「拿到 tessellate 后的三角形在 CPU 上自己扫一遍」
//! （软件光栅化器）。查明 egui 0.33 源码后**放弃了该方向**——
//! [`egui::ViewportCommand::Screenshot`] 是官方能力，且 eframe 的
//! **wgpu 与 glow 两个后端都已实现**（wgpu 走 [`egui_wgpu`] 的
//! `CaptureState`，glow 走 `painter.read_screen_rgba`），
//! 结果经 [`egui::Event::Screenshot`] 在**下一帧**回传。
//!
//! 官方路径的优势不是「少写几百行」，而是**保真度**：
//! 语法高亮是把每个 token 画进一张 `TextureId::User` 的图片纹理再贴图的，
//! 软件光栅化要额外接住纹理 atlas 与 `PaintCallback` 才能还原，
//! 而抗锯齿、emoji 字体回退也都要自己实现——每一项都是新的出错面。
//! 官方回读的是**已经光栅化完成的帧**，这些全都天然正确。
//!
//! # 能力边界：视口截图，不是整篇长图
//!
//! 官方 API 截的是**当前视口**，即用户此刻看到的画面。
//! 要「整篇文档的长图」得离屏重建完整布局（把所有块渲染到超出屏幕的画布上
//! 再滚一遍），那是另一个量级的工程。本模块只做前者，并在调用处如实告知。
//!
//! # 两帧延迟是 API 决定的，不是设计缺陷
//!
//! 帧 N 发出 `Screenshot` 命令 → 帧 N 绘制时被捕获 → 帧 N+1 收到
//! `Event::Screenshot`。故导出必然跨一帧，调用方需自己持有「等待中」状态。
//!
//! # 模块结构
//!
//! facade 保留错误类型、尺寸上限常量与一步到位的 `capture_region_to_png`；
//! 纯像素操作（裁剪、PNG 写盘、长图拼接）在 `image`。

mod image;

use std::path::Path;

use rustmark_core::shared::error::AppError;

pub use image::{crop, stitch, write_png};

/// 截图像素的端口表示：纯 RGBA8 字节，与 egui / GPU 无关。
///
/// 视口截图在消费者（`src/app/frame/capture.rs`）处由 egui 的 `ColorImage`
/// 转换而来；基础设施只处理端口字节，不认识任何渲染后端类型。这样
/// `viewport_capture` 簇可在无头单测里锁住裁剪 / 拼接 / 写盘逻辑，
/// 而不必引入 egui（V11-63 端口化目标之一）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortFrame {
    /// `[宽, 高]`（像素）。
    pub size: [usize; 2],
    /// 行优先的 RGBA8 像素（`[r, g, b, a]`，直 alpha）。
    pub pixels: Vec<[u8; 4]>,
}

/// 截图处理失败（转成 [`AppError::io`] 上报）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// 裁剪矩形落在画面之外（窗口移动 / 布局变化 / 预览区被折叠）。
    ClipOutOfBounds {
        /// 请求的裁剪矩形（物理像素）。
        requested: [i64; 4],
        /// 实际画面尺寸 `[宽, 高]`。
        actual: [usize; 2],
    },
    /// 裁剪结果宽或高为 0（预览区不可见、窗口被压扁等）。
    EmptyClip {
        /// 请求的裁剪矩形（物理像素）。
        requested: [i64; 4],
    },
    /// 图像超出尺寸上限，避免先撑爆内存再失败。
    TooLarge {
        /// 请求宽度。
        width: u32,
        /// 请求高度。
        height: u32,
        /// 允许的上限。
        limit: u32,
    },
    /// 写盘失败。
    Io {
        /// 底层错误描述。
        message: String,
    },
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 与 `From<CaptureError> for AppError` 的文案保持一致：
        // 用户看到的错误来自上层包装过的 `AppError`，若两处措辞不同，
        // 同一次失败会在不同路径下读起来像两个问题。
        write!(f, "{}", AppError::from(self.clone()))
    }
}

impl From<CaptureError> for AppError {
    fn from(e: CaptureError) -> Self {
        match e {
            CaptureError::ClipOutOfBounds { requested, actual } => AppError::io(format!(
                "截图区域 {requested:?} 不在画面 {actual:?} 内，请确认预览区可见后重试"
            )),
            CaptureError::EmptyClip { requested } => AppError::io(format!(
                "截图区域 {requested:?} 宽高为零，请确认预览区可见后重试"
            )),
            CaptureError::TooLarge {
                width,
                height,
                limit,
            } => AppError::io(format!(
                "截图尺寸 {width}x{height} 超过上限 {limit}，请缩小窗口或降低缩放"
            )),
            CaptureError::Io { message } => AppError::io(format!("写截图失败：{message}")),
        }
    }
}

/// 图像边长上限（单轴）。
///
/// 8192 ≈ A4@300dpi 的长边，再大对屏幕截图没有实际意义，
/// 却会让一张异常截图先吃掉数百 MB 再失败。
pub const MAX_DIMENSION: u32 = 8192;

/// 长图（多段拼接）成品的高度上限（物理像素）。
///
/// 拼接图在内存里整体驻留：宽 2000px × 高 16384px × 4 字节 ≈ 131 MB，
/// 是一次导出的**瞬时**峰值，可接受；再往上就该劝用户改用 HTML / PDF
/// （那两条路是流式的，不受窗口尺寸限制）。
pub const MAX_LONG_IMAGE_HEIGHT: u32 = 16_384;

/// 一步到位：整帧截图 → 裁剪 → 写 PNG。
pub fn capture_region_to_png(
    frame: &PortFrame,
    clip: [i64; 4],
    path: &Path,
) -> Result<usize, CaptureError> {
    let cropped = crop(frame, clip)?;
    write_png(&cropped, path)
}

#[cfg(test)]
mod tests;
