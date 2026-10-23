//! Markdown 上下文的错误类型（S3-10 配套）。
//!
//! 解析器与高亮器运行在**基础设施层**（`pulldown-cmark` / `syntect`），
//! 但其错误类型定义在领域层，以便应用层与表现层不必依赖外部库。

use thiserror::Error;

use crate::shared::error::AppError;

/// Markdown 解析失败。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("Markdown 解析失败：{message}")]
pub struct ParseError {
    /// 可直接呈现给用户的原因描述。
    pub message: String,
}

impl ParseError {
    /// 构造解析错误。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<ParseError> for AppError {
    fn from(e: ParseError) -> Self {
        Self::Parse { message: e.message }
    }
}

/// 语法高亮失败（语法集损坏、语言不受支持等）。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("语法高亮失败：{message}")]
pub struct HighlightError {
    /// 可直接呈现给用户的原因描述。
    pub message: String,
}

impl HighlightError {
    /// 构造高亮错误。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<HighlightError> for AppError {
    fn from(e: HighlightError) -> Self {
        Self::Highlight { message: e.message }
    }
}

/// 拼写检查失败（词典资源缺失或损坏）。
///
/// 拼写检查是**可选**的增值能力：词典出问题时应降级为「不做检查」而非让编辑器报错，
/// 故本错误**刻意不 impl `From<SpellCheckError> for AppError`**——应用层选择忽略并返回空标记集。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("拼写检查失败：{message}")]
pub struct SpellCheckError {
    /// 可直接呈现给用户的原因描述。
    pub message: String,
}

impl SpellCheckError {
    /// 构造拼写检查错误。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
