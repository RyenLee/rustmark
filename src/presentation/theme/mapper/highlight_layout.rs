//! 语法高亮逐行增量排版：命中两层缓存、仅重建变更行，再拼回整文档 `Galley`。

use std::sync::Arc;

use egui::text::TextWrapping;

use super::highlight_cache::{
    LARGE_FILE_BYTES, LAST_STATS, LINE_LAYOUT, LineLayout, WHOLE_CACHE, atlas_token,
    build_line_job, editor_metrics_signature,
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
///
/// # 缓存 miss 路径的成本纪律（问题 052）
///
/// 本函数在**每次击键**时都走完整条 miss 路径，故沿途**不得出现「与文档长度或
/// 行数成正比的重复劳动」**。实测（1MB / 58,514 行）曾有下列开销，均已消除：
///
/// | 位置 | 早先的写法 | 实测 |
/// |---|---|---|
/// | 整文档缓存读取 | `c.borrow().clone()`（`Ref<T>` 不实现 `Clone`，克隆的是整条条目，含 1MB 全文） | 0.23ms／帧 |
/// | 取上一版行布局 | `c.lines.clone()`（58k 次小堆分配）+ `c.galleys.clone()` | 2.47ms + 0.44ms |
/// | 回写行内容 | `lines.iter().map(to_string).collect()`，全部 58k 行重建 | 2.29ms |
/// | 回写行 Galley | `line_galleys.clone()` | 0.44ms |
/// | 整文档缓存回写 | `text.to_string()` | 0.23ms |
/// | 拆行 | `split('\n').collect()`（容量下界为 0 → 反复扩容搬移） | 1.17ms |
///
/// 现改为：整文档缓存在借用内比对（文本判据取 `galley.text()`）、上一版行布局
/// **整条移出**（`mem::take`）、行内容**移动复用**、行 Galley 表移动回写、
/// 拆行按上一版行数预置容量。全部是「本来就要做同一件事，只是不再重复分配」，
/// 对外可观测行为零变化（由 `tests/presentation/highlight_layout_test.rs` 的
/// 等价性与计数器断言钉住）。
///
/// **新增每帧查询前先问一次**：这次构造是 O(1)，还是 O(全文) / O(行数)？
/// （同问题 043 / 046 定下的纪律。）
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
    // 字体图集令牌：egui 重建图集后，跨帧缓存的 `Galley` 字形坐标全部失效，
    // 必须让缓存作废整篇重排（V11-17，见 `atlas_token` 的长注释）。
    let atlas = atlas_token(ctx);

    // 1) 整文档缓存：文本 / 主题 / 换行 / 查找命中 / 着色状态 / 排版度量 /
    //    图集令牌未变 → 直接返回。
    //
    // **在借用内比对，不克隆条目**（问题 052）：早先写成
    // `WHOLE_CACHE.with(|c| c.borrow().clone())`——而 `Ref<T>` 不实现 `Clone`，
    // 克隆的是 `T` 本身，于是每帧（含空闲帧）都在复制整条条目，含那份 1MB 全文。
    // 文本判据改取 `galley.text()`（见 `WholeCacheEntry` 的注释），整条命中路径
    // 只剩一次 `Arc<Galley>` 克隆。
    let hit = WHOLE_CACHE.with(|c| {
        let c = c.borrow();
        match c.as_ref() {
            Some((
                cached_sig,
                cached_wrap,
                galley,
                cached_find,
                cached_colored,
                cached_metrics,
                cached_atlas,
            )) if *cached_sig == sig
                && *cached_wrap == wrap_width
                && *cached_find == find_sig
                && *cached_colored == colored
                && *cached_metrics == metrics
                && *cached_atlas == atlas
                && galley.text() == text =>
            {
                Some(galley.clone())
            }
            _ => None,
        }
    });
    if let Some(galley) = hit {
        return galley;
    }

    // 大文件降级：跳过 span 着色，纯文本排版（仍逐行缓存）。

    // 取上一版行布局：**整条移出**（`mem::take`）而不是克隆，用完在函数末尾写回。
    //
    // 早先这里是 `c.lines.clone()` + `c.galleys.clone()`——1MB / 58k 行文档下每次
    // 击键克隆 58k 个 `String`（58k 次小堆分配，微基准 2.47ms）与 58k 个 `Arc`
    // （0.44ms）；而这份旧表比完即弃，纯属白干（问题 052）。
    //
    // 一致才谈得上复用：主题 / 换行宽度 / 查找命中 / 着色状态 / 排版度量 / 图集
    // 令牌任一不同即视为无上一版。`colored` 与 `atlas` 必须在键里，理由见
    // `LineLayout` 各字段的注释（前者防「打字后颜色再也不回来」，后者防 V11-17 乱码）。
    let previous: Option<LineLayout> = LINE_LAYOUT.with(|c| {
        let mut c = c.borrow_mut();
        let reusable = c.valid
            && c.sig == sig
            && c.wrap == wrap_width
            && c.find_sig == find_sig
            && c.colored == colored
            && c.metrics == metrics
            && c.atlas == atlas;
        reusable.then(|| std::mem::take(&mut *c))
    });

    // 上一版每行的内容与 Galley。
    //
    // `old_lines` 多包一层 `Option` 是为了下面能**移动**取走被复用的那一行
    // （`Option::take` + `mem::take` 都不产生分配）。
    let (mut old_lines, old_galleys): (Vec<Option<String>>, Vec<Arc<egui::Galley>>) = match previous
    {
        Some(p) => (p.lines.into_iter().map(Some).collect(), p.galleys),
        None => (Vec::new(), Vec::new()),
    };

    // 拆行：`split('\n')` 天然保留末尾空行语义（"a\nb\n" → ["a","b",""]），
    // 与 `concat` 的换行补齐一致。
    //
    // 容量按**上一版行数**预置（问题 052）：连续输入时行数几乎不变，于是这一次
    // 分配就够。`Split` 的 `size_hint` 下界是 0，不预置的话 `collect` 会按
    // 4→8→… 反复重新分配——1MB / 58k 行下实测 1.17ms，其中大半是这些无谓搬移。
    // 刻意**不**为此再扫一遍全文数 `\n`：`shared::text::line_count` 在 1MB 上自身
    // 就要 0.3~0.5ms，省下的分配还不够抵这一次额外扫描。
    let mut lines: Vec<&str> = Vec::with_capacity(old_lines.len() + 1);
    lines.extend(text.split('\n'));

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

    // 逐行判断「新第 i 行能否复用旧的某行」，得到一张映射表。
    //
    // 先看前后缀（吸收「行号整体平移」的情形：中间插入 / 删除一行时，
    // 前后大量行的**内容**其实没变），再对夹在中间的段落尝试逐行配对——
    // 后者覆盖「全文替换」这类多处散布的改动：行数不变、只有部分行真的变了。
    // 配对不上（两侧行数不同）就老实地重排中间段。
    //
    // 比的是**行内容**，与「该行所有权是否已被取走」无关（此刻尚未取任何一行）。
    let reuse_map: Vec<Option<usize>> = if old_lines.is_empty() {
        vec![None; lines.len()]
    } else {
        let (n, m) = (old_lines.len(), lines.len());
        // 旧表此刻必然全是 `Some`（还没开始取）；`unwrap_or("")` 只是防御。
        let old = |i: usize| old_lines[i].as_deref().unwrap_or("");
        let mut map = vec![None; m];

        let mut pre = 0usize;
        while pre < n.min(m) && old(pre) == lines[pre] {
            map[pre] = Some(pre);
            pre += 1;
        }
        let mut suf = 0usize;
        while pre + suf < n.min(m) && old(n - 1 - suf) == lines[m - 1 - suf] {
            map[m - 1 - suf] = Some(n - 1 - suf);
            suf += 1;
        }

        // 中间两段行数相同才做逐行配对，否则（插入 / 删除了行）无从对齐。
        let mid_new = pre..(m - suf);
        let mid_old = pre..(n - suf);
        if mid_new.len() == mid_old.len() {
            for k in 0..mid_new.len() {
                let (i, j) = (mid_old.start + k, mid_new.start + k);
                if old(i) == lines[j] {
                    map[j] = Some(i);
                }
            }
        }
        map
    };

    let mut line_galleys: Vec<Arc<egui::Galley>> = Vec::with_capacity(lines.len());
    // 新版每行的内容。所有权策略见循环内「行内容的所有权」一段（问题 052）。
    let mut new_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut stats = HighlightStats {
        total_lines: lines.len(),
        ..Default::default()
    };

    let mut next_span = 0usize; // `prepared` 中尚未入场的最小下标
    let mut active: Vec<usize> = Vec::with_capacity(prepared.len().min(64));
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
                        url: None,
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

        // 行内容的所有权（问题 052）：命中映射表就**移动**上一版那条 `String`，
        // 只有真正重排的行才新建。早先这里是
        // `lines.iter().map(|s| (*s).to_string()).collect()`——把 58k 行**全部**
        // 重新分配一遍（微基准 2.29ms），而其中绝大多数与上一版逐字节相同，
        // `reuse_map` 早就知道是哪几行了。
        //
        // 映射表是单射（前缀段 `< pre`、后缀段 `>= n - suf`、中段夹在两者之间，
        // 三段互不相交），故每行至多被取走一次。万一该不变式被破坏，
        // `unwrap_or_else` 退回「重新分配一条」——内容依旧正确，且绝不 panic
        // （`panic = "abort"` 下崩一次即整窗口消失）。
        let owned = reuse_idx.and_then(|i| old_lines.get_mut(i).and_then(Option::take));
        new_lines.push(owned.unwrap_or_else(|| (*line).to_owned()));

        // 复用行的 Galley 只是 `Arc` 克隆（薄外壳），不触发任何重新排版；
        // `concat` 拼接时同样只克隆各行的 `Arc<Row>`，故整篇 Galley 与这张逐行表
        // **共享**同一份行数据（内存不翻倍）。
        match reuse_idx.and_then(|i| old_galleys.get(i).cloned()) {
            Some(g) => {
                line_galleys.push(g);
                stats.cached_lines += 1;
            }
            None => {
                let job = build_line_job(theme, line, &line_spans, &line_bgs, wrap_width);
                // `Fonts::layout_job` 本身已返回 `Arc<Galley>`，不要再包一层。
                let g = ctx.fonts_mut(|f| f.layout_job(job));
                line_galleys.push(g);
                stats.reshaped_lines += 1;
            }
        }
    }

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
    // 顺带 `job.text` 也由此写成全文——它现在同时充当**整文档缓存的全文键**
    // （`galley.text() == text`，问题 052），故这一句 append 有两重作用，不可删。
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
    // `concat` 只借用 `line_galleys`，返回后它即可被移动进行缓存（不再克隆）。
    let combined = Arc::new(egui::Galley::concat(
        Arc::new(job),
        &line_galleys,
        ctx.pixels_per_point(),
    ));

    // 记住这一版，供下一次变更做差分。
    //
    // 两个 `Vec` 都是**移动**进来的（问题 052）：早先是 `new_lines` 全新重建 +
    // `c.galleys = line_galleys.clone()`，前者白花 2.29ms、后者白花 0.44ms。
    LINE_LAYOUT.with(|c| {
        let mut c = c.borrow_mut();
        c.lines = new_lines;
        c.galleys = line_galleys;
        c.sig = sig;
        c.wrap = wrap_width;
        c.find_sig = find_sig;
        c.colored = colored;
        c.metrics = metrics;
        c.atlas = atlas;
        c.valid = true;
    });

    // 整文档条目**不再另存一份全文**：`combined.text()`（= `job.text`，上一行刚写入
    // 的那份）就是缓存键里的全文判据（问题 052，见 `WholeCacheEntry` 的注释）。
    WHOLE_CACHE.with(|c| {
        *c.borrow_mut() = Some((
            sig,
            wrap_width,
            combined.clone(),
            find_sig,
            colored,
            metrics,
            atlas,
        ));
    });
    LAST_STATS.with(|s| *s.borrow_mut() = stats);

    combined
}
