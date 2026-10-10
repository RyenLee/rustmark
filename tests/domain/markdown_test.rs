//! Markdown 上下文单测（S3-09 / S3-10）。
//!
//! 重点验证 `source_range`（`FR-02` 滚动同步与 `FR-03` 增量高亮的基础）
//! 与 `BlockMapper` 的边界语义。

use rustmark::domain::markdown::aggregate::RenderedDocument;
use rustmark::domain::markdown::services::BlockMapper;
use rustmark::domain::markdown::value_objects::{BlockId, BlockKind, RenderedBlock};
use rustmark::shared::geometry::{ByteOffset, TextRange};

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(ByteOffset::new(start), ByteOffset::new(end))
}

fn block(id: u64, kind: BlockKind, span: TextRange, content: &str) -> RenderedBlock {
    RenderedBlock::new(BlockId::from_u64(id), kind, span, content)
}

fn sample_blocks() -> Vec<RenderedBlock> {
    vec![
        block(0, BlockKind::Heading { level: 1 }, range(0, 5), "标题"),
        block(1, BlockKind::Paragraph, range(6, 11), "正文"),
    ]
}

// ---------- 值对象 ----------

#[test]
fn every_block_carries_source_range() {
    for item in sample_blocks() {
        assert!(
            item.source_range().start().value() < item.source_range().end().value(),
            "块必须携带非空源码区间：{item:?}"
        );
    }
}

#[test]
fn block_kind_helpers() {
    let heading = BlockKind::Heading { level: 2 };
    assert!(heading.is_heading());
    assert_eq!(heading.heading_level(), Some(2));
    assert!(!BlockKind::Paragraph.is_heading());
    assert!(
        BlockKind::CodeBlock {
            lang: Some("rust".into())
        }
        .is_code()
    );
}

// ---------- BlockMapper 边界语义 ----------

#[test]
fn block_at_offset_inside_block() {
    let blocks = sample_blocks();
    assert_eq!(
        BlockMapper::block_at_offset(&blocks, ByteOffset::new(2)),
        Some(BlockId::from_u64(0))
    );
    assert_eq!(
        BlockMapper::block_at_offset(&blocks, ByteOffset::new(8)),
        Some(BlockId::from_u64(1))
    );
}

#[test]
fn block_at_offset_before_first_returns_first() {
    let blocks = vec![block(0, BlockKind::Paragraph, range(5, 10), "x")];
    assert_eq!(
        BlockMapper::block_at_offset(&blocks, ByteOffset::new(0)),
        Some(BlockId::from_u64(0)),
        "早于首块应归入首块（比例 0.0）"
    );
}

#[test]
fn block_at_offset_in_gap_returns_previous() {
    let blocks = sample_blocks();
    assert_eq!(
        BlockMapper::block_at_offset(&blocks, ByteOffset::new(5)),
        Some(BlockId::from_u64(0)),
        "块间空隙应归入前一个块"
    );
}

#[test]
fn block_at_offset_on_empty_slice_returns_none() {
    let blocks: Vec<RenderedBlock> = Vec::new();
    assert_eq!(
        BlockMapper::block_at_offset(&blocks, ByteOffset::new(0)),
        None
    );
}

#[test]
fn offset_to_block_returns_ratio_inside_block() {
    let blocks = sample_blocks();
    let (id, ratio) =
        BlockMapper::offset_to_block(&blocks, ByteOffset::new(8)).expect("应命中第二个块");
    assert_eq!(id, BlockId::from_u64(1));
    assert!(
        (ratio - 0.4).abs() < 1e-6,
        "8 在 [6,11) 中的比例应为 0.4，实际 {ratio}"
    );
}

#[test]
fn offset_to_block_at_block_end_returns_one() {
    let blocks = sample_blocks();
    let (_, ratio) =
        BlockMapper::offset_to_block(&blocks, ByteOffset::new(5)).expect("应命中第一个块");
    assert_eq!(ratio, 1.0, "块尾应返回 1.0");
}

#[test]
fn offset_to_block_beyond_last_returns_one() {
    let blocks = sample_blocks();
    let (id, ratio) = BlockMapper::offset_to_block(&blocks, ByteOffset::new(999)).unwrap();
    assert_eq!(id, BlockId::from_u64(1));
    assert_eq!(ratio, 1.0);
}

#[test]
fn offset_to_block_on_zero_length_block_returns_zero() {
    let blocks = vec![block(0, BlockKind::Rule, range(3, 3), "")];
    let (_, ratio) = BlockMapper::offset_to_block(&blocks, ByteOffset::new(3)).unwrap();
    assert_eq!(ratio, 0.0, "零长度块不得除零");
}

#[test]
fn blocks_in_range_returns_intersecting_blocks() {
    let blocks = sample_blocks();
    let hits = BlockMapper::blocks_in_range(&blocks, range(4, 7));
    assert_eq!(hits.len(), 2, "区间跨越两块时应全部命中");
}

#[test]
fn build_outline_collects_headings_only() {
    let text = "# A\n\ntext\n\n## B\n";
    let blocks = vec![
        block(0, BlockKind::Heading { level: 1 }, range(0, 3), "A"),
        block(1, BlockKind::Paragraph, range(5, 9), "text"),
        block(2, BlockKind::Heading { level: 2 }, range(11, 15), "B"),
    ];
    let outline = BlockMapper::build_outline(&blocks, text);
    assert_eq!(outline.len(), 2);
    assert_eq!(outline[0].level(), 1);
    assert_eq!(outline[0].text(), "A");
    assert_eq!(outline[0].line(), 0);
    assert_eq!(outline[1].level(), 2);
    assert_eq!(outline[1].line(), 4);
}

// ---------- RenderedDocument ----------

#[test]
fn rendered_document_tracks_revision_and_outline() {
    let text = "# A\n\ntext";
    let blocks = vec![
        block(0, BlockKind::Heading { level: 1 }, range(0, 3), "A"),
        block(1, BlockKind::Paragraph, range(5, 9), "text"),
    ];
    let document = RenderedDocument::new(7, text, blocks);
    assert_eq!(document.source_revision(), 7);
    assert_eq!(document.len(), 2);
    assert_eq!(document.outline().len(), 1);
    assert!(!document.is_stale_for(7));
    assert!(document.is_stale_for(8));
}

#[test]
fn rendered_document_block_lookup() {
    let document = RenderedDocument::new(1, "x", sample_blocks());
    let found = document.block(BlockId::from_u64(1)).expect("应能找到块");
    assert_eq!(found.content(), "正文");
    assert!(document.block(BlockId::from_u64(99)).is_none());
}
