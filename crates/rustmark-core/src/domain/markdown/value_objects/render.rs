//! 渲染结果值对象：[`RenderedBlock`]（含 `source_range`）与 [`Misspelling`]。
//!
//! `source_range` 是双向滚动同步（`FR-02`）与增量高亮（`FR-03`）的基础；
//! 省略它就只能靠重新解析全文定位，无法满足解析 ≤ 5ms 的性能目标。

use crate::shared::geometry::TextRange;

use super::{
    block::{BlockId, BlockKind, InlineImage},
    inline::InlineSpan,
};

/// 一个渲染完成的块。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RenderedBlock {
    id: BlockId,
    kind: BlockKind,
    /// 该块在**源码**中的字节区间（左闭右开）。
    source_range: TextRange,
    /// 该块的纯文本内容（不含 Markdown 标记），供大纲与搜索使用。
    content: String,
    /// 该块内的行内样式片段（相对 `content` 的区间；无样式时为空）。
    ///
    /// 与 `content` **并存**而非替代：`content` 始终是不带标记的纯文本
    /// （搜索 / 大纲 / 滚动同步都按它取字数与偏移），`spans` 只是叠加在
    /// 其上的「哪一段是什么样式」。渲染方按区间切片即可，
    /// 不必再解析一遍 Markdown。
    spans: Vec<InlineSpan>,
    /// 混排在文字里的图片（区间相对 `content`；见 [`InlineImage`]）。
    ///
    /// 只在「块里除图片外还有别的文字」时才有值——整块就一张图时块本身
    /// 已升级为 [`BlockKind::Image`]，不需要再挂行内图片。
    images: Vec<InlineImage>,
}

impl RenderedBlock {
    /// 构造渲染块。
    pub fn new(
        id: BlockId,
        kind: BlockKind,
        source_range: TextRange,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id,
            kind,
            source_range,
            content: content.into(),
            spans: Vec::new(),
            images: Vec::new(),
        }
    }

    /// 附带行内样式片段（构造链）。
    pub fn with_spans(mut self, spans: Vec<InlineSpan>) -> Self {
        self.spans = spans;
        self
    }

    /// 附带混排在文字里的图片（构造链）。
    pub fn with_images(mut self, images: Vec<InlineImage>) -> Self {
        self.images = images;
        self
    }

    /// 行内样式片段。
    pub fn spans(&self) -> &[InlineSpan] {
        &self.spans
    }

    /// 混排在文字里的图片。
    pub fn images(&self) -> &[InlineImage] {
        &self.images
    }

    /// 块 ID。
    pub const fn id(&self) -> BlockId {
        self.id
    }

    /// 块类型。
    pub const fn kind(&self) -> &BlockKind {
        &self.kind
    }

    /// 源码区间（`FR-02` / `FR-03` 的基础）。
    pub const fn source_range(&self) -> TextRange {
        self.source_range
    }

    /// 块内容。
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// 一个被判定为拼写错误的词及其位置。
///
/// # 为什么只存位置与原词，不存建议
///
/// 编辑器内的波浪线只需知道「哪里错了」。**不存建议**是为了让本值对象保持
/// `Copy + Eq`，从而能进 `Vec` 直接比较——渲染层每帧都要比对上一帧的标记集，
/// 若携带 `String` 建议会拖累该比对。
///
/// 建议（「你可能想输入」）由 [`crate::domain::markdown::services::SpellChecker::suggest`]
/// 按需现算（V11-07）：它只在用户右键某个错词时才被调用，
/// 不必跟着每帧的标记集一起走。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Misspelling {
    /// 拼错的词在原文中的**字节区间**（与 [`RenderedBlock::source_range`] 同坐标系）。
    pub range: TextRange,
}

impl Misspelling {
    /// 构造拼错标记。
    pub const fn new(range: TextRange) -> Self {
        Self { range }
    }
}
