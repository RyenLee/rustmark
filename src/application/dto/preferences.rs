//! 偏好设置上下文的 DTO：主题 / 编辑器设置 / lint / 写作目标 / 快捷键 / 设置总视图。

use serde::{Deserialize, Serialize};

use crate::domain::preferences::value_objects::CustomThemePalette;

/// 自定义主题调色板视图（与领域 [`crate::domain::preferences::value_objects::CustomThemePalette`] 对应）。
///
/// 颜色以 `#RRGGBB` 十六进制字符串表达（与领域层一致，避免在此引入 `egui` 依赖）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomThemePaletteView {
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
    /// 语法高亮：标题（`None` = 跟随内置主题，V11-09）。
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

impl Default for CustomThemePaletteView {
    fn default() -> Self {
        // 以内置深色主题的核心色作为起点，保证首次切换到「自定义」时配色协调。
        // 语法色一律 `None`（跟随主题）。
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

impl From<CustomThemePalette> for CustomThemePaletteView {
    fn from(p: CustomThemePalette) -> Self {
        Self {
            bg_primary: p.bg_primary,
            bg_secondary: p.bg_secondary,
            text_primary: p.text_primary,
            text_secondary: p.text_secondary,
            accent: p.accent,
            code_bg: p.code_bg,
            border: p.border,
            selection_bg: p.selection_bg,
            syn_heading: p.syn_heading,
            syn_bold: p.syn_bold,
            syn_italic: p.syn_italic,
            syn_code: p.syn_code,
            syn_link: p.syn_link,
            syn_quote: p.syn_quote,
            syn_marker: p.syn_marker,
            syn_meta: p.syn_meta,
            syn_table: p.syn_table,
            syn_other: p.syn_other,
        }
    }
}

impl From<CustomThemePaletteView> for CustomThemePalette {
    fn from(p: CustomThemePaletteView) -> Self {
        Self {
            bg_primary: p.bg_primary,
            bg_secondary: p.bg_secondary,
            text_primary: p.text_primary,
            text_secondary: p.text_secondary,
            accent: p.accent,
            code_bg: p.code_bg,
            border: p.border,
            selection_bg: p.selection_bg,
            syn_heading: p.syn_heading,
            syn_bold: p.syn_bold,
            syn_italic: p.syn_italic,
            syn_code: p.syn_code,
            syn_link: p.syn_link,
            syn_quote: p.syn_quote,
            syn_marker: p.syn_marker,
            syn_meta: p.syn_meta,
            syn_table: p.syn_table,
            syn_other: p.syn_other,
        }
    }
}

/// 主题设置视图（与领域 `Theme` 对应，枚举以字符串表达以解耦）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThemeView {
    /// 主题种类：`light` / `dark` / `system` / `custom`。
    pub kind: String,
    /// 字体族。
    pub font_family: String,
    /// 字号（px）。
    pub font_size: f32,
    /// 行高倍数。
    pub line_height: f32,
    /// 自定义调色板（仅 `kind == "custom"` 时生效）。
    #[serde(default)]
    pub custom: Option<CustomThemePaletteView>,
}

/// `EditorSettingsView::show_line_numbers` 的 serde 缺省值（显示）。
///
/// 与领域 `EditorSettings::default_settings()` 保持一致：行号栏默认显示，
/// 旧配置文件没有这个字段时也按显示读出。
fn default_show_line_numbers() -> bool {
    true
}

/// 编辑器设置视图（与领域 `EditorSettings` 对应）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditorSettingsView {
    /// 缩进空格数（2 / 4 / 8）。
    pub tab_size: u8,
    /// 是否自动换行。
    pub word_wrap: bool,
    /// 自动保存间隔（秒）；`None` 表示关闭自动保存。
    pub auto_save_interval: Option<u32>,
    /// 是否启用拼写检查。
    pub spell_check: bool,
    /// 是否显示行号栏；与领域默认一致（显示），旧配置缺省同样按显示处理。
    #[serde(default = "default_show_line_numbers")]
    pub show_line_numbers: bool,
    /// 编辑区 / 预览区分割比例（0.2 ~ 0.8）；旧配置缺省时按 `0.5` 处理。
    #[serde(default)]
    pub split_ratio: f32,
    /// 写作目标（字数）；`0` = 未设置（V11-05）。旧配置缺省按 `0` 处理。
    #[serde(default)]
    pub writing_goal: u32,
    /// 格式检查（lint）的规则开关（V11-08）。旧配置缺省按**全部启用**处理。
    #[serde(default)]
    pub lint: LintRulesView,
}

/// 格式检查规则开关视图（V11-08）。
///
/// 与领域 [`crate::domain::preferences::value_objects::LintSettings`] 一一对应；
/// 单独成结构是为了让表现层只消费 DTO（不得依赖 `domain`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct LintRulesView {
    /// 标题层级跳跃。
    pub heading_jump: bool,
    /// 列表符号混用。
    pub list_marker_mix: bool,
    /// 行尾多余空格。
    pub trailing_whitespace: bool,
    /// 未闭合代码围栏。
    pub unclosed_fence: bool,
}

impl Default for LintRulesView {
    fn default() -> Self {
        Self {
            heading_jump: true,
            list_marker_mix: true,
            trailing_whitespace: true,
            unclosed_fence: true,
        }
    }
}

/// 一处格式问题的展示投影（V11-08）。
///
/// 只带**可直接呈现**的字段（规则名、行号、列号、文案）：
/// 表现层拿不到领域类型，跳转所需的行号也必须是 0-based 的裸数字。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LintIssueView {
    /// 规则标识（稳定，可用于图标 / 过滤）。
    pub rule: String,
    /// 规则中文名。
    pub rule_label: String,
    /// 行号（**0-based**，展示时 +1）。
    pub line: usize,
    /// 列号（0-based 字符）。
    pub column: usize,
    /// 说明文案。
    pub message: String,
}

/// 写作目标进度（V11-05）。
///
/// # 为什么要这个 DTO，而不是让状态栏自己算百分比
///
/// 百分比与「是否达成」的算法属于**领域**（[`WritingGoal::progress_percent`]，
/// 含饱和上限与 100% 截断）；而表现层不得依赖 `crate::domain`（分层约束）。
/// 因此由应用层把领域结果投影成这份纯数据，状态栏只负责画。
///
/// 若让表现层自己写一遍 `words * 100 / goal`，就会出现**两份进度算法**：
/// 一处改截断、另一处没改，状态栏显示 137% 而设置页说已达成。
///
/// [`WritingGoal::progress_percent`]: crate::domain::preferences::value_objects::WritingGoal::progress_percent
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GoalProgressView {
    /// 目标字数；`0` = 未设置目标。
    pub goal: u32,
    /// 当前字数。
    pub words: usize,
    /// 进度百分比（0~100）；未设目标为 `None`。
    pub percent: Option<u8>,
    /// 是否已达成；未设目标恒为 `false`。
    pub reached: bool,
}

impl GoalProgressView {
    /// 是否设置了目标；未设置时状态栏**整项隐藏**（画一个 `0%` 的条是噪音）。
    pub const fn is_set(&self) -> bool {
        self.goal > 0
    }
}

/// 单条快捷键视图（菜单 / 命令面板 / 快捷键一览三处共用）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShortcutView {
    /// 动作的**稳定机器标识**（`snake_case`，如 `new_document`）。
    ///
    /// 表现层必须按此字段匹配动作，**不可**用 [`Self::action`]（中文显示名）——
    /// 显示名会随文案调整而变，拿它当键会让「改个菜单措辞」静默改掉快捷键归属。
    pub key: String,
    /// 动作名（中文显示名）。
    pub action: String,
    /// 组合键规范显示文本（如 `Ctrl+Shift+S`）。
    pub combo: String,
}

/// 设置视图（主题 + 编辑器 + 快捷键一览）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettingsView {
    /// 主题设置。
    pub theme: ThemeView,
    /// 编辑器设置。
    pub editor: EditorSettingsView,
    /// 全部快捷键（动作 + 规范显示文本）。
    pub shortcuts: Vec<ShortcutView>,
}

impl SettingsView {
    /// 读取失败时的兜底设置（**仅**供渲染层使用）。
    ///
    /// 存在理由：`get_settings` 读的是**运行期**配置，进程启动后该文件可能
    /// 被外部改坏（另一个实例、手工编辑、同步盘冲突）。此时渲染层需要
    /// 「继续画出一个可用界面」，而**绝不能** panic——
    /// 表现层的 panic 会直接终止整个窗口，用户连「重置配置」的机会都没有。
    ///
    /// 注意与领域 [`crate::domain::preferences::value_objects::Preferences`]
    /// 的默认值是两回事：这里连快捷键列表都是空的，
    /// 表现层据此回退到 `ShortcutRegistry::with_defaults()`，
    /// 仍能得到一份完整的默认键位。
    pub fn fallback() -> Self {
        Self {
            theme: ThemeView {
                kind: "system".to_string(),
                font_family: "monospace".to_string(),
                font_size: 14.0,
                line_height: 1.6,
                custom: None,
            },
            editor: EditorSettingsView {
                tab_size: 4,
                word_wrap: true,
                auto_save_interval: None,
                spell_check: false,
                show_line_numbers: true,
                split_ratio: 0.5,
                writing_goal: 0,
                lint: LintRulesView::default(),
            },
            shortcuts: Vec::new(),
        }
    }
}
