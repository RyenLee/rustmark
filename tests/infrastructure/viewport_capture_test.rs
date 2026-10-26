//! 视口截图裁剪与 PNG 落盘的集成测试。
//!
//! # 与模块内单测的分工
//!
//! 单测用**人造** `ColorImage`（坐标即颜色，断言精确到通道值）覆盖裁剪算法的
//! 全部边界；这里负责两件单测做不到的事：
//!
//! 1. **真值校验**——跑一帧真实 egui，断言它确实在预期区域产出了预期颜色的
//!    几何。这是「我们以为屏幕上有什么」与「egui 真的画了什么」之间唯一的接缝，
//!    裁剪的矩形算错了这里就会红。
//! 2. **PNG 无损性**——落盘再解码，逐字节比对，确认编码环节没动手脚。
//!
//! # 诚实说明：像素内容是**建模**出来的，不是真GPU 回读
//!
//! [`capture_region_to_png`] 的输入在真机上来自 wgpu 的 `COPY_SRC` 回读，
//! 无头环境**拿不到** GPU 渲染结果。因此本文件按上面第1 项验到的真实几何
//! （区域矩形 + 颜色）**建模**出一张与之一致的帧，再交给裁剪函数。
//!
//! 这样测的是「给定这张图，裁剪与编码是否正确」，而非「GPU 是否画对了」——
//! 后者由 egui/eframe 保证。若在此伪造一张图再断言图里有别的颜色，
//! 测试就只是在验证自己的假设，故此处不这么做。

use std::sync::Arc;

use egui::epaint::{Color32, Primitive};
use egui::{Pos2, Rect};
use rustmark::infrastructure::export::viewport_capture::{
    CaptureError, MAX_DIMENSION, PortFrame, capture_region_to_png, crop, write_png,
};

/// 画在帧里的两块色块（与 [`modeled_frame`] 严格对应）。
const BASE: Color32 = Color32::from_rgb(12, 34, 56);
/// 覆盖在底色之上的异色块。
const ACCENT: Color32 = Color32::from_rgb(200, 100, 50);

/// 异色块的逻辑矩形。
fn accent_rect() -> Rect {
    Rect::from_min_max(Pos2::new(20.0, 15.0), Pos2::new(60.0, 45.0))
}

/// 跑一帧真实 egui，返回 `tessellate` 产物、整帧像素尺寸与缩放比。
///
/// 只取**几何**不取像素：见文件头的诚实说明。
fn real_frame() -> (Vec<egui::ClippedPrimitive>, [usize; 2], f32) {
    let ctx = egui::Context::default();
    // 第一帧只用于让字库完成一次布局（否则图集为空，文字无处可取）。
    // egui 0.37 的 `FullOutput` 携带未应用的 `TexturesDelta`，直接丢弃会 panic，
    // 故用 `drop_without_applying_deltas()` 显式清空（本用例不需要纹理上传）。
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {});

    let screen = Rect::from_min_max(Pos2::ZERO, Pos2::new(80.0, 60.0));
    let mut output = crate::support::run_ui(
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.painter().rect_filled(screen, 0.0, BASE);
                ui.painter().rect_filled(accent_rect(), 0.0, ACCENT);
                ui.label("hello");
            });
        },
    );
    output.textures_delta.clear();
    let ppp = ctx.input(|i| i.pixels_per_point);
    let prims = ctx.tessellate(output.shapes, ppp);
    let size = [
        (screen.width() * ppp).round() as usize,
        (screen.height() * ppp).round() as usize,
    ];
    (prims, size, ppp)
}

/// 按真实帧的几何**建模**出一张与之一致的整帧图像（端口表示）。
///
/// 逐像素判断点落在哪块色块内——正是 GPU 会做的事（对这两块纯色矩形而言，
/// 软件填充的结果与光栅化结果逐字节相同）。输出为 `PortFrame`：
/// 行优先 RGBA8，与 egui 无关（V11-63 端口化后 `crop` 只认端口字节）。
fn modeled_frame(size: [usize; 2], ppp: f32) -> PortFrame {
    let [w, h] = size;
    let a = accent_rect();
    let mut pixels = vec![BASE.to_array(); w * h];
    for y in 0..h {
        for x in 0..w {
            // 像素中心（与光栅化的采样点约定一致）。
            let p = Pos2::new((x as f32 + 0.5) / ppp, (y as f32 + 0.5) / ppp);
            if a.contains(p) {
                pixels[y * w + x] = ACCENT.to_array();
            }
        }
    }
    PortFrame {
        size: [w, h],
        pixels,
    }
}

/// 断言真实帧的几何里确实出现了预期颜色的顶点，且落在预期区域内。
///
/// 这是本文件的核心断言：裁剪矩形若算错（比如忘了乘 `ppp`、
/// 或用了上一帧的旧矩形），后续基于它的内容断言就会与真实帧脱节。
fn assert_real_frame_geometry(prims: &[egui::ClippedPrimitive], ppp: f32) {
    let a = accent_rect();
    let mut saw_accent = false;
    let mut saw_base = false;
    for prim in prims {
        let Primitive::Mesh(mesh) = &prim.primitive else {
            continue;
        };
        for v in &mesh.vertices {
            if v.color == ACCENT {
                saw_accent = true;
                assert!(
                    a.expand(ppp).contains(v.pos),
                    "异色顶点应落在异色块内，实际在 {:?}",
                    v.pos
                );
            }
            if v.color == BASE {
                saw_base = true;
            }
        }
    }
    assert!(saw_accent, "真实帧应产出异色块的几何");
    assert!(saw_base, "真实帧应产出底色的几何");
}

/// 逻辑矩形 → 物理像素的 `[l, t, r, b]`（与组合根里的换算一致）。
fn px_rect(r: Rect, ppp: f32) -> [i64; 4] {
    [
        (r.min.x * ppp).round() as i64,
        (r.min.y * ppp).round() as i64,
        (r.max.x * ppp).round() as i64,
        (r.max.y * ppp).round() as i64,
    ]
}

#[test]
fn real_frame_geometry_matches_the_modelled_colors() {
    // 真值校验：egui 确实在预期区域画了预期颜色。
    // 少了这一条，下面的内容断言就是在验证「我们以为画面长什么样」，
    // 而模型帧正是按那个假设造的——等于自证。
    let (prims, size, ppp) = real_frame();
    assert_real_frame_geometry(&prims, ppp);
    // 建模帧的尺寸必须与真实帧一致，否则两者不可比。
    let frame = modeled_frame(size, ppp);
    assert_eq!(frame.size, size);
    assert!(
        frame.size[0] >= 60 && frame.size[1] >= 45,
        "视口应容得下异色块：{:?}",
        frame.size
    );
}

#[test]
fn crops_the_requested_region_of_a_real_frame() {
    let (prims, size, ppp) = real_frame();
    assert_real_frame_geometry(&prims, ppp);
    let frame = modeled_frame(size, ppp);
    let clip = px_rect(accent_rect(), ppp);
    let out = crop(&frame, clip).expect("裁剪应成功");

    let expect_w = (40.0 * ppp).round() as u32;
    let expect_h = (30.0 * ppp).round() as u32;
    assert_eq!(out.dimensions(), (expect_w, expect_h));
    // 裁剪区域中心在真实帧里是那块异色矩形。
    let c = out.get_pixel(expect_w / 2, expect_h / 2);
    assert_eq!(c.0, [200, 100, 50, 255], "中心应是被圈住的异色区域");
}

#[test]
fn regions_outside_the_clip_are_excluded() {
    let (prims, size, ppp) = real_frame();
    assert_real_frame_geometry(&prims, ppp);
    let frame = modeled_frame(size, ppp);
    // 只取左上一小块，那里是底色。
    let clip = px_rect(Rect::from_min_max(Pos2::ZERO, Pos2::new(15.0, 10.0)), ppp);
    let out = crop(&frame, clip).expect("裁剪应成功");
    let c = out.get_pixel(out.width() / 2, out.height() / 2);
    assert_eq!(c.0, [12, 34, 56, 255], "左上角应只有底色");
}

#[test]
fn clip_crossing_the_frame_edge_is_rejected() {
    let (_prims, size, ppp) = real_frame();
    let frame = modeled_frame(size, ppp);
    // 模拟窗口缩小：裁剪右边界超出画面。必须报错而不是静默产出残图。
    let w = frame.size[0] as i64;
    let clip = [w - 10, 0, w + 50, 20];
    let err = crop(&frame, clip).expect_err("越界应报错");
    assert!(
        matches!(err, CaptureError::ClipOutOfBounds { .. }),
        "应归类为越界：{err:?}"
    );
}

#[test]
fn realistic_png_round_trips_through_disk() {
    let (prims, size, ppp) = real_frame();
    assert_real_frame_geometry(&prims, ppp);
    let frame = modeled_frame(size, ppp);
    let clip = px_rect(
        Rect::from_min_max(Pos2::new(5.0, 5.0), Pos2::new(70.0, 55.0)),
        ppp,
    );
    let dir = std::env::temp_dir().join("rustmark-viewport-capture-it");
    let path = dir.join("frame.png");

    let bytes = capture_region_to_png(&frame, clip, &path).expect("应成功落盘");
    assert!(bytes > 0);

    let decoded = image::open(&path).expect("PNG 应可解码").to_rgba8();
    let direct = crop(&frame, clip).unwrap();
    assert_eq!(decoded.dimensions(), direct.dimensions());
    // 解码后的像素应与内存中裁出的完全一致（PNG 是无损的）。
    assert_eq!(
        decoded.as_raw(),
        direct.as_raw(),
        "PNG 编解码应无损还原像素"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn transparent_source_regions_stay_transparent_in_the_png() {
    // 全透明来源：裁剪结果必须保持透明（不能被填成黑或白）。
    let frame = Arc::new(PortFrame {
        size: [16, 16],
        pixels: vec![[0, 0, 0, 0]; 256],
    });
    let out = crop(&frame, [4, 4, 12, 12]).expect("裁剪应成功");
    assert!(
        out.pixels().all(|p| p.0[3] == 0),
        "透明来源的每个像素都应保持 alpha=0"
    );
}

#[test]
fn premultiplied_alpha_is_preserved_verbatim() {
    // egui 顶点色是**预乘** alpha。截图是回读，不该也不该由我们做去预乘——
    // 若这里做了转换，PNG 在半透明区域会明显发亮（经典 bug）。
    // 用一个预乘了 50% alpha 的红色验证通道值原样透传。
    let premul = Color32::from_rgba_premultiplied(128, 0, 0, 128);
    let frame = PortFrame {
        size: [4, 4],
        pixels: vec![premul.to_array(); 16],
    };
    let out = crop(&frame, [0, 0, 4, 4]).expect("裁剪应成功");
    let p = out.get_pixel(2, 2);
    assert_eq!(p.0, [128, 0, 0, 128], "预乘值应逐字节透传");
}

#[test]
fn crop_result_is_independent_of_source_ownership() {
    // `frame` 被交出所有权后仍应能裁剪——真机上它是 `Arc<ColorImage>`，
    // 存的是上一帧的引用，生命周期比本次调用长。
    let frame = Arc::new(PortFrame {
        size: [8, 8],
        pixels: vec![[90, 90, 90, 255]; 64],
    });
    let held = Arc::clone(&frame);
    let first = crop(&held, [0, 0, 4, 4]).unwrap();
    drop(held);
    let second = crop(&frame, [0, 0, 4, 4]).unwrap();
    assert_eq!(first.as_raw(), second.as_raw(), "多次裁剪结果应一致");
}

#[test]
fn write_png_reports_the_byte_count_actually_written() {
    // 返回值会显示在「已导出 N 字节」里，必须是**文件真实大小**。
    let frame = PortFrame {
        size: [32, 32],
        pixels: vec![[9, 8, 7, 255]; 1024],
    };
    let img = crop(&frame, [0, 0, 32, 32]).unwrap();
    let path = std::env::temp_dir()
        .join("rustmark-viewport-capture-it")
        .join("bytes.png");
    let reported = write_png(&img, &path).expect("写盘应成功");
    let actual = std::fs::metadata(&path).expect("文件应存在").len() as usize;
    assert_eq!(reported, actual, "上报字节数应等于文件真实大小");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn large_but_legal_clips_are_not_rejected() {
    // 上限是 8192，贴近它的合法尺寸必须放行——否则上限就成了
    // 「4K 屏用户永远导不出图」的暗坑。
    let w = MAX_DIMENSION as usize - 2;
    let frame = PortFrame {
        size: [w, 1],
        pixels: vec![[128, 128, 128, 255]; w],
    };
    let out = crop(&frame, [0, 0, w as i64, 1]).expect("贴近上限的合法尺寸应放行");
    assert_eq!(out.width(), w as u32);
}
