//! S8-06：预览防抖与解析性能（FR-01 / NFR-PERF-06）。
//!
//! # 验收口径
//!
//! 「连续输入 20 字符期间重解析次数 ≤ 2」——用 headless 驱动**真实**
//! `EditorApp::update` 帧循环来数，而不是单独测 `PreviewUiState` 的纯函数。
//! 理由：防抖要生效，依赖「编辑时打时间戳 → 渲染时判定」这条链路被正确接线；
//! 只测纯函数会漏掉「根本没人打时间戳」这类接线缺陷。
//!
//! # 为什么必须 headless 驱动真实帧
//!
//! eframe 为 kittest 提供了 `Frame::_new_kittest()`（无窗口、无 GPU 的空 `Frame`），
//! 而 `EditorApp::update` 的 `frame` 形参本就是 `_frame`（未使用），
//! 故可直接 `ctx.run(input, |ctx| eframe::App::update(app, ctx, frame))` 驱动。
//! 复用 S7-06 `crash_recovery_test` 已验证的做法，不引入任何新依赖。

use std::sync::Arc;
use std::time::Duration;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::state::preview_ui_state::PREVIEW_DEBOUNCE_MS;

use crate::support::fake_dialog::FakeDialog;

/// 有明确尺寸的 `RawInput`：`screen_rect` 为 `None` 时 egui 会退回默认巨幅布局。
fn frame_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    }
}

/// 驱动**一帧**真实 `update`。
fn drive(app: &mut EditorApp, ctx: &egui::Context, frame: &mut eframe::Frame) {
    let _ = ctx.run(frame_input(), |ctx| {
        eframe::App::update(app, ctx, frame);
    });
}

/// 起一个干净的 `EditorApp`（独立配置目录，不污染用户目录）。
fn app_in(dir: &std::path::Path) -> EditorApp {
    let services =
        Services::assemble_with_dialogs(ConfigPaths::in_dir(dir), Arc::new(FakeDialog::new()))
            .expect("装配应成功");
    EditorApp::new(services, false)
}

/// **验收核心**：连续输入 20 个字符，预览重解析次数 ≤ 2。
///
/// 输入节奏取 60ms/字符（约 200 CPM，比日常打字略快），总计 1.2s——
/// 是防抖间隔（150ms）的 8 倍。若实现是「节流」（每 150ms 一次）而非
/// 「防抖」（静默 150ms 才一次），本例会数到 8 次左右而变红。
#[test]
fn continuous_typing_triggers_at_most_two_reparses() {
    let dir = tempfile::tempdir().expect("临时目录应可建");
    let mut app = app_in(dir.path());
    let ctx = egui::Context::default();
    let mut frame = eframe::Frame::_new_kittest();

    // 先建一个文档：无活动文档时主视图走欢迎页分支，预览区根本不渲染。
    app.new_document().expect("新建应成功");
    // 建立基线：驱动两帧（`ensure_document` 在编辑区首帧才跑）。
    drive(&mut app, &ctx, &mut frame);
    drive(&mut app, &ctx, &mut frame);
    let baseline = app.preview.render_count;
    assert!(baseline >= 1, "建出活动文档后应完成一次渲染（基线）");

    // 连续输入 20 个字符，每个间隔 60ms（远小于防抖间隔 150ms）。
    for i in 0..20 {
        app.insert_at_cursor(&format!("字{i}")).expect("插入应成功");
        drive(&mut app, &ctx, &mut frame);
        std::thread::sleep(Duration::from_millis(60));
    }
    let during = app.preview.render_count - baseline;

    // 停手后等防抖窗口过完，再驱动一帧：此刻**应当**补一次渲染。
    std::thread::sleep(Duration::from_millis(PREVIEW_DEBOUNCE_MS + 80));
    drive(&mut app, &ctx, &mut frame);
    let after = app.preview.render_count - baseline;

    println!(
        "[metrics] 连续输入 20 字符（60ms/字符，共 ~1.2s）：输入期间重解析 {during} 次 \
         ｜ 停顿后累计 {after} 次（防抖 {PREVIEW_DEBOUNCE_MS}ms）"
    );

    assert!(
        during <= 2,
        "连续输入期间重解析 {during} 次，超出验收上限 2 次（防抖 {PREVIEW_DEBOUNCE_MS}ms）"
    );
    assert!(
        after > during,
        "停顿超过防抖窗口后应补一次渲染（输入期间 {during} → 停顿后 {after}）"
    );
}

/// 单次编辑后，预览**最终**会更新（防抖不能把更新吞掉）。
#[test]
fn debounce_eventually_renders_after_idle() {
    let dir = tempfile::tempdir().expect("临时目录应可建");
    let mut app = app_in(dir.path());
    let ctx = egui::Context::default();
    let mut frame = eframe::Frame::_new_kittest();

    app.new_document().expect("新建应成功");
    drive(&mut app, &ctx, &mut frame);
    drive(&mut app, &ctx, &mut frame);
    let baseline = app.preview.render_count;

    app.insert_at_cursor("# 新标题").expect("插入应成功");
    drive(&mut app, &ctx, &mut frame);
    // 防抖窗口过后再驱动一帧。
    std::thread::sleep(Duration::from_millis(PREVIEW_DEBOUNCE_MS + 80));
    drive(&mut app, &ctx, &mut frame);

    assert!(
        app.preview.render_count > baseline,
        "静默超过防抖窗口后必须补一次渲染，否则编辑结果永远看不到"
    );
    assert!(
        app.preview
            .blocks
            .iter()
            .any(|b| b.content.contains("新标题")),
        "预览块内容应反映最新编辑"
    );
}
