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
use std::sync::Arc;

use egui::epaint::ColorImage;

use crate::shared::error::AppError;

pub use image::{crop, stitch, write_png};

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
    frame: &Arc<ColorImage>,
    clip: [i64; 4],
    path: &Path,
) -> Result<usize, CaptureError> {
    let cropped = crop(frame, clip)?;
    write_png(&cropped, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    // 本模块的私有子模块也叫 `image`，glob 会把它带进来遮蔽外部 crate；
    // 显式导入让 `image::open` 等路径仍指向 image crate。
    use ::image;
    use egui::epaint::Color32;

    /// 构造 `w`×`h` 的测试图，像素值由坐标唯一确定（便于断言位置）。
    fn gradient(w: usize, h: usize) -> ColorImage {
        let mut img = ColorImage::new([w, h], vec![Color32::BLACK; w * h]);
        for y in 0..h {
            for x in 0..w {
                img.pixels[y * w + x] =
                    Color32::from_rgba_unmultiplied((x % 256) as u8, (y % 256) as u8, 128, 255);
            }
        }
        img
    }

    #[test]
    fn crop_extracts_the_requested_region() {
        let frame = gradient(20, 20);
        let out = crop(&frame, [5, 6, 15, 14]).expect("应成功");
        assert_eq!(out.dimensions(), (10, 8));
        // 左上角 (5,6) 的颜色应与源图同坐标一致。
        let p = out.get_pixel(0, 0);
        assert_eq!((p[0], p[1], p[2], p[3]), (5, 6, 128, 255));
        // 右下角对应源图 (14,13)。
        let p = out.get_pixel(9, 7);
        assert_eq!((p[0], p[1], p[2], p[3]), (14, 13, 128, 255));
    }

    #[test]
    fn crop_of_full_frame_is_identity() {
        let frame = gradient(12, 9);
        let out = crop(&frame, [0, 0, 12, 9]).expect("应成功");
        assert_eq!(out.dimensions(), (12, 9));
        for y in 0..9u32 {
            for x in 0..12u32 {
                assert_eq!(
                    out.get_pixel(x, y).0,
                    frame.pixels[(y * 12 + x) as usize].to_array()
                );
            }
        }
    }

    #[test]
    fn crop_of_single_pixel_works() {
        let frame = gradient(8, 8);
        let out = crop(&frame, [3, 3, 4, 4]).expect("应成功");
        assert_eq!(out.dimensions(), (1, 1));
        assert_eq!(out.get_pixel(0, 0).0, [3, 3, 128, 255]);
    }

    #[test]
    fn clip_exceeding_frame_is_rejected() {
        // 模拟窗口在发命令与回传之间被缩小：右边界超出画面。
        let frame = gradient(20, 20);
        let err = crop(&frame, [10, 10, 40, 20]).expect_err("越界应报错");
        assert_eq!(
            err,
            CaptureError::ClipOutOfBounds {
                requested: [10, 10, 40, 20],
                actual: [20, 20],
            }
        );
    }

    #[test]
    fn zero_area_clip_is_rejected() {
        let frame = gradient(20, 20);
        let err = crop(&frame, [5, 5, 5, 10]).expect_err("零宽应报错");
        assert_eq!(
            err,
            CaptureError::EmptyClip {
                requested: [5, 5, 5, 10]
            }
        );
        let err = crop(&frame, [5, 5, 10, 5]).expect_err("零高应报错");
        assert!(matches!(err, CaptureError::EmptyClip { .. }));
    }

    #[test]
    fn negative_clip_is_rejected() {
        let frame = gradient(20, 20);
        let err = crop(&frame, [-1, 0, 10, 10]).expect_err("负坐标应报错");
        assert!(matches!(err, CaptureError::ClipOutOfBounds { .. }));
    }

    #[test]
    fn oversized_clip_is_rejected_before_allocating() {
        // 上限检查必须发生在分配缓冲**之前**：一张 99999px 宽的裁剪请求
        // 若先分配再检查，会先吃掉数 GB 内存再报错。
        let w = MAX_DIMENSION as usize + 8;
        // 只分配一行像素元数据（内存可控），靠 `size[0]` 让 clip 越界 → 走 TooLarge 分支。
        let frame = ColorImage::new([w, 1], vec![Color32::BLACK; w]);
        let err = crop(&frame, [0, 0, w as i64, 1]).expect_err("超上限应报错");
        match err {
            CaptureError::TooLarge { width, limit, .. } => {
                assert_eq!(width, w as u32);
                assert_eq!(limit, MAX_DIMENSION);
            }
            other => panic!("应报 TooLarge，实际：{other:?}"),
        }
    }

    #[test]
    fn write_png_produces_a_readable_file() {
        let img = gradient(8, 6);
        let rgba = crop(&img, [0, 0, 8, 6]).expect("裁剪应成功");
        let dir = std::env::temp_dir().join("rustmark-capture-test");
        let path = dir.join("out.png");
        let n = write_png(&rgba, &path).expect("写盘应成功");
        assert!(n > 0, "应写出非空文件");
        let decoded = image::open(&path).expect("产出的 PNG 应能被解码");
        assert_eq!(decoded.to_rgba8().dimensions(), (8, 6));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_png_creates_missing_parent_directories() {
        let img = gradient(4, 4);
        let rgba = crop(&img, [0, 0, 4, 4]).expect("裁剪应成功");
        let dir = std::env::temp_dir()
            .join("rustmark-capture-test")
            .join("nested")
            .join("deep");
        let path = dir.join("out.png");
        write_png(&rgba, &path).expect("应自动创建父目录");
        assert!(path.exists(), "父目录应被创建且文件落盘");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_png_to_unwritable_path_reports_io_error() {
        let img = gradient(4, 4);
        let rgba = crop(&img, [0, 0, 4, 4]).expect("裁剪应成功");
        // 用一个已存在的**文件**当父目录，必然失败。
        let blocker = std::env::temp_dir().join("rustmark-capture-blocker");
        std::fs::write(&blocker, b"x").expect("准备阻塞文件");
        let path = blocker.join("out.png");
        let err = write_png(&rgba, &path).expect_err("应报 IO 错误");
        assert!(
            matches!(err, CaptureError::Io { .. }),
            "应归类为 IO 错误：{err:?}"
        );
        let _ = std::fs::remove_file(&blocker);
    }

    #[test]
    fn end_to_end_from_arc_frame_to_png() {
        let frame = Arc::new(gradient(16, 16));
        let dir = std::env::temp_dir().join("rustmark-capture-test");
        let path = dir.join("e2e.png");
        let n = capture_region_to_png(&frame, [4, 4, 12, 12], &path).expect("应成功");
        assert!(n > 0);
        let decoded = image::open(&path).expect("PNG 应可解码");
        assert_eq!(decoded.to_rgba8().dimensions(), (8, 8));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn errors_convert_to_readable_app_errors() {
        let e: AppError = CaptureError::EmptyClip {
            requested: [1, 1, 1, 1],
        }
        .into();
        assert!(e.to_string().contains("宽高为零"), "{e}");
        let e: AppError = CaptureError::TooLarge {
            width: 99999,
            height: 1,
            limit: MAX_DIMENSION,
        }
        .into();
        assert!(e.to_string().contains("超过上限"), "{e}");
        let e: AppError = CaptureError::ClipOutOfBounds {
            requested: [0, 0, 1, 1],
            actual: [0, 0],
        }
        .into();
        assert!(e.to_string().contains("不在画面"), "{e}");
    }
}

#[cfg(test)]
mod stitch_tests {
    use super::*;
    // 见 `tests` 模块：显式指向外部 image crate，避免被同名私有子模块遮蔽。
    use ::image::{self as image, RgbaImage};

    /// 构造单色测试段：每行行号编码进首像素，便于断言拼接位置。
    fn segment(width: u32, height: u32, tag: u8) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([tag, (y % 256) as u8, (x % 256) as u8, 255])
        })
    }

    #[test]
    fn stitch_joins_segments_without_overlap() {
        let parts = [segment(4, 3, 1), segment(4, 2, 2)];
        let out = stitch(&parts, &[0, 0]).expect("应成功");
        assert_eq!(out.dimensions(), (4, 5));
        // 首段最后一行（tag=1, y=2）之上应是 tag=1，之下是 tag=2 的第一行。
        assert_eq!(out.get_pixel(0, 2).0[0], 1);
        assert_eq!(out.get_pixel(0, 3).0[0], 2);
    }

    #[test]
    fn stitch_drops_overlap_rows_at_segment_top() {
        // 长图的末段与上一段重叠 1 行：第二段顶部 1 行必须被丢弃。
        let parts = [segment(4, 3, 1), segment(4, 3, 2)];
        let out = stitch(&parts, &[0, 1]).expect("应成功");
        assert_eq!(out.dimensions(), (4, 5));
        // 丢弃后总高 3 + 2 = 5；第 4 行应是第二段的 y=1。
        assert_eq!(out.get_pixel(0, 3).0[1], 1, "第二段第 0 行应被丢弃");
        assert_eq!(out.get_pixel(0, 4).0[1], 2);
    }

    #[test]
    fn stitch_of_single_segment_is_identity() {
        let parts = [segment(3, 4, 9)];
        let out = stitch(&parts, &[0]).expect("应成功");
        assert_eq!(out.dimensions(), (3, 4));
    }

    #[test]
    fn stitch_rejects_width_mismatch_and_bad_skip() {
        let parts = [segment(4, 2, 1), segment(8, 2, 2)];
        let err = stitch(&parts, &[0, 0]).expect_err("宽度不一致应报错");
        assert!(matches!(err, CaptureError::Io { .. }));

        let parts = [segment(4, 2, 1), segment(4, 2, 2)];
        // skip 等于段高 → 该段零贡献，必是调用方偏移算错。
        let err = stitch(&parts, &[0, 2]).expect_err("skip ≥ 段高应报错");
        assert!(matches!(err, CaptureError::Io { .. }));

        let err = stitch(&[], &[]).expect_err("空分段应报错");
        assert!(matches!(err, CaptureError::Io { .. }));

        let parts = [segment(4, 2, 1)];
        let err = stitch(&parts, &[]).expect_err("参数长度不一致应报错");
        assert!(matches!(err, CaptureError::Io { .. }));
    }

    #[test]
    fn stitch_enforces_the_long_image_height_limit() {
        // 两段各 MAX_LONG_IMAGE_HEIGHT/2+1 行，合计超限。
        let h = MAX_LONG_IMAGE_HEIGHT / 2 + 1;
        let parts = [segment(1, h, 1), segment(1, h, 2)];
        let err = stitch(&parts, &[0, 0]).expect_err("超过长图高度上限应报错");
        match err {
            CaptureError::TooLarge { height, limit, .. } => {
                assert_eq!(height, MAX_LONG_IMAGE_HEIGHT + 2);
                assert_eq!(limit, MAX_LONG_IMAGE_HEIGHT);
            }
            other => panic!("应报 TooLarge，实际：{other:?}"),
        }
    }
}
