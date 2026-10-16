//! 编辑器行为偏好值对象：[`TabSize`] / [`WritingGoal`] / [`EditorSettings`]。
//!
//! 外观与 lint 开关在 `theme.rs`（[`super::theme::LintSettings`] 是本簇唯一跨簇依赖）。

use serde::{Deserialize, Serialize};

use super::theme::LintSettings;

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

/// `EditorSettings::max_open_documents` 的 serde 缺省值（与默认设置一致）。
///
/// 旧配置文件里没有这个字段，缺了按默认上限读出，而不是让配置加载失败
/// （同 `split_ratio` / `writing_goal` 的处理）。
fn default_max_open_documents() -> u32 {
    MAX_OPEN_DOCUMENTS_DEFAULT
}

/// 反序列化 `max_open_documents`：把 0 收敛到 1。
///
/// # 为什么必须在 serde 入口收敛，而不是留给 `is_valid` 拒绝
///
/// 仓储 `load()` 对「校验失败」的处理是**备份 + 整体回退默认**
/// （见 `TomlPreferencesRepository::load`）——用户手改配置写了个 0，
/// 代价会是整个配置文件作废。0 的含义（「一个都不让开」）也没有意义，
/// 收敛到 1 既保住了其余字段，也给出可用的行为。
///
/// serde 派生会**绕过** `new()` 直接填字段，这是唯一能拦住手改配置的位置。
fn de_max_open_documents<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = u32::deserialize(deserializer)?;
    Ok(if value == 0 { 1 } else { value })
}

/// 打开文档数量的**默认上限**（问题 038：用户要求的标签条限额）。
///
/// 取 12：低于常见「同时对照几份文档」的工作量会碍事；高于此值后标签条
/// 本就靠横向滚动浏览，再大只是让「哪个是哪个」更难认。
pub const MAX_OPEN_DOCUMENTS_DEFAULT: u32 = 12;

/// 打开文档数量的**可选上限**（设置页滑块量程，含端点）。
///
/// 下限 1（「一次只看一份」也是合法用法）；上限 30 与默认值同源——
/// 再大没有意义，且 30 个标签早已超出任何常见窗口一屏放得下的宽度。
pub const MAX_OPEN_DOCUMENTS_MAX: u32 = 30;

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
    /// 打开文档数量上限（问题 038）：标签条里最多同时开几个文档。
    ///
    /// 达上限后**新的打开请求被拒绝**（状态栏提示），已打开的文档不受影响
    /// ——限额管的是「标签数量」，不是「不许切换」。
    ///
    /// `#[serde(default)]`：旧配置文件没有这个字段，缺了按
    /// [`MAX_OPEN_DOCUMENTS_DEFAULT`] 读出（同 `split_ratio`）。
    /// `deserialize_with`：手改配置写成 0 时收敛到 1——0 会让「一个文档都
    /// 打不开」，且若交给 `is_valid` 拒绝，代价是整个配置被回退默认。
    #[serde(
        default = "default_max_open_documents",
        deserialize_with = "de_max_open_documents"
    )]
    max_open_documents: u32,
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
            max_open_documents: MAX_OPEN_DOCUMENTS_DEFAULT,
        }
    }

    /// 构造设置。
    // 参数与 [`EditorSettingsView`] 的字段一一对应，收敛成结构体会让
    // 调用点失去编译期「漏传即错」的保障（同 `find_replace_ui_state` 的先例）。
    #[allow(clippy::too_many_arguments)] // 参数与 EditorSettingsView 字段一一对应；收敛成 struct 会失去编译期「漏传即错」
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
        max_open_documents: u32,
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
            // 原样存入：0 由 `is_valid` 拒绝（显式构造是编程错误，该响铃），
            // 而两条**外部**入口（serde 反序列化、DTO `to_editor`）各自收敛。
            max_open_documents,
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

    /// 打开文档数量上限（问题 038；恒 ≥ 1，构造时已把 0 收敛掉）。
    pub const fn max_open_documents(&self) -> u32 {
        self.max_open_documents
    }

    /// 设置打开文档数量上限，返回新的设置（值对象不可变）。
    ///
    /// 原样存入（0 由 [`Self::is_valid`] 拒绝）：收敛发生在两条外部入口
    /// ——serde 反序列化与 DTO [`crate::application::preferences`] 的
    /// `to_editor`，那里才是「不可信输入」进入领域的位置。
    pub const fn with_max_open_documents(self, max: u32) -> Self {
        Self {
            max_open_documents: max,
            ..self
        }
    }

    /// 校验取值是否合法（自动保存间隔不得为 0——会导致忙循环；
    /// 打开文档上限不得为 0——会导致一个文档都打不开）。
    pub const fn is_valid(&self) -> bool {
        !matches!(self.auto_save_interval, Some(0)) && self.max_open_documents >= 1
    }
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self::default_settings()
    }
}
