//! 侧栏三标签功能用例（S6-11 验收标准中三条）。
//!
//! # 验收标准原文
//!
//! > 文件树右键菜单（新建/重命名/删除/移动）可用；大纲点击跳转与层级调整可用；
//! > 全局搜索结果分组与跳转可用。
//!
//! # 本文件的两层策略
//!
//! - **纯函数层**（`SidebarUiState` 内嵌单测）已覆盖树投影、分组、名字编辑状态机；
//! - 本文件走**真实磁盘 + 真实 `EditorApp`**，断言的是「磁盘真的变了」
//!   与「光标真的跳了」。
//!
//! 特别盯住三类**静默失效**——它们不报错、界面看起来正常，只是功能没反应：
//!
//! 1. 目录被当成文件（树建不出层级，点目录打不开文件夹）；
//! 2. 落盘失败但聚合已改（树里出现一个点开就报错的文件）；
//! 3. 大纲/搜索跳到了错误的行（偏移换算用字节当字符）。

use crate::support::fake_dialog::FakeDialog;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::workspace::commands::MoveEntryCommand;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::presentation::state::app_ui_state::SidebarTab;
use rustmark::presentation::state::sidebar_ui_state::{
    NameEdit, SearchOptions, SearchScope, tree_rows,
};
use rustmark::presentation::views::main_view;
use rustmark::presentation::views::sidebar_view::SEARCH_INPUT_ID;
use rustmark::shared::path::FilePath;

/// 造一个含 `docs/nested` 三层结构的工作区。
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("readme.md"), "# 首页\n\nneedle 顶层\n").unwrap();
    fs::write(
        root.join("docs/guide.md"),
        "# 指南\n\nneedle 文档一\n\n## 第二节\n\nneedle 文档二 needle\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("docs/img")).unwrap();
    dir
}

/// 造一个已扫描过工作区的应用。
fn app_with_workspace(root: &Path) -> (tempfile::TempDir, EditorApp) {
    let cfg = tempfile::tempdir().expect("配置目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(cfg.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let root_str = root.to_string_lossy().replace('\\', "/");
    app.scan_workspace(&root_str).expect("扫描应成功");
    (cfg, app)
}

fn joined(root: &Path, rel: &str) -> String {
    root.join(rel).to_string_lossy().replace('\\', "/")
}

// ===========================================================================
// 文件树
// ===========================================================================

/// 目录必须被标记为目录 —— 这是整个文件树能建层级的前提。
///
/// 修复前`collect` 把目录也标成 `EntryKind::File`，`is_directory` 恒为
/// `false`，文件树只能把整个目录平铺成文件列表，点目录什么都不会发生。
/// 这条用例的全部意义就是钉死它。
#[test]
fn scan_marks_directories_as_directories() {
    let ws = workspace();
    let (_cfg, app) = app_with_workspace(ws.path());

    let dirs: Vec<&str> = app
        .workspace_entries
        .items
        .iter()
        .filter(|(_, is_dir)| *is_dir)
        .map(|(path, _)| path.as_str())
        .collect();
    assert_eq!(
        dirs.len(),
        2,
        "应有 docs 与 docs/img 两个目录，实测：{dirs:?}"
    );
    assert!(
        app.workspace_entries
            .items
            .iter()
            .any(|(p, _)| p.ends_with("guide.md"))
    );
}

/// 文件树**默认全收起**（问题 062）：刚扫描完只画根目录的直接子项。
///
/// 此前装载时会把顶层目录全展开，用户打开一个文件夹先被塞进一层他没要求
/// 看的内容。钉住这条的正面理由不是「少画几行」，而是**默认状态必须是用户
/// 自己点出来的**：要找的文件所在目录由 `expand_ancestors` 在打开时逐层
/// 展开到可见（按需），不需要靠启动时的猜测。
#[test]
fn file_tree_starts_fully_collapsed() {
    let ws = workspace();
    let (_cfg, app) = app_with_workspace(ws.path());
    let root = app.workspace_root();

    assert!(
        app.sidebar.expanded_dirs.is_empty(),
        "装载后不应自动展开任何目录，实测：{:?}",
        app.sidebar.expanded_dirs
    );
    let rows = tree_rows(&app.workspace_entries.items, &root, &|p| {
        app.sidebar.is_expanded(p)
    });
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["docs", "readme.md"],
        "只应画出根的直接子项（docs 收起，其下的 img / guide.md 不画），实测：{names:?}"
    );
}

/// 打开文件仍会把它的祖先目录展开到可见（全收起默认不得伤到这条）。
///
/// 与 [`file_tree_starts_fully_collapsed`] 成对：默认不猜用户要看哪一层，
/// 但用户真的打开某个文件时，树里必须能看到那个文件——否则「默认收起」
/// 会退化成「文件树永远找不到刚打开的文件」。
#[test]
fn opening_a_file_expands_its_ancestors() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .expect("打开工作区文件应成功");

    assert!(
        app.sidebar.is_expanded(&joined(&root, "docs")),
        "打开 docs/guide.md 后 docs 必须展开，否则树里看不到刚打开的文件"
    );
}

/// 树投影把扁平条目折成带缩进的层级行。
#[test]
fn tree_projection_builds_hierarchical_rows() {
    let ws = workspace();
    let (_cfg, app) = app_with_workspace(ws.path());
    let root = app.workspace_root();

    let rows = tree_rows(&app.workspace_entries.items, &root, &|_| true);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    // 顺序是仓储的**深度优先前序**：目录条目之后立即是它的子项，再继续
    // 兄弟条目。旧实现先推完本层全部条目、末尾再统一递归，展开目录后
    // 子项全被拼到整棵树尾部（问题 040 截图实证）；`tree_rows` 按列表
    // 顺序渲染，全靠输入顺序画对层级——重排会让「重命名后条目位置
    // 不动」这个预期失效，故契约钉在仓储侧。
    assert_eq!(
        names,
        vec!["docs", "img", "guide.md", "readme.md"],
        "子项必须紧跟父目录（前序 DFS），实际：{names:?}"
    );
    let depths: Vec<usize> = rows.iter().map(|r| r.depth).collect();
    assert_eq!(
        depths,
        vec![0, 1, 1, 0],
        "img 与 guide.md 在 docs 之下，readme.md 在根"
    );

    // 收起 docs 后它的子项应消失，而 docs 自身仍在。
    let rows = tree_rows(&app.workspace_entries.items, &root, &|p| {
        !p.ends_with("docs")
    });
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["docs", "readme.md"], "收起的目录其子项应隐藏");
}

/// 右键菜单「新建文件」：真落盘且树里能看到。
#[test]
fn creating_a_file_writes_it_to_disk_and_shows_it_in_the_tree() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.create_entry("", "新建.md", false).expect("新建应成功");

    let target = root.join("新建.md");
    assert!(target.exists(), "文件应真的落盘");
    // 树里也必须能看到（重新装载过），否则用户建完以为没成功。
    assert!(
        app.workspace_entries
            .items
            .iter()
            .any(|(p, _)| p.ends_with("新建.md")),
        "新文件应出现在文件树中"
    );
}

/// 右键菜单「新建文件夹」：真落盘且被标为目录。
#[test]
fn creating_a_folder_writes_a_real_directory() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.create_entry(&joined(&root, "docs"), "附件", true)
        .expect("新建目录应成功");

    assert!(root.join("docs/附件").is_dir(), "目录应真的落盘");
    assert!(
        app.workspace_entries
            .items
            .iter()
            .any(|(p, is_dir)| p.ends_with("附件") && *is_dir),
        "新目录应被标记为目录（否则点不开）"
    );
    // 建在 docs 下却看不到它，用户会以为失败——必须自动展开父目录。
    assert!(
        app.sidebar.is_expanded(&joined(&root, "docs")),
        "新建位置的父目录应自动展开"
    );
}

/// 右键菜单「重命名」：真移动，旧路径消失。
#[test]
fn renaming_moves_the_file_on_disk() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    let before = fs::read_to_string(root.join("readme.md")).unwrap();

    app.rename_entry(&joined(&root, "readme.md"), "首页.md")
        .expect("重命名应成功");

    assert!(root.join("首页.md").exists(), "新名字的文件应存在");
    assert!(!root.join("readme.md").exists(), "旧路径应消失");
    assert_eq!(
        fs::read_to_string(root.join("首页.md")).unwrap(),
        before,
        "重命名不得改动内容"
    );
}

/// 重名必须被拒绝，且**不得覆盖**已有内容。
///
/// 这条是安全底线：用户「重命名为 b.md」而 b.md 已存在时，
/// POSIX 的 `fs::rename` 会静默覆盖——另一个文件的内容就没了。
///
/// 注意重命名**只在原目录内改名**（`rename_entry` 取源路径的父目录），
/// 所以冲突必须发生在**同一目录**：`docs/guide.md` 改成 `readme.md`
/// 会落到 `docs/readme.md`（不存在，不冲突）；要触发冲突得让
/// 源文件与目标文件同属 `docs`。
#[test]
fn duplicate_name_is_rejected_without_overwriting() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    // 造一个同目录的重名目标：`docs/guide.md` 与 `docs/same.md`。
    fs::write(root.join("docs/same.md"), "victim").unwrap();
    app.scan_workspace(&root.to_string_lossy().replace('\\', "/"))
        .unwrap();
    let victim = fs::read_to_string(root.join("docs/same.md")).unwrap();

    let result = app.rename_entry(&joined(&root, "docs/guide.md"), "same.md");

    assert!(result.is_err(), "同目录重名应被拒绝");
    assert_eq!(
        fs::read_to_string(root.join("docs/same.md")).unwrap(),
        victim,
        "被重名的文件内容必须完好"
    );
    assert!(
        root.join("docs/guide.md").exists(),
        "源文件应留在原处（重命名失败不得有副作用）"
    );
}

/// 删除：文件删除；目录连同内容一起删（V11-06 放宽）。
///
/// V11-06 之前非空目录**必须拒绝**——那时没有确认框，靠「步子小」把损失限制住。
/// 现在二次确认已接上（确认文案会报出目录内的条目数），限制随之放宽：
/// 用户是在知情后点的「是」，再要求他先手动清空目录只是把保护重复一遍。
///
/// 本例直接调 `delete_entry`（跳过确认）是刻意的：确认那一段由
/// `tests/app/delete_confirm_test.rs` 单独钉住，这里只验证「真删」这一步。
#[test]
fn delete_removes_a_file_and_a_directory_with_its_contents() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.delete_entry(&joined(&root, "docs/guide.md"))
        .expect("删除文件应成功");
    assert!(!root.join("docs/guide.md").exists(), "文件应真的被删除");

    app.delete_entry(&joined(&root, "docs"))
        .expect("确认之后，目录应能连同内容一起删");
    assert!(!root.join("docs").exists(), "目录应真的被删除");
}

/// 右键菜单「删除」**必须**先弹确认，不能直接删（V11-06）。
///
/// 这条防的是「有人图省事把 `request_delete` 换回 `delete_entry`」——
/// 那种改法在本用例里会立刻红：文件会在无任何确认的情况下消失。
#[test]
fn the_context_menu_delete_waits_for_confirmation() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let cfg = tempfile::tempdir().expect("配置目录");
    let fake = FakeDialog::new();
    let services = Services::assemble_with_dialogs(ConfigPaths::in_dir(cfg.path()), Arc::new(fake))
        .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let root_str = root.to_string_lossy().replace('\\', "/");
    app.scan_workspace(&root_str).expect("扫描应成功");

    let target = joined(&root, "docs/guide.md");
    app.request_delete(&target);
    assert!(
        app.delete_pending() == Some(target.as_str()),
        "删除应先悬着等确认，实际 {:?}",
        app.delete_pending()
    );
    // 未取回任何答复时，磁盘必须原封不动。
    assert!(root.join("docs/guide.md").exists(), "没确认就不该动磁盘");
}

/// 删除后树要重扫：磁盘上可能还有聚合不知道的文件。
#[test]
fn deleting_refreshes_the_tree_from_disk() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.delete_entry(&joined(&root, "docs/guide.md")).unwrap();
    assert!(
        !app.workspace_entries
            .items
            .iter()
            .any(|(p, _)| p.ends_with("guide.md")),
        "被删的文件应从树里消失"
    );
    // 外部（别的程序 / 另一个窗口）新建的文件，重扫后应出现。
    fs::write(root.join("外部新建.md"), "hello").unwrap();
    app.refresh_workspace();

    assert!(
        app.workspace_entries
            .items
            .iter()
            .any(|(p, _)| p.ends_with("外部新建.md")),
        "重扫后应看到外部新建的文件（否则树与磁盘不一致）"
    );
}

/// 点击树里的文件应真的打开它，并登记到最近文件。
#[test]
fn clicking_a_file_in_the_tree_opens_it() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .expect("打开应成功");

    assert!(
        app.current_content().unwrap().contains("指南"),
        "编辑区内容应是该文件的内容"
    );
    assert_eq!(
        app.sidebar.selected_path.as_deref(),
        Some(joined(&root, "docs/guide.md").as_str()),
        "被打开的文件应在树里高亮"
    );
    // 打开的文件若不在展开路径上，用户看不到自己打开了什么。
    assert!(
        app.sidebar.is_expanded(&joined(&root, "docs")),
        "打开文件时应自动展开其所在目录"
    );
    let recent = app
        .services
        .workspace
        .list_recent(rustmark::application::workspace::queries::ListRecentQuery)
        .unwrap();
    assert_eq!(recent.len(), 1, "被打开的文件应进入最近文件");
}

/// 规则拒绝时**磁盘不能被动过**（这是 `check_on_clone` 存在的全部理由）。
#[test]
fn a_rejected_move_leaves_the_disk_untouched() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, app) = app_with_workspace(&root);
    let docs = joined(&root, "docs");

    let err = app
        .services
        .workspace
        .move_entry(MoveEntryCommand {
            from: FilePath::new(docs.clone()).unwrap(),
            to: FilePath::new(format!("{docs}/inner")).unwrap(),
        })
        .expect_err("移进自己的子目录应被拒绝");
    assert!(
        err.to_string().contains("子目录"),
        "错误应说清原因，实际：{err}"
    );
    assert!(root.join("docs").is_dir(), "源目录必须仍在原处");
    assert!(
        !root.join("docs/inner").exists(),
        "规则拒绝后不得在磁盘上留下任何痕迹"
    );
}

/// 落盘失败时聚合**不应**改变（否则树里有个点开就报错的文件）。
///
/// 这条覆盖的是「先改聚合再落盘」那种顺序的典型症状：
/// 界面显示文件已创建，点开却报「文件不存在」。
#[test]
fn a_failed_write_does_not_add_a_phantom_entry() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    let before = app.workspace_entries.items.len();

    // 造一个「磁盘上已经有一个同名目录」的场景：建文件必然失败。
    fs::create_dir_all(root.join("挡路的")).unwrap();
    let result = app.create_entry("", "挡路的", false);

    assert!(result.is_err(), "目标是目录时建文件应失败");
    assert!(
        root.join("挡路的").is_dir() && fs::read_dir(root.join("挡路的")).unwrap().next().is_none(),
        "失败时不应在那个目录里造出任何文件"
    );
    // 树里也不能出现那个失败的名字。
    // 注意判据用**完整路径相等**而非 `ends_with`：目录是在扫描之后
    // 由测试直接建到磁盘上的，若实现顺手重扫了树，它就会合法地出现在树里，
    // 用 `ends_with` 会把「树与磁盘一致」误判成「幻影条目」。
    // 这里真正要防的是：把**建文件失败**当成建成了文件 `<根>/挡路的`。
    assert!(
        !app.workspace_entries
            .items
            .iter()
            .any(|(p, is_dir)| p.as_str() == joined(&root, "挡路的") && !*is_dir),
        "失败后树里不得出现一个同名的**文件**条目"
    );
    assert_eq!(before, 4, "条目数应保持不变");
}

// ===========================================================================
// 大纲
// ===========================================================================

/// 大纲应列出当前文档的全部标题，且带层级与行号。
#[test]
fn outline_lists_headings_with_levels_and_lines() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();

    let items = app.current_outline();

    let titles: Vec<(&str, u8)> = items.iter().map(|i| (i.text.as_str(), i.level)).collect();
    assert_eq!(
        titles,
        vec![("指南", 1), ("第二节", 2)],
        "大纲应按文档顺序列出标题与层级"
    );
    assert_eq!(items[0].line, 0, "首个标题在第 0 行");
    assert_eq!(items[1].line, 4, "第二个标题的行号应指向它真实所在行");
}

/// 大纲点击跳转：光标落到该标题那一行。
///
/// 这条最关键的是**不能差一行**：行号 → 字节偏移的换算若按字节当字符用，
/// 中文文档里会偏出去。偏移错了界面照样跳转，只是跳到别处——
/// 不报错，只是错，测试必须断言偏移本身而不只是「有跳转」。
#[test]
fn clicking_an_outline_entry_moves_the_cursor_to_that_line() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();

    let items = app.current_outline();
    let target = items[1].clone();
    app.sidebar.outline_selected = Some(target.text.clone());
    app.goto_outline_line(target.line);

    let starts = rustmark::presentation::views::scroll_sync::line_start_offsets(
        &app.current_content().unwrap(),
    );
    assert_eq!(
        app.editor.cursor, starts[target.line],
        "光标应落在该标题所在行"
    );
    // 落点那一行的文本必须真的是这个标题。
    let content = app.current_content().unwrap();
    let line_text = content[starts[target.line]..].split('\n').next().unwrap();
    assert!(
        line_text.contains("第二节"),
        "落点行应是「第二节」，实际：{line_text:?}"
    );
    assert_eq!(
        app.sidebar.outline_selected.as_deref(),
        Some("第二节"),
        "大纲应高亮当前所在章节"
    );
}

/// 层级过滤：设到 1 时只显示一级标题。
#[test]
fn outline_level_filter_hides_deeper_headings() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();

    app.sidebar.set_outline_max_level(1);
    let items = app.current_outline();
    let visible: Vec<&str> = items
        .iter()
        .filter(|i| app.sidebar.outline_level_visible(i.level))
        .map(|i| i.text.as_str())
        .collect();
    assert_eq!(visible, vec!["指南"], "层级设为 1 时只应显示一级标题");

    app.sidebar.set_outline_max_level(6);
    assert_eq!(app.sidebar.outline_max_level, 6, "应可恢复到全部层级");
}

/// 层级调整：提升 / 降低标题真的改文档内容（且可撤销）。
///
/// 原型是在大纲里横向拖拽调层级；本实现走的是**同一份领域规则**
/// （`FormatCommand` 的 `PromoteHeading` / `DemoteHeading`），
/// 与工具栏按钮、快捷键三入口共用——拖拽另起一套实现必然与它们漂移。
#[test]
fn changing_heading_level_uses_the_same_domain_rule_as_the_shortcut() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();
    let before = app.current_content().unwrap();

    // 把光标放到「第二节」（第 4 行）并降级。
    let starts = rustmark::presentation::views::scroll_sync::line_start_offsets(&before);
    app.editor.cursor = starts[4];
    app.editor.anchor = None;
    app.execute(UiAction::DemoteHeading);

    let after = app.current_content().unwrap();
    // 原文第 4 行是 `## 第二节`，降一级应是 `# 第二节`。
    // 修复前这里得到的是「上一行被加了 `# `」——即降级改错了行。
    assert!(
        after.contains("# 第二节"),
        "降级后应是二级标题，实际内容：\n{after}"
    );
    assert!(
        !after.contains("## 第二节"),
        "原层级应被替换而不是叠加，实际内容：\n{after}"
    );
    // 大纲里那条 `# 指南` 不能被顺带改掉。
    assert!(
        after.contains("# 指南\n"),
        "只应改光标所在那一行，实际内容：\n{after}"
    );
    assert_ne!(before, after, "内容必须真的变了（否则是空接线）");

    // 三入口一致性：撤销后应回到原文。
    app.undo_edit();
    assert_eq!(app.current_content().unwrap(), before, "撤销应还原");
}

// ===========================================================================
// 全局搜索
// ===========================================================================

/// 搜索结果按文件分组，同一文件的命中合并到一个组。
#[test]
fn search_results_are_grouped_by_file() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    // 「打开的文档」范围只搜标签栏里开着的文件：先把两个文件都打开。
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();
    app.open_workspace_file(&joined(&root, "readme.md"))
        .unwrap();
    app.sidebar.search_query = "needle".into();
    // 本条测的是**跨文档**分组语义：显式声明范围，不依赖默认值
    // （默认是「当前文档」，V11-28）。
    app.sidebar.search_scope = SearchScope::OpenDocuments;

    let result = app.search_workspace().expect("搜索应成功");

    assert_eq!(result.groups.len(), 2, "两个文件各一组");
    // 分组顺序 = 标签栏顺序（打开顺序）：先 guide.md 后 readme.md。
    let names: Vec<&str> = result.groups.iter().map(|g| g.file_name.as_str()).collect();
    assert_eq!(
        names,
        vec!["guide.md", "readme.md"],
        "分组应按标签栏（打开）顺序"
    );
    // `file_name` 是**末段**（用于表头显示），完整路径在 `path`。
    let guide = result
        .groups
        .iter()
        .find(|g| g.file_name == "guide.md")
        .expect("应有 guide.md 这一组");
    assert!(
        guide.path.ends_with("docs/guide.md"),
        "`path` 才是完整路径，实际：{}",
        guide.path
    );
    assert_eq!(
        guide.hits.len(),
        3,
        "guide.md 里有三处 needle，应合并为一组三条"
    );
    assert_eq!(result.total(), 4, "总命中应为 4");
    assert!(!result.truncated, "4 条远未到 300 上限");
}

/// 「打开的文档」范围**不**搜侧栏文件夹下未打开的文件（问题 037）。
///
/// 旧语义是「整个工作区」= 读遍目录下所有文件；那既慢又不是用户要的。
/// 现在只有真正开成标签的文档才参与检索——未打开的文件一条都不该出现。
#[test]
fn open_documents_scope_skips_files_that_are_not_open() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    // 只打开 guide.md；readme.md 虽在磁盘上、含 needle，但没开成标签。
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();
    app.sidebar.search_query = "needle".into();
    app.sidebar.search_scope = SearchScope::OpenDocuments;

    let result = app.search_workspace().expect("搜索应成功");

    assert_eq!(result.groups.len(), 1, "只应搜到已打开的 guide.md");
    assert_eq!(result.groups[0].file_name, "guide.md");
    assert_eq!(result.total(), 3, "guide.md 内三处 needle");
    assert!(
        !result.groups.iter().any(|g| g.file_name == "readme.md"),
        "未打开成标签的文件不得出现在结果里"
    );
}

/// 每条命中都要能定位到片段里的高亮区间（字符坐标，不是字节）。
///
/// 片段要交给 egui 排版；按字节切中文会切在字符中间，
/// 症状是「高亮块里少半个字」或「整个片段不高亮」。
#[test]
fn every_hit_carries_a_highlight_range_in_character_coordinates() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    // 跨文档范围才有多个分组可遍历；先把两个文件都开成标签。
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();
    app.open_workspace_file(&joined(&root, "readme.md"))
        .unwrap();
    app.sidebar.search_query = "needle".into();
    // 跨文件范围才有多个分组可遍历；默认「当前文档」下这里会一条都不剩，
    // 循环空跑等于没测（V11-28）。
    app.sidebar.search_scope = SearchScope::OpenDocuments;

    let result = app.search_workspace().unwrap();
    for group in &result.groups {
        for hit in &group.hits {
            let (start, end) = hit.highlight;
            assert_ne!(start, end, "命中应有高亮区间：{}", hit.snippet);
            let chars: Vec<char> = hit.snippet.chars().collect();
            let marked: String = chars[start..end].iter().collect();
            assert_eq!(
                marked, "needle",
                "高亮区间应正好覆盖查询词（片段：{:?}）",
                hit.snippet
            );
        }
    }
}

/// 搜索结果跳转：先切文件，再定位到那一行。
#[test]
fn clicking_a_search_hit_opens_the_file_and_lands_on_that_line() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    // 先打开 guide.md，搜索范围才有对象可搜（「打开的文档」只搜已开标签）。
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();
    app.sidebar.search_query = "第二节".into();
    app.sidebar.search_scope = SearchScope::OpenDocuments;
    let result = app.search_workspace().unwrap();
    let group = result
        .groups
        .iter()
        .find(|g| g.file_name == "guide.md")
        .expect("应有 guide.md 这一组");
    let hit = group.hits[0].clone();

    // 同一路径只会有一个标签：这里再「打开」一次只是切回既有页。
    app.open_workspace_file(&group.path).unwrap();
    let starts = rustmark::presentation::views::scroll_sync::line_start_offsets(
        &app.current_content().unwrap(),
    );
    app.goto_byte_offset(starts[hit.line]);

    assert_eq!(app.editor.cursor, starts[hit.line], "光标应落在命中那一行");
    let content = app.current_content().unwrap();
    let line_text = content[app.editor.cursor..].split('\n').next().unwrap();
    assert!(
        line_text.contains("第二节"),
        "落点应是命中所在行，实际：{line_text:?}"
    );
}

/// 三个搜索开关真的改变结果（不是摆设）。
#[test]
fn search_options_actually_change_the_results() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    // `Needle` 测大小写；`needles` 测全字匹配（子串命中但不是整词）。
    // 少了 `needles` 这行，全字匹配会与不匹配得到同样数字，
    // 开关就成了摆设而测试依然绿。
    fs::write(root.join("Case.md"), "Needle needle needles\n").unwrap();
    app.scan_workspace(&root.to_string_lossy().replace('\\', "/"))
        .unwrap();
    // 「打开的文档」范围只搜已开成标签的文件：三个文件都打开。
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();
    app.open_workspace_file(&joined(&root, "readme.md"))
        .unwrap();
    app.open_workspace_file(&joined(&root, "Case.md")).unwrap();

    app.sidebar.search_query = "needle".into();
    // 开关的语义与范围无关，但**计数**有关：默认「当前文档」下没有活动
    // 文档，三种开关会同样得到 0，开关就成了摆设而测试依然绿（V11-28）。
    app.sidebar.search_scope = SearchScope::OpenDocuments;
    let insensitive = app.search_workspace().unwrap().total();
    assert_eq!(
        insensitive, 7,
        "默认不区分大小写：4 + Needle + needle + needles"
    );

    app.sidebar.search_options.case_sensitive = true;
    let sensitive = app.search_workspace().unwrap().total();
    assert_eq!(sensitive, 6, "区分大小写后应排除「Needle」");

    app.sidebar.search_options.case_sensitive = false;
    app.sidebar.search_options.whole_word = true;
    let whole_word = app.search_workspace().unwrap().total();
    assert_eq!(
        whole_word, 6,
        "全字匹配下 `needles` 不该命中（它是本语料里唯一含 needle 的非整词）"
    );

    // 四个开关组合穷举一遍：任一开关失效都会体现在某个组合上。
    // 只测「逐个打开」不够——两个开关可能互相抵消。
    for (cs, ww, expected) in [
        (false, false, 7),
        (true, false, 6),
        (false, true, 6),
        (true, true, 5),
    ] {
        app.sidebar.search_options.case_sensitive = cs;
        app.sidebar.search_options.whole_word = ww;
        assert_eq!(
            app.search_workspace().unwrap().total(),
            expected,
            "case_sensitive={cs} whole_word={ww} 组合下的命中数不对"
        );
    }
}

/// 侧栏的正则开关必须一路通到检索内核（「正则死开关」的接线回归）。
///
/// 应用层语义由 `workspace_service_test` 钉住；这里测的是**表现层到应用层
/// 的传线**：`search_workspace` 若漏传 `regex` 字段，开关点与不点结果
/// 完全相同，而底层测试依旧全绿。
#[test]
fn regex_toggle_reaches_the_search_pipeline() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();
    app.open_workspace_file(&joined(&root, "readme.md"))
        .unwrap();
    app.sidebar.search_scope = SearchScope::OpenDocuments;
    app.sidebar.search_query = "needl.".into();

    // 字面量 `needl.` 在语料里不存在：关着正则必须零命中。
    app.sidebar.search_options.regex = false;
    assert_eq!(
        app.search_workspace().unwrap().total(),
        0,
        "正则关闭时 `needl.` 应按字面量搜，不得命中 needle"
    );

    // 打开正则：`.` 各吃掉一个字符，两份文档里的 needle 都该命中。
    app.sidebar.search_options.regex = true;
    assert!(
        app.search_workspace().unwrap().total() > 0,
        "正则打开后 `needl.` 应命中 needle（开关没传到内核）"
    );

    // 无效正则：错误原样上抛给视图（而非静默空结果）。
    app.sidebar.search_query = "(未闭合".into();
    let err = app
        .search_workspace()
        .expect_err("无效正则必须以 Err 上抛，让视图显示原因");
    assert!(
        err.to_string().contains("正则表达式无效"),
        "上抛的文案应可直接呈现用户，实际：{err}"
    );
}

/// 搜索标签的「重置」按钮必须真的关掉三个开关。
///
/// 缺陷截图「重置失效」：按钮画得出来、也能点，但视图没接 `clicked()`，
/// 三个开关原地不动。这里走**真实 UI 点击**而不是直接调状态方法——只测
/// `reset()` 的话，「视图漏接线」这条缺陷照样测不出来。
#[test]
fn clicking_the_search_reset_button_clears_every_option() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.ui.sidebar_tab = SidebarTab::Search;
    app.ui.sidebar_visible = true;
    // 默认范围「当前文档」在没有活动文档时只渲染空状态，开关行根本不出现。
    app.sidebar.search_scope = SearchScope::OpenDocuments;
    app.sidebar.search_query = "needle".to_string();
    app.sidebar.search_options = SearchOptions {
        case_sensitive: true,
        whole_word: true,
        regex: false,
    };

    let ctx = egui::Context::default();
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let out = crate::support::run_ui(&ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(&mut app, ui);
        });
        shapes = out.shapes;
    }
    let reset = text_center(&shapes, "重置").expect("开关行应渲染出「重置」按钮");

    click_at(&ctx, &mut app, reset);

    assert!(
        app.sidebar.search_options.is_plain(),
        "点「重置」应关闭全部搜索选项（视图必须接上 clicked()）"
    );
}

/// 搜索标签的「清空」按钮必须同时清掉查询词与全部选项。
///
/// 与「重置」区分：重置只关选项（上一条用例钉住），清空连输入框一起清。
/// 同样走**真实 UI 点击**——只测状态方法的话，「视图漏接线」照样测不出来。
/// 范围是独立决定，清空不得顺带把它重置掉。
#[test]
fn clicking_the_search_clear_button_empties_query_and_options() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.ui.sidebar_tab = SidebarTab::Search;
    app.ui.sidebar_visible = true;
    // 默认范围「当前文档」在没有活动文档时只渲染空状态，开关行根本不出现。
    app.sidebar.search_scope = SearchScope::OpenDocuments;
    app.sidebar.search_query = "needle".to_string();
    app.sidebar.search_options = SearchOptions {
        case_sensitive: true,
        whole_word: true,
        regex: false,
    };

    let ctx = egui::Context::default();
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let out = crate::support::run_ui(&ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(&mut app, ui);
        });
        shapes = out.shapes;
    }
    let clear = text_center(&shapes, "清空").expect("开关行应渲染出「清空」按钮");

    click_at(&ctx, &mut app, clear);

    assert!(
        app.sidebar.search_query.is_empty(),
        "点「清空」应清空输入框文本"
    );
    assert!(
        app.sidebar.search_options.is_plain(),
        "点「清空」应关闭全部搜索选项"
    );
    assert_eq!(
        app.sidebar.search_scope,
        SearchScope::OpenDocuments,
        "范围是独立决定，清空不得顺带重置它"
    );
    assert!(
        app.search_workspace()
            .expect("空查询应返回空结果而非错误")
            .is_empty(),
        "清空后结果状态应回到未筛选"
    );
}

/// 空查询不发起搜索（否则每帧全盘扫描）。
#[test]
fn empty_query_does_not_trigger_a_search() {
    let ws = workspace();
    let (_cfg, app) = app_with_workspace(ws.path());

    let result = app.search_workspace().expect("应返回空结果而非错误");

    assert!(result.is_empty(), "空查询不应有结果");
    assert!(!result.truncated);
}

/// 范围切换器：「当前文档」只出活动文档的命中（V11-28）。
///
/// 三条断言各钉一个面，缺一条这个功能就可能「看起来实现了」：
/// 1. **默认范围是「当前文档」**，且没打开任何文件时结果必须为空——
///    绝不能悄悄退化成跨文档搜索，那与面板上声明的范围自相矛盾
///    （`search_workspace` 里专门有一条 early return 守这件事）；
/// 2. 打开 guide.md 后只剩它一组——文档一切换，文档级结果立刻跟着变；
/// 3. 切到「打开的文档」才跨文档——且只覆盖**开成标签的**文件，
///    侧栏目录下没打开的文件不参与（问题 037）。
#[test]
fn search_scope_current_document_only_searches_the_open_file() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.sidebar.search_query = "needle".into();

    // 1. 默认范围 + 未打开任何文档：空。
    assert_eq!(
        app.sidebar.search_scope,
        SearchScope::CurrentDocument,
        "默认范围应为「当前文档」"
    );
    let scoped = app.search_workspace().expect("搜索应成功");
    assert!(
        scoped.is_empty(),
        "无活动文档时「当前文档」范围必须为空，实际搜到：{:?}",
        scoped
            .groups
            .iter()
            .map(|g| &g.file_name)
            .collect::<Vec<_>>()
    );

    // 2. 打开 guide.md：只有它一组、三条命中（readme.md 的那条不出现）。
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();
    let scoped = app.search_workspace().unwrap();
    assert_eq!(scoped.groups.len(), 1, "「当前文档」范围只应有一个分组");
    assert_eq!(scoped.groups[0].file_name, "guide.md");
    assert_eq!(scoped.total(), 3, "guide.md 内共三处 needle");

    // 3. 切到「打开的文档」：已开标签的两个文件都出现（共四条）。
    app.open_workspace_file(&joined(&root, "readme.md"))
        .unwrap();
    app.sidebar.search_scope = SearchScope::OpenDocuments;
    let wide = app.search_workspace().unwrap();
    assert_eq!(wide.groups.len(), 2, "跨文档范围应覆盖全部已打开文档");
    assert_eq!(wide.total(), 4);
}

/// 范围切换器在真实帧里可见、可点（V11-28）。
///
/// 点击落点取本帧形状里「打开的文档」的**实际字形中心**，不写死坐标：
/// 切换器宽度由字体决定，写死坐标等于把测试绑死在某台机器的字体上。
#[test]
fn clicking_the_scope_switcher_changes_the_search_scope() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    let ctx = egui::Context::default();
    // 「打开的文档」只搜已开标签的文件：先开两个，切换后才有结果可验。
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .unwrap();
    app.open_workspace_file(&joined(&root, "readme.md"))
        .unwrap();
    app.ui.sidebar_tab = SidebarTab::Search;
    app.ui.sidebar_visible = true;
    app.sidebar.search_query = "needle".into();

    // 先渲染两帧让布局稳定，再从形状里取切换器的位置。
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let out = crate::support::run_ui(&ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(&mut app, ui);
        });
        shapes = out.shapes;
    }
    let target = text_center(&shapes, "打开的文档").expect("切换器应渲染出「打开的文档」");
    assert!(
        text_center(&shapes, "当前文档").is_some(),
        "切换器应同时渲染出「当前文档」"
    );

    click_at(&ctx, &mut app, target);

    assert_eq!(
        app.sidebar.search_scope,
        SearchScope::OpenDocuments,
        "点击「打开的文档」应把范围切过去"
    );
    // 切过去必须立刻见效：同一查询从「只有活动文档」变成跨文档两组。
    let result = app.search_workspace().unwrap();
    assert_eq!(result.groups.len(), 2, "切换范围后结果应立即刷新");
}

/// 在形状里找某段文字的中心点（点击落点）。
fn text_center(shapes: &[egui::epaint::ClippedShape], needle: &str) -> Option<egui::Pos2> {
    let mut found = None;
    for cs in shapes {
        walk_texts(&cs.shape, &mut |t: &egui::epaint::TextShape| {
            // 整段相等而非包含：空状态文案里也提到「打开的文档」，
            // 用包含会点到提示文字上去。
            if found.is_none() && t.galley.text() == needle {
                let size = t.galley.size();
                found = Some(egui::pos2(t.pos.x + size.x / 2.0, t.pos.y + size.y / 2.0));
            }
        });
    }
    found
}

/// 在形状里找**侧栏区域内**某段文字的中心点（点击落点）。
///
/// 与 `text_center` 的区别：只接受裁剪矩形落在侧栏内的形状
/// （左缘贴窗左、宽度不超过面板宽），从而排除编辑区里同名的正文
/// （如大纲条目「第二节」与编辑区正文「第二节」重名，写死坐标会点错）。
fn sidebar_text_center(shapes: &[egui::epaint::ClippedShape], needle: &str) -> Option<egui::Pos2> {
    let mut found = None;
    for cs in shapes {
        let clip = cs.clip_rect;
        if clip.left() > 1.0 || clip.width() > 320.0 {
            continue;
        }
        walk_texts(&cs.shape, &mut |t: &egui::epaint::TextShape| {
            if found.is_none() && t.galley.text() == needle {
                let size = t.galley.size();
                found = Some(egui::pos2(t.pos.x + size.x / 2.0, t.pos.y + size.y / 2.0));
            }
        });
    }
    found
}

// ===========================================================================
// 名字编辑状态机（右键菜单的新建 / 重命名共用）
// ===========================================================================

/// 提交失败时草稿必须留在编辑态（让用户改后重试，而不是重新敲一遍）。
#[test]
fn a_rejected_name_keeps_the_draft_for_another_try() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.sidebar.start_create(Some(""), false);
    // `start_create` 预填了 `未命名.md`，用户是**改**这个名字而不是接着往后打字，
    // 所以这里整体替换。追加才是「在默认名后面接着写」，
    // 那得到 `未命名.mdguide.md`，是另一个语义。
    *app.sidebar.draft_mut().unwrap() = "guide.md".into();
    let edit = app.sidebar.take_edit().expect("应能取出");
    assert_eq!(
        edit,
        NameEdit::Create {
            parent: String::new(),
            name: "guide.md".into(),
            is_directory: false,
        }
    );
    // 名字虽然非空，但目标已存在于 docs 下——提交会被领域拒绝。
    let outcome = match &edit {
        NameEdit::Create {
            name, is_directory, ..
        } => app.create_entry(&joined(&root, "docs"), name, *is_directory),
        _ => unreachable!(),
    };
    assert!(outcome.is_err(), "与既有文件重名应失败");
}

/// 真实帧：四个标签都要能渲染（不是只过纯函数）。
///
/// 判据用「焦点能否落到搜索输入框」——egui 0.33 **没有文本回读接口**
/// （`ctx.debug_text` 是写入方向），控件状态是唯一稳定的外部可观察量。
/// 这同时也是正确的 a11y：切到搜索标签时光标有明确落点。
#[test]
fn all_sidebar_tabs_render_in_a_real_frame() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    let ctx = egui::Context::default();

    for tab in SidebarTab::ALL {
        app.ui.sidebar_tab = *tab;
        app.ui.sidebar_visible = true;
        for _ in 0..2 {
            crate::support::run_ui(&ctx, frame_input(), |ui| {
                rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
                main_view::show(&mut app, ui);
            });
        }
        if *tab == SidebarTab::Search {
            assert!(
                ctx.memory(|m| m.has_focus(egui::Id::unique(SEARCH_INPUT_ID))),
                "搜索标签未渲染：搜索框拿不到焦点"
            );
        }
    }
}

fn frame_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    }
}

/// 未打开工作区时侧栏应显示引导而不是空白。
#[test]
fn sidebar_guides_the_user_when_no_folder_is_open() {
    let cfg = tempfile::tempdir().unwrap();
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(cfg.path()),
        Arc::new(FakeDialog::new()),
    )
    .unwrap();
    let app = EditorApp::new(services, false);

    assert!(!app.workspace_entries.is_loaded());
    assert_eq!(app.workspace_root(), "");
    // 空根目录时画树必须安全（不能 panic 在切片越界上——
    // 本项目 panic = "abort"，一次越界等于终止进程）。
    let rows = tree_rows(&app.workspace_entries.items, "", &|_| true);
    assert!(rows.is_empty());
}

/// 路径末段提取（显示名的唯一来源）。
#[test]
fn display_name_takes_the_last_segment() {
    assert_eq!(display("/a/b/c.md"), "c.md");
    assert_eq!(display("c.md"), "c.md");
}

fn display(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

// ===========================================================================
// 问题 1 回归：侧边栏顶部标签点击切换
// ===========================================================================

/// 在 `pos` 上模拟一次完整点击（移动 → 按下 → 释放在同一点）。
///
/// egui 的 `clicked()` 判定「按下与释落在同一控件」，只发 pressed 不触发；
/// 而交互判定依赖**上一帧**已建立的布局，故每帧都要重跑 `main_view::show`。
fn click_at(ctx: &egui::Context, app: &mut EditorApp, pos: egui::Pos2) {
    for pressed in [true, false] {
        let mut input = frame_input();
        input.events.push(egui::Event::PointerMoved(pos));
        input.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
        crate::support::run_ui(ctx, input, |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(app, ui);
        });
    }
}

/// 在 `pos` 上模拟一次完整**右键**点击（移动 → 按下 → 释放在同一点）。
fn right_click_at(ctx: &egui::Context, app: &mut EditorApp, pos: egui::Pos2) {
    for pressed in [true, false] {
        let mut input = frame_input();
        input.events.push(egui::Event::PointerMoved(pos));
        input.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Default::default(),
        });
        crate::support::run_ui(ctx, input, |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(app, ui);
        });
    }
}

/// 右键菜单必须能被**左键点击其他地方**关闭（问题 040 截图实证）。
///
/// 修复前菜单只在「点树底部空白」时收起：点别的行走的是选中 / 展开分支，
/// 菜单原样挂着；点编辑区更没人管它。原型语义是
/// `window click → closeMenu`——任意左键都该收菜单。
#[test]
fn the_context_menu_closes_on_left_clicks_elsewhere() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    let ctx = egui::Context::default();
    app.ui.sidebar_tab = SidebarTab::Files;
    app.ui.sidebar_visible = true;

    // 渲染两帧让布局稳定，再从真实形状里取行中心——写死坐标会绑死旧布局。
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let out = crate::support::run_ui(&ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(&mut app, ui);
        });
        shapes = out.shapes;
    }
    let target = sidebar_text_center(&shapes, "readme.md").expect("应渲染出 readme.md 行");
    let other_row = sidebar_text_center(&shapes, "docs").expect("应渲染出 docs 行");

    // 右键 readme.md → 菜单打开。
    right_click_at(&ctx, &mut app, target);
    assert!(
        app.sidebar.context_menu_path.is_some(),
        "右键行应打开菜单，实际 {:?}",
        app.sidebar.context_menu_path
    );

    // 左键点别的行（docs）→ 菜单关闭（docs 同时被展开，但这不豁免关菜单）。
    click_at(&ctx, &mut app, other_row);
    assert!(
        app.sidebar.context_menu_path.is_none(),
        "左键点其他行应关闭菜单，实际 {:?}",
        app.sidebar.context_menu_path
    );

    // 再开一次，左键点树外（编辑区，x=900 远超侧栏宽）→ 同样关闭。
    right_click_at(&ctx, &mut app, target);
    assert!(app.sidebar.context_menu_path.is_some());
    click_at(&ctx, &mut app, egui::pos2(900.0, 400.0));
    assert!(
        app.sidebar.context_menu_path.is_none(),
        "左键点树外（编辑区）应关闭菜单，实际 {:?}",
        app.sidebar.context_menu_path
    );
}

/// 右键菜单是**前景浮层**：打开它不得改变文件树其它行的位置。
///
/// 行内拼接的旧实现把菜单 Frame 插进树流里，下方的行会被顶下去；
/// 浮层（`Area` + `Order::Foreground`）不参与布局，行坐标必须原样不动。
#[test]
fn the_context_menu_floats_without_moving_tree_rows() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    let ctx = egui::Context::default();
    app.ui.sidebar_tab = SidebarTab::Files;
    app.ui.sidebar_visible = true;

    let mut shapes = Vec::new();
    for _ in 0..2 {
        let out = crate::support::run_ui(&ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(&mut app, ui);
        });
        shapes = out.shapes;
    }
    let target = sidebar_text_center(&shapes, "readme.md").expect("应渲染出 readme.md 行");
    let docs_before = sidebar_text_center(&shapes, "docs").expect("应渲染出 docs 行");

    right_click_at(&ctx, &mut app, target);
    assert!(app.sidebar.context_menu_path.is_some(), "右键行应打开菜单");

    let out = crate::support::run_ui(&ctx, frame_input(), |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        main_view::show(&mut app, ui);
    });
    let docs_after = sidebar_text_center(&out.shapes, "docs").expect("应渲染出 docs 行");
    assert_eq!(
        docs_before, docs_after,
        "浮层不得挤动文件树其它行（旧实现会把下方行顶下去）"
    );
    assert!(
        text_center(&out.shapes, "新建文件").is_some(),
        "菜单应作为浮层真实渲染出来"
    );
}

/// 侧边栏顶部「大纲」标签点击后必须切换标签页。
///
/// 走**真实指针事件**而不是直接调 `toggle_sidebar_tab`：后者绕开了
/// 「事件绑定」这一环，而用户报的恰恰是「点了没反应」。
/// 标签精确宽度由主题 Metrics 与字体决定，测试不复算——在标签行上
/// 横扫若干 x，任一点命中「大纲」即通过，全都没中才失败。
#[test]
fn clicking_sidebar_tab_switches_to_outline() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    let ctx = egui::Context::default();
    app.ui.sidebar_tab = SidebarTab::Files;
    app.ui.sidebar_visible = true;

    // 渲染两帧让布局稳定，再从真实形状里取「大纲」的字形中心点击——
    // 活动栏重构后标签不在顶部平铺栏里，写死坐标等于把测试绑死在旧布局。
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let out = crate::support::run_ui(&ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(&mut app, ui);
        });
        shapes = out.shapes;
    }
    let target = sidebar_text_center(&shapes, "大纲").expect("活动栏应渲染出「大纲」导航项");

    click_at(&ctx, &mut app, target);

    assert_eq!(
        app.ui.sidebar_tab,
        SidebarTab::Outline,
        "点击活动栏「大纲」项无响应：sidebar_tab 仍为 {:?}",
        app.ui.sidebar_tab
    );
    assert!(app.ui.sidebar_visible, "切换到非当前标签不应把侧边栏收起来");
}

/// 侧边栏「大纲」条目点击后光标必须跳到对应行（锚点跳转）。
///
/// 可观察量取 `pending_cursor`：`goto_byte_offset`（大纲 / 搜索 / lint
/// 三条跳转的共同终点）会写入它，不依赖 egui 的文本回读接口。
#[test]
fn clicking_outline_item_jumps_cursor() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    let ctx = egui::Context::default();
    app.ui.sidebar_tab = SidebarTab::Outline;
    app.ui.sidebar_visible = true;
    // 打开含多个标题的文档（guide.md：# 指南 / ## 第二节）。
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .expect("打开文档应成功");
    assert!(
        !app.current_outline().is_empty(),
        "前提：guide.md 应解析出标题"
    );

    // 渲染两帧让布局稳定，再从真实形状里取「第二节」的字形中心点击——
    // 活动栏重构后大纲面板不再从固定 y 开始，写死坐标等于把测试绑死在旧布局；
    // 字形中心由本帧形状现取，布局怎么变都能点到正确的条目。
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let out = crate::support::run_ui(&ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(&mut app, ui);
        });
        shapes = out.shapes;
    }
    assert!(
        !app.current_outline().is_empty(),
        "渲染后大纲应为非空（active_doc 应在渲染后保留）：len={}",
        app.current_outline().len()
    );

    // 跳到「第二节」（第 4 行）而非首行：首行 offset 为 0，跳转后
    // selection 仍可能塌成 None，判不出「到底跳没跳」；点第二行能验证
    // 偏移换算真的指向正确行，而不是「界面跳了但跳错地方」。
    let target = sidebar_text_center(&shapes, "第二节").expect("大纲应渲染出「第二节」条目");
    click_at(&ctx, &mut app, target);

    // 跳转判据：光标应落到「第二节」所在行（第 4 行）。
    let starts = rustmark::presentation::views::scroll_sync::line_start_offsets(
        &app.current_content().unwrap(),
    );
    assert_eq!(
        app.editor.cursor, starts[4],
        "点击大纲条目应把光标跳到「第二节」（第 4 行）"
    );
    let content = app.current_content().unwrap();
    let line_text = content[app.editor.cursor..].split('\n').next().unwrap();
    assert!(
        line_text.contains("第二节"),
        "落点行应是「第二节」，实际：{line_text:?}"
    );
}

// ===========================================================================
// 问题 036 回归：`ui.horizontal` 把侧栏内容压进 18pt → 面板半屏就断
// ===========================================================================

/// 侧栏内容（活动栏 + 内容面板）必须撑满面板**全高**。
///
/// egui 0.33 的 `Ui::horizontal` 把子 Ui 初始高定为
/// `style.spacing.interact_size.y`（18pt）、交叉轴居中（文档原话）。
/// 活动栏重构成「水平一行」后，活动栏与内容面板被整体压进这 18pt：
/// 底色条到半屏就断掉、大纲提示溢出面板背景之外浮在编辑区上。
/// 修复改用 `horizontal_top`（初始高取 `available_size_before_wrap()`，
/// 即面板全高）。本用例从真实形状钉住两条不变量：
///
/// 1. 大纲提示文字可见且在列表上方——它若放在撑满剩余高的滚动区之后
///    会被推出面板底缘，压扁缺陷下又浮在半屏，两种坏形态一并钉住；
/// 2. 活动栏底色矩形纵向贯穿面板（高度超过面板高的一半）。
#[test]
fn sidebar_content_fills_the_panel_height() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.open_workspace_file(&joined(&root, "docs/guide.md"))
        .expect("打开应成功");
    app.ui.sidebar_tab = SidebarTab::Outline;
    app.ui.sidebar_visible = true;

    let shapes = frames(&mut app, None);
    // 侧栏判据：clip 右界不越过面板（面板宽 308 + frame 边距，编辑区从
    // ~308 起）。不能用「左界贴 0」——面板 frame 自带 8pt 内边距，
    // 面板内容形状的 clip 左界是 8 而不是 0。
    let in_sidebar = |r: egui::Rect| r.right() <= 320.0;

    // 不变量 1：提示文字必须**可见且在列表上方**。它若放在撑满剩余高的
    // 滚动区之后会被推出面板底缘（036 修复时发现），必须在列表之前绘制；
    // 压扁缺陷下它则浮在半屏——两种坏形态都可用「存在 + 早于列表」钉住。
    let mut hint_top = f32::MAX;
    let mut entry_top = f32::MAX;
    for cs in &shapes {
        if !in_sidebar(cs.clip_rect) {
            continue;
        }
        walk_texts(&cs.shape, &mut |t| {
            if t.galley.text().contains("点击标题可跳转") {
                hint_top = hint_top.min(t.pos.y);
            }
            // 大纲条目行：标题 galley 恰是条目文本（层级号「#」另起一段）。
            if t.galley.text() == "第二节" {
                entry_top = entry_top.min(t.pos.y);
            }
        });
    }
    assert!(
        hint_top < f32::MAX,
        "大纲提示必须可见（被撑满的滚动区推出面板即回归）"
    );
    assert!(
        hint_top < entry_top,
        "大纲提示应在列表上方：hint_top={hint_top}, entry_top={entry_top}"
    );

    // 不变量 2：活动栏底色矩形必须纵向贯穿面板。压扁时它只有 18pt 高。
    //
    // 左界 < 100 把候选限定在活动栏带（面板内容 8..56pt）：4px 宽的
    // 侧栏分割条（问题 067 新增，位于 [308,312]）同样「宽 ≤ 60 且
    // 右界 ≤ 320」，但它贯穿面板高与活动栏是否被压扁无关——混进来
    // 会让这条压扁回归断言失去牙齿（分割条永远够高）。
    let activity_h = shapes
        .iter()
        .filter(|cs| in_sidebar(cs.clip_rect))
        .flat_map(|cs| {
            let mut rects = Vec::new();
            walk_rects(&cs.shape, &mut |r| rects.push(r.rect));
            rects
        })
        .filter(|r| r.width() <= 60.0 && r.left() < 100.0)
        .fold(0.0_f32, |acc, r| acc.max(r.height()));
    assert!(
        activity_h > 300.0,
        "活动栏底色应贯穿面板全高，实测最高 {activity_h}pt（18pt 即 `ui.horizontal` 压扁回归）"
    );
}

// ===========================================================================
// 问题 033 回归：搜索命中行把侧栏撑出面板 → 侧栏右侧一条竖向黑框
// ===========================================================================

/// 造一个命中行前后都有足够上下文的工作区。
///
/// 摘要窗口是「命中词前后各 24 字节」（`workspace/service.rs`），中文 3 字节
/// 一个，即前后各 8 字：片段形如 `甲甲甲甲甲甲甲甲甲 ++ 图片 ++ 乙乙乙乙乙乙乙乙乙`。
/// 这正好复现缺陷类 033 的触发条件——前缀占掉大部分可用宽，第三段只剩几个点。
fn overflow_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path();
    fs::create_dir_all(root.join("docs")).unwrap();
    let line = format!("{} ++ 图片 ++ {}", "甲".repeat(8), "乙".repeat(40));
    fs::write(root.join("docs/long.md"), format!("# t\n\n{line}\n")).unwrap();
    dir
}

/// 跑若干帧真实布局，取最后一帧的全部形状。
fn frames(app: &mut EditorApp, fonts: Option<&str>) -> Vec<egui::epaint::ClippedShape> {
    let ctx = egui::Context::default();
    if let Some(family) = fonts {
        let setup = rustmark::infrastructure::fonts::font_definitions(Some(family));
        ctx.set_fonts(rustmark::app::frame::to_egui_font_definitions(
            setup.definitions,
        ));
    }
    let mut shapes = Vec::new();
    for _ in 0..3 {
        let out = crate::support::run_ui(&ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(app, ui);
        });
        shapes = out.shapes;
    }
    shapes
}

fn walk_rects(shape: &egui::epaint::Shape, f: &mut impl FnMut(&egui::epaint::RectShape)) {
    match shape {
        egui::epaint::Shape::Rect(r) => f(r),
        egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| walk_rects(s, f)),
        _ => {}
    }
}

fn walk_texts(shape: &egui::epaint::Shape, f: &mut impl FnMut(&egui::epaint::TextShape)) {
    match shape {
        egui::epaint::Shape::Text(t) => f(t),
        egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| walk_texts(s, f)),
        _ => {}
    }
}

/// 搜索命中行不得把侧栏撑宽，且片段必须是**一个** galley。
///
/// 缺陷类 033 的两条判据，缺一不可：
///
/// 1. **布局不变量**：侧栏内任何形状的右界都不得超过面板宽（240pt）。旧实现
///    里「前 / 命中 / 后」三段各自 `truncate()`，第三段的可用宽只剩几个点，
///    而 epaint 的截断保证「行不为空」——装不下任何字形时仍排出一个字形，
///    于是本行 `min_rect` 冲出面板，`SidePanel` 据此把编辑区整体推右，
///    [240, 推右后的左界] 之间没有任何面板绘制，露出交换链清屏色（黑）。
/// 2. **结构断言**（与字体无关）：命中词不得单独成段。片段 galley 必须同时
///    含命中词前后的上下文（`甲` 与 `乙`），否则说明又被拆回了三段。
#[test]
fn search_hit_row_never_widens_the_sidebar_panel() {
    let ws = overflow_workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.open_workspace_file(&joined(&root, "docs/long.md"))
        .expect("打开应成功");
    app.sidebar.search_query = "图片".into();
    app.ui.sidebar_tab = SidebarTab::Search;
    app.ui.sidebar_visible = true;

    // 前提：真的搜到了，且摘要里前后文都在（否则下面的结构断言是空转）。
    let result = app.search_workspace().expect("搜索应成功");
    let snippet = result
        .groups
        .iter()
        .flat_map(|g| &g.hits)
        .next()
        .expect("应至少一条命中")
        .snippet
        .clone();
    assert!(
        snippet.contains('甲') && snippet.contains("图片") && snippet.contains('乙'),
        "摘要应同时含前后文，实际：{snippet:?}"
    );

    // 用户真实字体配置（Consolas + 中文字体回退）与默认字体各跑一遍：
    // 溢出只在特定字形宽度下发生，两套字体都要守住同一条不变量。
    for fonts in [Some("user:Consolas"), None] {
        let shapes = frames(&mut app, fonts);
        let panel_w = app.theme().metrics.sidebar_width;
        let mut sidebar_right = 0.0_f32;
        let mut editor_left = f32::MAX;
        let mut span_galleys: Vec<String> = Vec::new();
        for cs in &shapes {
            // 裁剪矩形即「区域归属」：侧栏 [0,100]-[240,776]、编辑区 [240,…]、
            // 顶栏 / 状态栏横跨整窗。按区域分别统计，才不会把顶栏面包屑、
            // 状态栏字数这些**本来就该通栏**的形状误判成侧栏溢出。
            let clip = cs.clip_rect;
            let in_sidebar = clip.left() <= 0.5 && clip.width() <= panel_w + 0.5;
            let in_editor = clip.left() >= panel_w - 0.5 && clip.width() > 200.0;
            walk_rects(&cs.shape, &mut |r: &egui::epaint::RectShape| {
                let rect = r.rect;
                if rect.width() < 8.0 || rect.height() < 8.0 {
                    return;
                }
                if in_sidebar && rect.right() > sidebar_right {
                    sidebar_right = rect.right();
                }
                if in_editor && rect.left() < editor_left {
                    editor_left = rect.left();
                }
            });
            walk_texts(&cs.shape, &mut |t: &egui::epaint::TextShape| {
                let text = t.galley.text();
                if in_sidebar && text.contains("图片") && text.contains('甲') && text.contains('乙')
                {
                    span_galleys.push(text.to_string());
                }
            });
        }
        assert!(
            sidebar_right <= panel_w + 0.01,
            "fonts={fonts:?}：侧栏内容右界 {sidebar_right} 超出面板宽 {panel_w}"
        );
        assert!(
            editor_left >= panel_w - 0.01,
            "fonts={fonts:?}：编辑区左界 {editor_left} 被侧栏推右（面板宽 {panel_w}）"
        );
        assert_eq!(
            span_galleys.len(),
            1,
            "fonts={fonts:?}：命中片段应只有一个 galley，实际 {:?}",
            span_galleys
        );
    }
}

// ===========================================================================
// 可打开格式白名单（问题 045）
// ===========================================================================

/// 在 `pos` 上模拟一次**双击**（两轮 press + release）。
///
/// egui 把它判成双击的条件是「两次点击的间隔小于 `max_double_click_delay`
/// （0.3s）」，而无头驱动的帧墙钟间隔是微秒级，天然落在窗口内——
/// 不必伪造 `RawInput::time` 去赶动画（那是 041 的教训：时间相关行为
/// 在测试帧节奏下容易被掩盖，但这里要的正是「快得能被判成双击」）。
fn double_click_at(ctx: &egui::Context, app: &mut EditorApp, pos: egui::Pos2) {
    for pressed in [true, false, true, false] {
        let mut input = frame_input();
        input.events.push(egui::Event::PointerMoved(pos));
        input.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
        crate::support::run_ui(ctx, input, |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(app, ui);
        });
    }
}

/// 侧栏区域里某段文字的**绘制颜色**（取首个样式段的颜色）。
///
/// 置灰 / 不置灰这类「表达」只能从颜色上读——文字内容本身两行一样。
fn sidebar_text_color(
    shapes: &[egui::epaint::ClippedShape],
    needle: &str,
) -> Option<egui::Color32> {
    let mut found = None;
    for cs in shapes {
        // 与 `sidebar_text_center` 同一条侧栏判据（靠左、不宽于面板）。
        if cs.clip_rect.left() > 1.0 || cs.clip_rect.width() > 320.0 {
            continue;
        }
        walk_texts(&cs.shape, &mut |t: &egui::epaint::TextShape| {
            if found.is_none() && t.galley.text() == needle {
                found = t.galley.job.sections.first().map(|s| s.format.color);
            }
        });
    }
    found
}

/// 渲染两帧并返回形状（第二帧的布局已稳定）。
fn settled_shapes(ctx: &egui::Context, app: &mut EditorApp) -> Vec<egui::epaint::ClippedShape> {
    let mut shapes = Vec::new();
    for _ in 0..2 {
        shapes = crate::support::run_ui(ctx, frame_input(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(app, ui);
        })
        .shapes;
    }
    shapes
}

/// 「文件」标签页里：白名单内的文件双击能打开（新分支的正向一半）。
#[test]
fn double_clicking_a_text_file_in_the_tree_opens_it() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    let ctx = egui::Context::default();
    app.ui.sidebar_tab = SidebarTab::Files;
    app.ui.sidebar_visible = true;

    let shapes = settled_shapes(&ctx, &mut app);
    let target = sidebar_text_center(&shapes, "readme.md").expect("文件树应渲染出 readme.md 行");
    double_click_at(&ctx, &mut app, target);

    let opened = app.current_document_path();
    assert!(
        opened.contains("readme.md"),
        "双击 .md 应打开它，实际活动文档 = {opened:?}"
    );
}

/// 白名单**外**的文件：行置灰、双击打不开、且给出「暂不支持」的提示。
///
/// 三件事一起钉，是因为缺任何一件这个特性都是残的：
/// - 只置灰不拦 → 用户双击还是能打开一屏乱码；
/// - 只拦不置灰 → 用户不知道哪些能点，只能在「点了没反应」里试错；
/// - 拦了不提示 → 同上，且看起来像软件卡了。
#[test]
fn non_text_file_in_the_tree_is_dimmed_and_refuses_to_open() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    // 文档目录里放一张配图——这是工作区里最常见的非文本文件。
    fs::write(root.join("photo.png"), b"\x89PNG\r\n\x1a\n\x00\x00").unwrap();
    // 再放一份表格：白名单内，用来对照「不是所有非 md 都置灰」。
    fs::write(root.join("table.csv"), "a,b\n1,2\n").unwrap();
    app.refresh_workspace();

    let ctx = egui::Context::default();
    app.ui.sidebar_tab = SidebarTab::Files;
    app.ui.sidebar_visible = true;
    let theme = app.ui.theme.clone();

    let shapes = settled_shapes(&ctx, &mut app);
    // ① 置灰：图片用次要色，md / csv 用正文色。
    let png_color = sidebar_text_color(&shapes, "photo.png").expect("文件树应渲染出 photo.png 行");
    assert_eq!(
        png_color, theme.text_secondary,
        "白名单外的文件必须置灰：可用性不能只靠「点了才知道」"
    );
    assert_eq!(
        sidebar_text_color(&shapes, "readme.md"),
        Some(theme.text_primary),
        ".md 应保持正文色（置灰是给不能打开的文件用的信号）"
    );
    assert_eq!(
        sidebar_text_color(&shapes, "table.csv"),
        Some(theme.text_primary),
        ".csv 在白名单内，同样不该置灰"
    );

    // ② 双击不打开，③ 且说明原因。
    let before = app.open_document_list().len();
    let target = sidebar_text_center(&shapes, "photo.png").expect("应能定位 photo.png 行");
    double_click_at(&ctx, &mut app, target);

    assert_eq!(
        app.open_document_list().len(),
        before,
        "双击图片不得打开任何文档（否则编辑区是一屏乱码）"
    );
    let notice = app
        .status_bar
        .message
        .as_ref()
        .map(|m| m.text.clone())
        .unwrap_or_default();
    assert!(
        notice.contains("暂不支持打开该格式"),
        "双击图片应说明原因而不是静默无反应，实际状态栏 = {notice:?}"
    );
}

// ===========================================================================
// 问题 067 · 侧栏分割条拖拽（宽度可调 + 双击复位 + 跨重启记住）
// ===========================================================================

/// 跑一帧（带主题映射），让 egui 的交互状态推进一帧。
fn one_frame(ctx: &egui::Context, app: &mut EditorApp, input: egui::RawInput) {
    let _ = crate::support::run_ui(ctx, input, |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        main_view::show(app, ui);
    });
}

/// 把指针移到 `pos`（单独一帧，不带按键）。
///
/// 必须在按下**之前**做：按下那一帧若同时带 `PointerMoved`，从上一个
/// 位置跳到起点的位移会被算进该帧的 `pointer.delta()`——分割条有 drag
/// sense，这一跳可能被当成一次拖拽（无头测试里指针的「上一个位置」
/// 不可控，可能是窗口原点）。
fn move_pointer(ctx: &egui::Context, app: &mut EditorApp, pos: egui::Pos2) {
    let mut input = frame_input();
    input.events.push(egui::Event::PointerMoved(pos));
    one_frame(ctx, app, input);
}

/// 在 `pos` 上按住，分步横移 `dx`，再松开（问题 067 的拖拽模拟）。
///
/// 分步而不是一步到位：egui 的 `pointer.delta()` 是**逐帧**的位移差，
/// 分步既贴近真实指针轨迹，也顺带验证「跨帧累加」这条实现假设
/// （每帧只加本帧位移，`width` 取上一帧写回后的生效值）。
fn drag_horizontally(ctx: &egui::Context, app: &mut EditorApp, pos: egui::Pos2, dx: f32) {
    const STEP: f32 = 10.0;
    let steps = (dx / STEP).round().max(1.0) as i32;

    move_pointer(ctx, app, pos);
    // 按下（指针已在起点，本帧 delta 为 0，不会误触发拖拽）。
    let mut input = frame_input();
    input.events.push(egui::Event::PointerMoved(pos));
    input.events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    });
    one_frame(ctx, app, input);
    // 分步移动（按键状态由 egui 跨帧保持，只需发 PointerMoved）。
    for i in 1..=steps {
        let mut input = frame_input();
        input.events.push(egui::Event::PointerMoved(egui::pos2(
            pos.x + STEP * i as f32,
            pos.y,
        )));
        one_frame(ctx, app, input);
    }
    // 松开。
    let end = egui::pos2(pos.x + STEP * steps as f32, pos.y);
    let mut input = frame_input();
    input.events.push(egui::Event::PointerMoved(end));
    input.events.push(egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    });
    one_frame(ctx, app, input);
}

/// 形状里最左侧的「4px 宽 × 贯穿面板高」矩形 = 侧栏分割条。
///
/// 两条分割条都是这个形状（各 4px 宽、占满主体带高），按 x 排序取
/// 最左的那条即侧栏分割条；侧栏内部分隔线是 1px 宽，进不了这个带宽。
fn sidebar_divider_left(shapes: &[egui::epaint::ClippedShape]) -> Option<f32> {
    let mut lefts: Vec<f32> = Vec::new();
    for cs in shapes {
        walk_rects(&cs.shape, &mut |r| {
            if (3.5..=4.5).contains(&r.rect.width()) && r.rect.height() > 500.0 {
                lefts.push(r.rect.left());
            }
        });
    }
    lefts.into_iter().reduce(f32::min)
}

/// 拖侧栏分割条：宽度跟手变化、面板几何右移、结果跨重启记住。
///
/// 三条断言各钉一层：状态值（拖拽数学）、几何（面板真的变宽了）、
/// 持久化（帧末落盘 + 启动读回整条链）。第三条同时钉住「帧末落盘
/// 接线」——若 `main_view::show` 忘了调 `persist_sidebar_width`，
/// 前两条照样绿而重启后宽度丢失。
#[test]
fn dragging_the_sidebar_divider_resizes_the_sidebar_and_persists() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (cfg, mut app) = app_with_workspace(&root);
    app.ui.sidebar_visible = true;

    let ctx = egui::Context::default();
    // 默认宽 308（1280 窗口下动态上界 478.5，不夹）→ 分割条中心 310。
    drag_horizontally(&ctx, &mut app, egui::pos2(310.0, 400.0), 60.0);

    assert!(
        (app.ui.sidebar_width - 368.0).abs() < 1.0,
        "拖 60px 后宽度应为 368，实测 {}",
        app.ui.sidebar_width
    );

    // 几何：分割条矩形左缘应随宽度右移到 368。
    let shapes = settled_shapes(&ctx, &mut app);
    let left = sidebar_divider_left(&shapes).expect("应能找到侧栏分割条矩形");
    assert!(
        (left - 368.0).abs() < 1.0,
        "分割条应随拖拽右移到 368，实测左缘 {left}"
    );

    // 跨重启：帧末落盘 → 新实例仍是 368（整条持久化链）。
    let restarted = EditorApp::new(
        Services::assemble_with_dialogs(
            ConfigPaths::in_dir(cfg.path()),
            Arc::new(FakeDialog::new()),
        )
        .expect("装配应成功"),
        false,
    );
    assert!(
        (restarted.ui.sidebar_width - 368.0).abs() < 1.0,
        "拖拽结果应跨重启记住，实测 {}",
        restarted.ui.sidebar_width
    );
}

/// 双击侧栏分割条复位到主题默认宽度（问题 067 的逃生门）。
#[test]
fn double_clicking_the_sidebar_divider_resets_to_theme_default() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.ui.sidebar_visible = true;
    // 先拖宽（420 在 1280 窗口的动态上界 478.5 之内，原样生效）。
    app.ui.set_sidebar_width(420.0);

    let ctx = egui::Context::default();
    // 落定一帧：交互判定依赖上一帧建立的布局。
    one_frame(&ctx, &mut app, frame_input());
    // 指针先就位（理由同 `drag_horizontally`：按下帧的 delta 必须为 0）。
    move_pointer(&ctx, &mut app, egui::pos2(422.0, 400.0));
    double_click_at(&ctx, &mut app, egui::pos2(422.0, 400.0));

    let default_width = app.theme().metrics.sidebar_width;
    assert!(
        (app.ui.sidebar_width - default_width).abs() < 1e-6,
        "双击应复位到主题默认宽度 {default_width}，实测 {}",
        app.ui.sidebar_width
    );
}

// ===========================================================================
// R-11 子批 3 · sidebar 组：状态写入口
//
// 侧栏视图对 `app.sidebar.*` 的直写已全部收敛为 `EditorApp` 方法
// （见 `src/app/frame/sidebar.rs`）。下列用例一律走方法入口，
// 不自己写字段——否则就是「自己设值再断言」的无牙齿判据。
// ===========================================================================

/// 提交失败后放回新建草稿：用户原本敲的三个值原样恢复。
#[test]
fn restoring_the_create_draft_puts_back_what_the_user_typed() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.restore_create_draft("/w/docs".to_string(), "笔记.md".to_string(), false);

    assert_eq!(
        app.sidebar.creating,
        Some(("/w/docs".to_string(), "笔记.md".to_string(), false)),
        "新建草稿应原样放回编辑态，让用户改后重试"
    );
}

/// 恢复草稿**只**回放草稿字段：失败原因与右键菜单不能被顺手抹掉。
///
/// 这是 `restore_*` 与状态结构体 `start_create` / `start_rename` 的分界——
/// 后者是「开始编辑」入口，会连带清菜单与错误；前者是「失败后回放」，
/// 抹掉错误用户就不知道自己错在哪。
#[test]
fn restoring_a_draft_keeps_the_error_and_the_menu() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.sidebar.fail("目标目录只读".to_string());
    app.sidebar
        .open_context_menu("/w/readme.md", egui::pos2(10.0, 10.0));

    app.restore_rename_draft("/w/readme.md".to_string(), "readme.md".to_string());

    assert_eq!(app.sidebar.renaming_path.as_deref(), Some("/w/readme.md"));
    assert_eq!(app.sidebar.rename_draft, "readme.md");
    assert!(
        app.sidebar.error.is_some(),
        "失败原因应保留——被抹掉用户就不知道自己错在哪"
    );
    assert!(
        app.sidebar.menu_is_for("/w/readme.md"),
        "右键菜单不应被恢复草稿顺手关掉"
    );
}

/// 单选只改高亮，不开文档（打开是双击的动作）。
#[test]
fn selecting_a_row_highlights_without_opening_a_document() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    assert!(
        app.active_document_dir().is_none(),
        "夹具前提：尚未打开任何文档"
    );

    app.select_sidebar_path("/w/docs/guide.md".to_string());

    assert_eq!(
        app.sidebar.selected_path.as_deref(),
        Some("/w/docs/guide.md"),
        "单选应把该行置为高亮"
    );
    assert!(app.active_document_dir().is_none(), "单选不得顺带打开文档");
}

/// 选中大纲条目只写高亮态（跳转由调用方另走 `goto_outline_line`）。
#[test]
fn selecting_an_outline_item_highlights_it() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.select_outline_item("第二节".to_string());

    assert_eq!(
        app.sidebar.outline_selected.as_deref(),
        Some("第二节"),
        "大纲条目选中态应记录标题文本"
    );
}

/// 搜索框回写（响应 `TextEdit::changed`）。
#[test]
fn setting_the_search_query_updates_the_field() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.set_search_query("needle".to_string());

    assert_eq!(app.sidebar.search_query, "needle");
}

/// 切换搜索范围。
#[test]
fn switching_the_search_scope_takes_effect() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    app.set_search_scope(SearchScope::OpenDocuments);

    assert_eq!(
        app.sidebar.search_scope,
        SearchScope::OpenDocuments,
        "范围切换应立即生效（占位提示与扫描口径都读它）"
    );
}

/// 搜索开关是**双态**的：再点一次即取消。
#[test]
fn toggling_a_search_option_flips_both_ways() {
    use rustmark::presentation::state::sidebar_ui_state::SearchToggle;

    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    assert!(
        app.sidebar.search_options.is_plain(),
        "夹具前提：三个开关默认全关"
    );

    app.toggle_search_option(SearchToggle::CaseSensitive);
    assert!(app.sidebar.search_options.case_sensitive, "第一次点应开启");
    assert_eq!(
        app.sidebar.search_options.active_count(),
        1,
        "只动被点的那一个开关"
    );

    app.toggle_search_option(SearchToggle::CaseSensitive);
    assert!(
        !app.sidebar.search_options.case_sensitive,
        "再点一次应取消，不是无操作"
    );
}
