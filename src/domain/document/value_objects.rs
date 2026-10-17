//! Document 上下文的值对象（S3-01）。
//!
//! 全部字段私有、通过访问器暴露（`.agent/conventions.md` 第 10 条）。
//! 值对象不持有任何 IO 或 UI 状态。

use crate::shared::geometry::{ByteOffset, TextRange};
use crate::shared::path::FilePath;
use crate::shared::time::Timestamp;

/// 光标与选区。
///
/// `position` 是**移动的一端**（光标所在处），`anchor` 是选区的固定端。
/// `anchor == None` 表示无选区（光标为插入点）。
///
/// # 可变 vs 不可变
///
/// 提供两套方法：
/// - `move_to` / `select_to` / `clear_selection` 为 `&mut self`，供聚合根原地更新；
/// - `with_position` / `with_selection` 返回新值，供纯函数与测试构造使用。
///
/// 理由：光标是聚合根的**内部状态**，其生命周期与聚合一致，原地改写避免了
/// 「每次移动都复制一次聚合」的开销；而纯函数路径需要值语义以返回结果。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct Cursor {
    position: ByteOffset,
    anchor: Option<ByteOffset>,
}

impl Cursor {
    /// 构造一个位于 `position` 的无选区光标。
    pub fn new(position: ByteOffset) -> Self {
        Self {
            position,
            anchor: None,
        }
    }

    /// 构造一个带选区的光标（`anchor` 为固定端，`position` 为移动端）。
    pub fn with_anchor(position: ByteOffset, anchor: ByteOffset) -> Self {
        Self {
            position,
            anchor: Some(anchor),
        }
    }

    /// 光标位置。
    pub const fn position(self) -> ByteOffset {
        self.position
    }

    /// 选区固定端；无选区时为 `None`。
    pub const fn anchor(self) -> Option<ByteOffset> {
        self.anchor
    }

    /// 将光标移动到 `position`，并**清除选区**。
    pub fn move_to(&mut self, position: ByteOffset) {
        self.position = position;
        self.anchor = None;
    }

    /// 从当前锚点（或当前位置，若无锚点）拖选到 `position`。
    ///
    /// 语义：无选区时以当前 `position` 作为锚点开始选择；
    /// `position` 与锚点相同时**取消选区**（拖回原点等于没选）。
    pub fn select_to(&mut self, position: ByteOffset) {
        let anchor = self.anchor.unwrap_or(self.position);
        self.position = position;
        self.anchor = if position == anchor {
            None
        } else {
            Some(anchor)
        };
    }

    /// 清除选区，光标停在 `position`。
    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// 是否存在非空选区。
    pub fn has_selection(self) -> bool {
        match self.anchor {
            Some(anchor) => anchor != self.position,
            None => false,
        }
    }

    /// 选区区间（已归一化，起点 ≤ 终点）；无选区时为 `None`。
    pub fn selection(self) -> Option<TextRange> {
        self.anchor
            .filter(|_| self.has_selection())
            .map(|anchor| TextRange::new(anchor, self.position))
    }

    /// 返回移动到 `position` 后的新光标（原值不变）。
    pub fn with_position(self, position: ByteOffset) -> Self {
        Self::new(position)
    }
}

/// 一次文本修改的类型。
///
/// 用于撤销合并判定：只有**同种类型**的连续修改才会被合并为一个撤销点。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModificationKind {
    /// 纯插入。
    Insert,
    /// 纯删除。
    Remove,
    /// 替换（删除 + 插入）。
    Replace,
}

impl ModificationKind {
    /// 是否可与另一次修改合并为同一个撤销点。
    ///
    /// 只有连续插入会合并——删除与替换各自独立成撤销点，避免一次撤销吃掉大段内容。
    pub fn mergeable_with(self, other: Self) -> bool {
        matches!((self, other), (Self::Insert, Self::Insert))
    }
}

/// 文档元数据。
///
/// 只记录**与文档身份有关**的信息；滚动位置、折叠状态等 UI 状态不在此处
/// （`.agent/conventions.md` 第 30 条）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DocumentMeta {
    title: String,
    created_at: Timestamp,
    last_saved_at: Option<Timestamp>,
}

impl DocumentMeta {
    /// 构造一个「未命名」文档的元数据。
    pub fn new_untitled(created_at: Timestamp) -> Self {
        Self {
            title: "未命名".to_string(),
            created_at,
            last_saved_at: None,
        }
    }

    /// 按文件路径构造元数据（标题取文件名去掉扩展名）。
    pub fn from_path(path: &FilePath, created_at: Timestamp) -> Self {
        let name = path.file_name();
        let title = match name.rfind('.') {
            Some(dot) if dot > 0 => name[..dot].to_string(),
            _ => name.to_string(),
        };
        Self {
            title,
            created_at,
            last_saved_at: None,
        }
    }

    /// 标题。
    pub fn title(&self) -> &str {
        &self.title
    }

    /// 重命名（不触发保存状态变化）。
    pub fn rename(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    /// 创建时间。
    pub const fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// 最近一次保存时间；从未保存过为 `None`。
    pub const fn last_saved_at(&self) -> Option<Timestamp> {
        self.last_saved_at
    }

    /// 标记为「已在 `at` 时刻保存」。
    pub fn mark_saved(&mut self, at: Timestamp) {
        self.last_saved_at = Some(at);
    }
}

/// 文档的修改状态。
///
/// 语义：`Clean` 表示缓冲区内容与磁盘一致；任何编辑命令都会置为 `Dirty`。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct ModificationState {
    dirty: bool,
}

impl ModificationState {
    /// 未修改状态。
    pub const fn clean() -> Self {
        Self { dirty: false }
    }

    /// 已修改状态。
    pub const fn dirty() -> Self {
        Self { dirty: true }
    }

    /// 是否存在未保存的修改。
    pub const fn is_dirty(self) -> bool {
        self.dirty
    }

    /// 是否需要保存（与 [`ModificationState::is_dirty`] 同义，语义更贴近调用点）。
    pub const fn needs_save(self) -> bool {
        self.dirty
    }

    /// 置为已修改。
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// 置为未修改。
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }
}

/// 一次原子编辑：把 `range` 区间的内容替换为 `replacement`。
///
/// 插入 = 空区间 + 非空替换；删除 = 非空区间 + 空替换。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Edit {
    range: TextRange,
    replacement: String,
}

impl Edit {
    /// 构造替换编辑。
    pub fn new(range: TextRange, replacement: impl Into<String>) -> Self {
        Self {
            range,
            replacement: replacement.into(),
        }
    }

    /// 构造插入编辑（在 `at` 处插入 `text`）。
    pub fn insertion(at: ByteOffset, text: impl Into<String>) -> Self {
        Self {
            range: TextRange::empty(at),
            replacement: text.into(),
        }
    }

    /// 构造删除编辑。
    pub fn deletion(range: TextRange) -> Self {
        Self {
            range,
            replacement: String::new(),
        }
    }

    /// 被替换的区间。
    pub const fn range(&self) -> TextRange {
        self.range
    }

    /// 替换内容。
    pub fn replacement(&self) -> &str {
        &self.replacement
    }

    /// 是否为无效果编辑（空区间且空替换）。
    ///
    /// 调用方可据此跳过「入撤销栈 / 发事件」，避免无意义的撤销点。
    pub fn is_noop(&self) -> bool {
        self.range.is_empty() && self.replacement.is_empty()
    }

    /// 该编辑的类型（供撤销合并判定使用）。
    pub fn kind(&self) -> ModificationKind {
        match (self.range.is_empty(), self.replacement.is_empty()) {
            (true, true) => ModificationKind::Insert,
            (true, false) => ModificationKind::Insert,
            (false, true) => ModificationKind::Remove,
            (false, false) => ModificationKind::Replace,
        }
    }

    /// 编辑完成后光标应处的位置（替换内容末尾）。
    pub fn cursor_after(&self) -> ByteOffset {
        ByteOffset::new(self.range.start().value() + self.replacement.len())
    }

    /// 把编辑应用到一段纯文本，返回新文本。
    ///
    /// 越界时返回 [`crate::domain::document::errors::DocumentError::InvalidRange`]，不 panic。
    pub fn apply(
        &self,
        text: &str,
    ) -> Result<String, crate::domain::document::errors::DocumentError> {
        let start = self.range.start().value();
        let end = self.range.end().value();
        if end > text.len() || start > end {
            return Err(
                crate::domain::document::errors::DocumentError::InvalidRange {
                    start,
                    end,
                    len: text.len(),
                },
            );
        }
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Err(
                crate::domain::document::errors::DocumentError::NotCharBoundary {
                    offset: if text.is_char_boundary(start) {
                        end
                    } else {
                        start
                    },
                },
            );
        }
        let mut out = String::with_capacity(text.len() - (end - start) + self.replacement.len());
        out.push_str(&text[..start]);
        out.push_str(&self.replacement);
        out.push_str(&text[end..]);
        Ok(out)
    }
}

/// 编辑计划：一次编辑 + 编辑完成后**建议的选区**。
///
/// 单独成值对象的原因：`wrap_selection` 等操作完成后需要把选区落在被包裹的内容上，
/// 若只返回 `Edit`，表现层需要自行重算选区，容易产生第二处真相。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EditPlan {
    edit: Edit,
    selection: Option<TextRange>,
}

impl EditPlan {
    /// 由编辑与选区构造。
    pub fn new(edit: Edit, selection: Option<TextRange>) -> Self {
        Self { edit, selection }
    }

    /// 只做编辑、不选区。
    pub fn plain(edit: Edit) -> Self {
        Self {
            edit,
            selection: None,
        }
    }

    /// 待执行的编辑。
    pub const fn edit(&self) -> &Edit {
        &self.edit
    }

    /// 编辑完成后建议的选区；`None` 表示光标落在编辑末尾、不选中。
    pub const fn selection(&self) -> Option<TextRange> {
        self.selection
    }
}
