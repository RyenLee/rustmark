//! 「全部关闭」的二次确认（问题 038）。
//!
//! # 为什么用应用内浮层
//!
//! 全部关闭会丢弃**所有**未保存修改，不可逆，必须二次确认。但此前走的是
//! `DialogRunner` → `DialogPort::confirm` → rfd 系统原生消息框：白底、灰按钮、
//! 黄色警告图标，与项目 egui 深色界面和交互完全割裂。现改为
//! `Overlay::Confirm` 应用内浮层 + 统一的「确定 / 取消」按钮。
//!
//! 本文件钉住六件事：
//! 1. **没有脏文档时不弹浮层**（零风险操作不该有摩擦）；
//! 2. 有脏文档时弹应用内浮层，且**完全不调用系统确认框**；
//! 3. 确认文案报出脏文档数；
//! 4. 答复「确定」才真关，并回到欢迎页状态；
//! 5. 答复「取消」/ Esc / 标题栏 × 同效：不关、不报错、闩锁被清；
//! 6. 已有一个确认在飞时重复请求被忽略（不会叠出第二个浮层）。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::InsertTextCommand;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::presentation::state::dialog_ui_state::Overlay;
use rustmark::shared::ids::DocumentId;

use crate::support::fake_dialog::FakeDialog;

fn app_with(fake: FakeDialog) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(ConfigPaths::in_dir(dir.path()), Arc::new(fake))
        .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

/// 打开 `n` 份磁盘文件（默认限额 12，够用）。
fn open_n(app: &mut EditorApp, dir: &std::path::Path, n: usize) {
    for i in 0..n {
        let path = dir.join(format!("doc{i}.md"));
        std::fs::write(&path, format!("body {i}\n")).expect("写文件应成功");
        app.open_path(&path.to_string_lossy()).expect("打开应成功");
    }
}

/// 把活动文档改脏。
fn dirty_active(app: &mut EditorApp) {
    let id: DocumentId = app.ensure_document().expect("应有活动文档");
    app.services
        .documents
        .insert_text(InsertTextCommand {
            id,
            offset: 0,
            text: "changed ".to_string(),
        })
        .expect("插入应成功");
}

fn open_count(app: &EditorApp) -> usize {
    app.open_document_list().len()
}

fn confirm_message(app: &EditorApp) -> String {
    app.dialog.confirm_message().unwrap_or_default().to_owned()
}

/// 零风险路径：没有脏文档时直接关，**不弹确认浮层**。
#[test]
fn closing_clean_documents_needs_no_confirmation() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    open_n(&mut app, ws.path(), 3);

    app.close_all_with_confirm();

    assert_eq!(open_count(&app), 0, "全部关闭后应一个不剩");
    assert!(app.should_show_welcome(), "应回到欢迎页状态");
    assert!(
        !app.dialog.is_open(Overlay::Confirm),
        "干净文档不该弹确认框"
    );
    assert_eq!(fake.confirm_calls(), 0, "不该调用系统确认框");
}

/// 有脏文档时先弹**应用内**浮层；文档在答复前必须还在。
#[test]
fn closing_dirty_documents_asks_first() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    open_n(&mut app, ws.path(), 3);
    dirty_active(&mut app);

    app.close_all_with_confirm();

    assert_eq!(open_count(&app), 3, "答复之前一个都不能关");
    assert!(app.dialog.is_open(Overlay::Confirm), "应打开应用内确认浮层");
    assert!(app.dialog.confirm_pending(), "应有待答复的确认请求");
    assert!(app.dialog.is_modal(), "确认浮层必须是模态");
    assert!(!confirm_message(&app).is_empty(), "确认文案不能为空");
    assert_eq!(fake.confirm_calls(), 0, "不再调用系统原生确认框");
}

/// 确认文案报出脏文档数——用户要凭这条信息决定「值不值得再存一次」。
#[test]
fn the_confirm_message_reports_the_dirty_count() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    open_n(&mut app, ws.path(), 3);
    // 把三份都改脏。
    for _ in 0..3 {
        dirty_active(&mut app);
        app.execute(UiAction::NextTab);
    }

    app.close_all_with_confirm();

    let message = confirm_message(&app);
    assert!(
        message.contains('3'),
        "确认框应报出脏文档数，实际 {message:?}"
    );
    assert!(
        message.contains("未保存"),
        "确认框应说明丢的是未保存修改，实际 {message:?}"
    );
}

/// 答复「确定」→ 真的全关，回到欢迎页，浮层与待确认请求一起清掉。
#[test]
fn confirming_closes_everything() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    open_n(&mut app, ws.path(), 3);
    dirty_active(&mut app);

    app.close_all_with_confirm();
    app.resolve_close_all_confirm(true);

    assert_eq!(open_count(&app), 0, "确认后应全部关闭");
    assert!(app.should_show_welcome(), "应回到欢迎页状态");
    assert!(!app.dialog.is_open(Overlay::Confirm), "确认后浮层应关闭");
    assert!(app.dialog.confirm_message().is_none(), "待确认请求应清空");
}

/// 答复「取消」与关掉窗口同效：不关、不报错，且闩锁被清（可再次请求）。
#[test]
fn cancelling_keeps_everything_and_clears_the_latch() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    open_n(&mut app, ws.path(), 2);
    dirty_active(&mut app);

    app.close_all_with_confirm();
    app.resolve_close_all_confirm(false);

    assert_eq!(open_count(&app), 2, "点了「取消」就不该关");
    assert!(!app.dialog.is_open(Overlay::Confirm), "取消后浮层应关闭");
    assert!(
        app.status_bar.message.as_ref().is_none_or(|m| !m.is_error),
        "「取消」是正常操作，不该报错"
    );
    // 只 close_top 不清闩锁的话，下一次点击会被静默吞掉。
    app.close_all_with_confirm();
    assert!(
        app.dialog.confirm_pending(),
        "取消后必须能重新发起「全部关闭」确认"
    );
    assert_eq!(open_count(&app), 2, "重新确认期间文档仍要保留");
}

/// Esc（走 `CloseOverlay` 动作）等价于「取消」，不能只关浮层不清闩锁。
#[test]
fn escape_is_equivalent_to_cancel() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    open_n(&mut app, ws.path(), 2);
    dirty_active(&mut app);

    app.close_all_with_confirm();
    app.execute(UiAction::CloseOverlay);

    assert_eq!(open_count(&app), 2, "Esc 取消不该关掉文档");
    assert!(!app.dialog.is_open(Overlay::Confirm), "Esc 应关掉确认浮层");
    assert!(
        app.dialog.confirm_message().is_none(),
        "Esc 应清掉待确认请求"
    );
    assert!(
        app.status_bar.message.as_ref().is_none_or(|m| !m.is_error),
        "Esc 取消不该报错"
    );
}

/// 已有一个「全部关闭」在等确认时再次请求应被忽略（不叠第二个浮层）。
#[test]
fn a_second_close_all_request_is_dropped_while_one_is_pending() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    open_n(&mut app, ws.path(), 2);
    dirty_active(&mut app);

    app.close_all_with_confirm();
    let first = confirm_message(&app);
    app.close_all_with_confirm();

    assert!(app.dialog.confirm_pending(), "第一个请求应仍在等确认");
    assert_eq!(confirm_message(&app), first, "重复请求不该改写文案");
    assert_eq!(fake.confirm_calls(), 0, "不该调用系统确认框");
    assert_eq!(open_count(&app), 2, "未确认前文档必须还在");
}
