//! 语法高亮逐行增量排版：命中两层缓存、仅重建变更行，再拼回整文档 `Galley`。

use std::sync::Arc;

use egui::text::TextWrapping;

use super::highlight_cache::{
    LARGE_FILE_BYTES, LAST_STATS, LINE_LAYOUT, WHOLE_CACHE, build_line_job,
    editor_metrics_signature,
};
use super::*;
use crate::application::dto::{RangeView, SpanView};
use crate::presentation::theme::builtin::palette_signature;

/// 构建语法高亮的 `Galley`（S6-04 增量版）。
///
/// 把 `spans`（字节区间 + 样式字符串，来自 `MarkdownAppService::highlight`）
/// 按行映射成 egui 文本格式，逐行排版并缓存；仅变更行重新排版，其余行从单行 LRU
/// 取回，再用 [`egui::Galley::concat`] 拼成 `TextEdit` 所需的整文档 Galley。
/// 无 span 时退化为纯文本排版。
///
/// 换主题后 `palette_signature(theme)` 变化 → 两层缓存全部失效 → 下一帧整篇重建，
/// 因此「切换主题无需重启」依旧成立。
/// 兼容旧签名的便捷封装：不带查找高亮（`find = None`）。
///
/// 既有测试与不需要高亮的调用方继续走这一条，签名不变。
pub fn highlight_layout(
    ctx: &egui::Context,
    theme: &Theme,
    text: &str,
    spans: Option<&[SpanView]>,
    wrap_width: f32,
) -> Arc<egui::Galley> {
    highlight_layout_with_find(ctx, theme, text, spans, wrap_width, None)
}

/// 构建语法高亮的 `Galley`（S6-04 增量版），可叠加编辑区查找命中高亮。
///
/// 把 `spans`（字节区间 + 样式字符串，来自 `MarkdownAppService::highlight`）
/// 按行映射成 egui 文本格式，逐行排版并缓存；仅变更行重新排版，其余行从单行 LRU
/// 取回，再用 [`egui::Galley::concat`] 拼成 `TextEdit` 所需的整文档 Galley。
/// 无 span 时退化为纯文本排版。
///
/// `find` 是编辑区当前的查找命中区间（文档字节坐标）；命中处叠加背景色
/// （[`Theme::find_bg`] / [`Theme::find_current_bg`]），让「在编辑器里看得到
/// 查找命中」与原型对齐。查找命中会改变排版结果，故其签名并入两层缓存键——
/// 命中集变化即整篇（或仅行布局）失效重建，不会残留旧高亮。
///
/// 换主题后 `palette_signature(theme)` 变化 → 两层缓存全部失效 → 下一帧整篇重建，
/// 因此「切换主题无需重启」依旧成立。
pub fn highlight_layout_with_find(
    ctx: &egui::Context,
    theme: &Theme,
    text: &str,
    spans: Option<&[SpanView]>,
    wrap_width: f32,
    find: Option<&[FindMark]>,
) -> Arc<egui::Galley> {
    let sig = palette_signature(theme);
    // 查找命中签名：命中集变化必须让缓存失效，否则会残留旧高亮。
    // 用 FNV 风格折叠即可——碰撞至多导致一次无关紧要的「外观未刷新」，
    // 不会造成崩溃或文本错位。
    let find_sig: u64 = match find {
        None => 0,
        Some(marks) => {
            let mut h = marks.len() as u64;
            for m in marks {
                h = h
                    .wrapping_mul(1099511628211)
                    .wrapping_add(((m.start as u64) << 32) | (m.end as u64))
                    .wrapping_add(m.current as u64);
            }
            h
        }
    };

    // 本帧是否带语法着色（OPT-011 ①）：防抖期内应用层返回空片段，
    // 文本与上一版相同但着色状态变了，必须让两层缓存都失效。
    let large = text.len() > LARGE_FILE_BYTES;
    let colored = !large && spans.is_some_and(|s| !s.is_empty());
    // 字号 / 行高变了必须重排（见 `editor_metrics_signature` 的文档注释）。
    let metrics = editor_metrics_signature(theme);

    // 1) 整文档缓存：文本 / 主题 / 换行 / 查找命中 / 着色状态 / 排版度量未变 → 直接返回。
    if let Some((
        cached_sig,
        cached_text,
        cached_wrap,
        galley,
        cached_find,
        cached_colored,
        cached_metrics,
    )) = WHOLE_CACHE.with(|c| c.borrow().clone())
        && cached_sig == sig
        && cached_text == text
        && cached_wrap == wrap_width
        && cached_find == find_sig
        && cached_colored == colored
        && cached_metrics == metrics
    {
        return galley;
    }

    // 大文件降级：跳过 span 着色，纯文本排版（仍逐行缓存）。

    // 拆行：`split('\n')` 天然保留末尾空行语义（"a\nb\n" → ["a","b",""]），
    // 与 `concat` 的换行补齐一致。
    let lines: Vec<&str> = text.split('\n').collect();

    // 按起点升序的 span 快照；`active` 用双指针维护，使 span 分配是 **O(行数 + span数)**。
    // （逐行全量 `.filter(...)` 会退化成 O(行数 × span数)，10KB 文档就足以让每次击键卡顿。）
    let prepared: Vec<(RangeView, &str)> = if large {
        Vec::new()
    } else {
        let mut v: Vec<(RangeView, &str)> = spans
            .unwrap_or(&[])
            .iter()
            .filter(|s| s.range.start < s.range.end)
            .map(|s| (s.range, s.style.as_str()))
            .collect();
        v.sort_by_key(|(r, _)| r.start);
        v
    };

    // 取上一版行布局（主题 / 换行宽度 / 查找命中 / 着色状态一致才谈得上复用）。
    //
    // `colored` 必须参与：行缓存按**行内容**比对，而防抖期排的纯文本行
    // 与停手后带色的同一行内容完全相同——不带这个标志就会被复用，
    // 颜色永远补不回来。
    let previous = LINE_LAYOUT.with(|c| {
        let c = c.borrow();
        if c.valid
            && c.sig == sig
            && c.wrap == wrap_width
            && c.find_sig == find_sig
            && c.colored == colored
            && c.metrics == metrics
        {
            Some((c.lines.clone(), c.galleys.clone()))
        } else {
            None
        }
    });

    // 逐行判断「新第 i 行能否复用旧的某行」，得到一张映射表。
    //
    // 先看前后缀（吸收「行号整体平移」的情形：中间插入 / 删除一行时，
    // 前后大量行的**内容**其实没变），再对夹在中间的段落尝试逐行配对——
    // 后者覆盖「全文替换」这类多处散布的改动：行数不变、只有部分行真的变了。
    // 配对不上（两侧行数不同）就老实地重排中间段。
    let reuse_map: Vec<Option<usize>> = match &previous {
        None => vec![None; lines.len()],
        Some((old_lines, _)) => {
            let (n, m) = (old_lines.len(), lines.len());
            let mut map = vec![None; m];

            let mut pre = 0usize;
            while pre < n.min(m) && old_lines[pre] == lines[pre] {
                map[pre] = Some(pre);
                pre += 1;
            }
            let mut suf = 0usize;
            while pre + suf < n.min(m) && old_lines[n - 1 - suf] == lines[m - 1 - suf] {
                map[m - 1 - suf] = Some(n - 1 - suf);
                suf += 1;
            }

            // 中间两段行数相同才做逐行配对，否则（插入 / 删除了行）无从对齐。
            let mid_new = pre..(m - suf);
            let mid_old = pre..(n - suf);
            if mid_new.len() == mid_old.len() {
                for k in 0..mid_new.len() {
                    let (i, j) = (mid_old.start + k, mid_new.start + k);
                    if old_lines[i] == lines[j] {
                        map[j] = Some(i);
                    }
                }
            }
            map
        }
    };

    let mut line_galleys: Vec<Arc<egui::Galley>> = Vec::with_capacity(lines.len());
    let mut stats = HighlightStats {
        total_lines: lines.len(),
        ..Default::default()
    };

    let mut next_span = 0usize; // `prepared` 中尚未入场的最小下标
    let mut active: Vec<usize> = Vec::new(); // 可能覆盖当前行及其后行的 span 下标
    let mut byte_cursor = 0usize;

    for (idx, line) in lines.iter().enumerate() {
        let line_start = byte_cursor;
        let line_end = line_start + line.len();
        byte_cursor = line_end + 1; // 跳过 '\n'（单字节）

        // 起点落在本行内的 span 入场。
        while next_span < prepared.len() && prepared[next_span].0.start < line_end {
            active.push(next_span);
            next_span += 1;
        }

        // 本行能否复用上一版：查差分映射表（`None` 表示真的变了，需重排）。
        let reuse_idx = reuse_map[idx];

        // 裁剪到行内并转成行内偏移；仍有余量的留到下一行。
        // 复用的行不必产出 span（省掉每段 `String` 的构造），但**必须照常推进游标**。
        let mut line_spans: Vec<SpanView> = Vec::new();
        active.retain(|&i| {
            let r = prepared[i].0;
            if r.end <= line_start {
                return false; // 已在本行之前结束
            }
            if reuse_idx.is_none() {
                let start = r.start.saturating_sub(line_start);
                let end = (r.end - line_start).min(line.len());
                if start < end {
                    line_spans.push(SpanView {
                        range: RangeView::new(start, end),
                        style: prepared[i].1.to_string(),
                    });
                }
            }
            r.end > line_end
        });
        // 查找命中也只在非复用行上叠加：复用行已把背景烘焙进 `Galley`。
        let mut line_bgs: Vec<(usize, usize, bool)> = Vec::new();
        if reuse_idx.is_none()
            && let Some(marks) = find
        {
            for m in marks {
                let s = m.start.max(line_start);
                let e = m.end.min(line_end);
                if s < e {
                    line_bgs.push((s - line_start, e - line_start, m.current));
                }
            }
        }

        match (reuse_idx, &previous) {
            (Some(old_idx), Some((_, old_galleys))) => {
                line_galleys.push(old_galleys[old_idx].clone());
                stats.cached_lines += 1;
            }
            _ => {
                let job = build_line_job(theme, line, &line_spans, &line_bgs, wrap_width);
                // `Fonts::layout_job` 本身已返回 `Arc<Galley>`，不要再包一层。
                let g = ctx.fonts_mut(|f| f.layout_job(job));
                line_galleys.push(g);
                stats.reshaped_lines += 1;
            }
        }
    }

    // 记住这一版，供下一次变更做差分。
    LINE_LAYOUT.with(|c| {
        let mut c = c.borrow_mut();
        c.lines = lines.iter().map(|s| (*s).to_string()).collect();
        c.galleys = line_galleys.clone();
        c.sig = sig;
        c.wrap = wrap_width;
        c.find_sig = find_sig;
        c.colored = colored;
        c.metrics = metrics;
        c.valid = true;
    });

    // 2) 拼回整文档 Galley：`job.text` 写全本（保证双击选词 / 三击选行基于全文），
    //    各行的 `ends_with_newline` 与纵向偏移由 `concat` 处理。
    //
    // **`sections` 必须非空（DEF-05）**：`Galley::concat` 只是把传入的 `job`
    // 原样搬到合并结果上（`rows / mesh_bounds / num_vertices` 由各子 Galley 汇总，
    // 唯独 `job` 是直接 clone 的），而下游有两处都把
    // `galley.is_empty()`（≡ `job.sections.is_empty()`）当作「整篇为空、不画」的判据：
    //   - `egui::Painter::galley`：`if !galley.is_empty() { self.add(Shape::galley(..)) }`
    //   - `epaint::Tessellator::tessellate_text`：`if galley.is_empty() { return; }`
    // 早先这里留空 `sections`，整篇 Galley 便被判为「空」——
    // 逐行 Galley 排得再正确，`Shape` 也进不了 `PaintList`，编辑区永远是白板
    // （深色 / 浅色主题一视同仁，与配色无关）。
    //
    // 修复方式只是「把 `sections` 撑起来」：用正文格式 append 整段文本。
    // 真正的着色早已烘焙在逐行 `Galley` 里，`concat` 不会重新排版，
    // 因此这段格式化信息只用于让 `is_empty()` 返回 `false`，不影响任何字形。
    // 顺带 `job.text` 也由此写成全文（与原来直接赋值等价）。
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
    job.append(text, 0.0, StyleMapper::new(theme).format("plain"));
    let combined = Arc::new(egui::Galley::concat(
        Arc::new(job),
        &line_galleys,
        ctx.pixels_per_point(),
    ));

    WHOLE_CACHE.with(|c| {
        *c.borrow_mut() = Some((
            sig,
            text.to_string(),
            wrap_width,
            combined.clone(),
            find_sig,
            colored,
            metrics,
        ));
    });
    LAST_STATS.with(|s| *s.borrow_mut() = stats);

    combined
}
