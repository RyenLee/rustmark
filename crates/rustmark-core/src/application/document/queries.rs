//! 文档用例查询（阶段 04 · S4-03）。
//!
//! 查询与命令命名不冲突（动名词区分）；`GetContentQuery` 支持 `range = None` 取全文。

use crate::shared::geometry::TextRange;
use crate::shared::ids::DocumentId;

/// 获取单个文档视图。
#[derive(Clone, Debug)]
pub struct GetDocumentQuery {
    /// 文档 ID。
    pub id: DocumentId,
}

/// 获取文档内容，`range = None` 取全文。
#[derive(Clone, Debug)]
pub struct GetContentQuery {
    /// 文档 ID。
    pub id: DocumentId,
    /// 取内容区间；`None` 表示全文。
    pub range: Option<TextRange>,
}

/// 获取字数统计。
#[derive(Clone, Debug)]
pub struct GetStatsQuery {
    /// 文档 ID。
    pub id: DocumentId,
}

/// 获取当前选区（无选区返回 `None`）。
#[derive(Clone, Debug)]
pub struct GetSelectionQuery {
    /// 文档 ID。
    pub id: DocumentId,
}

/// 列出全部已打开文档。
#[derive(Clone, Debug, Default)]
pub struct ListOpenDocumentsQuery;
