//! 应用 UI 状态（`AppUiState`）的写入口（R-11 子批 3 · ui 组）。
//!
//! # 判据一律走方法入口
//!
//! **不**直接给 `app.ui.<字段>` 赋值再断言它：那样断言读的是自己刚写进去的
//! 值，方法改成空实现也照样绿（判据自指，本项目已多次撞上）。唯一能区分
//! 「改对了」与「改漏了」的判据就是「方法真的把值写进了状态」。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;

use crate::support::fake_dialog::FakeDialog;

fn app_in(dir: &std::path::Path) -> EditorApp {
    let services =
        Services::assemble_with_dialogs(ConfigPaths::in_dir(dir), Arc::new(FakeDialog::new()))
            .expect("装配应成功");
    EditorApp::new(services, false)
}

/// 分割线拖拽标志经方法写入。
#[test]
fn split_dragging_is_written_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    assert!(!app.ui.split_dragging, "前置：初始未拖拽");

    app.set_split_dragging(true);
    assert!(app.ui.split_dragging, "拖拽开始应写 true");

    app.set_split_dragging(false);
    assert!(!app.ui.split_dragging, "松手应写 false");
}

/// 侧栏宽度拖拽标志经方法写入。
#[test]
fn sidebar_dragging_is_written_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    assert!(!app.ui.sidebar_dragging, "前置：初始未拖拽");

    app.set_sidebar_dragging(true);
    assert!(app.ui.sidebar_dragging, "拖拽开始应写 true");

    app.set_sidebar_dragging(false);
    assert!(!app.ui.sidebar_dragging, "松手应写 false");
}

/// 标签条最后激活项经方法写入。
#[test]
fn tabstrip_last_active_is_written_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    assert!(app.ui.tabstrip_last_active.is_none(), "前置：初始无激活项");

    app.set_tabstrip_last_active(Some(7));
    assert_eq!(app.ui.tabstrip_last_active, Some(7), "激活项应被写入");

    app.set_tabstrip_last_active(None);
    assert!(app.ui.tabstrip_last_active.is_none(), "应能清空");
}

/// 标签条横向滚动偏移经方法写入。
#[test]
fn tabstrip_scroll_x_is_written_through_the_app_method() {
    let dir = tempfile::tempdir().expect("工作区");
    let mut app = app_in(dir.path());
    assert_eq!(app.ui.tabstrip_scroll_x, 0.0, "前置：初始无偏移");

    app.set_tabstrip_scroll_x(55.0);
    assert_eq!(app.ui.tabstrip_scroll_x, 55.0, "偏移应被写入");
}
