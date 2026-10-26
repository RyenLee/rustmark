//! pulldown 标签 / 事件 → 领域类型的纯映射：
//! 块类型、行内样式、TOC、任务标记裁剪、列表深度与扁平文本等。

use pulldown_cmark::{CodeBlockKind, HeadingLevel, Tag};

use rustmark_core::domain::markdown::value_objects::{
    BlockKind, CellAlignment, DefinitionEntry, InlineSpan, InlineStyle, ListItem, TableData,
};

use super::stack::OpenBlock;

/// 把 pulldown 的**块级**标签映射为领域层 [`BlockKind`]。
///
/// 返回 `None` 表示该标签**不单独产出块**，分两类：
/// - `Tag::Item`：其内容已由内部的段落块覆盖，若自己也成块会在大纲 / 滚动映射里
///   产生「列表项」这一层噪声节点；
/// - **行内**标签（强调、链接、删除线等）：它们在事件流里同样是 Start/End 配对，
///   但**不是块级结构**——必须与Item 一样「进出栈但不产出块」。
///   若漏判这类标签，段落里的 `**粗体**` 会凭空多出一个 `Raw` 块。
pub(super) fn block_kind_of(tag: &Tag, ordered: bool) -> Option<BlockKind> {
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
/// `block_kind_of`）。它们的作用是给内部文本打上样式，供预览做行内富文本。
pub(super) fn inline_style_of(tag: &Tag) -> Option<InlineStyle> {
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
pub(super) fn toc_title_of_source(src: &str) -> Option<String> {
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
/// 勾选状态已由 `ListItem::checked` 单独承载，渲染时画成复选框；
/// 若文本里再留一份 `[x] ` 就会重复显示。未标记的项原样返回。
///
/// # 为什么必须一起挪片段
///
/// 片段区间是相对 `content` 的字节偏移。标记被切掉后若不移位，
/// `- [x] **重点**` 的 `重点` 会指向错误的位置（偏移 4 字节），
/// 渲染结果是「前两个字变粗、真正的内容没变」——比不渲染更难排查。
pub(super) fn strip_task_marker(text: &str, spans: Vec<InlineSpan>) -> (String, Vec<InlineSpan>) {
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
pub(super) fn trim_spans(text: &str, spans: Vec<InlineSpan>) -> Vec<InlineSpan> {
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
pub(super) fn list_depth_of(stack: &[OpenBlock]) -> u8 {
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
pub(super) fn root_list_index(stack: &[OpenBlock]) -> Option<usize> {
    stack
        .iter()
        .position(|f| matches!(f.kind, Some(BlockKind::List { .. })))
}

/// 最近的、会吞掉内层段落的帧下标（列表项 / 脚注定义 / 定义列表的定义 / 引用）。
pub(super) fn absorbing_index(stack: &[OpenBlock]) -> Option<usize> {
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
/// （否则预览里「复选框 + 字面 [x]」重复显示），但扁平文本要留着。
/// 两者是同一份内容面向不同消费方的两种投影，故分开生成。
pub(super) fn list_plain_text(items: &[ListItem]) -> String {
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
pub(super) fn definition_list_plain_text(entries: &[DefinitionEntry]) -> String {
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
pub(super) fn cell_alignment_of(align: &pulldown_cmark::Alignment) -> CellAlignment {
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
