//! 编辑区几何：框（`TextEdit` 自带的 frame）必须撑满编辑面板（问题 047）。
//!
//! # 为什么是几何断言而不是截图比对
//!
//! 用户报的「编辑区域和边框存在差异」不是配色问题，而是**尺寸**问题：
//! `TextEdit` 自带的 frame 尺寸 = 控件尺寸，而控件高度在默认配置下只由
//! `desired_rows(行数)` 决定——短文档下框只包住那几行，框底到状态栏之间
//! 留下一大片裸底色（044 像素取证实测约 220 逻辑像素）。
//!
//! 无头渲染拿不到像素，但**几何量全都可读**：
//!
//! - `Context::read_response(EditorApp::editor_id())` → 那个框的矩形；
//! - `app.ui.sync.geometry(Side::Editor)` → 编辑区视口高 / 内容高。
//!
//! 于是「框 = 视口」「框不随行数变矮」「横滚动条不吃掉视口高」三条都能
//! 确定性断言，不需要截图，也不会因为主题或字体变化而误报。
//!
//! # 为什么要显式推进时间轴
//!
//! egui 的滚动条显隐是 `animate_bool_responsive` 驱动的动画（`current_bar_use`
//! 逐帧插值），而测试帧的墙钟间隔是微秒级——动画永远走不完。不显式给
//! `RawInput::time` 就只能在「滚动条还没长出来」的中间态上取样，
//! 断言会碰巧通过（见 `MEMORY.md` 的测试帧硬规则）。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::state::scroll_sync_state::Side;
use std::fs;
use std::sync::Arc;

/// 测试窗口尺寸（与其余表现层用例一致）。
const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// 造一个已打开 `name` 的应用；文件内容为 `text`。
///
/// 走真实入口（`scan_workspace` + `open_workspace_file`），顺带经过 045 的
/// 文本格式白名单——`.md` 在白名单内，能开。
fn app_with_text(name: &str, text: &str) -> (tempfile::TempDir, tempfile::TempDir, EditorApp) {
    let ws = tempfile::tempdir().expect("工作区临时目录");
    fs::write(ws.path().join(name), text).expect("写样例文档");

    let cfg = tempfile::tempdir().expect("配置临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(cfg.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let root = ws.path().to_string_lossy().replace('\\', "/");
    app.scan_workspace(&root).expect("扫描应成功");
    app.open_workspace_file(&format!("{root}/{name}"))
        .expect("打开应成功");
    (ws, cfg, app)
}

/// 一帧编辑区几何快照。
struct Geom {
    /// `TextEdit` 控件矩形——就是用户看到的那个圆角框。
    frame: egui::Rect,
    /// 编辑区视口高（滚动区 `inner_rect.height()`）。
    viewport_h: f32,
    /// 滚动内容高。
    content_h: f32,
}

/// 把编辑区驱动到稳态（`frames` 帧，按 0.2s/帧推进时间轴），返回几何快照。
///
/// 0.2s/帧远大于 egui 滚动条的动画时间常数（0.1~0.3s 量级），
/// 六帧足以让 `current_bar_use` 收敛。
fn settle_geom(app: &mut EditorApp, wrap: bool, frames: usize) -> Geom {
    let ctx = egui::Context::default();
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {}); // 装载字库
    app.ui
        .set_theme(rustmark::presentation::theme::builtin::Theme::dark());
    // 关掉预览与侧栏：否则量到的是三个面板的合成几何，无法归因到编辑区。
    app.ui.preview_visible = false;
    app.ui.sidebar_visible = false;
    app.editor.word_wrap = wrap;

    for i in 0..frames {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            time: Some(i as f64 * 0.2),
            ..Default::default()
        };
        let _ = crate::support::run_ui(&ctx, input, |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ui);
        });
    }

    let frame = ctx
        .read_response(EditorApp::editor_id())
        .expect("编辑区控件本帧应当存在，否则 geometry 断言无从谈起")
        .rect;
    let g = app.ui.sync.geometry(Side::Editor);
    Geom {
        frame,
        viewport_h: g.viewport_h,
        content_h: g.content_h,
    }
}

/// 短文档：三行。
const SHORT: &str = "第一行\n第二行\n第三行\n";

// ============================================================================
// 1. 框撑满：短文档下框高 = 视口高（修复前 = 文本高，只有几十点）
// ============================================================================

#[test]
fn short_document_frame_fills_the_edit_pane() {
    let (_ws, _cfg, mut app) = app_with_text("short.md", SHORT);
    let g = settle_geom(&mut app, true, 6);

    // 视口高应接近整块编辑面板（窗口 800 减去顶栏 / 状态栏 / 工具栏）。
    // 用下界而不是精确值：面板高度的细节由主题与顶栏决定，不该在此钉死。
    assert!(
        g.viewport_h > 400.0,
        "编辑区视口高应当接近整块面板，实测 {}",
        g.viewport_h
    );
    assert!(
        (g.frame.height() - g.viewport_h).abs() <= 1.0,
        "框高应等于视口高（问题 047）：框 {} vs 视口 {}",
        g.frame.height(),
        g.viewport_h
    );
    // 短文档不该产生可滚动内容——否则会凭空冒出一条竖滚动条。
    assert!(
        g.content_h <= g.viewport_h + 0.5,
        "短文档的内容高不该超过视口（max_offset={}）",
        g.content_h - g.viewport_h
    );
}

// ============================================================================
// 2. 长文档：框仍至少撑满视口（文本放得下，其余靠滚动），且滚动能力未被改坏
// ============================================================================

#[test]
fn long_document_frame_is_at_least_the_viewport_and_still_scrolls() {
    let long: String = (1..=400).map(|i| format!("第{i}行\n")).collect();
    let (_ws, _cfg, mut app) = app_with_text("long.md", &long);
    let g = settle_geom(&mut app, true, 6);

    assert!(
        g.frame.height() >= g.viewport_h - 0.5,
        "长文档的框不应比视口矮：框 {} vs 视口 {}",
        g.frame.height(),
        g.viewport_h
    );
    assert!(
        g.content_h > g.viewport_h,
        "长文档必须仍然可滚（内容 {} vs 视口 {}）",
        g.content_h,
        g.viewport_h
    );
}

// ============================================================================
// 3. 不换行模式下，短文档的横滚动条不该吃掉视口高（044 的第 4 条成因）
// ============================================================================

#[test]
fn nowrap_short_document_does_not_lose_height_to_a_horizontal_bar() {
    let (_a, _b, mut wrapped_app) = app_with_text("short.md", SHORT);
    let wrapped = settle_geom(&mut wrapped_app, true, 6);

    let (_c, _d, mut nowrap_app) = app_with_text("short.md", SHORT);
    let nowrap = settle_geom(&mut nowrap_app, false, 6);

    // 换行模式只有竖滚动条（`ScrollArea::vertical()`），视口高即面板高，
    // 可作为基准。不换行模式下若还有一条横滚动条贴在视口内部，它会占掉
    // `current_bar_use.y`（约 12pt），框就会明显矮一截。
    assert!(
        (nowrap.frame.height() - wrapped.frame.height()).abs() <= 2.0,
        "不换行的短文档不该被横滚动条吃掉视口高：换行 {} vs 不换行 {}",
        wrapped.frame.height(),
        nowrap.frame.height()
    );
}
