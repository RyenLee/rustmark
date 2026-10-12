//! `MarkdownParser` 实现：事件流 → 带源码区间的块序列主循环。
//!
//! 原单文件 parse.rs 超过 500 行，按「三个 match 大分支」拆为：
//! - `start.rs`：`Event::Start`（含 Image Start 特判）；
//! - `end.rs`：`Event::End`（含 Image End 特判 + 通用 TagEnd 巨块）；
//! - `inline.rs`：行内事件 + `Event::Rule`。
//!
//! 可变状态打包在 [`ParseState`] 里，三个 free fn 各拿 `&mut ParseState`。

mod end;
mod inline;
mod start;

use pulldown_cmark::Event;

use crate::domain::markdown::errors::ParseError;
use crate::domain::markdown::services::MarkdownParser;
use crate::domain::markdown::value_objects::{
    BlockId, BlockKind, DefinitionBody, DefinitionEntry, InlineSpan, InlineStyle, ListItem,
    RenderedBlock, TableData,
};
use crate::shared::geometry::{ByteOffset, TextRange};

use super::PulldownMarkdownParser;
use super::event_map::{
    absorbing_index, block_kind_of, cell_alignment_of, definition_list_plain_text, inline_style_of,
    list_depth_of, list_plain_text, root_list_index, strip_task_marker, toc_title_of_source,
    trim_spans,
};
use super::image_size::desugar_image_sizes;
use super::stack::{
    OpenBlock, flush_inline_text, math_body_if_standalone, nearest_producer, push_content,
};

/// 解析主循环内的全部可变状态打包。
///
/// 拆分为三个 free fn 后，原 parse() 里散落的 6 个局部变量统一收在这里，
/// 子文件通过 `state.xxx` 访问——避免层层传参。
pub(super) struct ParseState {
    /// 最终产出的块序列（领域层契约要求按 `source_range.start` 升序）。
    pub(super) blocks: Vec<RenderedBlock>,
    /// 尚未闭合的块级开栈（`OpenBlock` 帧）。
    pub(super) stack: Vec<OpenBlock>,
    /// 下一个待分配的 `BlockId`。
    pub(super) next_id: u64,
    /// 当前所处的列表是否有序。
    pub(super) list_ordered: bool,
    /// 当前 `Tag::List` 给出的首项编号（无序列表无意义）。
    pub(super) list_first_ordinal: u64,
    /// 连续的文本类事件缓冲（见 `flush_inline_text`）。
    pub(super) pending: String,
}

impl MarkdownParser for PulldownMarkdownParser {
    fn parse(&self, text: &str) -> Result<Vec<RenderedBlock>, ParseError> {
        let mut state = ParseState {
            blocks: Vec::new(),
            stack: Vec::new(),
            next_id: 0,
            // 当前所处的列表是否有序——`Tag::List` 开启时写入，Item 继承之。
            list_ordered: false,
            // 当前 `Tag::List` 给出的首项编号（有序列表 `1.` 的 1；无序列表无意义）。
            list_first_ordinal: 1,
            // 连续的文本类事件先攒在这里（见 `flush_inline_text`）。
            pending: String::new(),
        };

        // 预处理：删掉 CSDN 的图片尺寸后缀（`![alt](url =30x30)`），
        // 否则 pulldown 认不出图片、整段按普通文本吐出。
        // 删除改变了偏移，故随后用 `offsets` 把事件偏移还原回**原文**坐标——
        // 块的 `source_range` 必须指向原文，否则点击跳转 / 滚动同步会错位。
        let (src, offsets) = desugar_image_sizes(text);

        for (event, range) in PulldownMarkdownParser::raw_parser(&src) {
            let range = offsets.to_original(range.start)..offsets.to_original(range.end);
            // 文本类事件只入缓冲、不立即处理：pulldown 可能把 `2^10^`
            // 拆成两个事件，逐事件扫描会漏掉跨事件配对的上下标定界符。
            // 其余事件一律先冲刷缓冲，保证文本与结构事件的先后顺序不变
            // （例如 `Start(Image)` 要用 content 长度记 alt 起点）。
            match &event {
                Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                    state.pending.push_str(t);
                    continue;
                }
                _ => flush_inline_text(&mut state.stack, &mut state.pending),
            }

            match event {
                Event::Start(tag) => start::handle_start(&mut state, tag, range),
                Event::End(tag_end) => end::handle_end(&mut state, tag_end, range, text),
                other => inline::handle_inline(&mut state, other, range, text),
            }
        }

        // 收尾冲刷：正常情况下栈已空（文本早已随各块的 End 落盘），
        // 这里只兜住「文档以未闭合文本结尾」的异常输入。
        flush_inline_text(&mut state.stack, &mut state.pending);

        // 防御：若文档以未闭合块结尾（理论上不会），补齐为块而非丢弃内容。
        for open in state.stack.into_iter().rev() {
            // 只补齐「本应产出块」的标签；行内标签与列表项不补。
            if let Some(kind) = open.kind.clone() {
                let end = open.start;
                state.blocks.push(
                    RenderedBlock::new(
                        BlockId::from_u64(state.next_id),
                        kind,
                        open.range(end, text),
                        open.content,
                    )
                    .with_spans(open.spans),
                );
                state.next_id += 1;
            }
        }

        // 领域层契约：块必须按 `source_range.start` 升序排列，否则
        // `BlockMapper::offset_to_block` 的查找会失效（`FR-02` 依赖此前提）。
        state
            .blocks
            .sort_by_key(|block| (block.source_range().start(), block.id()));

        Ok(state.blocks)
    }
}
