//! 大纲值对象 [`OutlineItem`]：标题块在大纲中的投影。

use super::block::BlockId;

/// 大纲条目（对应一个标题块）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OutlineItem {
    /// 标题级别（1–6）。
    level: u8,
    /// 标题文本（已去除 `#` 标记）。
    text: String,
    /// 该标题所在的源码行号（0-based）。
    line: usize,
    /// 对应的渲染块。
    block_id: BlockId,
}

impl OutlineItem {
    /// 构造大纲条目。
    pub fn new(level: u8, text: impl Into<String>, line: usize, block_id: BlockId) -> Self {
        Self {
            level,
            text: text.into(),
            line,
            block_id,
        }
    }

    /// 标题级别。
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// 标题文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 源码行号（0-based）。
    pub const fn line(&self) -> usize {
        self.line
    }
}
