//! 回归测试：浅色模式下侧栏 / 编辑区边界不应出现贯穿整高的黑色竖线（#039）。
//!
//! 根因（与 025/033/037 同源）：相邻 `SidePanel` 在分数缩放（如 125% DPI）下因
//! 像素取整在边界留下亚像素缝隙，缝隙无任何面板绘制，露出交换链清屏色（黑）。
//! 深色主题下缝隙同为黑、与深色面板浑然一体，故只在浅色模式被察觉。
//! 修复手段：在 `main_view::render_split` 起始处先把整块主体区铺一层 `bg_secondary`
//! （最底层），任何亚像素缝隙都会落回浅灰而非黑。
//!
//! 本测试在 ppp=1.3（让侧栏宽 308 逻辑点落到小数物理像素 400.4）下无头渲染，
//! 扫描侧栏/编辑区边界附近的每个整数列，断言其在内容高度上「被某个形状覆盖」——
//! 若存在一列在顶部与底部都未被覆盖，即等价于一条贯穿整高的黑缝，判为失败。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::state::app_ui_state::SidebarTab;
use rustmark::presentation::theme::builtin::Theme;
use rustmark::presentation::theme::mapper;
use rustmark::presentation::views::main_view;
use std::sync::Arc;

const SCREEN: egui::Vec2 = egui::vec2(527.0, 990.0);
const PPP: f32 = 1.3;

#[test]
fn sidebar_editor_boundary_has_no_black_gap_at_fractional_scale() {
    let dir = tempfile::tempdir().expect("临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配");
    let mut app = EditorApp::new(services, false);
    let path = dir.path().join("doc.md");
    std::fs::write(&path, "# 标题一\n\n正文\n\n## 标题二\n\n更多\n").expect("写");
    app.open_path(&path.to_string_lossy()).expect("打开");

    // 浅色主题 + 大纲标签 + 预览关闭（与用户报告场景一致）。
    app.ui.set_theme(Theme::light());
    app.ui.sidebar_tab = SidebarTab::Outline;
    app.ui.preview_visible = false;
    app.editor.line_numbers = true;

    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(PPP);
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {});
    mapper::clear_highlight_caches();

    // 跑几帧让布局/Window 稳定，并保留最后一帧的形状。
    let mut out = Vec::new();
    for _ in 0..4 {
        out = crate::support::run_ui(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
                ..Default::default()
            },
            |ui| {
                mapper::apply(ui.ctx(), &app.ui.theme);
                main_view::show(&mut app, ui);
            },
        )
        .shapes;
    }

    // 收集所有矩形与竖向线段（物理像素坐标）。
    let mut rects: Vec<(f32, f32, f32, f32)> = Vec::new();
    let mut vlines: Vec<(f32, f32, f32)> = Vec::new();
    for clipped in &out {
        walk(clipped, &mut rects, &mut vlines);
    }

    // 侧栏宽 308 逻辑点 → 物理 400.4；扫描其邻域。
    let boundary = 308.0 * PPP;
    let scan_min = (boundary - 15.0).floor() as i32;
    let scan_max = (boundary + 15.0).ceil() as i32;
    let y_top = 150.0;
    let y_bottom = 880.0;

    let mut gap_columns = Vec::new();
    for x in scan_min..=scan_max {
        let xf = x as f32;
        let covered_top = column_covered(xf, y_top, &rects, &vlines);
        let covered_bottom = column_covered(xf, y_bottom, &rects, &vlines);
        // 一条贯穿整高的黑缝：顶部与底部同一列都未被任何形状覆盖。
        if !covered_top && !covered_bottom {
            gap_columns.push(x);
        }
    }

    assert!(
        gap_columns.is_empty(),
        "侧栏/编辑区边界检测到贯穿整高的未覆盖列（黑缝）：{:?}（边界物理坐标 {boundary}）",
        gap_columns
    );
}

fn column_covered(
    x: f32,
    y: f32,
    rects: &[(f32, f32, f32, f32)],
    vlines: &[(f32, f32, f32)],
) -> bool {
    for (x0, x1, y0, y1) in rects {
        if x >= *x0 && x < *x1 && y >= *y0 && y <= *y1 {
            return true;
        }
    }
    for (sx, sy0, sy1) in vlines {
        if (sx - x).abs() < 0.5 && y >= *sy0 && y <= *sy1 {
            return true;
        }
    }
    false
}

fn walk(
    clipped: &egui::epaint::ClippedShape,
    rects: &mut Vec<(f32, f32, f32, f32)>,
    vlines: &mut Vec<(f32, f32, f32)>,
) {
    match &clipped.shape {
        egui::epaint::Shape::Rect(r) => {
            let q = r.rect;
            rects.push((q.min.x, q.max.x, q.min.y, q.max.y));
        }
        egui::epaint::Shape::Vec(v) => {
            for s in v {
                walk(
                    &egui::epaint::ClippedShape::new(clipped.clip_rect, s.clone()),
                    rects,
                    vlines,
                );
            }
        }
        egui::epaint::Shape::LineSegment { points, stroke: _ }
            if (points[0].x - points[1].x).abs() < 0.01 =>
        {
            vlines.push((points[0].x, points[0].y, points[1].y));
        }
        _ => {}
    }
}
