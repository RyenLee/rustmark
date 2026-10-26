//! TOML 偏好设置仓储（S5-06）。
//!
//! # 与领域聚合的关系：直接序列化聚合，而不是另建持久化模型
//!
//! `Preferences` 已派生 `Serialize / Deserialize` 且自带 `schema_version`，
//! 故本仓储直接存聚合。这样做的**前提**是领域层不因持久化而扭曲形状——
//! 事实也正是如此：聚合的四个字段都是值对象，没有 `Arc`、没有函数指针、
//! 没有需要跳过的字段。若将来聚合引入了无法序列化的运行时状态
//! （如缓存句柄），就应改为「持久化模型 ↔ 聚合」的显式转换。
//!
//! # 损坏与版本不匹配的处理（ADR-003）
//!
//! 策略统一为「**备份原文件 + 回退默认**」，理由与
//! [`super::recent_files_store`] 相同：配置文件损坏绝不能让编辑器打不开。
//! 二者的差别只在「回退到什么」——本仓储回退到 [`Preferences::default_preferences`]。
//!
//! # 未知字段：忽略而非报错
//!
//! 用户可能手工编辑配置文件，或从新版降级。若遇到未知字段就拒绝加载，
//! 会让「装了新版本又换回旧版」的用户丢掉全部配置。故 serde 一律
//! **不启用** `deny_unknown_fields`。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rustmark_core::domain::preferences::aggregate::{PREFERENCES_SCHEMA_VERSION, Preferences};
use rustmark_core::domain::preferences::repository::PreferencesRepository;
use rustmark_core::shared::error::AppError;

/// TOML 偏好设置仓储。
#[derive(Clone, Debug)]
pub struct TomlPreferencesRepository {
    file: PathBuf,
}

impl TomlPreferencesRepository {
    /// 以指定文件路径构造。
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// 配置文件路径。
    pub fn path(&self) -> &Path {
        &self.file
    }

    /// 备份损坏 / 版本不兼容的配置文件，返回备份路径。
    fn backup(&self, reason: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let name = self
            .file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("settings.toml");
        let backup = self.file.with_file_name(format!("{name}.bak-{stamp}"));
        if let Ok(bytes) = fs::read(&self.file) {
            let _ = super::recent_files_store::atomic_write(&backup, &bytes);
        }
        log::warn!(
            "配置文件 {} 处理（{reason}），已备份到 {}",
            self.file.display(),
            backup.display()
        );
        backup
    }
}

impl PreferencesRepository for TomlPreferencesRepository {
    fn load(&self) -> Result<Preferences, AppError> {
        // 文件不存在 = 首次启动，返回默认配置而**不报错**。
        if !self.file.exists() {
            return Ok(Preferences::default_preferences());
        }
        let raw = fs::read_to_string(&self.file).map_err(|e| {
            AppError::config(format!("无法读取配置文件 {}：{e}", self.file.display()))
        })?;
        if raw.trim().is_empty() {
            return Ok(Preferences::default_preferences());
        }

        // 解析失败（含非法取值）：备份 + 回退默认。
        let parsed: Preferences = match toml::from_str(&raw) {
            Ok(prefs) => prefs,
            Err(e) => {
                self.backup(&format!("解析失败：{e}"));
                return Ok(Preferences::default_preferences());
            }
        };

        // 版本高于当前实现：说明用户从新版降级或文件被外部改写。
        // 保留配置比丢弃更可能符合用户意图，故备份后**尝试**沿用，
        // 只有在结构确实不兼容时才回退默认。
        if parsed.schema_version() > PREFERENCES_SCHEMA_VERSION {
            log::warn!(
                "配置文件版本 {} 高于当前支持的 {}",
                parsed.schema_version(),
                PREFERENCES_SCHEMA_VERSION
            );
            self.backup("版本高于当前支持");
        }

        // 聚合自身的取值校验（字号 / 行高等）——反序列化成功不代表取值合法。
        if let Err(e) = parsed.validate() {
            self.backup(&format!("取值非法：{e}"));
            return Ok(Preferences::default_preferences());
        }
        Ok(parsed)
    }

    fn save(&self, preferences: &Preferences) -> Result<(), AppError> {
        // 写前校验：非法的配置不应落盘，否则下次启动又要走「备份 + 回退」，
        // 白白产生备份文件并丢失用户其它配置。
        preferences
            .validate()
            .map_err(|e| AppError::config(format!("拒绝保存非法的偏好设置：{e}")))?;
        let body = toml::to_string_pretty(preferences)
            .map_err(|e| AppError::config(format!("序列化偏好设置失败：{e}")))?;
        super::recent_files_store::atomic_write(&self.file, body.as_bytes())
            .map_err(|e| AppError::config(e.to_string()))
    }

    fn reset(&self) -> Result<Preferences, AppError> {
        let defaults = Preferences::default_preferences();
        // 「恢复出厂设置」的语义是「配置回到默认值」，
        // 因此重写一份默认配置而非删除文件——避免用户困惑于「文件怎么不见了」。
        self.save(&defaults)?;
        Ok(defaults)
    }
}
