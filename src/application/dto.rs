//! 应用层 DTO（阶段 04 · S4-01）。
//!
//! 表现层**唯一可见**的数据结构。硬性边界：
//! - 不含任何聚合根类型（`Document` / `Workspace` / `Preferences` / `ExportJob`）；
//! - 字段均为可序列化标量或其他 DTO，不持有 `Arc<Mutex<>>` 之类的内部可变共享；
//! - 全部 `Clone + Debug + Serialize`；仅 [`SettingsView`] 需要 `Deserialize`
//!   （设置页面回写）。

use serde::{Deserialize, Serialize};

use crate::domain::preferences::value_objects::CustomThemePalette;
use crate::shared::ids::{DocumentId, JobId};
use crate::shared::path::FilePath;

/// 文本区间（字节偏移，左闭右开），DTO 版本（与领域 [`crate::shared::geometry::TextRange`] 对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RangeView {
    /// 起点字节偏移。
    pub start: usize,
    /// 终点字节偏移（不含）。
    pub end: usize,
}

impl RangeView {
    /// 构造区间。
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// 区间长度（字节）。
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// 是否为空区间。
    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }
}

/// 单个打开文档的视图。
#[derive(Clone, Debug, Serialize)]
pub struct DocumentView {
    /// 文档 ID。
    pub id: DocumentId,
    /// 标题（文件名去扩展名，或「未命名」）。
    pub title: String,
    /// 保存路径；未保存过的新文档为 `None`。
    pub path: Option<FilePath>,
    /// 内容预览（前若干字符，用于标签页 tooltip / 欢迎页）。
    pub content_preview: String,
    /// 是否存在未保存修改。
    pub is_dirty: bool,
    /// 光标字节偏移。
    pub cursor: usize,
    /// 选区（无选区为 `None`）。
    pub selection: Option<RangeView>,
    /// 行数。
    pub line_count: usize,
    /// 字节长度。
    pub byte_len: usize,
}

/// 文档字数统计视图（字段对齐原型 `StatsDialog.vue`）。
///
/// # 口径必须与原型逐项一致
///
/// 状态栏的字数与统计浮层都直接显示这些数字，用户会拿它跟原型比。
/// 口径不一致不是「实现细节差异」而是**可见的错误**：原型里
/// 「这是一段中文 mixed with English」算 6 + 6 = 12 字，
/// 按空白切分只算 6 —— 差一倍。
#[derive(Clone, Debug, Serialize)]
pub struct StatsView {
    /// 字数：**中日韩按字 + 西文按词**（原型的 `countWords`）。
    ///
    /// 不能用 `split_whitespace().count()`：中文整段无空白，
    /// 一整篇中文会被算成 1 个「词」。
    pub words: usize,
    /// 字符数（含空格，Unicode 标量值计数）。
    pub characters: usize,
    /// 字符数（不含任何空白）。
    pub characters_no_space: usize,
    /// 行数。
    pub lines: usize,
    /// 段落数（非空段落：被空行分隔的连续非空行各算一段）。
    pub paragraphs: usize,
    /// 标题数（`#` ~ `######`，围栏代码块**内**的不计）。
    pub headings: usize,
    /// 围栏代码块数。
    pub code_blocks: usize,
    /// 链接数（`[文字](地址)`，**不含**图片）。
    pub links: usize,
    /// 图片数（`![说明](地址)`）。
    pub images: usize,
    /// 标题。
    pub title: String,
    /// 已完成任务数（`- [x]`）。
    pub tasks_done: usize,
    /// 任务总数（`- [ ]` + `- [x]`）。
    pub tasks_total: usize,
    /// 预计阅读时长（分钟）。
    ///
    /// 原型口径 `max(1, round(words / 300))`：中文文档普遍读得慢
    /// （300 字/分是偏快的估计），但**下限为 1** —— 一篇只有几个字
    /// 的草稿显示「约 0 分钟」比显示「约 1 分钟」更让人困惑。
    pub reading_minutes: u32,
}

/// 大纲条目视图。
#[derive(Clone, Debug, Serialize)]
pub struct OutlineItemView {
    /// 标题级别（1–6）。
    pub level: u8,
    /// 标题文本（已去 `#` 标记）。
    pub text: String,
    /// 源码行号（0-based）。
    pub line: usize,
}

/// 大纲视图（标题列表）。
#[derive(Clone, Debug, Serialize)]
pub struct OutlineView(pub Vec<OutlineItemView>);

/// 跨文件搜索命中视图。
#[derive(Clone, Debug, Serialize)]
pub struct SearchHitView {
    /// 命中所在文档 ID（磁盘文件由路径派生稳定 ID）。
    pub file_id: DocumentId,
    /// 命中所在路径。
    pub path: FilePath,
    /// 命中所在行（0-based）。
    pub line: usize,
    /// 命中所在列（0-based）。
    pub col: usize,
    /// 命中上下文片段（含前后若干字符）。
    pub snippet: String,
    /// 命中区间（相对整篇内容的字节偏移）。
    pub match_range: RangeView,
}

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

/// 表格单元格的水平对齐方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub enum CellAlignView {
    /// 未指定。
    None,
    /// 左对齐。
    Left,
    /// 居中。
    Center,
    /// 右对齐。
    Right,
}

/// 预览用表格结构。
#[derive(Clone, Debug, Serialize)]
pub struct TableView {
    /// 表头单元格。
    pub header: Vec<String>,
    /// 表体各行。
    pub rows: Vec<Vec<String>>,
    /// 每列对齐方式（长度对应 `header`）。
    pub aligns: Vec<CellAlignView>,
    /// 表头各单元格的行内样式片段（与 `header` 同形；区间相对该单元格文本）。
    ///
    /// 与 [`TableView::header`] 并列而不是塞进一个结构体：单元格文本早已被
    /// 多处调用方按 `Vec<String>` 读取（导出、搜索、测试断言），
    /// 换成 `Vec<Cell>` 会波及所有调用点；并行的片段表是**增量**改动。
    pub header_spans: Vec<Vec<SpanView>>,
    /// 表体各行各单元格的行内样式片段（与 `rows` 同形）。
    pub rows_spans: Vec<Vec<Vec<SpanView>>>,
}

/// 预览用列表项。
#[derive(Clone, Debug, Serialize)]
pub struct ListItemView {
    /// 该项纯文本（不含任务标记）。
    pub text: String,
    /// 勾选状态：`None` 表示普通列表项。
    pub checked: Option<bool>,
    /// 该项在源码中的字节区间起点。
    pub start: usize,
    /// 该项在源码中的字节区间终点（不含）。
    pub end: usize,
    /// 缩进层级（顶层为 `0`；嵌套列表按层递增）。
    ///
    /// **必须由解析层给出**：pulldown 的事件流里嵌套深度只体现为
    /// `List` 标签的开闭层次，块一旦被"拍平"成单个列表块（见解析器的
    /// `root_list_index`），层级信息就只存在于这个字段里了。
    /// 渲染层若想用「文本前导空格数」反推，会在 `1.` 与 `-` 混排时算错。
    pub depth: u8,
    /// 有序项的编号；无序项为 `None`。
    ///
    /// 不自己数序号的原因：有序列表可从任意数字起（`3.` 开头），
    /// 且中间可能被嵌套列表打断，渲染层从 1 开始数会与源文不符。
    pub ordinal: Option<u64>,
    /// 该项文本的行内样式片段（区间相对 [`Self::text`]）。
    pub spans: Vec<SpanView>,
}

/// 定义列表的**单条定义正文**视图。
#[derive(Clone, Debug, Serialize)]
pub struct DefinitionBodyView {
    /// 定义正文文本。
    pub text: String,
    /// 正文的行内样式片段（区间相对 `text`）。
    pub spans: Vec<SpanView>,
}

/// 定义列表的**一个词条**视图（一个术语 + 若干条定义）。
#[derive(Clone, Debug, Serialize)]
pub struct DefinitionEntryView {
    /// 术语（`术语` 行）。
    pub term: String,
    /// 术语的行内样式片段（区间相对 `term`）。
    pub term_spans: Vec<SpanView>,
    /// 该术语下的各条定义（`: 定义` 行，可多条）。
    pub definitions: Vec<DefinitionBodyView>,
}

/// 渲染块视图（预览区的单个块）。
///
/// # 为什么不是枚举
///
/// 领域层的 [`crate::domain::markdown::value_objects::BlockKind`] 是带载荷的枚举，
/// 但 DTO 用 **`kind` 字符串 + 可选字段**表达同样的信息：
/// 表现层按 `kind` 分发渲染器，再按需要读取对应字段。
/// 这样既能承载表格 / 列表结构，又不会把领域类型引进表现层（`check-arch` 的硬约束）。
#[derive(Clone, Debug, Serialize)]
pub struct BlockView {
    /// 块 ID（单次解析内唯一）。
    pub id: u64,
    /// 块类型（如 `heading` / `paragraph` / `code_block` / `table` / `task_list`）。
    pub kind: String,
    /// 块纯文本内容。
    pub content: String,
    /// 标题级别（1–6；`kind == "heading"` 时有值）。
    pub level: Option<u8>,
    /// 代码块语言（`kind == "code_block"` 且源码写了语言时有值）。
    pub lang: Option<String>,
    /// 列表是否有序（`kind` 为 `list` / `task_list` 时有意义）。
    pub ordered: bool,
    /// 列表项（`kind` 为 `list` / `task_list` 时有值）。
    pub items: Vec<ListItemView>,
    /// 表格结构（`kind == "table"` 时有值）。
    pub table: Option<TableView>,
    /// 图片替代文本（`kind == "image"` 时有值）。
    pub alt: Option<String>,
    /// 图片地址（本地路径 / 远程 URL / data URI；`kind == "image"` 时有值）。
    pub url: Option<String>,
    /// 未支持扩展块的标记（`kind` 形如 `raw:html`）。
    pub marker: Option<String>,
    /// 是否块级公式（`kind == "math"` 时有值；`true` 为 `$$...$$`，`false` 为 `$...$`）。
    pub display: Option<bool>,
    /// 图表引擎名（`kind == "diagram"` 时有值，当前恒为 `"mermaid"`）。
    pub diagram: Option<String>,
    /// 脚注标签（`kind == "footnote"` 时有值）。
    ///
    /// 解析器把 `[^1]: 内容` 收成一个 `footnote` 块，**正文放在 `content`**，
    /// 标签单列在此：渲染时要画成「`1.` 正文」而不是把标签再拼回正文里
    /// （正文里本来只有 `内容`，不含 `[^1]`）。
    pub label: Option<String>,
    /// 定义列表词条（`kind == "definition_list"` 时有值）。
    pub definitions: Vec<DefinitionEntryView>,
    /// 块内行内样式片段（区间相对 `content`；无样式时为空）。
    ///
    /// 复用 [`SpanView`] 的形状（区间 + 样式字符串）：它与编辑区高亮的
    /// 数据结构同形，表现层只需一套「按区间切段渲染」的代码。
    pub spans: Vec<SpanView>,
}

/// 渲染结果视图（预览区整体）。
#[derive(Clone, Debug, Serialize)]
pub struct RenderedView {
    /// 渲染块序列。
    pub blocks: Vec<BlockView>,
    /// 对应的源文档版本号。
    pub version: u64,
    /// 是否因源过大而截断（UI 据此降级）。
    pub truncated: bool,
}

/// 语法高亮片段视图。
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct SpanView {
    /// 片段字节区间（相对被高亮文本起点）。
    pub range: RangeView,
    /// 样式类别（如 `keyword` / `string` / `heading`）。
    pub style: String,
}

/// 导出结果视图。
#[derive(Clone, Debug, Serialize)]
pub struct ExportResultView {
    /// 导出任务 ID。
    pub job_id: JobId,
    /// 输出文件路径。
    pub output_path: FilePath,
    /// 输出字节数（**产出文件**的大小，不是源文长度）。
    ///
    /// `0` 表示「大小未知」：PDF 由系统打印对话框产出，本进程拿不到最终大小
    /// （用户可在对话框里改纸张与缩放）。见 `Exporter::export`。
    pub bytes: usize,
    /// 导出格式（中文名，如 `HTML`）。
    pub format: String,
}

/// 最近文件视图。
#[derive(Clone, Debug, Serialize)]
pub struct RecentFileView {
    /// 路径。
    pub path: FilePath,
    /// 最近打开时间（UNIX 毫秒）。
    pub last_opened: u64,
}

/// 文件树条目视图。
#[derive(Clone, Debug, Serialize)]
pub struct FileEntryView {
    /// 路径。
    pub path: FilePath,
    /// 是否为目录。
    pub is_directory: bool,
}

/// 标签页视图。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub struct TabView {
    /// 文档 ID。
    pub document_id: DocumentId,
}

/// 跨文件搜索结果视图。
#[derive(Clone, Debug, Serialize)]
pub struct SearchResultView {
    /// 命中列表（已按上限裁剪）。
    pub hits: Vec<SearchHitView>,
    /// 是否因超过总命中上限而被截断。
    pub truncated: bool,
}

/// 关闭文档的决策视图（供表现层决定是否弹保存对话框）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub enum CloseDecisionView {
    /// 文档已干净，可直接关闭。
    Closed,
    /// 文档有未保存修改，需先询问保存。
    NeedsSave,
}

/// 导出请求视图（表现层提交的导出参数）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportRequestView {
    /// 导出格式（`html` / `markdown` / `pdf` / `image`）。
    pub format: String,
    /// 是否生成目录。
    pub include_toc: bool,
    /// 是否内联图片为 data URI。
    pub embed_images: bool,
    /// 主题名。
    pub theme: String,
    /// 页面尺寸（`a4` / `letter`）。
    pub page_size: String,
    /// 是否分页（仅 PDF 有意义）。
    pub paginate: bool,
}

/// 快捷键冲突视图。
#[derive(Clone, Debug, Serialize)]
pub struct ConflictView {
    /// 冲突类型说明（含组合与动作名）。
    pub description: String,
    /// 冲突的组合键规范显示文本。
    pub combo: String,
    /// 涉及的动作显示名。
    pub actions: Vec<String>,
}

impl CloseDecisionView {
    /// 是否允许直接关闭。
    pub const fn can_close(self) -> bool {
        matches!(self, Self::Closed)
    }
}

/// 格式指令执行结果（S6-07）。
///
/// 表现层拿不到 `EditPlan`（`check-arch` 禁止 presentation 依赖 domain），
/// 故由应用层把「编辑 + 选区」一并折算成 DTO。
///
/// # 为什么返回**整篇文本**而不是「区间 + 替换串」
///
/// 二者等价，但 `TextEdit` 每帧都要整篇文本。若 DTO 只给区间，
/// 表现层还得再做一次「拼接 + 重算字符偏移」，那处换算既冗余又是最容易
/// 出错的地方（多字节边界）。让应用层一次算干净，表现层只做赋值。
#[derive(Clone, Debug)]
pub struct FormatResultView {
    /// 编辑后的整篇文本。
    pub text: String,
    /// 编辑完成后应选中的区间（**字节偏移**，与 `RangeView` 同语义）。
    ///
    /// 字节而非字符：应用层不知道 UI 用 egui，故不替它选坐标系；
    /// 表现层按需用 `char_to_byte` 的逆变换换算。
    pub selection: RangeView,
    /// 本次指令是否真的改变了内容（`false` 表示无需重排高亮与预览）。
    pub changed: bool,
}
