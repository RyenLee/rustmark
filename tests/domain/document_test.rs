//! Document 上下文单测：值对象、聚合结构与编辑命令（S3-01 / S3-06 / S3-07）。

use rustmark::domain::document::aggregate::Document;
use rustmark::domain::document::errors::DocumentError;
use rustmark::domain::document::events::DocumentEvent;
use rustmark::domain::document::value_objects::{Cursor, ModificationState};
use rustmark::shared::geometry::{ByteOffset, TextRange};
use rustmark::shared::path::FilePath;
use rustmark::shared::time::Timestamp;

use crate::support::text_buffer::buffer_of;

const T0: Timestamp = Timestamp::from_millis(1_000);

fn doc(text: &str) -> Document {
    Document::from_content(buffer_of(""), text, T0).expect("构造文档失败")
}

// ---------- S3-01 值对象 ----------

#[test]
fn cursor_move_clears_selection() {
    let mut cursor = Cursor::new(ByteOffset::new(0));
    cursor.select_to(ByteOffset::new(5));
    assert!(cursor.has_selection());

    cursor.move_to(ByteOffset::new(3));
    assert!(!cursor.has_selection());
    assert_eq!(cursor.position().value(), 3);
    assert_eq!(cursor.anchor(), None);
}

#[test]
fn cursor_select_to_same_position_is_noop() {
    let mut cursor = Cursor::new(ByteOffset::new(4));
    cursor.select_to(ByteOffset::new(4));
    assert!(!cursor.has_selection(), "拖回原点应取消选区");

    cursor.select_to(ByteOffset::new(9));
    assert!(cursor.has_selection());
    // 再次拖到同一位置：锚点未变，选区应保留（拖回锚点才会取消）。
    cursor.select_to(ByteOffset::new(9));
    assert!(cursor.has_selection());
    cursor.select_to(ByteOffset::new(4));
    assert!(!cursor.has_selection(), "拖回锚点应取消选区");
}

#[test]
fn cursor_selection_returns_normalized_range() {
    let forward = Cursor::with_anchor(ByteOffset::new(8), ByteOffset::new(2));
    let backward = Cursor::with_anchor(ByteOffset::new(2), ByteOffset::new(8));
    let expected = TextRange::new(ByteOffset::new(2), ByteOffset::new(8));

    assert_eq!(forward.selection(), Some(expected));
    assert_eq!(backward.selection(), Some(expected), "选区必须是归一化的");
}

#[test]
fn modification_state_transitions() {
    let mut state = ModificationState::clean();
    assert!(!state.is_dirty());
    state.mark_dirty();
    assert!(state.is_dirty());
    assert!(state.needs_save());
    state.mark_clean();
    assert!(!state.is_dirty());
    assert!(ModificationState::dirty().is_dirty());
}

// ---------- S3-06 结构与访问器 ----------

#[test]
fn new_untitled_is_clean_and_empty() {
    let document = Document::new_untitled(buffer_of(""), T0);
    assert_eq!(document.len_bytes(), 0);
    assert!(document.is_empty());
    assert!(!document.is_dirty());
    assert_eq!(document.revision(), 0);
    assert_eq!(document.path(), None);
    assert_eq!(document.meta().title(), "未命名");
    assert_eq!(document.meta().last_saved_at(), None);
}

#[test]
fn from_content_populates_buffer() {
    let document = doc("hello");
    assert_eq!(document.text(), "hello");
    assert_eq!(document.len_bytes(), 5);
}

#[test]
fn open_sets_path_title_and_clean_state() {
    let path = FilePath::new("/tmp/notes/readme.md").expect("路径应合法");
    let document = Document::open(
        rustmark::shared::ids::DocumentId::new(),
        path.clone(),
        buffer_of("# hi"),
        Some(T0),
        T0,
    );
    assert_eq!(document.path(), Some(&path));
    assert_eq!(document.meta().title(), "readme");
    assert!(!document.is_dirty());
    assert_eq!(document.text(), "# hi");
}

// ---------- S3-07 编辑命令 ----------

#[test]
fn insert_updates_length_and_cursor() {
    let mut document = doc("ab");
    document.insert_at(ByteOffset::new(1), "X", T0).unwrap();
    assert_eq!(document.text(), "aXb");
    assert_eq!(document.cursor().position().value(), 2);
    assert!(document.is_dirty());
    assert_eq!(document.revision(), 1);
}

#[test]
fn insert_with_selection_replaces_selection() {
    let mut document = doc("hello world");
    document.select(TextRange::new(ByteOffset::new(0), ByteOffset::new(5)));
    document.insert("HI", T0).unwrap();
    assert_eq!(document.text(), "HI world");
}

#[test]
fn remove_deletes_range() {
    let mut document = doc("hello world");
    document
        .remove(TextRange::new(ByteOffset::new(5), ByteOffset::new(11)), T0)
        .unwrap();
    assert_eq!(document.text(), "hello");
}

#[test]
fn remove_out_of_range_returns_err_instead_of_panic() {
    let mut document = doc("hello");
    let result = document.remove(TextRange::new(ByteOffset::new(0), ByteOffset::new(100)), T0);
    assert!(matches!(result, Err(DocumentError::InvalidRange { .. })));
    assert_eq!(document.text(), "hello", "失败的编辑不得修改缓冲区");
}

#[test]
fn insert_at_offset_beyond_end_returns_err() {
    let mut document = doc("hello");
    let result = document.insert_at(ByteOffset::new(99), "!", T0);
    assert!(matches!(result, Err(DocumentError::InvalidRange { .. })));
}

#[test]
fn insert_at_end_offset_is_allowed() {
    let mut document = doc("hello");
    document.insert_at(ByteOffset::new(5), "!", T0).unwrap();
    assert_eq!(document.text(), "hello!");
}

#[test]
fn consecutive_inserts_produce_two_events_then_queue_drains() {
    let mut document = doc("");
    document.insert_at(ByteOffset::new(0), "a", T0).unwrap();
    document.insert_at(ByteOffset::new(1), "b", T0).unwrap();

    let events = document.pending_events();
    assert_eq!(events.len(), 2, "每次编辑应产生一条 TextModified");
    assert!(events.iter().all(DocumentEvent::changes_content));

    let taken = document.take_events();
    assert_eq!(taken.len(), 2);
    assert!(document.take_events().is_empty(), "取走后队列应清空");
}

#[test]
fn noop_edit_produces_no_event() {
    let mut document = doc("x");
    document
        .replace(TextRange::empty(ByteOffset::new(0)), "", T0)
        .unwrap();
    assert!(document.pending_events().is_empty());
    assert_eq!(document.revision(), 0);
}

#[test]
fn event_carries_same_document_id() {
    let mut document = doc("x");
    document.insert_at(ByteOffset::new(0), "y", T0).unwrap();
    let events = document.take_events();
    let id = document.id();
    assert!(events.iter().all(|e| e.document_id() == id));
}

#[test]
fn cjk_insert_respects_utf8_boundaries() {
    let mut document = doc("中文");
    // 偏移 1 落在「中」的中间字节上，必须被拒绝而不是 panic。
    let result = document.insert_at(ByteOffset::new(1), "!", T0);
    assert!(matches!(result, Err(DocumentError::NotCharBoundary { .. })));

    // 偏移 3 是「中」之后的合法边界。
    document.insert_at(ByteOffset::new(3), "!", T0).unwrap();
    assert_eq!(document.text(), "中!文");
}

#[test]
fn move_cursor_clamps_to_length() {
    let mut document = doc("abc");
    document.move_cursor(ByteOffset::new(99));
    assert_eq!(document.cursor().position().value(), 3);
}

#[test]
fn select_clamps_to_length() {
    let mut document = doc("abc");
    document.select(TextRange::new(ByteOffset::new(0), ByteOffset::new(99)));
    assert_eq!(
        document.cursor().selection(),
        Some(TextRange::new(ByteOffset::new(0), ByteOffset::new(3)))
    );
}

// ---------- 保存 / 关闭 ----------

#[test]
fn mark_saved_clears_dirty_and_emits_event() {
    let mut document = doc("hello");
    document.insert_at(ByteOffset::new(5), "!", T0).unwrap();
    let path = FilePath::new("/tmp/a.md").expect("路径应合法");
    document.mark_saved(path.clone(), T0).unwrap();

    assert!(!document.is_dirty());
    assert_eq!(document.path(), Some(&path));
    assert_eq!(document.meta().last_saved_at(), Some(T0));
    let events = document.take_events();
    assert!(matches!(
        events.last(),
        Some(DocumentEvent::DocumentSaved { .. })
    ));
}

#[test]
fn mark_saved_on_clean_document_returns_not_dirty() {
    let mut document = doc("hello");
    let path = FilePath::new("/tmp/a.md").expect("路径应合法");
    // 首次保存到新路径：即使是干净文档也应允许（视为「另存为」）。
    document.mark_saved(path.clone(), T0).unwrap();
    let result = document.mark_saved(path, T0);
    assert!(matches!(result, Err(DocumentError::NotDirty)));
}

#[test]
fn to_snapshot_requires_path() {
    let mut document = doc("hello");
    assert!(document.to_snapshot().is_none());

    let path = FilePath::new("/tmp/a.md").expect("路径应合法");
    document.mark_saved(path, T0).unwrap();
    let snapshot = document.to_snapshot().expect("应有快照");
    assert_eq!(snapshot.content, "hello");
}

#[test]
fn mark_closed_emits_event() {
    let mut document = doc("hello");
    document.mark_closed();
    let events = document.take_events();
    assert!(matches!(
        events.first(),
        Some(DocumentEvent::DocumentClosed { .. })
    ));
}

// ---------- 事件转换（S3-03） ----------

#[test]
fn document_event_converts_to_domain_event_preserving_id() {
    use rustmark::shared::event::DomainEvent;

    let mut document = doc("hello");
    document.insert_at(ByteOffset::new(5), "!", T0).unwrap();
    let events = document.take_events();
    let id = document.id();

    for event in events {
        let domain_event: DomainEvent = event.into();
        assert_eq!(domain_event.document_id(), Some(id));
    }
}
