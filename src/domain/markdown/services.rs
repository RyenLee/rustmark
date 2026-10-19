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

/// 增量高亮会话（`FR-03` / OPT-011 复议）。
///
/// 由 [`SyntaxHighlighter::start_session`] 创建，生命周期与「一篇文档的
/// 高亮结果」对齐：同一份文本连续喂入时，实现可比对上次文本、只重解
/// 变更区间，而不是每次 O(全文)。
///
/// # 为什么接口只有一个 `advance(text, lang)`
///
/// **变更区间由实现自己算**。它本就持有上次文本，一次公共前后缀比对即可
/// 得到变更区间；若让调用方传，调用方就得再存一份上次全文（内存翻倍），
/// 而且一旦传错区间，高亮结果会与文本静默不符——这种错误既难在类型上
/// 防范，也难在测试里发现。
///
/// # 实现约束
///
/// - 会话不是线程安全的，调用方须独占持有（本项目由服务层用互斥锁保护）；
/// - `advance` 返回的片段区间相对 `text` 起点，语义与
///   [`SyntaxHighlighter::highlight`] 一致（**不裁剪**）。
pub trait HighlightSession: Send {
    /// 喂入新全文并返回高亮片段。
    ///
    /// - 文本与上次相同时应直接返回上次结果（O(1)）；
    /// - `lang` 变化时应重建（旧状态的语法上下文不再有效）；
    /// - 无法安全复用时必须退化为全量——**宁可慢，不可错**。
    fn advance(
        &mut self,
        text: &str,
        lang: Option<&str>,
    ) -> Result<Vec<SyntaxSpan>, HighlightError>;
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

    /// 开启一个增量高亮会话；`None` 表示该实现不支持增量。
    ///
    /// # 为什么给默认实现
    ///
    /// 这是一个**可选能力**：不支持增量的实现照常走 [`Self::highlight`] /
    /// [`Self::highlight_range`]，行为完全等价（只是慢）。给默认 `None` 而不是
    /// 必填方法，让既有实现（尤其是测试替身）无需改动即可继续工作——
    /// 否则每加一个可选能力都要改一圈 fake。
    fn start_session(&self) -> Option<Box<dyn HighlightSession>> {
        None
    }
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

    /// 给一个已判错的词返回「你可能想输入」的候选（按**贴近程度**降序）。
    ///
    /// # 为什么与 [`Self::check`] 分开
    ///
    /// `check` 每帧都可能被调用（输一个字符就重查一遍），它的返回类型刻意保持
    /// `Copy + Eq`（[`Misspelling`] 只带区间），好让表现层每帧比对上一帧的标记集。
    /// 而建议**只在用户右键某个错词时**才需要，携带 `String` 无所谓。
    /// 把两者绑在一起会让「每帧比对」背上堆分配的代价。
    ///
    /// # 边界语义
    ///
    /// - `word` 应为**小写纯字母**词；大小写与非字母由调用方归一化。
    /// - 找不到候选时返回**空集**（不是错误）。宁可不给建议，也不给错的——
    ///   一条离谱的建议比没有建议更伤信任。
    /// - `limit` 为 0 时返回空集；调用方用它控制菜单长度。
    /// - 默认实现返回空集：允许「只判错、不提建议」的最小实现存在。
    fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        let _ = (word, limit);
        Vec::new()
    }
}

/// 编译期断言：跨层 trait 必须 `Send + Sync`（`.agent/conventions.md` §三-9）。
fn _assert_spell_checker_send_sync<T: SpellChecker + Send + Sync>() {}
