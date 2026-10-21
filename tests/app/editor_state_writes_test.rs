//! 编辑区 UI 状态的写入口（R-11 子批 3 · editor 组）。
//!
//! # 判据一律走方法入口
//!
//! **不**直接给 `app.editor.<字段>` 赋值再断言它：那样断言读的是自己刚写
//! 进去的值，方法改成空实现也照样绿（判据自指，本项目已多次撞上）。
//! R-11 是纯重构、用户可见行为不变，唯一能区分「改对了」与「改漏了」的
//! 判据就是「方法真的把值写进了状态」。
//!
//! 切换类方法用「调用后 == !调用前」而不是断言某个具体值：初始值由偏好
//! 播种，写死会让用例在默认值变更时假红。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::state::editor_ui_state::SpellSuggestMenu;

use crate::support::fake_dialog::FakeDialog;

fn app_in(dir: &std::path::Path) -> EditorApp {
    let services =
        Services::assemble_with_dialogs(ConfigPaths::in_dir(dir), Arc::new(FakeDialog::new()))
            .expect("装配应成功");
    EditorApp::new(services, false)
}

fn menu() -> SpellSuggestMenu {
    SpellSuggestMenu::new(
        0,
        4,
        "helo",
        vec!["hello".to_string()],
        egui::pos2(10.0, 20.0),
    )
}

/// 光标 / 选区经方法写入——两者一起来。
#[test]
fn editor_selection_is_written_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());

    app.set_editor_selection(12, Some(3));

    assert_eq!(app.editor.cursor, 12, "光标应被写入");
    assert_eq!(app.editor.anchor, Some(3), "锚点应一并写入");
}

/// 右键开出的拼写菜单经方法写入。
#[test]
fn spell_menu_opens_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    assert!(app.editor.spell_menu.is_none(), "前置：初始无菜单");

    app.set_spell_menu(Some(menu()));

    assert!(app.editor.spell_menu.is_some(), "菜单应被写入");
    assert_eq!(
        app.editor.spell_menu.as_ref().map(|m| m.word.as_str()),
        Some("helo"),
        "写入的应是传进去的那个菜单"
    );
}

/// 关闭拼写菜单经方法写入。
#[test]
fn spell_menu_closes_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    app.set_spell_menu(Some(menu()));
    assert!(app.editor.spell_menu.is_some(), "前置：菜单已开");

    app.close_spell_menu();

    assert!(app.editor.spell_menu.is_none(), "关闭后应为空");
}

/// 编辑区滚动偏移经方法写入。
#[test]
fn editor_scroll_is_written_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());

    app.set_editor_scroll_y(123.5);

    assert_eq!(app.editor.scroll_y, 123.5, "滚动偏移应被写入");
}

/// 行号开关经方法写入。
#[test]
fn line_numbers_flag_is_written_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    let before = app.editor.line_numbers;

    app.set_line_numbers(!before);
    assert_eq!(app.editor.line_numbers, !before, "行号开关应被写入");

    app.set_line_numbers(before);
    assert_eq!(app.editor.line_numbers, before, "应能改回来");
}

/// 拼写检查是**切换**而不是置位。
#[test]
fn spell_check_toggles_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    let before = app.editor.spell_check;

    app.toggle_spell_check();
    assert_eq!(app.editor.spell_check, !before, "切换一次应取反");

    app.toggle_spell_check();
    assert_eq!(app.editor.spell_check, before, "再切一次应回到原值");
}

/// 自动换行是**切换**而不是置位。
#[test]
fn word_wrap_toggles_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    let before = app.editor.word_wrap;

    app.toggle_word_wrap();
    assert_eq!(app.editor.word_wrap, !before, "切换一次应取反");

    app.toggle_word_wrap();
    assert_eq!(app.editor.word_wrap, before, "再切一次应回到原值");
}
