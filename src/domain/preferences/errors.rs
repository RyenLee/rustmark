//! Preferences 上下文的错误类型。

use thiserror::Error;

use crate::shared::error::AppError;

/// 偏好设置相关的错误。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PreferencesError {
    /// 快捷键组合字符串无法解析。
    ///
    /// 触发条件：空串、缺少主键、含多个主键、主键名无法识别。
    #[error("无法识别的快捷键：{raw}")]
    InvalidCombo { raw: String },

    /// 设置项取值非法。
    ///
    /// 触发条件：字号 / 行高越界、字体族为空、自动保存间隔为 0。
    #[error("设置项 {field} 取值非法：{reason}")]
    InvalidSetting { field: String, reason: String },
}

impl PreferencesError {
    /// 构造一个设置项错误。
    pub fn invalid_setting(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidSetting {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

impl From<PreferencesError> for AppError {
    fn from(e: PreferencesError) -> Self {
        Self::Config {
            message: e.to_string(),
        }
    }
}
