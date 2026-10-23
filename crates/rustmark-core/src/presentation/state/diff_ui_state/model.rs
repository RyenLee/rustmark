//! Diff 数据模型：行级 / 字符级差异类型、渲染行与降级阈值常量。

/// 触发降级的最大比较规模（行数乘积）。
///
/// 与原型 `lib/diff.ts` 的 `MAX_CELLS` 取同值：约 1400×1400 行。
/// 再大就该换 Myers 算法而不是继续等。
pub const MAX_CELLS: usize = 2_000_000;

/// 行级差异类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffOp {
    /// 两侧相同。
    Equal,
    /// 仅右侧（当前文档）有。
    Add,
    /// 仅左侧（对照文件）有。
    Remove,
}

/// 一行差异结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiffRow {
    /// 差异类型。
    pub op: DiffOp,
    /// 行文本。
    pub text: String,
    /// 左侧行号（1-based；`Add` 行为 `None`）。
    pub old_no: Option<usize>,
    /// 右侧行号（1-based；`Remove` 行为 `None`）。
    pub new_no: Option<usize>,
}

impl DiffRow {
    /// 构造相等行。
    pub(super) fn equal(text: &str, old_no: usize, new_no: usize) -> Self {
        Self {
            op: DiffOp::Equal,
            text: text.to_string(),
            old_no: Some(old_no),
            new_no: Some(new_no),
        }
    }

    /// 构造删除行（仅左侧有）。
    pub(super) fn remove(text: &str, old_no: usize) -> Self {
        Self {
            op: DiffOp::Remove,
            text: text.to_string(),
            old_no: Some(old_no),
            new_no: None,
        }
    }

    /// 构造新增行（仅右侧有）。
    pub(super) fn add(text: &str, new_no: usize) -> Self {
        Self {
            op: DiffOp::Add,
            text: text.to_string(),
            old_no: None,
            new_no: Some(new_no),
        }
    }
}

/// 增删行数统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DiffStat {
    /// 新增行数。
    pub added: usize,
    /// 删除行数。
    pub removed: usize,
}

/// 字符级差异段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharSegKind {
    /// 两侧共有。
    Equal,
    /// 仅新串有。
    Add,
    /// 仅旧串有。
    Remove,
}

/// 一段字符级差异。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CharSeg {
    /// 段类型。
    pub kind: CharSegKind,
    /// 段文本。
    pub text: String,
}

impl CharSeg {
    pub(super) fn new(kind: CharSegKind, text: &str) -> Self {
        Self {
            kind,
            text: text.to_string(),
        }
    }
}

/// 渲染用的一行：把紧邻的删除 + 新增合并成「替换」，并带上字符级差异。
///
/// 与原型 `DiffDialog.vue` 的 `DisplayRow` 同构。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DisplayRow {
    /// 两侧相同。
    Equal {
        /// 行文本。
        text: String,
        /// 左侧行号。
        old_no: usize,
        /// 右侧行号。
        new_no: usize,
    },
    /// 删除行。
    Remove {
        /// 行文本。
        text: String,
        /// 左侧行号。
        old_no: usize,
    },
    /// 新增行。
    Add {
        /// 行文本。
        text: String,
        /// 右侧行号。
        new_no: usize,
    },
    /// 替换行：同一位置上的旧行 → 新行，逐字符标出变化。
    Replace {
        /// 旧行文本。
        old_text: String,
        /// 新行文本。
        new_text: String,
        /// 左侧行号。
        old_no: usize,
        /// 右侧行号。
        new_no: usize,
        /// 旧行的字符级差异段。
        old_segs: Vec<CharSeg>,
        /// 新行的字符级差异段。
        new_segs: Vec<CharSeg>,
    },
}

impl DisplayRow {
    /// 该行属于哪一侧有 / 两侧都有的哪一类（只关心「是不是新增」）。
    pub const fn is_add(&self) -> bool {
        matches!(self, Self::Add { .. } | Self::Replace { .. })
    }

    /// 该行是否含删除内容。
    pub const fn is_remove(&self) -> bool {
        matches!(self, Self::Remove { .. } | Self::Replace { .. })
    }

    /// 左侧行号（若有）。
    pub fn old_no(&self) -> Option<usize> {
        match self {
            Self::Equal { old_no, .. }
            | Self::Remove { old_no, .. }
            | Self::Replace { old_no, .. } => Some(*old_no),
            Self::Add { .. } => None,
        }
    }

    /// 右侧行号（若有）。
    pub fn new_no(&self) -> Option<usize> {
        match self {
            Self::Equal { new_no, .. }
            | Self::Add { new_no, .. }
            | Self::Replace { new_no, .. } => Some(*new_no),
            Self::Remove { .. } => None,
        }
    }
}
