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
use rustmark::domain::markdown::value_objects::BlockKind;
use rustmark::infrastructure::parsing::pulldown_parser::PulldownMarkdownParser;
use rustmark::shared::geometry::ByteOffset;

fn parse(text: &str) -> Vec<rustmark::domain::markdown::value_objects::RenderedBlock> {
    PulldownMarkdownParser::new()
        .parse(text)
        .expect("解析不应失败")
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
fn thematic_break_is_parsed() {
    // pulldown-cmark 不为分隔线产出标签，这里只验证**不崩溃**且段落仍正确切分。
    let blocks = parse("a\n\n---\n\nb\n");
    assert!(!blocks.is_empty());
}

#[test]
fn footnote_definition_is_parsed() {
    let text = "正文[^1]\n\n[^1]: 脚注内容\n";
    let blocks = parse(text);
    assert!(
        blocks.iter().any(|b| b.kind() == &BlockKind::Footnote),
        "应产出 Footnote 块，实际：{:?}",
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
        BlockKind::Image { alt, url } => {
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
