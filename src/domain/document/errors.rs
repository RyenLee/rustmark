//! Document 上下文的错误类型（S3-04）。
//!
//! 领域错误一律是 `thiserror` **具体枚举**（`.agent/conventions.md` 第 13 条），
//! 禁止 `anyhow`、`Box<dyn Error>`；每个变体的 `Display` 文案可直接呈现给用户，
//! 故不含仅供开发者理解的缩写。

use thiserror::Error;

use crate::shared::error::AppError;
use crate::shared::ids::DocumentId;

/// 文本缓冲区的底层错误。
///
/// 由 [`crate::domain::document::services::TextBuffer`] 的实现返回；
/// 经 `From` 收敛为 [`DocumentError`] 的对应变体后向上传播。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TextBufferError {
    /// 偏移超出缓冲区长度。
    ///
    /// 触发条件：`offset > len_bytes()`（**等于长度合法**，表示「追加到末尾」）。
    #[error("偏移 {offset} 越界，缓冲区长度 {len} 字节")]
    OutOfRange { offset: usize, len: usize },

    /// 区间终点超出缓冲区长度，或起点大于终点。
    #[error("区间 [{start}, {end}) 非法，缓冲区长度 {len} 字节")]
    InvalidRange {
        start: usize,
        end: usize,
        len: usize,
    },

    /// 偏移落在 UTF-8 字符的中间字节上。
    ///
    /// 触发条件：按字节切分中文 / 全角字符（`FR-05` IME 输入场景）时未做边界对齐。
    #[error("偏移 {offset} 不在 UTF-8 字符边界上")]
    NotCharBoundary { offset: usize },
}

/// Document 上下文的领域错误。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DocumentError {
    /// 编辑偏移超出文档长度。
    ///
    /// 触发条件：`insert` / `remove` 的偏移或区间越界。
    #[error("偏移 {offset} 超出文档长度 {len}")]
    BufferOutOfRange { offset: usize, len: usize },

    /// 编辑区间非法（终点越界或起点大于终点）。
    #[error("区间 [{start}, {end}) 非法，文档长度 {len}")]
    InvalidRange {
        start: usize,
        end: usize,
        len: usize,
    },

    /// 偏移不在 UTF-8 字符边界上，继续执行会产生乱码。
    #[error("偏移 {offset} 不在 UTF-8 字符边界上")]
    NotCharBoundary { offset: usize },

    /// 试图保存一个未修改的文档。
    ///
    /// 调用方应先检查 [`crate::domain::document::aggregate::Document::is_dirty`]。
    #[error("文档没有未保存的修改")]
    NotDirty,

    /// 撤销栈为空。
    #[error("没有可撤销的操作")]
    UndoStackEmpty,

    /// 重做栈为空。
    #[error("没有可重做的操作")]
    RedoStackEmpty,

    /// 仓储中不存在指定 ID 的文档。
    #[error("找不到 ID 为 {0} 的文档")]
    NotFound(DocumentId),

    /// 需要访问外部资源（磁盘等）但该能力不可用。
    ///
    /// 领域层本身**不产生**此错误（零 IO），该变体用于承载基础设施层错误的语义分类，
    /// 便于表现层区分「规则拒绝」与「环境不可用」。
    #[error("无法访问文件：{message}")]
    IoUnavailable { message: String },
}

impl DocumentError {
    /// 构造一个「外部资源不可用」错误。
    pub fn io_unavailable(message: impl Into<String>) -> Self {
        Self::IoUnavailable {
            message: message.into(),
        }
    }
}

impl From<TextBufferError> for DocumentError {
    fn from(e: TextBufferError) -> Self {
        match e {
            TextBufferError::OutOfRange { offset, len } => Self::BufferOutOfRange { offset, len },
            TextBufferError::InvalidRange { start, end, len } => {
                Self::InvalidRange { start, end, len }
            }
            TextBufferError::NotCharBoundary { offset } => Self::NotCharBoundary { offset },
        }
    }
}

impl From<DocumentError> for AppError {
    fn from(e: DocumentError) -> Self {
        Self::Domain {
            message: e.to_string(),
        }
    }
}
