//! V11-06 · 侧栏删除二次确认。
//!
//! # 这里钉住的是什么
//!
//! 删除是工作区里**唯一不可逆**的操作。原先它没有任何拦截，靠「只能删空目录 /
//! 单文件」把损失限制在一小步；而 `DialogPort::confirm` 是同步阻塞的原生模态框，
//! 直接在 `update()` 里调会把帧循环卡死，所以确认一直没接上。
//!
//! V11-06 把确认框与「打开 / 另存为」同走 `DialogRunner` 的后台线程，
//! 于是可以安全地放宽为递归删除。本文件钉住四件事：
//!
//! 1. **不确认就不删**（这是默认语义，也是最重要的一条）；
//! 2. 确认后才删，且待删目标清空（否则下一次删除会被「已有确认在飞」吞掉）；
//! 3. 确认文案点名目标、说明不可恢复、删目录时报出内容条数；
//! 4. 拿不到答复（对话框不可用）时**报错且不删**——静默失败最危险。
//!
//! 用 `FakeDialog` 替身：真 `rfd` 会弹出系统窗口，无人值守的 CI 上会卡住。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::{EditorApp, PendingDialog};

use crate::support::fake_dialog::{FakeDialog, Reply};

/// 造一个带替身端口的应用。
fn app_with(fake: FakeDialog) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(ConfigPaths::in_dir(dir.path()), Arc::new(fake))
        .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

/// 把后台结果收回来并应用（等价于 `update` 里的那两步）。
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

/// 装载一个工作区（删除需要 `workspace_entries` 来判断目标是不是目录）。
fn load_workspace(app: &mut EditorApp, root: &str) {
    app.request_dialog(PendingDialog::PickFolder);
    assert!(drain(app), "工作区应装载完成");
    assert_eq!(app.workspace_root(), root);
}

/// **端到端主路径**：用户没确认 → 文件必须还在。
#[test]
fn an_unconfirmed_delete_leaves_the_file_alone() {
    let ws = tempfile::tempdir().expect("工作区");
    let file = ws.path().join("keep.md");
    std::fs::write(&file, "keep me\n").expect("应可写");
    let root = ws.path().to_string_lossy().replace('\\', "/");

    // 不预置确认回应 = 用户取消（替身的默认语义）。
    let (_cfg, mut app) = app_with(FakeDialog::picking(&root));
    load_workspace(&mut app, &root);

    let target = format!("{root}/keep.md");
    app.request_delete(&target);
    assert!(
        app.delete_pending() == Some(target.as_str()),
        "删除应先悬着等确认，实际 {:?}",
        app.delete_pending()
    );
    assert!(drain(&mut app), "确认框应有答复");

    assert!(file.exists(), "用户没确认，文件必须还在");
    assert!(
        app.delete_pending().is_none(),
        "答复之后待删目标必须清空，否则下次删除会被吞"
    );
}

/// 端到端：答复「是」→ 真的删掉。
#[test]
fn confirming_a_delete_removes_the_file() {
    let ws = tempfile::tempdir().expect("工作区");
    let file = ws.path().join("gone.md");
    std::fs::write(&file, "bye\n").expect("应可写");
    let root = ws.path().to_string_lossy().replace('\\', "/");

    let fake = FakeDialog::picking(&root);
    fake.set_confirm(Reply::Yes);
    let (_cfg, mut app) = app_with(fake.clone());
    load_workspace(&mut app, &root);

    app.request_delete(&format!("{root}/gone.md"));
    assert!(drain(&mut app), "确认框应有答复");

    assert!(!file.exists(), "确认之后文件应被删除");
    assert!(app.delete_pending().is_none());
    assert_eq!(fake.confirm_calls(), 1, "确认框只该弹一次");
}

/// 确认文案要点出被删的是**什么**，并说明不可恢复（否则用户在盲签）。
#[test]
fn the_confirm_message_names_the_target() {
    let ws = tempfile::tempdir().expect("工作区");
    std::fs::write(ws.path().join("note.md"), "x\n").expect("应可写");
    let root = ws.path().to_string_lossy().replace('\\', "/");

    let fake = FakeDialog::picking(&root);
    let (_cfg, mut app) = app_with(fake.clone());
    load_workspace(&mut app, &root);

    app.request_delete(&format!("{root}/note.md"));
    assert!(drain(&mut app));

    let message = fake.last_message();
    assert!(
        message.contains("note.md"),
        "确认框应点名要删的文件，实际 {message:?}"
    );
    assert!(
        message.contains("不可恢复"),
        "确认框应说明不可恢复，实际 {message:?}"
    );
}

/// 目录的文案要**明说会连内容一起删**——这是放宽为递归删除的前提。
#[test]
fn deleting_a_directory_warns_about_its_contents() {
    let ws = tempfile::tempdir().expect("工作区");
    let dir = ws.path().join("folder");
    std::fs::create_dir(&dir).expect("应可建目录");
    std::fs::write(dir.join("a.md"), "a\n").expect("应可写");
    std::fs::write(dir.join("b.md"), "b\n").expect("应可写");
    let root = ws.path().to_string_lossy().replace('\\', "/");

    let fake = FakeDialog::picking(&root);
    fake.set_confirm(Reply::Yes);
    let (_cfg, mut app) = app_with(fake.clone());
    load_workspace(&mut app, &root);

    app.request_delete(&format!("{root}/folder"));
    assert!(drain(&mut app));

    let message = fake.last_message();
    assert!(
        message.contains("目录"),
        "删目录的文案应说清是目录，实际 {message:?}"
    );
    assert!(message.contains("2"), "应报出目录内的条目数：{message:?}");
    assert!(
        !dir.exists(),
        "确认后目录（含其中内容）应被删除——V11-06 放宽了「只删空目录」限制"
    );
}

/// 答复「否」与取消同效：不删，且不弹错误。
#[test]
fn answering_no_is_the_same_as_cancelling() {
    let ws = tempfile::tempdir().expect("工作区");
    let file = ws.path().join("keep.md");
    std::fs::write(&file, "keep\n").expect("应可写");
    let root = ws.path().to_string_lossy().replace('\\', "/");

    let fake = FakeDialog::picking(&root);
    fake.set_confirm(Reply::No);
    let (_cfg, mut app) = app_with(fake);
    load_workspace(&mut app, &root);

    app.request_delete(&format!("{root}/keep.md"));
    assert!(drain(&mut app));

    assert!(file.exists(), "点了「否」就不该删");
    assert_eq!(app.sidebar.error, None, "「否」是正常操作，不该报错");
}

/// 对话框**不可用**时必须提示，且**不得**顺手删掉。
///
/// 静默是最坏的结果：用户以为删了、实际没删（或相反），
/// 而两种误判都会让他在错误的前提下继续操作。
#[test]
fn an_unavailable_confirm_reports_and_keeps_the_file() {
    let ws = tempfile::tempdir().expect("工作区");
    let file = ws.path().join("keep.md");
    std::fs::write(&file, "keep\n").expect("应可写");
    let root = ws.path().to_string_lossy().replace('\\', "/");

    let fake = FakeDialog::picking(&root);
    fake.set_confirm(Reply::Unavailable("no display".to_string()));
    let (_cfg, mut app) = app_with(fake);
    load_workspace(&mut app, &root);

    app.request_delete(&format!("{root}/keep.md"));
    assert!(drain(&mut app));

    assert!(file.exists(), "拿不到答复就不能删");
    assert!(
        app.sidebar.error.is_some(),
        "对话框不可用必须报错，不能静默"
    );
    assert!(
        app.delete_pending().is_none(),
        "出错后也要清空待删目标，否则用户永远删不掉任何东西"
    );
}

/// 已有一个删除在等确认时再次请求应被忽略（不该弹出第二个系统窗口）。
#[test]
fn a_second_delete_request_is_dropped_while_one_is_pending() {
    let ws = tempfile::tempdir().expect("工作区");
    std::fs::write(ws.path().join("a.md"), "a\n").expect("应可写");
    std::fs::write(ws.path().join("b.md"), "b\n").expect("应可写");
    let root = ws.path().to_string_lossy().replace('\\', "/");

    let fake = FakeDialog::picking(&root);
    let (_cfg, mut app) = app_with(fake.clone());
    load_workspace(&mut app, &root);

    app.request_delete(&format!("{root}/a.md"));
    app.request_delete(&format!("{root}/b.md"));
    assert!(
        app.delete_pending() == Some(format!("{root}/a.md").as_str()),
        "待删目标应仍是第一个，实际 {:?}",
        app.delete_pending()
    );
    assert!(drain(&mut app));
    assert_eq!(fake.confirm_calls(), 1, "只该弹一个确认框");
}
