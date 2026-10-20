//! 外观偏好值对象：[`ThemeKind`] / [`CustomThemePalette`] / [`LintSettings`] / [`Theme`]。
//!
//! 按「内聚的属性与行为边界」从 value_objects.rs 迁出；本簇只管主题与 lint 开关，
//! 编辑器行为偏好（缩进、行号、写作目标等）在 `editor.rs`。

use serde::{Deserialize, Serialize};

/// 主题种类（**唯一**真源见 [`crate::shared::theme`]）。
///
/// # 为什么不在这里定义
///
/// 主题种类是**三方**共同需要的：领域（持久化 `Theme.kind`）、
/// 应用（DTO ↔ 领域转换）、表现（设置页选项 / 主题解析 / 关于面板）。
/// 而表现层**不得**依赖领域（`.agent/architecture.md` §4；
/// `check-arch` 的 `PRESENTATION_FORBIDDEN` 拦 `crate::domain`），
/// 三方共同的依赖点只有 `shared`。
///
/// 2026-10-20 之前本文件定义着领域版 `ThemeKind`（4 变体），
/// 表现层另有一份 `BuiltinTheme`（**3** 变体，无 `Custom`），
/// 加上应用层与组合根各一份手写 `match`，共**四份**副本。
/// 唯一真源现落在 [`crate::shared::theme`]。
///
/// 公开路径 `domain::preferences::value_objects::ThemeKind` **保持不变**，
/// 故既有调用与测试零改动。
pub use crate::shared::theme::ThemeKind;

/// 自定义主题调色板（P2 自定义主题）。
///
/// 8 个**核心语义色**必填；语法高亮配色（V11-09）为**可选覆盖**。
/// 其余（尺寸、差异色、字体）沿用内置主题同类值，
/// 避免把 30+ 颜色全部开放带来的「调出来不好看」风险。
/// 颜色以 `#RRGGBB` 十六进制字符串存储——领域层不依赖 egui，
/// 真正 `#RRGGBB` ↔ `Color32` 的颜色转换在表现层完成。
///
/// # 语法色为什么是 `Option<String>`
///
/// `None` = **跟随内置主题**。这不是省事，而是为了两件事：
///
/// 1. **向后兼容**：旧配置文件里没有 `syn_*` 字段，`#[serde(default)]`
///    让它们读作 `None`，于是旧自定义主题的语法色自动跟随其背景明暗，
///    加载一次都不会失败；
/// 2. **明暗自适应**：自定义主题的明暗由主背景亮度决定，而用户很可能只调
///    背景色。若语法色是必填的固定深色值，把一个浅色背景调成深色后，
///    语法高亮会继续用浅色主题的暗色值，在深底上糊成一片。
///    跟随语义让「没明确指定的」随明暗走，用户只需覆盖他真正在意的那几个。
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct CustomThemePalette {
    /// 主背景（编辑区）。
    pub bg_primary: String,
    /// 次背景（工具栏 / 状态栏）。
    pub bg_secondary: String,
    /// 主文本色。
    pub text_primary: String,
    /// 次文本色（占位符 / 禁用态）。
    pub text_secondary: String,
    /// 强调色（选中项 / 链接）。
    pub accent: String,
    /// 代码块背景。
    pub code_bg: String,
    /// 边框 / 分割线。
    pub border: String,
    /// 选区背景。
    pub selection_bg: String,
    /// 语法高亮：标题（`None` = 跟随内置主题）。
    #[serde(default)]
    pub syn_heading: Option<String>,
    /// 语法高亮：加粗。
    #[serde(default)]
    pub syn_bold: Option<String>,
    /// 语法高亮：斜体。
    #[serde(default)]
    pub syn_italic: Option<String>,
    /// 语法高亮：行内代码 / 代码块。
    #[serde(default)]
    pub syn_code: Option<String>,
    /// 语法高亮：链接。
    #[serde(default)]
    pub syn_link: Option<String>,
    /// 语法高亮：引用块。
    #[serde(default)]
    pub syn_quote: Option<String>,
    /// 语法高亮：标记符（`#` / `-` 等）。
    #[serde(default)]
    pub syn_marker: Option<String>,
    /// 语法高亮：元信息（围栏语言名等）。
    #[serde(default)]
    pub syn_meta: Option<String>,
    /// 语法高亮：表格。
    #[serde(default)]
    pub syn_table: Option<String>,
    /// 语法高亮：其它（默认正文色）。
    #[serde(default)]
    pub syn_other: Option<String>,
}

impl Default for CustomThemePalette {
    fn default() -> Self {
        // 以内置深色主题的核心色作为起点，保证首次切换到「自定义」时配色协调。
        // 语法色一律 `None`（跟随主题），让用户从「什么都不用调」开始。
        Self {
            bg_primary: "#14161B".to_string(),
            bg_secondary: "#1A1D24".to_string(),
            text_primary: "#E8EAF0".to_string(),
            text_secondary: "#9AA4B4".to_string(),
            accent: "#6AA9FF".to_string(),
            code_bg: "#1C212B".to_string(),
            border: "#2B323D".to_string(),
            selection_bg: "#6AA9FF".to_string(),
            syn_heading: None,
            syn_bold: None,
            syn_italic: None,
            syn_code: None,
            syn_link: None,
            syn_quote: None,
            syn_marker: None,
            syn_meta: None,
            syn_table: None,
            syn_other: None,
        }
    }
}

/// 格式检查（lint）的规则开关（V11-08）。
///
/// # 为什么默认**全开**
///
/// lint 的价值在于「说出用户没注意到的地方」，默认只开一半等于
/// 把另一半规则藏起来——大多数人不会去翻设置页。
/// 代价是首次打开「检查」标签时可能列出若干问题，但那正是功能在工作的样子。
///
/// # 为什么每条都要 `serde(default = …)`
///
/// `Preferences` 是持久化的：旧配置文件里**没有**这几个字段，
/// 缺了必须按「启用」读出（与默认设置一致），
/// 而不是让整个配置加载失败（同 `show_line_numbers`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct LintSettings {
    /// 标题层级跳跃。
    #[serde(default = "default_lint_on")]
    pub heading_jump: bool,
    /// 列表符号混用。
    #[serde(default = "default_lint_on")]
    pub list_marker_mix: bool,
    /// 行尾多余空格。
    #[serde(default = "default_lint_on")]
    pub trailing_whitespace: bool,
    /// 未闭合代码围栏。
    #[serde(default = "default_lint_on")]
    pub unclosed_fence: bool,
}

/// lint 规则的缺省值：启用。
fn default_lint_on() -> bool {
    true
}

impl Default for LintSettings {
    fn default() -> Self {
        Self {
            heading_jump: true,
            list_marker_mix: true,
            trailing_whitespace: true,
            unclosed_fence: true,
        }
    }
}

impl LintSettings {
    /// 全部启用。
    pub fn all_enabled() -> Self {
        Self::default()
    }
}

/// 主题设置。
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Theme {
    kind: ThemeKind,
    font_family: String,
    font_size: f32,
    line_height: f32,
    /// 自定义调色板（仅当 `kind == Custom` 时生效）。
    #[serde(default)]
    pub custom: Option<CustomThemePalette>,
}

/// 字号合法区间（含端点）。
pub const FONT_SIZE_RANGE: std::ops::RangeInclusive<f32> = 8.0..=48.0;
/// 行高合法区间（含端点）。
pub const LINE_HEIGHT_RANGE: std::ops::RangeInclusive<f32> = 1.0..=3.0;

impl Theme {
    /// 默认主题：跟随系统、14px、行高 1.6。
    pub fn default_theme() -> Self {
        Self {
            kind: ThemeKind::System,
            font_family: "system-ui".to_string(),
            font_size: 14.0,
            line_height: 1.6,
            custom: None,
        }
    }

    /// 构造主题。
    pub fn new(
        kind: ThemeKind,
        font_family: impl Into<String>,
        font_size: f32,
        line_height: f32,
    ) -> Self {
        Self {
            kind,
            font_family: font_family.into(),
            font_size,
            line_height,
            custom: None,
        }
    }

    /// 主题种类。
    pub const fn kind(&self) -> ThemeKind {
        self.kind
    }

    /// 字体族。
    pub fn font_family(&self) -> &str {
        &self.font_family
    }

    /// 字号。
    pub const fn font_size(&self) -> f32 {
        self.font_size
    }

    /// 行高倍数。
    pub const fn line_height(&self) -> f32 {
        self.line_height
    }

    /// 自定义调色板（仅 `Custom` 种类下有意义）。
    pub fn palette(&self) -> Option<&CustomThemePalette> {
        self.custom.as_ref()
    }

    /// 校验取值是否在合法区间内。
    pub fn is_valid(&self) -> bool {
        !self.font_family.is_empty()
            && FONT_SIZE_RANGE.contains(&self.font_size)
            && LINE_HEIGHT_RANGE.contains(&self.line_height)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_theme()
    }
}
