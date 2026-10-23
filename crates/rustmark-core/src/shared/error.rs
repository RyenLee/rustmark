//! 全应用统一的错误类型。
//!
//! 分层约定：领域层与应用层用 `thiserror` 定义各自的**具体**错误类型，
//! 再转换为本文件的 [`AppError`]；`anyhow` 仅允许出现在 `app.rs` / `main.rs` 等二进制入口。

use thiserror::Error;

/// 跨层传递的统一错误类型。
///
/// 每个变体的 `Display` 文案可直接呈现给用户，故文案需自解释，
/// 不含仅供开发者理解的缩写。
#[derive(Debug, Error)]
pub enum AppError {
    /// 文件系统或进程外资源读写失败。携带具体描述，不含裸 `io::Error`（保持可序列化与可复制）。
    #[error("I/O 失败：{message}")]
    Io {
        /// 可直接呈现给用户的失败描述。
        message: String,
    },

    /// 领域规则拒绝了该操作（如撤销栈为空、越界编辑）。
    ///
    /// S3-04 起各上下文的具体错误（`DocumentError` 等）会通过 `From` 收敛到本变体。
    #[error("{message}")]
    Domain {
        /// 源错误 `Display` 出来的规则拒绝文案。
        message: String,
    },

    /// Markdown 解析失败。
    #[error("Markdown 解析失败：{message}")]
    Parse {
        /// 解析器给出的失败位置与原因。
        message: String,
    },

    /// 语法高亮失败（如语法集损坏）。
    #[error("语法高亮失败：{message}")]
    Highlight {
        /// 高亮子系统的失败描述。
        message: String,
    },

    /// 导出失败（HTML / PDF / 图片）。
    #[error("导出失败：{message}")]
    Export {
        /// 导出格式与失败原因。
        message: String,
    },

    /// 原生对话框调用失败。
    #[error("对话框调用失败：{message}")]
    Dialog {
        /// 对话框后端的失败描述。
        message: String,
    },

    /// 配置读取、解析或迁移失败。
    #[error("配置错误：{message}")]
    Config {
        /// 配置文件路径与失败原因（如有）。
        message: String,
    },

    /// 用户在对话框中主动取消操作。
    ///
    /// 与「失败」区分：调用方应静默返回，而不是提示错误。
    #[error("操作已取消")]
    Cancelled,
}

impl AppError {
    /// 构造一个 I/O 错误。
    pub fn io(message: impl Into<String>) -> Self {
        Self::Io {
            message: message.into(),
        }
    }

    /// 构造一个领域错误。
    pub fn domain(message: impl Into<String>) -> Self {
        Self::Domain {
            message: message.into(),
        }
    }

    /// 构造一个配置错误。
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
        }
    }

    /// 该错误是否由用户主动取消导致（调用方通常应静默处理）。
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Io {
            message: e.to_string(),
        }
    }
}
