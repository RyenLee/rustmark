//! 统一的结果类型别名。
//!
//! 全应用（除二进制入口）统一使用 [`AppResult`]，避免多层错误类型互相嵌套。

use crate::shared::error::AppError;

/// 以 [`AppError`] 为错误类型的结果别名。
pub type AppResult<T> = Result<T, AppError>;
