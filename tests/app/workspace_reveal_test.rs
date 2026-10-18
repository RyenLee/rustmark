//! 欢迎页「打开文件夹」后的界面切换（问题 064）。
//!
//! # 钉住的不变量
//!
//! 1. **首次打开（用户没决定过侧栏开合）**：离开欢迎页 + 侧栏自动展开 +
//!    切到文件面板 + 根节点是刚打开的文件夹（「一层」由树投影保证）；
//! 2. **切换文件夹**：旧工作区的展开项被清空——换根后旧路径全部失效；
//! 3. **侧栏已隐藏（用户明确决定过收起）**：打开文件夹不强制展开；
//! 4. **自动展开会被记住**：帧末落盘后重启仍是展开（与 062 的持久化合流）。
//!
//! 全部走真实入口 `apply_dialog_outcome`（对话框结果的唯一应用通路，
//! 公开就是为了让集成测试驱动它）——若测试自己另写一遍「拿到路径就打开」，
//! 「通路没接线」这类缺陷当场测不出来。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::dialog_runner::DialogOutcome;
use rustmark::app::frame::EditorApp;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::presentation::state::app_ui_state::SidebarTab;
use rustmark::presentation::state::sidebar_ui_state::tree_rows;
use rustmark::shared::path::FilePath;

use crate::support::fake_dialog::FakeDialog;

/// 造一个装好默认配置的应用：侧栏决策 = `None`（用户没决定过开合）。
fn app() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

/// 用同一份配置目录重新装配（模拟重启，验证持久化）。
fn restart(dir: &std::path::Path) -> EditorApp {
    EditorApp::new(
        Services::assemble_with_dialogs(ConfigPaths::in_dir(dir), Arc::new(FakeDialog::new()))
            .expect("装配应成功"),
        false,
    )
}

/// 造一个两层结构的工作区（根下一个目录 + 一个文件，目录下一个文件）。
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/guide.md"), "# 指南\n").unwrap();
    std::fs::write(root.join("readme.md"), "# 首页\n").unwrap();
    dir
}

/// 走**真实入口**应用一次「用户在对话框里选了这个文件夹」的结果。
fn open_folder(app: &mut EditorApp, root: &std::path::Path) {
    let root_str = root.to_string_lossy().replace('\\', "/");
    let path = FilePath::new(&root_str).expect("临时目录路径应合法");
    app.apply_dialog_outcome(DialogOutcome::PickFolder(Ok(path)));
}

/// 树投影的顶层行名（depth 0：根的直接子项，任何展开状态下都可见）。
fn top_level_names(app: &EditorApp) -> Vec<String> {
    let root = app.workspace_root();
    tree_rows(&app.workspace_entries.items, &root, &|p| {
        app.sidebar.is_expanded(p)
    })
    .into_iter()
    .map(|r| r.name)
    .collect()
}

/// 场景一 · 首次打开：离开欢迎页、侧栏展开、文件面板、根 + 一层可见。
///
/// 旧行为停在本页：`should_show_welcome` 只看 `active_doc`，而打开文件夹
/// 不建文档——文件夹扫完了、树装好了，屏幕上却什么都没有。
#[test]
fn opening_a_folder_from_welcome_reveals_workspace_view() {
    let (_cfg, mut app) = app();
    assert!(app.should_show_welcome(), "初始应停留在欢迎页");

    let ws = workspace();
    open_folder(&mut app, ws.path());

    assert!(
        !app.should_show_welcome(),
        "打开文件夹后应离开欢迎页（进入工作区视图）"
    );
    assert!(
        app.ui.sidebar_visible,
        "首次打开（没决定过开合）应自动展开侧栏"
    );
    assert_eq!(
        app.ui.sidebar_tab,
        SidebarTab::Files,
        "应切到文件面板——打开文件夹的意图就是看文件树"
    );
    assert_eq!(
        app.workspace_root(),
        ws.path().to_string_lossy().replace('\\', "/"),
        "根节点应是刚打开的文件夹"
    );
    // 「默认展开一层」= 根的直接子项可见（depth 0 恒可见，不依赖展开态）。
    assert_eq!(
        top_level_names(&app),
        vec!["docs".to_string(), "readme.md".to_string()],
        "根节点下应直接看到一层内容，实测：{:?}",
        top_level_names(&app)
    );
}

/// 场景二 · 切换文件夹：旧工作区的展开项不得带到新根下。
///
/// `expanded_dirs` 存的是路径字符串，换根后全部失效；不清则新根下会
/// 出现「莫名其妙」处于展开态的目录，且每换一次工作区累积一批死路径。
#[test]
fn switching_workspace_clears_stale_expansions() {
    let (_cfg, mut app) = app();
    let first = workspace();
    open_folder(&mut app, first.path());
    assert!(app.ui.sidebar_visible, "前提：首次打开已展开侧栏");

    // 用户手动展开一层（真实浏览行为）。
    let docs = format!("{}/docs", app.workspace_root());
    app.sidebar.expand(&docs);
    assert!(
        app.sidebar.is_expanded(&docs),
        "手动展开应立即生效（测试前提）"
    );

    let second = workspace();
    open_folder(&mut app, second.path());

    assert_eq!(
        app.workspace_root(),
        second.path().to_string_lossy().replace('\\', "/"),
        "根应换成新文件夹"
    );
    assert!(
        app.sidebar.expanded_dirs.is_empty(),
        "换工作区后旧展开项必须清掉，实测：{:?}",
        app.sidebar.expanded_dirs
    );
    // 清展开项不等于清内容：新根的一层仍然直接可见。
    assert_eq!(
        top_level_names(&app),
        vec!["docs".to_string(), "readme.md".to_string()],
        "新根的一层内容应照常可见"
    );
}

/// 场景三 · 侧栏已隐藏（用户明确决定过收起）：打开文件夹不强制展开。
///
/// 「尊重已决定的状态」只针对侧栏开合，不是把整个界面切换关掉——
/// 欢迎页照退、面板照切、树照画。
#[test]
fn opening_a_folder_respects_user_collapsed_decision() {
    let (_cfg, mut app) = app();
    // 用户先展开再收起：两次都走真实入口并落盘，决定变成 `Some(false)`。
    app.execute(UiAction::ToggleSidebar);
    app.persist_sidebar_visibility();
    app.execute(UiAction::ToggleSidebar);
    app.persist_sidebar_visibility();
    assert!(!app.ui.sidebar_visible, "前提：用户已把侧栏收起");

    let ws = workspace();
    open_folder(&mut app, ws.path());

    assert!(
        !app.ui.sidebar_visible,
        "用户明确收起过（Some(false)），打开文件夹不应强制展开"
    );
    assert!(!app.should_show_welcome(), "仍应离开欢迎页");
    assert_eq!(app.ui.sidebar_tab, SidebarTab::Files, "仍应切到文件面板");
    assert_eq!(
        top_level_names(&app),
        vec!["docs".to_string(), "readme.md".to_string()],
        "侧栏收起不影响树投影本身（展开后一层就在）"
    );
}

/// 场景四 · 自动展开会被记住：帧末落盘后重启仍是展开。
///
/// 与 062 的持久化链路合流——reveal 只改 UI 状态，落盘仍由帧末
/// `persist_sidebar_visibility` 统一负责（本测试直接调它，等价于
/// 真帧里 `main_view::show` 末尾那一拍）。
#[test]
fn revealed_sidebar_is_persisted_across_restart() {
    let (dir, mut app) = app();
    let ws = workspace();
    open_folder(&mut app, ws.path());
    assert!(app.ui.sidebar_visible, "前提：刚打开文件夹时自动展开了");
    app.persist_sidebar_visibility();

    let mut restarted = restart(dir.path());
    assert!(
        restarted.ui.sidebar_visible,
        "打开文件夹时的自动展开应落盘记住，重启后仍是展开"
    );

    // 反证：重启后的决策已是 `Some(true)`，再打开文件夹不改变开合。
    let second = workspace();
    open_folder(&mut restarted, second.path());
    assert!(
        restarted.ui.sidebar_visible,
        "已决定展开后，打开文件夹不改变开合"
    );
    assert_eq!(
        restarted.workspace_root(),
        second.path().to_string_lossy().replace('\\', "/"),
        "但根节点仍应换成新文件夹"
    );
}
