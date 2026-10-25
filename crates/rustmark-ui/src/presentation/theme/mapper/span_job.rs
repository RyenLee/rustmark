//! span → [`LayoutJob`] 的**唯一**构造器（R-10）。
//!
//! # 为什么要有这个模块
//!
//! 「把带样式的一段文本排成 `LayoutJob`」这件事，本项目原先有**三套**
//! 独立实现：
//!
//! | 实现 | 位置 | 场景 |
//! |---|---|---|
//! | `build_line_job` | `mapper/galley_cache.rs` | 编辑区逐行排版 |
//! | `highlighted_code_job` | `views/preview_view/code.rs` | 预览区代码块 |
//! | `build_text_job` | `views/preview_view/inline.rs` | 预览区行内富文本 |
//!
//! 三套实现各自演化出不同的边界处理、不同的样式表、不同的附加行为，
//! 于是**同一个 Markdown 在编辑区与预览区的颜色会漂移**——
//! 方案文档说「用户可见效果应完全不变」，但现状**本就已经漂了**。
//!
//! 本模块把「**怎么切段**」收敛到一处，三处差异用**显式参数**
//! （[`SpanPolicy`]）表达，**不复制实现**。
//!
//! # 本模块管什么、不管什么
//!
//! **管**：文本怎么被切成若干 section、每段取哪段字节、边界怎么防护。
//!
//! **不管**：某个样式名对应什么颜色 / 字号 / 字体——那是**样式表**的事，
//! 由各调用方通过 [`SpanPolicy::style_of`] 闭包提供。原因是三处的
//! 样式表**确实不同**（见下）。
//!
//! # 为什么样式表**不**合并成一张
//!
//! 方案文档第 3 条说「`inline.rs` 的 `span_format` 并入 `mapper.rs` 的
//! `format`」。实测**不成立**：
//!
//! - **字号体系不同**：`mapper.format` 用 `theme.editor_font_size`
//!   （14px，用户可配置），`span_format` 用预览字号族（15px，固定）——
//!   合并会让「预览字号跟随编辑区设置漂移」（`span_format` 的文档注释
//!   明确说这是**刻意**的）；
//! - **覆盖面几乎不重叠**：`mapper.format` 覆盖 26 个样式名，其中 19 个
//!   是**代码语法**（`keyword` / `string` / `comment` / …）与编辑区特有
//!   （`plain` / `list_marker` / `table_border` / `task_marker` / `image`）；
//!   `span_format` 覆盖 14 个，其中 7 个是**行内富文本**特有
//!   （`math` / `footnote_ref` / `keycap` / `mark` / `sub` / `sup` /
//!   `task_done`）。**共同**的只有 7 个。
//!
//! 而那 7 个**共同**样式的**颜色**本来就取自**同一批 `theme.syn_*` 字段**
//! ——**没有重复可合**。唯一的实际差异是**线条粗细**
//! （编辑区用 `hairline`，预览区用 `Stroke::new(1.0)`），
//! 那也是**刻意**的（编辑区是源码视图，hairline 更细）。
//!
//! 故本模块**不**合并样式表。

use egui::text::{LayoutJob, TextFormat, TextWrapping};

use rustmark_core::application::dto::SpanView;
use rustmark_core::shared::geometry::{clamp_to_char_boundaries, floor_char_boundary};

use crate::presentation::theme::builtin::Theme;

/// 切段策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Segmentation {
    /// 顺序游标：按 span 起点顺序推进，span 之间的空隙走基础样式。
    ///
    /// 适用于「span 不重叠」的场景（行内富文本、代码块）。
    Cursor,
    /// 边界**并集**：取所有 span 与查找命中的边界并集，逐段决定样式。
    ///
    /// 能表达**重叠** span（查找命中背景叠加在语法高亮之上），
    /// 顺序游标做不到。
    Union,
}

/// 字符边界防护策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoundaryGuard {
    /// 向下吸附到字符边界（`floor_char_boundary`）。
    ///
    /// 最坏只是某一段少上色，**绝不崩**。
    Floor,
    /// `clamp_to_char_boundaries`：越界则收缩，落在字符中间则吸附。
    Clamp,
}

/// 文本改写器：由样式名与原文产出改写后的文本。
///
/// 抽成别名不是为了好看——`Option<&'a dyn Fn(&str, &str) -> String>`
/// 这个形状在结构体字段与两个构造器里各写一遍，clippy 的 `type_complexity`
/// 会在**每处**报一遍；别名让「改写器是什么」只有一个真源。
pub(crate) type Rewrite<'a> = &'a dyn Fn(&str, &str) -> String;

/// 一次排版的**上下文**：样式怎么查、怎么切段、文本要不要改写。
///
/// 三个实现各自提供一份 [`SpanPolicy`]，差异**全部**在这里显式表达。
pub struct SpanPolicy<'a, F> {
    /// 切段策略。
    pub(crate) segmentation: Segmentation,
    /// 字符边界防护策略。
    pub(crate) boundary: BoundaryGuard,
    /// 基础样式（无 span 覆盖时的样式）。
    pub(crate) base: TextFormat,
    /// 折行宽度（≤ 0 = 不折行）。
    pub(crate) wrap_width: f32,
    /// 由样式名求出该 span 的样式。
    ///
    /// 用**闭包**而非枚举：不同场景的样式表覆盖面不同
    /// （编辑区 26 种 / 预览行内 14 种），
    /// 硬编码成枚举反而要造一个「两者并集」的庞类型。
    pub(crate) style_of: F,
    /// 行内公式 / 脚注引用的**文本改写**（返回改写后的字符串）。
    ///
    /// `None` = 不改写（编辑区 / 代码块）。
    pub(crate) rewrite: Option<Rewrite<'a>>,
    /// 查找命中背景区间 `(start, end, is_current)`。
    ///
    /// 仅 [`Segmentation::Union`] 用得上——顺序游标无法表达背景叠加。
    pub(crate) find_marks: &'a [(usize, usize, bool)],
}

impl<'a, F> SpanPolicy<'a, F>
where
    F: Fn(&str) -> TextFormat,
{
    /// 顺序游标策略（span 不重叠）。
    pub fn cursor(
        base: TextFormat,
        wrap_width: f32,
        style_of: F,
        rewrite: Option<Rewrite<'a>>,
    ) -> Self {
        Self {
            segmentation: Segmentation::Cursor,
            boundary: BoundaryGuard::Clamp,
            base,
            wrap_width,
            style_of,
            rewrite,
            find_marks: &[],
        }
    }

    /// 边界并集策略（可叠加查找命中背景）。
    pub fn union(
        base: TextFormat,
        wrap_width: f32,
        style_of: F,
        find_marks: &'a [(usize, usize, bool)],
    ) -> Self {
        Self {
            segmentation: Segmentation::Union,
            boundary: BoundaryGuard::Floor,
            base,
            wrap_width,
            style_of,
            rewrite: None,
            find_marks,
        }
    }
}

/// 折行参数。
///
/// 宽度 ≤ 0 时退化为**不折行**（单行长排）——三个实现原本各写一遍
/// 「`if width > 0.0 { width } else { f32::INFINITY }`」。
fn wrap_of(width: f32) -> TextWrapping {
    TextWrapping {
        max_width: if width > 0.0 { width } else { f32::INFINITY },
        ..Default::default()
    }
}

/// 空 `LayoutJob`（`text` 必须留空）。
///
/// `text` 预填会导致 egui 0.37 的 `debug_sanity_check` 失败——
/// 它要求各 `section` 从字节 0 起、无缝隙地铺满 `job.text`。
fn empty_job(wrap_width: f32) -> LayoutJob {
    LayoutJob {
        wrap: wrap_of(wrap_width),
        ..Default::default()
    }
}

/// 一个需要交互的片段的命中区间。
///
/// `idx` = span 在输入列表中的序号；`c0` / `c1` = **字符**区间
/// （不是字节区间——点击命中是按字符算的）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpanHit {
    /// span 序号。
    pub idx: usize,
    /// 起始字符下标。
    pub c0: usize,
    /// 结束字符下标（不含）。
    pub c1: usize,
}

/// 构造 `LayoutJob`。
///
/// 按 [`SpanPolicy::segmentation`] 分派到两种切段实现。
///
/// `on_hit` 判定某 span 是否需要记录命中区间（行内富文本的
/// 链接 / 脚注引用靠它做点击命中）；**仅**顺序游标策略会调用它。
pub fn build_job(
    theme: &Theme,
    content: &str,
    spans: &[SpanView],
    policy: &SpanPolicy<'_, impl Fn(&str) -> TextFormat>,
    on_hit: impl Fn(&str) -> bool,
) -> (LayoutJob, Vec<SpanHit>) {
    match policy.segmentation {
        Segmentation::Cursor => build_cursor_job(theme, content, spans, policy, on_hit),
        Segmentation::Union => (build_union_job(theme, content, spans, policy), Vec::new()),
    }
}

/// 顺序游标切段：按 span 起点顺序推进。
fn build_cursor_job(
    _theme: &Theme,
    content: &str,
    spans: &[SpanView],
    policy: &SpanPolicy<'_, impl Fn(&str) -> TextFormat>,
    on_hit: impl Fn(&str) -> bool,
) -> (LayoutJob, Vec<SpanHit>) {
    let mut job = empty_job(policy.wrap_width);
    let mut hits: Vec<SpanHit> = Vec::new();
    let len = content.len();
    let mut cursor = 0usize;
    let mut char_cursor = 0usize;

    if spans.is_empty() {
        job.append(content, 0.0, policy.base.clone());
        return (job, hits);
    }

    for (idx, span) in spans.iter().enumerate() {
        // 越界 / 落在多字节字符中间的区间会被吸附或退化为空——跳过而不是崩。
        let Some((start, end)) = guard_range(
            content,
            span.range.start.min(len),
            span.range.end.min(len),
            policy.boundary,
        ) else {
            continue;
        };
        // **三行钳位共同保证 `cursor` 单调不减**：
        // `start >= cursor` → `end >= start >= cursor` → `cursor = end` 不会倒退。
        //
        // 旧 `code.rs` 缺这组钳位（`cursor = end` 直接赋值），实测在 span
        // 重叠时**文本重复**：原文 `"abcdefgh"`（8 字符）被输出成
        // `"abcdedefgh"`（10 字符）——屏幕上多出一段重复的 "de"。
        //
        // `end.max(cursor)` 这一个 `max` 当前**无作用**（`end >= cursor`
        // 已由上面两行保证，实测：把它改成 `end` 后全部用例仍绿）。
        // 保留它是给「将来有人删掉 `end.max(start)`」留的保险——那时
        // `cursor` 就可能倒退、文本就会重复。三行是一组，拆开看会误以为
        // `cursor = end` 本身是安全的。
        let start = start.max(cursor);
        let end = end.max(start);
        if start > cursor {
            let base_slice = &content[cursor..start];
            job.append(base_slice, 0.0, policy.base.clone());
            char_cursor += base_slice.chars().count();
        }
        if end > start {
            let fmt = (policy.style_of)(&span.style);
            let raw = &content[start..end];
            // 文本改写（math 分隔符剥离 / 脚注标号）：`math` 与
            // `footnote_ref` 的**语法标记**不该出现在成品里。
            let slice = match policy.rewrite {
                Some(rewrite) => rewrite(raw, &span.style),
                None => raw.to_string(),
            };
            let slice_chars = slice.chars().count();
            job.append(&slice, 0.0, fmt);
            if on_hit(&span.style) {
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
        job.append(&content[cursor..], 0.0, policy.base.clone());
    }
    (job, hits)
}

/// 边界并集切段：取所有 span 与查找命中的边界并集，逐段决定样式。
fn build_union_job(
    theme: &Theme,
    content: &str,
    spans: &[SpanView],
    policy: &SpanPolicy<'_, impl Fn(&str) -> TextFormat>,
) -> LayoutJob {
    let mut job = empty_job(policy.wrap_width);
    let base = policy.base.clone();

    // 边界并集：所有 span 与查找命中的边界，**先吸附到字符边界**。
    //
    // span 与查找命中的坐标都来自外部（解析器 / 查找器），与 `content`
    // 未必同源——文本已改而 span 停在上一版时，边界会落到多字节字符的
    // 中间，下面的 `&content[lo..hi]` 就会 panic。本项目 `panic = "abort"`，
    // 一次 panic 即窗口消失，故一律向下吸附：最坏只是某一段少上色，绝不崩。
    let snap = |b: usize| floor_char_boundary(content, b);
    let mut bounds: Vec<usize> = vec![0, content.len()];
    for s in spans {
        bounds.push(snap(s.range.start));
        bounds.push(snap(s.range.end));
    }
    for &(s, e, _) in policy.find_marks {
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
        // 前景色：覆盖 [lo, hi) 的语法 span，否则基础色。
        //
        // 用 `find`（**首个**命中）而非「后者覆盖前者」——这是既有语义，
        // 改动会让「查找命中背景叠加在语法色之上」变成「语法色盖住背景」。
        let mut fmt = spans
            .iter()
            .find(|s| s.range.start <= lo && lo < s.range.end)
            .map(|s| (policy.style_of)(&s.style))
            .unwrap_or_else(|| base.clone());
        // 查找命中背景：该段落在任一命中区间内则叠加背景。
        if let Some(&(_, _, current)) = policy
            .find_marks
            .iter()
            .find(|&&(s, e, _)| s <= lo && lo < e)
        {
            fmt.background = if current {
                theme.find_current_bg
            } else {
                theme.find_bg
            };
        }
        job.append(&content[lo..hi], 0.0, fmt);
    }
    // 空行（content 为空）必须保留一个「空 section」，否则 egui 会把该行
    // 排成 0 高度：见 epaint `text_layout.rs` —— `Paragraph::empty_paragraph_height`
    // 只在 `layout_section` 处理到**非空** section 时才被置为 `line_height`；
    // 纯空 job 不触发任何 `layout_section`，空段落高度维持默认 0.0，
    // 于是整行高度为 0。拼回整文档 `Galley` 后，连续回车产生的空行统统不可见，
    // 行号栏又按 0 高度空行定位 → 与正文错位。这正是 V11-16「连续 N 次回车只显示
    // 一个空行、须再打字符才换行」的真正根因——与 IME / 回车事件无关，是自定义
    // 逐行排版器对空行高度处理的缺失。补一个空 section 即可让 egui 正确赋予行高。
    if job.sections.is_empty() {
        job.append(content, 0.0, base);
    }
    job
}

/// 字符边界防护。
///
/// 返回 `None` = 该区间**退化**（落点非法），调用方应跳过。
fn guard_range(
    content: &str,
    start: usize,
    end: usize,
    guard: BoundaryGuard,
) -> Option<(usize, usize)> {
    match guard {
        BoundaryGuard::Floor => {
            let lo = floor_char_boundary(content, start.min(content.len()));
            let hi = floor_char_boundary(content, end.min(content.len()));
            Some((lo, hi))
        }
        BoundaryGuard::Clamp => {
            clamp_to_char_boundaries(content, start.min(content.len()), end.min(content.len()))
        }
    }
}
