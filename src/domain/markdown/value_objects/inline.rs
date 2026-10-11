//! 行内富文本值对象：[`InlineStyle`] / [`InlineSpan`] / [`SpanStyle`]
//! / [`SyntaxSpan`]。
//!
//! 这里的取值是 **Markdown 语义**（强调、行内代码、键帽……），由解析结果决定，
//! 与表现层的高亮样式词表（`"keyword"` / `"heading"`）刻意分开；
//! [`SpanStyle`] 同时覆盖语义样式与代码语法样式，但都是**自有枚举**，
//! 绝不携带 `syntect` 的类型。

use crate::shared::geometry::TextRange;

/// 行内文本的样式类别（用于预览区的行内富文本渲染）。
///
/// # 为什么要单独建一个类型，而不是复用高亮用的样式字符串
///
/// 高亮的样式字符串（`"keyword"` / `"heading"`）是**表现层**的词表，随主题 /
/// 语法高亮器变化；而这里的取值是 **Markdown 语义**（强调、行内代码、键帽……），
/// 由解析结果决定，与主题无关。领域层持有语义、表现层决定长什么样，
/// 正是分层契约要求的形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum InlineStyle {
    /// `**粗体**` / `__粗体__`。
    Strong,
    /// `*斜体*` / `_斜体_`。
    Emphasis,
    /// `~~删除~~`。
    Strikethrough,
    /// 行内代码 `` `code` ``。
    Code,
    /// 链接文本 `[文本](url)`。
    Link,
    /// 行内公式 `$...$`。
    Math,
    /// HTML 键帽 `<kbd>Ctrl</kbd>`（CSDN / 掘金等编辑器的快捷键写法）。
    Keycap,
    /// 标记文本 `==高亮==`（pulldown 不支持，由解析器自行切分）。
    Mark,
    /// 下标 `H~2~O`。
    Subscript,
    /// 上标 `2^10^`。
    Superscript,
    /// 脚注引用 `[^1]`（片段文本是 `[^1]` 原文，渲染层负责剥语法）。
    ///
    /// 内容里保留 `[^1]` 原文与「行内公式补回 `$`」同一取向：搜索、
    /// 字数统计看到的仍是用户写下的字符；渲染时由表现层换成标号
    /// （见 `preview_view::footnote_ref_mark`），不在此处改写内容。
    FootnoteRef,
}

/// 块内的一段样式化文本（相对块 `content` 的字节区间）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InlineSpan {
    /// 起点字节偏移（相对块 `content`）。
    start: usize,
    /// 终点字节偏移（不含，相对块 `content`）。
    end: usize,
    /// 样式类别。
    style: InlineStyle,
}

impl InlineSpan {
    /// 构造行内片段。
    pub const fn new(start: usize, end: usize, style: InlineStyle) -> Self {
        Self { start, end, style }
    }

    /// 起点。
    pub const fn start(&self) -> usize {
        self.start
    }

    /// 终点（不含）。
    pub const fn end(&self) -> usize {
        self.end
    }

    /// 样式。
    pub const fn style(&self) -> InlineStyle {
        self.style
    }

    /// 片段长度（字节）。
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// 是否为空片段。
    pub const fn is_empty(&self) -> bool {
        self.end <= self.start
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
