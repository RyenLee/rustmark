//! 「全部关闭」的二次确认（问题 038）。
//!
//! # 为什么需要确认
//!
//! 全部关闭会丢弃**所有**未保存修改，不可逆；而 `DialogPort::confirm` 是
//! 同步阻塞的原生模态框，不能在 `update()` 里直接调。故与 V11-06 的删除
//! 确认同一套路：登记意图 → 后台确认框 → 每帧取回。
//!
//! 本文件钉住五件事：
//! 1. **没有脏文档时不弹框**（零风险操作不该有摩擦）；
//! 2. 有脏文档时先确认，且确认文案报出脏文档数；
//! 3. 答复「是」才真关，且回到欢迎页状态（`should_show_welcome()`）；
//! 4. 答复「否」/ 取消同效：不关、不报错；
//! 5. 对话框不可用时报错且**不关**（静默是最坏结果）。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::InsertTextCommand;
use rustmark::shared::ids::DocumentId;

use crate::support::fake_dialog::{FakeDialog, Reply};

fn app_with(fake: FakeDialog) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(ConfigPaths::in_dir(dir.path()), Arc::new(fake))
        .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

/// 把后台结果收回来并应用（等价于 `update` 里的那两步）。
///
/// 注意时序：`DialogRunner::spawn` 起**后台线程**调 `DialogPort::confirm`，
/// `confirm_calls` 是那个线程里自增的——**drain 之前**读它是在和线程
/// 赛跑（既有删除确认测试一律在 drain 之后才断言语义次数的原因）。
/// 同步可观测的「确认框已弹出」用 [`EditorApp::dialog_busy`]。
fn drain(app: &mut EditorApp) -> bool {
    for _ in 0..200 {
        if let Some(outcome) = app.dialogs.poll() {
            app.apply_dialog_outcome(outcome);
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
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

/// 零风险路径：没有脏文档时直接关，**不弹确认框**。
#[test]
fn closing_clean_documents_needs_no_confirmation() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    open_n(&mut app, ws.path(), 3);

    app.close_all_with_confirm();

    assert_eq!(open_count(&app), 0, "全部关闭后应一个不剩");
    assert!(app.should_show_welcome(), "应回到欢迎页状态");
    assert_eq!(fake.confirm_calls(), 0, "没有脏文档不该弹确认框");
}

/// 有脏文档时先确认，文档在答复前必须还在。
#[test]
fn closing_dirty_documents_asks_first() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new(); // 不预置回应 = 取消
    let (_cfg, mut app) = app_with(fake.clone());
    open_n(&mut app, ws.path(), 3);
    dirty_active(&mut app);

    app.close_all_with_confirm();

    assert_eq!(open_count(&app), 3, "答复之前一个都不能关");
    assert!(app.dialog_busy(), "有脏文档必须先弹确认框");
    assert!(drain(&mut app), "确认框应有答复");
    assert_eq!(fake.confirm_calls(), 1, "应弹一次确认框");
    assert_eq!(open_count(&app), 3, "用户没确认，文档必须还在");
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
        app.execute(rustmark::presentation::shortcuts::registry::UiAction::NextTab);
    }

    app.close_all_with_confirm();
    assert!(drain(&mut app));

    let message = fake.last_message();
    assert!(
        message.contains('3'),
        "确认框应报出脏文档数，实际 {message:?}"
    );
    assert!(
        message.contains("未保存"),
        "确认框应说明丢的是未保存修改，实际 {message:?}"
    );
}

/// 答复「是」→ 真的全关，回到欢迎页。
#[test]
fn confirming_closes_everything() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    fake.set_confirm(Reply::Yes);
    let (_cfg, mut app) = app_with(fake.clone());
    open_n(&mut app, ws.path(), 3);
    dirty_active(&mut app);

    app.close_all_with_confirm();
    assert!(drain(&mut app));

    assert_eq!(open_count(&app), 0, "确认后应全部关闭");
    assert!(app.should_show_welcome(), "应回到欢迎页状态");
    assert_eq!(fake.confirm_calls(), 1, "确认框只该弹一次");
}

/// 答复「否」与取消同效：不关、不报错。
#[test]
fn answering_no_keeps_everything() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    fake.set_confirm(Reply::No);
    let (_cfg, mut app) = app_with(fake);
    open_n(&mut app, ws.path(), 2);
    dirty_active(&mut app);

    app.close_all_with_confirm();
    assert!(drain(&mut app));

    assert_eq!(open_count(&app), 2, "点了「否」就不该关");
    assert!(
        app.status_bar.message.as_ref().is_none_or(|m| !m.is_error),
        "「否」是正常操作，不该报错"
    );
}

/// 对话框不可用：报错且不关（静默会让用户以为关了、实际没关）。
#[test]
fn an_unavailable_confirm_reports_and_keeps_documents() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    fake.set_confirm(Reply::Unavailable("no display".to_string()));
    let (_cfg, mut app) = app_with(fake);
    open_n(&mut app, ws.path(), 2);
    dirty_active(&mut app);

    app.close_all_with_confirm();
    assert!(drain(&mut app));

    assert_eq!(open_count(&app), 2, "拿不到答复就不能关");
    assert!(
        app.status_bar.message.as_ref().is_some_and(|m| m.is_error),
        "对话框不可用必须报错，不能静默"
    );
}

/// 已有一个「全部关闭」在等确认时再次请求应被忽略（不弹第二个窗口）。
#[test]
fn a_second_close_all_request_is_dropped_while_one_is_pending() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    open_n(&mut app, ws.path(), 2);
    dirty_active(&mut app);

    app.close_all_with_confirm();
    app.close_all_with_confirm();

    assert!(app.dialog_busy(), "第一个请求应已弹出确认框");
    assert!(drain(&mut app));
    assert_eq!(fake.confirm_calls(), 1, "只该弹一个确认框");
    assert_eq!(open_count(&app), 2, "未确认前文档必须还在");
}
