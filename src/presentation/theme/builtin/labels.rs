//! 主题种类的**界面专属**数据（显示名）。
//!
//! # 为什么这些不在枚举上
//!
//! 主题种类的**真源**是 [`crate::shared::theme::ThemeKind`]（领域 / 应用 /
//! 表现三方共用）。而「中文显示名」是**纯 UI 私货**——领域层不需要它，
//! 把它塞进枚举就等于让三方共用的真源携带一层只有表现层在意的数据。
//!
//! 这与 R-07 的处理同构：导出格式的 `pdf_caveat()` 等界面专属数据
//! 从枚举上拆到独立函数表。写成**独立函数**而非「枚举 + 同构表」，
//! 是为了不引入第二份「随枚举一起改」的清单——那种清单正是本次
//! 改造要消灭的东西。
//!
//! # 为什么不叫 `ThemeKindLabel`
//!
//! 它不是「种类的标签」，而是「这个种类在界面上怎么称呼」。
//! 命名上刻意与 [`ThemeKind`] 区分开，避免后人以为二者必须一一对应
//! （将来加一种「自动（按时间）」主题时，显示名可能需要特殊措辞）。

use crate::shared::theme::ThemeKind;

/// 主题种类在设置页 / 状态栏 / 关于面板里的中文显示名。
pub const fn theme_kind_label(kind: ThemeKind) -> &'static str {
    match kind {
        ThemeKind::Light => "浅色",
        ThemeKind::Dark => "深色",
        ThemeKind::System => "跟随系统",
        ThemeKind::Custom => "自定义",
    }
}
