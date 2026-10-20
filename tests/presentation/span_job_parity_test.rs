//! R-10 对拍用例：统一构造器 vs 三套旧实现。
//!
//! # 这份用例在防什么
//!
//! R-10 把「带样式的一段文本 → `LayoutJob`」的三套独立实现收敛成一个
//! （[`build_job`](/rustmark/presentation/theme/mapper/fn.build_job.html)）。
//! 收敛本身有**真实的回归风险**：三套实现原本的边界处理、样式表、附加行为
//! 各不相同，收敛后任何一处「顺手统一」都可能悄悄改变用户可见效果。
//!
//! 方案文档要求的判据是「差异仅允许出现在重叠 span 的背景层」。但**实测**
//! 发现的差异不止那一条（见下），故本文件分成两类用例：
//!
//! 1. **等价用例**（[`editor_union_matches_legacy_implementation`] 等）：
//!    把 git HEAD 的旧实现**原文抄进本文件**当 oracle，与新实现逐项比对
//!    （每段文本 / 颜色 / 背景 / 字号 / 字体族 / 粗斜体下划线）。这些用例
//!    锁住「本该不变的地方真的没变」。
//!
//! 2. **有意分歧用例**（[`code_block_base_segment_uses_preview_font`] 等）：
//!    实测确认新实现修掉了旧实现的三个缺陷。这些**不是**「等价」，而是
//!    「旧实现本来就不对」——故用独立用例把**新行为**钉死，并在注释里
//!    写清旧行为错在哪、为什么必须变。
//!
//! # 为什么旧实现要抄进来而不是删掉
//!
//! R-10 方案原文写的是「保留旧实现并行一个版本做对拍，确认等价后再删」。
//! 本文件就是那个「并行版本」。删掉它，就再没有东西能证明「收敛没改效果」——
//! 剩下的只有「新实现自证一致」，那证明不了任何事。
//!
//! # 实测到的三处分歧（2026-10-20）
//!
//! | 位置 | 旧行为 | 新行为 | 判定 |
//! |---|---|---|---|
//! | 代码块 base 段字号 | 编辑区字号（14） | 预览字号（15） | **修复**：旧实现漏覆盖 `font_id`，同一行代码出现两种字号 |
//! | 代码块 base 段切分 | base 段与后继 base 段分列两段 | 合成一段 | 上一项的连带结果 |
//! | 代码块 重叠 span 尾部 | `"defgh"`（`de` 重复） | `"fgh"` | **修复**：旧实现文本重复，8 字符产出 10 字符 |
//!
//! 编辑区（边界并集）路径实测**完全一致**（7 个用例），行内路径亦一致。

use egui::FontId;
use egui::text::{LayoutJob, TextWrapping};

use rustmark::application::dto::{RangeView, SpanView};
use rustmark::presentation::theme::builtin::Theme;
use rustmark::presentation::theme::mapper::{SpanPolicy, StyleMapper, build_job};
use rustmark::shared::geometry::{clamp_to_char_boundaries, floor_char_boundary};

// ============================================================================
// Oracle：git HEAD 的三套旧实现（原文抄录，只读不改）
// ============================================================================

/// 旧 `build_line_job`（边界并集语义）——编辑区那一侧。
///
/// 抄自 `67210f2^:src/presentation/theme/mapper/highlight_cache.rs`。
fn legacy_build_line_job(
    theme: &Theme,
    line: &str,
    spans: &[SpanView],
    bg_marks: &[(usize, usize, bool)],
    wrap_width: f32,
) -> LayoutJob {
    let mapper = StyleMapper::new(theme);
    let base = mapper.format("plain");
    let mut job = egui::text::LayoutJob {
        wrap: TextWrapping {
            max_width: if wrap_width > 0.0 {
                wrap_width
            } else {
                f32::INFINITY
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let snap = |b: usize| floor_char_boundary(line, b);
    let mut bounds: Vec<usize> = vec![0, line.len()];
    for s in spans {
        bounds.push(snap(s.range.start));
        bounds.push(snap(s.range.end));
    }
    for &(s, e, _) in bg_marks {
        bounds.push(snap(s));
        bounds.push(snap(e));
    }
    bounds.sort_unstable();
    bounds.dedup();

    for pair in bounds.windows(2) {
        let lo = pair[0];
        let hi = pair[1];
        if lo >= hi {
            continue;
        }
        let mut fmt = spans
            .iter()
            .find(|s| s.range.start <= lo && lo < s.range.end)
            .map(|s| mapper.format(&s.style))
            .unwrap_or_else(|| base.clone());
        if let Some(&(_, _, current)) = bg_marks.iter().find(|&&(s, e, _)| s <= lo && lo < e) {
            fmt.background = if current {
                theme.find_current_bg
            } else {
                theme.find_bg
            };
        }
        job.append(&line[lo..hi], 0.0, fmt);
    }
    if job.sections.is_empty() {
        job.append(line, 0.0, base);
    }
    job
}

/// 旧 `highlighted_code_job`（顺序游标语义）——预览区代码块那一侧。
///
/// 抄自 `67210f2^:src/presentation/views/preview_view/code.rs`。
fn legacy_highlighted_code_job(
    theme: &Theme,
    text: &str,
    spans: Option<&[SpanView]>,
    wrap_width: f32,
) -> LayoutJob {
    let mapper = StyleMapper::new(theme);
    let base = mapper.format("plain");
    let font = FontId::monospace(theme.metrics.preview_font_size);

    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = if wrap_width > 0.0 {
        wrap_width
    } else {
        f32::INFINITY
    };
    let mut cursor = 0usize;
    if let Some(spans) = spans {
        let mut sorted: Vec<_> = spans.to_vec();
        sorted.sort_by_key(|s| s.range.start);
        for span in &sorted {
            let Some((start, end)) =
                clamp_to_char_boundaries(text, span.range.start, span.range.end)
            else {
                continue;
            };
            if start > cursor {
                job.append(&text[cursor..start], 0.0, base.clone());
            }
            let mut fmt = mapper.format(&span.style);
            fmt.font_id = font.clone();
            job.append(&text[start..end], 0.0, fmt);
            cursor = end;
        }
    }
    if cursor < text.len() {
        let mut fmt = base;
        fmt.font_id = font;
        job.append(&text[cursor..], 0.0, fmt);
    }
    job
}

// ============================================================================
// 比对工具
// ============================================================================

/// `LayoutJob` 的可比指纹：每段的文本 + 全部可见样式。
///
/// **必须**包含字号与字体族——只比颜色会漏掉「同一行两种字号」这类缺陷
/// （旧代码块的 base 段正是这样错的）。
fn fingerprint(job: &LayoutJob) -> Vec<String> {
    job.sections
        .iter()
        .map(|s| {
            let t = job
                .text
                .get(s.byte_range.start.0..s.byte_range.end.0)
                .unwrap_or("<越界>");
            format!(
                "text={t:?} color={:?} bg={:?} font={:?} size={} ital={} strike={} ul={}",
                s.format.color,
                s.format.background,
                s.format.font_id.family,
                s.format.font_id.size,
                s.format.italics,
                s.format.strikethrough.width > 0.0,
                s.format.underline.width > 0.0,
            )
        })
        .collect()
}

/// `LayoutJob` 拼出的全文。
fn job_text(job: &LayoutJob) -> String {
    job.sections
        .iter()
        .map(|s| {
            job.text
                .get(s.byte_range.start.0..s.byte_range.end.0)
                .unwrap_or("")
        })
        .collect()
}

fn span(start: usize, end: usize, style: &str) -> SpanView {
    SpanView {
        range: RangeView::new(start, end),
        style: style.to_string(),
        url: None,
    }
}

/// 新代码块那一侧的构造（与 `code.rs` 现在的调用一致）。
fn new_code_job(theme: &Theme, text: &str, spans: &[SpanView], wrap_width: f32) -> LayoutJob {
    let mapper = StyleMapper::new(theme);
    let font = FontId::monospace(theme.metrics.preview_font_size);
    let mut base = mapper.format("plain");
    base.font_id = font.clone();
    let style_of = move |s: &str| {
        let mut f = mapper.format(s);
        f.font_id = font.clone();
        f
    };
    let mut sorted = spans.to_vec();
    sorted.sort_by_key(|s| s.range.start);
    let policy = SpanPolicy::cursor(base, wrap_width, style_of, None);
    let (job, _) = build_job(theme, text, &sorted, &policy, |_| false);
    job
}

/// 新编辑区那一侧的构造（与 `highlight_cache.rs` 现在的调用一致）。
fn new_editor_job(
    theme: &Theme,
    line: &str,
    spans: &[SpanView],
    marks: &[(usize, usize, bool)],
    wrap_width: f32,
) -> LayoutJob {
    let mapper = StyleMapper::new(theme);
    let base = mapper.format("plain");
    let policy = SpanPolicy::union(base, wrap_width, |s| mapper.format(s), marks);
    let (job, _) = build_job(theme, line, spans, &policy, |_| false);
    job
}

// ============================================================================
// 一、编辑区（边界并集）——实测完全等价
// ============================================================================

/// 编辑区（边界并集）路径：新实现必须与旧实现**逐段等价**。
///
/// 覆盖：无 span / 单 span / span + 查找命中 / 命中落在 span 内 /
/// 重叠 span / 多字节字符 / 空行。
#[test]
fn editor_union_matches_legacy_implementation() {
    let theme = Theme::dark();
    let line = "中文abc def";
    // 类型别名：`Vec<(&str, Vec<SpanView>, Vec<(usize, usize, bool)>)>` 触发
    // clippy 的 `type_complexity`（嵌套元组 + 嵌套 Vec）。
    type Case = (&'static str, Vec<SpanView>, Vec<(usize, usize, bool)>);
    let cases: Vec<Case> = vec![
        ("无 span", vec![], vec![]),
        ("单 span", vec![span(0, 5, "keyword")], vec![]),
        (
            "span + 查找命中",
            vec![span(0, 5, "keyword")],
            vec![(3, 8, false)],
        ),
        (
            "查找命中在 span 内",
            vec![span(0, 10, "string")],
            vec![(2, 5, true)],
        ),
        (
            "span 起点落在字符中间",
            vec![span(0, 8, "keyword"), span(5, 12, "string")],
            vec![],
        ),
        (
            // **真重叠**：两个 span 的起点都落在**字符边界**上，于是同一段
            // 文字同时被两者覆盖。
            //
            // 上一条「span 起点落在字符中间」**测不到**重叠语义：并集的
            // **边界**按 `snap()`（吸附到字符边界）计算，而**覆盖判定**用的是
            // **原始** `range`。`span(5,12)` 的起点 5 落在「文」（3..6）中间
            // → 边界被吸附到 3，但判定仍按 5 起算 → `lo=3` 时它不匹配 →
            // 两个 span 从未同时命中同一段 → 「首个命中优先」与「后者覆盖前者」
            // 两种语义产出的颜色完全相同（实测：把 `find` 改成 `last`，
            // 全部用例仍绿）。
            //
            // 本条用 `span(3,12)`（起点 3 正是「文」的字符边界）修掉这个夹具
            // 缺陷——变异验证（`find` → `filter().last()`）现在会让本条变红。
            "真重叠（起点在字符边界）",
            vec![span(0, 8, "keyword"), span(3, 12, "string")],
            vec![],
        ),
        (
            "多字节",
            vec![span(0, 3, "keyword"), span(3, 6, "string")],
            vec![(1, 4, false)],
        ),
        ("空行", vec![], vec![]),
    ];

    for (name, spans, marks) in cases {
        let old = legacy_build_line_job(&theme, line, &spans, &marks, 0.0);
        let new = new_editor_job(&theme, line, &spans, &marks, 0.0);
        assert_eq!(
            fingerprint(&old),
            fingerprint(&new),
            "编辑区路径 [{name}] 与旧实现不等价"
        );
    }
}

/// 编辑区路径：空行必须仍产出一个空 section（否则行高为 0）。
///
/// 这条是旧实现就有的约束（V11-16 连续回车只显示一行的根因），
/// 收敛后不能丢。
#[test]
fn editor_union_keeps_empty_section_for_blank_line() {
    let theme = Theme::dark();
    let job = new_editor_job(&theme, "", &[], &[], 0.0);
    assert_eq!(
        job.sections.len(),
        1,
        "空行必须保留一个空 section，否则 egui 排成 0 高度"
    );
    assert_eq!(job_text(&job), "");
}

// ============================================================================
// 二、预览区代码块（顺序游标）——等价部分
// ============================================================================

/// 代码块路径：以下形态新旧实现**逐段等价**。
///
/// 覆盖：无 span / 相邻 span / 多字节 / 非字符边界。
///
/// 「有间隙」与「重叠 span」不在此列——那两格旧实现是错的，见第三节。
#[test]
fn code_block_matches_legacy_implementation_on_equal_cases() {
    let theme = Theme::dark();
    let cases: Vec<(&str, &str, Vec<SpanView>)> = vec![
        ("无 span", "let x = 1;", vec![]),
        (
            "相邻 span",
            "abcdef",
            vec![span(0, 3, "keyword"), span(3, 6, "string")],
        ),
        (
            "多字节",
            "中文abc",
            vec![span(0, 3, "keyword"), span(3, 6, "string")],
        ),
        ("非字符边界", "中文abc", vec![span(1, 5, "keyword")]),
    ];

    for (name, text, spans) in cases {
        let old = legacy_highlighted_code_job(&theme, text, Some(&spans), 0.0);
        let new = new_code_job(&theme, text, &spans, 0.0);
        assert_eq!(
            fingerprint(&old),
            fingerprint(&new),
            "代码块路径 [{name}] 与旧实现不等价"
        );
    }
}

// ============================================================================
// 三、预览区代码块——实测到的三处「有意分歧」（旧实现的缺陷）
// ============================================================================

/// **分歧 1（修复）**：代码块的 base 段必须用**预览字号**。
///
/// 旧实现只给 span 段与**尾部**覆盖了 `font_id = 预览等宽`，**中间的 base
/// 段漏了** → 同一行代码里，span 之间的空隙用编辑区字号（14）、其余用预览
/// 字号（15）。这是一眼可见的字号跳变。
///
/// 新实现把 `base` 一次性设成预览字号 → 全行一致。
#[test]
fn code_block_base_segment_uses_preview_font() {
    let theme = Theme::dark();
    let preview_size = theme.metrics.preview_font_size;
    let editor_size = theme.editor_font_size;

    // "let " 是 base 段（span 之前的空隙），"x" 是 span 段。
    let job = new_code_job(&theme, "let x = 1;", &[span(4, 5, "keyword")], 0.0);
    let sizes: Vec<f32> = job.sections.iter().map(|s| s.format.font_id.size).collect();

    assert!(
        sizes
            .iter()
            .all(|s| (*s - preview_size).abs() < f32::EPSILON),
        "代码块所有段都应是预览字号 {preview_size}，实测 {sizes:?}（编辑区字号是 {editor_size}）"
    );
    assert!(
        (preview_size - editor_size).abs() > f32::EPSILON,
        "用例前提：预览字号与编辑区字号必须不同，否则本用例测不出东西"
    );
}

/// **分歧 2（修复）**：代码块 base 段与后继 base 段应合成一段。
///
/// 旧实现里 base 段（14 号）与尾部 base 段（15 号）是两段，字号还不一样；
/// 合并后是一段（15 号），排版结果一致但少一个 section。
#[test]
fn code_block_merges_adjacent_base_segments() {
    let theme = Theme::dark();
    // span(0,3) 之后剩下 " x = 1;" 整段是 base。
    let job = new_code_job(&theme, "let x = 1;", &[span(0, 3, "keyword")], 0.0);
    assert_eq!(
        job.sections.len(),
        2,
        "「let」+「 x = 1;」应合成两段，实测 {} 段",
        job.sections.len()
    );
    assert_eq!(job_text(&job), "let x = 1;");
}

/// **分歧 3（修复，最严重）**：重叠 span **不得**导致文本重复。
///
/// 旧实现的 `cursor = end`（而非 `end.max(cursor)`）在 span 重叠时会让
/// 尾部 base 段从**更早**的位置开始 → 已渲染过的字符被输出第二遍。
///
/// 实测：原文 `"abcdefgh"`（8 字符），旧实现产出 `"abcdedefgh"`（**10 字符**）
/// —— 屏幕上会看到重复的 "de"。新实现产出 `"abcdefgh"`。
#[test]
fn code_block_does_not_duplicate_text_on_overlapping_spans() {
    let theme = Theme::dark();
    let text = "abcdefgh";
    let spans = vec![span(0, 5, "keyword"), span(3, 8, "string")];

    let old = legacy_highlighted_code_job(&theme, text, Some(&spans), 0.0);
    let new = new_code_job(&theme, text, &spans, 0.0);

    assert_eq!(
        job_text(&old),
        "abcdedefgh",
        "用例前提：旧实现确实会重复输出文本；若哪天旧实现被改好，本用例需同步更新"
    );
    assert_eq!(
        job_text(&new),
        text,
        "重叠 span 不得导致文本重复（实测拼接={:?}）",
        job_text(&new)
    )
}

/// **分歧 3 续**：span **完全被覆盖**时（`end` 倒退），同样不得重复。
///
/// 上一条覆盖的是「部分重叠」（span2 的 `end` 比 span1 大）→ 旧实现的
/// 尾部起点提前一点，重复 2 个字符。这条覆盖更极端的情形：**span2 完全
/// 落在 span1 内部** → 旧实现的 `cursor` 会从 6 **倒退**到 3，尾部段从 3
/// 开始 → 重复 5 个字符。
///
/// 实测：原文 8 字符 → 旧实现产出 **13 字符** `"abcdefbcdefgh"`。
///
/// 这条用例的存在理由：上一条**测不到** `cursor = end.max(cursor)` 这个
/// 钳位——因为「部分重叠」时 `end` 本来就在增长，去掉 `max` 看不出差别。
/// 变异验证（把 `cursor = end.max(cursor)` 改成 `cursor = end`）只有本条会红。
#[test]
fn code_block_does_not_duplicate_text_when_span_is_fully_covered() {
    let theme = Theme::dark();
    let text = "abcdefgh";
    // span2 完全落在 span1 内部 → `end` 倒退（6 → 3）。
    let spans = vec![span(0, 6, "keyword"), span(1, 3, "string")];

    let old = legacy_highlighted_code_job(&theme, text, Some(&spans), 0.0);
    let new = new_code_job(&theme, text, &spans, 0.0);

    assert_eq!(
        job_text(&old),
        "abcdefbcdefgh",
        "用例前提：旧实现在 span 完全被覆盖时确实会大量重复文本"
    );
    assert_eq!(
        job_text(&new),
        text,
        "span 被完全覆盖时不得重复文本（实测拼接={:?}）",
        job_text(&new)
    );
}

/// 拼接文本必须与原文**逐字节相同**（通用不变量）。
///
/// 这条比任何逐段比对都强：无论切段怎么变，屏幕上显示的字符必须一字不差。
#[test]
fn job_text_always_equals_source_text() {
    let theme = Theme::dark();
    let cases: Vec<(&str, Vec<SpanView>)> = vec![
        ("let x = 1;", vec![span(4, 5, "keyword")]),
        ("let x = 1;", vec![span(0, 3, "keyword")]),
        (
            "abcdefgh",
            vec![span(0, 5, "keyword"), span(3, 8, "string")],
        ),
        ("ab  cd", vec![span(0, 2, "keyword"), span(4, 6, "string")]),
        ("中文abc", vec![span(0, 3, "keyword"), span(3, 6, "string")]),
    ];

    for (text, spans) in cases {
        let new = new_code_job(&theme, text, &spans, 0.0);
        assert_eq!(
            job_text(&new),
            text,
            "拼接文本必须与原文逐字节相同（text={text:?}）"
        );
    }
}

// ============================================================================
// 四、折行参数
// ============================================================================

/// 折行宽度 ≤ 0 时**不折行**（单行长排），不得退化成「每字一行」。
///
/// epaint 的 `max_width = 0.0` 会让每个字符各占一行（实测
/// `"Hello world"` → 11 行），与「不折行」完全不是一回事。
///
/// 旧 inline 用 `wrap_width.max(0.0)`（0.0 → 0.0 → 每字一行），
/// 旧 code.rs 用 `if > 0 { w } else { INFINITY }`（0.0 → INFINITY）——
/// **两个旧实现本身就不一致**。统一后取 code.rs 的口径（不折行）。
#[test]
fn zero_wrap_width_means_no_wrapping() {
    let theme = Theme::dark();
    let job = new_code_job(&theme, "Hello world", &[], 0.0);
    assert!(
        job.wrap.max_width.is_infinite(),
        "折行宽度 ≤ 0 应退化为不折行（INFINITY），实测 {}",
        job.wrap.max_width
    );
}

/// 折行宽度为正时必须原样传给 epaint。
#[test]
fn positive_wrap_width_is_passed_through() {
    let theme = Theme::dark();
    let job = new_code_job(&theme, "Hello world", &[], 120.0);
    assert_eq!(job.wrap.max_width, 120.0);
}

// ============================================================================
// 五、守卫：新增一种 span 样式只改一处
// ============================================================================

/// M2 里程碑判据「新增 X 只改 N 处」：新增一个样式名，只需在**样式表**里
/// 加一个分支，**切段逻辑**不应随之改动。
///
/// 本用例的形态：把「样式表」换成一个返回**与 base 完全相同**格式的假实现，
/// 断言切段产出的**文本**完全不变——即切段逻辑对样式名**无感**。
///
/// 注意不能断言「段数」：egui 的 `LayoutJob::append` 会**合并相邻同格式
/// section**（`epaint` `text_layout_types.rs` 的 `append`），所以样式表一改，
/// 段数就会变——那是 epaint 的行为，不是切段逻辑的。用「文本不变」做判据
/// 才抓得住真正的耦合：若有人把样式判断写进切段逻辑（例如「`code` 样式
/// 要剥掉背景」），本用例会立刻变红。
#[test]
fn segmentation_is_independent_of_style_names() {
    let theme = Theme::dark();
    let text = "let x = 1;";
    let spans = vec![span(0, 3, "keyword"), span(4, 5, "identifier")];

    let mapper = StyleMapper::new(&theme);
    let font = FontId::monospace(theme.metrics.preview_font_size);
    let mut base = mapper.format("plain");
    base.font_id = font.clone();

    // 假样式表：无论叫什么名字，都返回与 base **完全相同**的格式。
    let style_of = move |_s: &str| {
        let mut f = mapper.format("plain");
        f.font_id = font.clone();
        f
    };
    let mut sorted = spans.clone();
    sorted.sort_by_key(|s| s.range.start);
    let policy = SpanPolicy::cursor(base, 0.0, style_of, None);
    let (job, _) = build_job(&theme, text, &sorted, &policy, |_| false);

    // 样式名换了，切出来的**文本**必须一字不差。
    assert_eq!(
        job_text(&job),
        text,
        "切段逻辑不得依赖样式名（样式表换了，文本却变了）"
    );
}

/// **分歧 4**：base 段（span 未覆盖的文本）必须用 **base 样式**，
/// 不能借用 span 的样式表。
///
/// 两条路径的样式表**不同**：
/// - 编辑区（`mapper.format`）：base = `format("plain")`，span = `format(span.style)`；
/// - 行内（`span_format`）：base = `span_format(base_style)`，span = `span_format(span.style)`。
///
/// 若把 base 段也走 span 样式表，则 base_style 指定的基准样式（标题色 /
/// 粗体 / 引用斜体…）会**丢失**——行内基准是 `"heading"` / `"strong"` /
/// `"quote"` 时尤其明显。
///
/// 判据形态：在**顺序游标**路径上，用一个「span 样式表返回固定颜色」的
/// 假实现 + 一个**明显不同**的 base 颜色，断言 base 段的颜色是 base 的
/// 那个。若 base 段被换成 span 样式表 → 颜色变成 span 的那个 → 用例变红。
#[test]
fn base_segment_uses_base_style_not_span_style_table() {
    let theme = Theme::dark();
    let text = "abc def";
    // span 盖住 "abc"，" def" 是 base 段。
    let spans = vec![span(0, 3, "keyword")];

    let mapper = StyleMapper::new(&theme);
    // base 用「正文主色」，span 样式表一律返回「标题色」——两者必须不同。
    let base_color = theme.text_primary;
    let span_color = theme.syn_heading;
    assert_ne!(
        base_color, span_color,
        "用例前提：base 色与 span 色必须不同，否则测不出 base 段是否走错样式表"
    );

    let mut base = mapper.format("plain");
    base.color = base_color;
    let style_of = move |_s: &str| {
        let mut f = mapper.format("plain");
        f.color = span_color;
        f
    };
    let policy = SpanPolicy::cursor(base, 0.0, style_of, None);
    let (job, _) = build_job(&theme, text, &spans, &policy, |_| false);

    let base_seg_color = job
        .sections
        .iter()
        .find(|s| {
            job.text
                .get(s.byte_range.start.0..s.byte_range.end.0)
                .is_some_and(|t| t.contains(" def"))
        })
        .map(|s| s.format.color)
        .expect("应存在 base 段");
    assert_eq!(
        base_seg_color, base_color,
        "base 段必须用 base 样式（正文主色），不得走 span 样式表"
    );
}

/// 反向对照：段数随格式而变（egui 合并相邻同格式 section）。
///
/// 上一条用例的「为什么不断言段数」的注释需要事实支撑：段数确实随格式
/// 是否相同而变（egui 合并相邻同格式 section）。本用例把这个事实钉住。
#[test]
fn section_count_depends_on_format_not_on_style_names() {
    let theme = Theme::dark();
    let text = "let x = 1;";
    let spans = vec![span(0, 3, "keyword"), span(4, 5, "identifier")];

    let mapper = StyleMapper::new(&theme);
    let font = FontId::monospace(theme.metrics.preview_font_size);
    let mut base = mapper.format("plain");
    base.font_id = font.clone();

    // 假样式表：与 base 相同 → 全行一段。
    let font_same = font.clone();
    let same = move |_s: &str| {
        let mut f = mapper.format("plain");
        f.font_id = font_same.clone();
        f
    };
    let mut sorted = spans.clone();
    sorted.sort_by_key(|s| s.range.start);
    let policy_same = SpanPolicy::cursor(base.clone(), 0.0, same, None);
    let (job_same, _) = build_job(&theme, text, &sorted, &policy_same, |_| false);

    // 真样式表：span 段颜色不同 → 多段。
    let real = move |s: &str| {
        let mut f = mapper.format(s);
        f.font_id = font.clone();
        f
    };
    let policy_real = SpanPolicy::cursor(base, 0.0, real, None);
    let (job_real, _) = build_job(&theme, text, &sorted, &policy_real, |_| false);

    assert_eq!(
        job_text(&job_same),
        job_text(&job_real),
        "样式表不同不影响文本"
    );
    assert!(
        job_same.sections.len() < job_real.sections.len(),
        "格式不同 → egui 不合并 → 段数应变多：同格式 {} 段 vs 不同格式 {} 段",
        job_same.sections.len(),
        job_real.sections.len()
    );
}
