//! 回归：自动换行（`word_wrap`）开关必须真正生效，且换行模式下编辑区随窗口宽度重排。
//!
//! # 背景
//!
//! egui 0.33 的 `TextEdit` **没有** `.wrap(bool)` 方法，换行与否完全由
//! `desired_width` + 自定义 `layouter` 决定。此前 `editor_view` 恒用有限
//! `desired_width`，等于「永远换行」，设置 / 状态栏里的「自动换行」开关形同虚设
//! （缺陷：开 = 关）。本文件用「同一份超长文本在两种窗口宽、两种开关状态下，
//! 编辑区内容 galley 的排版宽度」来断言：
//!
//! - 开关开 + 窄窗：排版宽明显小于面板宽（已折行），且窄窗排版宽 < 宽窗排版宽
//!   （随窗口重排）。
//! - 同窗口下：开关 **关** 时的排版宽必须显著大于开关 **开** 时（不再折行）。

use crate::support::fake_dialog::FakeDialog;
use egui::epaint::{ClippedShape, Shape};
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::theme::builtin::Theme;
use rustmark::presentation::views::main_view;
use std::sync::Arc;

fn app_fixture() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

/// 在指定窗口宽下跑一帧，返回全部绘制指令。
fn render_at(app: &mut EditorApp, w: f32) -> Vec<ClippedShape> {
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {}); // 装载字库
    app.ui.set_theme(Theme::dark());
    app.ui.preview_visible = false;
    app.ui.sidebar_visible = false;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(w, 800.0),
        )),
        ..Default::default()
    };
    let out = ctx.run(input, |ctx| {
        rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
        main_view::show(app, ctx);
    });
    out.shapes
}

/// 找到编辑区内容 galley 的排版宽度：唯一包含 `needle` 的那段 `Shape::Text`。
/// （`TextEdit` 把整篇文档作为一个 `Shape::Text` 画出；行号栏等短文本不含该长串。）
fn editor_galley_width(shapes: &[ClippedShape], needle: &str) -> f32 {
    fn walk(shape: &Shape, needle: &str, best: &mut f32) {
        match shape {
            Shape::Text(t) => {
                if t.galley.text().contains(needle) {
                    *best = t.galley.rect.width().max(*best);
                }
            }
            Shape::Vec(v) => {
                for s in v {
                    walk(s, needle, best);
                }
            }
            _ => {}
        }
    }
    let mut best = 0.0_f32;
    for c in shapes {
        walk(&c.shape, needle, &mut best);
    }
    best
}

#[test]
fn wrap_on_reflows_with_window_width() {
    let (dir, mut app) = app_fixture();
    let file = dir.path().join("long.md");
    // 一段超长、无换行的连续文本。
    let long = "长串".repeat(200);
    std::fs::write(&file, &long).expect("写文件应成功");
    app.open_path(&file.to_string_lossy()).expect("打开应成功");

    let needle = &long[0..12]; // 6 个汉字，仅出现在编辑区正文。
    // 基准：同窗口**不折行**的排版宽（长串一行铺开，数千 px）。
    // 阈值不写死像素——编辑区可用宽随布局（如行号栏开关、侧栏比例）变化，
    // 与「不折行」对比才能表达「已折行」的本意且不随布局改动失效。
    app.editor.word_wrap = false;
    let unwrapped = editor_galley_width(&render_at(&mut app, 1280.0), needle);

    app.editor.word_wrap = true;
    let w1280 = editor_galley_width(&render_at(&mut app, 1280.0), needle);
    let w640 = editor_galley_width(&render_at(&mut app, 640.0), needle);

    assert!(
        unwrapped > 2000.0,
        "不折行时长串应铺成超宽单行：{unwrapped}"
    );
    assert!(w1280 > 0.0, "应能在画面上找到编辑区内容 galley");
    // 折行后必须显著小于不折行的排版宽（说明确实折成了多行）。
    assert!(
        w1280 < unwrapped / 2.0,
        "换行开 @1280：排版宽 {w1280} 应远小于不折行的 {unwrapped}（应在窗口内折行）"
    );
    assert!(
        w640 < w1280,
        "较窄窗口排版宽 {w640} 应小于较宽窗口 {w1280}（随窗口重排）"
    );
}

#[test]
fn wrap_off_disables_wrapping() {
    let (dir, mut app) = app_fixture();
    let file = dir.path().join("long.md");
    let long = "长串".repeat(200);
    std::fs::write(&file, &long).expect("写文件应成功");
    app.open_path(&file.to_string_lossy()).expect("打开应成功");
    let needle = &long[0..12];

    // 同窗口（1280）下，对比开关两种状态。
    app.editor.word_wrap = true;
    let on_w = editor_galley_width(&render_at(&mut app, 1280.0), needle);
    app.editor.word_wrap = false;
    let off_w = editor_galley_width(&render_at(&mut app, 1280.0), needle);

    assert!(
        off_w > on_w + 1000.0,
        "关掉自动换行后排版宽应显著增大（{off_w:.0} vs {on_w:.0}）；否则开关无效、依旧折行"
    );
}
