//! Markdown 上下文的聚合根 `RenderedDocument`（需求文档 §6.1）。
//!
//! 这是**值对象聚合**：由 `Document` 的文本派生而来、不独立持久化、不产生
//! 领域事件；其生命周期由应用层的预览缓存管理，靠 `source_revision` 判断过期。

use super::services::BlockMapper;
use super::value_objects::{BlockId, OutlineItem, RenderedBlock};

/// 一次完整解析的结果。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RenderedDocument {
    /// 该结果对应的源文档版本号（`Document::revision()`）。
    source_revision: u64,
    /// 渲染块序列（按 `source_range` 升序）。
    blocks: Vec<RenderedBlock>,
    /// 大纲（由标题块派生）。
    outline: Vec<OutlineItem>,
    /// 解析所依据的源码文本（用于行号与滚动映射换算）。
    source: String,
}

impl RenderedDocument {
    /// 构造解析结果并**同时**生成大纲。
    pub fn new(
        source_revision: u64,
        source: impl Into<String>,
        blocks: Vec<RenderedBlock>,
    ) -> Self {
        let source = source.into();
        let outline = BlockMapper::build_outline(&blocks, &source);
        Self {
            source_revision,
            blocks,
            outline,
            source,
        }
    }

    /// 源文档版本号。
    pub const fn source_revision(&self) -> u64 {
        self.source_revision
    }

    /// 大纲。
    pub fn outline(&self) -> &[OutlineItem] {
        &self.outline
    }

    /// 是否已过期（源文档版本号已推进）。
    ///
    /// 应用层据此决定是否需要重新解析（`FR-01`：防抖 150ms 后触发）。
    pub const fn is_stale_for(&self, revision: u64) -> bool {
        self.source_revision != revision
    }

    /// 按 ID 查找块。
    pub fn block(&self, id: BlockId) -> Option<&RenderedBlock> {
        self.blocks.iter().find(|block| block.id() == id)
    }

    /// 块数量。
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// 是否没有任何块（[`Self::len`] 的配套判空，clippy 约定）。
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}
