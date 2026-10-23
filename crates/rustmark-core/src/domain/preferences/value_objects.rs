//! Preferences 上下文的值对象（S3-12）。
//!
//! # 关键规则：`KeyCombo` 归一化与冲突判定
//!
//! 原型（`markdown-editor-prototype`）的快捷键存在「四处真相」缺陷：
//! 菜单、命令面板、快捷键一览各有一份匹配逻辑，且匹配层直接拒绝了纯单键
//! （导致 `Escape` / `F1`–`F12` 无法绑定）。本设计把规则收敛为两处纯函数：
//! - [`KeyCombo::parse`] + [`KeyCombo::display`]：**唯一**的字符串 ↔ 组合转换与显示源；
//! - [`ShortcutTable::conflicts`]：**唯一**的冲突判定，且只**报告**不拒绝。
//!
//! # 模块结构
//!
//! 按内聚的属性与行为边界拆为五个私有子模块，本文件作为 facade
//! 全量 re-export，外部路径 `preferences::value_objects::*` 保持不变：
//! - `theme`：主题种类 / 自定义调色板 / lint 开关 / 主题设置；
//! - `editor`：缩进 / 写作目标 / 编辑器设置；
//! - `keys`：修饰键 / 功能键 / 主键 / 组合键（归一化模型）；
//! - `action`：可绑定动作目录；
//! - `shortcut_table`：绑定、冲突模型与默认绑定表。

mod action;
mod editor;
mod keys;
mod shortcut_table;
mod theme;

pub use action::ActionId;
pub(crate) use editor::MAX_OPEN_DOCUMENTS_DEFAULT;
pub use editor::{EditorSettings, MAX_OPEN_DOCUMENTS_MAX, TabSize, WritingGoal};
pub use keys::{Key, KeyCombo, Modifiers, NamedKey};
pub use shortcut_table::{Conflict, ConflictKind, ShortcutBinding, ShortcutTable};
pub use theme::{CustomThemePalette, LintSettings, Theme, ThemeKind};
