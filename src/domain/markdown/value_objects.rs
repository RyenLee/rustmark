//! Markdown 上下文的值对象（S3-09）。
//!
//! # `source_range` 为什么必须存在
//!
//! 每个渲染块都携带**源码字节区间**，这是 `FR-02`（双向滚动同步）与
//! `FR-03`（增量高亮）能否实现的决定因素。若省略，阶段 06 只能靠重新解析
//! 全文来定位，性能目标（`NFR-PERF` 解析 ≤ 5ms）无法达成。
//!
//! # 与 syntect 解耦
//!
//! [`SpanStyle`] 是**自有枚举**，绝不携带 `syntect` 的类型——否则领域层会被
//! 外部库污染（`.agent/architecture.md`：domain 禁止 `syntect`）。

use crate::shared::geometry::TextRange;

/// 渲染块的稳定标识（单次解析结果内唯一）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct BlockId(u64);

impl BlockId {
    /// 由序号构造。
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// 取出序号。
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "block#{}", self.0)
    }
}

/// 表格单元格的水平对齐方式（GFM 分隔行 `:` 决定）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum CellAlignment {
    /// 未指定（按源写作惯例：表头居中、表体左对齐）。
    #[default]
    None,
    /// 左对齐（`:---`）。
    Left,
    /// 居中（`:---:`）。
    Center,
    /// 右对齐（`---:`）。
    Right,
}

/// 列表的一项。
///
/// # 为什么要单独存 `source_range`
///
/// 任务列表的「点击勾选」要回写源码，就必须知道该行在文档里的字节区间——
/// 否则 UI 只能拿着文本去全文里模糊匹配，既慢又会改错同名项。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ListItem {
    /// 该项的纯文本。
    text: String,
    /// 勾选状态：`Some(true)` 已勾选、`Some(false)` 未勾选、`None` 表示普通列表项。
    checked: Option<bool>,
    /// 该项在源码中的字节区间。
    source_range: TextRange,
}

impl ListItem {
    /// 构造列表项。
    pub fn new(text: impl Into<String>, checked: Option<bool>, source_range: TextRange) -> Self {
        Self {
            text: text.into(),
            checked,
            source_range,
        }
    }

    /// 该项的纯文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 勾选状态（`None` 表示非任务项）。
    pub const fn checked(&self) -> Option<bool> {
        self.checked
    }

    /// 该项在源码中的字节区间。
    pub const fn source_range(&self) -> TextRange {
        self.source_range
    }
}

/// GFM 表格的结构化内容。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TableData {
    /// 表头单元格。
    header: Vec<String>,
    /// 表体各行（每行长度可能不足列数——GFM 允许行末缺单元格）。
    rows: Vec<Vec<String>>,
    /// 每列的对齐方式（长度对应 `header`）。
    aligns: Vec<CellAlignment>,
}

impl TableData {
    /// 构造表格数据。
    pub fn new(header: Vec<String>, rows: Vec<Vec<String>>, aligns: Vec<CellAlignment>) -> Self {
        // 对齐数组与列数对齐：GFM 里「列数」以表头为准，缺少的按 `None` 补。
        let mut aligns = aligns;
        aligns.resize(header.len(), CellAlignment::None);
        Self {
            header,
            rows,
            aligns,
        }
    }

    /// 表头单元格。
    pub fn header(&self) -> &[String] {
        &self.header
    }

    /// 表体行。
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// 列对齐方式。
    pub fn aligns(&self) -> &[CellAlignment] {
        &self.aligns
    }

    /// 列数（以表头为准）。
    pub fn columns(&self) -> usize {
        self.header.len()
    }
}

/// 渲染块的类型。
///
/// 覆盖需求文档 §3.1 的 P0 特性；P1（数学公式、Mermaid）已落地为
/// [`BlockKind::Math`] / [`BlockKind::Diagram`]，其余未支持的扩展块仍走
/// [`BlockKind::Raw`]。
///
/// # 为什么列表 / 表格要把结构留在 `BlockKind` 里
///
/// S6-05 的预览要把表格画成网格、把任务列表画成可点击的复选框，
/// 而一个扁平字符串（`"列A列B1234"`）做不到——但结构是**解析期**才知道的，
/// 到表现层已无法还原。故结构性载荷必须由本层持有。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BlockKind {
    /// 标题（1–6 级）。
    Heading { level: u8 },
    /// 段落。
    Paragraph,
    /// 围栏或缩进代码块。
    CodeBlock { lang: Option<String> },
    /// GFM 表格（含表头 / 表体 / 列对齐）。
    Table { data: TableData },
    /// 有序 / 无序列表。
    List {
        /// 是否为有序列表。
        ordered: bool,
        /// 列表项（保序）。
        items: Vec<ListItem>,
    },
    /// GFM 任务列表。
    ///
    /// 与 [`BlockKind::List`] 分开设：任务项的 `checked` 需要参与渲染与回写，
    /// 用一个 `Vec<ListItem>` 承载可以让「勾选状态」成为结构而非文本解析的结果。
    TaskList { items: Vec<ListItem> },
    /// 引用块。
    Quote,
    /// 图片（本地 / 远程 / data URI）。
    Image { alt: String, url: String },
    /// 分割线。
    Rule,
    /// 脚注定义。
    Footnote,
    /// 数学公式（行内或块级）。
    ///
    /// `display` 为 `true` 表示块级公式（`$$...$$`），`false` 表示行内（`$...$`）。
    /// 编辑器内以「结构化源码卡」呈现（零依赖，不内嵌 KaTeX/MathJax 的像素级渲染）；
    /// 导出 HTML 时由 pulldown 输出 `<span class="math">`，交浏览器 / MathJax 渲染。
    Math { display: bool },
    /// 图表（如 Mermaid）。`kind` 标记图表引擎，当前仅 `"mermaid"`。
    ///
    /// ` ```mermaid ` 围栏经解析器识别后落到此处，而非当作普通代码块。
    Diagram { kind: String },
    /// 原始 HTML 等尚未支持的扩展块。
    Raw { marker: String },
}

impl BlockKind {
    /// 是否为可在大纲中显示的标题。
    pub const fn is_heading(&self) -> bool {
        matches!(self, Self::Heading { .. })
    }

    /// 是否为列表（含任务列表）。
    pub const fn is_list(&self) -> bool {
        matches!(self, Self::List { .. } | Self::TaskList { .. })
    }

    /// 标题级别（非标题为 `None`）。
    pub const fn heading_level(&self) -> Option<u8> {
        match self {
            Self::Heading { level } => Some(*level),
            _ => None,
        }
    }

    /// 是否为需要语法高亮的块（代码块）。
    pub const fn is_code(&self) -> bool {
        matches!(self, Self::CodeBlock { .. })
    }

    /// 代码块语言（非代码块为 `None`）。
    pub fn language(&self) -> Option<&str> {
        match self {
            Self::CodeBlock { lang: Some(lang) } => Some(lang.as_str()),
            _ => None,
        }
    }

    /// 列表项（非列表为 `None`）。
    ///
    /// 任务列表与普通列表共用同一份 [`ListItem`]，故两者在这里表现一致。
    pub fn list_items(&self) -> Option<&[ListItem]> {
        match self {
            Self::List { items, .. } | Self::TaskList { items } => Some(items),
            _ => None,
        }
    }

    /// 是否为任务列表。
    pub const fn is_task_list(&self) -> bool {
        matches!(self, Self::TaskList { .. })
    }

    /// 表格数据（`kind` 为表格时有值）。
    pub const fn table(&self) -> Option<&TableData> {
        match self {
            Self::Table { data } => Some(data),
            _ => None,
        }
    }
}

/// 一个渲染完成的块。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RenderedBlock {
    id: BlockId,
    kind: BlockKind,
    /// 该块在**源码**中的字节区间（左闭右开）。
    source_range: TextRange,
    /// 该块的纯文本内容（不含 Markdown 标记），供大纲与搜索使用。
    content: String,
}

impl RenderedBlock {
    /// 构造渲染块。
    pub fn new(
        id: BlockId,
        kind: BlockKind,
        source_range: TextRange,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id,
            kind,
            source_range,
            content: content.into(),
        }
    }

    /// 块 ID。
    pub const fn id(&self) -> BlockId {
        self.id
    }

    /// 块类型。
    pub const fn kind(&self) -> &BlockKind {
        &self.kind
    }

    /// 源码区间（`FR-02` / `FR-03` 的基础）。
    pub const fn source_range(&self) -> TextRange {
        self.source_range
    }

    /// 块内容。
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// 文本片段的样式类别。
///
/// 同时覆盖 Markdown 语义样式与代码语法样式；全部为**自有枚举**，可序列化。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SpanStyle {
    /// 无特殊样式。
    Plain,
    /// 标题（携带级别，便于按级着色）。
    Heading { level: u8 },
    /// 强调（斜体）。
    Emphasis,
    /// 加粗。
    Strong,
    /// 删除线。
    Strikethrough,
    /// 行内代码。
    Code,
    /// 链接文本。
    Link,
    /// 图片标记。
    Image,
    /// 引用。
    Quote,
    /// 列表标记。
    ListMarker,
    /// 表格边框。
    TableBorder,
    /// 任务列表勾选框。
    TaskMarker { done: bool },
    /// 代码：关键字。
    Keyword,
    /// 代码：字符串字面量。
    String,
    /// 代码：注释。
    Comment,
    /// 代码：数字。
    Number,
    /// 代码：函数名。
    Function,
    /// 代码：类型名。
    Type,
    /// 代码：变量名。
    Variable,
    /// 代码：运算符。
    Operator,
}

/// 一个带样式的文本片段。
///
/// `range` 相对于**被高亮的那段文本**（代码块内容或整段源码）的起点，
/// 由调用方负责与全局偏移做换算。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SyntaxSpan {
    range: TextRange,
    style: SpanStyle,
}

impl SyntaxSpan {
    /// 构造样式片段。
    pub fn new(range: TextRange, style: SpanStyle) -> Self {
        Self { range, style }
    }

    /// 片段区间。
    pub const fn range(&self) -> TextRange {
        self.range
    }

    /// 片段样式。
    pub const fn style(&self) -> SpanStyle {
        self.style
    }
}

/// 大纲条目（对应一个标题块）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OutlineItem {
    /// 标题级别（1–6）。
    level: u8,
    /// 标题文本（已去除 `#` 标记）。
    text: String,
    /// 该标题所在的源码行号（0-based）。
    line: usize,
    /// 对应的渲染块。
    block_id: BlockId,
}

impl OutlineItem {
    /// 构造大纲条目。
    pub fn new(level: u8, text: impl Into<String>, line: usize, block_id: BlockId) -> Self {
        Self {
            level,
            text: text.into(),
            line,
            block_id,
        }
    }

    /// 标题级别。
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// 标题文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 源码行号（0-based）。
    pub const fn line(&self) -> usize {
        self.line
    }

    /// 对应块 ID。
    pub const fn block_id(&self) -> BlockId {
        self.block_id
    }
}

/// 一个被判定为拼写错误的词及其位置。
///
/// # 为什么只存位置与原词，不存建议
///
/// 编辑器内的波浪线只需知道「哪里错了」；拼写建议（did you mean...）属于
/// 交互增强，留给后续的右键菜单。**不存建议**是为了让本值对象保持
/// `Copy + Eq`，从而能进 `Vec` 直接比较——渲染层每帧都要比对上一帧的标记集，
/// 若携带 `String` 建议会拖累该比对。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Misspelling {
    /// 拼错的词在原文中的**字节区间**（与 [`RenderedBlock::source_range`] 同坐标系）。
    pub range: TextRange,
}

impl Misspelling {
    /// 构造拼错标记。
    pub const fn new(range: TextRange) -> Self {
        Self { range }
    }
}
