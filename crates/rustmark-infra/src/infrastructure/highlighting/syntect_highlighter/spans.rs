//! `syntect_highlighter` 的 scope→样式 判定与片段合并（从主文件抽出；R-07 Batch 1）。

use super::data::style_selectors;
use rustmark_core::domain::markdown::value_objects::{SpanStyle, SyntaxSpan};
use rustmark_core::shared::geometry::{ByteOffset, TextRange};
use syntect::parsing::ScopeStack;

/// 按 scope 栈判定语义样式。
///
/// 遍历 [`style_selectors`]，取**第一个**命中的选择器。
/// 正确性依赖 [`STYLE_SELECTORS`] 的顺序（具体优先），
/// 这与 syntect 自己按主题顺序应用样式的方式一致，且顺序可被测试锁定。
pub(crate) fn classify(stack: &ScopeStack) -> SpanStyle {
    let scopes = stack.as_slice();
    for (style, selector) in style_selectors() {
        if selector.does_match(scopes).is_some() {
            return *style;
        }
    }
    SpanStyle::Plain
}

/// 追加片段，并合并**同一行内**相邻且同样式的区间。
///
/// # 为什么合并不能跨行
///
/// syntect 是逐行解析的，跨行的块注释会表现为「每行一个片段」。
/// 若把它们合并成一个横跨多行的片段，就会与
/// [`SyntaxHighlighter::highlight_range`] 的结果**不一致**：
/// 全量高亮得到 `[30..54]` 一个片段，而只请求第 3 行时得到 `[30..40]`——
/// 验收标准④（增量与全量一致）直接失败。
/// 因此合并只在同一行内进行，跨行片段保持各行独立。
///
/// 这也是编辑器渲染器更需要的形状：egui 逐行布局，跨行片段反而要额外拆分。
///
/// # 合并的价值
///
/// 重绘是逐帧按片段遍历的，一个 200 行的大纲若产生上千个零散片段
/// 会明显增加每帧开销；而连续同语义的片段本就是冗余的。
pub(crate) fn push_span(
    spans: &mut Vec<SyntaxSpan>,
    start: usize,
    end: usize,
    style: SpanStyle,
    line_start: usize,
) {
    if let Some(last) = spans.last_mut()
        && last.style() == style
        && last.range().end().value() == start
        // 上一片段必须结束在**当前行之内**，才允许合并。
        && last.range().end().value() > line_start
    {
        *last = SyntaxSpan::new(
            TextRange::new(last.range().start(), ByteOffset::new(end)),
            style,
        );
        return;
    }
    spans.push(SyntaxSpan::new(
        TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
        style,
    ));
}
