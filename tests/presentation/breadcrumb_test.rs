//! 面包屑（顶栏「文件路径」一行）的**截断**回归（问题 059）。
//!
//! # 症状与根因
//!
//! 用户截图实证：窄窗口下路径文字一路延伸出面板右缘被**硬切**
//! （`logs\问题记录1.txt` 只剩残形）。两层根因：
//!
//! 1. `draw_breadcrumb` 用裸 `ui.label(path)`——横向布局里长文本不截断；
//! 2. 就算给标签挂 `.truncate()` 也**不够**：put/布局用的「环境可用宽」
//!    本身已被污染——egui 的 `Placer::expand_to_include_rect` 把 Ui 的
//!    min_rect **与 max_rect 一起**扩，同面板里更早的超宽内容（菜单栏、
//!    标签条横向溢出）把面板 Ui 的 max_rect 连带撑出窗口（实测 360pt
//!    窗口下漂到 440）。
//!
//! 修法：`ui.put` 显式矩形 + `truncate()`，矩形右界按 `clip_rect`
//! （不随内容膨胀的唯一稳定面板界）对称推出。
//!
//! # 钉住的不变量
//!
//! 1. **窄窗口：截断而不是硬切**——面包屑 galley 标记 `elided`（末尾
//!    补省略号），绘制矩形右缘不超窗口右缘。
//! 2. **宽窗口：不误伤**——路径放得下时完整显示且不 elide。
//! 3. **语义不丢**——elide 只发生在字形层，galley 全文与悬停提示仍是
//!    完整路径（egui `show_tooltip_when_elided` 走全文）。
//!
//! 两例都跑**真帧**、从 `FullOutput::shapes` 里取面包屑行（顶栏最后
//! 一行，高 `tabbar_height`）内的文字图元来断言——从领域到屏幕之间的
//! 渲染行为只能靠几何验。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use std::sync::Arc;

/// 窄窗口宽度：足以容纳菜单栏与标签条，但远装不下超长路径。
const NARROW_W: f32 = 360.0;
/// 宽窗口宽度：与常规视觉用例一致，短路径必须完整放下。
const WIDE_W: f32 = 1280.0;
const SCREEN_H: f32 = 800.0;

// ============================================================================
// 基础设施（与 `toolbar_visual_test` 同款手法，只保留本文件需要的部分）
// ============================================================================

/// 造一个打开了**超长路径**文档的应用。
///
/// 目录用 CJK 名字叠 15 层（Windows `MAX_PATH` 260 内、但渲染宽度远超
/// 窄窗口），保证窄窗口下必然触发截断。
fn app_with_long_path_doc() -> (tempfile::TempDir, EditorApp, String) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let long_dir = dir.path().join("超长目录名".repeat(15));
    std::fs::create_dir_all(&long_dir).expect("应可创建长名目录");
    let file = long_dir.join("问题记录.md");
    std::fs::write(&file, "# t\n").expect("写文件应成功");
    let path = file.to_string_lossy().replace('\\', "/");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    app.open_path(&path).expect("打开应成功");
    (dir, app, path)
}

/// 造一个打开了**短路径**文档的应用（临时目录默认层级，宽度足够放下）。
fn app_with_short_path_doc() -> (tempfile::TempDir, EditorApp, String) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let file = dir.path().join("note.md");
    std::fs::write(&file, "# t\n").expect("写文件应成功");
    let path = file.to_string_lossy().replace('\\', "/");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    app.open_path(&path).expect("打开应成功");
    (dir, app, path)
}

fn ctx_with_app_fonts() -> egui::Context {
    let ctx = egui::Context::default();
    let setup = rustmark::infrastructure::fonts::font_definitions(None);
    ctx.set_fonts(setup.definitions);
    // 首帧落定字体（`panic = "abort"` 下字体解析失败会直接终止）。
    crate::support::run_ui(&ctx, raw_input(WIDE_W), |_| {});
    ctx
}

fn raw_input(w: f32) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w, SCREEN_H),
        )),
        ..Default::default()
    }
}

/// 一段文字图元：绘制矩形、galley 全文、是否被 elide。
type TextShape = (egui::Rect, String, bool);

/// 从一帧里取回全部文字图元。
///
/// 多跑几帧：首帧是测量 pass，不下发绘制指令。
fn render_texts(ctx: &egui::Context, app: &mut EditorApp, w: f32) -> Vec<TextShape> {
    let mut texts = Vec::new();
    for _ in 0..3 {
        texts.clear();
        let out = crate::support::run_ui(ctx, raw_input(w), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ui);
        });
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut texts);
        }
    }
    texts
}

fn walk(shape: &egui::epaint::Shape, texts: &mut Vec<TextShape>) {
    match shape {
        egui::epaint::Shape::Text(x) => texts.push((
            x.visual_bounding_rect(),
            x.galley.text().to_string(),
            x.galley.elided,
        )),
        egui::epaint::Shape::Vec(v) => {
            for s in v {
                walk(s, texts);
            }
        }
        _ => {}
    }
}

/// 挑出面包屑行内的文字：顶栏最后一行，高 `tabbar_height`。
fn breadcrumb_texts(texts: &[TextShape], app: &EditorApp) -> Vec<TextShape> {
    let m = app.theme().metrics;
    let top_limit = m.topbar_height + m.tabstrip_height + m.tabbar_height;
    let band_top = top_limit - m.tabbar_height;
    texts
        .iter()
        .filter(|(r, _, _)| r.top() >= band_top && r.top() < top_limit)
        .cloned()
        .collect()
}

// ============================================================================
// 用例
// ============================================================================

/// 窄窗口：超长路径必须**截断**（elide + 省略号，右缘不出窗口），不能硬切。
#[test]
fn long_path_is_truncated_within_window() {
    let (_dir, mut app, path) = app_with_long_path_doc();
    let ctx = ctx_with_app_fonts();
    let texts = render_texts(&ctx, &mut app, NARROW_W);
    let crumbs = breadcrumb_texts(&texts, &app);

    assert_eq!(
        crumbs.len(),
        1,
        "面包屑行应恰有一段文字（图标是向量图元不计）\
         ——多段说明发生了换行/溢出，实际：{crumbs:?}"
    );
    let (rect, galley_text, elided) = &crumbs[0];

    let panel_bottom = app.theme().metrics.topbar_height
        + app.theme().metrics.tabstrip_height
        + app.theme().metrics.tabbar_height;
    assert!(
        rect.right() <= NARROW_W + 0.5,
        "面包crumb 文字右缘 {} 超出窗口右缘 {NARROW_W}——路径没有被截断，\
         正沿横向布局延伸出面板被硬切（问题 059 症状）",
        rect.right()
    );
    assert!(
        rect.bottom() <= panel_bottom + 0.5,
        "面包屑文字底部 {} 越出面板底缘 {panel_bottom}——文字被竖向裁切\
         （问题 060 症状：原 `ui.horizontal` 交叉轴居中，窄带里 15pt 文字\
         居中后底部探出 1pt）",
        rect.bottom()
    );
    assert!(elided, "窄窗口下 galley 应标记 elided（末尾补省略号）");
    // elide 只发生在字形层：galley 全文仍是完整路径（无障碍 / 悬停提示语义不丢）。
    assert_eq!(*galley_text, path, "galley 全文应保留完整路径");
}

/// 宽窗口：短路径必须**完整显示**（不误伤）。
#[test]
fn short_path_shows_in_full_when_it_fits() {
    let (_dir, mut app, path) = app_with_short_path_doc();
    let ctx = ctx_with_app_fonts();
    let texts = render_texts(&ctx, &mut app, WIDE_W);
    let crumbs = breadcrumb_texts(&texts, &app);

    assert_eq!(crumbs.len(), 1, "面包屑行应恰有一段文字，实际：{crumbs:?}");
    let (rect, galley_text, elided) = &crumbs[0];
    assert!(!elided, "路径放得下时不应 elide");
    let panel_bottom = app.theme().metrics.topbar_height
        + app.theme().metrics.tabstrip_height
        + app.theme().metrics.tabbar_height;
    assert!(
        rect.bottom() <= panel_bottom + 0.5,
        "面包屑文字底部 {} 越出面板底缘 {panel_bottom}（问题 060）",
        rect.bottom()
    );
    assert_eq!(
        *galley_text, path,
        "路径放得下时应完整显示（含文件名），实际：{galley_text:?}"
    );
}
