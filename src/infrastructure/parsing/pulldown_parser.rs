//! `pulldown-cmark` 版 Markdown 解析器（S5-02）。
//!
//! # 核心职责：把事件流转成「带源码区间的块序列」
//!
//! 领域层 [`MarkdownParser`] 要求每个 [`RenderedBlock`] 携带真实的
//! `source_range`（字节区间）。这是 `FR-02`（双向滚动同步）与 `FR-03`（增量高亮）
//! 的前提——没有它，表现层只能重解析全文来定位，性能目标（`NFR-PERF-06`
//! 10KB ≤ 5ms）无法达成。
//!
//! pulldown-cmark 的事件流是**流式**的，只有 `into_offset_iter()` 版本才携带
//! `Range<usize>`。本实现按「块级 Start 开栈、对应 End 出栈」配对来推导区间：
//! Start 的偏移是块的**开始**，End 的偏移是块的**结束**。
//!
//! # 为什么必须用栈，不能只记「上一个 Start」
//!
//! 三处让简单写法失效：
//! 1. 列表项可**嵌套**（`- a` 里嵌列表），`Item` 与 `List` 交错开闭；
//! 2. `TableHead` 之后**没有** `TableBody` 标签，表体行直接跟在后面；
//! 3. 标题虽然是自闭合语义，事件流里仍表现为 `Start` + `End` 一对。
//!
//! 故本实现维护一个显式的 [`OpenBlock`] 栈，End 到来时弹出栈顶配对。
//!
//! # 任务列表的定型时机
//!
//! `Tag::Item` 到底是普通列表项还是任务列表项，要看它内部是否出现
//! `Event::TaskListMarker`——该事件在 `End(Item)` **之前**才到达。
//! 因此 Item 在闭合时才能定型，故栈项上挂 `is_task` 标记。

use crate::domain::markdown::errors::ParseError;
use crate::domain::markdown::services::MarkdownParser;
use crate::domain::markdown::value_objects::{
    BlockId, BlockKind, CellAlignment, DefinitionBody, DefinitionEntry, InlineSpan, InlineStyle,
    ListItem, RenderedBlock, TableData,
};
use crate::shared::geometry::{ByteOffset, TextRange};
use pulldown_cmark::{
    CodeBlockKind, DefaultBrokenLinkCallback, Event, HeadingLevel, OffsetIter, Options, Parser,
    Tag, TagEnd,
};

/// 解析器使用的 pulldown-cmark 选项集（对应需求文档 §3.1 的 P0 特性）。
///
/// **GFM 表格 / 任务列表 / 脚注在 pulldown-cmark 中是运行期 `Options` 位flag，
/// 而非 Cargo feature**（已在 ADR-002 记录此事实，极易误解为 feature）。
/// `ENABLE_GFM` 隐含表格与删除线，但**不含**任务列表与脚注，故仍需显式并列。
///
/// 导出器（`pulldown_html_exporter`）直接复用本函数，**这是唯一的选项来源**：
/// 早先两边各写一份，结果预览支持上下标而导出不支持（用户看到「预览有、导出没有」）。
pub fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_GFM
        // 上下标（CSDN / 掘金语法）：`H~2~O`、`2^10^`。
        // 注意 `ENABLE_SUBSCRIPT` 必须显式开——**关着时 `~x~` 会被当作删除线**
        // （pulldown 文档明确写了这个回退），于是 `H~2~O` 会显示成被划掉的 2。
        // 实测 pulldown 只在定界符**前面有空白**时才识别（`H ~2~ O` 能识别、
        // `H~2~O` 不能），故中文习惯写法仍由 `push_markup_aware` 兜底。
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_SUPERSCRIPT
        // 数学公式：`$...$`（行内）/ `$$...$$`（块级）需此选项才会被识别为
        // `Event::InlineMath` / `Event::DisplayMath`（否则按字面文本处理）。
        | Options::ENABLE_MATH
        // 定义列表（`术语` + 缩进的 `: 定义`）。不开时 pulldown 会把两行当成
        // **一个普通段落**，预览里就是「术语 A: 定义 A 的内容」糊在一行——
        // 用户写的层级结构全部丢失。
        | Options::ENABLE_DEFINITION_LIST
}

/// 基于 `pulldown-cmark` 的 Markdown 解析器。
#[derive(Clone, Copy, Debug, Default)]
pub struct PulldownMarkdownParser;

impl PulldownMarkdownParser {
    /// 构造解析器。
    pub fn new() -> Self {
        Self
    }

    /// 按当前选项集创建**带偏移**的 pulldown 解析器。
    ///
    /// 导出器（S5-09）与解析器共用同一份 [`parser_options`]，保证「预览所见」与
    /// 「导出所得」结构一致——若两者选项不同，用户会看到预览有表格而导出没有。
    ///
    /// 返回 [`OffsetIter`] 而非 `Parser`：只有它的事件流携带 `Range<usize>`，
    /// 而 `source_range` 正是 `FR-02` 的基础。
    ///
    /// 注意：调用方若关心**源码区间**，应先把文本交给 [`desugar_image_sizes`]
    /// 预处理再传进来（`parse` 就是这么做的）。
    pub fn raw_parser(text: &str) -> OffsetIter<'_, DefaultBrokenLinkCallback> {
        Parser::new_ext(text, parser_options()).into_offset_iter()
    }
}

/// `=宽x高` 的长度（含前导 `=`，不含尾随的 `)`）；不是该形状时返回 `None`。
///
/// 尺寸后缀是 CSDN 的私有扩展（`![alt](url =30x30)`），不是 Markdown。
fn size_suffix_len(s: &str) -> Option<usize> {
    let rest = s.strip_prefix('=')?;
    let digits = |t: &str| t.bytes().take_while(u8::is_ascii_digit).count();
    let w = digits(rest);
    if w == 0 {
        return None;
    }
    let rest = rest[w..].strip_prefix('x')?;
    let h = digits(rest);
    if h == 0 {
        return None;
    }
    Some(1 + w + 1 + h)
}

/// `pos` 处是否位于某个链接 / 图片的**无引号地址**之内。
///
/// 判据：向前找最近的 `](`，且它与 `pos` 之间不含空白或括号——
/// 若含空白，说明 `pos` 已经越出了地址（比如落在正文里），
/// 那就绝不能改写它，否则会把正常文字改坏。
fn in_link_destination(text: &str, pos: usize) -> bool {
    let Some(open) = text[..pos].rfind("](") else {
        return false;
    };
    let dest = &text[open + 2..pos];
    !dest.contains([' ', '\t', '\n', '\r', '(', ')'])
}

/// 源码预处理：删掉 CSDN 风格的图片尺寸后缀 ` =宽x高`，并给出偏移映射表。
///
/// # 为什么必须处理
///
/// `![alt](url =30x30)` 不是合法 Markdown：无引号的链接地址**一遇空白即终止**，
/// `=30x30` 于是落进括号里成为语法错误，pulldown 索性把整段按**普通文本**吐出——
/// 用户在预览里看到的就是「一长串原始 Markdown，还带着 base64 图片地址」。
/// 删掉这段后缀后，`![alt](url)` 是标准图片语法，pulldown 正常产出 `Tag::Image`，
/// 预览与导出都能显示图片。
///
/// # 为什么同时返回映射表
///
/// 删除会改变字节偏移，而每个块自带的 `source_range` 必须指向**原文**
/// （点击跳转、滚动同步、增量重解析全靠它）。调用方用 [`SourceOffsets`]
/// 把 pulldown 给出的偏移还原回原文坐标即可。没有可删时返回借用，
/// 不做任何分配。
pub fn desugar_image_sizes(text: &str) -> (std::borrow::Cow<'_, str>, SourceOffsets) {
    let mut removals: Vec<(usize, usize)> = Vec::new();
    if text.contains(" =") {
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        // `copied` 是原文中已拷进 `out` 的位置。
        let mut copied = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == b' '
                && in_link_destination(text, i)
                && let Some(len) = size_suffix_len(&text[i + 1..])
                && bytes.get(i + 1 + len) == Some(&b')')
            {
                out.push_str(&text[copied..i]);
                // 记的是「处理后」的偏移，供还原用。
                removals.push((out.len(), 1 + len));
                i += 1 + len;
                copied = i;
                continue;
            }
            i += 1;
        }
        if !removals.is_empty() {
            out.push_str(&text[copied..]);
            return (std::borrow::Cow::Owned(out), SourceOffsets { removals });
        }
    }
    (
        std::borrow::Cow::Borrowed(text),
        SourceOffsets {
            removals: Vec::new(),
        },
    )
}

/// [`desugar_image_sizes`] 的偏移映射表。
pub struct SourceOffsets {
    /// `(预处理后文本中的偏移, 该处被删掉的字节数)`，按偏移升序。
    removals: Vec<(usize, usize)>,
}

impl SourceOffsets {
    /// 把「预处理后文本」的偏移还原成「原文」偏移。
    pub fn to_original(&self, offset: usize) -> usize {
        offset
            + self
                .removals
                .iter()
                .take_while(|(at, _)| *at <= offset)
                .map(|(_, len)| *len)
                .sum::<usize>()
    }

    /// 是否发生过预处理（没发生时可跳过一切还原开销）。
    pub fn is_identity(&self) -> bool {
        self.removals.is_empty()
    }
}

/// 栈上的一项：一个尚未闭合的块级 Start。
///
/// # `is_item` / `is_cell` 为什么不再混入 `is_inline`
///
/// 早期版本把「列表项」与「单元格」一律按行内标签处理（只进栈保配平、不收集内容），
/// 结果所有文本都被并进了外层块的 `content`：列表变成 `"项一项二"`，
/// 表格变成 `"列A列B1234"`（S6-05 实测），预览区无法还原结构。
/// 故这两类现在**各自收集自己的文本**，在闭合时归入父块的 `items` / `current_row`，
/// 同时也并入父块的 `content`（保持搜索与大纲所需的扁平文本）。
///
/// # 为什么还要 `absorbs`
///
/// `- 项`（松散列表）与 `[^1]: 正文` 的正文在事件流里都包着一层 `Paragraph`，
/// 若让那层 Paragraph 照常产出块，预览里就会出现「列表项 + 同文段落」两份，
/// 脚注的定义正文更是会变成一个**游离在文档末尾的普通段落**。
/// `> 引用` 的正文同理——Paragraph 若照常成块，预览里就是一根**空色条**
/// 加一个游离在色条**外面**的正文段落（实测 `[Quote, Paragraph]`）。
/// 故这四类帧（列表项 / 脚注定义 / 定义列表的定义 / 引用）标记 `absorbs`：
/// 内层段落块的文本已由 [`push_content`] 回流到本帧，块本身则被丢弃。
struct OpenBlock {
    /// 该块产出的领域类型；`None` 表示该标签**不产出块**（行内标签 / 列表项 / 单元格）。
    kind: Option<BlockKind>,
    /// 该块在源码中的开始偏移。
    start: usize,
    /// 该块已收集到的纯文本内容（供大纲与搜索）。
    content: String,
    /// 该标签是否为**不产出块**的标签（行内标签；列表项与单元格另有用例）。
    is_inline: bool,
    /// 该 `Start` 是否为 `Tag::Item`（列表项，需把内容归入所属列表的 `items`）。
    is_item: bool,
    /// 该 `Start` 是否为 `Tag::TableCell`（单元格，需把内容归入表格当前行）。
    is_cell: bool,
    /// 内层段落块是否应被本帧吞掉（列表项 / 脚注定义 / 定义列表的定义 / 引用）。
    absorbs: bool,
    /// 该 `Start` 是否为 `Tag::DefinitionListTitle`（术语，闭合时开一个新条目）。
    is_dl_title: bool,
    /// 该 `Start` 是否为 `Tag::DefinitionListDefinition`（定义，闭合时追加一条定义）。
    is_dl_def: bool,
    /// 该列表（或其任一列表项）内是否出现 `TaskListMarker`。
    is_task: bool,
    /// 该列表项是否已出现 `TaskListMarker`（决定 `checked` 取值）。
    item_checked: Option<bool>,
    /// 该列表已收集到的列表项（含任务标记状态与源码区间）。
    items: Vec<ListItem>,
    /// 表格已收集到的完整表体行。
    rows: Vec<Vec<String>>,
    /// 表格正在收集的一行（单元格逐个追加，行闭合时整体归入 `rows` 或表头）。
    current_row: Vec<String>,
    /// 当前行各单元格的行内样式片段（与 `current_row` 一一对应）。
    current_row_spans: Vec<Vec<InlineSpan>>,
    /// 表格的表头行。
    header: Vec<String>,
    /// 表头行各单元格的行内样式片段。
    header_spans: Vec<Vec<InlineSpan>>,
    /// 表体各行各单元格的行内样式片段。
    rows_spans: Vec<Vec<Vec<InlineSpan>>>,
    /// 表格每列的对齐方式（来自 `Tag::Table(aligns)`）。
    aligns: Vec<CellAlignment>,
    /// 当前是否处于 `TableHead` 之内（决定表体行归入表头还是表体）。
    in_head: bool,
    /// 该列表的嵌套层级（顶层为 0）；仅列表帧有意义。
    list_depth: u8,
    /// 该有序列表下一个待分配的序号（`Tag::List(Some(n))` 给初值）。
    list_next_ordinal: u64,
    /// 该项是否为独立图片（`Tag::Image` 需要单独成块）。
    ///
    /// 值为 `(alt 或 title, url)`。
    image: Option<(String, String)>,
    /// `image` 生效时父块 `content` 的长度（用来从后续文本里截出 alt）。
    image_mark: Option<usize>,
    /// 行内样式帧的样式（仅合成帧与行内样式标签帧有值）。
    style: Option<InlineStyle>,
    /// 该块已收集到的行内样式片段（相对本帧 `content` 的字节区间）。
    spans: Vec<InlineSpan>,
    /// 本帧刚吞掉一个段落，**下一段**的文字到达时要先补一个换行分隔。
    ///
    /// 多段脚注体（`[^1]: 第一段` 换行缩进续 `第二段`）的两个段落都会被
    /// 本帧吞掉；不加分隔的话两段文字会连成「第一段第二段」（实测）。
    /// 标记在**段落闭合**时挂上、在下一次 [`push_content`] 写入时消费——
    /// 这样分隔只出现在两段**之间**，末尾不会多出一个换行。
    pending_break: bool,
}

impl OpenBlock {
    /// 空白帧：给出产出类型与起始偏移，其余字段取默认。
    ///
    /// 存在的意义是**避免每加一个字段就要改所有构造点**——本结构已有 20+ 字段，
    /// 散落的字面量构造会在下一次加字段时静默漏改（漏改处编译报错还算是好的，
    /// 漏成默认值就会改变行为）。
    fn new(kind: Option<BlockKind>, start: usize) -> Self {
        Self {
            kind,
            start,
            content: String::new(),
            is_inline: false,
            is_item: false,
            is_cell: false,
            absorbs: false,
            is_dl_title: false,
            is_dl_def: false,
            is_task: false,
            item_checked: None,
            items: Vec::new(),
            rows: Vec::new(),
            current_row: Vec::new(),
            current_row_spans: Vec::new(),
            header: Vec::new(),
            header_spans: Vec::new(),
            rows_spans: Vec::new(),
            aligns: Vec::new(),
            in_head: false,
            list_depth: 0,
            list_next_ordinal: 1,
            image: None,
            image_mark: None,
            style: None,
            spans: Vec::new(),
            pending_break: false,
        }
    }

    /// 该帧是否会把文本**回流**给它的祖先（列表项 / 单元格 / 脚注 / 定义）。
    const fn absorbs_text(&self) -> bool {
        self.is_item || self.is_cell || self.absorbs
    }
}

/// 取最内层「会产出块」的栈帧（列表 / 表格 / 段落等）。
///
/// 列表项与单元格只收集文本、不产出块，故要跳过它们去找真正承载结构的父亲。
fn nearest_producer(stack: &mut [OpenBlock]) -> Option<&mut OpenBlock> {
    stack
        .iter_mut()
        .rev()
        .find(|f| !f.is_item && !f.is_cell && !f.is_inline)
}

/// 把一段文本并入当前块的 `content`，并（可选）记录行内样式区间。
///
/// # 文本去哪
///
/// 目标是最内层**非行内**帧。若该帧自己**不回流**文本（即它不是列表项 /
/// 单元格 / 脚注定义 / 定义列表的定义），则再回流一份给最近的回流帧——
/// 松散列表（`- 项` 之后空一行再写）的文本会落在内部段落帧上，
/// 不回流的话列表项文本会是空的；脚注定义内的段落同理。
///
/// # ⚠️ 为什么回流的条件里带「目标自己不是回流帧」
///
/// 早先的写法是无条件回流，于是嵌套列表里子项的文字会一路回流到**父项**：
/// `1. 一级` 里嵌 `- 二级 a`，父项文本变成 `"一级二级 a"`（实测），
/// 父项与子项都显示同一段字。收紧后：目标是项/单元格时说明文本本就属于它，
/// 不再往祖先回流。
///
/// # 样式区间记在哪
///
/// 记在「会产出块的帧」以及「回流帧」上——列表项与单元格的内容虽被父亲
/// 吸收成结构，但它们各自有独立的 `text` / 单元格文本需要行内富文本
/// （`- **粗体**` 里的粗体在预览里必须显示为粗体）。
fn push_content(stack: &mut [OpenBlock], s: &str, style: Option<InlineStyle>) {
    let Some(target) = stack.iter().rposition(|f| !f.is_inline) else {
        return;
    };
    // 空文本不记片段（零长度片段没有意义，还会让渲染方多切一刀）。
    let style = if s.is_empty() { None } else { style };
    // 先读出来，避免随后可变借用期间再借用 `stack`。
    let target_is_owner = stack[target].absorbs_text();
    // 段落间的分隔（见 `pending_break`）：上一段刚结束，先补一个换行
    // 再写入本段文字——**必须**在记片段起点之前做，否则片段区间会错位。
    if stack[target].pending_break {
        stack[target].pending_break = false;
        if !stack[target].content.is_empty() {
            stack[target].content.push('\n');
        }
    }
    let start = stack[target].content.len();
    stack[target].content.push_str(s);
    if let Some(style) = style
        && (stack[target].kind.is_some() || target_is_owner)
    {
        stack[target]
            .spans
            .push(InlineSpan::new(start, start + s.len(), style));
    }
    // 回流：仅在目标自己不是回流帧时进行（见上文「为什么」）。
    if !target_is_owner
        && let Some(owner) = stack[..target].iter().rposition(OpenBlock::absorbs_text)
    {
        if stack[owner].pending_break {
            stack[owner].pending_break = false;
            if !stack[owner].content.is_empty() {
                stack[owner].content.push('\n');
            }
        }
        let ostart = stack[owner].content.len();
        stack[owner].content.push_str(s);
        if let Some(style) = style {
            stack[owner]
                .spans
                .push(InlineSpan::new(ostart, ostart + s.len(), style));
        }
    }
}

/// 取当前生效的行内样式（最内层行内帧的样式；无则 `None`）。
fn current_inline_style(stack: &[OpenBlock]) -> Option<InlineStyle> {
    stack
        .iter()
        .rev()
        .find(|f| f.is_inline)
        .and_then(|f| f.style)
}

/// 把一段**纯文本**并入当前块，并切分其中的 `==高亮==`。
///
/// `==标记文本==` 是 CSDN / 掘金等中文编辑器的常用语法，但 **pulldown-cmark
/// 没有对应选项**（GFM 也不含它），解析出来就是一段带 `==` 的普通文本；
/// 不处理的话预览里会原样显示两个等号。规则保守：`==` 必须成对且中间非空。
fn push_plain_run(stack: &mut [OpenBlock], s: &str) {
    let style = current_inline_style(stack);
    if style.is_some_and(|st| st != InlineStyle::Keycap) {
        // 已处于某种行内样式（粗体 / 代码 / 链接…）内：不再叠标记切分。
        push_content(stack, s, style);
        return;
    }
    let mut rest = s;
    while let Some(open) = rest.find("==") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("==") else {
            break;
        };
        if close == 0 {
            // `====` 这类空的标记段：原样保留前两个等号，继续往后找。
            push_content(stack, &rest[..open + 2], style);
            rest = &rest[open + 2..];
            continue;
        }
        push_content(stack, &rest[..open], style);
        push_content(stack, &after[..close], Some(InlineStyle::Mark));
        rest = &after[close + 2..];
    }
    if !rest.is_empty() {
        push_content(stack, rest, style);
    }
}

/// 压入一层合成的「键帽」样式帧（由 `<kbd>` 开启，`</kbd>` 关闭）。
///
/// 合成帧不进 `kind`（不产出块），只用来在扫描期间维持「当前是否在键帽里」。
/// 之所以需要它：`<kbd>` 与 `</kbd>` 可能落在**不同事件**里，状态必须跨事件保留。
fn push_keycap_frame(stack: &mut Vec<OpenBlock>, start: usize) {
    let mut frame = OpenBlock::new(None, start);
    frame.is_inline = true;
    frame.style = Some(InlineStyle::Keycap);
    stack.push(frame);
}

/// 弹掉栈顶的合成键帽帧（顶层不是键帽帧则忽略）。
fn pop_keycap_frame(stack: &mut Vec<OpenBlock>) {
    if stack
        .last()
        .is_some_and(|f| f.is_inline && f.style == Some(InlineStyle::Keycap))
    {
        stack.pop();
    }
}

/// 成对定界符（`~下标~` / `^上标^`）的切分：返回 `(内容, 剩余)`。
///
/// 保守条件：内容非空、首尾非空白；紧跟第二个定界符（`~~`）时直接放弃——
/// 那是删除线，不能误判成下标。
fn split_paired(tail: &str, delim: char) -> Option<(&str, &str)> {
    let after = tail.strip_prefix(delim)?;
    if after.starts_with(delim) {
        return None;
    }
    let end = after.find(delim)?;
    let inner = &after[..end];
    if inner.is_empty()
        || inner.starts_with(char::is_whitespace)
        || inner.ends_with(char::is_whitespace)
    {
        return None;
    }
    let consumed = delim.len_utf8() + end + delim.len_utf8();
    Some((inner, &tail[consumed..]))
}

/// 行内标记扫描：切开 `<kbd>` / `</kbd>` / `<br>` / `~下标~` / `^上标^`，
/// 其余部分交给 [`push_plain_run`]（它负责 `==高亮==`）。
///
/// # ⚠️ 必须传「合并后的连续文本」，不能逐事件调用
///
/// pulldown 会**按自己的需要**把同一段文字切成多个 `Text` 事件，切点毫无
/// 规律可言。实测 `2^10^` 被切成 `Text("2^10")` + `Text("^")`——
/// 定界符的**开闭两只分居两个事件**，逐事件扫描时第一段里找不到配对的 `^`
/// （于是退回字面量），第二段只剩一个孤立的 `^` 也配不上，
/// 结果就是「上标没渲染、`^` 却原样留在正文里」。
/// 调用方必须先经 [`flush_inline_text`] 把相邻文本拼成一段再来扫。
///
/// # 为什么这些标记必须自己扫
///
/// 1. **`</kbd>` 在 CommonMark 里不是行内 HTML**：`Ctrl</kbd>` 的 `<` 前面
///    紧挨字母，扫描器不认，pulldown 会把它当**普通文本**吐出来。只看
///    `Event::InlineHtml` 会把 `</kbd>` 原样留在正文里（用户看到的就是
///    「快捷键这一段没渲染」）。
/// 2. **`H~2~O` / `2^10^` 同样不被 pulldown 识别**：它的上下标要求定界符
///    前有空白（实测 `H ~2~ O` 能识别、`H~2~O` 不能），而中文写作习惯恰恰
///    不写空格。这里按 CSDN 语义补上这一层，两种写法都能渲染。
fn push_markup_aware(stack: &mut Vec<OpenBlock>, s: &str) {
    let mut rest = s;
    while !rest.is_empty() {
        // 最近的标记起点：`<` / `~` / `^`（`==` 由 `push_plain_run` 内部处理）。
        let pos = ['<', '~', '^'].iter().filter_map(|m| rest.find(*m)).min();
        let Some(idx) = pos else {
            push_plain_run(stack, rest);
            break;
        };
        if idx > 0 {
            push_plain_run(stack, &rest[..idx]);
            rest = &rest[idx..];
            continue;
        }
        // 至此 `rest` 以 `<` / `~` / `^` 开头。
        let head: String = rest.chars().take(6).collect();
        let head_lower = head.to_ascii_lowercase();
        if head_lower.starts_with("</kbd") || head_lower.starts_with("<kbd") {
            match rest.find('>') {
                Some(gt) => {
                    if head_lower.starts_with("</") {
                        pop_keycap_frame(stack);
                    } else {
                        push_keycap_frame(stack, 0);
                    }
                    rest = &rest[gt + 1..];
                }
                None => {
                    push_plain_run(stack, rest);
                    break;
                }
            }
            continue;
        }
        if head_lower.starts_with("<br") {
            match rest.find('>') {
                Some(gt) => {
                    push_content(stack, "\n", None);
                    rest = &rest[gt + 1..];
                }
                None => {
                    push_plain_run(stack, rest);
                    break;
                }
            }
            continue;
        }
        if rest.starts_with('~')
            && let Some((inner, tail)) = split_paired(rest, '~')
        {
            push_content(stack, inner, Some(InlineStyle::Subscript));
            rest = tail;
            continue;
        }
        if rest.starts_with('^')
            && let Some((inner, tail)) = split_paired(rest, '^')
        {
            push_content(stack, inner, Some(InlineStyle::Superscript));
            rest = tail;
            continue;
        }
        // 其余以 `<` / `~` / `^` 开头的字符：按一个字符的纯文本吃掉，
        // 保证循环必然前进（落单的 `~` 不会被无限重试）。
        let one = rest.chars().next().map_or(1, char::len_utf8);
        push_plain_run(stack, &rest[..one]);
        rest = &rest[one..];
    }
}

/// 把攒下的连续文本一次性交给 [`push_markup_aware`] 扫描，并清空缓冲。
///
/// 见 `push_markup_aware` 的文档：pulldown 会把一段文字切成多个事件，
/// 跨事件的定界符只有在**合并后**才配得上，故解析主循环遇文本类事件只
/// 追加不处理，遇任何**非文本**事件（块开关、行内代码、公式、换行…）
/// 才先冲刷——这样既拿到了完整字符串，又不会打乱内容顺序。
fn flush_inline_text(stack: &mut Vec<OpenBlock>, pending: &mut String) {
    if pending.is_empty() {
        return;
    }
    let run = std::mem::take(pending);
    push_markup_aware(stack, &run);
}

/// 若块内容**整体**就是一个块级公式，返回公式本体（已去 `$$` 与空白）。
///
/// 判定条件刻意保守：必须以 `$$` 开头、以 `$$` 结尾，且两者之间非空。
/// 段落里混排的公式（如「由 $$x$$ 可知」）不走这条路——它属于正文，
/// 拆成独立块会割裂语义。内部还含 `$$` 时同样拒绝（那不是合法的块级公式）。
fn math_body_if_standalone(content: &str) -> Option<String> {
    let inner = content.trim().strip_prefix("$$")?.strip_suffix("$$")?;
    let inner = inner.trim();
    if inner.is_empty() || inner.contains("$$") {
        return None;
    }
    Some(inner.to_string())
}

impl OpenBlock {
    /// 以开闭偏移构造 `source_range`。
    ///
    /// 结束偏移取 `max(end, start)`：pulldown 对空块（如空列表项）可能给出
    /// `end == start`，此时区间长度为 0，与领域层允许的「零长度块」语义一致
    /// （`BlockMapper` 对其返回比例 0.0，即滚动到块顶）。
    ///
    /// `source` 用于裁掉区间末尾的换行：pulldown 的 `End` 偏移**包含**块后的 `\n`
    /// （`"# 标题\n\n正文"` 的标题区间是 `0..8` 而非 `0..7`）。若不裁掉，
    /// 相邻两个块的区间会在空行处**重叠**，而
    /// [`BlockMapper::blocks_in_range`]（`FR-03` 增量重渲染的依据）
    /// 以「有交集」判定选中块，重叠就会把无关块纳入重渲染。
    fn range(&self, end: usize, source: &str) -> TextRange {
        let start = self.start.min(end);
        let mut stop = end.max(self.start);
        //逐字节回退掉块后的换行（可能有多个连续空行）；只退到 `start` 为止，
        // 保证零长度块仍是零长度而非下溢。
        while stop > start {
            match source.as_bytes().get(stop - 1) {
                Some(b'\n') | Some(b'\r') => stop -= 1,
                _ => break,
            }
        }
        TextRange::new(ByteOffset::new(start), ByteOffset::new(stop))
    }
}

/// 把 pulldown 的**块级**标签映射为领域层 [`BlockKind`]。
///
/// 返回 `None` 表示该标签**不单独产出块**，分两类：
/// - `Tag::Item`：其内容已由内部的段落块覆盖，若自己也成块会在大纲 / 滚动映射里
///   产生「列表项」这一层噪声节点；
/// - **行内**标签（强调、链接、删除线等）：它们在事件流里同样是 Start/End 配对，
///   但**不是块级结构**——必须与Item 一样「进出栈但不产出块」。
///   若漏判这类标签，段落里的 `**粗体**` 会凭空多出一个 `Raw` 块。
fn block_kind_of(tag: &Tag, ordered: bool) -> Option<BlockKind> {
    let kind = match tag {
        Tag::Paragraph => BlockKind::Paragraph,
        Tag::Heading { level, .. } => BlockKind::Heading {
            level: heading_level_of(*level),
        },
        Tag::BlockQuote(_) => BlockKind::Quote,
        Tag::CodeBlock(kind) => {
            if let CodeBlockKind::Fenced(lang) = kind
                && !lang.is_empty()
                && lang.to_ascii_lowercase() == "mermaid"
            {
                // Mermaid 图表：` ```mermaid ` 围栏不应被当作普通代码块，
                // 而是落到专用 `Diagram` 变体，预览区以图表卡呈现、导出保留原样。
                return Some(BlockKind::Diagram {
                    kind: "mermaid".to_string(),
                });
            }
            BlockKind::CodeBlock {
                lang: match kind {
                    CodeBlockKind::Fenced(lang) if !lang.is_empty() => Some(lang.to_string()),
                    _ => None,
                },
            }
        }
        Tag::HtmlBlock => BlockKind::Raw {
            marker: "html".to_string(),
        },
        Tag::List(_) => BlockKind::List {
            ordered,
            items: Vec::new(),
        },
        Tag::FootnoteDefinition(label) => BlockKind::Footnote {
            label: label.to_string(),
        },
        Tag::DefinitionList => BlockKind::DefinitionList {
            entries: Vec::new(),
        },
        Tag::Table(aligns) => BlockKind::Table {
            data: TableData::new(
                Vec::new(),
                Vec::new(),
                aligns.iter().map(cell_alignment_of).collect(),
            ),
        },
        _ => return None,
    };
    Some(kind)
}

/// 行内标签 → 该标签内文本的样式（非样式标签返回 `None`）。
///
/// 这些标签在事件流里同样是 Start/End 配对，但**不产出块**（见
/// [`block_kind_of`]）。它们的作用是给内部文本打上样式，供预览做行内富文本。
fn inline_style_of(tag: &Tag) -> Option<InlineStyle> {
    match tag {
        Tag::Strong => Some(InlineStyle::Strong),
        Tag::Emphasis => Some(InlineStyle::Emphasis),
        Tag::Strikethrough => Some(InlineStyle::Strikethrough),
        Tag::Link { .. } => Some(InlineStyle::Link),
        Tag::Subscript => Some(InlineStyle::Subscript),
        Tag::Superscript => Some(InlineStyle::Superscript),
        _ => None,
    }
}

/// 从段落**源码原文**里识别目录标记，返回目录标题。
///
/// 支持 `@[TOC](标题)`（括号内为标题，空括号按「目录」）与 `[TOC]` / `[[TOC]]`。
///
/// 必须看源码而不是扁平内容：`@[TOC](欢迎)` 在 pulldown 眼里是
/// 「文本 `@` + 链接 `[TOC](欢迎)`」，扁平后只剩 `@TOC`——括号里的标题
/// 与方括号都不见了，从内容里再也认不出来。
fn toc_title_of_source(src: &str) -> Option<String> {
    let s = src.trim();
    let lower = s.to_ascii_lowercase();
    if lower == "[toc]" || lower == "[[toc]]" {
        return Some("目录".to_string());
    }
    let rest = s
        .strip_prefix("@[TOC]")
        .or_else(|| s.strip_prefix("@[toc]"))
        .or_else(|| s.strip_prefix("@[Toc]"))?;
    let inner = rest.trim().strip_prefix('(')?.strip_suffix(')')?.trim();
    Some(if inner.is_empty() {
        "目录".to_string()
    } else {
        inner.to_string()
    })
}

/// 去掉列表项文本开头的任务标记（`[x] ` / `[ ] `），并**同步修正样式片段偏移**。
///
/// 勾选状态已由 [`ListItem::checked`] 单独承载，渲染时画成复选框；
/// 若文本里再留一份 `[x] ` 就会重复显示。未标记的项原样返回。
///
/// # 为什么必须一起挪片段
///
/// 片段区间是相对 `content` 的字节偏移。标记被切掉后若不移位，
/// `- [x] **重点**` 的 `重点` 会指向错误的位置（偏移 4 字节），
/// 渲染结果是「前两个字变粗、真正的内容没变」——比不渲染更难排查。
fn strip_task_marker(text: &str, spans: Vec<InlineSpan>) -> (String, Vec<InlineSpan>) {
    let s = text.trim_start();
    let stripped = match s.strip_prefix("[x] ").or_else(|| s.strip_prefix("[ ] ")) {
        Some(rest) => rest.trim_start(),
        None => s,
    };
    let removed = text.len().saturating_sub(stripped.len());
    if removed == 0 {
        return (stripped.to_string(), spans);
    }
    let moved = spans
        .into_iter()
        .filter_map(|span| {
            let start = span.start().saturating_sub(removed);
            let end = span.end().saturating_sub(removed);
            (end > start).then(|| InlineSpan::new(start, end, span.style()))
        })
        .collect();
    (stripped.to_string(), moved)
}

/// 把「已 `trim` 过的文本」对应的样式片段平移到新坐标系。
///
/// # 为什么独立于 `strip_task_marker`
///
/// 单元格、定义列表术语、定义正文在收集时都做了 `.trim()`，而它们的样式片段
/// 仍以**未裁剪**的 `content` 为坐标系。若直接拿 `spans` 去渲染裁剪后的文本，
/// 首尾空白会让所有片段整体右移若干字节：`| **粗** |` 的粗体会落到「粗」后面
/// 的字符上（表格里表现为「样式漂移一格」）。
///
/// 与 `strip_task_marker` 的区别：那个切的是**头部标记**（还带 `[x] ` 的语义），
/// 这个只做纯空白裁剪，且必须同时砍掉落在空白区里的片段——两类偏移问题不同源。
fn trim_spans(text: &str, spans: Vec<InlineSpan>) -> Vec<InlineSpan> {
    let lead = text.len().saturating_sub(text.trim_start().len());
    if lead == 0 && text.trim_end().len() == text.len() {
        return spans;
    }
    spans
        .into_iter()
        .filter_map(|span| {
            let start = span.start().saturating_sub(lead);
            let end = span.end().saturating_sub(lead);
            (end > start).then(|| InlineSpan::new(start, end, span.style()))
        })
        .collect()
}

/// 列表的嵌套层级：`stack` 里已有的列表帧数量即当前深度（顶层列表为 0）。
fn list_depth_of(stack: &[OpenBlock]) -> u8 {
    let n = stack
        .iter()
        .filter(|f| matches!(f.kind, Some(BlockKind::List { .. })))
        .count();
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// 最外层（根）列表帧的下标——嵌套列表的全部项都归它，从而拍平成一棵树。
///
/// 取**最外层**而非最近：若归到最近的内层列表，`1. 一级` 与 `- 二级 a`
/// 会各自成块，父项的文本又会被子项回流污染；拍平后一个块就能表达整棵树。
fn root_list_index(stack: &[OpenBlock]) -> Option<usize> {
    stack
        .iter()
        .position(|f| matches!(f.kind, Some(BlockKind::List { .. })))
}

/// 最近的、会吞掉内层段落的帧下标（列表项 / 脚注定义 / 定义列表的定义 / 引用）。
fn absorbing_index(stack: &[OpenBlock]) -> Option<usize> {
    stack.iter().rposition(OpenBlock::absorbs_text)
}

/// 列表块的扁平文本：按**源码顺序**把各项文本用空格连起来，并**还原任务标记**。
///
/// 不用 `open.content` 直接当内容，有两个毛病（均为实测）：
/// 1. 那是各 `Item` 帧按**收集顺序**拼的，而嵌套列表的 `End(Item)` 由内向外
///    到达，于是顺序是「先子后父」——搜索命中片段会显示成 `三级 x二级 a一级`；
/// 2. 相邻项的文本之间**没有任何分隔符**（`…代码[ ] 任务 1`），是粘连的。
///
/// # 为什么要按 `checked` 把 `[x] ` 写回去
///
/// 块的扁平 `content` 是大纲与跨文件搜索的**唯一**文本来源
/// （见 `list_content_still_contains_flat_text_for_search`）：
/// 用户搜 `[x]` 就是想筛出已完成的待办。渲染用的项文本必须去掉标记
/// （否则预览里会「复选框 + 字面 [x]」重复显示），但扁平文本要留着。
/// 两者是同一份内容面向不同消费方的两种投影，故分开生成。
fn list_plain_text(items: &[ListItem]) -> String {
    items
        .iter()
        .map(|item| match item.checked() {
            Some(true) => format!("[x] {}", item.text()),
            Some(false) => format!("[ ] {}", item.text()),
            None => item.text().to_string(),
        })
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 定义列表块的扁平文本：`术语: 定义` 逐条一行。
fn definition_list_plain_text(entries: &[DefinitionEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            let mut line = entry.term().to_string();
            for body in entry.definitions() {
                line.push_str(": ");
                line.push_str(body.text());
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// pulldown 的列对齐转为自有枚举。
fn cell_alignment_of(align: &pulldown_cmark::Alignment) -> CellAlignment {
    match align {
        pulldown_cmark::Alignment::None => CellAlignment::None,
        pulldown_cmark::Alignment::Left => CellAlignment::Left,
        pulldown_cmark::Alignment::Center => CellAlignment::Center,
        pulldown_cmark::Alignment::Right => CellAlignment::Right,
    }
}

/// pulldown 的 `HeadingLevel` 转为 1-based 级别。
fn heading_level_of(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

impl MarkdownParser for PulldownMarkdownParser {
    fn parse(&self, text: &str) -> Result<Vec<RenderedBlock>, ParseError> {
        let mut blocks: Vec<RenderedBlock> = Vec::new();
        let mut stack: Vec<OpenBlock> = Vec::new();
        let mut next_id: u64 = 0;
        // 当前所处的列表是否有序——`Tag::List` 开启时写入，Item 继承之。
        let mut list_ordered = false;
        // 当前 `Tag::List` 给出的首项编号（有序列表 `1.` 的 1；无序列表无意义）。
        let mut list_first_ordinal: u64 = 1;
        // 连续的文本类事件先攒在这里（见 `flush_inline_text`）。
        let mut pending = String::new();

        // 预处理：删掉 CSDN 的图片尺寸后缀（`![alt](url =30x30)`），
        // 否则 pulldown 认不出图片、整段按普通文本吐出。
        // 删除改变了偏移，故随后用 `offsets` 把事件偏移还原回**原文**坐标——
        // 块的 `source_range` 必须指向原文，否则点击跳转 / 滚动同步会错位。
        let (src, offsets) = desugar_image_sizes(text);

        for (event, range) in PulldownMarkdownParser::raw_parser(&src) {
            let range = offsets.to_original(range.start)..offsets.to_original(range.end);
            // 文本类事件只入缓冲、不立即处理：pulldown 可能把 `2^10^`
            // 拆成两个事件，逐事件扫描会漏掉跨事件配对的上下标定界符。
            // 其余事件一律先冲刷缓冲，保证文本与结构事件的先后顺序不变
            // （例如 `Start(Image)` 要用 content 长度记 alt 起点）。
            match &event {
                Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                    pending.push_str(t);
                    continue;
                }
                _ => flush_inline_text(&mut stack, &mut pending),
            }

            match event {
                // 图片是**行内**标签，但需要独立成块以便 S5-11 按 URL 加载资源，
                // 故必须先于下方通用的 `Event::Start(tag)` 分支匹配——
                // 后者会吃掉所有 Start 标签，导致此分支永远不可达。
                // pulldown 的 `Tag::Image` 只给出 `title`（`url "标题"` 里的标题），
                // **没有 alt**——alt 是写在 `![这里](url)` 方括号里的文本，
                // 以普通 Text 事件夹在 Start(Image) 与 End(Image) 之间。
                // 故记录 Start 时父块 content 的长度，到 End 时取增量即为 alt。
                Event::Start(Tag::Image {
                    dest_url, title, ..
                }) => {
                    if let Some(top) = stack.last_mut() {
                        top.image_mark = Some(top.content.len());
                        top.image = Some((title.to_string(), dest_url.to_string()));
                    }
                }

                Event::Start(tag) => {
                    // `Tag::List(Some(n))` 表示有序列表（首项编号为 `n`）。
                    if let Tag::List(first) = tag {
                        list_ordered = first.is_some();
                        list_first_ordinal = first.unwrap_or(1);
                    }
                    let is_item = matches!(tag, Tag::Item);
                    let is_cell = matches!(tag, Tag::TableCell);
                    // 定义列表的术语 / 定义：都不产出块，但要各自收集文本。
                    let is_dl_title = matches!(tag, Tag::DefinitionListTitle);
                    let is_dl_def = matches!(tag, Tag::DefinitionListDefinition);
                    // 脚注定义的正文包在段落里，必须回流到本帧（否则会变成
                    // 一个游离在文档里的普通段落）。
                    let is_footnote = matches!(tag, Tag::FootnoteDefinition(_));
                    // 列表项与单元格要各自收集文本（见 `OpenBlock` 的类型注释），
                    // 故不能标记为 `is_inline`——否则文本会被并到外层块去。
                    // 定义列表的术语 / 定义同理。
                    let kind = block_kind_of(&tag, list_ordered);
                    let is_inline =
                        kind.is_none() && !is_item && !is_cell && !is_dl_title && !is_dl_def;
                    // 行内样式标签（粗体 / 链接 / 上下标…）把样式挂在帧上，
                    // 其内文本按该样式记录（见 `current_inline_style`）。
                    let style = inline_style_of(&tag);
                    let aligns = match &tag {
                        Tag::Table(a) => a.iter().map(cell_alignment_of).collect(),
                        _ => Vec::new(),
                    };
                    let opens_head = matches!(tag, Tag::TableHead);
                    let mut frame = OpenBlock::new(kind, range.start);
                    frame.is_item = is_item;
                    frame.is_cell = is_cell;
                    frame.is_inline = is_inline;
                    // 会吞掉内层段落块的四类帧（见 `OpenBlock::absorbs`）。
                    let is_quote = matches!(tag, Tag::BlockQuote(_));
                    frame.absorbs = is_item || is_dl_def || is_footnote || is_quote;
                    frame.is_dl_title = is_dl_title;
                    frame.is_dl_def = is_dl_def;
                    frame.aligns = aligns;
                    frame.style = style;
                    if matches!(tag, Tag::List(_)) {
                        // 深度取「已有的列表帧个数」——本帧尚未入栈，故顶层为 0。
                        frame.list_depth = list_depth_of(&stack);
                        frame.list_next_ordinal = list_first_ordinal;
                    }
                    stack.push(frame);
                    // 进入表头：把所属表格切到「表头模式」，后续整行归为表头。
                    if opens_head {
                        let len = stack.len();
                        if len >= 2 {
                            stack[len - 2].in_head = true;
                        }
                    }
                }

                // 图片闭合：此刻方括号里的文本已并入父块 content，
                // 用 Start 时记下的长度把它截出来，覆盖掉占位用的 title。
                Event::End(TagEnd::Image) => {
                    if let Some(top) = stack.last_mut()
                        && let Some(mark) = top.image_mark.take()
                    {
                        let captured = top.content[mark..].trim().to_string();
                        if !captured.is_empty()
                            && let Some((alt, _)) = top.image.as_mut()
                        {
                            *alt = captured;
                        }
                    }
                }

                Event::End(tag_end) => {
                    let Some(mut open) = stack.pop() else {
                        // 事件流不应出现无 Start 的 End；防御性跳过而非 panic。
                        continue;
                    };
                    let end = range.end;
                    // 兜底：`<kbd>` 若在本块内没有闭合，键帽帧会一直挂在栈上，
                    // 把 Keycap 样式泄漏给后面**所有**段落（实测会把整篇染成键帽）。
                    // 块一闭合就清掉栈顶残留的合成帧——它们只可能来自本块内部。
                    while stack
                        .last()
                        .is_some_and(|f| f.is_inline && f.style == Some(InlineStyle::Keycap))
                    {
                        stack.pop();
                    }

                    // ---- 单元格：文本归入所属表格的当前行 ----
                    if open.is_cell {
                        let cell = open.content.trim().to_string();
                        let spans = trim_spans(&open.content, std::mem::take(&mut open.spans));
                        if let Some(table) = nearest_producer(&mut stack) {
                            // 单元格文本归表格的扁平 `content`：搜索与大纲要能命中表格里的字
                            // （单元格现在自己收集文本，不并入 content 的话它会变空）。
                            table.content.push_str(&cell);
                            table.current_row.push(cell);
                            table.current_row_spans.push(spans);
                        }
                        continue;
                    }

                    // ---- 表格行：整体归入表体 ----
                    //
                    // 注意 pulldown 的**表头行不带 TableRow 包裹**：序列是
                    // `Start(TableHead) 单元格… End(TableHead)`，
                    // 之后才出现第一个 `Start(TableRow)`。故表头要由
                    // `End(TableHead)` 收口，不能指望收到一次 TableRow。
                    if matches!(tag_end, TagEnd::TableRow) {
                        if let Some(table) = nearest_producer(&mut stack) {
                            if table.in_head {
                                table.header = std::mem::take(&mut table.current_row);
                                table.header_spans = std::mem::take(&mut table.current_row_spans);
                            } else {
                                table.rows.push(std::mem::take(&mut table.current_row));
                                table
                                    .rows_spans
                                    .push(std::mem::take(&mut table.current_row_spans));
                            }
                        }
                        continue;
                    }

                    // ---- 表头结束：当前行就是表头，随后切回表体模式 ----
                    if matches!(tag_end, TagEnd::TableHead) {
                        if let Some(table) = nearest_producer(&mut stack) {
                            table.header = std::mem::take(&mut table.current_row);
                            table.header_spans = std::mem::take(&mut table.current_row_spans);
                            table.in_head = false;
                        }
                        continue;
                    }

                    // ---- 定义列表的术语：开启一个新条目 ----
                    if open.is_dl_title {
                        let term = open.content.trim().to_string();
                        let spans = trim_spans(&open.content, std::mem::take(&mut open.spans));
                        if let Some(list) = nearest_producer(&mut stack)
                            && let Some(BlockKind::DefinitionList { entries }) = list.kind.as_mut()
                        {
                            entries.push(DefinitionEntry::new(term, spans));
                        }
                        continue;
                    }

                    // ---- 定义列表的定义：追加到当前条目的定义列表 ----
                    if open.is_dl_def {
                        let text = open.content.trim().to_string();
                        let spans = trim_spans(&open.content, std::mem::take(&mut open.spans));
                        if let Some(list) = nearest_producer(&mut stack)
                            && let Some(BlockKind::DefinitionList { entries }) = list.kind.as_mut()
                            && let Some(entry) = entries.last_mut()
                        {
                            entry.push_definition(DefinitionBody::new(text, spans));
                        }
                        continue;
                    }

                    // ---- 列表项：闭合时定型，并**拍平**到根列表 ----
                    //
                    // 列表项在闭合时定型（TaskListMarker 此刻已到达）：
                    // 收成一个 `ListItem` 归入**根**列表，同时把文本并入列表的
                    // 扁平 `content`（搜索与大纲仍能看到整段列表文字）。
                    if open.is_item {
                        // 最近的那个列表帧决定：本项的缩进层级、是否有序、序号取几。
                        let mut depth = 0u8;
                        let mut ordinal = None;
                        if let Some(li) = stack
                            .iter()
                            .rposition(|f| matches!(f.kind, Some(BlockKind::List { .. })))
                        {
                            depth = stack[li].list_depth;
                            if matches!(stack[li].kind, Some(BlockKind::List { ordered: true, .. }))
                            {
                                ordinal = Some(stack[li].list_next_ordinal);
                                stack[li].list_next_ordinal += 1;
                            }
                        }
                        let range = open.range(end, text);
                        let (item_text, item_spans) =
                            strip_task_marker(&open.content, std::mem::take(&mut open.spans));
                        let mut item = ListItem::new(item_text, open.item_checked, range)
                            .with_depth(depth)
                            .with_spans(item_spans);
                        if let Some(n) = ordinal {
                            item = item.with_ordinal(n);
                        }
                        if let Some(root) = root_list_index(&stack) {
                            let list = &mut stack[root];
                            list.is_task |= open.item_checked.is_some();
                            // 注意这里**不能**顺手拼 `open.content`：项是按
                            // 由内向外的顺序闭合的，拼出来是乱序且带任务标记的
                            // 文本；扁平内容统一在列表块定型时按源码顺序重建
                            // （见 `list_plain_text`）。
                            list.items.push(item);
                        }
                        continue;
                    }

                    // 其余行内标签（强调 / 链接 / 删除线 / 图片的 alt 文本等）：
                    // 进出栈即可，**不产出块**——它们的文本已由内层块收集。
                    if open.is_inline {
                        continue;
                    }

                    // ---- 被吸收的段落：不再自己成块 ----
                    //
                    // 脚注定义 `[^1]: 内容` 与定义列表的定义 `: 内容` 里，正文
                    // 被 pulldown 包在**段落**里。该段落的文本已由 `push_content`
                    // 回流给外层的脚注 / 定义帧；若这里再让它成块，文档流里就
                    // 会多出一个游离段落，预览中脚注内容会**显示两遍**
                    // （实测：`[Paragraph, Footnote, Paragraph]`）。
                    //
                    // 同时挂上 `pending_break`：若后面**还有**段落被同一帧吞掉，
                    // 下一段的文字到达时先补一个换行，两段才不会连成一处
                    // （实测 `[^m]: 第一段` + 缩进续段 → `"第一段第二段"`）。
                    if matches!(open.kind, Some(BlockKind::Paragraph))
                        && let Some(owner) = absorbing_index(&stack)
                    {
                        stack[owner].pending_break = true;
                        continue;
                    }

                    // ---- 嵌套引用：内层并入外层，不单独成块 ----
                    //
                    // `> > 深层` 里内层引用的文字已回流给外层（引用现在也是
                    // 回流帧）；若内层再照常产出 Quote 块，外层就是一根
                    // **空色条**、内层的字反而在空色条的下方（与修复前的
                    // 顶层引用同一缺陷）。把内层的内容与行内样式片段整体
                    // 搬给外层后丢弃——内容顺序也是对的：内层的 End 由内
                    // 向外到达，此刻外层 `content` 里只有它**之前**的段落。
                    if matches!(open.kind, Some(BlockKind::Quote))
                        && let Some(owner) = absorbing_index(&stack)
                    {
                        // 分隔规则与 [`push_content`] 的 `pending_break` 一致：
                        // 先补换行再写入，片段基点取换行之后的长度，区间不错位。
                        if stack[owner].pending_break {
                            stack[owner].pending_break = false;
                            if !stack[owner].content.is_empty() {
                                stack[owner].content.push('\n');
                            }
                        }
                        let base = stack[owner].content.len();
                        stack[owner].content.push_str(&open.content);
                        for span in std::mem::take(&mut open.spans) {
                            stack[owner].spans.push(InlineSpan::new(
                                base + span.start(),
                                base + span.end(),
                                span.style(),
                            ));
                        }
                        continue;
                    }

                    // ---- 嵌套列表：项已归入根列表，内层是空壳 ----
                    //
                    // `root_list_index` 把整棵列表树的项都拍平给**根**列表，
                    // 于是每个内层 `List` 帧闭合时 `items` 都是空的。
                    // 不丢弃的话每深一层就多一个空列表块——预览里表现为
                    // 列表下方多出一片莫名的空白（零项列表仍占一个段落间距）。
                    if matches!(
                        open.kind,
                        Some(BlockKind::List { .. } | BlockKind::TaskList { .. })
                    ) && open.items.is_empty()
                        && root_list_index(&stack).is_some()
                    {
                        continue;
                    }

                    // 图片：产出携带 `alt` / `url` 的独立块（供 S5-11 加载资源）。
                    if let Some((alt, url)) = open.image.take() {
                        blocks.push(RenderedBlock::new(
                            BlockId::from_u64(next_id),
                            BlockKind::Image { alt, url },
                            open.range(end, text),
                            open.content,
                        ));
                        next_id += 1;
                        continue;
                    }

                    // 表格结构：`current_row` / `rows` / `header` 已在单元格与行
                    // 闭合时收集完毕，此处一并装进 `TableData`。
                    //
                    // 整段只有一个块级公式时升级为 `Math` 块：pulldown 把 `$$...$$`
                    // 放在段落里，但「一个段落只有一条公式」在视觉与语义上都等价于
                    // 独立公式块，交给表现层居中成卡更贴合预期。
                    let promoted_math = math_body_if_standalone(&open.content);
                    // 目录标记要从**源码原文**识别（扁平后只剩 `@TOC`）。
                    // 只在「整段就是一个标记」时成立。
                    let toc = matches!(open.kind, Some(BlockKind::Paragraph))
                        .then(|| {
                            text.get(open.start.min(end)..end)
                                .and_then(toc_title_of_source)
                        })
                        .flatten();
                    let kind = open.kind.clone();

                    // 列表在闭合时定型：任一列表项带任务标记 → 整个列表是任务列表。
                    let kind = match (kind, open.is_task) {
                        (Some(BlockKind::List { .. }), true) => BlockKind::TaskList {
                            items: std::mem::take(&mut open.items),
                        },
                        (Some(BlockKind::List { ordered, .. }), false) => BlockKind::List {
                            ordered,
                            items: std::mem::take(&mut open.items),
                        },
                        (Some(BlockKind::Table { .. }), _) => BlockKind::Table {
                            data: TableData::new(
                                std::mem::take(&mut open.header),
                                std::mem::take(&mut open.rows),
                                std::mem::take(&mut open.aligns),
                            )
                            .with_cell_spans(
                                std::mem::take(&mut open.header_spans),
                                std::mem::take(&mut open.rows_spans),
                            ),
                        },
                        (Some(BlockKind::Paragraph), _) if toc.is_some() => BlockKind::Toc {
                            title: toc.clone().unwrap_or_default(),
                        },
                        (Some(BlockKind::Paragraph), _) if promoted_math.is_some() => {
                            BlockKind::Math { display: true }
                        }
                        (Some(kind), _) => kind,
                        // `kind` 为 None 说明是无需产出块的标签，已在上面 continue。
                        (None, _) => continue,
                    };

                    // 列表项排序：项在各自的 `End(Item)` 收集，而嵌套列表的 End
                    // 由内向外到达，收集顺序天然是「先子后父」（实测得到
                    // `[三级 x, 二级 a, 二级 b, 一级, 一级二]`）。不排序的话预览里
                    // 子项会整片跑到父项**上面**。按源码起点排即恢复文档顺序。
                    let mut kind = kind;
                    if let BlockKind::List { items, .. } | BlockKind::TaskList { items } = &mut kind
                    {
                        items.sort_by_key(|item| item.source_range().start().value());
                    }

                    // 公式块的content 存**公式本体**（不含`$$`），
                    // 表现层直接展示 / 复制，无需再剥分隔符。
                    //
                    // 区间要在移动 `content` **之前**算好：`range()` 借用 `self`，
                    // 先移走字段再借用会触发 E0382（部分移动后借用）。
                    let range = open.range(end, text);
                    let mut spans = std::mem::take(&mut open.spans);
                    let content = match (&kind, promoted_math) {
                        // 公式块的内容换成「公式本体」，区间不再对应 → 丢弃。
                        (BlockKind::Math { .. }, Some(body)) => {
                            spans.clear();
                            body
                        }
                        // 目录块的内容就是**标题**。
                        //
                        // 为什么不留在 `BlockKind::Toc { title }` 里给表现层读：
                        // 架构约束（`check-arch`）禁止表现层依赖领域枚举，
                        // `BlockView` 只带一个扁平 `kind: String` 与 `content`，
                        // 表现层唯一能拿到的标题来源就是 `content`。
                        // 而扁平后的内容只剩 `@TOC`（`@[TOC](欢迎)` 中的 `@` 是
                        // 文本、`[TOC](欢迎)` 是链接，括号里的标题早被吃掉），
                        // 所以必须在这里把源码里认出来的标题写回去。
                        // 顺带让大纲 / 搜索能看到目录标题。
                        (BlockKind::Toc { title }, _) => {
                            spans.clear();
                            title.clone()
                        }
                        // 列表 / 定义列表的扁平内容按**源码顺序**重建
                        // （理由见 `list_plain_text`）；块级 `spans` 对应的是
                        // 被替换掉的旧 `content`，一并清掉以免错位。
                        (BlockKind::List { items, .. } | BlockKind::TaskList { items }, _) => {
                            spans.clear();
                            list_plain_text(items)
                        }
                        (BlockKind::DefinitionList { entries }, _) => {
                            spans.clear();
                            definition_list_plain_text(entries)
                        }
                        _ => open.content,
                    };

                    blocks.push(
                        RenderedBlock::new(BlockId::from_u64(next_id), kind, range, content)
                            .with_spans(spans),
                    );
                    next_id += 1;
                }

                // ---- 行内事件：文本并入**最近的产出块**的栈帧 ----
                //
                // 注意不能简单地写入 `stack.last_mut()`：`这是 **粗体**` 这样的文本
                // 在事件流里是 `Start(Paragraph) Text Start(Strong) Text End(Strong) Text`，
                // 处理 `Text` 时栈顶是**行内**的 `Strong` 帧。若写进去，
                // 文本会随该帧一起被丢弃——表现为 `content` 出现空洞
                // （曾实测 `这是 **粗体** 与 ` + "`代码`" + ` 文本` 变成 `这是  与 代码 文本`）。
                //
                // 文本类事件在本轮循环开头已进缓冲并 `continue`，走不到这个分支；
                // 保留臂只是为了覆盖枚举，并指明它们的去处。
                Event::Text(_) | Event::Html(_) | Event::InlineHtml(_) => {}
                Event::Code(t) => {
                    // 行内代码：文本保留（搜索 / 大纲看得到），样式记为 Code。
                    push_content(&mut stack, &t, Some(InlineStyle::Code));
                }
                Event::SoftBreak | Event::HardBreak => {
                    // 换行不带样式：样式片段不跨行，预览按行切分渲染才不会错位。
                    push_content(&mut stack, "\n", None);
                }
                Event::InlineMath(t) => {
                    // 行内公式是**段落内容的一部分**，不独立成块：`$...$` 前后
                    // 通常还有正文，独立成块会把一句话拆成三块、破坏段落语义。
                    // 补回 `$` 分隔符，让 `content` 仍是可搜索的原文。
                    push_content(&mut stack, &format!("${t}$"), Some(InlineStyle::Math));
                }
                Event::DisplayMath(t) => {
                    // 块级公式同理并入当前块内容；`$$` 换行写法在 pulldown 里
                    // 会带上首尾换行，trim 后再补分隔符避免出现空公式。
                    push_content(
                        &mut stack,
                        &format!("$${}\n$$", t.trim()),
                        Some(InlineStyle::Math),
                    );
                }
                Event::FootnoteReference(label) => {
                    // 内容保留 `[^1]` 原文（搜索 / 字数看到的是用户写的字符），
                    // 但**挂上脚注引用样式**：渲染层据此把它换成上标标号，
                    // 否则正文里会以原始 Markdown 形态显示 `[^1]`——
                    // 看起来就像「脚注没渲染」（实测）。
                    push_content(
                        &mut stack,
                        &format!("[^{label}]"),
                        Some(InlineStyle::FootnoteRef),
                    );
                }
                // 任务列表标记：落到**列表项**帧（列表项已能收集自己的文本），
                // 并记录该 `Range` 的勾选状态以便 S6-05 渲染复选框、回写源码。
                // 标记文本仍并进内容（`- [x] ` 的勾选状态是 Outliner 与搜索需要的信息）。
                Event::TaskListMarker(checked) => {
                    push_content(&mut stack, if checked { "[x] " } else { "[ ] " }, None);
                    if let Some(item) = stack.iter_mut().rev().find(|f| f.is_item) {
                        item.item_checked = Some(checked);
                    }
                }

                // 分割线：pulldown 把它作为独立事件（无配对标签），
                // 故当场产出一个 `Rule` 块，区间取事件自带偏移
                //（同样裁掉尾随换行，避免与相邻块区间重叠）。
                Event::Rule => {
                    let mut stop = range.end;
                    while stop > range.start
                        && matches!(text.as_bytes().get(stop - 1), Some(b'\n') | Some(b'\r'))
                    {
                        stop -= 1;
                    }
                    blocks.push(RenderedBlock::new(
                        BlockId::from_u64(next_id),
                        BlockKind::Rule,
                        TextRange::new(ByteOffset::new(range.start), ByteOffset::new(stop)),
                        String::new(),
                    ));
                    next_id += 1;
                }
            }
        }

        // 收尾冲刷：正常情况下栈已空（文本早已随各块的 End 落盘），
        // 这里只兜住「文档以未闭合文本结尾」的异常输入。
        flush_inline_text(&mut stack, &mut pending);

        // 防御：若文档以未闭合块结尾（理论上不会），补齐为块而非丢弃内容。
        for open in stack.into_iter().rev() {
            // 只补齐「本应产出块」的标签；行内标签与列表项不补。
            if let Some(kind) = open.kind.clone() {
                let end = open.start;
                blocks.push(
                    RenderedBlock::new(
                        BlockId::from_u64(next_id),
                        kind,
                        open.range(end, text),
                        open.content,
                    )
                    .with_spans(open.spans),
                );
                next_id += 1;
            }
        }

        // 领域层契约：块必须按 `source_range.start` 升序排列，否则
        // `BlockMapper::offset_to_block` 的查找会失效（`FR-02` 依赖此前提）。
        blocks.sort_by_key(|block| (block.source_range().start(), block.id()));

        Ok(blocks)
    }
}
