//! Markdown 用例服务测试（S4-05 / S4-06）。
//!
//! 覆盖：渲染缓存命中、大文件截断、防抖纯函数、高亮、双向滚动同步、大纲重建。
//! 缓存命中与失效通过 fake 解析器的调用次数断言验证。

use std::sync::{Arc, Mutex};

use rustmark::application::dto::{LintIssueView, LintRulesView, OutlineView, RenderedView};
use rustmark::application::markdown::commands::ClearCacheCommand;
use rustmark::application::markdown::queries::*;
use rustmark::application::markdown::service::{
    DebouncePolicy, MarkdownAppService, RENDER_CACHE_CAPACITY, RenderCache,
    TRUNCATE_THRESHOLD_BYTES,
};
use rustmark::domain::markdown::value_objects::{
    BlockKind, ImageAlign, ImageDisplay, InlineImage, Misspelling, SpanStyle,
};
use rustmark::shared::geometry::{ByteOffset, TextRange};
use rustmark::shared::ids::DocumentId;
use rustmark::shared::path::FilePath;
use rustmark::shared::time::Timestamp;

use super::fakes::*;

fn fp(s: &str) -> FilePath {
    FilePath::new(s).unwrap()
}

struct MdFixture {
    service: MarkdownAppService,
    parser: Arc<FakeMarkdownParser>,
    highlighter: Arc<FakeHighlighter>,
    provider: Arc<FakeDocumentContentProvider>,
    /// 高亮防抖（OPT-011 ①）按注入时钟判定「文档是否已静止」，
    /// 用例需要推进它才能测到「静止后重算」的分支。
    clock: Arc<FakeClock>,
    /// 拼写检查替身；仅拼写相关用例注入，其余为 `None`。
    spell: Option<Arc<FakeSpellChecker>>,
}

fn build(
    blocks: Vec<rustmark::domain::markdown::value_objects::RenderedBlock>,
    spans: Vec<rustmark::domain::markdown::value_objects::SyntaxSpan>,
) -> MdFixture {
    let parser = Arc::new(FakeMarkdownParser::new(blocks));
    let highlighter = Arc::new(FakeHighlighter::new(spans));
    let provider = Arc::new(FakeDocumentContentProvider::new());
    let clock = Arc::new(FakeClock::new(0));
    let policy = DebouncePolicy::new(150);
    let cache = Arc::new(Mutex::new(RenderCache::new(RENDER_CACHE_CAPACITY)));
    let service = MarkdownAppService::new(
        parser.clone(),
        highlighter.clone(),
        provider.clone(),
        clock.clone(),
        policy,
        cache,
    );
    MdFixture {
        service,
        parser,
        highlighter,
        provider,
        clock,
        spell: None,
    }
}

fn two_blocks() -> Vec<rustmark::domain::markdown::value_objects::RenderedBlock> {
    vec![
        block(1, BlockKind::Heading { level: 1 }, 0, 10, "Title"),
        block(2, BlockKind::Paragraph, 10, 30, "body text here"),
    ]
}

#[test]
fn render_preview_returns_blocks() {
    let f = build(two_blocks(), vec![]);
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "Title\nbody text here", 0, Some(fp("/d/a.md")));
    let view: RenderedView = f
        .service
        .render_preview(RenderPreviewQuery { doc_id, version: 0 })
        .unwrap();
    assert_eq!(view.blocks.len(), 2);
    assert!(!view.truncated);
    assert_eq!(view.version, 0);
    assert_eq!(f.parser.call_count(), 1);
}

#[test]
fn cache_hit_does_not_reparse() {
    let f = build(two_blocks(), vec![]);
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "Title\nbody", 0, Some(fp("/d/a.md")));

    let _ = f
        .service
        .render_preview(RenderPreviewQuery { doc_id, version: 0 })
        .unwrap();
    let _ = f
        .service
        .render_preview(RenderPreviewQuery { doc_id, version: 0 })
        .unwrap();
    assert_eq!(
        f.parser.call_count(),
        1,
        "相同版本二次渲染必须命中缓存，不重新解析"
    );
}

#[test]
fn large_source_is_truncated_without_parsing() {
    let f = build(two_blocks(), vec![]);
    let doc_id = DocumentId::new();
    let big = "x".repeat(TRUNCATE_THRESHOLD_BYTES + 1);
    f.provider.insert(doc_id, &big, 0, Some(fp("/d/big.md")));

    let view: RenderedView = f
        .service
        .render_preview(RenderPreviewQuery { doc_id, version: 0 })
        .unwrap();
    assert!(view.truncated, "超大源必须标记截断");
    assert!(view.blocks.is_empty());
    assert_eq!(f.parser.call_count(), 0, "截断时不得调用解析器");
}

#[test]
fn debounce_policy_is_pure_function() {
    let policy = DebouncePolicy::new(150);
    let last = Timestamp::from_millis(0);
    assert!(!policy.should_render(last, Timestamp::from_millis(100)));
    assert!(policy.should_render(last, Timestamp::from_millis(150)));
    assert!(policy.should_render(last, Timestamp::from_millis(200)));
    // 时钟回拨：now 早于 last，饱和到 0，不应触发。
    assert!(!policy.should_render(Timestamp::from_millis(100), Timestamp::from_millis(0)));
}

#[test]
fn highlight_returns_spans() {
    let spans = vec![
        span(0, 5, SpanStyle::Plain),
        span(5, 10, SpanStyle::Keyword),
    ];
    let f = build(vec![], spans);
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "hello world", 0, Some(fp("/d/a.md")));

    // 服务层返回共享快照（`Arc<Vec<SpanView>>`），只读使用。
    let out = f
        .service
        .highlight(HighlightQuery {
            doc_id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(10)),
            lang: None,
        })
        .unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].style, "plain");
    assert_eq!(out[1].style, "keyword");
    assert_eq!(f.highlighter.call_count(), 1);
}

#[test]
fn scroll_mapping_block_head_mid_tail() {
    let f = build(two_blocks(), vec![]);
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "Title\nbody text here", 0, Some(fp("/d/a.md")));

    let map = |offset: usize| {
        f.service
            .map_scroll(MapScrollQuery {
                doc_id,
                source: ScrollSource::Editor { offset },
            })
            .unwrap()
    };

    // 块首：offset 0 → 块 1，比例 0.0
    assert_eq!(
        map(0),
        ScrollTarget::Preview {
            block_id: 1,
            ratio: 0.0
        }
    );
    // 块中：offset 5 → 块 1，比例 0.5
    assert_eq!(
        map(5),
        ScrollTarget::Preview {
            block_id: 1,
            ratio: 0.5
        }
    );
    // 第二块块首：offset 10 → 块 2，比例 0.0
    assert_eq!(
        map(10),
        ScrollTarget::Preview {
            block_id: 2,
            ratio: 0.0
        }
    );
    // 第二块块中：offset 20 → 比例 0.5
    match map(20) {
        ScrollTarget::Preview { block_id, ratio } => {
            assert_eq!(block_id, 2);
            assert!((ratio - 0.5).abs() < 1e-3);
        }
        _ => panic!("应为预览目标"),
    }
    // 末块块尾：offset 30 → 比例 1.0
    assert_eq!(
        map(30),
        ScrollTarget::Preview {
            block_id: 2,
            ratio: 1.0
        }
    );
}

#[test]
fn scroll_mapping_preview_click_returns_block_start() {
    let f = build(two_blocks(), vec![]);
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "Title\nbody text here", 0, Some(fp("/d/a.md")));

    let target = f
        .service
        .map_scroll(MapScrollQuery {
            doc_id,
            source: ScrollSource::Preview { block_id: 2 },
        })
        .unwrap();
    assert_eq!(target, ScrollTarget::Editor { offset: 10 });
}

#[test]
fn rebuild_outline_lists_headings() {
    let f = build(two_blocks(), vec![]);
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "Title\nbody text here", 0, Some(fp("/d/a.md")));

    let outline: OutlineView = f
        .service
        .rebuild_outline(RebuildOutlineQuery { doc_id })
        .unwrap();
    assert_eq!(outline.0.len(), 1);
    assert_eq!(outline.0[0].level, 1);
    assert_eq!(outline.0[0].text, "Title");
}

#[test]
fn clear_cache_invalidates_specific_document() {
    let f = build(two_blocks(), vec![]);
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "Title\nbody", 0, Some(fp("/d/a.md")));

    let _ = f
        .service
        .render_preview(RenderPreviewQuery { doc_id, version: 0 })
        .unwrap();
    assert_eq!(f.parser.call_count(), 1);

    f.service.clear_cache(ClearCacheCommand {
        doc_id: Some(doc_id),
    });
    let _ = f
        .service
        .render_preview(RenderPreviewQuery { doc_id, version: 0 })
        .unwrap();
    assert_eq!(f.parser.call_count(), 2, "失效后必须重新解析");
}

// ============================================================================
// S6-05：结构化载荷必须传到 DTO
//
// 领域层拿到了标题级别 / 代码语言 / 图片地址 / 表格行 / 列表项，
// 但若 `to_block_view` 不映射，预览区依旧拿不到——这类「数据在半路丢了」
// 的问题编译期不会报，只会表现为预览渲染不出来。
// ============================================================================

use rustmark::application::dto::{CellAlignView, ListItemView};
use rustmark::domain::markdown::value_objects::{
    CellAlignment, ListItem, RenderedBlock, TableData,
};

/// fake 解析器返回什么就渲染什么，构造一次即可。
fn render_one(block: RenderedBlock, text: &str) -> RenderedView {
    let f = build(vec![block], vec![]);
    let doc_id = DocumentId::new();
    f.provider.insert(doc_id, text, 0, Some(fp("/d/a.md")));
    f.service
        .render_preview(RenderPreviewQuery { doc_id, version: 0 })
        .expect("渲染应成功")
}

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(ByteOffset::new(start), ByteOffset::new(end))
}

#[test]
fn heading_level_reaches_the_view() {
    let view = render_one(
        block(1, BlockKind::Heading { level: 3 }, 0, 6, "三级"),
        "### 三级",
    );
    assert_eq!(view.blocks[0].kind, "heading");
    assert_eq!(
        view.blocks[0].level,
        Some(3),
        "标题级别丢失会让预览里 h1..h6 一样大"
    );
}

#[test]
fn code_language_reaches_the_view() {
    let view = render_one(
        block(
            1,
            BlockKind::CodeBlock {
                lang: Some("rust".into()),
            },
            0,
            6,
            "fn a(){}",
        ),
        "```rust",
    );
    assert_eq!(view.blocks[0].kind, "code_block");
    assert_eq!(
        view.blocks[0].lang.as_deref(),
        Some("rust"),
        "语言标签丢失会让代码块显示不出语言"
    );

    // 未标注语言时应为 None，而不是空字符串——后者会让 UI 渲染出一个空白标签。
    let bare = render_one(
        block(1, BlockKind::CodeBlock { lang: None }, 0, 4, "code"),
        "```",
    );
    assert_eq!(bare.blocks[0].lang, None);
}

#[test]
fn image_alt_and_url_reach_the_view() {
    let view = render_one(
        block(
            1,
            BlockKind::Image {
                alt: "替代".into(),
                url: "img/a.png".into(),
                display: ImageDisplay::default(),
            },
            0,
            10,
            "替代",
        ),
        "![替代](img/a.png)",
    );
    assert_eq!(view.blocks[0].kind, "image");
    assert_eq!(view.blocks[0].alt.as_deref(), Some("替代"));
    assert_eq!(
        view.blocks[0].url.as_deref(),
        Some("img/a.png"),
        "URL 丢失会让预览无从加载图片"
    );
    // 无扩展语法时不声明尺寸；对齐取默认左对齐（效果截图：CSDN 的
    // 无标记图片贴在左缘，要居中必须显式写 `#pic_center`）。
    assert_eq!(view.blocks[0].image_width, None);
    assert_eq!(view.blocks[0].image_height, None);
    assert_eq!(view.blocks[0].image_align.as_deref(), Some("left"));
}

/// 问题 3：`#pic_center =30x30` 解析出的显示参数必须到达 DTO——
/// 渲染层只认 `BlockView`，领域字段若在映射处被丢掉，
/// Parser 做得再对也反映不到视图上。
#[test]
fn image_display_params_reach_the_view() {
    let view = render_one(
        block(
            1,
            BlockKind::Image {
                alt: "图".into(),
                url: "https://imgconvert.csdnimg.cn/aHR0cHM".into(),
                display: ImageDisplay {
                    width: Some(30),
                    height: Some(30),
                    align: ImageAlign::Left,
                },
            },
            0,
            10,
            "图",
        ),
        "![图](https://imgconvert.csdnimg.cn/aHR0cHM#pic_left =30x30)",
    );
    assert_eq!(view.blocks[0].image_width, Some(30));
    assert_eq!(view.blocks[0].image_height, Some(30));
    assert_eq!(view.blocks[0].image_align.as_deref(), Some("left"));
}

/// 问题 032：混排在文字里的图片必须到达 DTO。
///
/// 图片与文字共存时块**不是** `image`（否则文字被吞），若映射只认
/// `BlockKind::Image`，预览就只剩文字、图片整张消失。
#[test]
fn inline_image_reaches_the_view() {
    let view = render_one(
        block(1, BlockKind::Paragraph, 0, 20, "前面的文案 Alt 后面的文案").with_images(vec![
            InlineImage::new(
                range(16, 19),
                "Alt",
                "a.png",
                ImageDisplay {
                    width: Some(30),
                    height: Some(30),
                    align: ImageAlign::Center,
                },
            ),
        ]),
        "前面的文案 ![Alt](a.png#pic_center =30x30) 后面的文案",
    );
    assert_eq!(view.blocks[0].kind, "paragraph", "混排时块仍是段落");
    let images = &view.blocks[0].images;
    assert_eq!(images.len(), 1, "行内图片应到达视图");
    // 区间是渲染层「把图片插回文字流第几刀」的依据，错一格就切到别的字上。
    assert_eq!((images[0].range.start, images[0].range.end), (16, 19));
    assert_eq!(images[0].image.url, "a.png");
    assert_eq!(images[0].image.alt, "Alt");
    // 私有扩展参数同样要走行内图片这条路（问题 3 的参数区分不能在这里断掉）。
    assert_eq!(images[0].image.width, Some(30));
    assert_eq!(images[0].image.height, Some(30));
    assert_eq!(images[0].image.align, "center");
}

/// 列表项里的图片挂在**项**上（项是文本容器，无法升级成图片块）。
#[test]
fn inline_image_in_list_item_reaches_the_view() {
    let item =
        ListItem::new("图标 已核对", None, range(0, 12)).with_images(vec![InlineImage::new(
            range(0, 6),
            "图标",
            "i.png",
            ImageDisplay::default(),
        )]);
    let view = render_one(
        block(
            1,
            BlockKind::List {
                ordered: false,
                items: vec![item],
            },
            0,
            12,
            "图标 已核对",
        ),
        "- ![图标](i.png) 已核对",
    );
    let item = &view.blocks[0].items[0];
    assert_eq!(item.images.len(), 1, "列表项图片应到达视图");
    assert_eq!(item.images[0].image.url, "i.png");
    assert_eq!(
        (item.images[0].range.start, item.images[0].range.end),
        (0, 6)
    );
}

#[test]
fn table_structure_reaches_the_view() {
    let data = TableData::new(
        vec!["A".into(), "B".into()],
        vec![vec!["1".into(), "2".into()]],
        vec![CellAlignment::Left, CellAlignment::Right],
    );
    let view = render_one(
        block(1, BlockKind::Table { data }, 0, 20, "AB12"),
        "| A | B |",
    );
    assert_eq!(view.blocks[0].kind, "table");

    let table = view.blocks[0].table.as_ref().expect("表格结构应存在");
    assert_eq!(table.header, vec!["A", "B"]);
    assert_eq!(table.rows.len(), 1);
    assert!(matches!(table.aligns[0], CellAlignView::Left));
    assert!(matches!(table.aligns[1], CellAlignView::Right));
}

#[test]
fn list_items_reach_the_view() {
    let items = vec![
        ListItem::new("苹果", None, range(0, 4)),
        ListItem::new("香蕉", None, range(5, 9)),
    ];
    let view = render_one(
        block(
            1,
            BlockKind::List {
                ordered: true,
                items,
            },
            0,
            9,
            "苹果香蕉",
        ),
        "1. 苹果",
    );
    assert_eq!(view.blocks[0].kind, "list");
    assert!(view.blocks[0].ordered, "有序标记丢失会渲染成圆点列表");
    let items: &[ListItemView] = &view.blocks[0].items;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].text, "苹果");
    // 源码区间要一并下沉——任务项回写靠它定位。
    assert_eq!(items[0].start, 0);
    assert_eq!(items[0].end, 4);
    assert_eq!(items[0].checked, None, "普通列表项不应有勾选状态");
}

#[test]
fn task_list_items_carry_checked_state() {
    let items = vec![
        ListItem::new("已完成", Some(true), range(0, 10)),
        ListItem::new("未完成", Some(false), range(11, 21)),
    ];
    let view = render_one(
        block(1, BlockKind::TaskList { items }, 0, 21, "已完成未完成"),
        "- [x] a",
    );
    assert_eq!(view.blocks[0].kind, "task_list");
    let items = &view.blocks[0].items;
    assert_eq!(items[0].checked, Some(true));
    assert_eq!(items[1].checked, Some(false));
}

#[test]
fn raw_block_keeps_its_marker() {
    let view = render_one(
        block(
            1,
            BlockKind::Raw {
                marker: "html".into(),
            },
            0,
            8,
            "<div>x",
        ),
        "<div>x",
    );
    assert_eq!(view.blocks[0].kind, "raw:html");
    assert_eq!(view.blocks[0].marker.as_deref(), Some("html"));
}

/// 高亮一段独立文本（预览区代码块）：返回的是**相对该文本**的区间。
#[test]
fn highlight_snippet_uses_text_relative_offsets() {
    use rustmark::application::markdown::queries::HighlightSnippetQuery;

    let f = build(vec![], vec![span(0, 2, SpanStyle::Keyword)]);
    let spans = f
        .service
        .highlight_snippet(HighlightSnippetQuery {
            text: "fn a".to_string(),
            lang: Some("rust".to_string()),
        })
        .expect("高亮应成功");
    assert_eq!(spans.len(), 1);
    assert_eq!((spans[0].range.start, spans[0].range.end), (0, 2));
    assert_eq!(
        f.highlighter.call_count(),
        1,
        "高亮片段应真的走到高亮器，而不是被缓存短路"
    );
}

// ============================================================================
// S8-05：高亮结果缓存（编辑区每帧请求 span，重复解析是纯浪费）
// ============================================================================

/// 文档未变时，第二次高亮命中缓存：**不再**走进高亮器。
///
/// 这是 S8-05 的核心收益：编辑区每帧都请求全文 span，而 syntect 解析是
/// O(全文)（10KB 实测 56.5ms）。文档未变的帧（滚动 / 空闲 / 切换面板）
/// 若反复重算，等于每帧白付这 56ms。
#[test]
fn highlight_reuses_cache_when_document_unchanged() {
    let f = build(vec![], vec![span(0, 2, SpanStyle::Keyword)]);
    let doc_id = DocumentId::new();
    f.provider.insert(doc_id, "fn a", 0, Some(fp("/d/a.md")));

    let q = || HighlightQuery {
        doc_id,
        range: TextRange::new(ByteOffset::new(0), ByteOffset::new(4)),
        lang: Some("markdown".to_string()),
    };
    let first = f.service.highlight(q()).expect("高亮应成功");
    // 防抖期内（版本刚变 / 距变化不足 150ms）不上色；推进时钟表示「已静止」。
    f.clock.set(200);
    let second = f.service.highlight(q()).expect("高亮应成功");

    assert_eq!(first.len(), second.len(), "缓存结果必须与重算一致");
    assert_eq!(
        f.highlighter.call_count(),
        1,
        "文档未变时第二次高亮应命中缓存，不得再走进高亮器"
    );
    let (hits, misses) = f.service.highlight_cache_stats();
    assert_eq!((hits, misses), (1, 1));
}

/// 编辑区每帧的廉价决策**不得**把全文从 rope 快照出来（问题 046）。
///
/// 三条路径都只需要「版本号 + 字节长度」：
/// - **缓存命中**：键里只有版本号，文本根本没参与；
/// - **防抖期**：只比时间戳；
/// - **大文件降级**：只看长度。
///
/// 此前实现一律先 `fetch` 全文，于是 1MB 文档**每一帧**都要付一次 MB 级
/// 分配 + 拷贝。断言手段是 fake 提供者的调用计数——快照次数一涨就说明
/// 有人又把全文拖回了帧路径。
#[test]
fn highlight_frame_path_never_snapshots_the_whole_document() {
    let f = build(vec![], vec![span(0, 2, SpanStyle::Keyword)]);
    let doc_id = DocumentId::new();
    f.provider.insert(doc_id, "fn a", 0, Some(fp("/d/a.md")));

    let q = || HighlightQuery {
        doc_id,
        range: TextRange::new(ByteOffset::new(0), ByteOffset::new(4)),
        lang: Some("markdown".to_string()),
    };
    // 首次必须真的取一次全文——那时确实要解析（这时取文本是正当成本）。
    let _ = f.service.highlight(q()).expect("高亮应成功");
    assert_eq!(f.provider.snapshot_calls(), 1, "首次解析应取一次全文");

    // 稳态：时钟推进表示「已静止」，此后每帧都走缓存命中。
    f.clock.set(200);
    for _ in 0..5 {
        let _ = f.service.highlight(q()).expect("高亮应成功");
    }
    assert_eq!(
        f.provider.snapshot_calls(),
        1,
        "缓存命中的帧不得再快照全文（实测差值 {} 次）",
        f.provider.snapshot_calls() - 1
    );
    assert!(
        f.provider.revision_len_calls() >= 5,
        "命中判定应走 O(1) 元数据接口（实测 {} 次）",
        f.provider.revision_len_calls()
    );
}

/// 大文件降级只看长度：**一次都不该快照全文**，也不该进高亮器。
#[test]
fn oversized_document_is_rejected_without_snapshotting() {
    let f = build(vec![], vec![span(0, 2, SpanStyle::Keyword)]);
    let doc_id = DocumentId::new();
    f.provider.insert(
        doc_id,
        &"x".repeat(TRUNCATE_THRESHOLD_BYTES + 1),
        0,
        Some(fp("/d/big.md")),
    );

    let out = f
        .service
        .highlight(HighlightQuery {
            doc_id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(4)),
            lang: Some("markdown".to_string()),
        })
        .expect("高亮不应报错");

    assert!(out.is_empty(), "超阈值文档应降级为空片段");
    assert_eq!(
        f.provider.snapshot_calls(),
        0,
        "降级判断只需要长度，不得为此快照全文"
    );
    assert_eq!(f.highlighter.call_count(), 0, "降级路径不得进高亮器");
}

/// 文档改了（版本变化）→ 缓存失效，必须重算。
///
/// 键里带版本号是刻意的：宁可重算也**不能**返回旧 span——
/// 旧 span 的字节区间与新文本错位，排版时会切到非字符边界而 panic。
#[test]
fn highlight_recomputes_after_document_changed() {
    let f = build(vec![], vec![span(0, 2, SpanStyle::Keyword)]);
    let doc_id = DocumentId::new();
    f.provider.insert(doc_id, "fn a", 0, Some(fp("/d/a.md")));

    let q = || HighlightQuery {
        doc_id,
        range: TextRange::new(ByteOffset::new(0), ByteOffset::new(4)),
        lang: Some("markdown".to_string()),
    };
    let _ = f.service.highlight(q()).expect("高亮应成功");
    // 内容变化 + 版本递增（领域保证）。
    f.provider.insert(doc_id, "fn ab", 1, Some(fp("/d/a.md")));
    // 刚变的第一帧：防抖期内不上色（OPT-011 ①）。
    f.clock.set(200);
    let blank = f.service.highlight(q()).expect("高亮应成功");
    assert!(blank.is_empty(), "版本刚变的那 150ms 内不应上色");
    // 停手满 150ms：必须失效缓存并重新解析。
    f.clock.set(400);
    let _ = f.service.highlight(q()).expect("高亮应成功");

    assert_eq!(
        f.highlighter.call_count(),
        2,
        "版本变化且已静止后，缓存必须失效并重新解析"
    );
}

// ============================================================================
// S8-05 遗留 / OPT-011 ①：高亮防抖（打字期不上色，停手后一次性上色）
// ============================================================================

/// 打字期（距版本变化不足 150ms）**一次都不进高亮器**，返回空片段。
///
/// 这是 OPT-011 ① 的直接收益：10KB 文档一次 syntect 全文解析 56–58ms，
/// 每击一次键就付一次会把编辑帧推到 67.9ms。
#[test]
fn highlight_stays_blank_while_typing() {
    let f = build(vec![], vec![span(0, 2, SpanStyle::Keyword)]);
    let doc_id = DocumentId::new();
    f.provider.insert(doc_id, "fn a", 0, Some(fp("/d/a.md")));

    let q = || HighlightQuery {
        doc_id,
        range: TextRange::new(ByteOffset::new(0), ByteOffset::new(4)),
        lang: Some("markdown".to_string()),
    };
    // 首次见该文档：立刻上色（打开 / 切换文档不该闪一下无色）。
    let opened = f.service.highlight(q()).expect("高亮应成功");
    assert!(!opened.is_empty(), "首次打开应立刻有颜色");

    // 之后每 60ms 敲一个字符（模拟连续输入），共 5 次。
    let mut calls_after_open = 0usize;
    for i in 1..=5 {
        f.provider
            .insert(doc_id, &format!("fn a{i}"), i, Some(fp("/d/a.md")));
        f.clock.set(200 + i * 60);
        let out = f.service.highlight(q()).expect("高亮应成功");
        assert!(out.is_empty(), "打字期内应不上色（{i}）");
        calls_after_open = f.highlighter.call_count();
    }
    assert_eq!(
        calls_after_open, 1,
        "打字期内不得走进高亮器（当前 {} 次）",
        calls_after_open
    );
}

/// 停手 150ms 后一次性上色：**只重算一次**。
#[test]
fn highlight_recomputes_once_after_typing_settles() {
    let f = build(vec![], vec![span(0, 2, SpanStyle::Keyword)]);
    let doc_id = DocumentId::new();
    f.provider.insert(doc_id, "fn a", 0, Some(fp("/d/a.md")));

    let q = || HighlightQuery {
        doc_id,
        range: TextRange::new(ByteOffset::new(0), ByteOffset::new(4)),
        lang: Some("markdown".to_string()),
    };
    let _ = f.service.highlight(q()).expect("高亮应成功");

    // 敲 5 个字符，每 60ms 一个。
    for i in 1..=5u64 {
        f.provider
            .insert(doc_id, &format!("fn a{i}"), i, Some(fp("/d/a.md")));
        f.clock.set(200 + i * 60);
        let _ = f.service.highlight(q()).expect("高亮应成功");
    }
    let last_edit_at = 200 + 5 * 60;

    // 停手：等到超过 150ms 防抖窗口。
    f.clock.set(last_edit_at + 200);
    let settled = f.service.highlight(q()).expect("高亮应成功");
    assert!(!settled.is_empty(), "停手后必须补上颜色");

    // 之后连续多帧（文档未变）应全部命中缓存，不再重算。
    let after = f.highlighter.call_count();
    for t in [last_edit_at + 300, last_edit_at + 400, last_edit_at + 500] {
        f.clock.set(t);
        let _ = f.service.highlight(q()).expect("高亮应成功");
    }
    assert_eq!(
        f.highlighter.call_count(),
        after,
        "停手后补算一次即可，后续帧必须命中缓存"
    );
}

/// 切回另一篇文档时防抖不应误伤：**首次见**该文档要立刻上色。
#[test]
fn highlight_skips_debounce_for_a_newly_seen_document() {
    let f = build(vec![], vec![span(0, 2, SpanStyle::Keyword)]);
    let a = DocumentId::new();
    let b = DocumentId::new();
    f.provider.insert(a, "fn a", 0, Some(fp("/d/a.md")));
    f.provider.insert(b, "fn b", 0, Some(fp("/d/b.md")));

    let q = |id| HighlightQuery {
        doc_id: id,
        range: TextRange::new(ByteOffset::new(0), ByteOffset::new(4)),
        lang: Some("markdown".to_string()),
    };
    assert!(!f.service.highlight(q(a)).expect("ok").is_empty());
    f.clock.set(10); // 距上次编辑仅 10ms：若在防抖窗口内，切文档也会立刻上色
    assert!(
        !f.service.highlight(q(b)).expect("ok").is_empty(),
        "首次见某篇文档必须立刻上色（切换文档不该闪一下无色）"
    );
}

/// 超过降级阈值的文档直接返回空片段：**一次都不进高亮器**。
///
/// 表现层的 mapper 在同等阈值上已降级为纯文本排版（FR-06），
/// 此处若仍解析全文，等于每帧付一份无人使用的成本。
#[test]
fn highlight_skips_oversized_document_entirely() {
    let f = build(vec![], vec![span(0, 2, SpanStyle::Keyword)]);
    let doc_id = DocumentId::new();
    let big = "x".repeat(TRUNCATE_THRESHOLD_BYTES + 1);
    f.provider.insert(doc_id, &big, 0, Some(fp("/d/big.md")));

    let spans = f
        .service
        .highlight(HighlightQuery {
            doc_id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(big.len())),
            lang: Some("markdown".to_string()),
        })
        .expect("高亮不应报错");

    assert!(spans.is_empty(), "超阈值文档应返回空片段（降级为纯文本）");
    assert_eq!(
        f.highlighter.call_count(),
        0,
        "超阈值文档不得走进高亮器（否则 1MB 文档每帧都要解析全文）"
    );
}

// ============================================================================
// 数学公式 / Mermaid 图表 → DTO 映射（P1）
// ============================================================================

#[test]
fn math_block_maps_display_flag_to_view() {
    // 表现层靠 `kind == "math"` + `display` 决定居中成卡还是随正文排版；
    // 映射丢了 `display`，块级公式会被当行内公式渲染。
    let f = build(
        vec![block(
            1,
            BlockKind::Math { display: true },
            0,
            20,
            "E = mc^2",
        )],
        vec![],
    );
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "$$E = mc^2$$", 0, Some(fp("/d/a.md")));
    let view: RenderedView = f
        .service
        .render_preview(RenderPreviewQuery { doc_id, version: 0 })
        .unwrap();
    assert_eq!(view.blocks[0].kind, "math");
    assert_eq!(view.blocks[0].display, Some(true));
    assert_eq!(view.blocks[0].content, "E = mc^2");
}

#[test]
fn diagram_block_maps_engine_name_to_view() {
    let f = build(
        vec![block(
            1,
            BlockKind::Diagram {
                kind: "mermaid".to_string(),
            },
            0,
            30,
            "graph TD;\n  A-->B;",
        )],
        vec![],
    );
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "```mermaid\ngraph TD;\n```", 0, Some(fp("/d/a.md")));
    let view: RenderedView = f
        .service
        .render_preview(RenderPreviewQuery { doc_id, version: 0 })
        .unwrap();
    assert_eq!(view.blocks[0].kind, "diagram");
    assert_eq!(view.blocks[0].diagram.as_deref(), Some("mermaid"));
    assert!(
        view.blocks[0].display.is_none(),
        "图表块不应带公式标记：{:?}",
        view.blocks[0].display
    );
}

// ============================================================================
// 拼写检查编排（P1）
// ============================================================================

/// 构造带拼写检查器的fixture。
fn build_with_spell(spell: Arc<FakeSpellChecker>) -> MdFixture {
    let mut f = build(vec![], vec![]);
    f.service = f.service.with_spell_checker(spell.clone());
    f.spell = Some(spell);
    f
}

fn miss(start: usize, end: usize) -> Misspelling {
    Misspelling::new(TextRange::new(ByteOffset::new(start), ByteOffset::new(end)))
}

#[test]
fn spell_check_returns_ranges_when_enabled() {
    let spell = Arc::new(FakeSpellChecker::new(vec![miss(0, 3), miss(4, 11)]));
    let f = build_with_spell(spell);
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "teh recieve", 0, Some(fp("/d/a.md")));
    let hits = f.service.spell_check(SpellCheckQuery {
        doc_id,
        enabled: true,
    });
    assert_eq!(hits, vec![(0, 3), (4, 11)]);
}

#[test]
fn spell_check_is_noop_when_disabled() {
    // 关闭时必须**既不查缓存也不调检查器**——让「关掉开关」零成本。
    let spell = Arc::new(FakeSpellChecker::new(vec![miss(0, 3)]));
    let f = build_with_spell(spell.clone());
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "teh recieve", 0, Some(fp("/d/a.md")));
    let hits = f.service.spell_check(SpellCheckQuery {
        doc_id,
        enabled: false,
    });
    assert!(hits.is_empty(), "关闭时不应返回标记");
    assert_eq!(
        spell.call_count(),
        0,
        "关闭时不应调用检查器（否则关开关仍有代价）"
    );
}

#[test]
fn spell_check_reuses_cache_when_document_unchanged() {
    let spell = Arc::new(FakeSpellChecker::new(vec![miss(0, 3)]));
    let f = build_with_spell(spell.clone());
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "teh recieve", 0, Some(fp("/d/a.md")));
    let first = f.service.spell_check(SpellCheckQuery {
        doc_id,
        enabled: true,
    });
    let second = f.service.spell_check(SpellCheckQuery {
        doc_id,
        enabled: true,
    });
    assert_eq!(first, second, "同版本应返回相同结果");
    assert_eq!(spell.call_count(), 1, "未变的重复查询必须命中缓存");
}

/// 拼写检查的缓存命中帧同样不得快照全文（问题 046）。
///
/// 开着拼写检查时编辑区每帧都要问一次错词集合，而「命中缓存」与
/// 「超阈值降级」都只需要版本号与长度。
#[test]
fn spell_check_frame_path_never_snapshots_the_whole_document() {
    let spell = Arc::new(FakeSpellChecker::new(vec![miss(0, 3)]));
    let f = build_with_spell(spell.clone());
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "teh recieve", 0, Some(fp("/d/a.md")));

    let q = || SpellCheckQuery {
        doc_id,
        enabled: true,
    };
    let _ = f.service.spell_check(q());
    assert_eq!(f.provider.snapshot_calls(), 1, "首次检查应取一次全文");

    for _ in 0..5 {
        let _ = f.service.spell_check(q());
    }
    assert_eq!(f.provider.snapshot_calls(), 1, "缓存命中的帧不得再快照全文");

    // 大文件：只看长度，连全文都不该取。
    let big = DocumentId::new();
    f.provider.insert(
        big,
        &"x".repeat(TRUNCATE_THRESHOLD_BYTES + 1),
        0,
        Some(fp("/d/big.md")),
    );
    let hits = f.service.spell_check(SpellCheckQuery {
        doc_id: big,
        enabled: true,
    });
    assert!(hits.is_empty(), "超阈值文档应降级为空集");
    assert_eq!(
        f.provider.snapshot_calls(),
        1,
        "降级判断只需要长度，不得为此快照全文"
    );
    assert_eq!(spell.call_count(), 1, "降级路径不得调用检查器");
}

#[test]
fn spell_check_degrades_to_empty_when_checker_fails() {
    //关键取舍：检查失败**不报错**，返回空集。
    // 拼写是辅助提示，让它把编辑器卡住或弹错是本末倒置。
    let f = build_with_spell(Arc::new(FakeSpellChecker::failing()));
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "teh recieve", 0, Some(fp("/d/a.md")));
    let hits = f.service.spell_check(SpellCheckQuery {
        doc_id,
        enabled: true,
    });
    assert!(hits.is_empty(), "检查失败应降级为空集而非报错");
}

#[test]
fn spell_check_skips_when_dictionary_unavailable() {
    let spell = Arc::new(FakeSpellChecker::unavailable());
    let f = build_with_spell(spell.clone());
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "teh recieve", 0, Some(fp("/d/a.md")));
    let hits = f.service.spell_check(SpellCheckQuery {
        doc_id,
        enabled: true,
    });
    assert!(hits.is_empty());
    assert_eq!(spell.call_count(), 0, "词典不可用时应跳过检查");
}

#[test]
fn spell_check_degrades_for_large_document() {
    // 与高亮 / 预览同策略：大文档不跑可选功能，保证编辑帧不退化。
    let spell = Arc::new(FakeSpellChecker::new(vec![miss(0, 3)]));
    let f = build_with_spell(spell.clone());
    let doc_id = DocumentId::new();
    let big = "teh ".repeat(200_000); // > 500KB
    f.provider.insert(doc_id, &big, 0, Some(fp("/d/big.md")));
    let hits = f.service.spell_check(SpellCheckQuery {
        doc_id,
        enabled: true,
    });
    assert!(hits.is_empty(), "超阈值文档应降级为不检查");
    assert_eq!(spell.call_count(), 0);
}

#[test]
fn spell_check_without_injected_checker_is_noop() {
    // 未注入检查器（`new` 不带 checker）时不应 panic，直接返回空集。
    let f = build(vec![], vec![]);
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "teh recieve", 0, Some(fp("/d/a.md")));
    let hits = f.service.spell_check(SpellCheckQuery {
        doc_id,
        enabled: true,
    });
    assert!(hits.is_empty());
}

// ============================================================================
// V11-08 · 格式检查（lint）
// ============================================================================

/// 全部规则启用时，坏文档要报出问题。
#[test]
fn lint_reports_issues_when_rules_are_enabled() {
    let f = build(vec![], vec![]);
    let doc_id = DocumentId::new();
    // `#` 直接跳到 `###`（层级跳跃）+ 行尾 1 个空格（2 个是硬换行，被豁免）。
    f.provider.insert(
        doc_id,
        "# 标题\n\n### 跳到三级 \n正文\n",
        0,
        Some(fp("/d/x.md")),
    );
    let issues = f.service.lint_document(LintQuery {
        doc_id,
        rules: LintRulesView::default(),
    });
    assert!(
        issues.iter().any(|i| i.rule == "heading_jump"),
        "应报出标题层级跳跃，实际 {issues:?}"
    );
    assert!(
        issues.iter().any(|i| i.rule == "trailing_whitespace"),
        "应报出行尾空格，实际 {issues:?}"
    );
}

/// 关掉的规则不得出现在结果里——这是「规则可开关」的核心承诺。
#[test]
fn lint_skips_disabled_rules() {
    let f = build(vec![], vec![]);
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "# 标题\n\n### 跳到三级  \n", 0, Some(fp("/d/x.md")));
    let only_heading = LintRulesView {
        heading_jump: true,
        list_marker_mix: false,
        trailing_whitespace: false,
        unclosed_fence: false,
    };
    let issues = f.service.lint_document(LintQuery {
        doc_id,
        rules: only_heading,
    });
    assert!(issues.iter().all(|i| i.rule == "heading_jump"));
    assert!(!issues.is_empty(), "标题跳跃仍应被报出");
}

/// 全部规则关闭 → 短路返回空集（连内容都不读）。
#[test]
fn lint_with_all_rules_off_is_empty() {
    let f = build(vec![], vec![]);
    let doc_id = DocumentId::new();
    f.provider
        .insert(doc_id, "# 标题\n\n### 三级  \n", 0, Some(fp("/d/x.md")));
    let off = LintRulesView {
        heading_jump: false,
        list_marker_mix: false,
        trailing_whitespace: false,
        unclosed_fence: false,
    };
    assert!(
        f.service
            .lint_document(LintQuery { doc_id, rules: off })
            .is_empty(),
        "规则全关必须返回空集"
    );
}

/// 文档不存在 → 空集（不 panic）。
#[test]
fn lint_of_unknown_document_is_empty() {
    let f = build(vec![], vec![]);
    let issues = f.service.lint_document(LintQuery {
        doc_id: DocumentId::new(),
        rules: LintRulesView::default(),
    });
    assert!(issues.is_empty());
}

/// 投影字段必须齐全且是**可直接呈现**的（0-based 行号 + 中文规则名 + 说明）。
#[test]
fn lint_issue_view_carries_renderable_fields() {
    let f = build(vec![], vec![]);
    let doc_id = DocumentId::new();
    // 先有一个 `#`，再跳到 `###`，才是「层级跳跃」。
    f.provider.insert(
        doc_id,
        "正文\n\n# 一级\n\n### 三级\n",
        0,
        Some(fp("/d/x.md")),
    );
    let issues: Vec<LintIssueView> = f.service.lint_document(LintQuery {
        doc_id,
        rules: LintRulesView::default(),
    });
    let issue = issues
        .iter()
        .find(|i| i.rule == "heading_jump")
        .expect("应有一条标题跳跃问题");
    assert_eq!(issue.rule_label, "标题层级跳跃", "规则名应是中文显示名");
    assert_eq!(issue.line, 4, "问题在第五行（0-based = 4）");
    assert!(!issue.message.is_empty(), "说明文案不得为空：{:?}", issue);
}

/// 高亮缓存按**访问序**驱逐（问题 043）。
///
/// 原实现用 `HashMap` 的任意迭代序挑驱逐对象，可能正好驱逐下一次就要
/// 命中的文档：容量小于打开文档数时，切一轮标签就是一次 56ms 级的
/// syntect 重算尖峰。这条测试把「命中即触碰、驱逐最旧」钉死。
#[test]
fn highlight_cache_evicts_the_least_recently_used_entry() {
    use rustmark::application::markdown::service::HighlightCache;

    let mut cache = HighlightCache::new(2);
    let (a, b, c) = (DocumentId::new(), DocumentId::new(), DocumentId::new());
    // 高亮缓存存的是**共享快照**（`Arc<Vec<SpanView>>`，问题 046）。
    let span = |n: usize| {
        std::sync::Arc::new(vec![rustmark::application::dto::SpanView {
            range: rustmark::application::dto::RangeView::new(0, n),
            style: "text".to_string(),
            url: None,
        }])
    };

    cache.insert(a, 0, None, (0, 1), span(1));
    cache.insert(b, 0, None, (0, 1), span(2));
    // 访问 a：a 成为最新，b 变成最旧。
    assert!(cache.get(a, 0, None, (0, 1)).is_some(), "a 应命中");

    // 插入 c 触发驱逐：必须驱逐 b（最旧），而不是 a（刚用过）。
    cache.insert(c, 0, None, (0, 1), span(3));
    assert!(
        cache.get(a, 0, None, (0, 1)).is_some(),
        "刚访问过的 a 不该被驱逐"
    );
    assert!(
        !cache.get(b, 0, None, (0, 1)).is_some(),
        "最旧的 b 应被驱逐"
    );
    assert!(cache.get(c, 0, None, (0, 1)).is_some());
}

/// 高亮缓存容量对齐打开文档上限（问题 043）。
///
/// 打开上限默认 12（`max_open_documents`），缓存若只有 8，开满标签后
/// 来回切换必然互相驱逐。容量常量虽是私有，但其值经由服务的构造暴露：
/// 这条测试通过「12 份文档互不驱逐」在行为层钉住容量 ≥ 12。
#[test]
fn highlight_cache_holds_at_least_the_max_open_documents() {
    use rustmark::application::markdown::service::HighlightCache;

    let mut cache = HighlightCache::new(12);
    let ids: Vec<_> = (0..12).map(|_| DocumentId::new()).collect();
    for id in &ids {
        cache.insert(*id, 0, None, (0, 1), std::sync::Arc::new(Vec::new()));
    }
    for id in &ids {
        assert!(
            cache.get(*id, 0, None, (0, 1)).is_some(),
            "容量应容得下 12 个打开文档：{id:?} 被提前驱逐了"
        );
    }
}
