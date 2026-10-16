//! 跨测试目标共享的测试替身与工具。
//!
//! 由各测试入口通过 `#[path = "../support/mod.rs"] mod support;` 引入
//! （`tests/support/` 下没有 `main.rs`，故不会被 Cargo 当成独立测试目标）。

pub mod clock;
pub mod fake_dialog;
pub mod keymap;
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
