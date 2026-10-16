//! 多文档联动用例（回归：编辑区与预览区都必须跟随**当前激活 tab**）。
//!
//! # 这两个缺陷长什么样
//!
//! 1. **编辑区**：逐行排得完全正确，却一帧一字都不画——`Galley::is_empty()`
//!    被判为真，`Painter::galley` 直接跳过 `Shape`。数据结构层面完全看不出来，
//!    只能从本帧的 `FullOutput::shapes` 里找字形。
//! 2. **预览区**：切 tab 后仍显示**第一份打开的**文档。原因不在渲染，
//!    而在缓存键——`revision` 是**每份文档各自**的计数器，两份未编辑的文档
//!    版本号相同，于是「版本号没变 ⇒ 内容没变 ⇒ 不用重排」这个推断把
//!    两份不同的文档当成了同一份。
//!
//! 两条都必须靠「跑真帧、看真输出」才能钉住，故放在这里而不是纯单元测试里。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::shortcuts::registry::UiAction;
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
    (dir, EditorApp::new(services, false))
}

fn input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    }
}

/// 跑一帧，返回本帧画出来的全部文本。
fn run_frame(ctx: &egui::Context, app: &mut EditorApp, dark: bool) -> String {
    let theme = if dark {
        rustmark::presentation::theme::builtin::Theme::dark()
    } else {
        rustmark::presentation::theme::builtin::Theme::light()
    };
    app.ui.set_theme(theme);
    let out = crate::support::run_ui(ctx, input(), |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(app, ui);
    });
    painted_text(&out.shapes)
}

/// 预览区当前缓存的块文本（拼接）。
fn preview_text(app: &EditorApp) -> String {
    app.preview
        .blocks
        .iter()
        .map(|b| b.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 关注一个「干净」的排版起点（高亮缓存是线程局部的，用例间会互相污染）。
fn fresh_ctx() -> egui::Context {
    rustmark::presentation::theme::mapper::clear_highlight_caches();
    let ctx = egui::Context::default();
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {}); // 装载字库
    ctx
}

/// 打开两份文档后切 tab：**编辑区**必须画出当前那份的正文。
///
/// 关掉预览与侧边栏，让画面上的正文只可能来自编辑区——否则预览区会「代答」。
#[test]
fn editor_paints_the_active_document_after_tab_switch() {
    let (dir, mut app) = app_fixture();
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.md");
    std::fs::write(&a, "AAA_ONLY first document body\n").expect("写 a 应成功");
    std::fs::write(&b, "BBB_ONLY second document body\n").expect("写 b 应成功");

    let ctx = fresh_ctx();

    // —— 只留编辑区 ——
    app.ui.preview_visible = false;
    app.ui.sidebar_visible = false;

    app.open_path(&a.to_string_lossy()).expect("打开 a 应成功");
    let painted = run_frame(&ctx, &mut app, true);
    assert!(
        painted.contains("AAA_ONLY"),
        "打开 A 后编辑区未画出 A 的正文；本帧画出的文本为：\n{painted}"
    );
    assert!(!painted.contains("BBB_ONLY"), "此时不该出现 B 的正文");

    app.open_path(&b.to_string_lossy()).expect("打开 b 应成功");
    let painted = run_frame(&ctx, &mut app, true);
    assert!(
        painted.contains("BBB_ONLY"),
        "切到 B 后编辑区未画出 B 的正文；本帧画出的文本为：\n{painted}"
    );

    // 键盘轮换回 A：编辑区必须跟着切回来。
    app.execute(UiAction::PrevTab);
    let painted = run_frame(&ctx, &mut app, true);
    assert!(
        painted.contains("AAA_ONLY"),
        "切回 A 后编辑区未画出 A 的正文；本帧画出的文本为：\n{painted}"
    );
    assert!(
        !painted.contains("BBB_ONLY"),
        "切回 A 后不该还画着 B 的正文"
    );
}

/// 同一场景在浅色主题下也必须成立（该缺陷与配色无关，两主题曾同时白板）。
#[test]
fn editor_paints_the_active_document_in_light_theme() {
    let (dir, mut app) = app_fixture();
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.md");
    std::fs::write(&a, "AAA_ONLY first document body\n").expect("写 a 应成功");
    std::fs::write(&b, "BBB_ONLY second document body\n").expect("写 b 应成功");

    let ctx = fresh_ctx();
    app.ui.preview_visible = false;
    app.ui.sidebar_visible = false;

    app.open_path(&a.to_string_lossy()).expect("打开 a 应成功");
    let painted = run_frame(&ctx, &mut app, false);
    assert!(
        painted.contains("AAA_ONLY"),
        "浅色主题下编辑区未画出正文；本帧画出的文本为：\n{painted}"
    );

    app.open_path(&b.to_string_lossy()).expect("打开 b 应成功");
    let painted = run_frame(&ctx, &mut app, false);
    assert!(
        painted.contains("BBB_ONLY"),
        "浅色主题下切到 B 后未画出 B 的正文；本帧画出的文本为：\n{painted}"
    );
}

/// 打开两份文档后切 tab：**预览区**必须显示当前那份的内容。
///
/// 两份文档都未编辑过，版本号相同——这正是原先被误判成「内容没变」的场景。
#[test]
fn preview_follows_the_active_tab() {
    let (dir, mut app) = app_fixture();
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.md");
    std::fs::write(&a, "AAA_ONLY first document body\n").expect("写 a 应成功");
    std::fs::write(&b, "BBB_ONLY second document body\n").expect("写 b 应成功");

    let ctx = fresh_ctx();
    app.ui.sidebar_visible = false; // 侧栏文件树会显示文件名，但不会显示正文
    assert!(app.ui.preview_visible, "用例前提：预览区默认可见");

    app.open_path(&a.to_string_lossy()).expect("打开 a 应成功");
    run_frame(&ctx, &mut app, true);
    assert!(
        preview_text(&app).contains("AAA_ONLY"),
        "打开 A 后预览区应显示 A；实际为：{:?}",
        preview_text(&app)
    );
    assert!(preview_text(&app).contains("first document body"));

    // 再开一份 B：B 立刻成为激活 tab，预览必须同步。
    app.open_path(&b.to_string_lossy()).expect("打开 b 应成功");
    run_frame(&ctx, &mut app, true);
    assert!(
        preview_text(&app).contains("BBB_ONLY"),
        "切到 B 后预览区仍显示旧文档；实际为：{:?}",
        preview_text(&app)
    );
    assert!(
        !preview_text(&app).contains("AAA_ONLY"),
        "切到 B 后预览区不应残留 A 的内容；实际为：{:?}",
        preview_text(&app)
    );

    // 切回 A：预览同样要跟回来。
    app.execute(UiAction::PrevTab);
    run_frame(&ctx, &mut app, true);
    assert!(
        preview_text(&app).contains("AAA_ONLY"),
        "切回 A 后预览区应显示 A；实际为：{:?}",
        preview_text(&app)
    );
    assert!(
        !preview_text(&app).contains("BBB_ONLY"),
        "切回 A 后预览区不应残留 B 的内容；实际为：{:?}",
        preview_text(&app)
    );
}

/// 切 tab 不得把上一份文档的滚动偏移带过来（预览与 tab 联动的另一半）。
///
/// 两份文档都**够长**，这样「900」在两边都是合法偏移——否则 `ScrollArea`
/// 会自己把越界偏移夹回 0，用例就变成假阳性（不重置也照样绿）。
#[test]
fn preview_scroll_does_not_bleed_across_tabs() {
    let (dir, mut app) = app_fixture();
    let a = dir.path().join("a.md");
    let b = dir.path().join("b.md");
    let long_a: String = (0..80).map(|i| format!("AAA_ONLY 段落 {i}\n\n")).collect();
    let long_b: String = (0..80).map(|i| format!("BBB_ONLY 段落 {i}\n\n")).collect();
    std::fs::write(&a, long_a).expect("写 a 应成功");
    std::fs::write(&b, long_b).expect("写 b 应成功");

    let ctx = fresh_ctx();
    app.ui.sidebar_visible = false;

    app.open_path(&a.to_string_lossy()).expect("打开 a 应成功");
    run_frame(&ctx, &mut app, true);
    // 模拟用户在 A 的长文里滚到了很下面。
    app.preview.scroll_y = 900.0;

    app.open_path(&b.to_string_lossy()).expect("打开 b 应成功");
    run_frame(&ctx, &mut app, true);
    assert_eq!(
        app.preview.scroll_y, 0.0,
        "切到另一份文档后预览应回到顶部，而不是沿用上一份文档的偏移"
    );
    assert!(
        preview_text(&app).contains("BBB_ONLY"),
        "预览内容也应换成 B；实际为：{:?}",
        preview_text(&app)
    );
}
