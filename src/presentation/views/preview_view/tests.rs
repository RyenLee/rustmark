//! 模块外置测试。
use super::*;
use crate::application::dto::{RangeView, SpanView};
// R-10 对拍用例要访问行内路径的私有项（样式表 / 构造器 / 命中区间）。
use super::inline::{Typography, build_text_job, done_overlay, span_format};
use crate::presentation::theme::mapper::SpanHit;
use crate::shared::geometry::clamp_to_char_boundaries;

/// DEF-04：span 边界落在多字节字符中间时，预览区代码块不得 panic。
///
/// `panic = "abort"` 下 panic 即进程终止，故这条是崩溃级回归。
/// 生产路径里 span 与文本同源，缺陷不会触发；但「同源」只是约定，
/// 缓存版本错位就能打破它——表现层必须自己扛住。
#[test]
fn highlighted_code_job_survives_non_char_boundary_spans() {
    let theme = Theme::dark();
    // "中文abc"：中=0..3、文=3..6。1 与 5 都落在字符内部。
    let text = "中文abc";
    assert!(
        !text.is_char_boundary(1) && !text.is_char_boundary(5),
        "用例前提：这两个偏移必须是非字符边界"
    );
    let spans = vec![SpanView {
        range: RangeView::new(1, 5),
        style: "keyword".to_string(),
        url: None,
    }];
    let job = highlighted_code_job(&theme, text, Some(&spans), f32::INFINITY);
    // 不 panic 即通过；再断言文本被完整覆盖（少上色可以，丢字符不行）。
    let covered: String = job
        .sections
        .iter()
        .map(|s| {
            job.text
                .get(s.byte_range.start.0..s.byte_range.end.0)
                .unwrap_or("")
        })
        .collect();
    assert_eq!(covered, text, "吸附边界后不得丢字符");
}

/// 无 span 时退化为纯文本，覆盖整段（防止吸附逻辑误伤正常路径）。
#[test]
fn highlighted_code_job_covers_whole_text_without_spans() {
    let theme = Theme::dark();
    let text = "fn main() {}\n";
    let job = highlighted_code_job(&theme, text, None, f32::INFINITY);
    let covered: String = job
        .sections
        .iter()
        .map(|s| {
            job.text
                .get(s.byte_range.start.0..s.byte_range.end.0)
                .unwrap_or("")
        })
        .collect();
    assert_eq!(covered, text);
}

/// 问题 2 回归：开启「自动换行」时，代码块必须按给定宽度折行。
///
/// 修复前 `draw_highlighted_code` 把 `wrap_width` 写死 `INFINITY`，
/// 长代码行永远撑出预览区横向滚动条——「自动换行」开关对代码块
/// 完全不生效。现在宽度由调用方（`draw_code_block` 的 `wrap` 分支）
/// 透传，本测试钉住这条通路。
#[test]
fn highlighted_code_job_wraps_at_requested_width() {
    let theme = Theme::dark();
    let text = "let x = 1;";
    let wrapped = highlighted_code_job(&theme, text, None, 120.0);
    assert_eq!(
        wrapped.wrap.max_width, 120.0,
        "开启自动换行时代码应按面板宽度折行"
    );
    // 关闭时（INFINITY）保持不折行：由预览区横向滚动条查看，
    // 与编辑区行为一致（既有设计不变）。
    let nowrap = highlighted_code_job(&theme, text, None, f32::INFINITY);
    assert!(
        nowrap.wrap.max_width.is_infinite(),
        "关闭自动换行时代码不应折行"
    );
}

/// 注脚跳转（S6-05 #10）：正文 `[^1]` 的显示标号必须与 `internal_targets`
/// 里脚注定义的 key 一致，点击才能命中对侧块。
#[test]
fn footnote_ref_mark_strips_brackets() {
    assert_eq!(footnote_ref_mark("[^1]"), "1");
    assert_eq!(footnote_ref_mark("[^note]"), "note");
    // 不完整片段（解析 / 渲染边界不一致时）原样返回，绝不剥错。
    assert_eq!(footnote_ref_mark("^1]"), "^1]");
    assert_eq!(footnote_ref_mark("[]"), "[]");
}

/// 标题锚点：`internal_targets` 的 key 由 `heading_slug` 生成，预览里
/// 点 `#锚点` 链接即据此匹配标题块。
#[test]
fn heading_slug_is_deterministic_and_ascii_safe() {
    assert_eq!(heading_slug("Hello World"), "hello-world");
    // 与 GitHub 锚点生成对齐：连续空白各转一个连字符，不合并
    // （GitHub 上 `## Trim  Me` 的锚点就是 `#trim--me`）。
    assert_eq!(heading_slug("  Trim  Me  "), "trim--me");
    // 标点直接丢弃；CJK 字符**保留**（`is_alphanumeric()` 对汉字为真），
    // 于是 `## 第一章` 的锚点就是 `#第一章`——中文标题得以被直接链接，
    // 且不同中文标题不会因「全部丢弃」而撞同一个空 slug。
    assert_eq!(heading_slug("第一章"), "第一章");
    assert_eq!(heading_slug("Chapter 1: 引言"), "chapter-1-引言");
}

// ========================================================================
// R-10 对拍：行内路径（顺序游标 + 文本改写 + 命中区间）
//
// 行内的 `span_format` / `done_overlay` / `build_text_job` 都是私有项，
// 集成测试（`tests/`）拿不到 → 对拍用例只能放在 crate 内部。
// ========================================================================

/// 旧 `build_text_job`（顺序游标语义）——R-10 收敛前的实现，原文抄录。
///
/// 抄自 `67210f2^:src/presentation/views/preview_view/inline.rs`。
fn legacy_build_text_job(
    theme: &Theme,
    content: &str,
    spans: &[SpanView],
    typo: Typography<'_>,
    done: bool,
    wrap_width: f32,
) -> (egui::text::LayoutJob, Vec<SpanHit>) {
    let Typography {
        size,
        line_height,
        base_style,
    } = typo;
    let base = span_format(theme, base_style, size, line_height);
    let mut job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: wrap_width.max(0.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut hits: Vec<SpanHit> = Vec::new();
    let len = content.len();
    let mut cursor = 0usize;
    let mut char_cursor = 0usize;
    if spans.is_empty() {
        let mut fmt = base.clone();
        if done {
            fmt = done_overlay(theme, fmt);
        }
        job.append(content, 0.0, fmt);
    } else {
        for (idx, span) in spans.iter().enumerate() {
            let Some((start, end)) = clamp_to_char_boundaries(
                content,
                span.range.start.min(len),
                span.range.end.min(len),
            ) else {
                continue;
            };
            let start = start.max(cursor);
            let end = end.max(start);
            if start > cursor {
                let base_slice = &content[cursor..start];
                job.append(base_slice, 0.0, base.clone());
                char_cursor += base_slice.chars().count();
            }
            if end > start {
                let mut fmt = span_format(theme, &span.style, size, line_height);
                if done {
                    fmt = done_overlay(theme, fmt);
                }
                let slice = match span.style.as_str() {
                    "math" => strip_math_delimiters(&content[start..end]),
                    "footnote_ref" => footnote_ref_mark(&content[start..end]),
                    _ => &content[start..end],
                };
                let slice_chars = slice.chars().count();
                job.append(slice, 0.0, fmt);
                if span.style == "link" || span.style == "footnote_ref" {
                    hits.push(SpanHit {
                        idx,
                        c0: char_cursor,
                        c1: char_cursor + slice_chars,
                    });
                }
                char_cursor += slice_chars;
            }
            cursor = end.max(cursor);
        }
        if cursor < len {
            let tail = &content[cursor..];
            job.append(tail, 0.0, base.clone());
        }
    }
    (job, hits)
}

/// `LayoutJob` 的可比指纹（每段文本 + 全部可见样式）。
fn job_fingerprint(job: &egui::text::LayoutJob) -> Vec<String> {
    job.sections
        .iter()
        .map(|s| {
            let t = job
                .text
                .get(s.byte_range.start.0..s.byte_range.end.0)
                .unwrap_or("<越界>");
            format!(
                "text={t:?} color={:?} bg={:?} size={} ital={} strike={} ul={}",
                s.format.color,
                s.format.background,
                s.format.font_id.size,
                s.format.italics,
                s.format.strikethrough.width > 0.0,
                s.format.underline.width > 0.0,
            )
        })
        .collect()
}

/// 行内路径：新实现必须与旧实现**逐段等价**。
///
/// 覆盖：无 span / 单 span / 多 span / 有间隙 / 多字节 / 非字符边界 /
/// `math` 改写 / `footnote_ref` 改写 / 已勾选任务项。
#[test]
fn inline_cursor_matches_legacy_implementation() {
    let theme = Theme::dark();
    let size = theme.metrics.preview_font_size;
    let line_height = size * theme.metrics.preview_line_height;
    let mk = |start: usize, end: usize, style: &str| SpanView {
        range: RangeView::new(start, end),
        style: style.to_string(),
        url: None,
    };

    let cases: Vec<(&str, &str, Vec<SpanView>, &str, bool)> = vec![
        ("无 span", "普通文本", vec![], "", false),
        // **字节区间**（不是字符数！）：中文字符占 3 字节。
        // 每条都带「前提」断言（见下）确保区间没写错。
        ("单 span", "a**粗**b", vec![mk(3, 6, "strong")], "", false),
        (
            "多 span",
            "a**粗**_斜_",
            vec![mk(3, 6, "strong"), mk(9, 12, "emphasis")],
            "",
            false,
        ),
        (
            "有间隙",
            "ab **粗** cd",
            vec![mk(5, 8, "strong")],
            "",
            false,
        ),
        ("多字节", "中**文**", vec![mk(5, 8, "strong")], "", false),
        ("非字符边界", "中文abc", vec![mk(1, 5, "strong")], "", false),
        (
            "math 改写",
            "值 $E=mc^2$ 结束",
            vec![mk(4, 12, "math")],
            "",
            false,
        ),
        (
            "footnote 改写",
            "见[^1]处",
            vec![mk(3, 7, "footnote_ref")],
            "",
            false,
        ),
        (
            // **两个命中 span**：命中区间的 `c0` / `c1` 是**字符**下标，
            // 必须随前文长度**累计**。
            //
            // 为什么单命中那条测不出来：只有一个命中 span 时，
            // `char_cursor` 无论怎么算都从 0 起（`c0=0`），
            // 而 `c1 = c0 + slice_chars` 也只依赖本段自身长度——
            // 把 `char_cursor += slice_chars` 改成 `+= 0` **看不出差别**。
            // 实测（变异验证）：本条是唯一能让该变异变红的用例。
            //
            // 字节区间："ab [x] cd [y] ef"
            //   a=0 b=1 ' '=2 [=3 x=4 ]=5 ' '=6 c=7 d=8 ' '=9
            //   [=10 y=11 ]=12 ' '=13 e=14 f=15
            // 第一个 link 占 3..6，第二个占 10..13。
            // 第二个命中的 `c0` 应为 10（字符下标），
            // 而不是「第一个 link 结束处」或别的错值。
            "两个命中 span",
            "ab [x] cd [y] ef",
            vec![mk(3, 6, "link"), mk(10, 13, "link")],
            "",
            false,
        ),
        ("已勾选", "已完成", vec![], "task_done", true),
        (
            "已勾选 + span",
            "已完成**重要**",
            vec![mk(11, 17, "strong")],
            "task_done",
            true,
        ),
    ];

    for (name, content, spans, base_style, done) in cases {
        // 前提：span 区间必须**非空**。
        //
        // 不加这条断言，字节数写错（如把「粗」的 3..6 写成 3..5）会让区间
        // 被吸附成空 → span 段根本不生成 → 用例「通过」但没测到东西。
        //
        // 注意**不能**断言「必须是字符边界」：本表里「非字符边界」那条
        // **故意**用非边界区间（测 DEF-04 的防护）。故只查非空，
        // 字符边界由下面「非字符边界」那条用例自己的断言负责。
        for s in &spans {
            assert!(
                s.range.end > s.range.start,
                "用例前提错误 [{name}]：span {:?} 为空区间",
                s.range
            );
        }
        let typo = Typography {
            size,
            line_height,
            base_style,
        };
        let (old, old_hits) = legacy_build_text_job(&theme, content, &spans, typo, done, 300.0);
        let (new, new_hits) = build_text_job(&theme, content, &spans, typo, done, 300.0);

        assert_eq!(
            job_fingerprint(&old),
            job_fingerprint(&new),
            "行内路径 [{name}] 与旧实现不等价"
        );
        assert_eq!(
            old_hits, new_hits,
            "行内路径 [{name}] 的命中区间与旧实现不等价"
        );
    }
}

/// 行内路径：base 段（span 未覆盖的文本）必须用 **base 样式**。
///
/// 行内的 base 样式由 `base_style` 决定（`"heading"` / `"strong"` /
/// `"quote"`…），与 span 样式表是**两套**。若 base 段也走 span 样式表，
/// 基准样式（标题色 / 粗体 / 引用斜体）就会**丢失**。
///
/// 判据：base_style = `"quote"`（次级色 + 斜体）时，base 段必须是
/// quote 的样式；span 段是 `strong` 的样式。两者必须不同。
#[test]
fn inline_base_segment_uses_base_style_not_span_style_table() {
    let theme = Theme::dark();
    let size = theme.metrics.preview_font_size;
    let line_height = size * theme.metrics.preview_line_height;
    let typo = Typography {
        size,
        line_height,
        base_style: "quote",
    };
    // "ab 粗 cd"：span 盖住中间的 "粗"（字节 3..6），两侧是 base 段。
    let spans = vec![SpanView {
        range: RangeView::new(3, 6),
        style: "strong".to_string(),
        url: None,
    }];
    assert!(
        "ab 粗 cd".is_char_boundary(3) && "ab 粗 cd".is_char_boundary(6),
        "用例前提：span 区间必须是字符边界"
    );
    let (job, _) = build_text_job(&theme, "ab 粗 cd", &spans, typo, false, 300.0);

    let quote_fmt = span_format(&theme, "quote", size, line_height);
    let strong_fmt = span_format(&theme, "strong", size, line_height);
    assert_ne!(
        quote_fmt.color, strong_fmt.color,
        "用例前提：quote 与 strong 的颜色必须不同，否则测不出 base 段是否走错样式表"
    );

    let base_seg = job
        .sections
        .iter()
        .find(|s| {
            job.text
                .get(s.byte_range.start.0..s.byte_range.end.0)
                .is_some_and(|t| t.contains("ab "))
        })
        .expect("应存在 base 段");
    assert_eq!(
        base_seg.format.color, quote_fmt.color,
        "base 段必须用 base 样式（quote），不得走 span 样式表"
    );
}

/// **用例自身的正确性**：每个 span 区间必须覆盖**预期的文本**。
///
/// 这条用例存在的理由：对拍用例的 span 区间是**字节**偏移，中文字符占
/// 3 字节——写错（把「粗」的 `3..6` 写成 `3..5`）会让区间被吸附成空
/// → span 段根本不生成 → 对拍用例「通过」但**根本没测到那条路径**。
///
/// 教训：判据里若含手工计算的数字，必须有一条独立用例钉住那些数字。
/// 否则数字错了，判据会**恒绿**且看起来一切正常。
#[test]
fn parity_fixture_spans_cover_expected_text() {
    // 类型别名：`Vec<(&str, &str, Vec<(usize, usize)>)>` 触发 clippy 的
    // `type_complexity`（嵌套元组 + 嵌套 Vec）。
    type Case = (&'static str, &'static str, Vec<(usize, usize)>);
    let cases: Vec<Case> = vec![
        ("a**粗**b", "粗", vec![(3, 6)]),
        ("a**粗**_斜_", "粗", vec![(3, 6)]),
        ("a**粗**_斜_", "斜", vec![(9, 12)]),
        ("ab **粗** cd", "粗", vec![(5, 8)]),
        ("中**文**", "文", vec![(5, 8)]),
        ("值 $E=mc^2$ 结束", "$E=mc^2$", vec![(4, 12)]),
        ("见[^1]处", "[^1]", vec![(3, 7)]),
        ("已完成**重要**", "重要", vec![(11, 17)]),
    ];
    for (text, expected, ranges) in cases {
        for (s, e) in ranges {
            assert!(
                text.is_char_boundary(s) && text.is_char_boundary(e),
                "用例前提错误：{text:?} 的 ({s},{e}) 不是字符边界"
            );
            assert_eq!(
                text.get(s..e),
                Some(expected),
                "用例前提错误：{text:?} 的 ({s},{e}) 应覆盖 {expected:?}"
            );
        }
    }
}

/// 行内路径：拼接文本必须与原文逐字节相同。
#[test]
fn inline_job_text_always_equals_source_text() {
    let theme = Theme::dark();
    let size = theme.metrics.preview_font_size;
    let line_height = size * theme.metrics.preview_line_height;
    let typo = Typography {
        size,
        line_height,
        base_style: "",
    };
    let cases: Vec<(&str, Vec<SpanView>)> = vec![
        ("普通文本", vec![]),
        (
            "a**粗**b",
            vec![SpanView {
                range: RangeView::new(1, 3),
                style: "strong".to_string(),
                url: None,
            }],
        ),
        (
            "值 $E=mc^2$ 结束",
            vec![SpanView {
                range: RangeView::new(2, 10),
                style: "math".to_string(),
                url: None,
            }],
        ),
    ];
    for (content, spans) in cases {
        let (job, _) = build_text_job(&theme, content, &spans, typo, false, 300.0);
        let text: String = job
            .sections
            .iter()
            .map(|s| {
                job.text
                    .get(s.byte_range.start.0..s.byte_range.end.0)
                    .unwrap_or("")
            })
            .collect();
        // `math` 会被改写（剥掉 `$` 分隔符）→ 文本**故意**不等于原文。
        // 这里只断言「不丢字符、不重复」：改写后的文本应是原文的子序列。
        assert!(
            text.chars().count() <= content.chars().count(),
            "行内文本不得变长（content={content:?} → {text:?}）"
        );
        if !content.contains('$') {
            assert_eq!(text, content, "行内文本必须与原文逐字节相同");
        }
    }
}

// ========================================================================
// 真帧对拍：编辑区路径 vs 预览区代码块路径
// ========================================================================

/// **真帧对拍**：编辑区路径与预览区代码块路径对**同一段代码**画出的
/// 颜色序列必须**逐 span 相同**。
///
/// # 为什么不是「两侧同一段 Markdown」
///
/// 方案文档的原始判据写的是「预览区与编辑区**同一段代码**的语法色
/// 逐 span 相同」。实测（2026-10-20，探针）：
///
/// - 编辑区对**全文**用 `lang = "markdown"` 高亮 → syntect 的 markdown
///   语法只把 ``` 围栏**标记本身**标成 `Code`，**围栏内的代码一个字
///   都不上色**（实测：`"```rust\nfn main() {...}\n```"` 只产出 2 个 span，
///   分别是 0..3 与 43..47 的围栏标记）；
/// - 预览区代码块用**围栏自己的语言**（如 `rust`）对**块内文本**单独
///   高亮 → 产出 6 个 span（`Keyword` / `Function` / …）。
///
/// 也就是说：**两侧的语法来源本就不同**，「同一段代码两侧语法色相同」
/// 在当前架构下**不可能达成**——它要的是「编辑区也按围栏语言高亮围栏
/// 内的代码」，那是**另一个改造项**（编辑区多语言高亮），不属于 R-10
/// （R-10 只统一「带样式的一段文本 → `LayoutJob`」的**构造器**）。
///
/// 故本条断言**可达成的等价命题**：
/// **同一段代码 + 同一套 span**，分别经
/// 「编辑区路径」（`build_line_job`，边界并集）与
/// 「预览区代码块路径」（`highlighted_code_job`，顺序游标）
/// 渲染，两侧画出的**颜色序列逐 span 相同**。
///
/// 这才是 R-10 的核心保证：**切段策略不同，但同一组 span 的配色必须一致**
/// ——否则用户会看到「同一段代码在编辑区一个色、在预览区另一个色」。
///
/// # 判据形态
///
/// 真帧（`ctx.run_ui` → `FullOutput::shapes` → `Shape::Text`），
/// 两侧各画一次 → 从画出的 `Shape::Text` 里取 `galley.job.sections` 的颜色。
/// **不**直接断言 `LayoutJob`——那只是中间产物；真帧才证明「画到屏幕上
/// 的颜色」一致。
///
/// 字号**允许不同**（编辑区 14px vs 预览区 15px，是刻意设计），
/// 故只比**颜色**。
///
/// # 为什么**背景色也要比**
///
/// 只比前景色会漏掉「查找命中背景叠加」这类差异：变异验证实测，
/// 给预览区路径的 span 段叠加一个背景色后，本用例**仍绿**——
/// 背景不同但前景色相同，肉眼看是「同一行代码有一段底色不同」，
/// 一样是用户可见的漂移。故前景与背景**都比**。
#[test]
fn real_frame_editor_and_preview_code_paths_paint_same_colors() {
    let theme = Theme::dark();
    // 一段有多种语法的代码：`fn` / `main` / `let` / `=` / `1` / 注释。
    let code = "fn main() { let x = 1; // hi\n}";
    let spans = vec![
        SpanView {
            range: RangeView::new(0, 2),
            style: "keyword".to_string(),
            url: None,
        },
        SpanView {
            range: RangeView::new(3, 7),
            style: "function".to_string(),
            url: None,
        },
        SpanView {
            range: RangeView::new(10, 13),
            style: "keyword".to_string(),
            url: None,
        },
        SpanView {
            range: RangeView::new(16, 17),
            style: "operator".to_string(),
            url: None,
        },
        SpanView {
            range: RangeView::new(18, 19),
            style: "number".to_string(),
            url: None,
        },
        SpanView {
            range: RangeView::new(21, 25),
            style: "comment".to_string(),
            url: None,
        },
    ];
    // 前提：span 区间必须落在字符边界上，否则会被吸附 → 段数变化，
    // 颜色序列不可比。
    for s in &spans {
        assert!(
            code.is_char_boundary(s.range.start) && code.is_char_boundary(s.range.end),
            "用例前提错误：span {:?} 不是字符边界",
            s.range
        );
    }

    // 两侧各画一次，收集画出的 `Shape::Text` 的颜色序列。
    let ctx = egui::Context::default();
    ctx.run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();

    let mut painted: Vec<Vec<(String, egui::Color32, egui::Color32)>> = Vec::new();
    for which in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 400.0),
            )),
            ..Default::default()
        };
        let job = if which == 0 {
            // 编辑区路径：边界并集 + 编辑区样式表。
            crate::presentation::theme::mapper::build_line_job(&theme, code, &spans, &[], 0.0)
        } else {
            // 预览区代码块路径：顺序游标 + 预览区等宽字体。
            highlighted_code_job(&theme, code, Some(&spans), f32::INFINITY)
        };
        let mut out = ctx.run_ui(input, |ui| {
            ui.add(egui::Label::new(std::sync::Arc::new(job.clone())));
        });
        // egui 0.37 起 `FullOutput` 携带未应用的 `TexturesDelta`，其 `Drop`
        // 会在非空即 panic（无头环境没有渲染后端去 apply）。
        out.textures_delta.clear();
        let seq: Vec<(String, egui::Color32, egui::Color32)> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t),
                _ => None,
            })
            .filter(|t| t.galley.text().contains("fn main"))
            .flat_map(|t| {
                t.galley.job.sections.iter().map(move |s| {
                    let txt = t
                        .galley
                        .job
                        .text
                        .get(s.byte_range.start.0..s.byte_range.end.0)
                        .unwrap_or("")
                        .to_string();
                    (txt, s.format.color, s.format.background)
                })
            })
            .collect();
        painted.push(seq);
    }

    let editor_seq = &painted[0];
    let preview_seq = &painted[1];

    assert!(
        !editor_seq.is_empty() && !preview_seq.is_empty(),
        "两侧都必须真的画出文本（editor={editor_seq:?} preview={preview_seq:?}）"
    );
    // 前提：两侧段数必须一致——段数不同说明切段策略在这组 span 上
    // 分歧，颜色序列逐项比对就没有意义了。
    assert_eq!(
        editor_seq.len(),
        preview_seq.len(),
        "两侧段数应一致（editor={editor_seq:?} preview={preview_seq:?}）"
    );
    // 前提：颜色确实有多种（若全是同一个颜色，逐项比对恒真）。
    let distinct: std::collections::HashSet<_> = editor_seq.iter().map(|(_, c, _)| *c).collect();
    assert!(
        distinct.len() >= 3,
        "用例前提：颜色种类应 ≥3（实测 {} 种）",
        distinct.len()
    );

    for (i, ((et, ec, eb), (pt, pc, pb))) in editor_seq.iter().zip(preview_seq.iter()).enumerate() {
        assert_eq!(
            et, pt,
            "第 {i} 段的文本不同：编辑区 {et:?} vs 预览区 {pt:?}"
        );
        assert_eq!(
            ec, pc,
            "第 {i} 段（{et:?}）前景色不同：编辑区 {ec:?} vs 预览区 {pc:?}"
        );
        assert_eq!(
            eb, pb,
            "第 {i} 段（{et:?}）背景色不同：编辑区 {eb:?} vs 预览区 {pb:?}"
        );
    }
}
