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
use rustmark::presentation::shortcuts::registry::UiAction;
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

// ————————————————————————————————————————————————————————————
// F-1：自动换行 / 拼写检查的「会话 ↔ 领域」双写
//
// 缺陷原状：设置页只写领域（当前会话不生效、重启不回读），状态栏点击只翻
// 会话（不落盘、重启丢失），两条路互相矛盾。以下用例用**同一个配置目录
// 重新装配**模拟重启，判据全部走方法入口。
// ————————————————————————————————————————————————————————————

/// 状态栏切换自动换行后，重启应回读到切换值（旧实现会退回领域默认）。
#[test]
fn word_wrap_toggle_survives_restart() {
    let dir = tempfile::tempdir().expect("工作区");
    let toggled = {
        let mut app = app_in(dir.path());
        let before = app.editor.word_wrap;
        app.toggle_word_wrap();
        assert_eq!(app.editor.word_wrap, !before, "前置：切换应生效");
        !before
    };

    let app = app_in(dir.path());
    assert_eq!(app.editor.word_wrap, toggled, "重启后会话值应回读");
    assert_eq!(
        app.settings().editor.word_wrap,
        toggled,
        "领域侧也应记下同一个值（两份值矛盾正是 F-1 的症状）"
    );
}

/// 状态栏切换拼写检查后，重启应回读到切换值。
#[test]
fn spell_check_toggle_survives_restart() {
    let dir = tempfile::tempdir().expect("工作区");
    let toggled = {
        let mut app = app_in(dir.path());
        let before = app.editor.spell_check;
        app.toggle_spell_check();
        assert_eq!(app.editor.spell_check, !before, "前置：切换应生效");
        !before
    };

    let app = app_in(dir.path());
    assert_eq!(app.editor.spell_check, toggled, "重启后会话值应回读");
    assert_eq!(
        app.settings().editor.spell_check,
        toggled,
        "领域侧也应记下同一个值"
    );
}

/// 设置页的置位入口：会话立即可见 + 领域同时写入 + 重启回读（三处齐）。
#[test]
fn set_word_wrap_writes_session_and_domain() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    let target = !app.editor.word_wrap;

    app.set_word_wrap(target);

    assert_eq!(app.editor.word_wrap, target, "当前会话应立即生效");
    assert_eq!(
        app.settings().editor.word_wrap,
        target,
        "领域应同时写入（否则重启丢）"
    );
    let app = app_in(dir.path());
    assert_eq!(app.editor.word_wrap, target, "重启后应回读");
}

/// 设置页的置位入口：拼写检查同样三处齐。
#[test]
fn set_spell_check_writes_session_and_domain() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    let target = !app.editor.spell_check;

    app.set_spell_check(target);

    assert_eq!(app.editor.spell_check, target, "当前会话应立即生效");
    assert_eq!(
        app.settings().editor.spell_check,
        target,
        "领域应同时写入（否则重启丢）"
    );
    let app = app_in(dir.path());
    assert_eq!(app.editor.spell_check, target, "重启后应回读");
}

/// 「取消」要把会话侧的两份镜像一起回滚：只回滚领域会让编辑区停在
/// 取消前的行为上，与设置页复选框显示的值矛盾。
#[test]
fn cancel_settings_rolls_back_word_wrap_and_spell_check() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    let wrap = app.editor.word_wrap;
    let spell = app.editor.spell_check;

    app.execute(UiAction::OpenSettings);
    app.set_word_wrap(!wrap);
    app.set_spell_check(!spell);
    assert_ne!(app.editor.word_wrap, wrap, "前置：设置期间已改");
    assert_ne!(app.editor.spell_check, spell, "前置：设置期间已改");

    app.cancel_settings();

    assert_eq!(app.editor.word_wrap, wrap, "会话侧自动换行应回滚");
    assert_eq!(app.editor.spell_check, spell, "会话侧拼写检查应回滚");
    assert_eq!(app.settings().editor.word_wrap, wrap, "领域侧也应回滚");
    assert_eq!(app.settings().editor.spell_check, spell, "领域侧也应回滚");
}

/// 远程图片开关（PRIV-01）：会话立即生效 + 领域同时写入 + 重启回读。
///
/// 三处缺一处的症状分别是「关了还联网」/「重启自动恢复联网」/「设置页显示
/// 关闭但实际在联网」——隐私承诺不能只靠内存里那个 bool。
#[test]
fn set_load_remote_images_writes_session_and_domain() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    assert!(app.editor.load_remote_images, "默认应为加载");

    app.set_load_remote_images(false);

    assert!(
        !app.editor.load_remote_images,
        "当前会话应立即生效（否则本帧照旧联网）"
    );
    assert!(
        !app.settings().editor.load_remote_images,
        "领域应同时写入（否则重启丢）"
    );
    let app = app_in(dir.path());
    assert!(!app.editor.load_remote_images, "重启后应回读");
}

/// 「取消」必须把**是否外联**也还原：只回滚领域会让本会话继续按取消前的
/// 决定联网（或继续断网），与设置页复选框显示的值矛盾。
#[test]
fn cancel_settings_rolls_back_remote_images() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    let before = app.editor.load_remote_images;

    app.execute(UiAction::OpenSettings);
    app.set_load_remote_images(!before);
    assert_ne!(app.editor.load_remote_images, before, "前置：设置期间已改");

    app.cancel_settings();

    assert_eq!(
        app.editor.load_remote_images, before,
        "会话侧远程图片开关应回滚"
    );
    assert_eq!(
        app.settings().editor.load_remote_images,
        before,
        "领域侧也应回滚"
    );
}
