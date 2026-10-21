//! Document 上下文的领域事件（S3-03）。
//!
//! 事件是**通知**而非命令（`docs/architecture-constraints.md`）：只携带 ID 与标量，
//! 不携带聚合引用；订阅者如需详情，应凭 ID 向应用层查询。

use crate::shared::event::DomainEvent;
use crate::shared::geometry::TextRange;
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;

use super::value_objects::ModificationKind;

/// Document 上下文的领域事件（命名一律过去式，`.agent/conventions.md`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DocumentEvent {
    /// 正文被修改。
    ///
    /// `range` 是**编辑前**坐标系下的受影响区间；订阅者据此增量刷新高亮与预览。
    /// `revision` 单调递增，订阅者可据此丢弃过期结果。
    TextModified {
        /// 文档 ID。
        document_id: DocumentId,
        /// 受影响区间（编辑前坐标）。
        range: TextRange,
        /// 修改类型。
        kind: ModificationKind,
        /// 修改后文档的版本号。
        revision: u64,
    },

    /// 文档已保存到磁盘。
    DocumentSaved {
        /// 文档 ID。
        document_id: DocumentId,
        /// 保存目标路径。
        path: FilePath,
    },

    /// 文档被关闭。
    DocumentClosed {
        /// 文档 ID。
        document_id: DocumentId,
    },

    /// 光标或选区发生变化。
    SelectionChanged {
        /// 文档 ID。
        document_id: DocumentId,
        /// 新的选区；`None` 表示仅有光标、无选区。
        selection: Option<TextRange>,
    },
}

impl DocumentEvent {
    /// 事件涉及的文档 ID。
    pub const fn document_id(&self) -> DocumentId {
        match self {
            Self::TextModified { document_id, .. }
            | Self::DocumentSaved { document_id, .. }
            | Self::DocumentClosed { document_id }
            | Self::SelectionChanged { document_id, .. } => *document_id,
        }
    }

    /// 该事件是否表示文档内容发生了变化（订阅者可据此决定是否重解析）。
    pub const fn changes_content(&self) -> bool {
        matches!(self, Self::TextModified { .. })
    }
}

impl From<DocumentEvent> for DomainEvent {
    fn from(e: DocumentEvent) -> Self {
        match e {
            DocumentEvent::TextModified {
                document_id,
                revision,
                ..
            } => Self::TextModified {
                document_id,
                revision,
            },
            DocumentEvent::DocumentSaved { document_id, .. } => Self::DocumentSaved(document_id),
            DocumentEvent::DocumentClosed { document_id } => Self::DocumentClosed(document_id),
            DocumentEvent::SelectionChanged {
                document_id,
                selection,
            } => Self::SelectionChanged {
                document_id,
                selection,
            },
        }
    }
}
