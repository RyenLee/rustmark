//! Document 聚合根（S3-06 结构与访问器、S3-07 编辑命令、S3-08 撤销重做）。
//!
//! # 一致性边界
//!
//! 聚合保证以下不变量：
//! 1. 光标偏移始终落在 `[0, len_bytes]` 内；
//! 2. 任何编辑命令成功后，`state` 为 `Dirty` 且 `revision` 自增 1；
//! 3. 撤销栈为**本聚合私有**，与其他文档不存在任何共享状态（原型缺陷 1.2 的根治）；
//! 4. 事件队列只增不减，由 [`Document::take_events`] 一次性取走并清空。
//!
//! # 不持有 IO
//!
//! 保存动作只在此处**记录状态**（`mark_saved`），真正的磁盘写入由应用层调用
//! [`crate::domain::document::repository::DocumentRepository`] 完成。

use crate::shared::geometry::{ByteOffset, TextRange};
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;
use crate::shared::time::Timestamp;

use super::errors::DocumentError;
use super::events::DocumentEvent;
use super::repository::DocumentSnapshot;
use super::services::TextBuffer;
use super::undo::{Snapshot, UndoPolicy, UndoStack};
use super::value_objects::{Cursor, DocumentMeta, Edit, EditPlan, ModificationState};

/// 文本缓冲区的trait 对象别名：跨层实现必须可安全共享。
pub type BufferHandle = Box<dyn TextBuffer + Send + Sync>;

/// 文档聚合根。
///
/// 通过构造参数注入 [`TextBuffer`] 实现（阶段 05 注入 `RopeTextBuffer`），
/// 因此领域层可以零 `ropey` 依赖地单测。
pub struct Document {
    id: DocumentId,
    buffer: BufferHandle,
    cursor: Cursor,
    meta: DocumentMeta,
    state: ModificationState,
    undo: UndoStack,
    events: Vec<DocumentEvent>,
    revision: u64,
    path: Option<FilePath>,
}

impl Document {
    // ---------- 构造（S3-06） ----------

    /// 新建一个空白未命名文档。
    pub fn new_untitled(buffer: BufferHandle, now: Timestamp) -> Self {
        Self {
            id: DocumentId::new(),
            buffer,
            cursor: Cursor::default(),
            meta: DocumentMeta::new_untitled(now),
            state: ModificationState::clean(),
            undo: UndoStack::default(),
            events: Vec::new(),
            revision: 0,
            path: None,
        }
    }

    /// 由已有文本新建文档（新文件，尚无路径）。
    ///
    /// `content` 写入缓冲区失败（越界）时返回错误。
    pub fn from_content(
        buffer: BufferHandle,
        content: &str,
        now: Timestamp,
    ) -> Result<Self, DocumentError> {
        let mut document = Self::new_untitled(buffer, now);
        document.buffer.insert(ByteOffset::new(0), content)?;
        Ok(document)
    }

    /// 由仓储快照打开已有文件。
    ///
    /// `buffer` 是**已填充内容**的缓冲区（由基础设施层构造，避免领域层复制大字符串）；
    /// 打开后状态为 `Clean`。
    pub fn open(
        id: DocumentId,
        path: FilePath,
        buffer: BufferHandle,
        saved_at: Option<Timestamp>,
        now: Timestamp,
    ) -> Self {
        let mut meta = DocumentMeta::from_path(&path, now);
        if let Some(at) = saved_at {
            meta.mark_saved(at);
        }
        Self {
            id,
            buffer,
            cursor: Cursor::default(),
            meta,
            state: ModificationState::clean(),
            undo: UndoStack::default(),
            events: Vec::new(),
            revision: 0,
            path: Some(path),
        }
    }

    /// 用自定义撤销策略打开已有文件（供偏好设置注入，F-1 接线）。
    ///
    /// 与 [`Self::open`] 的差别只有撤销栈的策略：合并窗口是**编辑时**的行为，
    /// 打开的文件同样要被编辑，故设置对打开路径也必须生效——
    /// 否则「改了合并窗口，只有新建文档听他的」会是第二个静默无效的设置。
    pub fn open_with_policy(
        id: DocumentId,
        path: FilePath,
        buffer: BufferHandle,
        saved_at: Option<Timestamp>,
        now: Timestamp,
        policy: UndoPolicy,
    ) -> Self {
        let mut document = Self::open(id, path, buffer, saved_at, now);
        document.undo = UndoStack::new(policy);
        document
    }

    /// 用自定义撤销策略新建空白文档（供偏好设置注入，S3-12 → S8）。
    pub fn new_untitled_with_policy(
        buffer: BufferHandle,
        policy: UndoPolicy,
        now: Timestamp,
    ) -> Self {
        let mut document = Self::new_untitled(buffer, now);
        document.undo = UndoStack::new(policy);
        document
    }

    // ---------- 访问器 ----------

    /// 文档 ID。
    pub const fn id(&self) -> DocumentId {
        self.id
    }

    /// 元数据。
    pub const fn meta(&self) -> &DocumentMeta {
        &self.meta
    }

    /// 修改状态。
    pub const fn state(&self) -> ModificationState {
        self.state
    }

    /// 光标与选区。
    pub const fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// 文件路径；未保存过的新文件为 `None`。
    pub const fn path(&self) -> Option<&FilePath> {
        self.path.as_ref()
    }

    /// 缓冲区字节长度。
    pub fn len_bytes(&self) -> usize {
        self.buffer.len_bytes()
    }

    /// 是否为空文档。
    pub fn is_empty(&self) -> bool {
        self.len_bytes() == 0
    }

    /// 是否存在未保存的修改。
    pub fn is_dirty(&self) -> bool {
        self.state.is_dirty()
    }

    /// 当前版本号（每次内容变化自增）。
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// 全文快照。
    ///
    /// 大文件下为 O(n) 操作；表现层请优先使用 [`Document::slice`] 取可视区。
    pub fn text(&self) -> String {
        self.buffer.snapshot()
    }

    /// 取 `range` 区间的文本；越界返回错误（不 panic）。
    pub fn slice(&self, range: TextRange) -> Result<String, DocumentError> {
        Ok(self.buffer.slice(range)?.into_owned())
    }

    /// 撤销栈当前深度（供状态栏与测试观察）。
    pub fn undo_depth(&self) -> usize {
        self.undo.undo_depth()
    }

    /// 重做栈当前深度。
    pub fn redo_depth(&self) -> usize {
        self.undo.redo_depth()
    }

    // ---------- 光标操作（不进撤销栈） ----------

    /// 移动光标到 `offset`（清除选区）；越界时**裁剪**到合法范围而非报错。
    pub fn move_cursor(&mut self, offset: ByteOffset) {
        let clamped = ByteOffset::new(offset.value().min(self.len_bytes()));
        self.cursor.move_to(clamped);
        self.push_selection_event();
    }

    /// 把选区设为 `range`（越界时裁剪）；并发出选区变化事件。
    pub fn select(&mut self, range: TextRange) {
        let limit = TextRange::new(ByteOffset::new(0), ByteOffset::new(self.len_bytes()));
        let clamped = range.clamp(limit);
        if clamped.is_empty() {
            self.cursor.move_to(clamped.start());
        } else {
            self.cursor = Cursor::with_anchor(clamped.end(), clamped.start());
        }
        self.push_selection_event();
    }

    // ---------- 编辑命令（S3-07） ----------

    /// 在光标处插入文本（若存在选区则先替换选区）。
    pub fn insert(&mut self, text: &str, now: Timestamp) -> Result<(), DocumentError> {
        let selection = self.cursor.selection();
        match selection {
            Some(range) => self.replace(range, text, now),
            None => self.insert_at(self.cursor.position(), text, now),
        }
    }

    /// 在 `offset` 处插入文本。
    pub fn insert_at(
        &mut self,
        offset: ByteOffset,
        text: &str,
        now: Timestamp,
    ) -> Result<(), DocumentError> {
        self.apply(Edit::insertion(offset, text), now)
    }

    /// 删除 `range` 区间。
    pub fn remove(&mut self, range: TextRange, now: Timestamp) -> Result<(), DocumentError> {
        self.apply(Edit::deletion(range), now)
    }

    /// 把 `range` 区间替换为 `text`。
    pub fn replace(
        &mut self,
        range: TextRange,
        text: &str,
        now: Timestamp,
    ) -> Result<(), DocumentError> {
        self.apply(Edit::new(range, text), now)
    }

    /// 应用一次原子编辑。
    ///
    /// 顺序：校验 → 记撤销快照 → 改缓冲区 → 移光标 → 置脏 → 自增版本 → 入事件队列。
    /// `edit` 为无效果编辑（空区间 + 空替换）时**直接返回**，不产生撤销点与事件。
    ///
    /// # 为什么先问「会不会合并」再取快照（问题 053）
    ///
    /// [`Self::snapshot_at`] 含一份**整篇文本**（1MB = 一次全文分配 + 复制）。
    /// 而 [`UndoStack::push`] 在合并命中时会把它直接丢弃——连续打字时每个键
    /// 都在合并窗口内，于是**每敲一个键白付一次全文复制**。改用
    /// [`UndoStack::would_merge`] 先判（只需 `(类型, 时刻)`，全是标量）：
    /// 命中走 [`UndoStack::merge`]（滚动时间窗，零分配），未命中才取快照。
    /// 两条分支的可见结果与旧实现逐例一致（判据同源，见 `undo.rs`）。
    pub fn apply(&mut self, edit: Edit, now: Timestamp) -> Result<(), DocumentError> {
        if edit.is_noop() {
            return Ok(());
        }
        self.validate_range(edit.range())?;
        let kind = edit.kind();

        if self.undo.would_merge(kind, now) {
            self.undo.merge(now);
        } else {
            let before = self.snapshot_at(now);
            self.undo.push(before, kind);
        }

        if !edit.range().is_empty() {
            self.buffer.remove(edit.range())?;
        }
        if !edit.replacement().is_empty() {
            self.buffer
                .insert(edit.range().start(), edit.replacement())?;
        }

        self.cursor.move_to(edit.cursor_after());
        self.state.mark_dirty();
        self.revision += 1;
        self.events.push(DocumentEvent::TextModified {
            document_id: self.id,
            range: edit.range(),
            kind,
            revision: self.revision,
        });
        Ok(())
    }

    /// 应用 [`EditPlan`]：执行编辑并把选区设为计划建议的选区。
    pub fn apply_plan(&mut self, plan: &EditPlan, now: Timestamp) -> Result<(), DocumentError> {
        self.apply(plan.edit().clone(), now)?;
        if let Some(selection) = plan.selection() {
            self.cursor = Cursor::with_anchor(selection.end(), selection.start());
            self.push_selection_event();
        }
        Ok(())
    }

    // ---------- 撤销 / 重做（S3-08） ----------

    /// 撤销一步；栈空时返回 [`DocumentError::UndoStackEmpty`]。
    pub fn undo(&mut self, now: Timestamp) -> Result<(), DocumentError> {
        if !self.undo.can_undo() {
            return Err(DocumentError::UndoStackEmpty);
        }
        let current = self.snapshot_at(now);
        let previous = match self.undo.undo(current) {
            Some(previous) => previous,
            None => return Err(DocumentError::UndoStackEmpty),
        };
        self.restore(previous)
    }

    /// 重做一步；栈空时返回 [`DocumentError::RedoStackEmpty`]。
    pub fn redo(&mut self, now: Timestamp) -> Result<(), DocumentError> {
        if !self.undo.can_redo() {
            return Err(DocumentError::RedoStackEmpty);
        }
        let current = self.snapshot_at(now);
        let next = match self.undo.redo(current) {
            Some(next) => next,
            None => return Err(DocumentError::RedoStackEmpty),
        };
        self.restore(next)
    }

    // ---------- 保存 / 关闭 ----------

    /// 标记为「已保存到 `path`」。
    ///
    /// 只更新聚合状态，**不写磁盘**；实际写入由应用层完成。
    /// 未修改时返回 [`DocumentError::NotDirty`]。
    pub fn mark_saved(&mut self, path: FilePath, now: Timestamp) -> Result<(), DocumentError> {
        if !self.state.is_dirty() && self.path.as_ref() == Some(&path) {
            return Err(DocumentError::NotDirty);
        }
        self.path = Some(path.clone());
        self.meta.mark_saved(now);
        self.state.mark_clean();
        self.events.push(DocumentEvent::DocumentSaved {
            document_id: self.id,
            path,
        });
        Ok(())
    }

    /// 标记为已关闭（发出事件，供工作区清理标签页）。
    pub fn mark_closed(&mut self) {
        self.events.push(DocumentEvent::DocumentClosed {
            document_id: self.id,
        });
    }

    /// 生成一次持久化快照（供仓储写入）。
    ///
    /// 尚未保存过（无路径）时返回 `None`——调用方应先走「另存为」取得路径。
    pub fn to_snapshot(&self) -> Option<DocumentSnapshot> {
        self.path.as_ref().map(|path| {
            DocumentSnapshot::new(
                self.id,
                path.clone(),
                self.text(),
                self.meta.last_saved_at(),
            )
        })
    }

    // ---------- 事件 ----------

    /// 取走并清空事件队列。
    pub fn take_events(&mut self) -> Vec<DocumentEvent> {
        std::mem::take(&mut self.events)
    }

    /// 查看事件队列（不取走）。
    pub fn pending_events(&self) -> &[DocumentEvent] {
        &self.events
    }

    // ---------- 内部 ----------

    /// 校验区间是否落在缓冲区内；非法时返回对应错误。
    fn validate_range(&self, range: TextRange) -> Result<(), DocumentError> {
        let len = self.len_bytes();
        let start = range.start().value();
        let end = range.end().value();
        if start > len || end > len {
            return Err(DocumentError::InvalidRange { start, end, len });
        }
        if self
            .buffer
            .char_boundary_floor(ByteOffset::new(start))
            .value()
            != start
        {
            return Err(DocumentError::NotCharBoundary { offset: start });
        }
        if self.buffer.char_boundary_ceil(ByteOffset::new(end)).value() != end {
            return Err(DocumentError::NotCharBoundary { offset: end });
        }
        Ok(())
    }

    /// 生成当前状态的撤销快照。
    fn snapshot_at(&self, now: Timestamp) -> Snapshot {
        Snapshot::new(self.text(), self.cursor.selection(), now)
    }

    /// 用快照整体替换缓冲区并恢复选区，随后自增版本并发出事件。
    fn restore(&mut self, target: Snapshot) -> Result<(), DocumentError> {
        self.replace_whole(target.content())?;
        self.cursor = match target.selection() {
            Some(selection) => Cursor::with_anchor(selection.end(), selection.start()),
            None => Cursor::new(ByteOffset::new(
                self.len_bytes().min(target.content().len()),
            )),
        };
        self.state.mark_dirty();
        self.revision += 1;
        self.events.push(DocumentEvent::TextModified {
            document_id: self.id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(self.len_bytes())),
            kind: super::value_objects::ModificationKind::Replace,
            revision: self.revision,
        });
        Ok(())
    }

    /// 把缓冲区整体替换为 `content`。
    fn replace_whole(&mut self, content: &str) -> Result<(), DocumentError> {
        let end = ByteOffset::new(self.len_bytes());
        if end.value() > 0 {
            self.buffer
                .remove(TextRange::new(ByteOffset::new(0), end))?;
        }
        if !content.is_empty() {
            self.buffer.insert(ByteOffset::new(0), content)?;
        }
        Ok(())
    }

    /// 发出选区变化事件。
    fn push_selection_event(&mut self) {
        self.events.push(DocumentEvent::SelectionChanged {
            document_id: self.id,
            selection: self.cursor.selection(),
        });
    }
}

/// 编译期断言：聚合根可跨线程转移（`Send`），且缓冲区实现可跨线程共享（`Send + Sync`）。
const fn _assert_document_send<T: Send>() {}
const _ASSERT_DOCUMENT_SEND: () = _assert_document_send::<Document>();
