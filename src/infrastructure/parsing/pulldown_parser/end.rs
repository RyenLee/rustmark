//! `Event::End` 分支的处理逻辑（含 Image End 特判 + 通用 TagEnd 巨块）。

use super::*;
use pulldown_cmark::TagEnd;

/// 处理 `Event::End(tag_end)`：出栈、定型、产出块。
///
/// 内部逻辑与原 parse() 中 `Event::End` match 分支完全一致，
/// 仅做了「局部变量 → `state.xxx` 字段访问」的搬运，
/// 原 `continue` 语义改为 `return`（因为不再是 for 循环体内）。
pub(super) fn handle_end(
    state: &mut ParseState,
    tag_end: TagEnd,
    range: std::ops::Range<usize>,
    text: &str,
) {
    // 图片闭合：此刻方括号里的文本已并入父块 content，
    // 用 Start 时记下的长度把它截出来，覆盖掉占位用的 title。
    if matches!(tag_end, TagEnd::Image) {
        if let Some(top) = state.stack.last_mut()
            && let Some(mark) = top.image_mark.take()
        {
            let captured = top.content[mark..].trim().to_string();
            if !captured.is_empty()
                && let Some((alt, _)) = top.image.as_mut()
            {
                *alt = captured;
            }
        }
        return;
    }

    let Some(mut open) = state.stack.pop() else {
        // 事件流不应出现无 Start 的 End；防御性跳过而非 panic。
        return;
    };
    let end = range.end;
    // 兜底：`<kbd>` 若在本块内没有闭合，键帽帧会一直挂在栈上，
    // 把 Keycap 样式泄漏给后面**所有**段落（实测会把整篇染成键帽）。
    // 块一闭合就清掉栈顶残留的合成帧——它们只可能来自本块内部。
    while state
        .stack
        .last()
        .is_some_and(|f| f.is_inline && f.style == Some(InlineStyle::Keycap))
    {
        state.stack.pop();
    }

    // ---- 单元格：文本归入所属表格的当前行 ----
    if open.is_cell {
        let cell = open.content.trim().to_string();
        let spans = trim_spans(&open.content, std::mem::take(&mut open.spans));
        if let Some(table) = nearest_producer(&mut state.stack) {
            // 单元格文本归表格的扁平 `content`：搜索与大纲要能命中表格里的字
            // （单元格现在自己收集文本，不并入 content 的话它会变空）。
            table.content.push_str(&cell);
            table.current_row.push(cell);
            table.current_row_spans.push(spans);
        }
        return;
    }

    // ---- 表格行：整体归入表体 ----
    //
    // 注意 pulldown 的**表头行不带 TableRow 包裹**：序列是
    // `Start(TableHead) 单元格… End(TableHead)`，
    // 之后才出现第一个 `Start(TableRow)`。故表头要由
    // `End(TableHead)` 收口，不能指望收到一次 TableRow。
    if matches!(tag_end, TagEnd::TableRow) {
        if let Some(table) = nearest_producer(&mut state.stack) {
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
        return;
    }

    // ---- 表头结束：当前行就是表头，随后切回表体模式 ----
    if matches!(tag_end, TagEnd::TableHead) {
        if let Some(table) = nearest_producer(&mut state.stack) {
            table.header = std::mem::take(&mut table.current_row);
            table.header_spans = std::mem::take(&mut table.current_row_spans);
            table.in_head = false;
        }
        return;
    }

    // ---- 定义列表的术语：开启一个新条目 ----
    if open.is_dl_title {
        let term = open.content.trim().to_string();
        let spans = trim_spans(&open.content, std::mem::take(&mut open.spans));
        if let Some(list) = nearest_producer(&mut state.stack)
            && let Some(BlockKind::DefinitionList { entries }) = list.kind.as_mut()
        {
            entries.push(DefinitionEntry::new(term, spans));
        }
        return;
    }

    // ---- 定义列表的定义：追加到当前条目的定义列表 ----
    if open.is_dl_def {
        let text = open.content.trim().to_string();
        let spans = trim_spans(&open.content, std::mem::take(&mut open.spans));
        if let Some(list) = nearest_producer(&mut state.stack)
            && let Some(BlockKind::DefinitionList { entries }) = list.kind.as_mut()
            && let Some(entry) = entries.last_mut()
        {
            entry.push_definition(DefinitionBody::new(text, spans));
        }
        return;
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
        if let Some(li) = state
            .stack
            .iter()
            .rposition(|f| matches!(f.kind, Some(BlockKind::List { .. })))
        {
            depth = state.stack[li].list_depth;
            if matches!(
                state.stack[li].kind,
                Some(BlockKind::List { ordered: true, .. })
            ) {
                ordinal = Some(state.stack[li].list_next_ordinal);
                state.stack[li].list_next_ordinal += 1;
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
        if let Some(root) = root_list_index(&state.stack) {
            let list = &mut state.stack[root];
            list.is_task |= open.item_checked.is_some();
            // 注意这里**不能**顺手拼 `open.content`：项是按
            // 由内向外的顺序闭合的，拼出来是乱序且带任务标记的
            // 文本；扁平内容统一在列表块定型时按源码顺序重建
            // （见 `list_plain_text`）。
            list.items.push(item);
        }
        return;
    }

    // 其余行内标签（强调 / 链接 / 删除线 / 图片的 alt 文本等）：
    // 进出栈即可，**不产出块**——它们的文本已由内层块收集。
    if open.is_inline {
        return;
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
        && let Some(owner) = absorbing_index(&state.stack)
    {
        state.stack[owner].pending_break = true;
        return;
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
        && let Some(owner) = absorbing_index(&state.stack)
    {
        // 分隔规则与 `push_content` 的 `pending_break` 一致：
        // 先补换行再写入，片段基点取换行之后的长度，区间不错位。
        if state.stack[owner].pending_break {
            state.stack[owner].pending_break = false;
            if !state.stack[owner].content.is_empty() {
                state.stack[owner].content.push('\n');
            }
        }
        let base = state.stack[owner].content.len();
        state.stack[owner].content.push_str(&open.content);
        for span in std::mem::take(&mut open.spans) {
            state.stack[owner].spans.push(InlineSpan::new(
                base + span.start(),
                base + span.end(),
                span.style(),
            ));
        }
        return;
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
        && root_list_index(&state.stack).is_some()
    {
        return;
    }

    // 图片：产出携带 `alt` / `url` 的独立块（供 S5-11 加载资源）。
    if let Some((alt, url)) = open.image.take() {
        state.blocks.push(RenderedBlock::new(
            BlockId::from_u64(state.next_id),
            BlockKind::Image { alt, url },
            open.range(end, text),
            open.content,
        ));
        state.next_id += 1;
        return;
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
        // `kind` 为 None 说明是无需产出块的标签，已在上面 return。
        (None, _) => return,
    };

    // 列表项排序：项在各自的 `End(Item)` 收集，而嵌套列表的 End
    // 由内向外到达，收集顺序天然是「先子后父」（实测得到
    // `[三级 x, 二级 a, 二级 b, 一级, 一级二]`）。不排序的话预览里
    // 子项会整片跑到父项**上面**。按源码起点排即恢复文档顺序。
    let mut kind = kind;
    if let BlockKind::List { items, .. } | BlockKind::TaskList { items } = &mut kind {
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

    state.blocks.push(
        RenderedBlock::new(BlockId::from_u64(state.next_id), kind, range, content)
            .with_spans(spans),
    );
    state.next_id += 1;
}
