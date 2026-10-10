//! 撤销 / 重做单测（S3-08）。
//!
//! 前 6 条是**原型缺陷 1.2 的免疫用例**：原型的全局 `lastCommit` 导致撤销跨文件污染，
//! 本设计的撤销栈与合并状态均为 per-Document，这些用例用于固化该行为。

use rustmark::domain::document::aggregate::Document;
use rustmark::domain::document::errors::DocumentError;
use rustmark::domain::document::undo::{Snapshot, UndoPolicy, UndoStack};
use rustmark::domain::document::value_objects::ModificationKind;
use rustmark::shared::geometry::{ByteOffset, TextRange};
use rustmark::shared::time::Timestamp;

use crate::support::text_buffer::buffer_of;

const T0: Timestamp = Timestamp::from_millis(1_000);

fn doc(text: &str) -> Document {
    Document::from_content(buffer_of(""), text, T0).expect("构造文档失败")
}

fn snapshot(content: &str, at: Timestamp) -> Snapshot {
    Snapshot::new(content, None, at)
}

// ---------- 原型缺陷免疫用例 ----------

/// 原型复现场景 1：在 A 打字 → 切到 B 打字 → 撤销 B，**不得**改动 A 的内容。
#[test]
fn undo_is_scoped_per_document() {
    let mut document_a = doc("AAA");
    let mut document_b = doc("BBB");

    document_a.insert_at(ByteOffset::new(3), "1", T0).unwrap();
    document_b.insert_at(ByteOffset::new(3), "2", T0).unwrap();
    assert_eq!(document_a.text(), "AAA1");
    assert_eq!(document_b.text(), "BBB2");

    document_b.undo(Timestamp::from_millis(2_000)).unwrap();

    assert_eq!(document_b.text(), "BBB", "B 应回到自己的历史");
    assert_eq!(document_a.text(), "AAA1", "A 的内容绝不能被 B 的撤销影响");
    assert_eq!(document_a.undo_depth(), 1, "A 的撤销栈不受 B 影响");
}

/// 原型复现场景 2：打开 B 后的第一笔输入**不得**与 A 的最后一次提交合并。
#[test]
fn undo_after_switch_does_not_merge_across_documents() {
    let mut document_a = doc("");
    let mut document_b = doc("");

    // A 在 t=1000 输入；B 在 t=1100（在 400ms 合并窗口之外但在原型中会误合并的间隔）输入。
    document_a
        .insert_at(ByteOffset::new(0), "a", Timestamp::from_millis(1_000))
        .unwrap();
    document_b
        .insert_at(ByteOffset::new(0), "b", Timestamp::from_millis(1_100))
        .unwrap();

    // B 的第一笔输入必须独立成为一个撤销点（原型里会因为全局 lastCommit 被吞掉）。
    assert_eq!(document_b.undo_depth(), 1, "B 的首笔输入必须独立成撤销点");
    assert_eq!(document_a.undo_depth(), 1);

    document_b.undo(Timestamp::from_millis(1_200)).unwrap();
    assert_eq!(document_b.text(), "");
    assert_eq!(document_a.text(), "a", "A 不受 B 撤销的影响");
}

/// 撤销后应恢复当时的选区，而不是把光标扔到文档末尾。
#[test]
fn undo_restores_selection() {
    let mut document = doc("hello world");
    let selection = TextRange::new(ByteOffset::new(0), ByteOffset::new(5));
    document.select(selection);
    document.insert("HI", T0).unwrap();
    assert_eq!(document.text(), "HI world");

    document.undo(Timestamp::from_millis(2_000)).unwrap();
    assert_eq!(document.text(), "hello world");
    assert_eq!(document.cursor().selection(), Some(selection));
}

/// 重做应重新应用被撤销的修改。
#[test]
fn redo_reapplies_change() {
    let mut document = doc("hello");
    document.insert_at(ByteOffset::new(5), "!", T0).unwrap();
    document.undo(Timestamp::from_millis(2_000)).unwrap();
    assert_eq!(document.text(), "hello");

    document.redo(Timestamp::from_millis(3_000)).unwrap();
    assert_eq!(document.text(), "hello!");
}

/// 撤销后输入新内容，只清空**当前文档**的重做栈。
#[test]
fn new_input_after_undo_clears_redo_only_for_current_document() {
    let mut document_a = doc("A");
    let mut document_b = doc("B");

    document_a.insert_at(ByteOffset::new(1), "1", T0).unwrap();
    document_b.insert_at(ByteOffset::new(1), "2", T0).unwrap();
    document_a.undo(Timestamp::from_millis(2_000)).unwrap();
    document_b.undo(Timestamp::from_millis(2_000)).unwrap();
    assert_eq!(document_a.redo_depth(), 1);
    assert_eq!(document_b.redo_depth(), 1);

    // 只给 A 输入新内容
    document_a
        .insert_at(ByteOffset::new(0), "x", Timestamp::from_millis(3_000))
        .unwrap();
    assert_eq!(document_a.redo_depth(), 0, "A 的重做链应失效");
    assert_eq!(document_b.redo_depth(), 1, "B 的重做链不受影响");
    assert!(document_b.redo(Timestamp::from_millis(4_000)).is_ok());
    assert_eq!(document_b.text(), "B2");
}

/// 空栈撤销是无操作，返回错误而不是 panic。
#[test]
fn undo_on_empty_stack_is_noop() {
    let mut document = doc("abc");
    assert!(matches!(
        document.undo(T0),
        Err(DocumentError::UndoStackEmpty)
    ));
    assert!(matches!(
        document.redo(T0),
        Err(DocumentError::RedoStackEmpty)
    ));
    assert_eq!(document.text(), "abc");
}

// ---------- 合并窗口 ----------

#[test]
fn merge_within_window_is_single_undo_point() {
    let mut document = doc("abc");
    document
        .insert_at(ByteOffset::new(3), "d", Timestamp::from_millis(1_000))
        .unwrap();
    document
        .insert_at(ByteOffset::new(4), "e", Timestamp::from_millis(1_200))
        .unwrap();
    document
        .insert_at(ByteOffset::new(5), "f", Timestamp::from_millis(1_300))
        .unwrap();
    assert_eq!(document.text(), "abcdef");
    assert_eq!(
        document.undo_depth(),
        1,
        "窗口内的连续插入应合并为一个撤销点"
    );

    document.undo(Timestamp::from_millis(5_000)).unwrap();
    assert_eq!(document.text(), "abc");
}

#[test]
fn beyond_window_creates_separate_undo_point() {
    let mut document = doc("abc");
    document
        .insert_at(ByteOffset::new(3), "d", Timestamp::from_millis(1_000))
        .unwrap();
    document
        .insert_at(ByteOffset::new(4), "e", Timestamp::from_millis(2_000))
        .unwrap();
    assert_eq!(
        document.undo_depth(),
        2,
        "超窗（1000ms > 400ms）应产生两个撤销点"
    );

    document.undo(Timestamp::from_millis(2_100)).unwrap();
    assert_eq!(document.text(), "abcd");
    document.undo(Timestamp::from_millis(2_200)).unwrap();
    assert_eq!(document.text(), "abc");
}

#[test]
fn remove_is_never_merged_with_insert() {
    let mut stack = UndoStack::new(UndoPolicy::new(400, 100));
    stack.push(snapshot("a", T0), ModificationKind::Insert);
    stack.push(snapshot("ab", T0), ModificationKind::Remove);
    stack.push(snapshot("a", T0), ModificationKind::Insert);
    // 插入→删除不合并；删除→插入不合并；只有两次插入之间才可能合并。
    assert_eq!(stack.undo_depth(), 3);
}

// ---------- 栈本身 ----------

#[test]
fn undo_stack_respects_max_depth() {
    let mut stack = UndoStack::new(UndoPolicy::new(0, 3));
    for i in 0..10 {
        stack.push(
            snapshot(
                &format!("state{i}"),
                Timestamp::from_millis(1_000 + i * 10_000),
            ),
            ModificationKind::Replace,
        );
    }
    assert_eq!(stack.undo_depth(), 3, "超出深度应丢弃最旧的快照");
}

#[test]
fn undo_then_redo_roundtrip() {
    let mut stack = UndoStack::with_default_policy();
    stack.push(snapshot("one", T0), ModificationKind::Insert);

    let restored = stack.undo(snapshot("two", T0)).expect("应有可撤销项");
    assert_eq!(restored.content(), "one");
    assert!(!stack.can_undo());
    assert!(stack.can_redo());

    let redone = stack.redo(snapshot("one", T0)).expect("应有可重做项");
    assert_eq!(redone.content(), "two");
    assert!(stack.can_undo());
    assert!(!stack.can_redo());
}

#[test]
fn undo_disconnects_merge_window() {
    let mut stack = UndoStack::new(UndoPolicy::new(400, 10));
    stack.push(
        snapshot("a", Timestamp::from_millis(1_000)),
        ModificationKind::Insert,
    );
    let _ = stack.undo(snapshot("ab", Timestamp::from_millis(1_100)));
    // 撤销后立刻插入：不得与撤销前的插入合并（否则会撤销掉不该撤销的内容）。
    let merged = stack.push(
        snapshot("a", Timestamp::from_millis(1_150)),
        ModificationKind::Insert,
    );
    assert!(!merged, "撤销必须重置合并窗口");
    assert_eq!(stack.undo_depth(), 1);
}

#[test]
fn clear_redo_drops_history() {
    let mut stack = UndoStack::with_default_policy();
    stack.push(snapshot("a", T0), ModificationKind::Insert);
    let _ = stack.undo(snapshot("b", T0));
    assert!(stack.can_redo());
    stack.clear_redo();
    assert!(!stack.can_redo());
}
