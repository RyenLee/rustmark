//! Export 上下文的错误类型（S3-13 配套）。

use thiserror::Error;

use crate::shared::error::AppError;

use super::value_objects::ExportFormat;

/// 导出失败或选项非法。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// 目标文件扩展名与导出格式不匹配。
    #[error("目标文件扩展名应为 .{expected}，实际为 .{actual}")]
    ExtensionMismatch {
        /// 期望的扩展名。
        expected: &'static str,
        /// 实际的扩展名。
        actual: String,
    },

    /// 该格式不支持目录。
    #[error("{format} 导出不支持目录")]
    TocNotSupported { format: ExportFormat },

    /// 该格式不支持分页。
    #[error("{format} 导出不支持分页")]
    PaginationNotSupported { format: ExportFormat },

    /// PDF 关闭分页（长图模式）时无法生成带页码的目录。
    #[error("PDF 关闭分页后无法生成目录，请开启分页或取消目录")]
    TocRequiresPagination,

    /// 主题名为空。
    #[error("导出主题名不能为空")]
    EmptyTheme,

    /// 目录条目含需转义字符但**未标记** `needs_escape`。
    ///
    /// 触发条件：调用方绕过 [`crate::domain::export::value_objects::TocEntry::new`]
    /// 手工构造条目，导致导出器漏转义（原型缺陷 2.4 的兜底防线）。
    #[error("目录条目「{title}」含需转义字符但未标记 needs_escape")]
    UnescapedTocEntry { title: String },

    /// 外部资源不可用（打印服务缺失、磁盘不可写等）。
    #[error("无法完成导出：{message}")]
    IoUnavailable { message: String },
}

impl ExportError {
    /// 构造一个「外部资源不可用」错误。
    pub fn io_unavailable(message: impl Into<String>) -> Self {
        Self::IoUnavailable {
            message: message.into(),
        }
    }

    /// 是否为「选项非法」（而非环境故障）：前者应在 UI 层提前拦截。
    pub const fn is_configuration_error(&self) -> bool {
        !matches!(self, Self::IoUnavailable { .. })
    }
}

impl From<ExportError> for AppError {
    fn from(e: ExportError) -> Self {
        match e {
            ExportError::IoUnavailable { message } => Self::Io { message },
            other => Self::Export {
                message: other.to_string(),
            },
        }
    }
}
