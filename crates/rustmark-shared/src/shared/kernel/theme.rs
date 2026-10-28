//! 主题种类（共享内核；R-09）。
//!
//! # 为什么单独一个模块
//!
//! 「用户选的是哪种主题」这条事实原先散落**六处**，各写一份：
//!
//! - `domain::preferences::value_objects::ThemeKind`（领域枚举，4 变体，
//!   含 `Custom`）；
//! - `presentation::theme::builtin::BuiltinTheme`（表现层副本，**3** 变体，
//!   无 `Custom`）——**R-09 已删除**；
//! - `app::frame::actions::open_overlay` 里的**第三份**手写 `match`
//!   （把主题翻译成导出用的字符串）；
//! - `application::preferences::service::to_theme` 里的**第四份**手写
//!   字符串 → 枚举 `match`；
//! - `presentation::views::settings_view::THEME_KINDS` 的**第五份**常量表
//!   （字符串 + 显示名的成对清单）；
//! - `presentation::views::settings_view::about::live_preview` 里的
//!   **第六份**手写 `match`。
//!
//! 六份副本必然漂移。最典型的一次已经发生：表现层副本 `BuiltinTheme`
//! **没有** `Custom` 变体，于是「自定义主题」在表现层无法表达——
//! 走 `Theme::from_custom_palette` 绕开，而那条路径又**不复用**
//! `BuiltinTheme::parse` 的容错逻辑。
//!
//! 本模块是这条事实的**唯一**出处：新增一种主题种类，只改这里。
//!
//! # 为什么放在 shared 而非 domain
//!
//! 表现层**不得**依赖领域（`docs/spec/architecture-constraints.md` §4，
//! `check-arch` 的 `PRESENTATION_FORBIDDEN` 拦 `crate::domain`），
//! 而主题种类是三方共同需要的：
//!
//! - 领域层：持久化 `Theme`（`kind` 字段的取值）；
//! - 应用层：DTO ↔ 领域转换（`ThemeView.kind` 是 `String`）；
//! - 表现层：设置页选项、主题解析、关于面板显示名。
//!
//! 三方共同的依赖点只有 `shared`——与 [`crate::shared::export_format`] /
//! [`crate::shared::text_format`] 同一模式（R-07 已开此先例）。
//!
//! # 变体语义
//!
//! - `Light` / `Dark`：两套内置配色；
//! - `System`：跟随系统（**不含配色**，由系统偏好决定落到 light 还是 dark）；
//! - `Custom`：用户自定义调色板（**不走内置配色表**）。
//!
//! 注意 `System` 与 `Custom` **不是**「第三、第四套配色」，
//! 而是「配色从哪来」的两种不同回答——故它们是**种类**而非**配色**。

use serde::{Deserialize, Serialize};

/// 主题种类（**唯一**真源）。
///
/// 持久化字符串固定为 `light` / `dark` / `system` / `custom`
/// （`serde(rename_all = "snake_case")`）——**改这些字符串会让用户配置失效**。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeKind {
    /// 浅色。
    Light,
    /// 深色。
    Dark,
    /// 跟随系统（默认）。
    #[default]
    System,
    /// 自定义（由调色板决定配色）。
    Custom,
}

impl ThemeKind {
    /// 全部取值（遍历用）。
    pub const ALL: &'static [Self] = &[Self::Light, Self::Dark, Self::System, Self::Custom];

    /// 规范字符串（持久化契约）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
            Self::Custom => "custom",
        }
    }

    /// 由持久化字符串解析（**穷举**，漏加变体时编译失败）。
    ///
    /// 大小写不敏感、忽略首尾空白。
    pub fn parse(raw: &str) -> Option<Self> {
        let needle = raw.trim().to_ascii_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|kind| kind.as_str() == needle)
    }

    /// 是否为深色。
    ///
    /// 只有 [`Self::Dark`] 是深色——`System` **不算**深色：
    /// 它尚未解析成具体配色（要等系统偏好），此时断言明暗会得到
    /// 「取决于运行环境」的答案。调用方若要判「实际生效的明暗」，
    /// 应先解析 `System`。
    pub const fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }

    /// 是否为**跟随系统**。
    pub const fn is_system(self) -> bool {
        matches!(self, Self::System)
    }

    /// 是否为**自定义调色板**。
    pub const fn is_custom(self) -> bool {
        matches!(self, Self::Custom)
    }
}

impl std::fmt::Display for ThemeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ThemeKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ThemeKind::parse(s).ok_or_else(|| {
            format!(
                "未知主题种类：{}（应为 light / dark / system / custom）",
                s.trim()
            )
        })
    }
}
