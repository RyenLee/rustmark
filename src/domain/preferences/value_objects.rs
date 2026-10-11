//! Preferences 上下文的值对象（S3-12）。
//!
//! # 关键规则：`KeyCombo` 归一化与冲突判定
//!
//! 原型（`markdown-editor-prototype`）的快捷键存在「四处真相」缺陷：
//! 菜单、命令面板、快捷键一览各有一份匹配逻辑，且匹配层直接拒绝了纯单键
//! （导致 `Escape` / `F1`–`F12` 无法绑定）。本设计把规则收敛为两处纯函数：
//! - [`KeyCombo::parse`] + [`KeyCombo::display`]：**唯一**的字符串 ↔ 组合转换与显示源；
//! - [`ShortcutTable::conflicts`]：**唯一**的冲突判定，且只**报告**不拒绝。

use std::fmt;

use serde::{Deserialize, Serialize};

/// 内置主题种类。
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
    /// 自定义（由 [`CustomThemePalette`] 决定配色）。
    Custom,
}

impl ThemeKind {
    /// 是否为深色（用于选择默认强调色）。
    pub const fn is_dark(&self) -> bool {
        matches!(self, Self::Dark)
    }

    /// 规范字符串（与表现层 `BuiltinTheme::as_str` 对齐，供 DTO 转换复用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
            Self::Custom => "custom",
        }
    }
}

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

    /// 全部关闭。
    pub fn all_disabled() -> Self {
        Self {
            heading_jump: false,
            list_marker_mix: false,
            trailing_whitespace: false,
            unclosed_fence: false,
        }
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

/// 缩进宽度。
///
/// 以**枚举**而非 `usize` 表达，避免原型缺陷 2.2（缩进宽度硬编码为 2，
/// 与偏好设置的 `tabSize` 不一致）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
pub enum TabSize {
    /// 2 个空格。
    Two,
    /// 4 个空格（默认）。
    #[default]
    Four,
    /// 8 个空格。
    Eight,
}

impl TabSize {
    /// 空格数量。
    pub const fn spaces(&self) -> usize {
        match self {
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }

    /// 生成对应的缩进字符串。
    pub fn indent_unit(&self) -> String {
        " ".repeat(self.spaces())
    }

    /// 由空格数还原；非 {2,4,8} 时返回 `None`。
    pub const fn from_spaces(spaces: usize) -> Option<Self> {
        match spaces {
            2 => Some(Self::Two),
            4 => Some(Self::Four),
            8 => Some(Self::Eight),
            _ => None,
        }
    }
}

/// 写作目标（字数，V11-05）。
///
/// `0` 表示**未设置目标**——这是有意的零值语义：新用户不该一上来就被一个
/// 「0 / 1000 字」的进度条盯着，而用 `Option<u32>` 会让每个调用点都写一遍
/// `unwrap_or(0)`（且领域层禁用 `unwrap`）。
///
/// 上限 [`Self::MAX`] 采用**饱和**而非报错：目标填多大都不该让设置保存失败，
/// 与 `TabSize` 等值对象「非法值向内收敛」的处理保持一致。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct WritingGoal(u32);

impl WritingGoal {
    /// 目标上限（100 万字）：再大没有意义，也避免进度计算里的极端值。
    pub const MAX: u32 = 1_000_000;

    /// 构造目标；超过 [`Self::MAX`] 饱和到上限（不报错、不 `panic`）。
    pub const fn new(words: u32) -> Self {
        Self(if words > Self::MAX { Self::MAX } else { words })
    }

    /// 目标字数。
    pub const fn words(self) -> u32 {
        self.0
    }

    /// 是否设置了目标（`0` = 未设置）。
    pub const fn is_set(self) -> bool {
        self.0 > 0
    }

    /// 进度百分比（0~100）；未设目标返回 `None`。
    ///
    /// 超过 100% 一律按 100 显示：进度条画到 137% 没有意义，
    /// 而「已达成」由 [`Self::is_reached`] 单独表达。
    pub const fn progress_percent(self, words: usize) -> Option<u8> {
        if !self.is_set() {
            return None;
        }
        // u128 中间量：避免 `words * 100` 在极端字数下溢出（usize 满值也安全）。
        if (words as u128) >= self.0 as u128 {
            return Some(100);
        }
        Some(((words as u128 * 100) / self.0 as u128) as u8)
    }

    /// 是否达成；未设目标恒为 `false`（没有目标就谈不上达成）。
    pub const fn is_reached(self, words: usize) -> bool {
        self.is_set() && (words as u128) >= self.0 as u128
    }
}

/// `EditorSettings::show_line_numbers` 的 serde 缺省值（与 `default_settings()` 一致）。
///
/// 单独成函数是因为 `#[serde(default = "...")]` 只接受路径：写 `#[serde(default)]`
/// 会取 `bool::default()`（`false`），与「旧配置也按显示行号读」的意图相反。
fn default_show_line_numbers() -> bool {
    true
}

/// 编辑器设置。
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct EditorSettings {
    tab_size: TabSize,
    word_wrap: bool,
    /// 自动保存间隔（秒）；`None` 表示关闭自动保存。
    auto_save_interval: Option<u32>,
    spell_check: bool,
    /// 是否显示行号栏。
    ///
    /// 默认**显示**。行号与正文的对齐由「编辑区所有内容统一字号 + 统一行高」
    /// 结构性地保证（见 `mapper::StyleMapper::format`）：每行高度恒等于
    /// `editor_font_size × editor_line_height`，行号按行带垂直居中描画，
    /// 与中英文字体回退无关。需要更干净的界面时可在设置里关闭。
    ///
    /// `#[serde(default = "default_show_line_numbers")]`：旧配置文件里没有这个
    /// 字段，缺了按「显示」读出（与默认设置一致），而不是让配置加载失败。
    #[serde(default = "default_show_line_numbers")]
    show_line_numbers: bool,
    /// 撤销合并窗口（毫秒），供 [`crate::document::undo::UndoPolicy`] 使用。
    undo_merge_window_ms: u64,
    /// 编辑区 / 预览区分割比例（0.2 ~ 0.8），持久化以记住用户拖拽结果。
    #[serde(default)]
    split_ratio: f32,
    /// 写作目标（字数）；`0` = 未设置（V11-05）。
    ///
    /// `#[serde(default)]`：**旧配置文件里没有这个字段**，缺了必须按
    /// 「未设置」读出来，而不是让整个配置加载失败（同 `split_ratio`）。
    #[serde(default)]
    writing_goal: WritingGoal,
    /// 格式检查（lint）的规则开关（V11-08）。
    ///
    /// 旧配置缺这个字段时按 [`LintSettings::default`]（全启用）读出。
    #[serde(default)]
    lint: LintSettings,
}

impl EditorSettings {
    /// 默认设置：4 空格缩进、自动换行、30 秒自动保存、不开拼写检查、
    /// 显示行号栏、400ms 撤销窗口、未设写作目标。
    pub fn default_settings() -> Self {
        Self {
            tab_size: TabSize::Four,
            word_wrap: true,
            auto_save_interval: Some(30),
            spell_check: false,
            show_line_numbers: true,
            undo_merge_window_ms: 400,
            split_ratio: 0.5,
            writing_goal: WritingGoal::default(),
            lint: LintSettings::all_enabled(),
        }
    }

    /// 构造设置。
    // 参数与 [`EditorSettingsView`] 的字段一一对应，收敛成结构体会让
    // 调用点失去编译期「漏传即错」的保障（同 `find_replace_ui_state` 的先例）。
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        tab_size: TabSize,
        word_wrap: bool,
        auto_save_interval: Option<u32>,
        spell_check: bool,
        show_line_numbers: bool,
        undo_merge_window_ms: u64,
        split_ratio: f32,
        writing_goal: WritingGoal,
        lint: LintSettings,
    ) -> Self {
        Self {
            tab_size,
            word_wrap,
            auto_save_interval,
            spell_check,
            show_line_numbers,
            undo_merge_window_ms,
            split_ratio,
            writing_goal,
            lint,
        }
    }

    /// 缩进宽度。
    pub const fn tab_size(&self) -> TabSize {
        self.tab_size
    }

    /// 是否自动换行。
    pub const fn word_wrap(&self) -> bool {
        self.word_wrap
    }

    /// 自动保存间隔（秒）。
    pub const fn auto_save_interval(&self) -> Option<u32> {
        self.auto_save_interval
    }

    /// 是否启用拼写检查。
    pub const fn spell_check(&self) -> bool {
        self.spell_check
    }

    /// 是否显示行号栏。
    pub const fn show_line_numbers(&self) -> bool {
        self.show_line_numbers
    }

    /// 撤销合并窗口（毫秒）。
    pub const fn undo_merge_window_ms(&self) -> u64 {
        self.undo_merge_window_ms
    }

    /// 编辑区 / 预览区分割比例（0.2 ~ 0.8）。
    pub const fn split_ratio(&self) -> f32 {
        self.split_ratio
    }

    /// 写作目标（字数）；`0` = 未设置（V11-05）。
    pub const fn writing_goal(&self) -> WritingGoal {
        self.writing_goal
    }

    /// 格式检查（lint）的规则开关（V11-08）。
    pub const fn lint(&self) -> LintSettings {
        self.lint
    }

    /// 设置写作目标，返回新的设置（值对象不可变）。
    pub const fn with_writing_goal(self, goal: WritingGoal) -> Self {
        Self {
            writing_goal: goal,
            ..self
        }
    }

    /// 替换 lint 规则开关，返回新的设置（值对象不可变）。
    pub const fn with_lint(self, lint: LintSettings) -> Self {
        Self { lint, ..self }
    }

    /// 校验取值是否合法（自动保存间隔不得为 0——会导致忙循环）。
    pub const fn is_valid(&self) -> bool {
        !matches!(self.auto_save_interval, Some(0))
    }
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self::default_settings()
    }
}

/// 修饰键组合（归一化：**与书写顺序无关**）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
pub struct Modifiers {
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
}

impl Modifiers {
    /// 无修饰键。
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    /// 构造修饰键组合。
    pub const fn new(ctrl: bool, alt: bool, shift: bool, meta: bool) -> Self {
        Self {
            ctrl,
            alt,
            shift,
            meta,
        }
    }

    /// 是否含 `Ctrl`。
    pub const fn ctrl(&self) -> bool {
        self.ctrl
    }

    /// 是否含 `Alt`。
    pub const fn alt(&self) -> bool {
        self.alt
    }

    /// 是否含 `Shift`。
    pub const fn shift(&self) -> bool {
        self.shift
    }

    /// 是否含 `Meta`（macOS `Cmd` / Windows `Win`）。
    pub const fn meta(&self) -> bool {
        self.meta
    }

    /// 是否不含任何修饰键。
    pub const fn is_empty(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.meta
    }

    /// 按固定顺序（Ctrl → Alt → Shift → Meta）拼出前缀，形如 `"Ctrl+Alt+"`。
    fn prefix(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.meta {
            out.push_str("Meta+");
        }
        out
    }
}

/// 功能键（非字符键）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedKey {
    /// `Esc`
    Escape,
    /// 回车
    Enter,
    /// 制表
    Tab,
    /// 空格
    Space,
    /// 退格
    Backspace,
    /// 删除
    Delete,
    /// 方向键上
    Up,
    /// 方向键下
    Down,
    /// 方向键左
    Left,
    /// 方向键右
    Right,
    /// `Home`
    Home,
    /// `End`
    End,
    /// `PageUp`
    PageUp,
    /// `PageDown`
    PageDown,
    /// `Insert`
    Insert,
    /// `F1`
    F1,
    /// `F2`
    F2,
    /// `F3`
    F3,
    /// `F4`
    F4,
    /// `F5`
    F5,
    /// `F6`
    F6,
    /// `F7`
    F7,
    /// `F8`
    F8,
    /// `F9`
    F9,
    /// `F10`
    F10,
    /// `F11`
    F11,
    /// `F12`
    F12,
}

impl NamedKey {
    /// 由常见别名解析（大小写不敏感）。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.to_ascii_lowercase().as_str() {
            "esc" | "escape" => Some(Self::Escape),
            "enter" | "return" | "cr" => Some(Self::Enter),
            "tab" => Some(Self::Tab),
            "space" | " " => Some(Self::Space),
            "backspace" => Some(Self::Backspace),
            "delete" | "del" => Some(Self::Delete),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "home" => Some(Self::Home),
            "end" => Some(Self::End),
            "pageup" | "pgup" => Some(Self::PageUp),
            "pagedown" | "pgdn" => Some(Self::PageDown),
            "insert" | "ins" => Some(Self::Insert),
            "f1" => Some(Self::F1),
            "f2" => Some(Self::F2),
            "f3" => Some(Self::F3),
            "f4" => Some(Self::F4),
            "f5" => Some(Self::F5),
            "f6" => Some(Self::F6),
            "f7" => Some(Self::F7),
            "f8" => Some(Self::F8),
            "f9" => Some(Self::F9),
            "f10" => Some(Self::F10),
            "f11" => Some(Self::F11),
            "f12" => Some(Self::F12),
            _ => None,
        }
    }

    /// 规范显示名。
    pub const fn display(&self) -> &'static str {
        match self {
            Self::Escape => "Esc",
            Self::Enter => "Enter",
            Self::Tab => "Tab",
            Self::Space => "Space",
            Self::Backspace => "Backspace",
            Self::Delete => "Delete",
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Home => "Home",
            Self::End => "End",
            Self::PageUp => "PageUp",
            Self::PageDown => "PageDown",
            Self::Insert => "Insert",
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::F3 => "F3",
            Self::F4 => "F4",
            Self::F5 => "F5",
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::F10 => "F10",
            Self::F11 => "F11",
            Self::F12 => "F12",
        }
    }
}

/// 组合键的主键：字符键或功能键。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Key {
    /// 字符键。**ASCII 字母一律归一化为大写**，故 `s` 与 `S` 判等。
    Char(char),
    /// 功能键。
    Named(NamedKey),
}

impl Key {
    /// 由字符串解析；失败时返回 `None`。
    pub fn parse(raw: &str) -> Option<Self> {
        if let Some(named) = NamedKey::parse(raw) {
            return Some(Self::Named(named));
        }
        let mut chars = raw.chars();
        let first = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        let normalized = if first.is_ascii_alphabetic() {
            first.to_ascii_uppercase()
        } else {
            first
        };
        Some(Self::Char(normalized))
    }

    /// 规范显示名。
    pub fn display(&self) -> String {
        match self {
            Self::Char(c) => c.to_string(),
            Self::Named(named) => named.display().to_string(),
        }
    }
}

/// 一个快捷键组合（修饰键 + 主键）。
///
/// 归一化保证：`Ctrl+Shift+S` 与 `Shift+Ctrl+s` **判等**（原型缺陷 C 的根治）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct KeyCombo {
    modifiers: Modifiers,
    key: Key,
}

impl KeyCombo {
    /// 构造组合。
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    /// 无修饰键的组合（如 `Escape`、`F5`）。
    pub const fn plain(key: Key) -> Self {
        Self::new(Modifiers::NONE, key)
    }

    /// 解析字符串形式，如 `"Ctrl+Shift+S"`。
    ///
    /// 规则：以 `+` 分隔；任一片段若匹配修饰键别名（大小写不敏感）则记为修饰键，
    /// 否则视为主键；出现多个主键或没有主键时返回 [`PreferencesError`]。
    pub fn parse(raw: &str) -> Result<Self, crate::domain::preferences::errors::PreferencesError> {
        use crate::domain::preferences::errors::PreferencesError;

        let mut modifiers = Modifiers::NONE;
        let mut key: Option<Key> = None;
        for token in raw.split('+') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" | "option" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                "meta" | "cmd" | "command" | "super" | "win" => modifiers.meta = true,
                other => {
                    let parsed =
                        Key::parse(other).ok_or_else(|| PreferencesError::InvalidCombo {
                            raw: raw.to_string(),
                        })?;
                    if key.is_some() {
                        return Err(PreferencesError::InvalidCombo {
                            raw: raw.to_string(),
                        });
                    }
                    key = Some(parsed);
                }
            }
        }
        match key {
            Some(key) => Ok(Self::new(modifiers, key)),
            None => Err(PreferencesError::InvalidCombo {
                raw: raw.to_string(),
            }),
        }
    }

    /// 修饰键。
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// 主键。
    pub const fn key(&self) -> Key {
        self.key
    }

    /// **唯一**的显示形式（菜单 / 命令面板 / 快捷键一览三处共用此函数）。
    ///
    /// 形如 `Ctrl+Shift+S`、`Esc`、`F5`。
    pub fn display(&self) -> String {
        format!("{}{}", self.modifiers.prefix(), self.key.display())
    }

    /// 是否可能与文本输入冲突：无修饰键的字符键（如裸 `A`）。
    ///
    /// 注意：这是**提示**而非禁止——`Escape` / `F1`–`F12` 等无修饰键的功能键
    /// 不属于冲突（原型缺陷 A 的根治：匹配层不得拒绝纯单键）。
    pub const fn clashes_with_text_input(&self) -> bool {
        self.modifiers.is_empty() && matches!(self.key, Key::Char(_))
    }
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

impl serde::Serialize for KeyCombo {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.display())
    }
}

impl<'de> serde::Deserialize<'de> for KeyCombo {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// 可绑定的动作（命令面板与快捷键共享同一份枚举）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionId {
    /// 新建文档
    NewDocument,
    /// 打开文档
    OpenDocument,
    /// 保存
    Save,
    /// 另存为
    SaveAs,
    /// 关闭标签页
    CloseTab,
    /// 撤销
    Undo,
    /// 重做
    Redo,
    /// 查找
    Find,
    /// 替换
    Replace,
    /// 加粗
    Bold,
    /// 斜体
    Italic,
    /// 删除线
    Strikethrough,
    /// 行内代码
    InlineCode,
    /// 插入链接
    InsertLink,
    /// 代码块
    CodeBlock,
    /// 插入表格
    InsertTable,
    /// 切换引用
    ToggleQuote,
    /// 提升标题级别
    PromoteHeading,
    /// 降低标题级别
    DemoteHeading,
    /// 缩进
    Indent,
    /// 反缩进
    Outdent,
    /// 切换任务勾选
    ToggleTask,
    /// 切换预览
    TogglePreview,
    /// 切换大纲
    ToggleOutline,
    /// 打开命令面板
    CommandPalette,
    /// 打开设置
    OpenSettings,
    /// 导出
    Export,
    /// 专注 / 阅读模式（默认 `F8`，`Esc` 退出）
    ///
    /// 之所以必须可绑定：原型缺陷 B-01 的根因是「匹配层拒绝纯单键」，
    /// 于是 `F8` 这类声明在设置里却永远不触发。此处入枚举即代表
    /// 「功能键与 `Esc` 是合法组合」，匹配层不得再拒绝。
    FocusMode,
    /// 关闭浮层 / 对话框（默认 `Esc`）
    ///
    /// 原型缺陷 B-03：该项可录制但被硬编码分支绕过。此处无任何特判，
    /// 改绑后 `Esc` 即失效（验收用例 ③）。
    CloseOverlay,
    /// 文档统计
    DocumentStats,
    /// 折叠 / 展开侧边栏
    ToggleSidebar,
    /// 显示 / 隐藏底部状态栏
    ToggleStatusbar,
    /// 在所有文件中查找
    FindInFiles,
    /// 保存全部已修改文档
    SaveAll,
    /// 插入符号 / Emoji
    InsertSymbol,
    /// 对比文件差异
    CompareDiff,
    /// 下一个标签页
    NextTab,
    /// 上一个标签页
    PrevTab,
}

impl ActionId {
    /// 全部动作（用于「快捷键一览」与冲突检测的全量遍历）。
    pub const fn all() -> &'static [ActionId] {
        &[
            Self::NewDocument,
            Self::OpenDocument,
            Self::Save,
            Self::SaveAs,
            Self::CloseTab,
            Self::Undo,
            Self::Redo,
            Self::Find,
            Self::Replace,
            Self::Bold,
            Self::Italic,
            Self::Strikethrough,
            Self::InlineCode,
            Self::InsertLink,
            Self::CodeBlock,
            Self::InsertTable,
            Self::ToggleQuote,
            Self::PromoteHeading,
            Self::DemoteHeading,
            Self::Indent,
            Self::Outdent,
            Self::ToggleTask,
            Self::TogglePreview,
            Self::ToggleOutline,
            Self::CommandPalette,
            Self::OpenSettings,
            Self::Export,
            Self::FocusMode,
            Self::CloseOverlay,
            Self::DocumentStats,
            Self::ToggleSidebar,
            Self::FindInFiles,
            Self::SaveAll,
            Self::InsertSymbol,
            Self::CompareDiff,
            Self::NextTab,
            Self::PrevTab,
        ]
    }

    /// 稳定 `snake_case` 键（跨层传递用）。
    ///
    /// 与 [`Self::label`] 的区别：label 是**文案**（会随产品打磨调整），
    /// key 是**契约**（改了要同步改配置里已存盘的绑定）。
    /// 表现层必须按 key 匹配动作，用 label 当键会让「改个菜单措辞」静默改掉快捷键归属。
    pub const fn key(&self) -> &'static str {
        match self {
            Self::NewDocument => "new_document",
            Self::OpenDocument => "open_document",
            Self::Save => "save",
            Self::SaveAs => "save_as",
            Self::SaveAll => "save_all",
            Self::CloseTab => "close_tab",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Find => "find",
            Self::Replace => "replace",
            Self::FindInFiles => "find_in_files",
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Strikethrough => "strikethrough",
            Self::InlineCode => "inline_code",
            Self::InsertLink => "insert_link",
            Self::CodeBlock => "code_block",
            Self::InsertTable => "insert_table",
            Self::ToggleQuote => "toggle_quote",
            Self::PromoteHeading => "promote_heading",
            Self::DemoteHeading => "demote_heading",
            Self::Indent => "indent",
            Self::Outdent => "outdent",
            Self::ToggleTask => "toggle_task",
            Self::TogglePreview => "toggle_preview",
            Self::ToggleOutline => "toggle_outline",
            Self::ToggleSidebar => "toggle_sidebar",
            Self::ToggleStatusbar => "toggle_statusbar",
            Self::CommandPalette => "command_palette",
            Self::OpenSettings => "open_settings",
            Self::Export => "export",
            Self::FocusMode => "focus_mode",
            Self::CloseOverlay => "close_overlay",
            Self::DocumentStats => "document_stats",
            Self::InsertSymbol => "insert_symbol",
            Self::CompareDiff => "compare_diff",
            Self::NextTab => "next_tab",
            Self::PrevTab => "prev_tab",
        }
    }

    /// 中文显示名（供菜单与命令面板共用）。
    pub const fn label(&self) -> &'static str {
        match self {
            Self::NewDocument => "新建文档",
            Self::OpenDocument => "打开文档",
            Self::Save => "保存",
            Self::SaveAs => "另存为",
            Self::CloseTab => "关闭标签页",
            Self::Undo => "撤销",
            Self::Redo => "重做",
            Self::Find => "查找",
            Self::Replace => "替换",
            Self::Bold => "加粗",
            Self::Italic => "斜体",
            Self::Strikethrough => "删除线",
            Self::InlineCode => "行内代码",
            Self::InsertLink => "插入链接",
            Self::CodeBlock => "代码块",
            Self::InsertTable => "插入表格",
            Self::ToggleQuote => "切换引用",
            Self::PromoteHeading => "提升标题级别",
            Self::DemoteHeading => "降低标题级别",
            Self::Indent => "缩进",
            Self::Outdent => "反缩进",
            Self::ToggleTask => "切换任务勾选",
            Self::TogglePreview => "切换预览",
            Self::ToggleOutline => "切换大纲",
            Self::CommandPalette => "命令面板",
            Self::OpenSettings => "设置",
            Self::Export => "导出",
            Self::FocusMode => "专注模式",
            Self::CloseOverlay => "关闭浮层",
            Self::DocumentStats => "文档统计",
            Self::ToggleSidebar => "折叠 / 展开侧边栏",
            Self::ToggleStatusbar => "显示 / 隐藏状态栏",
            Self::FindInFiles => "在所有文件中查找",
            Self::SaveAll => "保存全部",
            Self::InsertSymbol => "插入符号",
            Self::CompareDiff => "对比文件差异",
            Self::NextTab => "下一个标签页",
            Self::PrevTab => "上一个标签页",
        }
    }
}

/// 一条快捷键绑定。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct ShortcutBinding {
    action: ActionId,
    combo: KeyCombo,
}

impl ShortcutBinding {
    /// 构造绑定。
    pub const fn new(action: ActionId, combo: KeyCombo) -> Self {
        Self { action, combo }
    }

    /// 由字符串组合构造（配置反序列化路径）。
    pub fn parse(
        action: ActionId,
        raw: &str,
    ) -> Result<Self, crate::domain::preferences::errors::PreferencesError> {
        Ok(Self::new(action, KeyCombo::parse(raw)?))
    }

    /// 动作。
    pub const fn action(&self) -> ActionId {
        self.action
    }

    /// 组合键。
    pub const fn combo(&self) -> KeyCombo {
        self.combo
    }
}

/// 冲突类型。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ConflictKind {
    /// 同一个组合被绑定到多个动作（原型缺陷 D）。
    ///
    /// 严重：触发时行为不确定，必须提示用户。
    Duplicate,
    /// 该组合被操作系统或宿主环境占用。
    Reserved,
    /// 无修饰键的字符键，会与文本输入抢按键（提示级）。
    TextInputClash,
}

impl ConflictKind {
    /// 是否为阻断级冲突（`Duplicate` / `Reserved`）。
    pub const fn is_blocking(&self) -> bool {
        matches!(self, Self::Duplicate | Self::Reserved)
    }

    /// 说明文案（可直接呈现给用户）。
    pub const fn description(&self) -> &'static str {
        match self {
            Self::Duplicate => "该快捷键被绑定到了多个动作，触发时行为不确定",
            Self::Reserved => "该快捷键被操作系统占用，可能无法生效",
            Self::TextInputClash => "无修饰键的字母键会与文本输入冲突，建议增加修饰键",
        }
    }
}

/// 一条快捷键冲突。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Conflict {
    kind: ConflictKind,
    combo: KeyCombo,
    /// 涉及的动作（重复冲突时会有多个）。
    actions: Vec<ActionId>,
}

impl Conflict {
    /// 构造冲突。
    pub fn new(kind: ConflictKind, combo: KeyCombo, actions: Vec<ActionId>) -> Self {
        Self {
            kind,
            combo,
            actions,
        }
    }

    /// 冲突类型。
    pub const fn kind(&self) -> ConflictKind {
        self.kind
    }

    /// 冲突的组合键。
    pub const fn combo(&self) -> KeyCombo {
        self.combo
    }

    /// 涉及的动作。
    pub fn actions(&self) -> &[ActionId] {
        &self.actions
    }

    /// 人类可读描述（含组合与动作名）。
    pub fn describe(&self) -> String {
        let names: Vec<&str> = self.actions.iter().map(|a| a.label()).collect();
        format!(
            "{}：{}（{}）",
            self.combo.display(),
            self.kind.description(),
            names.join("、")
        )
    }
}

/// 快捷键表。
///
/// **唯一真相源**：菜单、命令面板、快捷键一览全部从本表读取显示文本
/// （原型缺陷 C 的根治）。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ShortcutTable {
    bindings: Vec<ShortcutBinding>,
}

/// 被操作系统占用的组合（跨平台保守取值）。
pub const RESERVED_COMBOS: &[&str] = &["Meta+Q", "Meta+W", "Alt+F4", "Ctrl+Alt+Delete"];

impl ShortcutTable {
    /// 由绑定列表构造。
    pub fn new(bindings: Vec<ShortcutBinding>) -> Self {
        Self { bindings }
    }

    /// 默认绑定表。
    ///
    /// 键位选择对齐原型 `mock/shortcuts.ts`，有两条必须如此：
    /// - `Ctrl+Shift+S` 归 [`ActionId::DocumentStats`]（原型的「文档统计」）。
    ///   领域层早期把它给了 [`ActionId::SaveAs`]，导致按验收用例②把统计改绑到
    ///   `Ctrl+Shift+S` 时被当成重复绑定而**拒绝**——用例根本走不通。
    /// - `F8` / `Esc` 必须有默认项。二者都是**无修饰键单键**，原型正是在这里
    ///   的匹配器把它们全拒了（B-01 / B-03），导致「声明了却永不触发」。
    ///
    /// 全表保证两两不重复，且不触碰 [`RESERVED_COMBOS`]。
    pub fn default_bindings() -> Result<Self, crate::domain::preferences::errors::PreferencesError>
    {
        use crate::domain::preferences::errors::PreferencesError;

        let raw: &[(ActionId, &str)] = &[
            // —— 文件 ——
            (ActionId::NewDocument, "Ctrl+N"),
            (ActionId::OpenDocument, "Ctrl+O"),
            (ActionId::Save, "Ctrl+S"),
            (ActionId::SaveAll, "Ctrl+Alt+S"),
            (ActionId::SaveAs, "Ctrl+Alt+Shift+S"),
            (ActionId::CloseTab, "Ctrl+Alt+W"),
            (ActionId::Export, "Ctrl+E"),
            // —— 编辑 ——
            (ActionId::Undo, "Ctrl+Z"),
            (ActionId::Redo, "Ctrl+Shift+Z"),
            (ActionId::Find, "Ctrl+F"),
            (ActionId::Replace, "Ctrl+H"),
            (ActionId::FindInFiles, "Ctrl+Shift+F"),
            // —— 格式 ——
            (ActionId::Bold, "Ctrl+B"),
            (ActionId::Italic, "Ctrl+I"),
            (ActionId::Strikethrough, "Ctrl+Shift+X"),
            (ActionId::InlineCode, "Ctrl+Shift+C"),
            (ActionId::InsertLink, "Ctrl+K"),
            (ActionId::CodeBlock, "Ctrl+Shift+K"),
            (ActionId::InsertTable, "Ctrl+Shift+T"),
            (ActionId::ToggleQuote, "Ctrl+Shift+Q"),
            (ActionId::PromoteHeading, "Ctrl+="),
            (ActionId::DemoteHeading, "Ctrl+-"),
            (ActionId::Indent, "Ctrl+]"),
            (ActionId::Outdent, "Ctrl+["),
            (ActionId::ToggleTask, "Ctrl+Shift+Enter"),
            // —— 视图 ——
            (ActionId::TogglePreview, "Ctrl+P"),
            (ActionId::ToggleOutline, "Ctrl+Shift+O"),
            (ActionId::ToggleSidebar, "Ctrl+\\"),
            (ActionId::DocumentStats, "Ctrl+Shift+S"),
            (ActionId::CommandPalette, "Ctrl+Shift+P"),
            (ActionId::InsertSymbol, "Ctrl+Shift+E"),
            (ActionId::CompareDiff, "Ctrl+Alt+D"),
            (ActionId::NextTab, "Ctrl+Shift+]"),
            (ActionId::PrevTab, "Ctrl+Shift+["),
            (ActionId::OpenSettings, "Ctrl+,"),
            // —— 无修饰键单键（原型在此处失效，见B-01 / B-03）——
            (ActionId::FocusMode, "F8"),
            (ActionId::CloseOverlay, "Esc"),
        ];
        let mut bindings = Vec::with_capacity(raw.len());
        for (action, combo) in raw {
            let parsed = KeyCombo::parse(combo).map_err(|_| PreferencesError::InvalidCombo {
                raw: combo.to_string(),
            })?;
            bindings.push(ShortcutBinding::new(*action, parsed));
        }
        Ok(Self::new(bindings))
    }

    /// 全部绑定。
    pub fn bindings(&self) -> &[ShortcutBinding] {
        &self.bindings
    }

    /// 查询某动作当前绑定的组合。
    pub fn binding_for(&self, action: ActionId) -> Option<KeyCombo> {
        self.bindings
            .iter()
            .find(|binding| binding.action() == action)
            .map(|binding| binding.combo())
    }

    /// 查询某组合触发的动作；未绑定时返回 `None`。
    pub fn action_for(&self, combo: KeyCombo) -> Option<ActionId> {
        self.bindings
            .iter()
            .find(|binding| binding.combo() == combo)
            .map(|binding| binding.action())
    }

    /// 重绑定某动作（已存在则替换，不存在则追加）。
    ///
    /// **不拒绝重复组合**——重复属于「可提示的冲突」，由 [`ShortcutTable::conflicts`]
    /// 统一报告（原型缺陷 D 的根治：判定集中在一处，而不是散落在 UI 里）。
    pub fn rebind(&mut self, action: ActionId, combo: KeyCombo) {
        match self
            .bindings
            .iter_mut()
            .find(|binding| binding.action() == action)
        {
            Some(binding) => *binding = ShortcutBinding::new(action, combo),
            None => self.bindings.push(ShortcutBinding::new(action, combo)),
        }
    }

    /// 恢复某动作为默认绑定（用户自定义后想还原时使用）。
    pub fn reset_action(
        &mut self,
        action: ActionId,
    ) -> Result<(), crate::domain::preferences::errors::PreferencesError> {
        let defaults = Self::default_bindings()?;
        match defaults.binding_for(action) {
            Some(combo) => {
                self.rebind(action, combo);
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// 检测全部冲突。
    ///
    /// 判定规则：
    /// 1. 同一组合绑定到 ≥2 个动作 → [`ConflictKind::Duplicate`]；
    /// 2. 组合属于 [`RESERVED_COMBOS`] → [`ConflictKind::Reserved`]；
    /// 3. 无修饰键的**字符键** → [`ConflictKind::TextInputClash`]（提示级，
    ///    功能键如 `Escape` / `F5` **不算**冲突——原型缺陷 A 的根治）。
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut out = Vec::new();

        let mut seen: Vec<(KeyCombo, Vec<ActionId>)> = Vec::new();
        for binding in &self.bindings {
            if let Some(entry) = seen.iter_mut().find(|(combo, _)| *combo == binding.combo()) {
                entry.1.push(binding.action());
            } else {
                seen.push((binding.combo(), vec![binding.action()]));
            }
        }
        for (combo, actions) in seen {
            let is_duplicate = actions.len() > 1;
            let displayed = combo.display();
            if is_duplicate {
                out.push(Conflict::new(
                    ConflictKind::Duplicate,
                    combo,
                    actions.clone(),
                ));
            }
            // 注意：重复冲突与保留/文本输入冲突可以叠加报出，故此处一律使用克隆，
            // 避免把 `actions` 移进第一个 `Conflict` 后无法复用。
            if RESERVED_COMBOS
                .iter()
                .any(|reserved| reserved.eq_ignore_ascii_case(&displayed))
            {
                out.push(Conflict::new(
                    ConflictKind::Reserved,
                    combo,
                    actions.clone(),
                ));
            }
            if combo.clashes_with_text_input() {
                out.push(Conflict::new(ConflictKind::TextInputClash, combo, actions));
            }
        }
        out
    }

    /// 是否只存在提示级冲突（没有阻断级冲突）。
    pub fn is_usable(&self) -> bool {
        !self.conflicts().iter().any(|c| c.kind().is_blocking())
    }

    /// 生成「快捷键一览」所需的全部条目（动作 + 规范显示文本）。
    ///
    /// 菜单与命令面板**必须**复用本函数的输出，避免第二处显示逻辑。
    pub fn display_all(&self) -> Vec<(ActionId, String)> {
        self.bindings
            .iter()
            .map(|binding| (binding.action(), binding.combo().display()))
            .collect()
    }
}

impl Default for ShortcutTable {
    fn default() -> Self {
        Self::default_bindings().unwrap_or_else(|_| Self::new(Vec::new()))
    }
}
