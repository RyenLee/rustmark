//! 「还原修改」（R-04a）：从磁盘重新加载，丢弃未保存修改。
//!
//! # 与撤销的关系
//!
//! `Revert` 是独立动作（UI 标签「还原修改」），不是 `Undo` 的别名：
//! 撤销回退一步编辑，还原回到**磁盘版本**。本文件钉住：
//! 1. 干净文档直接还原，**不弹确认框**（零风险操作不该有摩擦）；
//! 2. 脏文档先确认，答复前内容不许变；
//! 3. 答复「是」→ 内容回到磁盘版本、脏标记清除、草稿被清；
//! 4. 答复「否」/ 取消 → 修改原样保留；
//! 5. 还原本身**可撤销**（Ctrl+Z 回去）；
//! 6. 未命名文档无从还原，提示而非猜位置。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::InsertTextCommand;
use rustmark::application::document::queries::{GetContentQuery, GetDocumentQuery};
use rustmark::presentation::shortcuts::registry::UiAction;
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
/// 时序同删除确认测试：`confirm_calls` 在后台线程里自增，
/// **drain 之后**读它才是稳的。
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

/// 建一份磁盘文件并打开，返回 `(工作区目录, 文档 id)`。
fn open_disk_document(
    app: &mut EditorApp,
    ws: &std::path::Path,
    name: &str,
    body: &str,
) -> DocumentId {
    let path = ws.join(name);
    std::fs::write(&path, body).expect("写文件应成功");
    app.open_path(&path.to_string_lossy()).expect("打开应成功");
    app.ensure_document().expect("应有活动文档")
}

/// 把活动文档改脏（插入一段文本）。
fn dirty_active(app: &mut EditorApp) {
    let id = app.ensure_document().expect("应有活动文档");
    app.services
        .documents
        .insert_text(InsertTextCommand {
            id,
            offset: 0,
            text: "脏修改 ".to_string(),
        })
        .expect("插入应成功");
}

fn content_of(app: &EditorApp, id: DocumentId) -> String {
    app.services
        .documents
        .get_content(GetContentQuery { id, range: None })
        .expect("应可读取内容")
}

fn is_dirty(app: &EditorApp, id: DocumentId) -> bool {
    app.services
        .documents
        .get_tab_summary(GetDocumentQuery { id })
        .expect("应可读取摘要")
        .is_dirty
}

/// 触发「还原修改」（与菜单 / 命令面板 / 快捷键同一条真实路由）。
fn revert(app: &mut EditorApp) {
    app.execute(UiAction::Revert);
}

/// 核心场景：改 3 次 → 还原 → 内容回到磁盘版本，脏标记清除。
#[test]
fn revert_restores_the_disk_content() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    fake.set_confirm(Reply::Yes);
    let (_cfg, mut app) = app_with(fake);
    let id = open_disk_document(&mut app, ws.path(), "doc.md", "磁盘原文\n");
    for i in 0..3 {
        app.services
            .documents
            .insert_text(InsertTextCommand {
                id,
                offset: 0,
                text: format!("第{i}次修改 "),
            })
            .expect("插入应成功");
    }
    assert!(is_dirty(&app, id), "改过之后应是脏文档");

    revert(&mut app);
    assert!(drain(&mut app), "确认框应有答复");

    assert_eq!(
        content_of(&app, id),
        "磁盘原文\n",
        "还原后内容应回到磁盘版本"
    );
    assert!(!is_dirty(&app, id), "内容与磁盘一致，脏标记必须清除");
}

/// 零风险路径：文档干净时直接还原，**不弹确认框**。
#[test]
fn revert_without_changes_needs_no_confirmation() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    let id = open_disk_document(&mut app, ws.path(), "doc.md", "原文\n");

    revert(&mut app);

    assert_eq!(content_of(&app, id), "原文\n");
    assert_eq!(fake.confirm_calls(), 0, "干净文档还原不该弹确认框");
}

/// 脏文档先确认，且**答复前内容不许变**。
#[test]
fn revert_of_dirty_document_asks_first() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new(); // 不预置回应 = 取消
    let (_cfg, mut app) = app_with(fake.clone());
    let id = open_disk_document(&mut app, ws.path(), "doc.md", "磁盘原文\n");
    dirty_active(&mut app);

    revert(&mut app);

    assert!(app.dialog_busy(), "脏文档还原必须先弹确认框");
    assert_eq!(
        content_of(&app, id),
        "脏修改 磁盘原文\n",
        "答复之前内容必须原样"
    );
    assert!(drain(&mut app), "确认框应有答复");
    assert_eq!(
        content_of(&app, id),
        "脏修改 磁盘原文\n",
        "用户取消（未预置回应），修改必须保留"
    );
    assert_eq!(fake.confirm_calls(), 1, "应弹一次确认框");
}

/// 确认文案报出文档名——用户要凭它确认「丢的确实是这一份」。
#[test]
fn the_revert_message_names_the_document() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    open_disk_document(&mut app, ws.path(), "报告.md", "x\n");
    dirty_active(&mut app);

    revert(&mut app);
    assert!(drain(&mut app));

    let message = fake.last_message();
    assert!(
        message.contains("报告.md"),
        "确认框应报出文档名，实际 {message:?}"
    );
    assert!(
        message.contains("未保存"),
        "确认框应说明丢的是未保存修改，实际 {message:?}"
    );
}

/// 答复「否」→ 修改原样保留，不报错。
#[test]
fn answering_no_keeps_the_modifications() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    fake.set_confirm(Reply::No);
    let (_cfg, mut app) = app_with(fake);
    let id = open_disk_document(&mut app, ws.path(), "doc.md", "磁盘原文\n");
    dirty_active(&mut app);

    revert(&mut app);
    assert!(drain(&mut app));

    assert_eq!(
        content_of(&app, id),
        "脏修改 磁盘原文\n",
        "点了「否」修改必须还在"
    );
    assert!(is_dirty(&app, id), "文档应该仍是脏的");
}

/// 对话框不可用：报错且不还原（静默会让用户以为还原了、实际没变）。
#[test]
fn an_unavailable_confirm_reports_and_keeps_modifications() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    fake.set_confirm(Reply::Unavailable("no display".to_string()));
    let (_cfg, mut app) = app_with(fake);
    let id = open_disk_document(&mut app, ws.path(), "doc.md", "磁盘原文\n");
    dirty_active(&mut app);

    revert(&mut app);
    assert!(drain(&mut app));

    assert_eq!(
        content_of(&app, id),
        "脏修改 磁盘原文\n",
        "拿不到答复就不能还原"
    );
    assert!(
        app.status_bar.message.as_ref().is_some_and(|m| m.is_error),
        "对话框不可用必须报错，不能静默"
    );
}

/// 还原本身是一步**可撤销**的编辑：Ctrl+Z 把丢弃的修改找回来。
#[test]
fn revert_itself_is_undoable() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    fake.set_confirm(Reply::Yes);
    let (_cfg, mut app) = app_with(fake);
    let id = open_disk_document(&mut app, ws.path(), "doc.md", "磁盘原文\n");
    dirty_active(&mut app);

    revert(&mut app);
    assert!(drain(&mut app));
    assert_eq!(content_of(&app, id), "磁盘原文\n");

    app.execute(UiAction::Undo);
    assert_eq!(
        content_of(&app, id),
        "脏修改 磁盘原文\n",
        "撤销应把还原掉的内容找回来（还原走的是普通编辑管道）"
    );
}

/// 还原后**草稿必须清**：否则下次启动会问「是否恢复未保存修改」，
/// 把用户刚明确丢弃的内容又捡回来。
#[test]
fn revert_clears_the_stale_draft() {
    let ws = tempfile::tempdir().expect("工作区");
    let fake = FakeDialog::new();
    fake.set_confirm(Reply::Yes);
    let (_cfg, mut app) = app_with(fake);
    let id = open_disk_document(&mut app, ws.path(), "doc.md", "磁盘原文\n");
    dirty_active(&mut app);
    // 手动落一份草稿（等价于自动保存刚跑过）。
    app.services
        .documents
        .save_draft(&app.services.drafts_dir, id)
        .expect("草稿应可写入");
    let draft_file = app.services.drafts_dir.join(format!("{id}.draft"));
    assert!(draft_file.exists(), "前置条件：草稿应已存在");

    revert(&mut app);
    assert!(drain(&mut app));

    assert!(!draft_file.exists(), "还原后草稿必须被清掉");
}

/// 未命名文档没有磁盘版本：提示，不猜保存位置。
#[test]
fn reverting_an_unsaved_document_is_a_noop_notice() {
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    app.new_document().expect("新建应成功");
    let id = app.ensure_document().expect("应有活动文档");
    app.services
        .documents
        .insert_text(InsertTextCommand {
            id,
            offset: 0,
            text: "未命名内容".to_string(),
        })
        .expect("插入应成功");

    revert(&mut app);

    assert_eq!(
        content_of(&app, id),
        "未命名内容",
        "未命名文档无从还原，内容必须原样"
    );
    assert_eq!(fake.confirm_calls(), 0, "没有磁盘版本，不该弹确认框");
    let message = app.status_bar.message.as_ref().expect("应有一条提示");
    assert!(!message.is_error, "这是正常提示，不是错误");
    assert!(
        message.text.contains("未命名"),
        "提示应说明原因，实际 {}",
        message.text
    );
}
