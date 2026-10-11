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
//! - [`BuiltinTheme::Dark`]：深色（原型默认）
//! - [`BuiltinTheme::Light`]：浅色
//! - [`BuiltinTheme::System`]：**跟随系统**，本身不含配色，
//!   由 [`resolve`] 在运行时解析为深或浅。
//!
//! # 自定义主题（P2）的加载点
//!
//! [`Theme::custom`] 保留了从外部数据构造主题的能力。首版不实现
//! TOML/JSON 加载（避免过度设计），但**结构上留好了**：
//! 加载器只需把文件解析成 [`Theme`] 即可，无需改动任何渲染代码。

use egui::Color32;

use crate::application::dto::{CustomThemePaletteView, ThemeView};

/// 主题标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BuiltinTheme {
    /// 跟随系统。
    #[default]
    System,
    /// 深色。
    Dark,
    /// 浅色。
    Light,
}

impl BuiltinTheme {
    /// 由设置里的字符串解析（`light` / `dark` / `system`）。
    ///
    /// 未知值退回 [`BuiltinTheme::System`] 而非报错：旧配置里出现
    /// 将来才引入的取值时，程序应当照常启动。
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dark" => Self::Dark,
            "light" => Self::Light,
            _ => Self::System,
        }
    }

    /// 规范字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// 中文显示名（设置页三选一）。
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "跟随系统",
            Self::Dark => "深色",
            Self::Light => "浅色",
        }
    }
}

/// 尺寸token（对齐原型 `tokens.css` 的布局栅格）。
///
/// 这些值是**硬契约**：验收标准要求「1280×800 与 1440×900 两档窗口下
/// 布局不破版」，而破版与否直接取决于侧边栏是不是 240px。
/// 圆角同样取自原型（2026-10 精修后为 6 / 8 / 14），改动即等于偏离原型。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Metrics {
    /// 顶部工具栏高度（`--topbar-h`）。
    ///
    /// 这是**应用级菜单栏**那一行（文件 / 编辑 / 视图 / 设置），
    /// 不含编辑区工具栏——后者见 [`Metrics::editor_toolbar_height`]。
    pub topbar_height: f32,
    /// **编辑区工具栏**行高。
    ///
    /// # 为什么与应用级菜单栏分成两行
    ///
    /// 两者是**不同的层**：应用级菜单作用于整个应用（打开 / 保存 /
    /// 视图开关 / 设置），编辑区工具栏作用于当前编辑区的内容格式
    /// （加粗 / 列表 / 缩进……）。挤在一行里时，「文件」下拉与「加粗」
    /// 按钮看起来同级，职责边界随之消失。
    ///
    /// 该高度**不在原型 `tokens.css` 的栅格里**——原型把两者放在同一行
    /// （`--topbar-h: 40`），是本项目按「应用级 / 编辑区」分层后新增的一段。
    /// 取 34 而非 32：按钮本身高约 26，上下各留 4 的呼吸位，
    /// 视觉密度与 `--tabstrip-h: 32` 接近而略宽松。
    pub editor_toolbar_height: f32,
    /// 状态栏高度（`--statusbar-h`）。
    pub statusbar_height: f32,
    /// 标签条高度（`--tabbar-h`）。
    pub tabbar_height: f32,
    /// 多文件标签条高度（`--tabstrip-h`）。
    pub tabstrip_height: f32,
    /// 侧边栏宽度（`--sidebar-w`）。
    pub sidebar_width: f32,
    /// 分割条宽度（`--divider-w`）。
    pub divider_width: f32,
    /// 按钮圆角（`--radius-btn`）。
    pub radius_button: f32,
    /// 卡片圆角（`--radius-card`）。
    pub radius_card: f32,
    /// 对话框圆角（`--radius-dialog`）。
    pub radius_dialog: f32,
    /// 界面字号（`--ui-font-size`）。
    pub ui_font_size: f32,
    /// 预览区字号（`--preview-font-size`）。
    pub preview_font_size: f32,
    /// 预览区行高（`--preview-line-height`）。
    pub preview_line_height: f32,
    // —— 间距刻度（spacing scale，全应用唯一来源）——
    //
    // 原型 `tokens.css` 只定义了「尺寸」没有定义「间距刻度」，各视图于是
    // 各写各的 4 / 6 / 8 / 10 / 12 / 16 / 20 / 24：同类间距在不同区域忽大忽小，
    // 「分区留白」「元素内边距」「组件间距」没有可对照的规范。
    // 这里把它收敛成一条以 4pt 为基准、2pt 为咬合位的刻度
    // （与原型 `--sp-1: 4px` 同源）。任何 `add_space` / 内边距 / 组件间隙
    // 都应取这里的刻度值，而不是在现场发明新数字。
    /// 咬合间距：分隔线与内容、控件与贴身边缘的最小距离。
    pub space_xxs: f32,
    /// 紧凑间距：预览块与块之间、行内小控件之间。
    pub space_xs: f32,
    /// 小间距：图标与文字、卡片内边距的纵向分量。
    pub space_sm: f32,
    /// 中间距：组件默认间距（与全局 `Spacing::item_spacing.x` 同源）。
    pub space_md: f32,
    /// 大间距：字段行之间、下拉分组之间、对话框分区之间。
    pub space_lg: f32,
    /// 特大间距：功能区块之间、列表缩进。
    pub space_xl: f32,
    /// 顶级留白：对话框 / 欢迎页的顶级分区之间。
    pub space_xxl: f32,
    /// 面板 / 卡片 / 表格的四周内边距。
    pub pane_padding: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            topbar_height: 40.0,
            editor_toolbar_height: 34.0,
            statusbar_height: 24.0,
            tabbar_height: 28.0,
            tabstrip_height: 32.0,
            sidebar_width: 240.0,
            divider_width: 4.0,
            radius_button: 6.0,
            radius_card: 8.0,
            radius_dialog: 14.0,
            ui_font_size: 13.0,
            preview_font_size: 15.0,
            preview_line_height: 1.7,
            // 间距刻度：4pt 基网格 + 2pt 咬合位（见结构体字段文档）。
            space_xxs: 2.0,
            space_xs: 4.0,
            space_sm: 6.0,
            space_md: 8.0,
            space_lg: 12.0,
            space_xl: 16.0,
            space_xxl: 24.0,
            pane_padding: 8.0,
        }
    }
}

/// 完整主题：配色 + 尺寸 + 字体。
#[derive(Clone, PartialEq, Debug)]
pub struct Theme {
    /// 主题标识（`custom` 时为 [`BuiltinTheme::System`] 的值，但配色是自定义的）。
    pub kind: BuiltinTheme,
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
            kind: BuiltinTheme::Dark,
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
            kind: BuiltinTheme::Light,
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
        base: BuiltinTheme,
        font_family: &str,
        font_size: f32,
        line_height: f32,
    ) -> Self {
        let mut theme = match base {
            BuiltinTheme::Light => Self::light(),
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
            BuiltinTheme::Dark
        } else {
            BuiltinTheme::Light
        };
        theme
    }

    /// 是否为深色（决定 egui 的明暗模式）。
    pub const fn is_dark(&self) -> bool {
        matches!(self.kind, BuiltinTheme::Dark)
    }
}

/// 编辑区默认字体族。
///
/// egui 内置字体是 ProggyClean（位图字体），对中文与等宽排版都不友好。
/// 首版沿用内置字体以保证零额外体积（S5-03 的体积硬指标），
/// 阶段 08 再评估嵌入 `Noto Sans Mono` 子集——**必须**评估体积后再定。
pub const MONO_FAMILY: &str = "monospace";

/// 编辑区正文字体族（与原型 `--font-ui` 对应）。
pub const UI_FAMILY: &str = "ProggyClean";

/// 解析主题设置 → 具体主题。
///
/// `system_preference` 由调用方从系统读取（`true` = 系统是深色）。
/// 抽成参数而非在内部读系统，是为了让本函数**可测**——
/// CI 上没有「跟随系统」的真实环境。
pub fn resolve(view: &ThemeView, system_preference_dark: bool) -> Theme {
    // 自定义主题：以调色板为准。调色板缺失（旧配置 / 异常）时退回默认调色板，
    // 保证首次切到 custom（尚无任何配色改动）不崩、也不丢配色。
    if view.kind.trim().eq_ignore_ascii_case("custom") {
        let default_palette = CustomThemePaletteView::default();
        let palette = view.custom.as_ref().unwrap_or(&default_palette);
        return Theme::from_custom_palette(
            palette,
            &view.font_family,
            view.font_size,
            view.line_height,
        );
    }
    let kind = BuiltinTheme::parse(&view.kind);
    let base = match kind {
        BuiltinTheme::Light => BuiltinTheme::Light,
        BuiltinTheme::Dark => BuiltinTheme::Dark,
        BuiltinTheme::System => {
            if system_preference_dark {
                BuiltinTheme::Dark
            } else {
                BuiltinTheme::Light
            }
        }
    };
    let mut theme =
        Theme::with_font_settings(base, &view.font_family, view.font_size, view.line_height);
    // 解析后 `kind` 必须反映**实际生效**的那套，否则 `is_dark()` 会误报。
    theme.kind = base;
    theme
}

/// 主题变更签名：内容不变时返回 `None`，让预览缓存免于重建。
///
/// 验收标准要求「主题变更触发预览缓存失效（断言 Galley 重建）」，
/// 但**无关字段**（如字体）变化不该让预览重排——那是纯浪费。
/// 用签名把「配色变了」与「只是字号变了」区分开。
pub fn palette_signature(theme: &Theme) -> Option<[u8; 4]> {
    let mut hasher: u32 = 2166136261;
    for color in [
        theme.bg_primary,
        theme.bg_secondary,
        theme.bg_tertiary,
        theme.border,
        theme.text_primary,
        theme.text_secondary,
        theme.accent,
        theme.code_bg,
        theme.selection_bg,
        // 全部语法高亮色都纳入（V11-09）：任一语法色变化都必须让预览缓存失效，
        // 否则用户改了「关键字色」却发现预览区没变——那是最难解释的一类 bug。
        theme.syn_heading,
        theme.syn_bold,
        theme.syn_italic,
        theme.syn_code,
        theme.syn_link,
        theme.syn_quote,
        theme.syn_marker,
        theme.syn_meta,
        theme.syn_table,
        theme.syn_other,
    ] {
        let [r, g, b, a] = color.to_array();
        for byte in [r, g, b, a] {
            hasher ^= u32::from(byte);
            hasher = hasher.wrapping_mul(16777619);
        }
    }
    // `u32 → 4 字节` 折叠：签名只需「变了 / 没变」，不需密码学强度。
    let folded = hasher.to_le_bytes();
    Some([folded[0], folded[1], folded[2], folded[3]])
}

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
