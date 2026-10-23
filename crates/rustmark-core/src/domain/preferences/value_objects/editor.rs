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
    pub(crate) const fn spaces(&self) -> usize {
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

/// `EditorSettings::auto_save_interval` 的 serde 缺省值（问题 068）。
///
/// 旧配置文件里没有这个字段。该字段此前**没有** serde 缺省，而 TOML
/// 反序列化对「缺字段」直接报错——仓储 `load()` 的处理是「备份 + 整体回退
/// 默认」，代价是用户其余全部配置一起作废，且默认值里自动保存是开着的，
/// 行为上还算不出错；但只要手改 / 同步盘把这一行弄丢，整个配置就没了。
/// 补上缺省（与 `default_settings()` 的 `Some(30)` 一致）后，缺字段只影响
/// 这一个字段。
fn default_auto_save_interval() -> Option<u32> {
    Some(30)
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
pub(crate) const MAX_OPEN_DOCUMENTS_DEFAULT: u32 = 12;

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
    ///
    /// `#[serde(default = "default_auto_save_interval")]`（问题 068）：
    /// 旧配置 / 被外部改坏的配置缺这个字段时按「默认 30 秒」读出，
    /// 而不是让整份配置反序列化失败、触发「备份 + 回退默认」。
    #[serde(default = "default_auto_save_interval")]
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
    /// 侧栏开合的**持久化决定**（问题 062 引入，问题 064 由 `bool` 改三态）。
    ///
    /// `None` = **用户还没决定过**（默认，界面按收起渲染）；
    /// `Some(true/false)` = 决定过了（用户手动开合，或打开文件夹时自动展开
    /// 落下的）——此后一律尊重这个决定。
    ///
    /// # 为什么必须是三态而不是 `bool`
    ///
    /// 需求两头夹：① 打开文件夹后应自动展开侧栏，让刚打开的树露出来；
    /// ② 「若侧边栏此前被用户手动折叠，则尊重该状态不强制展开」。
    /// `bool` 无法表达「收起」到底是**默认值**还是**用户的选择**——两者都
    /// 是 `false`，于是要么每次打开文件夹都把用户收起的侧栏顶开，要么
    /// 永远不自动展开。`None` 这个第三态专门承载「没决定过」，两条需求
    /// 才能同时成立。
    ///
    /// `#[serde(default)]`：旧配置文件没有这个字段，缺了按 `None`（未决定）
    /// 读出，不让旧配置因缺字段而整体加载失败（同 `split_ratio` 的处理）。
    /// `skip_serializing_if`：`None` 写进 TOML 会变成「值缺失」，直接省掉
    /// 这个键，读回来仍是 `None`——语义闭合。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidebar_visible: Option<bool>,
    /// 侧栏宽度（pt）；`None` = 用户没拖过分割条，按主题默认宽度渲染。
    ///
    /// # 为什么是 `Option` 而不是裸 `f32`
    ///
    /// 「默认宽度」的真源在主题（`Metrics::sidebar_width`），领域层不该
    /// 把 308 这个数字抄一份进来——两处各存一个默认值，改主题尺寸时
    /// 必然只改到一处。`None` 表达「用主题默认」，与 `sidebar_visible`
    /// 的三态同一手法（缺字段 = 用户从未见过这个能力）。
    ///
    /// `#[serde(default)]`：旧配置文件没有这个字段，缺了按 `None` 读出，
    /// 不让旧配置因缺字段而整体加载失败（同 `split_ratio` 的处理）。
    /// `skip_serializing_if`：`None` 不写进 TOML——「没拖过」就不该在
    /// 配置里留一个与主题默认同值的键，读回来语义才闭合。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidebar_width: Option<f32>,
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
            sidebar_visible: None,
            sidebar_width: None,
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
        sidebar_visible: Option<bool>,
        sidebar_width: Option<f32>,
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
            sidebar_visible,
            sidebar_width,
            writing_goal,
            lint,
            // 原样存入：0 由 `is_valid` 拒绝（显式构造是编程错误，该响铃），
            // 而两条**外部**入口（serde 反序列化、DTO `to_editor`）各自收敛。
            max_open_documents,
        }
    }

    /// 缩进宽度。
    pub(crate) const fn tab_size(&self) -> TabSize {
        self.tab_size
    }

    /// 是否自动换行。
    pub(crate) const fn word_wrap(&self) -> bool {
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
    pub(crate) const fn show_line_numbers(&self) -> bool {
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

    /// 侧栏是否展开（`None` 即未决定，按收起渲染）。
    pub const fn sidebar_visible(&self) -> bool {
        matches!(self.sidebar_visible, Some(true))
    }

    /// 持久化的开合决定（三态，见字段文档）。
    ///
    /// 「打开文件夹要不要自动展开侧栏」只能问这里：问
    /// [`Self::sidebar_visible`]（已折叠成 `bool`）会把「默认收起」误判成
    /// 「用户收起」，于是永远不敢自动展开。
    pub const fn sidebar_visible_decision(&self) -> Option<bool> {
        self.sidebar_visible
    }

    /// 用户拖拽定下的侧栏宽度（pt）；`None` = 没拖过，按主题默认宽度渲染。
    ///
    /// 夹紧不在领域层做：合法区间（180~560pt 与「不超过编辑区+预览区
    /// 的 60%」）是**布局**约束，属于表现层（`AppUiState`），与
    /// `split_ratio` 同一分工——领域只存值，读入与写入两侧各自夹紧。
    pub const fn sidebar_width(&self) -> Option<f32> {
        self.sidebar_width
    }

    /// 设置侧栏开合，返回新的设置（值对象不可变）。
    ///
    /// 供「侧栏开合变化 → 落盘」这条链路使用：不新增第二份开合状态，
    /// 持久化的就是 `AppUiState::sidebar_visible` 的当前值。**落成 `Some`**
    /// ——写入即代表这是一次决定，之后不再被自动展开覆盖。
    pub const fn with_sidebar_visible(self, visible: bool) -> Self {
        Self {
            sidebar_visible: Some(visible),
            ..self
        }
    }

    /// 设置侧栏宽度，返回新的设置（值对象不可变）。
    ///
    /// 供「拖拽结束 → 落盘」这条链路使用：写入即代表用户定下了宽度，
    /// 与 `with_sidebar_visible` 同构。**落成 `Some`**——拖过之后的宽度
    /// 就是明确决定，不再跟随主题默认值变化。
    pub const fn with_sidebar_width(self, width: f32) -> Self {
        Self {
            sidebar_width: Some(width),
            ..self
        }
    }

    /// 写作目标（字数）；`0` = 未设置（V11-05）。
    pub const fn writing_goal(&self) -> WritingGoal {
        self.writing_goal
    }

    /// 格式检查（lint）的规则开关（V11-08）。
    pub const fn lint(&self) -> LintSettings {
        self.lint
    }

    /// 打开文档数量上限（问题 038；恒 ≥ 1，构造时已把 0 收敛掉）。
    pub const fn max_open_documents(&self) -> u32 {
        self.max_open_documents
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
