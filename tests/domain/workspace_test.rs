//! Workspace 上下文单测（S3-11）。
//!
//! 领域层**不做 IO**：条目由仓储扫描后装入，这里只验证结构与规则。

use rustmark::domain::workspace::aggregate::Workspace;
use rustmark::domain::workspace::errors::WorkspaceError;
use rustmark::domain::workspace::events::WorkspaceEvent;
use rustmark::domain::workspace::value_objects::{CloseDecision, EntryKind, WorkspaceEntry};
use rustmark::ports::time::Timestamp;
use rustmark::shared::ids::{DocumentId, WorkspaceId};
use rustmark::shared::path::FilePath;

fn path(raw: &str) -> FilePath {
    FilePath::new(raw).expect("测试路径应合法")
}

fn workspace() -> Workspace {
    Workspace::with_root(WorkspaceId::new(), path("/ws"))
}

// ---------- 条目规则 ----------

#[test]
fn add_entry_then_duplicate_is_rejected() {
    let mut workspace = workspace();
    workspace
        .add_entry(WorkspaceEntry::file(path("/ws/a.md")))
        .unwrap();
    let result = workspace.add_entry(WorkspaceEntry::file(path("/ws/a.md")));
    assert!(matches!(result, Err(WorkspaceError::DuplicateName { .. })));
    assert_eq!(workspace.entries().len(), 1, "被拒绝的操作不得改变状态");
}

#[test]
fn add_entry_outside_root_is_rejected() {
    let mut workspace = workspace();
    let result = workspace.add_entry(WorkspaceEntry::file(path("/other/a.md")));
    assert!(matches!(result, Err(WorkspaceError::OutsideRoot { .. })));
}

#[test]
fn remove_entry_also_removes_descendants() {
    let mut workspace = workspace();
    workspace
        .add_entry(WorkspaceEntry::directory(path("/ws/docs")))
        .unwrap();
    workspace
        .add_entry(WorkspaceEntry::file(path("/ws/docs/a.md")))
        .unwrap();
    workspace.remove_entry(&path("/ws/docs")).unwrap();

    assert!(
        workspace
            .entries()
            .iter()
            .all(|e| e.path() != &path("/ws/docs/a.md")),
        "移除目录必须连带移除其子条目"
    );
}

#[test]
fn remove_missing_entry_is_rejected() {
    let mut workspace = workspace();
    let result = workspace.remove_entry(&path("/ws/nope.md"));
    assert!(matches!(result, Err(WorkspaceError::NotFound { .. })));
}

#[test]
fn move_entry_into_own_descendant_is_rejected() {
    let mut workspace = workspace();
    workspace
        .add_entry(WorkspaceEntry::directory(path("/ws/docs")))
        .unwrap();
    workspace
        .add_entry(WorkspaceEntry::directory(path("/ws/docs/sub")))
        .unwrap();

    let result = workspace.move_entry(&path("/ws/docs"), &path("/ws/docs/sub/nested"));
    assert!(matches!(
        result,
        Err(WorkspaceError::MoveIntoDescendant { .. })
    ));
}

#[test]
fn move_entry_renames_descendants() {
    let mut workspace = workspace();
    workspace
        .add_entry(WorkspaceEntry::directory(path("/ws/docs")))
        .unwrap();
    workspace
        .add_entry(WorkspaceEntry::file(path("/ws/docs/a.md")))
        .unwrap();

    workspace
        .move_entry(&path("/ws/docs"), &path("/ws/notes"))
        .unwrap();

    let paths: Vec<&str> = workspace
        .entries()
        .iter()
        .map(|e| e.path().as_str())
        .collect();
    assert!(paths.contains(&"/ws/notes"));
    assert!(
        paths.contains(&"/ws/notes/a.md"),
        "子条目路径必须一并更新：{paths:?}"
    );
}

#[test]
fn move_entry_to_existing_path_is_rejected() {
    let mut workspace = workspace();
    workspace
        .add_entry(WorkspaceEntry::file(path("/ws/a.md")))
        .unwrap();
    workspace
        .add_entry(WorkspaceEntry::file(path("/ws/b.md")))
        .unwrap();
    let result = workspace.move_entry(&path("/ws/a.md"), &path("/ws/b.md"));
    assert!(matches!(result, Err(WorkspaceError::DuplicateName { .. })));
}

#[test]
fn is_directory_reflects_entry_kind() {
    let mut workspace = workspace();
    workspace
        .add_entry(WorkspaceEntry::new(path("/ws/docs"), EntryKind::Directory))
        .unwrap();
    workspace
        .add_entry(WorkspaceEntry::new(path("/ws/a.md"), EntryKind::File))
        .unwrap();
    assert!(workspace.is_directory(&path("/ws/docs")));
    assert!(!workspace.is_directory(&path("/ws/a.md")));
}

// ---------- 标签页 ----------

#[test]
fn close_tab_on_dirty_document_needs_save() {
    let mut workspace = workspace();
    let id = DocumentId::new();
    workspace.open_tab(id);

    let decision = workspace.close_tab(id, true).unwrap();
    assert_eq!(decision, CloseDecision::NeedsSave);
    assert!(!decision.can_close());
    assert_eq!(workspace.tabs().len(), 1, "需要保存时标签页不得被移除");
}

#[test]
fn close_tab_on_clean_document_closes() {
    let mut workspace = workspace();
    let id = DocumentId::new();
    workspace.open_tab(id);

    let decision = workspace.close_tab(id, false).unwrap();
    assert_eq!(decision, CloseDecision::Closed);
    assert!(workspace.tabs().is_empty());
}

#[test]
fn close_unknown_tab_is_rejected() {
    let mut workspace = workspace();
    let result = workspace.close_tab(DocumentId::new(), false);
    assert!(matches!(result, Err(WorkspaceError::TabNotFound(_))));
}

#[test]
fn open_tab_is_idempotent() {
    let mut workspace = workspace();
    let id = DocumentId::new();
    workspace.open_tab(id);
    workspace.open_tab(id);
    assert_eq!(workspace.tabs().len(), 1);
}

#[test]
fn force_close_tab_removes_even_when_dirty() {
    let mut workspace = workspace();
    let id = DocumentId::new();
    workspace.open_tab(id);
    workspace.force_close_tab(id);
    assert!(workspace.tabs().is_empty());
}

// ---------- 最近文件 ----------

#[test]
fn recent_files_dedup_and_order_by_recency() {
    let mut workspace = workspace();
    let a = path("/ws/a.md");
    let b = path("/ws/b.md");

    workspace.touch_recent(a.clone(), Timestamp::from_millis(100));
    workspace.touch_recent(b.clone(), Timestamp::from_millis(200));
    workspace.touch_recent(a.clone(), Timestamp::from_millis(300));

    let recent = workspace.recent().list();
    assert_eq!(recent.len(), 2, "同路径必须去重");
    assert_eq!(recent[0].path(), &a, "最近使用的应排在最前");
    assert_eq!(recent[1].path(), &b);
}

#[test]
fn recent_files_respects_limit() {
    let mut workspace = workspace();
    for i in 0..25 {
        workspace.touch_recent(
            path(&format!("/ws/file{i}.md")),
            Timestamp::from_millis(i + 1),
        );
    }
    assert_eq!(workspace.recent().len(), 20, "上限应为 20");
    // 最早的 5 个（i=0..4）应被裁掉。
    assert!(
        workspace
            .recent()
            .list()
            .iter()
            .all(|item| item.path() != &path("/ws/file0.md"))
    );
}

#[test]
fn forget_recent_removes_entry() {
    let mut workspace = workspace();
    workspace.touch_recent(path("/ws/a.md"), Timestamp::from_millis(1));
    workspace.forget_recent(&path("/ws/a.md"));
    assert!(workspace.recent().is_empty());
}

// ---------- 根目录与事件 ----------

#[test]
fn set_root_emits_event_and_keeps_tabs() {
    let mut workspace = workspace();
    let id = DocumentId::new();
    workspace.open_tab(id);
    workspace.take_events();

    workspace.set_root(path("/other"));
    assert_eq!(workspace.root(), Some(&path("/other")));
    assert_eq!(
        workspace.tabs().len(),
        1,
        "切换根目录不得丢失正在编辑的内容"
    );

    let events = workspace.take_events();
    assert!(matches!(
        events.first(),
        Some(WorkspaceEvent::RootChanged { .. })
    ));
    assert!(events.iter().all(|e| e.workspace_id() == workspace.id()));
}

#[test]
fn replace_entries_replaces_tree() {
    let mut workspace = workspace();
    workspace
        .add_entry(WorkspaceEntry::file(path("/ws/old.md")))
        .unwrap();
    workspace.replace_entries(vec![WorkspaceEntry::file(path("/ws/new.md"))]);
    assert_eq!(workspace.entries().len(), 1);
    assert_eq!(workspace.entries()[0].path(), &path("/ws/new.md"));
}
