//! `PulldownMarkdownParser` 单测（S5-02）。
//!
//! 断言分三类：
//! 1. **结构**：块类型 / 层级 / 顺序是否符合 P0 需求；
//! 2. **源码区间**：每个块的 `source_range` 必须真实覆盖其在源码中的位置——
//!    这是 `FR-02`（滚动同步）与 `FR-03`（增量高亮）能否实现的前提，
//!    故本文件用「取该区间切片应包含预期文本」的方式做**实质性**校验，
//!    而非只断言区间非空；
//! 3. **映射稳定性**：`BlockMapper` 对块首/ 块中 / 块尾 / 空隙 / 空输入的返回。

use rustmark::domain::markdown::services::{BlockMapper, MarkdownParser};
use rustmark::domain::markdown::value_objects::{
    BlockKind, ImageAlign, ImageDisplay, InlineStyle, RenderedBlock,
};
use rustmark::infrastructure::parsing::pulldown_parser::PulldownMarkdownParser;
use rustmark::shared::geometry::ByteOffset;

fn parse(text: &str) -> Vec<rustmark::domain::markdown::value_objects::RenderedBlock> {
    PulldownMarkdownParser::new()
        .parse(text)
        .expect("解析不应失败")
}

/// 文档里出现的**全部**图片：`(alt, url, display)`。
///
/// 图片是行内元素：写着前缀文案时（`图片: ![a](u.png)`）块保持段落类型、
/// 图片挂在块上（问题 032），只统计 `BlockKind::Image` 会把这些图整片漏掉。
fn all_images(blocks: &[RenderedBlock]) -> Vec<(String, String, ImageDisplay)> {
    blocks
        .iter()
        .flat_map(|block| match block.kind() {
            BlockKind::Image { alt, url, display } => {
                vec![(alt.clone(), url.clone(), *display)]
            }
            _ => block
                .images()
                .iter()
                .map(|image| {
                    (
                        image.alt().to_string(),
                        image.url().to_string(),
                        image.display(),
                    )
                })
                .collect(),
        })
        .collect()
}

/// 取某个块的源码切片（用于校验 `source_range` 的正确性）。
fn source_of(text: &str, index: usize) -> String {
    let blocks = parse(text);
    let range = blocks[index].source_range();
    text[range.as_usize_range()].to_string()
}

#[test]
fn empty_document_yields_no_blocks() {
    assert!(parse("").is_empty());
    assert!(parse("\n\n\n").is_empty(), "只有空行时不应产出块");
}

#[test]
fn paragraph_is_parsed_with_source_range() {
    let text = "hello world";
    let blocks = parse(text);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].kind(), &BlockKind::Paragraph);
    assert_eq!(blocks[0].content(), "hello world");
    assert_eq!(source_of(text, 0), "hello world", "区间应覆盖整段");
}

#[test]
fn headings_carry_level_and_text() {
    let text = "# 一级\n\n## 二级\n\n### 三级\n\n#### 四级\n\n##### 五级\n\n###### 六级\n";
    let blocks = parse(text);
    let headings: Vec<_> = blocks
        .iter()
        .filter_map(|b| b.kind().heading_level().map(|level| (level, b.content())))
        .collect();
    assert_eq!(
        headings,
        vec![
            (1, "一级"),
            (2, "二级"),
            (3, "三级"),
            (4, "四级"),
            (5, "五级"),
            (6, "六级"),
        ]
    );
}

#[test]
fn paragraph_containing_markdown_strips_markers_from_content() {
    // 强调 / 行内代码的标记不应出现在 content 里（大纲与搜索用纯文本）。
    let blocks = parse("这是 **粗体** 与 `代码` 文本");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].kind(), &BlockKind::Paragraph);
    assert_eq!(blocks[0].content(), "这是 粗体 与 代码 文本");
    assert!(
        !blocks[0].content().contains("**"),
        "content 不应残留 Markdown 标记"
    );
}

#[test]
fn unordered_and_ordered_lists_are_distinguished() {
    let text = "- alpha\n- beta\n\n1. one\n2. two\n";
    let blocks = parse(text);
    let lists: Vec<_> = blocks
        .iter()
        .filter_map(|b| match b.kind() {
            BlockKind::List { ordered, .. } => Some(*ordered),
            _ => None,
        })
        .collect();
    assert_eq!(lists, vec![false, true], "应依次为无序、有序列表");
}

#[test]
fn task_list_items_are_detected() {
    let text = "- [ ] 待办\n- [x] 已完成\n- 普通项\n";
    let blocks = parse(text);
    assert!(
        blocks.iter().any(|b| b.kind().is_task_list()),
        "含 TaskListMarker 的列表应产出 TaskList 块，实际：{:?}",
        blocks.iter().map(|b| b.kind()).collect::<Vec<_>>()
    );
}

#[test]
fn nested_list_preserves_structure() {
    let text = "- 外层\n  - 内层\n";
    let blocks = parse(text);
    assert!(
        !blocks.is_empty(),
        "嵌套列表不应产出空结果（实际块数：{}）",
        blocks.len()
    );
    // 块必须按源码起点升序（BlockMapper 的二分查找前提）。
    let starts: Vec<usize> = blocks
        .iter()
        .map(|b| b.source_range().start().value())
        .collect();
    let mut sorted = starts.clone();
    sorted.sort_unstable();
    assert_eq!(starts, sorted, "块必须按 source_range.start 升序");
}

#[test]
fn code_block_captures_language_and_body() {
    let text = "```rust\nfn main() {}\n```\n";
    let blocks = parse(text);
    let code = blocks
        .iter()
        .find(|b| b.kind().is_code())
        .expect("应产出代码块");
    assert_eq!(
        code.kind(),
        &BlockKind::CodeBlock {
            lang: Some("rust".to_string())
        }
    );
    assert!(code.content().contains("fn main"), "代码内容应保留");
    assert!(
        source_of(text, 0).contains("```rust"),
        "代码块区间应覆盖围栏本身"
    );
}

#[test]
fn code_block_without_language_has_none() {
    let blocks = parse("```\nplain\n```\n");
    let code = blocks
        .iter()
        .find(|b| b.kind().is_code())
        .expect("应有代码块");
    assert_eq!(code.kind(), &BlockKind::CodeBlock { lang: None });
}

#[test]
fn gfm_table_is_parsed() {
    let text = "| 列一 | 列二 |\n| --- | --- |\n| a | b |\n| c | d |\n";
    let blocks = parse(text);
    assert!(
        blocks.iter().any(|b| b.kind().table().is_some()),
        "应产出 Table 块，实际：{:?}",
        blocks.iter().map(|b| b.kind()).collect::<Vec<_>>()
    );
}

#[test]
fn blockquote_is_parsed() {
    let blocks = parse("> 引用内容\n");
    assert!(blocks.iter().any(|b| b.kind() == &BlockKind::Quote));
}

#[test]
fn blockquote_body_stays_inside_the_quote_block() {
    // 缺陷现场：正文段落照常成块，DTO 变成 `[Quote(空), Paragraph(正文)]`，
    // 预览里就是一根空色条 + 游离在色条**外面**的正文。
    let blocks = parse("> 引用正文\n");
    assert_eq!(
        blocks.len(),
        1,
        "引用正文不该再产出游离段落，实际：{:?}",
        blocks.iter().map(|b| b.kind()).collect::<Vec<_>>()
    );
    assert_eq!(blocks[0].kind(), &BlockKind::Quote);
    assert_eq!(blocks[0].content(), "引用正文");
}

#[test]
fn blockquote_keeps_inline_styles_and_paragraph_breaks() {
    let blocks = parse("> 见 **重点**\n>\n> 第二段\n");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].content(), "见 重点\n第二段");
    let bold = span_text(&blocks[0], InlineStyle::Strong);
    assert_eq!(bold, vec!["重点"], "回流给引用的行内样式区间不应丢失");
}

#[test]
fn nested_blockquote_merges_into_the_outer_quote() {
    // 内层若照常成块，外层就是空色条、内层文字在色条外面；并入外层后
    // 只留一个 Quote 块，文字完整（嵌套层级在扁平架构下不单独画条）。
    let blocks = parse("> > 深层引用\n");
    assert_eq!(
        blocks.len(),
        1,
        "嵌套引用不应产出空壳 Quote + 深层 Quote，实际：{:?}",
        blocks.iter().map(|b| b.kind()).collect::<Vec<_>>()
    );
    assert_eq!(blocks[0].kind(), &BlockKind::Quote);
    assert_eq!(blocks[0].content(), "深层引用");
}

#[test]
fn blockquote_adjacent_paragraphs_stay_separate() {
    // 引用与后续正文是两个块；引用吸掉的只是**自己内部**的段落。
    let blocks = parse("> 引用\n\n正文\n");
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].kind(), &BlockKind::Quote);
    assert_eq!(blocks[0].content(), "引用");
    assert_eq!(blocks[1].kind(), &BlockKind::Paragraph);
    assert_eq!(blocks[1].content(), "正文");
}

#[test]
fn thematic_break_is_parsed() {
    // `---` 必须解析成**分割线块**（`BlockKind::Rule`），而不是：
    // - 被丢弃（预览画不出水平线，滚动同步也会漏掉这条结构）；
    // - 当普通段落（预览画不出线，还会把 `---` 当正文显示出来）。
    // pulldown-cmark 为分隔线发**独立的** `Event::Rule`（无配对标签），
    // 解析器据此当场产出 `Rule` 块（见 `pulldown_parser/inline.rs` 的
    // `Event::Rule` 分支）。
    let text = "a\n\n---\n\nb\n";
    let blocks = parse(text);

    assert_eq!(
        blocks.len(),
        3,
        "应产出「段落 + 分割线 + 段落」三块，实际：{:?}",
        blocks.iter().map(|b| b.kind()).collect::<Vec<_>>()
    );
    assert_eq!(blocks[0].kind(), &BlockKind::Paragraph);
    assert_eq!(blocks[0].content(), "a");
    assert_eq!(
        blocks[1].kind(),
        &BlockKind::Rule,
        "`---` 应解析成分割线（Rule）而非普通段落或被丢弃"
    );
    assert_eq!(blocks[2].kind(), &BlockKind::Paragraph);
    assert_eq!(blocks[2].content(), "b");
    // 分割线块的源码区间必须恰好覆盖 `---`（滚动同步按区间定位，
    // 裁掉尾随换行是解析器的既有行为）。
    assert_eq!(
        source_of(text, 1),
        "---",
        "分割线块的源码区间应恰好覆盖 `---`"
    );
}

#[test]
fn footnote_definition_is_parsed() {
    let text = "正文[^1]\n\n[^1]: 脚注内容\n";
    let blocks = parse(text);
    let note = blocks
        .iter()
        .find(|b| matches!(b.kind(), BlockKind::Footnote { .. }))
        .unwrap_or_else(|| {
            panic!(
                "应产出 Footnote 块，实际：{:?}",
                blocks.iter().map(|b| b.kind()).collect::<Vec<_>>()
            )
        });
    // 标签与正文必须各归各位：`[^1]` 是标签、`脚注内容` 是正文。
    // 若解析时只为段落帧记 content 而不回流到脚注帧，这里拿到的是空串，
    // 正文会以**游离段落**的形式出现在文档流里（曾经的实测表现）。
    match note.kind() {
        BlockKind::Footnote { label } => assert_eq!(label, "1"),
        other => panic!("类型不符：{other:?}"),
    }
    assert_eq!(note.content(), "脚注内容", "脚注正文应回流进本块");
    // 正文不应该再作为独立段落重复出现。
    assert!(
        !blocks
            .iter()
            .any(|b| matches!(b.kind(), BlockKind::Paragraph) && b.content().contains("脚注内容")),
        "脚注正文不应额外产出游离段落：{:?}",
        blocks.iter().map(|b| b.kind()).collect::<Vec<_>>()
    );
    assert!(
        text.contains("[^1]"),
        "输入本身应含脚注引用（sanity check）"
    );
}

#[test]
fn image_becomes_its_own_block_with_url() {
    let text = "![替代文字](images/a.png)\n";
    let blocks = parse(text);
    let image = blocks
        .iter()
        .find(|b| matches!(b.kind(), BlockKind::Image { .. }))
        .expect("应产出图片块");
    match image.kind() {
        BlockKind::Image { url, .. } => assert_eq!(url, "images/a.png"),
        other => panic!("类型不符：{other:?}"),
    }
}

#[test]
fn raw_html_is_marked_as_raw_block() {
    let blocks = parse("<div>原始 HTML</div>\n");
    assert!(
        blocks
            .iter()
            .any(|b| matches!(b.kind(), BlockKind::Raw { .. })),
        "原始 HTML 应落Raw 块，实际：{:?}",
        blocks.iter().map(|b| b.kind()).collect::<Vec<_>>()
    );
}

#[test]
fn mixed_document_produces_ordered_blocks_covering_content() {
    let text = "# 标题\n\n段落一。\n\n- 列表项\n\n```js\ncode();\n```\n\n> 引用\n\n结尾。\n";
    let blocks = parse(text);
    assert!(
        blocks.len() >= 6,
        "应至少产出 6 个块，实际 {}",
        blocks.len()
    );

    // 每个块的区间切片都不应panic，且首块起点为0、末块终点为全文长度附近。
    for block in &blocks {
        let range = block.source_range();
        let slice = &text[range.as_usize_range()];
        assert!(
            range.end().value() <= text.len(),
            "区间终点越界：{range:?} vs 长度 {}",
            text.len()
        );
        assert!(!slice.is_empty(), "块区间不应为空切片：{range:?}");
    }
    assert_eq!(blocks[0].source_range().start(), ByteOffset::new(0));
}

#[test]
fn multibyte_chinese_offsets_are_byte_accurate() {
    // 中文每字 3 字节。若source_range 按**字符**而非字节计算，区间会错位。
    let text = "# 中文标题\n\n中文段落内容。\n";
    let blocks = parse(text);
    assert_eq!(blocks.len(), 2);
    let heading_slice = &text[blocks[0].source_range().as_usize_range()];
    assert_eq!(heading_slice, "# 中文标题", "标题区间应按字节精确切分");
    let para_slice = &text[blocks[1].source_range().as_usize_range()];
    assert!(para_slice.contains("中文段落内容"), "段落区间应覆盖正文");
}

#[test]
fn crlf_documents_parse_without_panic() {
    let blocks = parse("# 标题\r\n\r\n段落\r\n\r\n- 项\r\n");
    assert!(!blocks.is_empty());
    assert_eq!(blocks[0].kind().heading_level(), Some(1));
}

#[test]
fn block_mapper_is_stable_across_offset_kinds() {
    let text = "# 标题\n\n第一段内容。\n\n第二段内容。\n";
    let blocks = parse(text);
    assert!(blocks.len() >= 3);

    let second = &blocks[1];
    let range = second.source_range();
    let mid = range.start().value() + range.len() / 2;

    // 块首
    assert_eq!(
        BlockMapper::block_at_offset(&blocks, range.start()),
        Some(second.id())
    );
    // 块中
    assert_eq!(
        BlockMapper::block_at_offset(&blocks, ByteOffset::new(mid)),
        Some(second.id())
    );
    // 块尾（终点归入该块，避免出现「不属于任何块」的空档）
    assert_eq!(
        BlockMapper::block_at_offset(&blocks, range.end()),
        Some(second.id())
    );
    // 早于首块 → 归入首块
    assert_eq!(
        BlockMapper::block_at_offset(&blocks, ByteOffset::new(0)),
        Some(blocks[0].id())
    );
    // 空输入
    assert_eq!(BlockMapper::block_at_offset(&[], ByteOffset::new(0)), None);
}

#[test]
fn ratio_is_zero_at_block_start_and_one_at_end() {
    let text = "第一段内容。\n\n第二段内容。\n";
    let blocks = parse(text);
    let range = blocks[0].source_range();
    assert!(!range.is_empty());

    let (_, at_start) = BlockMapper::offset_to_block(&blocks, range.start()).expect("应命中块");
    let (_, at_end) = BlockMapper::offset_to_block(&blocks, range.end()).expect("应命中块");
    assert!((at_start - 0.0).abs() < f32::EPSILON, "块首比例应为 0.0");
    assert!((at_end - 1.0).abs() < f32::EPSILON, "块尾比例应为 1.0");
}

#[test]
fn large_document_parses_within_perf_budget() {
    // NFR-PERF-06：10KB 文档解析 ≤ 5ms。此处用 100 倍 于 10KB 的输入做压力，
    // 断言仅「不超时到不可用」——CI 机器性能差异大，过紧的耗时断言会频繁误报。
    let unit = "段落内容，包含中文与 English mixed text。\n\n";
    let text = unit.repeat(2000);
    assert!(text.len() > 100_000, "样本应显著大于 10KB");

    let start = std::time::Instant::now();
    let blocks = PulldownMarkdownParser::new()
        .parse(&text)
        .expect("大文档解析不应失败");
    let elapsed = start.elapsed();

    assert!(!blocks.is_empty(), "大文档应产出块");
    assert!(
        elapsed.as_millis() < 500,
        "解析 {} 字节耗时 {}ms，超出合理范围",
        text.len(),
        elapsed.as_millis()
    );
}

// ============================================================================
// S6-05 预览所需的结构化内容
//
// 早期实现把列表项 / 单元格当行内标签处理（只保栈配平、不收集文本），
// 于是所有文本被并入外层块：列表变成 "项一项二"、表格变成 "列A列B1234"，
// 预览区根本无法还原结构。以下用例逐项钉住「结构必须留在解析结果里」。
// ============================================================================

use rustmark::domain::markdown::value_objects::{CellAlignment, ListItem};

#[test]
fn heading_levels_are_preserved_in_kind() {
    let text = "# 一\n\n### 三\n\n###### 六\n";
    let levels: Vec<u8> = parse(text)
        .iter()
        .filter_map(|b| b.kind().heading_level())
        .collect();
    assert_eq!(levels, vec![1, 3, 6], "h1 / h3 / h6 的级别不得丢失");
}

#[test]
fn code_block_language_is_preserved() {
    let blocks = parse("```rust\nfn main() {}\n```\n");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].kind().language(), Some("rust"));

    // 没写语言时不报错，语言为 None（而不是空字符串——那会让 UI 显示空的标签）。
    let bare = parse("```\nplain code\n```\n");
    assert_eq!(bare[0].kind().language(), None);
}

#[test]
fn unordered_list_items_are_separated() {
    let blocks = parse("- 苹果\n- 香蕉\n- 橘子\n");
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].kind().is_list());

    let items = blocks[0].kind().list_items().expect("列表应带 items");
    let texts: Vec<&str> = items.iter().map(ListItem::text).collect();
    assert_eq!(
        texts,
        vec!["苹果", "香蕉", "橘子"],
        "列表项必须分开，而不是粘成一坨「苹果香蕉橘子」"
    );
    // 普通列表项没有勾选状态。
    assert!(items.iter().all(|i| i.checked().is_none()));
}

#[test]
fn ordered_list_is_flagged_and_items_are_separated() {
    let blocks = parse("1. 第一\n2. 第二\n");
    match blocks[0].kind() {
        BlockKind::List { ordered, items } => {
            assert!(ordered, "1. 开头应是有序列表");
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].text(), "第一");
        }
        other => panic!("期望 List，实际 {other:?}"),
    }
}

#[test]
fn task_list_records_checked_state_per_item() {
    let blocks = parse("- [x] 已完成\n- [ ] 未完成\n- [x] 也完成\n");
    assert_eq!(blocks.len(), 1);
    assert!(
        blocks[0].kind().is_task_list(),
        "含任务标记的列表应定型为 TaskList"
    );

    let items = blocks[0].kind().list_items().expect("任务列表应带 items");
    let checked: Vec<Option<bool>> = items.iter().map(|i| i.checked()).collect();
    assert_eq!(checked, vec![Some(true), Some(false), Some(true)]);

    // 文本里不应残留 `[x] ` 前缀——勾选状态已由 checked 单独承载，
    // 残留会导致预览里出现「[x] ☑ 已完成」这样的重复。
    let texts: Vec<&str> = items.iter().map(ListItem::text).collect();
    assert_eq!(texts, vec!["已完成", "未完成", "也完成"]);
}

#[test]
fn task_item_source_range_covers_the_real_source() {
    let text = "- [ ] 待办事项\n";
    let blocks = parse(text);
    let items = blocks[0].kind().list_items().expect("任务项应存在");

    // 回写勾选状态需要精确的源码区间，否则 UI 只能全文模糊匹配。
    let range = items[0].source_range().as_usize_range();
    let slice = &text[range];
    assert!(
        slice.contains("[ ]") && slice.contains("待办事项"),
        "任务项的源码区间应覆盖该项真实内容，实际 {slice:?}"
    );
}

#[test]
fn table_captures_header_rows_and_alignment() {
    let text = "| 名称 | 数量 | 价格 |\n| :--- | :--: | ---: |\n| A | 1 | 9.9 |\n| B | 2 | 19 |\n";
    let blocks = parse(text);
    assert_eq!(blocks.len(), 1, "表格应是单个块");

    let data = blocks[0].kind().table().expect("表格应有结构化数据");
    assert_eq!(data.columns(), 3);
    assert_eq!(data.header(), &["名称", "数量", "价格"]);
    assert_eq!(data.rows().len(), 2, "两条表体行");
    assert_eq!(data.rows()[0], &["A", "1", "9.9"]);
    assert_eq!(data.rows()[1], &["B", "2", "19"]);
    assert_eq!(
        data.aligns(),
        &[
            CellAlignment::Left,
            CellAlignment::Center,
            CellAlignment::Right
        ],
        "列对齐应由 GFM 分隔行的冒号决定"
    );
}

#[test]
fn table_rows_are_padded_to_column_count() {
    // GFM 允许行末缺单元格；pulldown 会照源码补出空单元格
    //（`| 只有一列 |` 的行尾竖线本身就意味着一个空的第二列）。
    // 对渲染器的意义是：每行长度一定等于列数，可以按列号安全取值。
    let blocks = parse("| A | B |\n| --- | --- |\n| 只有一列 |\n");
    let data = blocks[0].kind().table().expect("表格应有数据");
    assert_eq!(data.rows().len(), 1);
    assert_eq!(data.rows()[0], &["只有一列", ""]);
    assert_eq!(data.rows()[0].len(), data.columns(), "每行长度应与列数一致");

    // 完全没有尾竖线时同样补齐，不得少一格。
    let short = parse("| A | B |\n| --- | --- |\n| 单列\n");
    let short_data = short[0].kind().table().expect("表格应有数据");
    assert_eq!(short_data.rows()[0].len(), short_data.columns());
}

#[test]
fn image_url_is_preserved() {
    let blocks = parse("![替代文本](images/a.png)\n");
    assert_eq!(blocks.len(), 1);
    match blocks[0].kind() {
        BlockKind::Image { alt, url, .. } => {
            assert_eq!(alt, "替代文本");
            assert_eq!(url, "images/a.png", "图片地址丢失会让预览无从加载");
        }
        other => panic!("期望 Image，实际 {other:?}"),
    }
}

#[test]
fn list_content_still_contains_flat_text_for_search() {
    // 结构化不应牺牲原有能力：块的扁平 content 必须仍可用于大纲 / 搜索。
    let blocks = parse("- [x] 已完成\n- [ ] 未完成\n");
    let content = blocks[0].content();
    assert!(content.contains("已完成"), "内容应包含项文本：{content:?}");
    assert!(content.contains("未完成"), "内容应包含项文本：{content:?}");
    assert!(
        content.contains("[x]"),
        "任务标记也应留在扁平内容里（搜索「[x]」要能命中）：{content:?}"
    );
}

#[test]
fn loose_list_items_still_collect_their_text() {
    // 松散列表（项之间空行）在 pulldown 里会多包一层 Paragraph，
    // 若只按「最近非行内帧」收集，列表项文本会是空的。
    let blocks = parse("- 第一项\n\n- 第二项\n");
    let list = blocks
        .iter()
        .find(|b| b.kind().is_list())
        .expect("应产出列表块");
    let texts: Vec<String> = list
        .kind()
        .list_items()
        .expect("items")
        .iter()
        .map(|i| i.text().to_string())
        .collect();
    assert_eq!(texts, vec!["第一项", "第二项"], "松散列表的项不应为空");
}

// ============================================================================
// 数学公式（P1）
// ============================================================================

#[test]
fn inline_and_display_math_are_distinguished() {
    // 行内 `$...$` 属于段落内容，不独立成块（否则一句话会被拆成三块）；
    // 但 `$` 分隔符必须留在 content 里，否则大纲 / 搜索看不到公式。
    let inline = parse("质能方程 $E = mc^2$ 很有名。\n");
    assert!(
        matches!(inline[0].kind(), BlockKind::Paragraph),
        "行内公式所在块应仍是段落，实际 {:?}",
        inline[0].kind()
    );
    assert!(
        inline[0].content().contains("$E = mc^2$"),
        "行内公式应保留分隔符：{:?}",
        inline[0].content()
    );

    // 整段只有一个块级公式时升级为独立 `Math` 块，content 为公式本体。
    let display = parse("$$\n\\int_0^1 x^2 dx\n$$\n");
    assert!(
        matches!(display[0].kind(), BlockKind::Math { display: true }),
        "块级公式应为 display=true，实际 {:?}",
        display[0].kind()
    );
    assert_eq!(display[0].content(), "\\int_0^1 x^2 dx");
}

#[test]
fn display_math_mixed_with_text_stays_a_paragraph() {
    // 混排公式属于正文：拆成独立块会割裂语义，也���破坏段落渲染。
    let blocks = parse("由 $$x^2$$ 可知增长。\n");
    assert!(
        matches!(blocks[0].kind(), BlockKind::Paragraph),
        "混排公式不应独立成块，实际 {:?}",
        blocks[0].kind()
    );
}

#[test]
fn math_without_delimiters_is_not_a_math_block() {
    // 未启用数学选项时 pulldown 不产生数学事件；这里锁定「普通美元符号不被误判」，
    // 避免将来新增规则把货币写法 `$5 and $10` 拆成公式。
    let blocks = parse("It costs $5 and $10 total.\n");
    assert!(
        !blocks
            .iter()
            .any(|b| matches!(b.kind(), BlockKind::Math { .. })),
        "货币写法不应被识别为公式：{:?}",
        blocks.iter().map(|b| b.kind().clone()).collect::<Vec<_>>()
    );
}

// ============================================================================
// Mermaid 图表（P1）
// ============================================================================

#[test]
fn mermaid_fence_becomes_diagram_block() {
    // ` ```mermaid ` 围栏必须落到 `Diagram` 而非普通代码块——
    // 否则预览区无从区分「我想出图」和「我只是贴段代码」。
    let blocks = parse("```mermaid\ngraph TD;\n  A-->B;\n```\n");
    match blocks[0].kind() {
        BlockKind::Diagram { kind } => assert_eq!(kind, "mermaid"),
        other => panic!("期望 Diagram，实际 {other:?}"),
    }
    assert!(
        blocks[0].content().contains("graph TD;"),
        "图表源码应完整保留：{:?}",
        blocks[0].content()
    );
}

#[test]
fn mermaid_detection_is_case_insensitive_and_lang_scoped() {
    // 围栏语言大小写不敏感（```Mermaid 也要认），
    // 但 `mermaid-ish` 这类前缀相似语言不应被误判。
    let upper = parse("```Mermaid\ngraph LR;\n```\n");
    assert!(
        matches!(upper[0].kind(), BlockKind::Diagram { .. }),
        "大写 Mermaid 也应识别，实际 {:?}",
        upper[0].kind()
    );

    let other = parse("```mermaid-ish\ngraph LR;\n```\n");
    assert!(
        matches!(other[0].kind(), BlockKind::CodeBlock { .. }),
        "相似语言名不应误判为图表，实际 {:?}",
        other[0].kind()
    );
}

// ============================================================================
// 行内格式：快捷键 / 目录 / 高亮 / 上下标（CSDN 示例文档回归）
// ============================================================================
//
// 这批用例守的是同一类缺陷：**pulldown 不认识的写法被原样留在正文里**，
// 用户在预览里看到的就是「一堆标记符号 + 没渲染的语法」。
// 三个成因各不相同，逐个钉住：
//   1. `</kbd>` 在 CommonMark 里不算 HTML → 走普通文本事件；
//   2. `@[TOC](标题)` 扁平后只剩 `@TOC`，标题丢失；
//   3. `==高亮==` 没有任何 pulldown 选项，`H~2~O` / `2^10^` 需要前置空白。

/// 取某个块里指定样式的片段文本（按出现顺序）。
fn span_text(
    block: &rustmark::domain::markdown::value_objects::RenderedBlock,
    style: InlineStyle,
) -> Vec<&str> {
    block
        .spans()
        .iter()
        .filter(|s| s.style() == style)
        .map(|s| &block.content()[s.start()..s.end()])
        .collect()
}

#[test]
fn keycap_html_becomes_keycap_spans() {
    // CSDN 的快捷键写法。两个坑叠在一起：
    // 开标签 `<kbd>` 走行内 HTML 事件，而闭标签 `</kbd>` 因 `<` 紧挨字母
    // **不是** CommonMark 的行内 HTML，pulldown 当普通文本吐出来——
    // 只认 `Event::InlineHtml` 的实现会把 `</kbd>` 原样留在正文里。
    let blocks = parse("撤销：<kbd>Ctrl</kbd> + <kbd>Z</kbd>\n");
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].content(),
        "撤销：Ctrl + Z",
        "标签本身不该出现在正文里"
    );
    assert_eq!(span_text(&blocks[0], InlineStyle::Keycap), ["Ctrl", "Z"]);
}

#[test]
fn unbalanced_kbd_does_not_leak_into_later_blocks() {
    // `<kbd>` 没闭合时，合成的键帽帧会一直挂在栈上，把后续**所有**段落
    // 都染成键帽样式（曾实测整篇变样）。块闭合时必须清掉残留帧。
    let blocks = parse("<kbd>Ctrl\n\n后续段落\n");
    let last = blocks.last().expect("应有后续段落");
    assert_eq!(last.content(), "后续段落");
    assert!(
        last.spans().is_empty(),
        "键帽样式不应泄漏到后续段落：{:?}",
        last.spans()
    );
}

#[test]
fn toc_directive_becomes_toc_block_carrying_its_title() {
    // `@[TOC](欢迎)` 在 pulldown 眼里是「文本 @ + 链接 [TOC](欢迎)」，
    // 扁平后只剩 `@TOC`——括号里的标题只有看**源码**才认得出来。
    let blocks = parse("@[TOC](欢迎)\n\n# 一级标题\n");
    assert_eq!(blocks.len(), 2);
    match blocks[0].kind() {
        BlockKind::Toc { title } => assert_eq!(title, "欢迎"),
        other => panic!("期望 Toc，实际 {other:?}"),
    }
    // 表现层不允许依赖领域枚举（架构约束），目录标题只能经 `content` 传过去；
    // 留在扁平内容的 `@TOC` 会让预览把标题显示成「@TOC」。
    assert_eq!(blocks[0].content(), "欢迎");
}

#[test]
fn bare_toc_directive_defaults_to_generic_title() {
    let blocks = parse("[TOC]\n");
    match blocks[0].kind() {
        BlockKind::Toc { title } => assert_eq!(title, "目录"),
        other => panic!("期望 Toc，实际 {other:?}"),
    }

    // 大小写不敏感，`[[TOC]]` 也认。
    let wrapped = parse("[[toc]]\n");
    assert!(
        matches!(wrapped[0].kind(), BlockKind::Toc { .. }),
        "`[[toc]]` 也应识别，实际 {:?}",
        wrapped[0].kind()
    );
}

#[test]
fn ordinary_paragraph_is_not_mistaken_for_toc() {
    // 目录判定必须看整段源码，含 `TOC` 字样的正文不能被吃掉。
    let blocks = parse("这段讲了 TOC 的用法。\n");
    assert!(
        matches!(blocks[0].kind(), BlockKind::Paragraph),
        "含 TOC 字样的正文不应变成目录块，实际 {:?}",
        blocks[0].kind()
    );
}

#[test]
fn double_equals_becomes_mark_spans() {
    // pulldown 没有任何「高亮」选项，`==x==` 只能由解析器自己切分。
    let blocks = parse("==标记文本==\n");
    assert_eq!(blocks[0].content(), "标记文本", "`==` 不应残留");
    assert_eq!(span_text(&blocks[0], InlineStyle::Mark), ["标记文本"]);
}

#[test]
fn subscript_and_superscript_work_without_surrounding_spaces() {
    // 中文写作不写空格（`H~2~O` / `2^10^`），而 pulldown 只在定界符
    // **前面有空白**时才识别（`H ~2~ O` 行、`H~2~O` 不行），故需自行兜底。
    let sub = parse("H~2~O is是液体。\n");
    assert_eq!(sub[0].content(), "H2O is是液体。");
    assert_eq!(span_text(&sub[0], InlineStyle::Subscript), ["2"]);

    let sup = parse("2^10^ 运算\n");
    assert_eq!(sup[0].content(), "210 运算");
    assert_eq!(span_text(&sup[0], InlineStyle::Superscript), ["10"]);
}

#[test]
fn paired_delimiters_split_across_events_are_still_paired() {
    // pulldown 会把 `2^10^` 切成 `Text("2^10")` + `Text("^")`——
    // 定界符的**开与闭分居两个事件**。逐事件扫描时第一段里找不到配对的 `^`
    // （退回字面量），第二段只剩孤立的 `^` 也配不上，结果是
    // 「上标没渲染、`^` 却留在正文里」。解析器因此把相邻文本先合并再扫。
    for text in ["2^10^\n", "x^2^\n", "H~2~O\n", "^10^\n", "2^10^ 运算\n"] {
        let blocks = parse(text);
        let content = blocks[0].content();
        assert!(
            !content.contains('^') && !content.contains('~'),
            "{text:?} 的定界符不应残留在正文里，实际 {content:?}"
        );
        assert!(
            !blocks[0].spans().is_empty(),
            "{text:?} 应产出至少一个样式片段"
        );
    }
}

#[test]
fn standalone_caret_in_prose_is_still_literal() {
    // 兜底扫描不能贪心：孤立、配不上对的定界符必须原样保留。
    let blocks = parse("价格是 3^x 元，另加 ~10% 服务费\n");
    let content = blocks[0].content();
    assert!(content.contains("3^x"), "孤立 `^` 应保留：{content:?}");
    assert!(content.contains("~10%"), "孤立 `~` 应保留：{content:?}");
}

// ============================================================================
// CSDN 图片尺寸后缀 `![alt](url =30x30)`
// ============================================================================
//
// 这不是合法 Markdown：无引号的链接地址**一遇空白即终止**，`=30x30`
// 于是落进括号里成为语法错误，pulldown 索性把整段按普通文本吐出——
// 用户在预览里看到的是「一长串原始 Markdown，还带着 base64 图片地址」。

#[test]
fn csdn_image_size_suffix_still_parses_as_image() {
    let text = "带尺寸的图片: ![Alt](images/a.png =30x30)\n";
    let blocks = parse(text);
    assert_eq!(blocks.len(), 1);
    // 图片前还写着「带尺寸的图片: 」，块必须是**段落**而不是图片块：
    // 换成图片块那段前缀就随块一起消失了（问题 032）。
    assert!(
        matches!(blocks[0].kind(), BlockKind::Paragraph),
        "图片与文字混排时块应保持段落：{:?}",
        blocks[0].kind()
    );
    assert_eq!(blocks[0].content(), "带尺寸的图片: Alt");
    let images = blocks[0].images();
    assert_eq!(images.len(), 1, "图片应挂在块上：{images:?}");
    assert_eq!(images[0].alt(), "Alt");
    assert_eq!(images[0].url(), "images/a.png", "尺寸后缀不应混进图片地址");
    assert_eq!(
        images[0].display(),
        ImageDisplay {
            width: Some(30),
            height: Some(30),
            align: ImageAlign::Left,
        }
    );
}

/// 问题 3（CSDN 语法全链）：`![Alt](url#pic_center =30x30)` 的
/// **对齐标记与尺寸后缀必须随块上行**——脱糖把它们从解析副本里剥掉，
/// 但原文完好，`End(Image)` 锚点可精确取回。渲染层靠这两个字段
/// 区分每张图的尺寸与对齐，而不是所有图片一个样。
#[test]
fn csdn_align_and_size_params_reach_the_image_block() {
    let text = "![Alt](https://imgconvert.csdnimg.cn/aHR0cHM6Ly9hdmF0YXIuY3Nkbi5uZXQvNy83L0IvMV9yYWxmX2h4MTYzY29tLmpwZw#pic_center =30x30)\n";
    let blocks = parse(text);
    assert_eq!(blocks.len(), 1);
    match blocks[0].kind() {
        BlockKind::Image { url, display, .. } => {
            // URL 必须干净：对齐标记与尺寸后缀都不得混进地址
            // （否则加载器按带片段的路径请求，图片必然 404）。
            assert_eq!(
                url,
                "https://imgconvert.csdnimg.cn/aHR0cHM6Ly9hdmF0YXIuY3Nkbi5uZXQvNy83L0IvMV9yYWxmX2h4MTYzY29tLmpwZw"
            );
            assert_eq!(
                *display,
                ImageDisplay {
                    width: Some(30),
                    height: Some(30),
                    align: ImageAlign::Center,
                }
            );
        }
        other => panic!("期望 Image，实际 {other:?}"),
    }
}

/// 左对齐标记 + 非对称尺寸：`#pic_left =50x100` 三个字段各不相同，
/// 验证参数不是「有就全填」的占位。
#[test]
fn left_align_with_asymmetric_size_reaches_the_block() {
    let blocks = parse("![图](images/a.png#pic_left =50x100)\n");
    assert_eq!(blocks.len(), 1);
    match blocks[0].kind() {
        BlockKind::Image { display, .. } => {
            assert_eq!(
                *display,
                ImageDisplay {
                    width: Some(50),
                    height: Some(100),
                    align: ImageAlign::Left,
                }
            );
        }
        other => panic!("期望 Image，实际 {other:?}"),
    }
}

/// 反向顺序（先尺寸后对齐，「位置标记加在最后」）与 canonical 顺序
/// 在全链上完全一致：URL 洗净、参数上行。
#[test]
fn reverse_order_size_then_align_reaches_the_block() {
    let blocks = parse("![Alt](images/a.png =30x30#pic_center)\n");
    assert_eq!(blocks.len(), 1);
    match blocks[0].kind() {
        BlockKind::Image { url, display, .. } => {
            assert_eq!(url, "images/a.png", "URL 必须洗净，否则图片加载不到");
            assert_eq!(
                *display,
                ImageDisplay {
                    width: Some(30),
                    height: Some(30),
                    align: ImageAlign::Center,
                }
            );
        }
        other => panic!("期望 Image，实际 {other:?}"),
    }
}

/// 效果截图 / `docs/logs/示例文档.md` 的四种图片形态全链验证：
/// 无标记（左对齐）、仅尺寸（左对齐）、仅居中、居中 + 尺寸。
#[test]
fn sample_doc_four_image_forms_reach_the_blocks() {
    const URL: &str = "https://imgconvert.csdnimg.cn/aHR0cHM6Ly9hdmF0YXIuY3Nkbi5uZXQvNy83L0IvMV9yYWxmX2h4MTYzY29tLmpwZw";
    let text = format!(
        "图片: ![Alt]({URL})\n\n\
         带尺寸的图片: ![Alt]({URL} =30x30)\n\n\
         居中的图片: ![Alt]({URL}#pic_center)\n\n\
         居中并且带尺寸的图片: ![Alt]({URL}#pic_center =30x30)\n"
    );
    let blocks = parse(&text);
    // 四张图前面都写着「图片: 」这类前缀文案，块保持段落、图挂在块上
    // （问题 032）；`all_images` 同时覆盖「图片块」与「混排图片」两种形态。
    let images = all_images(&blocks);
    let displays: Vec<ImageDisplay> = images.iter().map(|(_, _, d)| *d).collect();
    assert_eq!(
        displays,
        vec![
            // 无标记 → 左对齐、按纹理实际尺寸（效果截图：贴在左缘）。
            ImageDisplay::default(),
            // 仅尺寸 → 左对齐 + 30×30。
            ImageDisplay {
                width: Some(30),
                height: Some(30),
                align: ImageAlign::Left,
            },
            // 仅居中标记 → 居中、实际尺寸。
            ImageDisplay {
                width: None,
                height: None,
                align: ImageAlign::Center,
            },
            // 居中 + 尺寸。
            ImageDisplay {
                width: Some(30),
                height: Some(30),
                align: ImageAlign::Center,
            },
        ]
    );
    // 四种形态的 URL 都必须洗净：任何后缀残留都会让图片加载不到。
    let urls: Vec<&str> = images.iter().map(|(_, u, _)| u.as_str()).collect();
    assert_eq!(urls, vec![URL; 4]);
    // 前缀文案必须留在块里（问题 032 的验收点）。
    assert!(
        blocks[0].content().contains("图片:"),
        "图片前的文案不应被吞：{:?}",
        blocks[0].content()
    );
}

/// 多图段落：每张图按**自己的** `)` 锚点提取，参数互不串扰。
#[test]
fn per_image_params_do_not_leak_between_images() {
    let text = "![a](x.png =1x1) 中间文字 ![b](y.png#pic_right =2x2)";
    let blocks = parse(text);
    assert_eq!(blocks.len(), 1, "混排多图仍是**一个**段落块");
    assert_eq!(blocks[0].content(), "a 中间文字 b");
    let images = all_images(&blocks);
    let displays: Vec<ImageDisplay> = images.iter().map(|(_, _, d)| *d).collect();
    // 两张图**都**要在（旧实现只有最后一张：后者覆盖前者的 `image` 槽），
    // 且各自带自己的参数。
    assert_eq!(
        displays,
        [
            ImageDisplay {
                width: Some(1),
                height: Some(1),
                align: ImageAlign::Left,
            },
            ImageDisplay {
                width: Some(2),
                height: Some(2),
                align: ImageAlign::Right,
            },
        ]
    );
}

// ============================================================================
// 行内图片：图片与文字混排（问题 032）
// ============================================================================
//
// 图片是**行内**元素，前后都能写字。旧实现只要块里出现过图片就把整块替换
// 成图片块，于是「图片: ![a](u.png)」里的「图片: 」被整段吞掉——用户看到的
// 就是「插入图片后前面的文案不见了」。现在的规则：整块只有图才升级为图片块，
// 否则块保持本来类型、图片挂在块 / 列表项上。

/// 图片前后的文案必须留在块里（问题 032 的正面验收）。
#[test]
fn text_around_inline_image_is_not_swallowed() {
    let text = "前面的文案 ![Alt](a.png) 后面的文案";
    let blocks = parse(text);
    assert_eq!(blocks.len(), 1, "混排图文仍是**一个**块");
    assert!(
        matches!(blocks[0].kind(), BlockKind::Paragraph),
        "块应保持段落：{:?}",
        blocks[0].kind()
    );
    assert_eq!(blocks[0].content(), "前面的文案 Alt 后面的文案");
    let images = blocks[0].images();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].alt(), "Alt");
    assert_eq!(images[0].url(), "a.png");
    // 区间必须落在 alt 上：渲染层按它把图片插回文字流，偏一格就会切错字。
    assert_eq!(
        (images[0].start(), images[0].end()),
        ("前面的文案 ".len(), "前面的文案 Alt".len()),
        "图片区间应正好覆盖 alt"
    );
}

/// 整块就一张图（没有别的文字）→ 仍升级为独立的图片块。
#[test]
fn standalone_image_still_promotes_to_image_block() {
    let blocks = parse("![Alt](a.png)\n");
    assert_eq!(blocks.len(), 1);
    assert!(
        matches!(blocks[0].kind(), BlockKind::Image { .. }),
        "独占图片应仍是图片块：{:?}",
        blocks[0].kind()
    );
    assert!(blocks[0].images().is_empty(), "图片块不需要再挂行内图片");
}

/// 引用里的图片：混排时挂在引用块上，独占时整块升级为图片块。
#[test]
fn inline_image_survives_quote_absorption() {
    // 引用会吞掉内层段落，图片必须跟着文字一起上行，否则图就丢了。
    let blocks = parse("> 引用 ![Alt](a.png) 引用后\n");
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0].kind(), BlockKind::Quote));
    assert_eq!(blocks[0].content(), "引用 Alt 引用后");
    let images = blocks[0].images();
    assert_eq!(images.len(), 1);
    assert_eq!((images[0].start(), images[0].end()), (7, 10));

    let only = parse("> ![Alt](a.png)\n");
    assert_eq!(only.len(), 1);
    assert!(
        matches!(only[0].kind(), BlockKind::Image { .. }),
        "引用里只有一张图时仍是图片块：{:?}",
        only[0].kind()
    );
}

/// 列表项里的图片：挂在项上（项是文本容器，无法升级成图片块），
/// 且要跟着任务标记 `[x] ` 的裁剪一起左移。
#[test]
fn inline_image_survives_list_item_and_task_marker() {
    let blocks = parse("- 项前 ![Alt](a.png) 项后\n");
    let items = blocks[0].kind().list_items().expect("列表块");
    assert_eq!(items[0].text(), "项前 Alt 项后");
    assert_eq!(items[0].images().len(), 1);
    assert_eq!(items[0].images()[0].url(), "a.png");

    let task = parse("- [x] ![图标](i.png) 已核对\n");
    let items = task[0].kind().list_items().expect("任务列表块");
    assert_eq!(items[0].text(), "图标 已核对", "任务标记不进项文本");
    let images = items[0].images();
    assert_eq!(images.len(), 1);
    // `图标` 是两个 3 字节汉字：裁剪掉 `[x] ` 后区间从 0 开始。
    assert_eq!(
        (images[0].start(), images[0].end()),
        (0, 6),
        "区间必须随任务标记的裁剪左移"
    );
}

/// 标题里的图片同样按混排处理（标题文字不能丢）。
#[test]
fn inline_image_in_heading_keeps_heading_text() {
    let blocks = parse("# 标题 ![Alt](a.png)\n");
    assert!(matches!(blocks[0].kind(), BlockKind::Heading { level: 1 }));
    assert_eq!(blocks[0].content(), "标题 Alt");
    assert_eq!(blocks[0].images().len(), 1);
}

/// 干净图片（无扩展）→ 默认参数：按纹理实际尺寸、居中。
#[test]
fn plain_image_gets_default_display() {
    let blocks = parse("![图](images/a.png)\n");
    match blocks[0].kind() {
        BlockKind::Image { display, .. } => {
            // 无标记 → **左对齐**（效果截图：CSDN 无标记图片贴在左缘，
            // 要居中必须显式写 `#pic_center`）+ 不声明尺寸。
            assert_eq!(
                *display,
                ImageDisplay {
                    width: None,
                    height: None,
                    align: ImageAlign::Left,
                }
            );
        }
        other => panic!("期望 Image，实际 {other:?}"),
    }
}

/// 问题 3：URL 的图片处理参数（`?x-oss-process=image/resize,w_50`）必须
/// 完整保留到 `BlockKind::Image::url`——预览渲染层靠它区分每张图的
/// 展示尺寸，查询串一旦被吞，所有图片就只能按同一实际尺寸渲染。
#[test]
fn image_url_query_params_survive_parsing() {
    let text = "![图](https://imgconvert.csdnimg.cn/aHR0cHM?x-oss-process=image/resize,w_50)\n";
    let blocks = parse(text);
    assert_eq!(blocks.len(), 1);
    match blocks[0].kind() {
        BlockKind::Image { url, .. } => {
            assert!(
                url.contains("x-oss-process=image/resize,w_50"),
                "查询参数必须完整保留，实际：{url}"
            );
        }
        other => panic!("期望 Image，实际 {other:?}"),
    }
}

/// 带查询参数的 URL 不得被 `desugar_image_sizes` 误伤（`?` 后的内容
/// 不是 CSDN 尺寸后缀，也不是 Typora 对齐后缀）。
#[test]
fn query_params_are_not_mistaken_for_size_suffix() {
    let text = "![a](https://x.test/y.png?x-oss-process=image/resize,m_fill,w_600,h_200)";
    let (src, off) = rustmark::infrastructure::parsing::pulldown_parser::desugar_image_sizes(text);
    assert_eq!(src, text);
    assert!(off.is_identity());
}

#[test]
fn image_size_suffix_removal_keeps_source_ranges_on_original_text() {
    // 删后缀会改变字节偏移；块的 `source_range` 必须仍指向**原文**，
    // 否则点击跳转与滚动同步（FR-02）会整体错位。
    let text = "带尺寸的图片: ![Alt](images/a.png =30x30)\n";
    let blocks = parse(text);
    assert_eq!(
        &text[blocks[0].source_range().as_usize_range()],
        "带尺寸的图片: ![Alt](images/a.png =30x30)"
    );
}

#[test]
fn image_size_suffix_removal_keeps_later_blocks_aligned() {
    // 一处删除会影响其后**所有**偏移，故逐块校验切片。
    let text = "![A](a.png =1x1)\n\n第二段\n\n![B](b.png =2x2)\n\n第三段\n";
    let blocks = parse(text);
    let slices: Vec<String> = blocks
        .iter()
        .map(|b| text[b.source_range().as_usize_range()].to_string())
        .collect();
    assert_eq!(slices.len(), 4, "实际块：{slices:?}");
    assert_eq!(slices[0], "![A](a.png =1x1)");
    assert_eq!(slices[1], "第二段");
    assert_eq!(slices[2], "![B](b.png =2x2)");
    assert_eq!(slices[3], "第三段");

    let urls: Vec<String> = blocks
        .iter()
        .filter_map(|b| match b.kind() {
            BlockKind::Image { url, .. } => Some(url.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(urls, ["a.png", "b.png"]);
}

#[test]
fn size_like_text_outside_a_link_is_left_alone() {
    // 只允许改写「链接 / 图片地址之内」的后缀。正文里出现同形状的文字
    // 绝不能被动到——否则会把用户的内容改坏。
    let text = "尺寸就写成 a =3x4) 这样\n";
    let blocks = parse(text);
    assert_eq!(blocks[0].content(), "尺寸就写成 a =3x4) 这样");
    assert!(blocks[0].spans().is_empty());
}

#[test]
fn size_suffix_inside_a_link_destination_is_also_removed() {
    // 同样形状出现在普通链接里也要认（CSDN 对链接也允许后缀）。
    let blocks = parse("[文档](docs/a.md =1x1)\n");
    match blocks[0].kind() {
        BlockKind::Paragraph => {
            assert_eq!(blocks[0].content(), "文档");
            assert_eq!(span_text(&blocks[0], InlineStyle::Link), ["文档"]);
        }
        other => panic!("期望 Paragraph，实际 {other:?}"),
    }
}

// ============================================================================
// 真实样例：CSDN 导出的整篇文档
// ============================================================================

/// 单条格式的用例各自锁住「认不认」；这条锁的是**互相干扰**。
///
/// 一篇文档里键帽、高亮、上下标、带尺寸图片、目录、公式、脚注、表格全都有，
/// 任何一处漏了状态清理或偏移算错都会在这里露出来
/// （曾实测：一处未闭合的 `<kbd>` 会把后面**整篇**染成键帽样式）。
///
/// 该文件是 `docs/` 下的真实样例而非标准 fixture，缺失时直接跳过，
/// 不让它成为构建的硬依赖。
#[test]
fn real_world_sample_document_parses_cleanly() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/logs/示例文档.md");
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("跳过：样例文档不存在（{path}）");
        return;
    };
    let blocks = parse(&text);
    assert!(!blocks.is_empty(), "样例文档应解析出块");

    // 1) 区间必须逐块递增且落在字符边界上——偏移一旦漂移，
    //    点击跳转与滚动同步会整体错位（`FR-02`）。
    for (i, block) in blocks.iter().enumerate() {
        let range = block.source_range().as_usize_range();
        assert!(
            text.is_char_boundary(range.start) && text.is_char_boundary(range.end),
            "第 {i} 个块的区间 {range:?} 落在字符中间"
        );
        assert!(range.start <= range.end, "第 {i} 个块的区间反了：{range:?}");
        if i > 0 {
            assert!(
                blocks[i - 1].source_range().start() <= block.source_range().start(),
                "块必须按源码起点升序"
            );
        }
    }

    // 2) 目录：`@[TOC](欢迎)` 的标题必须被认出来。
    let toc = blocks
        .iter()
        .find(|b| matches!(b.kind(), BlockKind::Toc { .. }))
        .expect("样例文档应有目录块");
    assert_eq!(toc.content(), "欢迎");

    // 3) 带尺寸后缀的图片也要被认出来，地址里不能混进 `=30x30`。
    //    样例文档里每张图前都写着「图片: 」这类文案，故它们以**行内图片**
    //    挂在段落上（问题 032），不是独立图片块。
    let images = all_images(&blocks);
    assert_eq!(images.len(), 4, "样例文档有四张图片，实际 {images:?}");
    assert!(
        images.iter().all(|(_, u, _)| !u.contains("=30x30")),
        "尺寸后缀不该混进地址：{images:?}"
    );

    // 4) 各类行内片段都要出现。
    let styles: Vec<InlineStyle> = blocks
        .iter()
        .flat_map(|b| b.spans().iter())
        .map(|s| s.style())
        .collect();
    for expected in [
        InlineStyle::Keycap,
        InlineStyle::Mark,
        InlineStyle::Subscript,
        InlineStyle::Superscript,
        InlineStyle::Link,
        InlineStyle::Code,
        InlineStyle::Strong,
    ] {
        assert!(
            styles.contains(&expected),
            "样例文档应产出 {expected:?} 片段"
        );
    }

    // 5) 键帽样式**只**应出现在快捷键那两段里。
    //    泄漏的表现是整篇被染成键帽——这条就是它的回归防线。
    let keycap_blocks = blocks
        .iter()
        .filter(|b| b.spans().iter().any(|s| s.style() == InlineStyle::Keycap))
        .count();
    assert!(
        keycap_blocks <= 3,
        "键帽样式疑似泄漏到 {keycap_blocks} 个块（样例里只有两段用了 <kbd>）"
    );
}

// ============================================================================
// 列表 / 表格 / 定义列表的行内样式与层级（预览区「格式不生效」类问题的防线）
// ============================================================================

/// 取文档里唯一的列表块（有序 / 无序 / 任务列表都算）。
fn only_list(text: &str) -> Vec<rustmark::domain::markdown::value_objects::ListItem> {
    let blocks = parse(text);
    blocks
        .iter()
        .find_map(|b| b.kind().list_items().map(<[_]>::to_vec))
        .unwrap_or_else(|| {
            panic!(
                "应产出列表块：{:?}",
                blocks.iter().map(|b| b.kind()).collect::<Vec<_>>()
            )
        })
}

#[test]
fn nested_list_items_carry_depth_in_source_order() {
    // 层次：一级(0) → 二级 a(1) → 三级 x(2) → 二级 b(1) → 一级二(0)
    let items = only_list("1. 一级\n   - 二级 a\n     - 三级 x\n   - 二级 b\n2. 一级二\n");
    let got: Vec<(u8, &str)> = items.iter().map(|i| (i.depth(), i.text())).collect();
    assert_eq!(
        got,
        vec![
            (0, "一级"),
            (1, "二级 a"),
            (2, "三级 x"),
            (1, "二级 b"),
            (0, "一级二"),
        ],
        "嵌套列表应按源码顺序给出各项，且深度逐层递增"
    );
}

#[test]
fn nested_list_does_not_duplicate_parent_text() {
    // 回归：曾出现「父项文本被子项回流污染」——`一级` 变成 `一级二级 a`。
    let items = only_list("1. 一级\n   - 二级 a\n");
    assert_eq!(items[0].text(), "一级", "父项文本不应含子项文字");
    assert_eq!(items[1].text(), "二级 a");
}

#[test]
fn ordered_list_items_carry_their_own_ordinal() {
    let items = only_list("1. 甲\n2. 乙\n3. 丙\n");
    let got: Vec<Option<u64>> = items
        .iter()
        .map(rustmark::domain::markdown::value_objects::ListItem::ordinal)
        .collect();
    assert_eq!(got, vec![Some(1), Some(2), Some(3)]);
    // 无序列表不带编号（渲染层据此决定画「•」还是画数字）。
    let items = only_list("- 甲\n- 乙\n");
    assert!(items.iter().all(|i| i.ordinal().is_none()));
}

#[test]
fn ordered_list_honours_its_starting_number() {
    // `3.` 起头时首项编号就是 3——渲染层若从 1 自己数就会与源文不符。
    let items = only_list("3. 甲\n4. 乙\n");
    let got: Vec<Option<u64>> = items
        .iter()
        .map(rustmark::domain::markdown::value_objects::ListItem::ordinal)
        .collect();
    assert_eq!(got, vec![Some(3), Some(4)]);
}

#[test]
fn list_item_inline_styles_are_recorded() {
    // 核心回归：列表项的行内样式（粗体 / 斜体 / 删除线 / 代码）此前**全部丢失**，
    // 预览里列表中的 **粗体** 与正文里的表现不一致。
    let items = only_list("- 项 **粗** *斜* ~~删~~ `码`\n");
    let styles: Vec<InlineStyle> = items[0].spans().iter().map(|s| s.style()).collect();
    for expected in [
        InlineStyle::Strong,
        InlineStyle::Emphasis,
        InlineStyle::Strikethrough,
        InlineStyle::Code,
    ] {
        assert!(
            styles.contains(&expected),
            "列表项应记录 {expected:?}：{styles:?}"
        );
    }
    // 片段必须落在**文本**的坐标系里：按区间切片应能取回原词。
    let text = items[0].text();
    for span in items[0].spans() {
        let slice = &text[span.start()..span.end()];
        assert!(
            matches!(slice, "粗" | "斜" | "删" | "码"),
            "片段区间错位，切出 {slice:?}（文本 {text:?}）"
        );
    }
}

#[test]
fn task_marker_removal_keeps_inline_span_offsets_aligned() {
    // 回归：切掉 `[x] ` 后若不移位片段，`重点` 会指向前 4 个字节，
    // 渲染成「前两个字变粗、内容没变」。
    let items = only_list("- [x] **重点**\n");
    assert_eq!(items[0].text(), "重点");
    assert_eq!(items[0].checked(), Some(true));
    let span = &items[0].spans()[0];
    assert_eq!(span.style(), InlineStyle::Strong);
    assert_eq!(&items[0].text()[span.start()..span.end()], "重点");
}

#[test]
fn table_cell_inline_styles_are_recorded() {
    // 核心回归：表格单元格样式此前丢失，`| a **粗** |` 显示成纯文本 `a 粗`。
    let text = "| 左 | 中 |\n|:---|---:|\n| a **粗** | b |\n";
    let blocks = parse(text);
    let data = blocks
        .iter()
        .find_map(|b| b.kind().table())
        .expect("应产出表格块");
    assert_eq!(data.rows()[0][0], "a 粗");
    let spans = data.cell_spans(0, 0);
    assert_eq!(spans.len(), 1, "单元格应记录 1 个片段：{spans:?}");
    assert_eq!(spans[0].style(), InlineStyle::Strong);
    assert_eq!(
        &data.rows()[0][0][spans[0].start()..spans[0].end()],
        "粗",
        "单元格片段区间必须相对**裁剪后**的单元格文本"
    );
}

#[test]
fn table_header_inline_styles_are_recorded() {
    let text = "| **表头** | 中 |\n|:---|---:|\n| a | b |\n";
    let blocks = parse(text);
    let data = blocks.iter().find_map(|b| b.kind().table()).expect("表格");
    assert_eq!(data.header()[0], "表头");
    let spans = data.header_spans(0);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].style(), InlineStyle::Strong);
    assert_eq!(&data.header()[0][spans[0].start()..spans[0].end()], "表头");
}

#[test]
fn definition_list_is_parsed_into_entries() {
    // 核心回归：不开 `ENABLE_DEFINITION_LIST` 时 pulldown 把两行并成一个段落，
    // 预览里显示成「术语 A: 定义 A 的内容」糊在一行。
    let text = "术语 A\n: 定义 A 的内容\n\n术语 B\n: 定义 B 的内容\n";
    let blocks = parse(text);
    let entries = blocks
        .iter()
        .find_map(|b| b.kind().definitions())
        .unwrap_or_else(|| {
            panic!(
                "应产出定义列表块：{:?}",
                blocks.iter().map(|b| b.kind()).collect::<Vec<_>>()
            )
        });
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].term(), "术语 A");
    assert_eq!(entries[0].definitions()[0].text(), "定义 A 的内容");
    assert_eq!(entries[1].term(), "术语 B");
    assert_eq!(entries[1].definitions().len(), 1);
}

#[test]
fn definition_body_is_not_a_stray_paragraph() {
    // 回归：定义正文曾被包在段落里，若不让外层帧吸收就会变成游离段落
    // （预览里定义内容显示两遍）。
    let text = "术语 A\n: 定义 A 的内容\n";
    let blocks = parse(text);
    let stray = blocks
        .iter()
        .filter(|b| matches!(b.kind(), BlockKind::Paragraph) && b.content().contains("定义 A"))
        .count();
    assert_eq!(
        stray,
        0,
        "定义正文不应额外产出段落：{:?}",
        blocks.iter().map(|b| b.kind()).collect::<Vec<_>>()
    );
}

#[test]
fn multiple_definitions_under_one_term_are_kept_apart() {
    let text = "术语 A\n: 第一条\n: 第二条\n";
    let blocks = parse(text);
    let entries = blocks
        .iter()
        .find_map(|b| b.kind().definitions())
        .expect("定义列表");
    let texts: Vec<&str> = entries[0].definitions().iter().map(|d| d.text()).collect();
    assert_eq!(texts, vec!["第一条", "第二条"]);
}
