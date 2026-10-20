//! 内置主题（阶段 06 · S6-10）。
//!
//! 配色**逐值抄自**原型 `src/styles/tokens.css`，不自行发挥——
//! 视觉对齐的验收方式是「与原型逐屏比对」，一旦本地「顺手调好看一点」，
//! 比对就失去意义，且没人能说清差在哪。
//!
//! # 2026-10 视觉精修同步
//!
//! 原型的 `tokens.css` 已升级为「深色专注型」调色板：更深的画布、分明的
//! 表面层次（frame / canvas / elevated / sunken）、克制的单一冷蓝强调色。
//! 本文件的取值随之**重抄一遍**，让桌面端与原型再次逐值对齐；圆角 token
//! 也同步收敛到原型的 `--radius-btn/card/dialog` = 6 / 8 / 14。
//!
//! 注意 `Theme` 仍是「原型令牌的子集」：`--bg-elevated` / `--bg-sunken`
//! 等暂时没有对应字段，相关控件沿用 `bg_secondary` / `bg_tertiary`，
//! 不为此新增字段（新增字段要动所有构造点与自定义调色板，收益不成正比）。
//!
//! # 三套主题
//!
//! - [`ThemeKind::Dark`]：深色（原型默认）
//! - [`ThemeKind::Light`]：浅色
//! - [`ThemeKind::System`]：**跟随系统**，本身不含配色，
//!   由 [`resolve`] 在运行时解析为深或浅。
//!
//! # 主题种类的唯一真源
//!
//! 种类枚举是 [`crate::shared::theme::ThemeKind`]（三方共用）——
//! 表现层**不得**依赖领域（`check-arch` 拦 `crate::domain`），
//! 故真源落在 `shared`。
//!
//! 2026-10-20 之前，表现层另有一份 `BuiltinTheme`（**3** 变体，无
//! `Custom`），加上领域版、应用层与组合根各一份手写 `match`，共**四份**
//! 副本。表现层那份缺 `Custom` 正是「自定义主题在表现层无法如实表达」
//! 的根因——本文件 [`Theme::kind`] 曾用 `System` 顶替 `Custom`。
//! R-09 已全部收敛，见 `docs/architecture-refactor-plan.md` §4.2 R-09。
//!
//! # 自定义主题（P2）的加载点
//!
//! [`Theme::custom`] 保留了从外部数据构造主题的能力。首版不实现
//! TOML/JSON 加载（避免过度设计），但**结构上留好了**：
//! 加载器只需把文件解析成 [`Theme`] 即可，无需改动任何渲染代码。
//!
//! # 模块结构
//!
//! - `labels`：[`theme_kind_label`] 界面专属显示名；
//! - `metrics`：[`Metrics`] 尺寸 / 间距 token；
//! - `resolve`：设置解析 [`resolve`] 与配色变更签名 [`palette_signature`]；
//! - 本文件保留 [`Theme`] 结构、内置配色构造与颜色辅助函数。

mod labels;
mod metrics;
mod resolve;

pub use self::labels::theme_kind_label;
pub use self::metrics::Metrics;
pub use self::resolve::{palette_signature, resolve};

use crate::shared::theme::ThemeKind;

use egui::Color32;

use crate::application::dto::CustomThemePaletteView;

/// 完整主题：配色 + 尺寸 + 字体。
#[derive(Clone, PartialEq, Debug)]
pub struct Theme {
    /// **实际生效的明暗**（只可能是 [`ThemeKind::Light`] 或
    /// [`ThemeKind::Dark`]）。
    ///
    /// # 为什么不是「用户选的种类」
    ///
    /// 「用户选了什么」与「现在是什么颜色」是**两件事**：
    ///
    /// - 用户选 `system` → 实际生效取决于系统偏好；
    /// - 用户选 `custom` → 实际生效取决于其调色板主背景的亮度。
    ///
    /// 渲染层只关心后者（`is_dark()` 决定 egui 明暗模式），
    /// 「用户选了什么」由**设置里的 `kind` 字符串**承载（那是持久化的真源）。
    ///
    /// 此前这个字段是表现层私有的 `BuiltinTheme`，`custom` 时被迫用
    /// `System` 顶替——于是「当前是不是跟随系统」这件事**在运行时无从判断**
    /// （`refresh_system_theme` 的 `kind != System` 判断因此恒成立，
    /// 跟随系统时系统外观变化不会自动刷新）。
    /// 改用 [`ThemeKind`] 后，用户选的种类可直接从设置读出，
    /// 不再靠这个字段间接推断。
    pub kind: ThemeKind,
    /// 尺寸 token。
    pub metrics: Metrics,

    // —— 基础色（`--bg-*` / `--border`）——
    /// 主背景（编辑区背景）。
    pub bg_primary: Color32,
    /// 次背景（工具栏、状态栏）。
    pub bg_secondary: Color32,
    /// 三级背景（输入框、悬浮卡片）。
    pub bg_tertiary: Color32,
    /// 边框 / 分割线。
    pub border: Color32,

    // —— 文本（`--text-*`）——
    /// 主文本色。
    pub text_primary: Color32,
    /// 次文本色（占位符、禁用态）。
    pub text_secondary: Color32,

    // —— 语义色 ——
    /// 强调色（选中项、链接）。
    pub accent: Color32,
    /// 强调色的柔和底（选中行背景）。
    pub accent_soft: Color32,
    /// 成功 / 新增。
    pub success: Color32,
    /// 警告。
    pub warning: Color32,
    /// 错误 / 删除。
    pub error: Color32,

    // —— 差异对比 ——
    /// 新增行前景。
    pub diff_add: Color32,
    /// 删除行前景。
    pub diff_del: Color32,
    /// 新增行背景。
    pub diff_add_bg: Color32,
    /// 删除行背景。
    pub diff_del_bg: Color32,

    // —— 语法高亮（`--syn-*`）——
    /// 标题。
    pub syn_heading: Color32,
    /// 粗体。
    pub syn_bold: Color32,
    /// 斜体。
    pub syn_italic: Color32,
    /// 行内代码 / 链接。
    pub syn_code: Color32,
    /// 链接。
    pub syn_link: Color32,
    /// 引用。
    pub syn_quote: Color32,
    /// 列表标记 / 强调标记。
    pub syn_marker: Color32,
    /// 元信息（HTML 标签名等）。
    pub syn_meta: Color32,
    /// 表格。
    pub syn_table: Color32,
    /// 其它语义（类型名、函数名等未单列的 token）。
    pub syn_other: Color32,

    // —— 编辑器状态 ——
    /// 当前行高亮背景。
    pub current_line: Color32,
    /// 选区背景。
    pub selection_bg: Color32,
    /// 查找命中背景。
    pub find_bg: Color32,
    /// 当前查找命中背景。
    pub find_current_bg: Color32,
    /// 代码块背景。
    pub code_bg: Color32,
    /// 滚动条。
    pub scrollbar: Color32,

    /// 编辑区字体族。
    pub editor_font_family: String,
    /// 编辑区字号。
    pub editor_font_size: f32,
    /// 编辑区行高倍数。
    pub editor_line_height: f32,
    /// 编辑区缩进空格数。
    pub editor_tab_size: u8,
}

impl Theme {
    /// 深色主题（深色专注型，对齐原型 `tokens.css` 的 `[data-theme='dark']`）。
    pub fn dark() -> Self {
        Self {
            kind: ThemeKind::Dark,
            metrics: Metrics::default(),
            bg_primary: rgb(0x14, 0x16, 0x1B),
            bg_secondary: rgb(0x1A, 0x1D, 0x24),
            bg_tertiary: rgb(0x23, 0x28, 0x33),
            border: rgb(0x2B, 0x32, 0x3D),
            text_primary: rgb(0xE8, 0xEA, 0xF0),
            text_secondary: rgb(0x9A, 0xA4, 0xB4),
            accent: rgb(0x6A, 0xA9, 0xFF),
            accent_soft: rgba(0x6A, 0xA9, 0xFF, 0.14),
            success: rgb(0x4E, 0xCF, 0xA4),
            warning: rgb(0xE0, 0xB0, 0x54),
            error: rgb(0xF2, 0x72, 0x7A),
            diff_add: rgb(0x4E, 0xCF, 0xA4),
            diff_del: rgb(0xF2, 0x72, 0x7A),
            diff_add_bg: rgba(0x4E, 0xCF, 0xA4, 0.13),
            diff_del_bg: rgba(0xF2, 0x72, 0x7A, 0.14),
            syn_heading: rgb(0x6A, 0xA9, 0xFF),
            syn_bold: rgb(0xE0, 0x98, 0x7A),
            syn_italic: rgb(0xC5, 0x8A, 0xF9),
            syn_code: rgb(0x4E, 0xD8, 0xB4),
            syn_link: rgb(0x4E, 0xD8, 0xB4),
            syn_quote: rgb(0x7D, 0xA3, 0x5F),
            syn_marker: rgb(0xE2, 0xC0, 0x7D),
            syn_meta: rgb(0x7D, 0xA3, 0x5F),
            syn_table: rgb(0x9C, 0xD4, 0xFE),
            syn_other: rgb(0xE8, 0xEA, 0xF0),
            current_line: rgba(0xFF, 0xFF, 0xFF, 0.045),
            selection_bg: rgba(0x6A, 0xA9, 0xFF, 0.26),
            find_bg: rgba(0xE0, 0xB0, 0x54, 0.28),
            find_current_bg: rgba(0xE0, 0xB0, 0x54, 0.60),
            code_bg: rgb(0x1C, 0x21, 0x2B),
            scrollbar: rgba(0xFF, 0xFF, 0xFF, 0.16),
            editor_font_family: MONO_FAMILY.to_string(),
            editor_font_size: 14.0,
            editor_line_height: 1.6,
            editor_tab_size: 4,
        }
    }

    /// 浅色主题（对齐原型 `tokens.css` 的 `[data-theme='light']`）。
    pub fn light() -> Self {
        Self {
            kind: ThemeKind::Light,
            metrics: Metrics::default(),
            bg_primary: rgb(0xFF, 0xFF, 0xFF),
            bg_secondary: rgb(0xF7, 0xF8, 0xFA),
            bg_tertiary: rgb(0xEE, 0xF1, 0xF5),
            border: rgb(0xE3, 0xE6, 0xEC),
            text_primary: rgb(0x1A, 0x1D, 0x23),
            text_secondary: rgb(0x5C, 0x65, 0x75),
            accent: rgb(0x25, 0x63, 0xEB),
            accent_soft: rgba(0x25, 0x63, 0xEB, 0.10),
            success: rgb(0x0F, 0x8A, 0x63),
            warning: rgb(0x96, 0x69, 0x0A),
            error: rgb(0xD1, 0x34, 0x38),
            diff_add: rgb(0x0F, 0x8A, 0x63),
            diff_del: rgb(0xD1, 0x34, 0x38),
            diff_add_bg: rgba(0x0F, 0x8A, 0x63, 0.10),
            diff_del_bg: rgba(0xD1, 0x34, 0x38, 0.10),
            syn_heading: rgb(0x1F, 0x4F, 0xB8),
            syn_bold: rgb(0xB3, 0x26, 0x1E),
            syn_italic: rgb(0x8B, 0x2F, 0x6B),
            syn_code: rgb(0x0F, 0x7A, 0x63),
            syn_link: rgb(0x0F, 0x7A, 0x63),
            syn_quote: rgb(0x2F, 0x7D, 0x32),
            syn_marker: rgb(0xA1, 0x54, 0x1C),
            syn_meta: rgb(0x2F, 0x7D, 0x32),
            syn_table: rgb(0x16, 0x55, 0xA8),
            syn_other: rgb(0x1A, 0x1D, 0x23),
            current_line: rgba(0x0F, 0x17, 0x2A, 0.045),
            selection_bg: rgba(0x25, 0x63, 0xEB, 0.22),
            find_bg: rgba(0xFF, 0xD6, 0x33, 0.45),
            find_current_bg: rgba(0xFF, 0xB4, 0x00, 0.70),
            code_bg: rgb(0xF2, 0xF4, 0xF8),
            scrollbar: rgba(0x0F, 0x17, 0x2A, 0.18),
            editor_font_family: MONO_FAMILY.to_string(),
            editor_font_size: 14.0,
            editor_line_height: 1.6,
            editor_tab_size: 4,
        }
    }

    /// 以某套内置主题为底，覆盖字体设置（设置页改动只影响字体，不影响配色）。
    pub fn with_font_settings(
        base: ThemeKind,
        font_family: &str,
        font_size: f32,
        line_height: f32,
    ) -> Self {
        let mut theme = match base {
            ThemeKind::Light => Self::light(),
            _ => Self::dark(),
        };
        if !font_family.trim().is_empty() {
            theme.editor_font_family = font_family.to_string();
        }
        theme.editor_font_size = font_size;
        theme.editor_line_height = line_height;
        theme
    }

    /// 由自定义调色板构造主题（P2 自定义主题 + V11-09 语法色覆盖）。
    ///
    /// 设计取舍：8 个**核心语义色**必填，语法高亮色可选覆盖；
    /// 其余（差异 / 语义色 / 尺寸）沿用内置主题的同类值——把 30+ 颜色
    /// 全部开放既增加 UI 负担，也容易出现「调出来不好看」的风险。
    ///
    /// # 基线为什么按明暗选，而不是固定深色
    ///
    /// 未覆盖的项（含全部语法色）必须**随明暗走**。若固定以 `dark()` 为底，
    /// 一个浅色背景的自定义主题会拿到深色主题的语法高亮色——浅底上出现
    /// 一堆低对比的浅色关键字。故先由主背景亮度定明暗，再取同明暗的内置
    /// 主题作底，未覆盖项天然协调。
    ///
    /// `kind` 不固定为 `Dark`：依据主背景亮度决定，驱动 egui 的明暗模式，
    /// 这样用户选浅色背景时界面整体走浅色，不会被锁在深色视觉里。
    pub fn from_custom_palette(
        palette: &CustomThemePaletteView,
        font_family: &str,
        font_size: f32,
        line_height: f32,
    ) -> Self {
        // 先解析主背景（决定明暗），再取同明暗的内置主题作基线。
        let bg_primary = parse_hex_color(&palette.bg_primary, Self::dark().bg_primary);
        let dark = is_dark_color(bg_primary);
        let mut theme = if dark { Self::dark() } else { Self::light() };

        theme.bg_primary = bg_primary;
        theme.bg_secondary = parse_hex_color(&palette.bg_secondary, theme.bg_secondary);
        // 三级背景（输入框 / 悬浮卡片）无独立入口，跟随次背景。
        theme.bg_tertiary = theme.bg_secondary;
        theme.border = parse_hex_color(&palette.border, theme.border);
        theme.text_primary = parse_hex_color(&palette.text_primary, theme.text_primary);
        theme.text_secondary = parse_hex_color(&palette.text_secondary, theme.text_secondary);
        theme.accent = parse_hex_color(&palette.accent, theme.accent);
        // 强调色柔和底由 accent 派生（调色板不暴露该值）。
        theme.accent_soft = Color32::from_rgba_unmultiplied(
            theme.accent.r(),
            theme.accent.g(),
            theme.accent.b(),
            51,
        );
        theme.code_bg = parse_hex_color(&palette.code_bg, theme.code_bg);
        theme.selection_bg = parse_hex_color(&palette.selection_bg, theme.selection_bg);

        // 语法高亮：只覆盖显式给出的项（`None` 保持基线值）。
        apply_override(&mut theme.syn_heading, &palette.syn_heading);
        apply_override(&mut theme.syn_bold, &palette.syn_bold);
        apply_override(&mut theme.syn_italic, &palette.syn_italic);
        apply_override(&mut theme.syn_code, &palette.syn_code);
        apply_override(&mut theme.syn_link, &palette.syn_link);
        apply_override(&mut theme.syn_quote, &palette.syn_quote);
        apply_override(&mut theme.syn_marker, &palette.syn_marker);
        apply_override(&mut theme.syn_meta, &palette.syn_meta);
        apply_override(&mut theme.syn_table, &palette.syn_table);
        apply_override(&mut theme.syn_other, &palette.syn_other);

        if !font_family.trim().is_empty() {
            theme.editor_font_family = font_family.to_string();
        }
        theme.editor_font_size = font_size;
        theme.editor_line_height = line_height;

        // 由主背景亮度决定 egui 明暗模式（custom 不直接映射 Light/Dark）。
        theme.kind = if dark {
            ThemeKind::Dark
        } else {
            ThemeKind::Light
        };
        theme
    }

    /// 是否为深色（决定 egui 的明暗模式）。
    pub const fn is_dark(&self) -> bool {
        matches!(self.kind, ThemeKind::Dark)
    }
}

/// 编辑区默认字体族。
///
/// egui 内置字体是 ProggyClean（位图字体），对中文与等宽排版都不友好。
/// 首版沿用内置字体以保证零额外体积（S5-03 的体积硬指标），
/// 阶段 08 再评估嵌入 `Noto Sans Mono` 子集——**必须**评估体积后再定。
pub const MONO_FAMILY: &str = "monospace";

/// 不透明色。
fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// 带 alpha 的色（`opacity` 为 0.0–1.0 的**不透明度**）。
///
/// 原型的 `rgba(86, 156, 214, 0.16)` 是「不透明度」写法，
/// 而 `Color32` 的第 4 通道是 **alpha（透明度）**，两者相差 `1 - a`。
/// 直接把 `0.16` 当 alpha 用会让「16% 不透明」变成「16% 透明」，
/// 视觉上差得极远（深色主题下背景会明显发灰）。
fn rgba(r: u8, g: u8, b: u8, opacity: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, (opacity.clamp(0.0, 1.0) * 255.0) as u8)
}

/// 解析 `#RRGGBB`（兼容无 `#` 前缀）为 `Color32`。
///
/// 长度非 6 或含非法十六进制字符时退回 `fallback` 而非 panic——
/// 自定义主题的颜色来自用户配置，解析错误不应让程序崩溃。
fn parse_hex_color(raw: &str, fallback: Color32) -> Color32 {
    let s = raw.trim().trim_start_matches('#');
    if s.len() != 6 {
        return fallback;
    }
    let r = u8::from_str_radix(&s[0..2], 16);
    let g = u8::from_str_radix(&s[2..4], 16);
    let b = u8::from_str_radix(&s[4..6], 16);
    match (r, g, b) {
        (Ok(r), Ok(g), Ok(b)) => Color32::from_rgb(r, g, b),
        _ => fallback,
    }
}

/// 若调色板给出了覆盖值，则用它替换目标色；否则保持不动（跟随基线）。
///
/// `None` 是「跟随主题」而不是「黑色」——这是 V11-09 的核心语义：
/// 用 `parse_hex_color(hex, *target)` 让非法值退回**当前基线**，
/// 而不是退回某个硬编码默认色。
fn apply_override(target: &mut Color32, value: &Option<String>) {
    if let Some(hex) = value {
        *target = parse_hex_color(hex, *target);
    }
}

/// 给定颜色是否偏暗（相对亮度近似），用于自定义主题决定 egui 明暗模式。
fn is_dark_color(c: Color32) -> bool {
    let [r, g, b, _] = c.to_array();
    let lum = 0.2126 * (r as f32) + 0.7152 * (g as f32) + 0.0722 * (b as f32);
    lum < 128.0
}
