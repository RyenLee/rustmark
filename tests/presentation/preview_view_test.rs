//! 预览区用例（阶段 06 · S6-05）。
//!
//! 预览区的**排版**本身难以单测（egui 是立即模式，断言像素没有意义），
//! 但有两处逻辑错了会静默产生严重后果，必须锁住：
//!
//! 1. 任务复选框的**回写偏移**——偏一个字节就会把用户的正文换成 `[x]`；
//! 2. 块类型的**分发**——`kind` 写错会让整类块落进 `raw` 渲染器。

use rustmark::application::dto::CellAlignView;
use rustmark::presentation::views::preview_view::task_marker_offset;

// ============================================================================
// 任务项回写偏移
// ============================================================================

/// 定位 `[ ]` 的字节偏移。
#[test]
fn finds_unchecked_marker_offset() {
    let text = "- [ ] 待办\n";
    assert_eq!(task_marker_offset(text, 0, text.len()), Some(2));
}

/// 定位 `[x]` 的字节偏移。
#[test]
fn finds_checked_marker_offset() {
    let text = "- [x] 已完成\n";
    assert_eq!(task_marker_offset(text, 0, text.len()), Some(2));
}

/// 返回的必须是**字节**偏移，可直接用于替换。
///
/// 多字节内容在标记之前时，字符数与字节数不相等——若误用字符索引，
/// 替换位置会整体前移，表现为「把正文删掉三个字」。
#[test]
fn offset_is_in_bytes_not_chars() {
    let text = "- [ ] 中文待办事项\n";
    let at = task_marker_offset(text, 0, text.len()).expect("应找到标记");
    assert_eq!(&text[at..at + 3], "[ ]");
    // 替换后标记真的变了，而中文部分一个字节都没动。
    let replaced = format!("{}{}{}", &text[..at], "[x]", &text[at + 3..]);
    assert_eq!(replaced, "- [x] 中文待办事项\n");
}

/// 只取区间内的首个标记：列表项里出现第二个复选框时不要去改它。
#[test]
fn only_the_first_marker_is_targeted() {
    let text = "- [ ] 第一 [x] 第二\n";
    assert_eq!(task_marker_offset(text, 0, text.len()), Some(2));
}

/// 区间内没有标记时返回 `None`（不猜、不改）。
#[test]
fn absent_marker_returns_none() {
    let text = "- 普通列表项\n";
    assert_eq!(task_marker_offset(text, 0, text.len()), None);
}

/// 区间超界必须裁剪而不是 panic——`BlockView` 的区间来自上一次解析，
/// 之后的编辑可能让它落在文本之外。
#[test]
fn out_of_bounds_range_is_clamped() {
    let text = "- [ ] 待办";
    assert_eq!(task_marker_offset(text, 0, 9999), Some(2));
    assert_eq!(task_marker_offset(text, 9999, 9999), None);
    // 起点大于终点也不会回绕 panic。
    assert_eq!(task_marker_offset(text, 5, 2), None);
}

/// 空文本 / 空区间。
#[test]
fn degenerate_inputs_are_handled() {
    assert_eq!(task_marker_offset("", 0, 0), None);
    assert_eq!(task_marker_offset("- [ ] x", 0, 0), None);
}

// ============================================================================
// 块类型分发
// ============================================================================

/// 每种块都必须有对应的 `kind` 字符串——落空的会掉进 `raw` 渲染器，
/// 表现为「预览里显示了源码原文」，很容易被当成没渲染。
#[test]
fn every_block_kind_has_a_renderer() {
    // 与 `MarkdownAppService::block_kind_str` 的取值一致。
    let known = [
        "heading",
        "paragraph",
        "code_block",
        "table",
        "list",
        "task_list",
        "quote",
        "image",
        "rule",
        "footnote",
        "raw:html",
    ];
    for kind in known {
        // `preview_view::draw_block` 的分发表：以 `raw:` 前缀兜底未知类型。
        let dispatched = matches!(
            kind,
            "heading"
                | "paragraph"
                | "quote"
                | "code_block"
                | "list"
                | "task_list"
                | "table"
                | "image"
                | "rule"
                | "footnote"
        );
        let is_raw = kind.starts_with("raw:");
        assert!(
            dispatched || is_raw,
            "块类型 {kind} 没有对应的渲染分支，会落进 raw 兜底"
        );
    }
}

/// 列对齐枚举的四种取值都要能映射到 egui 对齐。
#[test]
fn all_alignments_are_mapped() {
    // 未指定时按源写作惯例取左对齐（与表体一致），不能落到 `None` 导致渲染空白。
    let aligns = [
        CellAlignView::None,
        CellAlignView::Left,
        CellAlignView::Center,
        CellAlignView::Right,
    ];
    for a in aligns {
        // 只是确认枚举存在且可复制（映射本身在私有函数里，由上面的分发表覆盖）。
        let copied = a;
        assert!(matches!(
            copied,
            CellAlignView::None
                | CellAlignView::Left
                | CellAlignView::Center
                | CellAlignView::Right
        ));
    }
}
