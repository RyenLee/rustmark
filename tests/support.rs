//! 跨测试目标共享的测试替身与工具。
//!
//! 由各测试入口通过 `#[path = "../support.rs"] mod support;` 引入
//! （`tests/` 只认显式 `[[test]]`，故本文件不会被 Cargo 当成独立测试目标）。
//!
//! 子模块用 `#[path]` 显式指向 `tests/support/`：本文件由 `#[path]` 加载，
//! 其子模块会按「本文件所在目录 + 子模块名」查找（即 `tests/clock.rs`），
//! 而不是常规的「文件名目录」（`tests/support/clock.rs`）。

#[path = "support/clock.rs"]
pub mod clock;
#[path = "support/fake_dialog.rs"]
pub mod fake_dialog;
#[path = "support/keymap.rs"]
pub mod keymap;
#[path = "support/text_buffer.rs"]
pub mod text_buffer;

/// 无头驱动一帧 egui，并在返回前清空 `FullOutput` 的 `TexturesDelta`。
///
/// egui 0.37 起 `FullOutput` 携带未应用的 `TexturesDelta`，其 `Drop` 会
/// `debug_assert!` 非空即 panic（"Dropped TexturesDelta with N unapplied deltas"）。
/// 无头测试没有渲染后端来 apply 这些增量，故统一在此清空——
/// 各用例直接调用它即可，不必每处自行处理。
///
/// 语义与 `Context::run_ui` 完全一致，只是多了一步 `textures_delta.clear()`。
///
/// `dead_code` 豁免：本模块被**每个**测试目标用 `#[path]` 引入，但并非每个
/// 目标都会用到这里的每个工具（如 `domain` / `e2e` 不用 `run_ui`）。
#[allow(dead_code)]
pub fn run_ui(
    ctx: &egui::Context,
    input: egui::RawInput,
    ui_fn: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut out = ctx.run_ui(input, ui_fn);
    out.textures_delta.clear();
    out
}

/// 建一个**装了生产字体**的 `egui::Context`。
///
/// REL-03 关掉 egui 的 `default_fonts` 后，`egui::Context::default()` 的两个
/// 族都是空的——文字只会落到「合成豆腐块」，它自带 `0.1 × 字号` 的侧边距、
/// 字宽与基线也和真实字体不同（实测差值 1~2pt）。依赖真实字宽 / 基线的
/// 几何断言因此会误报。生产在 `app/frame.rs` 首帧 `set_fonts`，
/// 无头测试用本函数补齐同一份定义。
#[allow(dead_code)]
pub fn ctx_with_app_fonts() -> egui::Context {
    let ctx = egui::Context::default();
    let setup = rustmark::infrastructure::fonts::font_definitions(None);
    ctx.set_fonts(rustmark::app::frame::to_egui_font_definitions(
        setup.definitions,
    ));
    // 首帧落定字体：`panic = "abort"` 下字体解析失败会直接终止进程。
    let _ = run_ui(&ctx, egui::RawInput::default(), |_| {});
    ctx
}

/// 空跑一帧应用视图，让 egui 的**首帧测量 pass** 落定。
///
/// egui 0.37 起，第一帧量出的控件尺寸与稳定帧不同：实测菜单栏首帧占 43pt、
/// 之后恒为 44pt，连带标签条 `strip_top` 在 54 / 56 之间漂移 2pt。渲染类
/// 断言若取首帧画面，会与后续帧的几何差出 1~2pt——「三条顶栏同一水平线」
/// 「标签同一行」这类 1pt 容差的断言于是稳定误报（且两处取证分别落在不同
/// 帧时，差值直接翻倍）。取画面之前先空跑一帧即可稳定。
///
/// 不带事件：只推进 egui 的测量状态，不改指针 / 键盘输入。
///
/// `dead_code` 豁免理由同 [`run_ui`]。
#[allow(dead_code)]
pub fn settle(ctx: &egui::Context, app: &mut rustmark::app::frame::EditorApp, screen: egui::Vec2) {
    let _ = run_ui(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            ..Default::default()
        },
        |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ui);
        },
    );
}
