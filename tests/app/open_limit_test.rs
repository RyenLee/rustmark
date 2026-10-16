//! 打开文档数量上限（问题 038）。
//!
//! # 为什么需要它
//!
//! 标签条没有限额时，用户可以把整个工作区逐个点开——几十个标签一字排开，
//! 「哪个是哪个」再也认不出来，而横向滚动条也救不了认知负担。
//! 上限可在设置页调整（`EditorSettings::max_open_documents`）。
//!
//! 本文件钉住五件事：
//! 1. 达上限后**新的**打开被拒绝，并给出带上限值的提示；
//! 2. **已打开**的文档不受限额影响（走的是「切换」，不占新名额）——
//!    这条最容易被写错：把限额检查放在去重之前，就会挡住最常见的
//!    「切回那个已经开着的文档」；
//! 3. 关掉一个就腾出一个名额（限额不是单向阀）；
//! 4. 新建文档同样受限额管（它也是「多开一个标签」）；
//! 5. 限额经设置页写入即生效（实时落盘，重启后仍在）。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::shortcuts::registry::UiAction;

use crate::support::fake_dialog::FakeDialog;

fn app() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

/// 在 `dir` 下写一份文件并打开，返回其路径。
fn open(app: &mut EditorApp, dir: &std::path::Path, name: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, format!("{name}\n")).expect("写文件应成功");
    let text = path.to_string_lossy().to_string();
    app.open_path(&text).expect("打开应成功");
    text
}

/// 当前打开的文档标题列表（标签条顺序；文件标题为**去扩展名的词干**，
/// 与标签条上显示的一致）。
fn open_titles(app: &EditorApp) -> Vec<String> {
    app.open_document_list()
        .into_iter()
        .map(|d| d.title)
        .collect()
}

/// 状态栏当前文案（无消息时空串）。
fn status_text(app: &EditorApp) -> String {
    app.status_bar
        .message
        .as_ref()
        .map(|m| m.text.clone())
        .unwrap_or_default()
}

/// 主路径：达上限后拒绝新打开，提示带上限值。
#[test]
fn opening_beyond_the_limit_is_refused_with_a_notice() {
    let (dir, mut app) = app();
    app.apply_editor_field(|e| e.max_open_documents = 2);
    open(&mut app, dir.path(), "a.md");
    open(&mut app, dir.path(), "b.md");

    open(&mut app, dir.path(), "c.md");

    assert_eq!(open_titles(&app), vec!["a", "b"], "第三个应被拒绝");
    let text = status_text(&app);
    assert!(
        text.contains("上限") && text.contains('2'),
        "提示应说清已达上限及上限值，实际 {text:?}"
    );
    assert!(
        !app.status_bar.message.as_ref().is_some_and(|m| m.is_error),
        "限额拒绝是策略而非异常，不该报红"
    );
}

/// 已打开的文档在达上限后仍可打开（切换，不占名额）。
///
/// 这条是限额最容易被写坏的地方：检查若放在去重之前，
/// 「切回已经开着的文档」会被一起挡掉。
#[test]
fn already_open_documents_still_switch_at_the_limit() {
    let (dir, mut app) = app();
    app.apply_editor_field(|e| e.max_open_documents = 2);
    let a = open(&mut app, dir.path(), "a.md");
    open(&mut app, dir.path(), "b.md");

    // 名额已满，再「打开」a.md —— 应切回去而不是被拒。
    app.open_path(&a).expect("切换到已打开文档应成功");

    assert_eq!(open_titles(&app), vec!["a", "b"], "不该多出标签");
    assert!(
        app.current_document_path().ends_with("a.md"),
        "应切回 a.md，实际 {:?}",
        app.current_document_path()
    );
    assert!(
        !status_text(&app).contains("上限"),
        "切换不该触发限额提示，实际 {:?}",
        status_text(&app)
    );
    // b.md 仍然在列表里（切换不能把别人关掉）。
    assert!(open_titles(&app).contains(&"b".to_string()));
}

/// 同一文件的**另一种分隔符形态**也必须去重（Windows 拖放 / 原生对话框
/// 给 `\`，文件树与快速打开给归一化的 `/`）。
///
/// 库里存的是 `FilePath` 归一化形式；去重若只做原始字符串比较，
/// 同一文件从两条入口打开会被判成两个文档——既重复开标签，
/// 又会在达上限时把「切回已打开文档」一起挡掉（038 修复的真实缺口，
/// 由 `already_open_documents_still_switch_at_the_limit` 首先踩到）。
#[test]
fn the_same_file_with_os_native_separators_dedups() {
    let (dir, mut app) = app();
    let a = open(&mut app, dir.path(), "a.md"); // 第一次：tempdir 原生形态
    // 同一文件的另一种分隔符写法（Unix 上反之，同样覆盖）。
    let alt = a.replace('/', "\\");

    app.open_path(&alt).expect("重复打开应成功");

    assert_eq!(open_titles(&app), vec!["a"], "两种分隔符形态是同一个文档");
    assert!(
        app.current_document_path().ends_with("a.md"),
        "应切回既有文档，实际 {:?}",
        app.current_document_path()
    );
}

/// 关掉一个就腾出一个名额。
#[test]
fn closing_one_frees_a_slot() {
    let (dir, mut app) = app();
    app.apply_editor_field(|e| e.max_open_documents = 2);
    open(&mut app, dir.path(), "a.md");
    open(&mut app, dir.path(), "b.md");
    open(&mut app, dir.path(), "c.md"); // 被拒

    app.execute(UiAction::CloseTab); // 关掉当前的 b.md
    assert_eq!(open_titles(&app).len(), 1, "关掉一个后应只剩一个");

    open(&mut app, dir.path(), "c.md");
    assert_eq!(
        open_titles(&app),
        vec!["a", "c"],
        "腾出名额后 c.md 应能打开"
    );
}

/// 新建文档同样受限额管（它也是「多开一个标签」）。
#[test]
fn new_document_respects_the_limit() {
    let (dir, mut app) = app();
    app.apply_editor_field(|e| e.max_open_documents = 1);
    open(&mut app, dir.path(), "a.md");

    app.execute(UiAction::NewDocument);

    assert_eq!(open_titles(&app), vec!["a"], "新建应被限额拒绝");
    assert!(
        status_text(&app).contains("上限"),
        "新建被拒也要提示，实际 {:?}",
        status_text(&app)
    );
}

/// 限额经设置页路径写入即生效（实时落盘，重启后仍在）。
#[test]
fn the_limit_is_configurable_and_persisted() {
    let (dir, mut app) = app();
    assert_eq!(
        app.settings().editor.max_open_documents,
        12,
        "默认上限应为 12"
    );

    app.apply_editor_field(|e| e.max_open_documents = 5);
    assert_eq!(app.settings().editor.max_open_documents, 5, "应立即生效");

    // 用同一份配置目录重建应用：读的是落盘的配置。
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("重新装配应成功");
    let app2 = EditorApp::new(services, false);
    assert_eq!(
        app2.settings().editor.max_open_documents,
        5,
        "上限必须落盘，否则重启后限额悄悄变回默认值"
    );
}
