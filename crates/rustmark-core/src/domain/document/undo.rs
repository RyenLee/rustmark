//! 撤销 / 重做：**每个 Document 私有的**值对象（S3-08）。
//!
//! # 为什么是 per-Document 而不是全局
//!
//! 原型（`markdown-editor-prototype`）用一个全局 `lastCommit` 时间戳驱动撤销合并，
//! 导致「在 A 文档打字 → 切到 B 文档 → 立刻撤销」会撤销到**错误文档的历史**，
//! 表现为跨文件写坏数据。本设计把撤销栈与合并窗口状态一起放进 `Document` 聚合，
//! 从类型系统层面消除这一整类缺陷：两个文档**不可能**共享合并状态。
//!
//! # 契约
//!
//! - 调用方在**执行编辑之前**调用 [`UndoStack::push`]，传入编辑前的快照；
//! - `undo` / `redo` 由调用方传入「当前快照」，返回「应恢复到的快照」；
//! - 合并只在**同一栈内、同一类型、同一时间窗**内发生。

use crate::ports::time::Timestamp;
use crate::shared::geometry::TextRange;

use super::value_objects::ModificationKind;

/// 撤销 / 重做栈的默认总字节预算（64 MiB）。
///
/// 快照是**全文副本**，只按条数限制挡不住内存：1MB 文档 200 份即 200 MiB，
/// 单独超过 NFR-PERF-03 的 150 MB 预算。故在条数之外再加一道按字节的硬上限，
/// 超限从**最旧**的快照开始丢弃（先丢 redo 的远端，再丢 undo 的最旧项）。
pub const DEFAULT_MAX_BYTES: usize = 64 * 1024 * 1024;

/// 撤销栈的合并与容量策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UndoPolicy {
    /// 合并窗口（毫秒）：窗口内连续的同类编辑合并为一个撤销点。
    merge_window_ms: u64,
    /// 撤销栈最大深度；超出后**丢弃最旧**的快照。
    max_depth: usize,
    /// undo + redo 两份栈的合计字节上限；超出后丢弃最旧的快照。
    max_bytes: usize,
}

impl UndoPolicy {
    /// 默认策略：400ms 合并窗口（需求文档 §10 的初值），最大深度 200，
    /// 合计字节预算 64 MiB。
    pub const fn default_policy() -> Self {
        Self {
            merge_window_ms: 400,
            max_depth: 200,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }

    /// 自定义策略；`max_depth` 为 0 时按 1 处理（至少保留一个撤销点），
    /// 字节预算沿用 [`DEFAULT_MAX_BYTES`]。
    pub fn new(merge_window_ms: u64, max_depth: usize) -> Self {
        Self {
            merge_window_ms,
            max_depth: max_depth.max(1),
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }

    /// 覆盖合计字节预算；0 按 1 处理（至少留一份快照，避免撤销能力被关闭）。
    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes.max(1);
        self
    }

    /// 合并窗口（毫秒）。
    pub const fn merge_window_ms(&self) -> u64 {
        self.merge_window_ms
    }

    /// 撤销栈最大深度。
    pub const fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// undo + redo 的合计字节上限。
    pub const fn max_bytes(&self) -> usize {
        self.max_bytes
    }
}

impl Default for UndoPolicy {
    fn default() -> Self {
        Self::default_policy()
    }
}

/// 撤销快照：某一时刻的完整文本与选区。
///
/// 之所以存**完整文本**而非反向补丁：撤销合并需要任意取回「某次编辑之前的全文」，
/// 且文本规模在目标场景（≤ 1 MB）下可接受；阶段 08 若实测成为热点，
/// 再在本类型内部改为补丁链，**对外 API 不变**。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    content: String,
    selection: Option<TextRange>,
    at: Timestamp,
}

impl Snapshot {
    /// 构造快照。
    pub fn new(content: impl Into<String>, selection: Option<TextRange>, at: Timestamp) -> Self {
        Self {
            content: content.into(),
            selection,
            at,
        }
    }

    /// 快照文本。
    pub fn content(&self) -> &str {
        &self.content
    }

    /// 快照时刻的选区。
    pub(crate) const fn selection(&self) -> Option<TextRange> {
        self.selection
    }

    /// 快照时刻。
    pub(crate) const fn at(&self) -> Timestamp {
        self.at
    }
}

/// 撤销 / 重做栈（每文档一份）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UndoStack {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    policy: UndoPolicy,
    /// 上一次记录的编辑类型与时刻——**随栈一起归属文档**，绝不共享。
    last_kind: Option<ModificationKind>,
    last_at: Option<Timestamp>,
}

impl UndoStack {
    /// 按策略构造一个空栈。
    pub fn new(policy: UndoPolicy) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            policy,
            last_kind: None,
            last_at: None,
        }
    }

    /// 使用默认策略构造空栈。
    pub fn with_default_policy() -> Self {
        Self::new(UndoPolicy::default_policy())
    }

    /// 记录「编辑发生之前」的状态。
    ///
    /// - `before`：编辑前的快照；
    /// - `kind`：即将执行的编辑类型。
    ///
    /// 返回 `true` 表示**已与上一次编辑合并**（未产生新撤销点）；`false` 表示已入栈。
    ///
    /// 合并条件（三者同时满足）：
    /// ① 上一次编辑也是插入类；② 距上一次记录在 `merge_window_ms` 之内（含端点）；
    /// ③ 上一次记录就发生在**本栈**（由类型归属保证，跨文档不可能合并）。
    ///
    /// ⚠ **要合并的那份快照是白构造的**：合并命中时它被直接丢弃，而它含一份整篇
    /// 文本。调用方若在意这个成本（编辑聚合的每击键路径就很在意），应先用
    /// [`Self::would_merge`] 判、命中则改调 [`Self::merge`]——见
    /// [`crate::domain::document::aggregate::Document::apply`]。
    pub fn push(&mut self, before: Snapshot, kind: ModificationKind) -> bool {
        if self.merges_with_last(kind, before.at()) {
            // 滚动窗口：以本次时间作为新的窗口起点，未入栈。
            self.last_at = Some(before.at());
            return true;
        }

        let at = before.at();
        self.undo.push(before);
        // 新的编辑使重做链失效（标准撤销语义）；先清再结算预算，
        // 避免刚被作废的 redo 快照继续计入内存。
        self.redo.clear();
        self.enforce_budget();
        self.last_kind = Some(kind);
        self.last_at = Some(at);
        false
    }

    /// 只回答「这次编辑会不会与上一次合并」，**不构造任何快照**（问题 053）。
    ///
    /// 判据与 [`Self::push`] 完全同源（共用 [`Self::merges_with_last`]），
    /// 故「先判后建」与「先建后判」的结果逐例一致。
    pub(crate) fn would_merge(&self, kind: ModificationKind, at: Timestamp) -> bool {
        self.merges_with_last(kind, at)
    }

    /// 合并命中：只把窗口起点推到 `at`，不入栈、不构造快照。
    ///
    /// 与 [`Self::push`] 的合并分支逐字段等价（那里写的就是 `last_at = at`）。
    pub(crate) fn merge(&mut self, at: Timestamp) {
        self.last_at = Some(at);
    }

    /// 合并判据的唯一实现（[`Self::push`] 与 [`Self::would_merge`] 共用）。
    fn merges_with_last(&self, kind: ModificationKind, at: Timestamp) -> bool {
        matches!(
            (self.last_kind, self.last_at),
            (Some(last_kind), Some(last_at))
                if last_kind.mergeable_with(kind)
                    && at.within(last_at, self.policy.merge_window_ms)
        )
    }

    /// 撤销一步：弹出最近一次「编辑前的状态」，并把 `current` 压入重做栈。
    ///
    /// 栈空时返回 `None`（**不报错**——「没有可撤销的操作」通常是正常的 UI 状态，
    /// 而 [`crate::domain::document::errors::DocumentError::UndoStackEmpty`] 用于调用方
    /// 需要向用户解释的场景）。
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        self.enforce_budget();
        self.reset_merge_state();
        Some(previous)
    }

    /// 重做一步：弹出最近一次被撤销的状态，并把 `current` 压入撤销栈。
    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.enforce_budget();
        self.reset_merge_state();
        Some(next)
    }

    /// 是否可撤销。
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// 是否可重做。
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 当前撤销栈深度（已合并的连续插入只算一个）。
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// 当前重做栈深度。
    pub(crate) fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// 清空重做栈（例如文档被外部程序改写，历史不再可信）。
    pub fn clear_redo(&mut self) {
        self.redo.clear();
    }

    /// 结算条数与字节预算：先按深度裁 undo，再按合计字节丢弃最旧快照。
    ///
    /// 字节压力下**优先丢弃 redo 的远端**（那是用户最后才会重做到的步骤），
    /// 再丢 undo 的最旧项；无论如何至少保留一份快照，否则撤销能力被关闭。
    fn enforce_budget(&mut self) {
        if self.undo.len() > self.policy.max_depth {
            let excess = self.undo.len() - self.policy.max_depth;
            self.undo.drain(0..excess);
        }
        while self.total_bytes() > self.policy.max_bytes && self.undo.len() + self.redo.len() > 1 {
            if !self.redo.is_empty() {
                self.redo.remove(0);
            } else {
                self.undo.remove(0);
            }
        }
    }

    /// undo + redo 合计字节数（快照是全文副本，故按 `content.len()` 计）。
    fn total_bytes(&self) -> usize {
        self.undo
            .iter()
            .chain(self.redo.iter())
            .map(|snapshot| snapshot.content.len())
            .sum()
    }

    /// 重置合并窗口状态。
    ///
    /// 撤销 / 重做之后必须与上一次编辑「断开」，否则撤销后立刻输入会与撤销前的输入合并。
    fn reset_merge_state(&mut self) {
        self.last_kind = None;
        self.last_at = None;
    }
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::with_default_policy()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(text: &str, ms: u64) -> Snapshot {
        Snapshot::new(text, None, Timestamp::from_millis(ms))
    }

    /// 只按条数限制挡不住全文快照：预算收紧后应丢最旧项。
    #[test]
    fn byte_budget_evicts_oldest_snapshot() {
        let mut stack = UndoStack::new(UndoPolicy::new(0, 200).with_max_bytes(75));
        for ms in 1..=10 {
            stack.push(snap(&"x".repeat(30), ms), ModificationKind::Insert);
        }
        // 30 B/份、预算 75 B → 只应保留 2 份（60 B），第 3 份会越预算。
        assert_eq!(stack.undo_depth(), 2);
    }

    /// 字节压力下先丢 redo 的远端，保住 undo 的可撤销性。
    #[test]
    fn byte_budget_prefers_dropping_redo() {
        let mut stack = UndoStack::new(UndoPolicy::new(0, 10).with_max_bytes(150));
        stack.push(snap(&"a".repeat(100), 1), ModificationKind::Insert);
        stack.push(snap(&"b".repeat(50), 2), ModificationKind::Insert);
        assert_eq!(stack.undo_depth(), 2);

        let _ = stack.undo(snap(&"c".repeat(100), 3));
        assert_eq!(stack.undo_depth(), 1, "undo 的一步应保留");
        assert!(!stack.can_redo(), "超预算时应优先丢弃 redo");
    }
}
