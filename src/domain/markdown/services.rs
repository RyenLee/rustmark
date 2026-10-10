//! Markdown 上下文的领域 trait 与映射服务（S3-10）。
//!
//! 三个 trait 的签名在**本阶段冻结**，阶段 05 据此实现基础设施适配器
//! （`pulldown_parser` / `syntect_highlighter`），阶段 04 可并行编排用例。
//!
//! 全部 `Send + Sync`：预览解析会放到后台线程以避免阻塞 UI 帧（`NFR-PERF` 编辑响应 ≤ 16ms）。

use crate::shared::geometry::{ByteOffset, TextRange};

use super::errors::{HighlightError, ParseError, SpellCheckError};
use super::value_objects::{BlockId, Misspelling, OutlineItem, RenderedBlock, SyntaxSpan};

/// Markdown 解析器。
///
/// 实现由基础设施层提供（S5-04）。返回的块**必须按 `source_range` 升序排列**，
/// 否则 [`BlockMapper`] 的二分查找不成立。
pub trait MarkdownParser: Send + Sync {
    /// 解析全文，返回渲染块序列。
    ///
    /// 每个块的 `source_range` 必须覆盖其在源码中的真实区间——
    /// 这是 `FR-02`（滚动同步）与 `FR-03`（增量高亮）的前提。
    fn parse(&self, text: &str) -> Result<Vec<RenderedBlock>, ParseError>;
}

/// 语法高亮器（用于**编辑器内的代码块**与预览）。
pub trait SyntaxHighlighter: Send + Sync {
    /// 高亮整段文本；`lang` 为 `None` 时按纯文本处理（不报错）。
    fn highlight(&self, text: &str, lang: Option<&str>) -> Result<Vec<SyntaxSpan>, HighlightError>;

    /// 只高亮 `range` 区间（`FR-03` 增量高亮）。
    ///
    /// 边界语义：`range` 越界时实现应**裁剪**而非报错；返回的片段区间相对于
    /// `text` 的起点（与 [`SyntaxHighlighter::highlight`] 一致），便于调用方统一换算。
    fn highlight_range(
        &self,
        text: &str,
        range: TextRange,
        lang: Option<&str>,
    ) -> Result<Vec<SyntaxSpan>, HighlightError>;

    /// 该语言是否受支持；不支持时调用方应降级为纯文本而不是报错。
    fn supports(&self, lang: &str) -> bool;
}

/// 块 ↔ 偏移映射（**无状态纯函数**，供 `FR-02` 滚动同步使用）。
///
/// 全部函数要求 `blocks` 已按 `source_range.start` 升序排列（由解析器保证）。
pub struct BlockMapper;

impl BlockMapper {
    /// `offset` 落在哪个块内。
    ///
    /// 边界语义：
    /// - `offset` 恰为某块的终点 → 归入**该块**（块为左闭右开，但终点视为仍在块内，
    ///   否则在块尾按方向键会出现「不属于任何块」的空档）；
    /// - `offset` 落在两块之间的空隙 → 归入**前一个块**；
    /// - `offset` 小于首块起点 → 归入**首块**；
    /// - `blocks` 为空 → `None`。
    pub fn block_at_offset(blocks: &[RenderedBlock], offset: ByteOffset) -> Option<BlockId> {
        Self::offset_to_block(blocks, offset).map(|(id, _)| id)
    }

    /// `offset` 落在哪个块内，以及**在该块中的比例**（0.0–1.0）。
    ///
    /// 返回值用于把编辑区的滚动位置线性映射到预览区：
    /// `预览 y = 块顶 y + ratio × 块高`。
    ///
    /// 边界语义：
    /// - 命中块内：`ratio = (offset - start) / (end - start)`，空块（长度为 0）返回 `0.0`；
    /// - 命中块尾或之后的空隙 / 超出末块：返回该块的 `1.0`；
    /// - 早于首块：返回首块的 `0.0`；
    /// - `blocks` 为空：`None`。
    pub fn offset_to_block(blocks: &[RenderedBlock], offset: ByteOffset) -> Option<(BlockId, f32)> {
        let first = blocks.first()?;
        let target = offset.value();

        if target < first.source_range().start().value() {
            return Some((first.id(), 0.0));
        }

        let mut candidate = first;
        for block in blocks.iter() {
            if block.source_range().start().value() <= target {
                candidate = block;
            } else {
                break;
            }
        }

        let range = candidate.source_range();
        // 零长度块（如空段落）没有「块内比例」可言，返回 0.0 以滚动到块顶；
        // 该分支必须先于「命中块尾」判断，否则 `start == end` 会被误判成块尾（1.0）。
        let ratio = if range.is_empty() {
            0.0
        } else if target >= range.end().value() {
            1.0
        } else {
            let span = range.len();
            ((target - range.start().value()) as f32 / span as f32).clamp(0.0, 1.0)
        };
        Some((candidate.id(), ratio))
    }

    /// 与 `range` 有交集的全部块 ID（用于「选区变化后重渲染哪些块」）。
    pub fn blocks_in_range(blocks: &[RenderedBlock], range: TextRange) -> Vec<BlockId> {
        blocks
            .iter()
            .filter(|block| block.source_range().intersects(range))
            .map(|block| block.id())
            .collect()
    }

    /// 由标题块生成大纲。
    ///
    /// `text` 用于计算行号（0-based）；非标题块被忽略。
    pub fn build_outline(blocks: &[RenderedBlock], text: &str) -> Vec<OutlineItem> {
        blocks
            .iter()
            .filter_map(|block| {
                let level = block.kind().heading_level()?;
                let start = block.source_range().start().value().min(text.len());
                let line = text.as_bytes()[..start]
                    .iter()
                    .filter(|b| **b == b'\n')
                    .count();
                Some(OutlineItem::new(
                    level,
                    block.content().to_string(),
                    line,
                    block.id(),
                ))
            })
            .collect()
    }
}

/// 拼写检查器（编辑器内英文错词波浪线，路线①零依赖实现）。
///
/// # 边界
///
/// - **只检查英文**：中文按字切分、无空格，本就不属「拼写」范畴，直接跳过；
/// - **不报错给用户**：词典缺失等异常由实现降级为「无错词」，故返回类型里
///   没有错误分支的必要性——实现仍可返回 [`SpellCheckError`]，但调用方
///   应当把它当作「跳过检查」而非「向用户报错」；
/// - **实现须 `Send + Sync`**：与解析 / 高亮同理由，检查会放到后台线程。
pub trait SpellChecker: Send + Sync {
    /// 检查文本，返回拼错词的区间（按起点升序，且互不重叠）。
    fn check(&self, text: &str) -> Result<Vec<Misspelling>, SpellCheckError>;

    /// 该词典是否可用；不可用时调用方应跳过检查而非报错。
    fn is_available(&self) -> bool;
}
