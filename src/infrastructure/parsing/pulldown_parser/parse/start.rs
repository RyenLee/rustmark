//! `Event::Start` 分支（含 Image Start 特判）的处理逻辑。

use super::*;
use pulldown_cmark::{Event, Tag, TagEnd};

/// 处理 `Event::Start(tag)`：开栈、设置列表上下文、推入帧。
///
/// 内部逻辑与原 parse() 中 `Event::Start` match 分支完全一致，
/// 仅做了「局部变量 → `state.xxx` 字段访问」的搬运。
pub(super) fn handle_start(state: &mut ParseState, tag: Tag, range: std::ops::Range<usize>) {
    // 图片是**行内**标签，但需要独立成块以便 S5-11 按 URL 加载资源，
    // 故必须先于下方通用的 `Event::Start(tag)` 分支匹配——
    // 后者会吃掉所有 Start 标签，导致此分支永远不可达。
    // pulldown 的 `Tag::Image` 只给出 `title`（`url "标题"` 里的标题），
    // **没有 alt**——alt 是写在 `![这里](url)` 方括号里的文本，
    // 以普通 Text 事件夹在 Start(Image) 与 End(Image) 之间。
    // 故记录 Start 时父块 content 的长度，到 End 时取增量即为 alt。
    if let Tag::Image {
        dest_url, title, ..
    } = tag
    {
        if let Some(top) = state.stack.last_mut() {
            top.image_mark = Some(top.content.len());
            top.image = Some((title.to_string(), dest_url.to_string()));
        }
        return;
    }

    // `Tag::List(Some(n))` 表示有序列表（首项编号为 `n`）。
    if let Tag::List(first) = &tag {
        state.list_ordered = first.is_some();
        state.list_first_ordinal = first.unwrap_or(1);
    }
    let is_item = matches!(&tag, Tag::Item);
    let is_cell = matches!(&tag, Tag::TableCell);
    // 定义列表的术语 / 定义：都不产出块，但要各自收集文本。
    let is_dl_title = matches!(&tag, Tag::DefinitionListTitle);
    let is_dl_def = matches!(&tag, Tag::DefinitionListDefinition);
    // 脚注定义的正文包在段落里，必须回流到本帧（否则会变成
    // 一个游离在文档里的普通段落）。
    let is_footnote = matches!(&tag, Tag::FootnoteDefinition(_));
    // 列表项与单元格要各自收集文本（见 `OpenBlock` 的类型注释），
    // 故不能标记为 `is_inline`——否则文本会被并到外层块去。
    // 定义列表的术语 / 定义同理。
    let kind = block_kind_of(&tag, state.list_ordered);
    let is_inline = kind.is_none() && !is_item && !is_cell && !is_dl_title && !is_dl_def;
    // 行内样式标签（粗体 / 链接 / 上下标…）把样式挂在帧上，
    // 其内文本按该样式记录（见 `current_inline_style`）。
    let style = inline_style_of(&tag);
    let aligns = match &tag {
        Tag::Table(a) => a.iter().map(cell_alignment_of).collect(),
        _ => Vec::new(),
    };
    let opens_head = matches!(&tag, Tag::TableHead);
    let mut frame = OpenBlock::new(kind, range.start);
    frame.is_item = is_item;
    frame.is_cell = is_cell;
    frame.is_inline = is_inline;
    // 会吞掉内层段落块的四类帧（见 `OpenBlock::absorbs`）。
    let is_quote = matches!(&tag, Tag::BlockQuote(_));
    frame.absorbs = is_item || is_dl_def || is_footnote || is_quote;
    frame.is_dl_title = is_dl_title;
    frame.is_dl_def = is_dl_def;
    frame.aligns = aligns;
    frame.style = style;
    if matches!(&tag, Tag::List(_)) {
        // 深度取「已有的列表帧个数」——本帧尚未入栈，故顶层为 0。
        frame.list_depth = list_depth_of(&state.stack);
        frame.list_next_ordinal = state.list_first_ordinal;
    }
    state.stack.push(frame);
    // 进入表头：把所属表格切到「表头模式」，后续整行归为表头。
    if opens_head {
        let len = state.stack.len();
        if len >= 2 {
            state.stack[len - 2].in_head = true;
        }
    }
}
