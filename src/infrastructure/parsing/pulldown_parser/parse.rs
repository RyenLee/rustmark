//! `MarkdownParser` 实现：事件流 → 带源码区间的块序列主循环。

use pulldown_cmark::{Event, Tag, TagEnd};

use crate::domain::markdown::errors::ParseError;
use crate::domain::markdown::services::MarkdownParser;
use crate::domain::markdown::value_objects::{
    BlockId, BlockKind, DefinitionBody, DefinitionEntry, InlineSpan, InlineStyle, ListItem,
    RenderedBlock, TableData,
};
use crate::shared::geometry::{ByteOffset, TextRange};

use super::PulldownMarkdownParser;
use super::event_map::{
    absorbing_index, block_kind_of, cell_alignment_of, definition_list_plain_text, inline_style_of,
    list_depth_of, list_plain_text, root_list_index, strip_task_marker, toc_title_of_source,
    trim_spans,
};
use super::image_size::desugar_image_sizes;
use super::stack::{
    OpenBlock, flush_inline_text, math_body_if_standalone, nearest_producer, push_content,
};

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
                        // 分隔规则与 `push_content` 的 `pending_break` 一致：
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
                    // 否则正文里以原始 Markdown 形态显示 `[^1]`——
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
