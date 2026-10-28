//! 撤销**策略**单测：合并窗口的滚动语义与容量下限（评估报告 E-26）。
//!
//! 与 `undo_test.rs` 的分工：那边测**文档级**行为（跨文档不串、撤销恢复选区、
//! 重做链清空…），走 `Document::apply`；这边只测 [`UndoPolicy`] / [`UndoStack`]
//! 自身的两条纯逻辑契约——它们都没有经过文档聚合，属于「换个调用方就没人守」
//! 的部分：
//!
//! 1. **合并窗口是滚动的**：每次命中合并都把窗口起点推到**本次**编辑时刻，
//!    而不是钉死在「第一次编辑」。故连续打字（击键间隔 < 窗口）无论打多久
//!    都只产生一个撤销点。朴素实现若用 `first_edit_at` 计时，会每隔一个窗口
//!    多切一刀——用户按一次撤销只退掉最近几个字符，且越长的段落越碎。
//! 2. **容量有下限**：`max_depth` / `max_bytes` 为 0 时按 1 处理，且字节淘汰
//!    永远不会丢掉最后一份快照（否则等于静默关闭撤销能力）。

use rustmark::domain::document::undo::{Snapshot, UndoPolicy, UndoStack};
use rustmark::domain::document::value_objects::ModificationKind;
use rustmark::ports::time::Timestamp;

/// 默认合并窗口（`UndoPolicy::default_policy()` 为 400ms）。
const WINDOW_MS: u64 = 400;

fn snap(text: &str, ms: u64) -> Snapshot {
    Snapshot::new(text, None, Timestamp::from_millis(ms))
}

fn stack(window_ms: u64) -> UndoStack {
    UndoStack::new(UndoPolicy::new(window_ms, 200))
}

/// 连续打字（每次间隔 300ms，均 < 400ms 窗口）只产生**一个**撤销点。
///
/// 同时钉死 `push` 的返回值语义：`false` = 新入栈，`true` = 已合并。
#[test]
fn rolling_window_merges_an_arbitrarily_long_typing_run() {
    let mut stack = stack(WINDOW_MS);
    assert!(
        !stack.push(snap("x", 0), ModificationKind::Insert),
        "首次必入栈"
    );
    for ms in [300, 600, 900, 1_200] {
        assert!(
            stack.push(snap("x", ms), ModificationKind::Insert),
            "{ms}ms 处应与上一次合并（距上次 300ms < {WINDOW_MS}ms 窗口）"
        );
    }
    assert_eq!(stack.undo_depth(), 1, "整段连续输入应并成一个撤销点");
}

/// 间隔超过窗口即新开撤销点——窗口不是「整段会话只算一次」。
#[test]
fn gap_beyond_window_starts_a_new_undo_point() {
    let mut stack = stack(WINDOW_MS);
    stack.push(snap("x", 0), ModificationKind::Insert);
    stack.push(snap("x", 300), ModificationKind::Insert);
    // 距上一次命中合并后的窗口起点（300ms）已过 700ms。
    assert!(!stack.push(snap("x", 1_000), ModificationKind::Insert));
    assert_eq!(stack.undo_depth(), 2);
}

/// 恰好等于窗口宽度仍算窗口内（`Timestamp::within` 的端点是闭的）。
#[test]
fn merge_window_boundary_is_inclusive() {
    let mut stack = stack(WINDOW_MS);
    stack.push(snap("x", 0), ModificationKind::Insert);
    assert!(
        stack.push(snap("x", WINDOW_MS), ModificationKind::Insert),
        "elapsed == window 应判为窗口内"
    );
    assert_eq!(stack.undo_depth(), 1);
}

/// 零值容量按 1 处理：撤销能力不得被配置成「关闭」。
#[test]
fn policy_clamps_zero_limits_to_one() {
    let policy = UndoPolicy::new(0, 0).with_max_bytes(0);
    assert_eq!(policy.merge_window_ms(), 0, "窗口 0 是合法值（不合并）");
    assert_eq!(policy.max_depth(), 1);
    assert_eq!(policy.max_bytes(), 1);
}

/// 字节预算再紧也保留最后一份快照。
///
/// 判据取「预算 1 B、快照 100 B」：淘汰循环的条件含
/// `undo.len() + redo.len() > 1`，故唯一一份必须留下——
/// 否则用户会在一次大粘贴后再也撤销不了，且没有任何提示。
#[test]
fn byte_budget_keeps_at_least_one_snapshot() {
    let mut stack = UndoStack::new(UndoPolicy::new(0, 200).with_max_bytes(1));
    stack.push(snap(&"x".repeat(100), 1), ModificationKind::Insert);
    assert_eq!(stack.undo_depth(), 1, "唯一一份快照不得被丢弃");
    assert!(stack.can_undo(), "撤销能力必须还在");
}

/// 空栈重做返回 `None` 且不改变状态（与撤销空栈对称）。
#[test]
fn redo_on_empty_stack_is_noop() {
    let mut stack = stack(WINDOW_MS);
    assert!(stack.redo(snap("x", 1)).is_none());
    assert!(!stack.can_redo());
    assert_eq!(stack.undo_depth(), 0);
}
