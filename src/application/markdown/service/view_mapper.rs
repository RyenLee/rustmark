//! 领域渲染产物 → DTO 视图的映射函数（块 / 行内片段 / 样式稳定字符串）。

use crate::domain::markdown::value_objects::{
    BlockKind, CellAlignment, InlineSpan, InlineStyle, RenderedBlock, SpanStyle,
};

use crate::application::dto::{
    BlockView, CellAlignView, DefinitionBodyView, DefinitionEntryView, ListItemView, RangeView,
    SpanView, TableView,
};

/// 渲染块 → DTO 视图。
pub(super) fn to_block_view(block: &RenderedBlock) -> BlockView {
    let kind = block.kind();
    BlockView {
        id: block.id().as_u64(),
        kind: block_kind_str(kind),
        content: block.content().to_string(),
        level: kind.heading_level(),
        lang: kind.language().map(|l| l.to_string()),
        ordered: matches!(kind, BlockKind::List { ordered: true, .. }),
        items: kind.list_items().map_or_else(Vec::new, |items| {
            items
                .iter()
                .map(|item| ListItemView {
                    text: item.text().to_string(),
                    checked: item.checked(),
                    start: item.source_range().start().value(),
                    end: item.source_range().end().value(),
                    depth: item.depth(),
                    ordinal: item.ordinal(),
                    spans: spans_to_view(item.spans()),
                })
                .collect()
        }),
        table: kind.table().map(|data| TableView {
            header: data.header().to_vec(),
            rows: data.rows().to_vec(),
            aligns: data
                .aligns()
                .iter()
                .map(|a| match a {
                    CellAlignment::None => CellAlignView::None,
                    CellAlignment::Left => CellAlignView::Left,
                    CellAlignment::Center => CellAlignView::Center,
                    CellAlignment::Right => CellAlignView::Right,
                })
                .collect(),
            // 片段表要**按列补齐到与表体等长**：源文里某些行可能少写单元格
            // （`| a | b |` 后面一行只写 `| c |`），此时解析器只收集到 1 组
            // 片段而 `rows` 仍是 2 行。渲染层按 `(r, c)` 索引时若越界就得
            // 处处判空，补齐在这里一次性做掉。
            header_spans: (0..data.header().len())
                .map(|c| spans_to_view(data.header_spans(c)))
                .collect(),
            rows_spans: (0..data.rows().len())
                .map(|r| {
                    (0..data.columns())
                        .map(|c| spans_to_view(data.cell_spans(r, c)))
                        .collect()
                })
                .collect(),
        }),
        alt: match kind {
            BlockKind::Image { alt, .. } => Some(alt.clone()),
            _ => None,
        },
        url: match kind {
            BlockKind::Image { url, .. } => Some(url.clone()),
            _ => None,
        },
        marker: match kind {
            BlockKind::Raw { marker } => Some(marker.clone()),
            _ => None,
        },
        display: match kind {
            BlockKind::Math { display } => Some(*display),
            _ => None,
        },
        diagram: match kind {
            BlockKind::Diagram { kind } => Some(kind.clone()),
            _ => None,
        },
        label: kind.footnote_label().map(str::to_string),
        definitions: kind.definitions().map_or_else(Vec::new, |entries| {
            entries
                .iter()
                .map(|entry| DefinitionEntryView {
                    term: entry.term().to_string(),
                    term_spans: spans_to_view(entry.term_spans()),
                    definitions: entry
                        .definitions()
                        .iter()
                        .map(|body| DefinitionBodyView {
                            text: body.text().to_string(),
                            spans: spans_to_view(body.spans()),
                        })
                        .collect(),
                })
                .collect()
        }),
        spans: spans_to_view(block.spans()),
    }
}

/// 领域行内片段 → DTO 片段（区间 + 样式字符串）。
///
/// 单独提出来是因为**五处**都要做同一件事（块 / 列表项 / 定义术语 / 定义正文 /
/// 表格单元格）。各自内联一遍 `map(...)` 的话，将来往 `SpanView` 加字段
/// （比如「是否可点击」）就会漏改其中几处，且漏改后仍能编译。
fn spans_to_view(spans: &[InlineSpan]) -> Vec<SpanView> {
    spans
        .iter()
        .map(|span| SpanView {
            range: RangeView::new(span.start(), span.end()),
            style: inline_style_str(span.style()).to_string(),
            url: span.dest().map(|d| d.to_string()),
        })
        .collect()
}

/// 行内样式 → 稳定字符串（供表现层分发渲染器）。
fn inline_style_str(style: InlineStyle) -> &'static str {
    match style {
        InlineStyle::Strong => "strong",
        InlineStyle::Emphasis => "emphasis",
        InlineStyle::Strikethrough => "strikethrough",
        InlineStyle::Code => "code",
        InlineStyle::Link => "link",
        InlineStyle::Math => "math",
        InlineStyle::Keycap => "keycap",
        InlineStyle::Mark => "mark",
        InlineStyle::Subscript => "sub",
        InlineStyle::Superscript => "sup",
        InlineStyle::FootnoteRef => "footnote_ref",
    }
}

/// 块类型 → 稳定字符串（供表现层分发渲染器）。
fn block_kind_str(kind: &BlockKind) -> String {
    match kind {
        BlockKind::Heading { .. } => "heading".to_string(),
        BlockKind::Paragraph => "paragraph".to_string(),
        BlockKind::CodeBlock { .. } => "code_block".to_string(),
        BlockKind::Table { .. } => "table".to_string(),
        // 任务列表与普通列表共用渲染器（按 `items[].checked` 有无区分），
        // 故只输出 `list` 会让任务列表丢掉「可勾选」语义。
        BlockKind::List { .. } => "list".to_string(),
        BlockKind::TaskList { .. } => "task_list".to_string(),
        BlockKind::Quote => "quote".to_string(),
        BlockKind::Image { .. } => "image".to_string(),
        BlockKind::Rule => "rule".to_string(),
        // 标签（`label`）与正文分开展示，但**共用同一个 kind 字符串**：
        // 表现层只需要一个渲染器，标签从 `BlockView::label` 读。
        BlockKind::Footnote { .. } => "footnote".to_string(),
        // 定义列表（`术语` / `: 定义`）——与普通列表渲染形态不同（无项目符号、
        // 定义要缩进在术语下方），故单列一种 kind。
        BlockKind::DefinitionList { .. } => "definition_list".to_string(),
        // 行内 / 块级公式共用 `math` 类型，由 `BlockView::display` 区分呈现。
        BlockKind::Math { .. } => "math".to_string(),
        BlockKind::Diagram { .. } => "diagram".to_string(),
        // 目录：`@[TOC](标题)` / `[TOC]`，预览里渲染成可点击的标题列表。
        BlockKind::Toc { .. } => "toc".to_string(),
        BlockKind::Raw { marker } => format!("raw:{marker}"),
    }
}

/// 语法样式 → 稳定字符串（供表现层取色）。
pub(super) fn span_style_str(style: SpanStyle) -> String {
    match style {
        SpanStyle::Plain => "plain".to_string(),
        SpanStyle::Heading { .. } => "heading".to_string(),
        SpanStyle::Emphasis => "emphasis".to_string(),
        SpanStyle::Strong => "strong".to_string(),
        SpanStyle::Strikethrough => "strikethrough".to_string(),
        SpanStyle::Code => "code".to_string(),
        SpanStyle::Link => "link".to_string(),
        SpanStyle::Image => "image".to_string(),
        SpanStyle::Quote => "quote".to_string(),
        SpanStyle::ListMarker => "list_marker".to_string(),
        SpanStyle::TableBorder => "table_border".to_string(),
        SpanStyle::TaskMarker { done } => format!("task_marker:{}", done),
        SpanStyle::Keyword => "keyword".to_string(),
        SpanStyle::String => "string".to_string(),
        SpanStyle::Comment => "comment".to_string(),
        SpanStyle::Number => "number".to_string(),
        SpanStyle::Function => "function".to_string(),
        SpanStyle::Type => "type".to_string(),
        SpanStyle::Variable => "variable".to_string(),
        SpanStyle::Operator => "operator".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::spans_to_view;
    use crate::domain::markdown::value_objects::{InlineSpan, InlineStyle};

    /// 超链接修复（S6-05 #9）：链接目标的 `dest` 必须下沉到 DTO 的 `url`，
    /// 否则预览区点击链接时拿不到地址（见 `preview_view::handle_span_click`）。
    #[test]
    fn link_dest_propagates_to_span_view_url() {
        let spans = vec![InlineSpan::new_link(
            0,
            11,
            InlineStyle::Link,
            "https://example.com".to_string(),
        )];
        let views = spans_to_view(&spans);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].style, "link");
        assert_eq!(views[0].url.as_deref(), Some("https://example.com"));
    }

    /// 非链接片段不得携带 `url`：链接目标与「粗体 / 斜体」等纯样式正交，
    /// 不应污染普通片段。
    #[test]
    fn non_link_span_has_no_url() {
        let spans = vec![
            InlineSpan::new(0, 4, InlineStyle::Strong),
            InlineSpan::new_link(5, 16, InlineStyle::Link, "https://x.test".to_string()),
        ];
        let views = spans_to_view(&spans);
        assert_eq!(views[0].url, None);
        assert_eq!(views[1].url.as_deref(), Some("https://x.test"));
    }
}
