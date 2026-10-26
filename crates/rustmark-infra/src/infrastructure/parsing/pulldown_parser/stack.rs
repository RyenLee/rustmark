//! 块级开栈的栈帧 [`OpenBlock`] 与全部栈操作：
//! 文本回流、行内标记扫描、键帽合成帧、区间收口、独立公式判定。

use rustmark_core::domain::markdown::value_objects::{
    BlockKind, CellAlignment, InlineImage, InlineSpan, InlineStyle, ListItem,
};
use rustmark_core::shared::geometry::{ByteOffset, TextRange};

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
/// 内层段落块的文本已由 `push_content` 回流到本帧，块本身则被丢弃。
pub(crate) struct OpenBlock {
    /// 该块产出的领域类型；`None` 表示该标签**不产出块**（行内标签 / 列表项 / 单元格）。
    pub(super) kind: Option<BlockKind>,
    /// 该块在源码中的开始偏移。
    pub(super) start: usize,
    /// 该块已收集到的纯文本内容（供大纲与搜索）。
    pub(super) content: String,
    /// 该标签是否为**不产出块**的标签（行内标签；列表项与单元格另有用例）。
    pub(super) is_inline: bool,
    /// 该 `Start` 是否为 `Tag::Item`（列表项，需把内容归入所属列表的 `items`）。
    pub(super) is_item: bool,
    /// 该 `Start` 是否为 `Tag::TableCell`（单元格，需把内容归入表格当前行）。
    pub(super) is_cell: bool,
    /// 内层段落块是否应被本帧吞掉（列表项 / 脚注定义 / 定义列表的定义 / 引用）。
    pub(super) absorbs: bool,
    /// 该 `Start` 是否为 `Tag::DefinitionListTitle`（术语，闭合时开一个新条目）。
    pub(super) is_dl_title: bool,
    /// 该 `Start` 是否为 `Tag::DefinitionListDefinition`（定义，闭合时追加一条定义）。
    pub(super) is_dl_def: bool,
    /// 该列表（或其任一列表项）内是否出现 `TaskListMarker`。
    pub(super) is_task: bool,
    /// 该列表项是否已出现 `TaskListMarker`（决定 `checked` 取值）。
    pub(super) item_checked: Option<bool>,
    /// 该列表已收集到的列表项（含任务标记状态与源码区间）。
    pub(super) items: Vec<ListItem>,
    /// 表格已收集到的完整表体行。
    pub(super) rows: Vec<Vec<String>>,
    /// 表格正在收集的一行（单元格逐个追加，行闭合时整体归入 `rows` 或表头）。
    pub(super) current_row: Vec<String>,
    /// 当前行各单元格的行内样式片段（与 `current_row` 一一对应）。
    pub(super) current_row_spans: Vec<Vec<InlineSpan>>,
    /// 表格的表头行。
    pub(super) header: Vec<String>,
    /// 表头行各单元格的行内样式片段。
    pub(super) header_spans: Vec<Vec<InlineSpan>>,
    /// 表体各行各单元格的行内样式片段。
    pub(super) rows_spans: Vec<Vec<Vec<InlineSpan>>>,
    /// 表格每列的对齐方式（来自 `Tag::Table(aligns)`）。
    pub(super) aligns: Vec<CellAlignment>,
    /// 当前是否处于 `TableHead` 之内（决定表体行归入表头还是表体）。
    pub(super) in_head: bool,
    /// 该列表的嵌套层级（顶层为 0）；仅列表帧有意义。
    pub(super) list_depth: u8,
    /// 该有序列表下一个待分配的序号（`Tag::List(Some(n))` 给初值）。
    pub(super) list_next_ordinal: u64,
    /// 本帧文字里**已收集完**的图片（区间相对 `content`）。
    ///
    /// 一张块里可以有多张图（`前 ![a](1.png) 中 ![b](2.png) 后`），故是
    /// `Vec` 而不是 `Option`：旧实现只留一张，第二张会整张丢掉。
    pub(super) images: Vec<InlineImage>,
    /// 正在收集 alt 的图片（`Start(Image)` 已到达、`End(Image)` 未到达）。
    ///
    /// 图片是行内标签、不进栈（见 `start.rs`），故「在图里」这个状态只能
    /// 挂在**宿主帧**上；`start` 记录 alt 在本帧 `content` 里的起点。
    pub(super) image_open: Option<PendingImage>,
    /// 行内样式帧的样式（仅合成帧与行内样式标签帧有值）。
    pub(super) style: Option<InlineStyle>,
    /// 链接帧的目标地址（`Tag::Link { dest_url }` 进入时记录）。
    ///
    /// 与 `style` 并列而非并入：同一帧可能是「链接 + 加粗」嵌套，目标与样式
    /// 正交；链接内的文本按该目标记录 `InlineSpan::dest`（见 `push_content`）。
    pub(super) link_dest: Option<String>,
    /// 该块已收集到的行内样式片段（相对本帧 `content` 的字节区间）。
    pub(super) spans: Vec<InlineSpan>,
    /// 本帧刚吞掉一个段落，**下一段**的文字到达时要先补一个换行分隔。
    ///
    /// 多段脚注体（`[^1]: 第一段` 换行缩进续 `第二段`）的两个段落都会被
    /// 本帧吞掉；不加分隔的话两段文字会连成「第一段第二段」（实测）。
    /// 标记在**段落闭合**时挂上、在下一次 `push_content` 写入时消费——
    /// 这样分隔只出现在两段**之间**，末尾不会多出一个换行。
    pub(super) pending_break: bool,
}

impl OpenBlock {
    /// 空白帧：给出产出类型与起始偏移，其余字段取默认。
    ///
    /// 存在的意义是**避免每加一个字段就要改所有构造点**——本结构已有 20+ 字段，
    /// 散落的字面量构造会在下一次加字段时静默漏改（漏改处编译报错还算是好的，
    /// 漏成默认值就会改变行为）。
    pub(super) fn new(kind: Option<BlockKind>, start: usize) -> Self {
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
            images: Vec::new(),
            image_open: None,
            style: None,
            link_dest: None,
            spans: Vec::new(),
            pending_break: false,
        }
    }

    /// 该帧是否会把文本**回流**给它的祖先（列表项 / 单元格 / 脚注 / 定义）。
    pub(super) const fn absorbs_text(&self) -> bool {
        self.is_item || self.is_cell || self.absorbs
    }
}

/// 正在收集 alt 的图片（见 [`OpenBlock::image_open`]）。
///
/// 图片是行内标签、不进栈，故「在图里」这个状态只能挂在宿主帧上。
///
/// 显示参数（`#pic_center` / ` =30x30`）不在这里：它们要在 `End(Image)`
/// 时按**原文**偏移取（此刻偏移才还原，见 `end.rs`），`Start` 阶段取不到。
#[derive(Clone, Debug)]
pub(super) struct PendingImage {
    /// alt 文本在宿主帧 `content` 里的起点字节（`Start(Image)` 时的长度）。
    pub(super) start: usize,
    /// `![a](url "标题")` 里的标题（方括号里没字时充当 alt）。
    pub(super) title: String,
    /// 图片地址。
    pub(super) url: String,
}

/// 把一组图片区间整体平移 `delta` 字节（正 = 右移）。
///
/// 两类场合必须挪：文本**回流**给外层帧（引用 / 列表项吞掉内层段落时，
/// 段落的图片区间是相对段落 `content` 的）与列表项的**任务标记裁剪**
/// （`strip_task_marker` 砍掉头部若干字节）。不挪的话图片会落在别的字上。
///
/// 平移后区间退化（起点越界或倒挂）的图片直接丢弃——它们的文字已被裁掉，
/// 留着只会让渲染方切出错误的段。
pub(super) fn shifted_images(images: &[InlineImage], delta: i64) -> Vec<InlineImage> {
    images
        .iter()
        .filter_map(|img| {
            let start = i64::try_from(img.start()).unwrap_or(i64::MAX) + delta;
            let end = i64::try_from(img.end()).unwrap_or(i64::MAX) + delta;
            if start < 0 || end <= start {
                return None;
            }
            let Ok(start) = usize::try_from(start) else {
                return None;
            };
            let Ok(end) = usize::try_from(end) else {
                return None;
            };
            Some(InlineImage::new(
                TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
                img.alt(),
                img.url(),
                img.display(),
            ))
        })
        .collect()
}

/// 该帧文字里**除图片外是否还有别的文字**。
///
/// `true` 表示整块就只有图（可以有连续多张）——此时把整块升级为图片块
/// 没有任何代价，沿用旧行为；`false` 表示图片前后还有文案，块必须保持
/// 本来类型，否则那些文案会随块一起消失（问题 032）。
///
/// 判定方式是「抠掉所有图片区间后剩下的是不是只有空白」：`content` 里
/// 图片占的那段文字就是 alt，不会是用户写的正文。
pub(super) fn has_text_outside_images(content: &str, images: &[InlineImage]) -> bool {
    if images.is_empty() {
        return true;
    }
    let mut rest = String::new();
    let mut cursor = 0usize;
    for img in images {
        let start = img.start().min(content.len());
        let end = img.end().min(content.len());
        if start > cursor {
            rest.push_str(content.get(cursor..start).unwrap_or_default());
        }
        cursor = cursor.max(end);
    }
    if cursor < content.len() {
        rest.push_str(content.get(cursor..).unwrap_or_default());
    }
    !rest.trim().is_empty()
}

/// 取最内层「会产出块」的栈帧（列表 / 表格 / 段落等）。
///
/// 列表项与单元格只收集文本、不产出块，故要跳过它们去找真正承载结构的父亲。
pub(super) fn nearest_producer(stack: &mut [OpenBlock]) -> Option<&mut OpenBlock> {
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
pub(super) fn push_content(stack: &mut [OpenBlock], s: &str, style: Option<InlineStyle>) {
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
        // 链接片段附带目标地址，供预览点击打开（见 `current_link_dest`）。
        let dest = if style == InlineStyle::Link {
            current_link_dest(stack)
        } else {
            None
        };
        stack[target].spans.push(match dest {
            Some(d) => InlineSpan::new_link(start, start + s.len(), style, d),
            None => InlineSpan::new(start, start + s.len(), style),
        });
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
            let dest = if style == InlineStyle::Link {
                current_link_dest(stack)
            } else {
                None
            };
            stack[owner].spans.push(match dest {
                Some(d) => InlineSpan::new_link(ostart, ostart + s.len(), style, d),
                None => InlineSpan::new(ostart, ostart + s.len(), style),
            });
        }
    }
}

/// 取当前生效的行内样式（最内层行内帧的样式；无则 `None`）。
pub(super) fn current_inline_style(stack: &[OpenBlock]) -> Option<InlineStyle> {
    stack
        .iter()
        .rev()
        .find(|f| f.is_inline)
        .and_then(|f| f.style)
}

/// 取当前生效的链接目标（最内层「且带 `link_dest`」的行内帧；无则 `None`）。
///
/// 与 [`current_inline_style`] 同源但独立：链接文本可能嵌套在别的行内样式里
/// （`[**粗链**](url)` 的 `Link` 帧不一定是最内层），而目标只属于 `Link` 帧。
/// 取**最内层带目标**的帧，正好对应 `push_content` 给链接片段附目标的需求。
pub(super) fn current_link_dest(stack: &[OpenBlock]) -> Option<String> {
    stack.iter().rev().find_map(|f| f.link_dest.clone())
}

/// 把一段**纯文本**并入当前块，并切分其中的 `==高亮==`。
///
/// `==标记文本==` 是 CSDN / 掘金等中文编辑器的常用语法，但 **pulldown-cmark
/// 没有对应选项**（GFM 也不含它），解析出来就是一段带 `==` 的普通文本；
/// 不处理的话预览里会原样显示两个等号。规则保守：`==` 必须成对且中间非空。
pub(super) fn push_plain_run(stack: &mut [OpenBlock], s: &str) {
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
pub(super) fn push_keycap_frame(stack: &mut Vec<OpenBlock>, start: usize) {
    let mut frame = OpenBlock::new(None, start);
    frame.is_inline = true;
    frame.style = Some(InlineStyle::Keycap);
    stack.push(frame);
}

/// 弹掉栈顶的合成键帽帧（顶层不是键帽帧则忽略）。
pub(super) fn pop_keycap_frame(stack: &mut Vec<OpenBlock>) {
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
/// 其余部分交给 `push_plain_run`（它负责 `==高亮==`）。
///
/// # ⚠️ 必须传「合并后的连续文本」，不能逐事件调用
///
/// pulldown 会**按自己的需要**把同一段文字切成多个 `Text` 事件，切点毫无
/// 规律可言。实测 `2^10^` 被切成 `Text("2^10")` + `Text("^")`——
/// 定界符的**开闭两只分居两个事件**，逐事件扫描时第一段里找不到配对的 `^`
/// （于是退回字面量），第二段只剩一个孤立的 `^` 也配不上，
/// 结果就是「上标没渲染、`^` 却原样留在正文里」。
/// 调用方必须先经 `flush_inline_text` 把相邻文本拼成一段再来扫。
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
pub(super) fn push_markup_aware(stack: &mut Vec<OpenBlock>, s: &str) {
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

/// 把攒下的连续文本一次性交给 `push_markup_aware` 扫描，并清空缓冲。
///
/// 见 `push_markup_aware` 的文档：pulldown 会把一段文字切成多个事件，
/// 跨事件的定界符只有在**合并后**才配得上，故解析主循环遇文本类事件只
/// 追加不处理，遇任何**非文本**事件（块开关、行内代码、公式、换行…）
/// 才先冲刷——这样既拿到了完整字符串，又不会打乱内容顺序。
pub(super) fn flush_inline_text(stack: &mut Vec<OpenBlock>, pending: &mut String) {
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
pub(super) fn math_body_if_standalone(content: &str) -> Option<String> {
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
    /// `BlockMapper::blocks_in_range`（`FR-03` 增量重渲染的依据）
    /// 以「有交集」判定选中块，重叠就会把无关块纳入重渲染。
    pub(super) fn range(&self, end: usize, source: &str) -> TextRange {
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
