//! 侧栏收起/展开（问题 061 引入；问题 063 按钮挪到面包屑行左端）：
//! 默认收起 + 顶栏常驻切换按钮。
//!
//! # 钉住的不变量
//!
//! 1. **默认收起**——首次打开（`AppUiState::new`）侧栏不可见，内容区优先。
//! 2. **按钮常驻面包屑行最左端、与侧栏相邻**——收起后整个侧栏面板（含
//!    活动栏与面板头）都不再渲染，头部入口会消失，按钮必须活在两种状态
//!    都可见的位置；且两种状态下按钮都贴着侧栏的边（展开时贴侧栏右缘、
//!    收起时贴窗口左上角），不随状态跳到窗口另一头（问题 063：右上角
//!    离侧栏太远，不人性化）。
//! 3. **点击即切换**——按钮走 `UiAction::ToggleSidebar` 汇入
//!    `EditorApp::execute`，与 Ctrl+\ / 视图菜单同一路径；连点两次回到
//!    原状态（不是一次性触发）。
//! 4. **展开真的有内容**——展开后活动栏的「文件」导航项出现在左侧，
//!    防止「状态翻了、界面没变」的假开关。
//!
//! 按钮定位：从真帧的绘制指令里取面包屑行（顶栏最后一行）内**最左的
//! 描边圆角矩形**——那是 `Icon::Sidebar` 的外框（行内另一枚描边圆角矩形
//! 是它右侧的文档图标）。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use std::sync::Arc;

const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

fn app_fixture() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let app = app_at(dir.path());
    (dir, app)
}

/// 用**同一份配置目录**再装配一个应用（= 模拟重启）。
fn app_at(config_dir: &std::path::Path) -> EditorApp {
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(config_dir),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    EditorApp::new(services, false)
}

fn raw_input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
        events,
        ..Default::default()
    }
}

/// 干净上下文 + 应用字库（「文件」导航项是 CJK，默认 egui 字库没有）。
fn fresh_ctx() -> egui::Context {
    rustmark::presentation::theme::mapper::clear_highlight_caches();
    let ctx = egui::Context::default();
    let setup = rustmark::infrastructure::fonts::font_definitions(None);
    ctx.set_fonts(setup.definitions);
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {});
    ctx
}

/// 跑一帧并返回原始绘制指令。
fn out_shapes(ctx: &egui::Context, app: &mut EditorApp) -> Vec<egui::epaint::ClippedShape> {
    crate::support::settle(ctx, app, SCREEN);
    let out = crate::support::run_ui(ctx, raw_input(Vec::new()), |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(app, ui);
    });
    out.shapes
}

/// 面包屑行的 y 区间（顶栏最后一行，高 `tabbar_height`）。
fn breadcrumb_band(app: &EditorApp) -> egui::Rect {
    let m = app.theme().metrics;
    let top_h = m.topbar_height + m.tabstrip_height + m.tabbar_height;
    egui::Rect::from_min_max(
        egui::pos2(0.0, top_h - m.tabbar_height),
        egui::pos2(SCREEN.x, top_h),
    )
}

/// 面包屑行内**最左**的描边圆角矩形（= `Icon::Sidebar` 外框；行内另一枚
/// 描边圆角矩形是它右侧的文档图标）。
fn sidebar_button_rect(shapes: &[egui::epaint::ClippedShape], band: egui::Rect) -> egui::Rect {
    fn walk(shape: &egui::epaint::Shape, band: egui::Rect, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::epaint::Shape::Rect(r) => {
                if r.fill.a() == 0 && r.stroke.width > 0.0 && band.contains(r.rect.center()) {
                    out.push(r.rect);
                }
            }
            egui::epaint::Shape::Vec(inner) => {
                for s in inner {
                    walk(s, band, out);
                }
            }
            _ => {}
        }
    }
    let mut rects = Vec::new();
    for clipped in shapes {
        walk(&clipped.shape, band, &mut rects);
    }
    rects.sort_by(|a, b| a.left().total_cmp(&b.left()));
    rects
        .first()
        .copied()
        .expect("面包屑行内应画出侧栏切换按钮（描边外框）")
}

fn press_at(pos: egui::Pos2) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    }
}

fn release_at(pos: egui::Pos2) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    }
}

fn click_at(ctx: &egui::Context, app: &mut EditorApp, pos: egui::Pos2) {
    crate::support::run_ui(
        ctx,
        raw_input(vec![egui::Event::PointerMoved(pos), press_at(pos)]),
        |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ui);
        },
    );
    crate::support::run_ui(
        ctx,
        raw_input(vec![egui::Event::PointerMoved(pos), release_at(pos)]),
        |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ui);
        },
    );
}

/// 屏幕左侧（x < 100）是否存在「文件」文字（限定 y 在 `min_y` 以下，
/// 用于区分活动栏导航项与顶栏菜单）。
fn text_has_files_on_left(out: &egui::FullOutput, min_y: f32) -> bool {
    fn walk(shape: &egui::epaint::Shape, min_y: f32, out: &mut bool) {
        match shape {
            egui::epaint::Shape::Text(t) => {
                if t.galley.text() == "文件" && t.pos.x < 100.0 && t.pos.y > min_y {
                    *out = true;
                }
            }
            egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| walk(s, min_y, out)),
            _ => {}
        }
    }
    let mut hit = false;
    for clipped in &out.shapes {
        walk(&clipped.shape, min_y, &mut hit);
    }
    hit
}

/// 默认收起 + 切换按钮可往返、展开后有真实内容。
#[test]
fn sidebar_defaults_to_collapsed_and_topbar_button_toggles_it() {
    let (_dir, mut app) = app_fixture();
    let ctx = fresh_ctx();

    // ① 默认收起（AppUiState::new 的初始值）。
    assert!(
        !app.ui.sidebar_visible,
        "首次打开侧栏应默认收起（问题 061 用户诉求：内容区优先）"
    );

    // ② 按钮在窗口左上角（问题 063：收起态下面包屑行从窗口左缘起排，
    //    按钮贴行首）：行内最左的描边圆角矩形，右缘应落在窗口左侧 3% 内。
    let band = breadcrumb_band(&app);
    let btn = sidebar_button_rect(&out_shapes(&ctx, &mut app), band);
    assert!(
        btn.right() < SCREEN.x * 0.03,
        "收起状态下侧栏切换按钮应贴窗口左上角，实际左缘 x={:.1}",
        btn.left()
    );

    // ③ 收起状态下左侧没有活动栏（「文件」导航项不在屏幕左侧）。
    let texts = crate::support::run_ui(&ctx, raw_input(Vec::new()), |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(&mut app, ui);
    });
    assert!(
        !text_has_files_on_left(&texts, band.top()),
        "收起状态下活动栏不应渲染（「文件」导航项出现在屏幕左侧）"
    );

    // ④ 点按钮 → 展开；再点 → 收起（可往返，不是一次性触发）。
    let btn = sidebar_button_rect(&out_shapes(&ctx, &mut app), band);
    click_at(&ctx, &mut app, btn.center());
    assert!(
        app.ui.sidebar_visible,
        "点击按钮应展开侧栏（与 Ctrl+\\ / 视图菜单同路径）"
    );

    // ⑤ 展开真的有内容：活动栏「文件」导航项出现在屏幕左侧。
    let texts = crate::support::run_ui(&ctx, raw_input(Vec::new()), |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(&mut app, ui);
    });
    assert!(
        text_has_files_on_left(&texts, 0.0),
        "展开后活动栏应渲染「文件」导航项"
    );

    // ⑤' 展开态按钮紧贴侧栏右缘（问题 063 的核心诉求）：展开下面包屑行
    //    从侧栏右侧起排，按钮应贴行首、落在窗口左侧 1/4 内——而不是
    //    061 之前那样孤悬在窗口右上角。
    let btn = sidebar_button_rect(&out_shapes(&ctx, &mut app), band);
    assert!(
        btn.right() < SCREEN.x * 0.25,
        "展开状态下侧栏切换按钮应紧贴侧栏右缘，实际左缘 x={:.1}",
        btn.left()
    );

    let btn = sidebar_button_rect(&out_shapes(&ctx, &mut app), band);
    click_at(&ctx, &mut app, btn.center());
    assert!(
        !app.ui.sidebar_visible,
        "再次点击按钮应收起侧栏（按钮是收起/展开双向切换）"
    );
}

/// 开合状态**跨重启保留**（问题 062）：点按钮展开 → 帧末落盘 → 用同一份
/// 配置重新装配的应用仍是展开；反过来收起后重启仍是收起。
///
/// 刻意走「真帧点击 → 帧末落盘」而不是直接调持久化方法：
/// 「落盘发生在帧末」正是这条需求最容易漏的一环——点击发生在顶栏绘制
/// 阶段，写盘点若排在它之前，用户点完立刻关窗口就丢了。真帧跑一遍才能
/// 钉住这个时序。
#[test]
fn sidebar_visibility_is_remembered_across_restarts() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let mut app = app_at(dir.path());
    let ctx = fresh_ctx();

    // ① 展开（真帧点击切换按钮）。
    let band = breadcrumb_band(&app);
    let btn = sidebar_button_rect(&out_shapes(&ctx, &mut app), band);
    click_at(&ctx, &mut app, btn.center());
    assert!(app.ui.sidebar_visible, "点击后应展开");
    // ② 再画一帧：落盘发生在这一帧的末尾。
    out_shapes(&ctx, &mut app);
    drop(app);

    // ③ 重启：上次是展开，启动就该是展开。
    let restarted = app_at(dir.path());
    assert!(
        restarted.ui.sidebar_visible,
        "上次展开过，重启后应仍是展开（记住上次的开合状态）"
    );

    // ④ 反向同样成立：收起后重启仍是收起（不是「只记得展开」）。
    let mut reopened = app_at(dir.path());
    reopened.ui.toggle_sidebar();
    assert!(!reopened.ui.sidebar_visible);
    let ctx2 = fresh_ctx();
    let _ = out_shapes(&ctx2, &mut reopened);
    drop(reopened);

    let collapsed = app_at(dir.path());
    assert!(
        !collapsed.ui.sidebar_visible,
        "上次收起过，重启后应仍是收起"
    );
}
