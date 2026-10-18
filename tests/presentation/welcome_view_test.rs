//! 欢迎页「最近文件」的**呈现契约**回归（用户 2026-10-18 要求：
//! 「倒序显示 5 条就行」）。
//!
//! # 为什么这条契约要钉在呈现层
//!
//! 领域层（[`RecentFiles`]）早已保证「`last_opened` 降序、最新在前」，
//! 域内也有对应用例（`recent_files_dedup_and_order_by_recency`）。但
//! **用户看到的是欢迎页**：从领域到屏幕之间还有三段——`list_recent`
//! 映射、`take(MAX_RECENT)` 截断、渲染顺序。任何一段被改坏
//! （截断数被调大、渲染时 `rev()`、DTO 映射时排序丢失），域内用例
//! 全绿而用户看到的是另一个列表。故本文件跑**真帧**，把
//! 「恰好 5 行、最新在上、更早的进页脚」钉在输出形状上。
//!
//! # 「倒序」的语义
//!
//! 中文语境里时间列表的「倒序」= **新的在上**（与领域层降序一致）。
//! 若用户要的是相反方向（最旧在上），应改渲染处而非这里——
//! 本用例钉的是当前契约。
//!
//! [`RecentFiles`]: rustmark::domain::workspace::value_objects::RecentFiles

use std::sync::Arc;

use crate::support::clock::TestClock;
use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::workspace::commands::TouchRecentCommand;
use rustmark::shared::path::FilePath;

/// 验收窗口尺寸（与 `top_menu_test` / `theme_mapper_test` 一致）。
const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// 注入的最近文件总数：**超过**展示上限，才能验到「截断」与「页脚」。
const TOTAL: usize = 7;

/// 期望展示的行（按从上到下）：最新的 5 条。
///
/// `r{ TOTAL - 1 }.md` 是最后 touch 的（最新），应排第一行；
/// 最早的两条（`r0` / `r1`）被截断，一行都不出现。
fn expected_rows() -> Vec<String> {
    (2..TOTAL).rev().map(|i| format!("r{i}.md")).collect()
}

// ============================================================================
// 装置
// ============================================================================

/// 造一个注满 `TOTAL` 条最近文件的应用。
///
/// 时钟用 [`TestClock`] 且**每条之间显式推进**：真实时钟毫秒分辨率下
/// 连 touch 多条可能落在同一毫秒，`sort_by_key` 是稳定排序，同值条目
/// 会保持插入顺序——「最新在上」就测不出来了（测出一个碰巧的顺序）。
fn app_with_recent() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let clock = Arc::new(TestClock::default());
    let services = Services::assemble_with_clock_and_dialogs(
        ConfigPaths::in_dir(dir.path()),
        clock.clone(),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let app = EditorApp::new(services, false);
    for i in 0..TOTAL {
        let file = dir.path().join(format!("r{i}.md"));
        std::fs::write(&file, "# r\n").expect("写文件应成功");
        // 推进 1s：时间戳严格递增，排序结果与插入意图一一对应。
        clock.advance(1000);
        app.services
            .workspace
            .touch_recent(TouchRecentCommand {
                path: FilePath::new(file.to_string_lossy().replace('\\', "/")).expect("路径应合法"),
            })
            .expect("应可登记");
    }
    (dir, app)
}

/// 装上与真实应用**同一套**字体（含系统 CJK 兜底）的上下文。
///
/// 直接 `Context::default()` 只有 egui 自带的西文字体，文字形状的
/// 测量结果与本机真实运行不一致（与 `toolbar_visual_test` 同理由）。
fn ctx_with_app_fonts() -> egui::Context {
    let ctx = egui::Context::default();
    let setup = rustmark::infrastructure::fonts::font_definitions(None);
    ctx.set_fonts(setup.definitions);
    crate::support::run_ui(&ctx, raw_input(Vec::new()), |_| {});
    ctx
}

fn raw_input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
        events,
        ..Default::default()
    }
}

/// 跑三帧主界面，取回最后一帧的全部**文字**（矩形 + 内容）。
///
/// 三帧：`ScrollArea` / 面板首帧走 sizing pass，那一帧不下发绘制指令
/// （与 `toolbar_visual_test::render` 同理由）。
fn render_texts(ctx: &egui::Context, app: &mut EditorApp) -> Vec<(egui::Rect, String)> {
    let mut texts = Vec::new();
    for _ in 0..3 {
        texts.clear();
        let out = crate::support::run_ui(ctx, raw_input(Vec::new()), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ui);
        });
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut texts);
        }
    }
    texts
}

fn walk(shape: &egui::epaint::Shape, texts: &mut Vec<(egui::Rect, String)>) {
    match shape {
        egui::epaint::Shape::Text(x) => {
            texts.push((x.visual_bounding_rect(), x.galley.text().to_string()));
        }
        egui::epaint::Shape::Vec(v) => {
            for s in v {
                walk(s, texts);
            }
        }
        _ => {}
    }
}

/// 从一帧文字里按**文件名**挑出最近文件的行，按阅读顺序（上到下）返回。
///
/// 只认**完整等于**文件名的文字：目录文字是父路径（长串），
/// 状态栏 / 菜单里也不会恰好出现 `r3.md` 这种串，不会误配。
fn recent_rows(texts: &[(egui::Rect, String)]) -> Vec<String> {
    let names: Vec<String> = (0..TOTAL).map(|i| format!("r{i}.md")).collect();
    let mut rows: Vec<(egui::Rect, String)> = texts
        .iter()
        .filter(|(_, t)| names.contains(t))
        .cloned()
        .collect();
    // 形状顺序 = 绘制顺序，垂直列表里就是阅读顺序；仍按 (y, x) 排一次，
    // 免得未来绘制顺序变化让用例假红。
    rows.sort_by(|a, b| {
        a.0.min
            .y
            .partial_cmp(&b.0.min.y)
            .unwrap()
            .then(a.0.min.x.partial_cmp(&b.0.min.x).unwrap())
    });
    rows.into_iter().map(|(_, t)| t).collect()
}

// ============================================================================
// 用例
// ============================================================================

#[test]
fn welcome_recent_shows_exactly_five_rows() {
    let (_dir, mut app) = app_with_recent();
    // 侧栏文件树降噪（欢迎页占满主体区，侧栏开与否不影响本断言）。
    app.ui.sidebar_visible = false;
    let ctx = ctx_with_app_fonts();
    let texts = render_texts(&ctx, &mut app);
    let rows = recent_rows(&texts);
    assert_eq!(rows.len(), 5, "最近文件应恰好展示 5 行，实际 {:?}", rows);
}

#[test]
fn welcome_recent_is_newest_first() {
    let (_dir, mut app) = app_with_recent();
    app.ui.sidebar_visible = false;
    let ctx = ctx_with_app_fonts();
    let texts = render_texts(&ctx, &mut app);
    let rows = recent_rows(&texts);
    assert_eq!(
        rows,
        expected_rows(),
        "最近文件应按时间倒序（最新在上）展示"
    );
}

#[test]
fn welcome_recent_older_tail_goes_to_footer() {
    let (_dir, mut app) = app_with_recent();
    app.ui.sidebar_visible = false;
    let ctx = ctx_with_app_fonts();
    let texts = render_texts(&ctx, &mut app);
    let rows = recent_rows(&texts);
    // 被截掉的最早两条一行都不出现。
    assert!(
        !rows.iter().any(|r| r == "r0.md" || r == "r1.md"),
        "更早的文件不应占行，实际 {:?}",
        rows
    );
    // 页脚如实报数：总数 − 展示数。
    let footer = format!("另有 {} 个更早的文件", TOTAL - 5);
    assert!(
        texts.iter().any(|(_, t)| *t == footer),
        "应显示「{}」，实际文字：{:?}",
        footer,
        texts.iter().map(|(_, t)| t).collect::<Vec<_>>()
    );
}
