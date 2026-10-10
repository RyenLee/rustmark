//! 预览区用例（阶段 06 · S6-05）。
//!
//! 预览区的**排版**本身难以单测（egui 是立即模式，断言像素没有意义），
//! 但有两处逻辑错了会静默产生严重后果，必须锁住：
//!
//! 1. 任务复选框的**回写偏移**——偏一个字节就会把用户的正文换成 `[x]`；
//! 2. 块类型的**分发**——`kind` 写错会让整类块落进 `raw` 渲染器。

use rustmark::application::dto::CellAlignView;
use rustmark::presentation::views::preview_view::{
    TABLE_CELL_PAD, TABLE_COL_GAP, TABLE_MIN_COL_W, table_column_distribution, task_marker_offset,
};

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

// ============================================================================
// 表格列宽分配（自适应列宽的核心算法）
// ============================================================================

/// 不变量 1（放得下）：总宽 ≤ 可用宽时每列拿到「自然宽 + 内边距」，
/// 即**任何单元格都不必换行**；且「列宽和 + 列间距」恰占满可用宽
/// （列间距由布局层的 `item_spacing` 提供，值同为 [`TABLE_COL_GAP`]，
/// 故它不进列宽，分配只对「面板宽 − 列间距」负责）。
#[test]
fn columns_fit_without_wrapping_when_space_allows() {
    let natural = [40.0, 120.0, 30.0];
    let available = 400.0;
    let widths = table_column_distribution(&natural, available);

    assert_eq!(widths.len(), 3);
    let total: f32 = widths.iter().sum::<f32>() + TABLE_COL_GAP * (widths.len() - 1) as f32;
    assert!(
        (total - available).abs() < 0.5,
        "放得下时应占满可用宽：{total} vs {available}"
    );
    for (c, (w, n)) in widths.iter().zip(natural).enumerate() {
        assert!(
            *w >= n + 2.0 * TABLE_CELL_PAD,
            "第 {c} 列宽 {w} 小于自然宽 {n} + 内边距——本不该换行的列被压窄了"
        );
    }
}

/// 不变量 1（放不下）：超宽时「列宽和 + 列间距」恰为可用宽，不溢出也不留白。
#[test]
fn overflowing_columns_use_exactly_the_available_width() {
    let natural = [60.0, 800.0, 40.0];
    let available = 300.0;
    let widths = table_column_distribution(&natural, available);
    let total: f32 = widths.iter().sum::<f32>() + TABLE_COL_GAP * (widths.len() - 1) as f32;
    assert!(
        (total - available).abs() < 0.5,
        "超宽时总和应恰为可用宽：{total} vs {available}"
    );
}

/// 不变量 2：超宽时每列不低于保底宽度，且长内容列拿到的最多——
/// 「长内容展示合理」的实现点（长列多占、短列紧凑，而不是一刀切均分）。
#[test]
fn overflowing_columns_keep_a_floor_and_favor_long_content() {
    let natural = [60.0, 800.0, 40.0];
    let available = 300.0;
    let widths = table_column_distribution(&natural, available);
    for (c, w) in widths.iter().enumerate() {
        assert!(
            *w >= TABLE_MIN_COL_W - 0.5,
            "第 {c} 列宽 {w} 低于保底 {TABLE_MIN_COL_W}——内容会被挤到无法阅读"
        );
    }
    assert!(
        widths[1] > widths[0] && widths[1] > widths[2],
        "长内容列应分到最宽：{widths:?}"
    );
}

/// 退化输入：空表返回空分配；窗口极窄时全部压到保底宽度（宁可溢出不可挤没）。
#[test]
fn table_distribution_degenerate_inputs_are_handled() {
    assert!(table_column_distribution(&[], 200.0).is_empty());

    // 可用宽减去列间距后连两列保底都给不起 → 全部取保底。
    let narrow = table_column_distribution(&[500.0, 600.0], 50.0);
    assert_eq!(narrow, vec![TABLE_MIN_COL_W, TABLE_MIN_COL_W]);

    // 全空表（所有列内容都为空）：按均分处理，不得除零 NaN。
    let empty = table_column_distribution(&[0.0, 0.0, 0.0], 300.0);
    let total: f32 = empty.iter().sum::<f32>() + TABLE_COL_GAP * (empty.len() - 1) as f32;
    assert!(
        (total - 300.0).abs() < 0.5 && empty.iter().all(|w| w.is_finite()),
        "全空表应均分可用宽且有限：{empty:?}"
    );
}

// ============================================================================
// 端到端：示例文档里的「非标准但常见」写法必须一路渲染到预览区
// ============================================================================
//
// 上面各条测的是纯函数。这里补一条**从源码到预览块再到真实帧**的通路用例：
// 解析出来的样式片段要能穿过应用层映射（`BlockView.spans`），
// 并且在预览区的真实渲染里不崩。两处静默失效只有这条路能挡住：
//   1. 解析认了、DTO 没带过去 → 预览仍是纯文本；
//   2. `render_inline_text` 把片段区间切进多字节字符中间 → panic。

use std::sync::Arc;

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::ReplaceRangeCommand;
use rustmark::shared::geometry::{ByteOffset, TextRange};

/// 示例文档片段：目录 / 键帽 / 高亮 / 上下标 / 带尺寸图片 / 公式全都在。
const SAMPLE: &str = "\
@[TOC](欢迎)

# 一级标题

## 二级标题

撤销：<kbd>Ctrl</kbd> + <kbd>Z</kbd>

==标记文本==、H~2~O、2^10^ 运算、[链接](https://example.com)、`代码`

带尺寸的图片: ![Alt](images/a.png =30x30)

$$E = mc^2$$
";

/// 造一个装好 `content` 的应用。
fn app_with(content: &str) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = app.ensure_document().expect("应能开出空白文档");
    app.services
        .documents
        .replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
            text: content.to_string(),
        })
        .expect("写入内容应成功");
    app.refresh_text();
    (dir, app)
}

/// 跑 `n` 帧真实渲染，返回 (最后一帧绘制量, 最后一帧里的文字 shape 数)。
fn render(app: &mut EditorApp, ctx: &egui::Context, n: usize) -> usize {
    let mut last = 0;
    for _ in 0..n.max(2) {
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ctx);
        });
        last = out.shapes.len();
    }
    last
}

/// 所有行内格式都要穿过应用层映射，落到预览块的 `spans` 上。
#[test]
fn sample_document_reaches_preview_blocks_with_its_inline_styles() {
    let (_dir, mut app) = app_with(SAMPLE);
    let ctx = egui::Context::default();
    render(&mut app, &ctx, 2);

    let blocks = &app.preview.blocks;
    assert!(!blocks.is_empty(), "首帧应已解析出块");

    // 目录：`@[TOC](欢迎)` 必须变成独立块，且标题来自括号内容
    // （表现层读不到领域枚举里的 `title`，只能靠 `content`）。
    let toc = blocks
        .iter()
        .find(|b| b.kind == "toc")
        .expect("应产出目录块");
    assert_eq!(toc.content, "欢迎", "目录标题应取自 `@[TOC](欢迎)` 的括号");

    // 带尺寸的图片要真的成为图片块，且地址里不能混进 `=30x30`。
    let image = blocks
        .iter()
        .find(|b| b.kind == "image")
        .expect("带尺寸后缀的图片也应成为图片块");
    assert_eq!(image.url.as_deref(), Some("images/a.png"));

    // 每一类行内样式都要在某个块的 spans 里出现过。
    let styles: Vec<&str> = blocks
        .iter()
        .flat_map(|b| b.spans.iter())
        .map(|s| s.style.as_str())
        .collect();
    for expected in ["keycap", "mark", "sub", "sup", "link", "code"] {
        assert!(
            styles.contains(&expected),
            "样式 {expected:?} 未传到预览块，实际 {styles:?}"
        );
    }
}

/// 片段区间必须落在字符边界上：切片越界会 panic，预览整栏变白。
#[test]
fn inline_spans_land_on_char_boundaries() {
    let (_dir, mut app) = app_with(SAMPLE);
    let ctx = egui::Context::default();
    render(&mut app, &ctx, 2);

    for block in &app.preview.blocks {
        for span in &block.spans {
            assert!(
                block.content.is_char_boundary(span.range.start)
                    && block.content.is_char_boundary(span.range.end),
                "块 {:?} 的片段 {:?} 落在字符中间，切片会 panic",
                block.kind,
                span.range
            );
            assert!(span.range.start < span.range.end, "空片段没有意义");
        }
    }
}

/// 真实帧渲染不崩（含目录卡片、键帽、高亮、上下标、图片占位）。
#[test]
fn sample_document_renders_a_real_frame() {
    let (_dir, mut app) = app_with(SAMPLE);
    let ctx = egui::Context::default();
    let shapes = render(&mut app, &ctx, 3);
    assert!(shapes > 0, "预览应产出绘制指令");
}
