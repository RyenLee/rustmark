//! Preferences 用例查询（阶段 04 · S4-08）。
//!
//! 查询与命令命名不冲突（动名词区分）。三处消费方（设置页、菜单、命令面板）共用
//! [`GetSettingsQuery`] 返回的同一份 [`SettingsView`]，避免「四处真相」缺陷。

/// 获取全部设置（主题 + 编辑器 + 快捷键一览）。
#[derive(Clone, Debug, Default)]
pub struct GetSettingsQuery;

/// 获取当前快捷键冲突列表。
#[derive(Clone, Debug, Default)]
pub struct GetConflictsQuery;
