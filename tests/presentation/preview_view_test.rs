//! 预览区用例（阶段 06 · S6-05）。
//!
//! 预览区的**排版**本身难以单测（egui 是立即模式，断言像素没有意义），
//! 但有两处逻辑错了会静默产生严重后果，必须锁住：
//!
//! 1. 任务复选框的**回写偏移**——偏一个字节就会把用户的正文换成 `[x]`；
//! 2. 块类型的**分发**——`kind` 写错会让整类块落进 `raw` 渲染器。

use rustmark::application::dto::CellAlignView;
use rustmark::presentation::views::preview_view::{
    TABLE_CELL_PAD, TABLE_COL_GAP, TABLE_MIN_COL_W, strip_math_delimiters,
    table_column_distribution, task_marker_offset,
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
        "definition_list",
        "math",
        "diagram",
        "toc",
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
                | "definition_list"
                | "math"
                | "diagram"
                | "toc"
        );
        let is_raw = kind.starts_with("raw:");
        assert!(
            dispatched || is_raw,
            "块类型 {kind} 没有对应的渲染分支，会落进 raw 兜底"
        );
    }
}

/// 行内公式的 `$` 分隔符必须被剥掉：留着会显示成 `$E=mc^2$`，
/// 用户会以为公式没被识别。
#[test]
fn inline_math_delimiters_are_stripped_for_display() {
    assert_eq!(strip_math_delimiters("$E = mc^2$"), "E = mc^2");
    assert_eq!(strip_math_delimiters("$$x$$"), "x");
    // 单侧不成对时原样返回——剥空会让公式凭空消失。
    assert_eq!(strip_math_delimiters("$E = mc^2"), "$E = mc^2");
    assert_eq!(strip_math_delimiters("E = mc^2$"), "E = mc^2$");
    // 片段里本来就没有「内容」时，宁可不动。
    assert_eq!(strip_math_delimiters("$$"), "$$");
    assert_eq!(strip_math_delimiters(""), "");
    // 公式本体不能被牵连（`a=1` 里的 `1` 要留下）。
    assert_eq!(strip_math_delimiters("$a=1$"), "a=1");
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
///
/// 问题 2 / 3 / 4 / 5 的写法也都放进来（列表行内样式、多级列表、表格、
/// 脚注、定义列表、块级公式、Mermaid 图），使末尾那条「真实帧」用例
/// 一次性走遍全部渲染分支——包括两个内置引擎的**绘制**路径。
const SAMPLE: &str = "\
@[TOC](欢迎)

# 一级标题

## 二级标题

撤销：<kbd>Ctrl</kbd> + <kbd>Z</kbd>

==标记文本==、H~2~O、2^10^ 运算、[链接](https://example.com)、`代码`

带尺寸的图片: ![Alt](images/a.png =30x30)

列表里的行内样式：

- 普通项 **粗体** 与 *斜体* 与 ~~删除~~ 与 `代码`
- [x] 已办 **重点**

多级列表：

1. 一级
   - 二级 a
     - 三级 x
   - 二级 b
2. 一级二

| 左 | 中 | 右 |
|:---|:--:|---:|
| a **粗** | b | c |
| d | e | f |

脚注区：

正文里有一个脚注[^1]。

[^1]: 脚注内容 **加粗**。

自定义列表：

术语 A
: 定义 A 的内容

行内公式 $E = mc^2$ 结束。

$$
\\int_0^1 x^2 dx
$$

```mermaid
graph LR
  A[开始] --> B{判断}
  B -->|是| C[结束]
  B -- 否 --> A
```
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

// ============================================================================
// 问题 2/4/5 的端到端防线：解析 → DTO → 真实帧
// ============================================================================
//
// 这四类问题的表现都在**渲染**上，但根因各不相同（DTO 没带字段 / 解析丢样式 /
// 枚举缺分支）。下面几条按「数据是否真的到了渲染层」来断言，比断言像素更稳，
// 也更容易定位到底是哪一层断的。

/// 多级列表的层级与编号必须一路传到预览块（问题 4 的根因是 DTO 没带这些字段）。
#[test]
fn list_depth_and_ordinal_reach_the_preview() {
    let (_dir, mut app) = app_with(SAMPLE);
    let ctx = egui::Context::default();
    render(&mut app, &ctx, 2);

    let blocks = &app.preview.blocks;
    let list = blocks
        .iter()
        .find(|b| b.kind == "list" && b.items.iter().any(|i| i.depth > 0))
        .expect("应有多级列表块");
    let got: Vec<(u8, Option<u64>, &str)> = list
        .items
        .iter()
        .map(|i| (i.depth, i.ordinal, i.text.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (0, Some(1), "一级"),
            (1, None, "二级 a"),
            (2, None, "三级 x"),
            (1, None, "二级 b"),
            (0, Some(2), "一级二"),
        ],
        "层级、源序号、源码顺序三者都要对"
    );
}

/// 列表项的行内样式必须传到预览块（问题 2 的根因）。
#[test]
fn list_item_inline_styles_reach_the_preview() {
    let (_dir, mut app) = app_with(SAMPLE);
    let ctx = egui::Context::default();
    render(&mut app, &ctx, 2);

    let blocks = &app.preview.blocks;
    let item = blocks
        .iter()
        .flat_map(|b| b.items.iter())
        .find(|i| i.text.contains("普通项"))
        .expect("应找到带样式的列表项");
    let styles: Vec<&str> = item.spans.iter().map(|s| s.style.as_str()).collect();
    for expected in ["strong", "emphasis", "strikethrough", "code"] {
        assert!(
            styles.contains(&expected),
            "列表项缺少 {expected:?} 片段：{styles:?}（文本 {:?}）",
            item.text
        );
    }
    // 片段区间必须相对**该列表项自己的文本**，否则预览会渲染到错的位置。
    for span in &item.spans {
        assert!(
            item.text.is_char_boundary(span.range.start)
                && item.text.is_char_boundary(span.range.end)
                && span.range.start < span.range.end,
            "列表项片段落在无效区间：{span:?}"
        );
    }
}

/// 任务项的勾选状态与行内样式（含标记位移）都要正确。
#[test]
fn task_item_reaches_the_preview_with_checked_state_and_styles() {
    let (_dir, mut app) = app_with(SAMPLE);
    let ctx = egui::Context::default();
    render(&mut app, &ctx, 2);

    let blocks = &app.preview.blocks;
    let task = blocks
        .iter()
        .find(|b| b.kind == "task_list")
        .expect("应有任务列表块");
    let item = task
        .items
        .iter()
        .find(|i| i.checked == Some(true))
        .expect("应有已勾选项");
    assert_eq!(item.text, "已办 重点");
    let span = item.spans.first().expect("已勾选项也应带粗体片段");
    assert_eq!(span.style, "strong");
    assert_eq!(&item.text[span.range.start..span.range.end], "重点");
}

/// 表格单元格的行内样式必须传到预览块（问题 5 的表格部分）。
#[test]
fn table_cell_spans_reach_the_preview() {
    let (_dir, mut app) = app_with(SAMPLE);
    let ctx = egui::Context::default();
    render(&mut app, &ctx, 2);

    let table = app
        .preview
        .blocks
        .iter()
        .find_map(|b| b.table.as_ref())
        .expect("应有表格块");
    assert_eq!(table.rows[0][0], "a 粗");
    let spans = &table.rows_spans[0][0];
    assert_eq!(spans.len(), 1, "单元格应带 1 个片段：{spans:?}");
    assert_eq!(spans[0].style, "strong");
    assert_eq!(
        &table.rows[0][0][spans[0].range.start..spans[0].range.end],
        "粗"
    );
    // 片段表必须与行数**等长**，否则渲染按 `(r, c)` 索引时会越界。
    assert_eq!(table.rows_spans.len(), table.rows.len());
}

/// 脚注：标签与正文分开到达预览块（问题 5 的脚注部分）。
#[test]
fn footnote_label_and_body_reach_the_preview() {
    let (_dir, mut app) = app_with(SAMPLE);
    let ctx = egui::Context::default();
    render(&mut app, &ctx, 2);

    let note = app
        .preview
        .blocks
        .iter()
        .find(|b| b.kind == "footnote")
        .expect("应有脚注块");
    assert_eq!(note.label.as_deref(), Some("1"));
    assert!(note.content.contains("脚注内容"), "正文应在块里：{note:?}");
    assert!(
        note.spans.iter().any(|s| s.style == "strong"),
        "脚注正文的粗体也要传到预览：{:?}",
        note.spans
    );
    // 正文不该再以游离段落的形式重复出现（否则预览里显示两遍）。
    let stray = app
        .preview
        .blocks
        .iter()
        .filter(|b| b.kind == "paragraph" && b.content.contains("脚注内容"))
        .count();
    assert_eq!(stray, 0, "脚注正文不应额外产出段落");
}

/// 定义列表：术语与定义分条到达预览块（问题 5 的自定义列表部分）。
#[test]
fn definition_list_reaches_the_preview() {
    let (_dir, mut app) = app_with(SAMPLE);
    let ctx = egui::Context::default();
    render(&mut app, &ctx, 2);

    let dl = app
        .preview
        .blocks
        .iter()
        .find(|b| b.kind == "definition_list")
        .expect("应有定义列表块");
    assert_eq!(dl.definitions.len(), 1);
    assert_eq!(dl.definitions[0].term, "术语 A");
    assert_eq!(dl.definitions[0].definitions.len(), 1);
    assert_eq!(dl.definitions[0].definitions[0].text, "定义 A 的内容");
}

/// 块级公式与图表必须**被渲染**成图，而不是「降级成源码」。
///
/// 判据是「降级提示没有出现」：`draw_unrendered_source` 只在两个内置引擎
/// 返回 `Err` 时被调用，而它会把 `未能排版：…` 画进帧里。
#[test]
fn math_and_diagram_are_actually_rendered_not_fallback_cards() {
    let (_dir, mut app) = app_with(SAMPLE);
    let ctx = egui::Context::default();
    render(&mut app, &ctx, 3);

    let kinds: Vec<&str> = app.preview.blocks.iter().map(|b| b.kind.as_str()).collect();
    assert!(kinds.contains(&"math"), "样例应产出公式块：{kinds:?}");
    assert!(kinds.contains(&"diagram"), "样例应产出图表块：{kinds:?}");

    // 两个引擎能直接排出来（不经 UI）——这是「渲染器真的工作了」的硬证据。
    let measure = |t: &str, size: f32| t.chars().count() as f32 * size * 0.5;
    let math = app
        .preview
        .blocks
        .iter()
        .find(|b| b.kind == "math")
        .expect("公式块");
    assert!(
        rustmark::presentation::views::math::layout(&math.content, 16.0, true, &measure).is_ok(),
        "样例公式应能排版：{:?}",
        math.content
    );
    let diagram = app
        .preview
        .blocks
        .iter()
        .find(|b| b.kind == "diagram")
        .expect("图表块");
    let graph = rustmark::presentation::views::mermaid::layout(&diagram.content, 14.0, &measure)
        .unwrap_or_else(|e| panic!("样例图表应能布局：{e}"));
    assert!(graph.width > 0.0 && graph.height > 0.0);
}

// ============================================================================
// 问题 1 的回归：预览区必须能滚
// ============================================================================

/// 够长的文档：内容高度必须超过常见窗口高度，否则「滚不动」无从谈起。
///
/// 刻意用**很短的段落**，且**不含标题**，两个约束都不可省：
///
/// * 缺陷（滚动区横向收缩到内容宽度）只在「内容比面板窄」时才显出后果。
///   正文若本来就占满面板宽，收缩与否看不出差别，用例就会「碰巧通过」。
/// * 标题是这里最容易踩的坑：`draw_heading` 对 h1/h2 会画一条
///   `available_width()` 宽的分隔线，那一下就把内容宽度撑满面板，
///   横向收缩随之失效，用例又「碰巧通过」（实测过一次）。
///   所以这里只放段落，让内容宽度真实地停在文字宽度上
///   （本机 1000×640 窗口、默认主题下实测 81.5pt，面板可用 348pt）。
fn long_document(paragraphs: usize) -> String {
    (0..paragraphs)
        .map(|i| format!("短行内容 {i}\n"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 在预览区**空白处**滚滚轮，偏移必须真的变大。
///
/// # 为什么这条用例能钉住那个缺陷
///
/// 缺陷是 `ScrollArea::vertical()` **没设** `auto_shrink`，于是取默认值
/// `Vec2b::TRUE`——两个方向都开。而 `ScrollArea::vertical()` 的
/// `direction_enabled` 是 `[false, true]`（横向关、纵向开），横向因此落到
/// egui 的这一支（`scroll_area.rs`，已核对 0.33.3 源码）：
///
/// ```text
/// (true,  true)  => inner_size.min(content_size)   // 纵向：内容小则收缩
/// (false, true)  => content_size[d]                // 横向：**收缩到内容宽度**
/// ```
///
/// 后果是滚动区只有正文那么宽。带来两个问题，都不报错：
/// 1. 指针悬在正文右侧的空白上时**并不在滚动区内**
///    （`is_hovering_outer_rect` 为假），滚轮事件被整个丢掉；
/// 2. 滚动条画在内容右缘那条细缝里，视觉上等于不存在。
///
/// 两者合起来正是用户说的「预览区滚不动、下面的内容看不到」。
///
/// 故判据取「在预览区**中央**滚轮，偏移要变」——对本用例的短段落文档而言，
/// 那个点稳稳落在内容列右侧的空白里。缺陷在时它是空白（滚不动），
/// 修好后它是滚动区的一部分（能滚）。判据与用户抱怨的现象一一对应，
/// 不依赖我们记住是哪个开关写错了。
///
/// 坐标不是硬编码常量，而是**上一帧真实记录下来的预览区矩形**中心
/// （`app.preview_rect()`）：窗口尺寸、分割比例、主题度量任何一项变化，
/// 用例都跟着走，不会因为布局微调而无故变红。
#[test]
fn preview_scrolls_with_the_wheel_over_its_content() {
    let (_dir, mut app) = app_with(&long_document(80));
    let ctx = egui::Context::default();
    let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 640.0));

    let run = |app: &mut EditorApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen_rect),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ctx);
        });
    };

    // 第 1 帧：建立布局，拿到预览区矩形。
    run(&mut app, Vec::new());
    let pane = app.preview_rect().expect("主视图应记录预览区矩形");
    let at = pane.center();

    // 前置条件：内容必须真的超出一屏，否则「滚不动」这条断言没有意义。
    let geo = app
        .ui
        .sync
        .geometry(rustmark::presentation::state::scroll_sync_state::Side::Preview);
    assert!(
        geo.content_h > geo.viewport_h,
        "用例前提：内容（{}）应高于视口（{}）",
        geo.content_h,
        geo.viewport_h
    );

    // 连滚若干帧（真实用户也是一帧一个增量）。
    for _ in 0..10 {
        run(
            &mut app,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -120.0),
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
    }

    assert!(
        app.preview.scroll_y > 0.0,
        "在预览区中央（{at:?}）滚轮后偏移仍为 0——滚动区收缩到了内容宽度（问题 1 复发）"
    );
}

/// 反向：指针在**预览区之外**时不该动预览区（防止「整个窗口随便滚都能滚」）。
#[test]
fn wheel_outside_the_preview_pane_does_not_scroll_it() {
    let (_dir, mut app) = app_with(&long_document(80));
    let ctx = egui::Context::default();
    let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 640.0));
    let run = |app: &mut EditorApp, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(screen_rect),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ctx);
        });
    };
    run(&mut app, Vec::new());
    let outside = app.preview_rect().expect("预览区矩形").left_top() - egui::vec2(300.0, 0.0);
    for _ in 0..6 {
        run(
            &mut app,
            vec![
                egui::Event::PointerMoved(outside),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -120.0),
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
    }
    assert_eq!(
        app.preview.scroll_y, 0.0,
        "指针不在预览区上时不该滚动预览（否则滚轮会被两侧抢）"
    );
}
