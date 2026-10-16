//! 编辑区渲染用例（回归：打开文件后编辑区必须真的把文本画出来）。
//!
//! # 为什么必须断言「画面上有字形」而不是「数据结构里有文本」
//!
//! `editor_text`、领域内容、行号栏三者都可能正确，而编辑区仍然一片空白——
//! 只要 `TextEdit` 拿到的排版结果没被真正提交成绘制指令。这类缺陷
//! 在数据结构层面**完全看不出来**，只能从本帧的 `FullOutput::shapes` 里
//! 找字形。本文件因此直接遍历绘制指令，断言文档正文出现在画面上。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use std::sync::Arc;

/// 把一帧的全部绘制指令里的文本拼起来（用于「画面上有没有这段字」）。
fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> String {
    fn walk(shape: &egui::epaint::Shape, out: &mut String) {
        match shape {
            egui::epaint::Shape::Text(text) => {
                out.push_str(text.galley.text());
                out.push('\n');
            }
            egui::epaint::Shape::Vec(inner) => {
                for s in inner {
                    walk(s, out);
                }
            }
            _ => {}
        }
    }
    let mut out = String::new();
    for clipped in shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// 造一个装好默认配置的应用（不打开任何文档）。
fn app_fixture() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let app = EditorApp::new(services, false);
    (dir, app)
}

/// 跑一帧，返回本帧画出来的全部文本。
///
/// # 为什么要关掉预览区与侧边栏
///
/// 预览区渲染的是**同一份** Markdown，正文必然也在其中出现。
/// 若让它开着，`painted.contains("article_id")` 会被预览区「代答」，
/// 编辑区即便一个字都不画，用例照样绿——这正是本文件要防的那个假阳性
/// （编辑区因 `Galley::is_empty()` 不落笔时，旧版本用例就是这么被骗过去的）。
/// 关掉预览 / 侧栏后，画面上的正文**只可能**来自编辑区。
fn run_frame(app: &mut EditorApp, dark: bool) -> String {
    let ctx = egui::Context::default();
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {}); // 装载字库
    let theme = if dark {
        rustmark::presentation::theme::builtin::Theme::dark()
    } else {
        rustmark::presentation::theme::builtin::Theme::light()
    };
    app.ui.set_theme(theme);
    app.ui.preview_visible = false;
    app.ui.sidebar_visible = false;

    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };
    let mut painted = String::new();
    let output = crate::support::run_ui(&ctx, input, |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(app, ui);
    });
    painted.push_str(&painted_text(&output.shapes));
    painted
}

/// 打开一份真实文件后，编辑区必须把它的正文画出来（深色主题）。
#[test]
fn opened_document_text_is_painted_in_dark_theme() {
    let (dir, mut app) = app_fixture();
    let file = dir.path().join("note.md");
    std::fs::write(&file, "# 标题\n\narticle_id 是文章标识\n").expect("写文件应成功");

    app.open_path(&file.to_string_lossy()).expect("打开应成功");
    // `open_path` 之后领域内容已是文件内容——先钉住这一层。
    assert!(
        app.current_content()
            .unwrap_or_default()
            .contains("article_id"),
        "领域内容应已装载文件正文"
    );

    let painted = run_frame(&mut app, true);
    assert!(
        painted.contains("article_id"),
        "深色主题下编辑区未画出正文；本帧画出的文本为：\n{painted}"
    );
}

/// 同一断言在浅色主题下也必须成立（回归：两个主题曾同时空白）。
#[test]
fn opened_document_text_is_painted_in_light_theme() {
    let (dir, mut app) = app_fixture();
    let file = dir.path().join("note.md");
    std::fs::write(&file, "# 标题\n\narticle_id 是文章标识\n").expect("写文件应成功");

    app.open_path(&file.to_string_lossy()).expect("打开应成功");
    let painted = run_frame(&mut app, false);
    assert!(
        painted.contains("article_id"),
        "浅色主题下编辑区未画出正文；本帧画出的文本为：\n{painted}"
    );
}
