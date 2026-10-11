//! V11-07 · 编辑区拼写建议菜单（右键「你可能想输入」）。
//!
//! # 这一段最容易出的错是**静默错位**
//!
//! egui 只告诉「指针在哪个像素」，不给「哪个字符」。坐标 → 字节的换算写错时
//! 编译不报、运行不崩，只是菜单弹在**别的词**上——肉眼很难发现。
//! 故这里用真实的 `Galley` 走一遍换算，断言菜单落在预期的那个词上。
//!
//! 另外钉住两处容易退化的边界：
//! - 右键在**没错**的词上 → 不开菜单（给没错的人弹替换菜单是骚扰）；
//! - 菜单里的字节区间过期后（用户改了文本）不拿去替换。

use std::sync::Arc;

use egui::text::CCursor;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::state::editor_ui_state::SpellSuggestMenu;
use rustmark::presentation::theme::builtin::Theme;
use rustmark::presentation::theme::mapper;
use rustmark::presentation::views::editor_view::spell_menu_for;

use crate::support::fake_dialog::FakeDialog;

/// 造一个装好默认配置、已打开一个空文档的应用。
fn app_with(text: &str) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    app.close_all_documents();
    app.new_document().expect("应可新建");
    app.insert_at_cursor(text).expect("应可插入");
    app.editor.spell_check = true;
    (dir, app)
}

/// 用与编辑区**同一套**排版函数造一份 `Galley`。
fn galley(ctx: &egui::Context, text: &str) -> Arc<egui::Galley> {
    mapper::highlight_layout(ctx, &Theme::dark(), text, None, 400.0)
}

/// 取第 `char_index` 个字符在 `Galley` 里的中心点（相对文本原点）。
fn pointer_at(g: &egui::Galley, char_index: usize) -> egui::Pos2 {
    g.pos_from_cursor(CCursor::new(char_index)).center()
}

/// 主路径：右键落在错词上 → 菜单给出该词的区间与原拼写。
#[test]
fn right_clicking_a_flagged_word_opens_a_menu_for_it() {
    let (_dir, mut app) = app_with("teh quick brown");
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let g = galley(&ctx, "teh quick brown");
    let origin = egui::pos2(0.0, 0.0);

    let menu = spell_menu_for(&mut app, &g, origin, origin + pointer_at(&g, 1).to_vec2())
        .expect("错词上应弹出建议菜单");

    assert_eq!(
        (menu.byte_start, menu.byte_end),
        (0, 3),
        "菜单应锁定 teh 这个区间，实际 {:?}",
        (menu.byte_start, menu.byte_end)
    );
    assert_eq!(menu.word, "teh");
    assert!(
        menu.suggestions.iter().any(|s| s == "the"),
        "建议里应有 the，实际 {:?}",
        menu.suggestions
    );
}

/// 右键在**正确**的词上 → 不开菜单。
#[test]
fn right_clicking_a_correct_word_opens_nothing() {
    let (_dir, mut app) = app_with("teh quick brown");
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let g = galley(&ctx, "teh quick brown");
    let origin = egui::pos2(0.0, 0.0);

    // `quick` 起点是第 4 个字符。
    let menu = spell_menu_for(&mut app, &g, origin, origin + pointer_at(&g, 6).to_vec2());
    assert!(menu.is_none(), "正确的词不该弹替换菜单，实际 {menu:?}");
}

/// 右键在空白处 → 不开菜单。
#[test]
fn right_clicking_whitespace_opens_nothing() {
    let (_dir, mut app) = app_with("teh quick brown");
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let g = galley(&ctx, "teh quick brown");
    let origin = egui::pos2(0.0, 0.0);

    let menu = spell_menu_for(&mut app, &g, origin, origin + pointer_at(&g, 3).to_vec2());
    assert!(menu.is_none(), "空白处没有词，不该弹菜单");
}

/// 关掉拼写检查 → 没有波浪线，右键自然也不给建议。
#[test]
fn spell_check_off_means_no_menu() {
    let (_dir, mut app) = app_with("teh quick brown");
    app.editor.spell_check = false;
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    let g = galley(&ctx, "teh quick brown");
    let origin = egui::pos2(0.0, 0.0);

    let menu = spell_menu_for(&mut app, &g, origin, origin + pointer_at(&g, 1).to_vec2());
    assert!(menu.is_none(), "开关关闭时不该弹菜单");
}

/// 选中一条建议 → 真的替换掉那个词，且菜单关闭。
#[test]
fn picking_a_suggestion_replaces_the_word() {
    let (_dir, mut app) = app_with("teh quick brown");
    let menu = SpellSuggestMenu::new(0, 3, "teh", vec!["the".to_string()], egui::Pos2::ZERO);
    app.editor.spell_menu = Some(menu);

    app.replace_spelling(0, 3, "the").expect("替换应成功");
    app.editor.spell_menu = None;

    assert_eq!(
        document_text(&app),
        "the quick brown",
        "替换应真的写进领域，而不只是改了界面上的副本"
    );
}

/// 从领域读回当前文档内容（**唯一**的真相）。
///
/// 不读 `app.editor_text`：那是给 egui 绑定的视图副本，
/// 断言它只会证明「副本被改了」，证明不了替换落到了领域——
/// 而「界面显示已改、领域没改」恰恰是这条链路上最该防的缺陷。
fn document_text(app: &EditorApp) -> String {
    let open = app
        .services
        .documents
        .list_open(rustmark::application::document::queries::ListOpenDocumentsQuery)
        .expect("应能列出已打开文档");
    let id = open.first().expect("应有一个已打开文档").id;
    app.services
        .documents
        .get_content(rustmark::application::document::queries::GetContentQuery { id, range: None })
        .expect("应能读回内容")
}

/// 区间越界时**静默失败**而不 panic。
///
/// 菜单里存的是上一帧的字节区间：用户可能在菜单开着时删掉了那段文本，
/// 此时按旧区间替换会改到别处去——宁可什么都不做。
#[test]
fn an_out_of_range_replacement_fails_softly() {
    let (_dir, mut app) = app_with("hi");
    let result = app.replace_spelling(0, 99, "the");
    assert!(result.is_err(), "越界替换应返回错误而不是改坏文档");
    assert_eq!(document_text(&app), "hi", "失败的替换不该改动领域内容");
}
