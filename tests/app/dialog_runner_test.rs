//! 原生对话框后台执行器的用例（S6-11f）。
//!
//! # 这里测的是什么
//!
//! S6-11e 遗留项「`take_pending_dialog` 无人调用」的根因是：
//! 登记表有人写、没人读。本文件钉住三件事：
//!
//! 1. **请求真的被执行**（不是在通道里躺着）；
//! 2. **取消是静默的**（不弹错误框）；
//! 3. **同一时刻只有一个对话框**（系统窗口互相遮挡）。
//!
//! 用 `FakeDialog` 替身而不是真 `rfd`：真替身会弹出系统窗口，
//! 无人值守的 CI 上测试进程会卡在「等用户点确定」。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::dialog_runner::{DialogOutcome, DialogRequest, DialogRunner};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::shared::error::AppError;

use crate::support::fake_dialog::{FakeDialog, Reply};

/// 造一个带替身端口的执行器。
fn runner(fake: FakeDialog) -> DialogRunner {
    DialogRunner::new(Arc::new(fake))
}

/// 轮询直到拿到结果或超时（后台线程是并发的，不能只 poll 一次）。
fn wait(runner: &mut DialogRunner) -> Option<DialogOutcome> {
    for _ in 0..200 {
        if let Some(outcome) = runner.poll() {
            return Some(outcome);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    None
}

/// 结果真的会被执行器取回。
#[test]
fn a_spawned_request_comes_back_through_poll() {
    let mut runner = runner(FakeDialog::opening("/tmp/x.md"));
    assert!(!runner.is_busy(), "还没请求时不该是忙");
    runner.spawn(DialogRequest::OpenFile);
    assert!(runner.is_busy(), "请求发出后应标记为忙");
    let outcome = wait(&mut runner).expect("应拿到结果");
    assert!(!runner.is_busy(), "取回结果后应回到空闲");
    match outcome {
        DialogOutcome::OpenFile(Ok(p)) => assert_eq!(p.as_str(), "/tmp/x.md"),
        other => panic!("应回填打开结果，实际 {other:?}"),
    }
}

/// 取消返回 `Cancelled` 而不是空路径。
#[test]
fn cancelling_is_reported_as_cancelled() {
    let mut runner = runner(FakeDialog::new());
    runner.spawn(DialogRequest::OpenFile);
    let outcome = wait(&mut runner).expect("应拿到结果");
    assert!(outcome.is_cancelled());
    assert!(matches!(
        outcome.path_result(),
        Some(Err(AppError::Cancelled))
    ));
}

/// 一次只允许一个对话框在飞。
#[test]
fn only_one_dialog_may_be_in_flight() {
    let mut runner = runner(FakeDialog::opening("/tmp/first.md"));
    runner.spawn(DialogRequest::OpenFile);
    runner.spawn(DialogRequest::PickFolder); // 应被忽略
    let outcome = wait(&mut runner).expect("应拿到第一个结果");
    match outcome {
        // 若是第二个请求的结果（PickFolder）就说明没忽略。
        DialogOutcome::OpenFile(_) => {}
        other => panic!("应只执行第一个请求，实际 {other:?}"),
    }
}

/// 被忽略的请求**不会**调到端口。
#[test]
fn a_dropped_request_never_reaches_the_port() {
    let fake = FakeDialog::new();
    let mut r = runner(fake.clone());
    r.spawn(DialogRequest::OpenFile);
    r.spawn(DialogRequest::PickFolder);
    let _ = wait(&mut r);
    let (open, _save, folder) = fake.call_counts();
    assert_eq!(open, 1, "打开只该被请求一次");
    assert_eq!(folder, 0, "第二个请求应被丢弃，压根没到端口");
}

/// `poll` 在没有结果时立即返回 `None`（**不阻塞**）。
#[test]
fn polling_an_idle_runner_returns_none_immediately() {
    let mut r = runner(FakeDialog::new());
    let start = std::time::Instant::now();
    assert!(r.poll().is_none());
    assert!(
        start.elapsed() < std::time::Duration::from_millis(50),
        "空闲轮询必须立刻返回，不能等"
    );
}

// ============================================================================
// 端到端：动作 → 执行器 → 应用副作用
// ============================================================================

/// 造一个带替身端口的应用。
fn app_with(fake: FakeDialog) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(ConfigPaths::in_dir(dir.path()), Arc::new(fake))
        .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

/// 把后台结果收回来并应用（等价于 `update` 里的那两步）。
fn drain(app: &mut EditorApp) -> bool {
    let mut got = false;
    for _ in 0..200 {
        match app.dialogs.poll() {
            Some(outcome) => {
                app.apply_dialog_outcome(outcome);
                got = true;
                break;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(5)),
        }
    }
    got
}

/// 用户选了文件 → 真的打开了（产生新标签）。
#[test]
fn picking_a_file_in_the_native_dialog_actually_opens_it() {
    let ws = tempfile::tempdir().expect("工作区");
    let file = ws.path().join("picked.md");
    std::fs::write(&file, "# hi\n").expect("应可写");
    let path_str = file.to_string_lossy().replace('\\', "/");

    let (_cfg, mut app) = app_with(FakeDialog::opening(&path_str));
    // 走**真实入口**：系统文件对话框在快速打开面板的「浏览本机文件…」按钮上
    // （`Ctrl+O` 归快速打开浮层，见 `execute` 里的注释）。
    app.request_dialog(rustmark::app::frame::PendingDialog::OpenFile);
    assert!(drain(&mut app), "应拿到对话框结果");
    assert_eq!(
        app.open_document_list().len(),
        1,
        "选中的文件应被打开成一个标签"
    );
    assert_eq!(app.current_document_path(), path_str);
}

/// 用户**取消**打开 → 什么都不发生，且**没有**错误提示。
#[test]
fn cancelling_the_open_dialog_is_silent() {
    let (_cfg, mut app) = app_with(FakeDialog::new());
    app.request_dialog(rustmark::app::frame::PendingDialog::OpenFile);
    assert!(drain(&mut app));
    assert!(app.open_document_list().is_empty(), "取消不该打开任何文档");
    let now = app.services.markdown.now().millis();
    let msg = app.status_bar.take_message(now);
    assert!(
        msg.as_ref().is_none_or(|m| !m.is_error),
        "取消是正常操作，不该报错：{msg:?}"
    );
}

/// 对话框**不可用** → 必须提示（静默会让用户以为点了没反应）。
#[test]
fn an_unavailable_dialog_is_reported() {
    let fake = FakeDialog::new();
    fake.set_open_file(Reply::Unavailable("no display".to_string()));
    let (_cfg, mut app) = app_with(fake);
    app.request_dialog(rustmark::app::frame::PendingDialog::OpenFile);
    assert!(drain(&mut app));
    let now = app.services.markdown.now().millis();
    let msg = app.status_bar.take_message(now).expect("应有提示");
    assert!(msg.is_error, "对话框不可用必须提示：{msg:?}");
}

/// 未命名文档按 `Ctrl+S` → 另存为 → 真落盘。
///
/// 这条同时验证三处：请求携带了建议文件名、后台线程真的执行了、
/// `save_to` 把内容写到了用户选的位置。
#[test]
fn save_as_writes_to_the_chosen_path() {
    let out = tempfile::tempdir().expect("输出目录");
    let target = out.path().join("saved.md");
    let target_str = target.to_string_lossy().replace('\\', "/");

    let fake = FakeDialog::saving(&target_str);
    let (_cfg, mut app) = app_with(fake.clone());
    app.close_all_documents();
    app.new_document().expect("应可新建");
    app.insert_at_cursor("# 内容").expect("应可插入");

    app.execute(UiAction::Save);
    assert!(drain(&mut app));

    assert_eq!(fake.call_counts().1, 1, "应调用了一次另存为");
    assert!(
        fake.last_suggested().ends_with(".md"),
        "建议文件名应带扩展名，实际 {:?}",
        fake.last_suggested()
    );
    let on_disk = std::fs::read_to_string(&target).expect("文件应真的落盘");
    assert!(
        on_disk.contains("内容"),
        "落盘内容应含文档正文：{on_disk:?}"
    );
    assert!(!app.current_document_dirty(), "保存后不该再是脏的");
}

/// 打开对话框确实带了 Markdown 过滤器（用户不该在「所有文件」里翻）。
#[test]
fn open_dialog_carries_the_markdown_filter() {
    let fake = FakeDialog::new();
    let (_cfg, mut app) = app_with(fake.clone());
    app.request_dialog(rustmark::app::frame::PendingDialog::OpenFile);
    assert!(drain(&mut app));
    let filters = fake.last_filters();
    assert_eq!(filters.len(), 1);
    assert!(
        filters[0].extensions.contains(&"md".to_string()),
        "过滤器应含 md，实际 {:?}",
        filters[0].extensions
    );
}

/// 选择文件夹 → 真的扫描并装载工作区。
#[test]
fn picking_a_folder_loads_the_workspace() {
    let ws = tempfile::tempdir().expect("工作区");
    std::fs::write(ws.path().join("m.md"), "x\n").expect("应可写");
    let root_str = ws.path().to_string_lossy().replace('\\', "/");

    let (_cfg, mut app) = app_with(FakeDialog::picking(&root_str));
    app.request_dialog(rustmark::app::frame::PendingDialog::PickFolder);
    assert!(drain(&mut app));
    assert!(app.workspace_entries.is_loaded(), "工作区应已装载");
    assert!(
        app.workspace_entries
            .items
            .iter()
            .any(|(p, _)| p.ends_with("m.md")),
        "工作区条目应含刚选中的文件"
    );
}
