//! Preferences 上下文的仓储 trait。
//!
//! 由 `infrastructure::persistence::TomlPreferencesRepository` 实现（S5-05）。
//! 配置文件必须带 **schema 版本**（ADR-003），读取时按版本迁移，未知版本报错而非静默丢弃。

use crate::shared::error::AppError;

use super::aggregate::Preferences;

/// 偏好设置仓储。
pub trait PreferencesRepository: Send + Sync {
    /// 读取配置；不存在时返回**默认配置**（不报错）。
    fn load(&self) -> Result<Preferences, AppError>;

    /// 原子写入配置（临时文件 + rename，同 `FR-04`）。
    fn save(&self, preferences: &Preferences) -> Result<(), AppError>;

    /// 恢复出厂设置（删除配置文件并重建默认配置）。
    fn reset(&self) -> Result<Preferences, AppError>;
}
