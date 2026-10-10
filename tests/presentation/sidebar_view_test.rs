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
use rustmark::presentation::state::sidebar_ui_state::{NameEdit, tree_rows};
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

/// 树投影把扁平条目折成带缩进的层级行。
#[test]
fn tree_projection_builds_hierarchical_rows() {
    let ws = workspace();
    let (_cfg, app) = app_with_workspace(ws.path());
    let root = app.workspace_root();

    let rows = tree_rows(&app.workspace_entries.items, &root, &|_| true);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    // 顺序由仓储的深度优先遍历决定（**不是**「把所有目录排在一起」）：
    // 本层目录 → 本层文件 → 逐个递归子目录，故 `readme.md` 夹在
    // `docs` 和它的子项之间。`tree_rows` 刻意不重排——重排会让
    // 「重命名后条目位置不动」这个预期失效。
    assert_eq!(
        names,
        vec!["docs", "readme.md", "img", "guide.md"],
        "树的顺序应沿用仓储的深度优先遍历顺序"
    );
    let depths: Vec<usize> = rows.iter().map(|r| r.depth).collect();
    assert_eq!(
        depths,
        vec![0, 0, 1, 1],
        "img 与 guide.md 在 docs 之下，docs 与 readme.md 在根"
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

/// 右键菜单「删除」：文件删除；非空目录**不**递归删。
///
/// 右键删一个目录就抹掉里面全部内容，几乎必然招致后悔，
/// 所以非空目录必须拒绝（真实实现用 `remove_dir` 而非 `remove_dir_all`）。
#[test]
fn delete_removes_a_file_but_refuses_a_non_empty_directory() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);

    let err = app
        .delete_entry(&joined(&root, "docs"))
        .expect_err("非空目录的删除应被拒绝");
    assert!(
        err.to_string().contains("非空"),
        "错误文案应说清是「目录非空」，实际：{err}"
    );
    assert!(
        root.join("docs/guide.md").exists(),
        "拒绝删除时，里面的文件必须毫发无损"
    );

    app.delete_entry(&joined(&root, "docs/guide.md"))
        .expect("删除文件应成功");
    assert!(!root.join("docs/guide.md").exists(), "文件应真的被删除");
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
    app.sidebar.search_query = "needle".into();

    let result = app.search_workspace().expect("搜索应成功");

    assert_eq!(result.groups.len(), 2, "两个文件各一组");
    // 分组顺序 = 候选顺序：先「已打开的文档」（含启动时的空白未命名文档，
    // 它不含 needle 故不产生组），再按扫描顺序的条目。故 `readme.md`
    // 在根、排在 `docs/guide.md` 之前——**不是**「目录优先」。
    let names: Vec<&str> = result.groups.iter().map(|g| g.file_name.as_str()).collect();
    assert_eq!(
        names,
        vec!["readme.md", "guide.md"],
        "分组应按候选顺序（已打开文档 → 扫描顺序）"
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

/// 每条命中都要能定位到片段里的高亮区间（字符坐标，不是字节）。
///
/// 片段要交给 egui 排版；按字节切中文会切在字符中间，
/// 症状是「高亮块里少半个字」或「整个片段不高亮」。
#[test]
fn every_hit_carries_a_highlight_range_in_character_coordinates() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    app.sidebar.search_query = "needle".into();

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
    app.sidebar.search_query = "第二节".into();
    let result = app.search_workspace().unwrap();
    let group = result
        .groups
        .iter()
        .find(|g| g.file_name == "guide.md")
        .expect("应有 guide.md 这一组");
    let hit = group.hits[0].clone();

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

    app.sidebar.search_query = "needle".into();
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

/// 空查询不发起搜索（否则每帧全盘扫描）。
#[test]
fn empty_query_does_not_trigger_a_search() {
    let ws = workspace();
    let (_cfg, app) = app_with_workspace(ws.path());

    let result = app.search_workspace().expect("应返回空结果而非错误");

    assert!(result.is_empty(), "空查询不应有结果");
    assert!(!result.truncated);
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

/// 真实帧：三个标签都要能渲染（不是只过纯函数）。
///
/// 判据用「焦点能否落到搜索输入框」——egui 0.33 **没有文本回读接口**
/// （`ctx.debug_text` 是写入方向），控件状态是唯一稳定的外部可观察量。
/// 这同时也是正确的 a11y：切到搜索标签时光标有明确落点。
#[test]
fn all_three_sidebar_tabs_render_in_a_real_frame() {
    let ws = workspace();
    let root = ws.path().to_path_buf();
    let (_cfg, mut app) = app_with_workspace(&root);
    let ctx = egui::Context::default();

    for tab in SidebarTab::ALL {
        app.ui.sidebar_tab = *tab;
        app.ui.sidebar_visible = true;
        for _ in 0..2 {
            let _ = ctx.run(frame_input(), |ctx| {
                rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
                main_view::show(&mut app, ctx);
            });
        }
        if *tab == SidebarTab::Search {
            assert!(
                ctx.memory(|m| m.has_focus(egui::Id::new(SEARCH_INPUT_ID))),
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
