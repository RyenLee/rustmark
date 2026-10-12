//! 行内事件（Code / SoftBreak / 行内公式 / 脚注引用 / TaskListMarker / Rule）的处理逻辑。

use super::*;
use pulldown_cmark::Event;

/// 处理非 Start / 非 End 的事件：行内样式、公式、换行、脚注引用、任务标记、分割线。
///
/// 内部逻辑与原 parse() 中对应 match 分支完全一致，
/// 仅做了「局部变量 → `state.xxx` 字段访问」的搬运。
pub(super) fn handle_inline(
    state: &mut ParseState,
    event: Event,
    range: std::ops::Range<usize>,
    text: &str,
) {
    // ---- 行内事件：文本并入**最近的产出块**的栈帧 ----
    //
    // 注意不能简单地写入 `stack.last_mut()`：`这是 **粗体**` 这样的文本
    // 在事件流里是 `Start(Paragraph) Text Start(Strong) Text End(Strong) Text`，
    // 处理 `Text` 时栈顶是**行内**的 `Strong` 帧。若写进去，
    // 文本会随该帧一起被丢弃——表现为 `content` 出现空洞
    // （曾实测 `这是 **粗体** 与 ` + "`代码`" + ` 文本` 变成 `这是  与 代码 文本`）。
    //
    // 文本类事件在本轮循环开头已进缓冲并 `continue`，走不到这个分支；
    // 保留臂只是为了覆盖枚举，并指明它们的去处。
    match event {
        Event::Text(_) | Event::Html(_) | Event::InlineHtml(_) => {}
        Event::Code(t) => {
            // 行内代码：文本保留（搜索 / 大纲看得到），样式记为 Code。
            push_content(&mut state.stack, &t, Some(InlineStyle::Code));
        }
        Event::SoftBreak | Event::HardBreak => {
            // 换行不带样式：样式片段不跨行，预览按行切分渲染才不会错位。
            push_content(&mut state.stack, "\n", None);
        }
        Event::InlineMath(t) => {
            // 行内公式是**段落内容的一部分**，不独立成块：`$...$` 前后
            // 通常还有正文，独立成块会把一句话拆成三块、破坏段落语义。
            // 补回 `$` 分隔符，让 `content` 仍是可搜索的原文。
            push_content(&mut state.stack, &format!("${t}$"), Some(InlineStyle::Math));
        }
        Event::DisplayMath(t) => {
            // 块级公式同理并入当前块内容；`$$` 换行写法在 pulldown 里
            // 会带上首尾换行，trim 后再补分隔符避免出现空公式。
            push_content(
                &mut state.stack,
                &format!("$${}\n$$", t.trim()),
                Some(InlineStyle::Math),
            );
        }
        Event::FootnoteReference(label) => {
            // 内容保留 `[^1]` 原文（搜索 / 字数看到的是用户写的字符），
            // 但**挂上脚注引用样式**：渲染层据此把它换成上标标号，
            // 否则正文里以原始 Markdown 形态显示 `[^1]`——
            // 看起来就像「脚注没渲染」（实测）。
            push_content(
                &mut state.stack,
                &format!("[^{label}]"),
                Some(InlineStyle::FootnoteRef),
            );
        }
        // 任务列表标记：落到**列表项**帧（列表项已能收集自己的文本），
        // 并记录该 `Range` 的勾选状态以便 S6-05 渲染复选框、回写源码。
        // 标记文本仍并进内容（`- [x] ` 的勾选状态是 Outliner 与搜索需要的信息）。
        Event::TaskListMarker(checked) => {
            push_content(
                &mut state.stack,
                if checked { "[x] " } else { "[ ] " },
                None,
            );
            if let Some(item) = state.stack.iter_mut().rev().find(|f| f.is_item) {
                item.item_checked = Some(checked);
            }
        }

        // 分割线：pulldown 把它作为独立事件（无配对标签），
        // 故当场产出一个 `Rule` 块，区间取事件自带偏移
        //（同样裁掉尾随换行，避免与相邻块区间重叠）。
        Event::Rule => {
            let mut stop = range.end;
            while stop > range.start
                && matches!(text.as_bytes().get(stop - 1), Some(b'\n') | Some(b'\r'))
            {
                stop -= 1;
            }
            state.blocks.push(RenderedBlock::new(
                BlockId::from_u64(state.next_id),
                BlockKind::Rule,
                TextRange::new(ByteOffset::new(range.start), ByteOffset::new(stop)),
                String::new(),
            ));
            state.next_id += 1;
        }
        _ => {}
    }
}
