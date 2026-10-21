//! 收尾四组状态写入口（R-11 子批 3 · settings_ui / find / status_bar /
//! shortcut_overview）。
//!
//! # 判据一律走方法入口
//!
//! **不**直接给 `app.<state>.<字段>` 赋值再断言它（判据自指，方法改空实现也
//! 照样绿，本项目已多次撞上）。唯一能区分「改对了」与「改漏了」的判据是
//! 「方法真的把值写进了状态」。clear 类方法用「先直接种入已知值、再调方法、
//! 断言被清空」——写入路径走的是被测方法，直接赋值只作 setup。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::state::settings_ui_state::{PendingConflict, SettingsTab};
use rustmark::shared::encoding::TextEncoding;

use crate::support::fake_dialog::FakeDialog;

fn app_in(dir: &std::path::Path) -> EditorApp {
    let services =
        Services::assemble_with_dialogs(ConfigPaths::in_dir(dir), Arc::new(FakeDialog::new()))
            .expect("装配应成功");
    EditorApp::new(services, false)
}

/// 设置分区切换经方法写入。
#[test]
fn settings_section_is_written_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    assert_eq!(
        app.settings_ui.section,
        SettingsTab::Appearance,
        "前置：默认外观"
    );

    app.set_settings_section(SettingsTab::Shortcuts);
    assert_eq!(
        app.settings_ui.section,
        SettingsTab::Shortcuts,
        "分区应被写入"
    );
}

/// 错误 / 提示一并清空经方法写入。
#[test]
fn settings_messages_clear_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    // setup：直接种入已知值（非判据）。
    app.settings_ui.error = Some("坏值".to_string());
    app.settings_ui.notice = Some("提示".to_string());

    app.clear_settings_messages();

    assert!(app.settings_ui.error.is_none(), "错误应被清空");
    assert!(app.settings_ui.notice.is_none(), "提示应被清空");
}

/// 未确认快捷键冲突经方法清空。
#[test]
fn settings_pending_conflict_clears_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    // setup：直接种入已知冲突（非判据）。
    app.settings_ui.pending_conflict = Some(PendingConflict {
        combo: "Ctrl+K".to_string(),
        with_label: "另存为".to_string(),
        with_key: "save_as".to_string(),
        with_combo: "Ctrl+S".to_string(),
    });

    app.clear_settings_pending_conflict();

    assert!(
        app.settings_ui.pending_conflict.is_none(),
        "待确认冲突应被清空"
    );
}

/// 查找框焦点标志经方法写入。
#[test]
fn find_focused_is_written_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    assert!(!app.find.find_focused, "前置：初始未聚焦");

    app.set_find_focused(true);
    assert!(app.find.find_focused, "聚焦应写 true");

    app.set_find_focused(false);
    assert!(!app.find.find_focused, "失焦应写 false");
}

/// 状态栏编码经方法写入。
#[test]
fn status_bar_encoding_is_written_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    assert!(app.status_bar.encoding.is_none(), "前置：初始无编码");

    app.set_status_bar_encoding(Some(TextEncoding::Utf8));
    assert_eq!(
        app.status_bar.encoding,
        Some(TextEncoding::Utf8),
        "编码应被写入"
    );

    app.set_status_bar_encoding(None);
    assert!(app.status_bar.encoding.is_none(), "应能清空");
}

/// 快捷键总览筛选框文本经方法写入。
#[test]
fn shortcut_overview_query_is_written_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    assert!(app.shortcut_overview_ui.query.is_empty(), "前置：初始空串");

    app.set_shortcut_overview_query("save".to_string());
    assert_eq!(app.shortcut_overview_ui.query, "save", "筛选文本应被写入");
}
