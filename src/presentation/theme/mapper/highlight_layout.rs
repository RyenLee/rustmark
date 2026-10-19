//! 语法高亮逐行增量排版：命中两层缓存、仅重建变更行，再拼回整文档 `Galley`。

use std::sync::Arc;

use egui::emath::GuiRounding;
use egui::text::{ByteIndex, TextWrapping};

use super::highlight_cache::{
    HighlightStats, LARGE_FILE_BYTES, LAST_STATS, LINE_LAYOUT, LineLayout, WHOLE_CACHE,
    atlas_token, build_line_job, editor_metrics_signature,
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

    // 2) 快路径：单行编辑时在上一版整篇 `Galley` 上**原地打补丁**。
    //
    // 走到这里说明整篇缓存已经 miss（文本变了），但「改一行」与「换一篇」的成本
    // 差着三个数量级：前者只需重排那一行、替换它在旧 Galley 里的 rows 切片；
    // 后者要拆行 + 差分 + 逐行遍历 + `concat` 全部 58k 行。
    // 1MB 实测后者 4.08ms，其中 3.0ms 是与「改了几行」无关的固定动作（§3.2.13）。
    //
    // 前置条件不成立（插入/删除换行、折行数变化、整篇替换……）就原样退回下面的
    // 全量重建——**正确性不依赖本路径是否命中**，它只是同一语义的更快实现。
    let mut previous = previous;
    let prev_whole: Option<Arc<egui::Galley>> = if previous.is_some() {
        // 整篇缓存与行布局同键同生命周期，且本帧必然覆盖它，故直接取走。
        // 少这一个 `Arc` 引用，下面才有机会 `try_unwrap` 出独占的 `Galley` 原地改写。
        WHOLE_CACHE.with(|c| c.borrow_mut().take().map(|entry| entry.2))
    } else {
        None
    };
    if let (Some(prev), Some(whole)) = (previous.as_mut(), prev_whole) {
        // `try_unwrap` 失败说明上一帧的 Galley 仍被别处持有（`TextEdit` 的
        // `TextEditOutput` 尚未释放）：退化为一次克隆（58k 个 `Arc` 克隆），
        // 仍比整篇重建便宜。
        let base = Arc::try_unwrap(whole).unwrap_or_else(|arc| (*arc).clone());
        let input = PatchInput {
            ctx,
            theme,
            text,
            spans: if large { &[] } else { spans.unwrap_or(&[]) },
            find,
            wrap_width,
        };
        if let Some((galley, stats)) = try_patch_single_line(&input, prev, base) {
            LINE_LAYOUT.with(|c| *c.borrow_mut() = std::mem::take(prev));
            WHOLE_CACHE.with(|c| {
                *c.borrow_mut() = Some((
                    sig,
                    wrap_width,
                    galley.clone(),
                    find_sig,
                    colored,
                    metrics,
                    atlas,
                ));
            });
            LAST_STATS.with(|s| *s.borrow_mut() = stats);
            return galley;
        }
    }

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
    // 逻辑行 → 整篇 `rows` 下标（末尾追加总行数）；单行编辑快路径据此定位切片。
    let mut row_starts: Vec<u32> = Vec::with_capacity(lines.len() + 1);
    let mut row_cursor = 0u32;
    row_starts.push(0);
    // 最宽行：整篇 `rect.width()` 由它决定（见 `LineLayout::widest_line`）。
    let mut widest_line = 0usize;
    let mut max_line_width = 0.0f32;
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
        if let Some(g) = line_galleys.last() {
            row_cursor += g.rows.len() as u32;
            if g.rect.width() > max_line_width {
                max_line_width = g.rect.width();
                widest_line = line_galleys.len() - 1;
            }
        }
        row_starts.push(row_cursor);
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
        c.row_starts = row_starts;
        c.widest_line = widest_line;
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

// ============================================================================
// 单行编辑快路径（A1-4）
// ============================================================================

/// 两段字节序列的公共前缀长度。
///
/// 按 `u64` 分组比较：1MB 全等前缀约 0.1ms，而逐字节循环要 1ms 量级
/// （LLVM 未必把 `zip` 向量化成 memcmp）。
fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + 8 <= n {
        // `i + 8 <= n <= 两段长度` ⇒ 切片必为 8 字节；`unwrap_or_default` 只是
        // 给「万一被改坏」留一个不 panic 的退路（本项目 `panic = "abort"`）。
        let x = u64::from_ne_bytes(a[i..i + 8].try_into().unwrap_or_default());
        let y = u64::from_ne_bytes(b[i..i + 8].try_into().unwrap_or_default());
        if x != y {
            break;
        }
        i += 8;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

/// 两段字节序列的公共后缀长度（从末尾往前比，同样 8 字节一组）。
fn common_suffix_len(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    while i + 8 <= n {
        let ea = a.len() - i - 8;
        let eb = b.len() - i - 8;
        let x = u64::from_ne_bytes(a[ea..ea + 8].try_into().unwrap_or_default());
        let y = u64::from_ne_bytes(b[eb..eb + 8].try_into().unwrap_or_default());
        if x != y {
            break;
        }
        i += 8;
    }
    while i < n && a[a.len() - 1 - i] == b[b.len() - 1 - i] {
        i += 1;
    }
    i
}

/// 新旧文本的差异边界 `(公共前缀长, 公共后缀长)`，两端都吸附到字符边界。
///
/// 必须吸附：两段 UTF-8 的公共前缀可能停在某个多字节字符的中间
/// （如 `é` = C3 A9 与 `è` = C3 A8 共享首字节 C3），此时按字节切会 panic。
fn diff_bounds(old: &str, new: &str) -> (usize, usize) {
    let mut prefix = common_prefix_len(old.as_bytes(), new.as_bytes());
    while prefix > 0 && !(old.is_char_boundary(prefix) && new.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let mut suffix = common_suffix_len(&old.as_bytes()[prefix..], &new.as_bytes()[prefix..]);
    while suffix > 0
        && !(old.is_char_boundary(old.len() - suffix) && new.is_char_boundary(new.len() - suffix))
    {
        suffix -= 1;
    }
    (prefix, suffix)
}

/// 快路径的排版输入（打包成一个结构体，避免 8 参数签名）。
struct PatchInput<'a> {
    ctx: &'a egui::Context,
    theme: &'a Theme,
    text: &'a str,
    spans: &'a [SpanView],
    find: Option<&'a [FindMark]>,
    wrap_width: f32,
}

/// 单行编辑的**原地补丁**快路径。
///
/// # 做什么
///
/// 文本只改了某一行时，不必拆行 + 差分 + 逐行遍历 + `concat` 整篇：
/// 只需把那一行重排，再把它在上一版整篇 `Galley::rows` 里的切片换掉，
/// 并修正 `num_vertices` / `num_indices` / `rect` / `mesh_bounds` / `job.text`。
///
/// # 前置条件（任一不成立即返回 `None`，调用方退回全量重建）
///
/// 1. 变更区内**没有换行**——行数不变，行号映射才成立；
/// 2. 变更行重排后的**视觉行数与上一版相同**——否则后续所有行的 y 偏移
///    要整体平移（软换行也会让一个逻辑行占多行），本路径不处理；
/// 3. 行布局的 `row_starts` 与整篇 `rows` 长度自洽。
///
/// # 为什么**不修改任何状态就返回 `None`**
///
/// 所有判定都在改 `prev` / `base` 之前完成。调用方在 `None` 时会拿同一个
/// `prev` 继续走全量重建，若快路径中途留了一半改动，重建出来的就是混合态。
///
/// # 已知取舍
///
/// - `rect` 按「各行宽度的最大值 × 总高」**精确重建**（行高由前置条件保证不变），
///   不做并集——否则删长短行后横向滚动条回不去。
/// - `mesh_bounds` 取并集（只增不减）：它只用于剔除，偏大是安全的；
///   精确重算是 O(全部视觉行)，与快路径的目的相悖。
/// - `Galley::intrinsic_size` 是 `pub(crate)`，外部改不了，保持旧值。
///   全仓只有 `egui` 的 atomics 会读它，`TextEdit` 不读，故无副作用。
fn try_patch_single_line(
    input: &PatchInput<'_>,
    prev: &mut LineLayout,
    base: egui::Galley,
) -> Option<(Arc<egui::Galley>, HighlightStats)> {
    let PatchInput {
        ctx,
        theme,
        text,
        spans,
        find,
        wrap_width,
    } = *input;
    // ---- 前置条件 3：行布局与整篇 rows 自洽 ----
    let line_count = prev.lines.len();
    if line_count == 0
        || prev.galleys.len() != line_count
        || prev.row_starts.len() != line_count + 1
        || *prev.row_starts.last()? as usize != base.rows.len()
    {
        return None;
    }

    // ---- 前置条件 1：变更区不跨行 ----
    let (old_len, old_ls, old_le, new_ls, new_le, line_idx) = {
        let old = base.text();
        let old_len = old.len();
        let (prefix, suffix) = diff_bounds(old, text);
        if prefix == old_len && prefix == text.len() {
            return None; // 文本没变：本来不该走到这里
        }
        let old_end = old_len - suffix;
        let new_end = text.len() - suffix;
        if old[prefix..old_end].contains('\n') || text[prefix..new_end].contains('\n') {
            return None; // 增删了换行：行数/行号变了
        }
        let old_ls = old[..prefix].rfind('\n').map_or(0, |i| i + 1);
        let new_ls = text[..prefix].rfind('\n').map_or(0, |i| i + 1);
        if old_ls != new_ls {
            return None; // 前缀相同 ⇒ 两者必然相等；纯防御
        }
        let old_le = old[old_end..].find('\n').map_or(old_len, |i| old_end + i);
        let new_le = text[new_end..]
            .find('\n')
            .map_or(text.len(), |i| new_end + i);
        let line_idx = text[..new_ls].matches('\n').count();
        (old_len, old_ls, old_le, new_ls, new_le, line_idx)
    };
    if line_idx >= line_count {
        return None;
    }

    // ---- 重排这一行（其余行不动）----
    let line = &text[new_ls..new_le];
    let mut line_spans: Vec<SpanView> = Vec::new();
    for s in spans {
        if s.range.start < s.range.end && s.range.start < new_le && s.range.end > new_ls {
            let start = s.range.start.max(new_ls) - new_ls;
            let end = s.range.end.min(new_le) - new_ls;
            if start < end {
                line_spans.push(SpanView {
                    range: RangeView::new(start, end),
                    style: s.style.clone(),
                    url: None,
                });
            }
        }
    }
    line_spans.sort_by_key(|s| s.range.start);

    let mut line_bgs: Vec<(usize, usize, bool)> = Vec::new();
    if let Some(marks) = find {
        for m in marks {
            let s = m.start.max(new_ls);
            let e = m.end.min(new_le);
            if s < e {
                line_bgs.push((s - new_ls, e - new_ls, m.current));
            }
        }
    }

    let job = build_line_job(theme, line, &line_spans, &line_bgs, wrap_width);
    let new_line_galley = ctx.fonts_mut(|f| f.layout_job(job));

    // ---- 前置条件 2：折行数与行高都不变 ----
    //
    // 折行数变了 → 后续行的 y 偏移要整体平移；行高变了（例如插入 emoji 命中
    // 更高的兜底字体）→ 同样会把后面的行顶歪。两者本路径都不处理。
    let row_start = prev.row_starts[line_idx] as usize;
    let row_end = prev.row_starts[line_idx + 1] as usize;
    let old_line_galley = Arc::clone(&prev.galleys[line_idx]);
    if row_end - row_start != new_line_galley.rows.len()
        || (new_line_galley.rect.height() - old_line_galley.rect.height()).abs() > 0.01
    {
        return None;
    }

    // ==== 以下开始修改；到这一步所有前置条件都已成立 ====
    let mut base = base;
    let y0 = base.rows[row_start].pos.y;
    let ppp = base.pixels_per_point;
    let is_last_line = line_idx + 1 == line_count;
    let mut mesh_bounds = base.mesh_bounds;

    for (k, placed) in new_line_galley.rows.iter().enumerate() {
        // `concat` 的定位公式：子 Galley 的相对坐标 + 该行的累计 y，再按像素取整。
        let pos = (placed.pos + egui::vec2(0.0, y0)).round_to_pixels(ppp);
        // `concat` 会给「某行的最后一条视觉行」补换行标记（末行除外）。
        let mut ends_with_newline = placed.ends_with_newline;
        if k + 1 == new_line_galley.rows.len() {
            ends_with_newline |= !is_last_line;
        }
        mesh_bounds |= placed.visuals.mesh_bounds.translate(pos.to_vec2());
        base.rows[row_start + k] = egui::epaint::text::PlacedRow {
            pos,
            row: Arc::clone(&placed.row),
            ends_with_newline,
        };
    }

    // 行内容复用原有分配（`clear` + `push_str`），不产生新的 `String`。
    prev.lines[line_idx].clear();
    prev.lines[line_idx].push_str(line);
    prev.galleys[line_idx] = Arc::clone(&new_line_galley);

    // 整篇 `rect` = 「各行宽度的最大值 × 总高」（行高已由前置条件保证不变）。
    // 只有「改的就是最宽行」或「新行更宽」时才需要重扫；重扫是 O(行数)，
    // 但只在缩短最宽行时发生，日常连续输入走不到。
    let width = if line_idx == prev.widest_line || new_line_galley.rect.width() >= base.rect.width()
    {
        let mut best = (0.0f32, 0usize);
        for (i, g) in prev.galleys.iter().enumerate() {
            let w = g.rect.width();
            if w > best.0 {
                best = (w, i);
            }
        }
        prev.widest_line = best.1;
        best.0
    } else {
        base.rect.width()
    };
    base.rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, base.rect.height()));

    let vertices = base.num_vertices as isize - old_line_galley.num_vertices as isize
        + new_line_galley.num_vertices as isize;
    let indices = base.num_indices as isize - old_line_galley.num_indices as isize
        + new_line_galley.num_indices as isize;
    base.num_vertices = vertices.max(0) as usize;
    base.num_indices = indices.max(0) as usize;
    base.mesh_bounds = mesh_bounds;

    // `job.text` 必须与正文同步：`Galley::text()` 既是整篇缓存的全文键，
    // 也是 `TextEdit` 算光标 / 选区的依据。
    //
    // 独占且只有一段时**就地替换那一段**（O(尾部位移)），不重建整份 1MB 字符串；
    // 拿不到独占（上一帧的 Galley 还被别处持有）才退回「重建 job」。
    let patched_job = match Arc::get_mut(&mut base.job) {
        Some(job)
            if job.sections.len() == 1
                && job.sections[0].byte_range == (ByteIndex(0)..ByteIndex(old_len)) =>
        {
            job.text.replace_range(old_ls..old_le, line);
            job.sections[0].byte_range = ByteIndex(0)..ByteIndex(job.text.len());
            true
        }
        _ => false,
    };
    if !patched_job {
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
        base.job = Arc::new(job);
    }

    // 行内容复用原有分配（`clear` + `push_str`），不产生新的 `String`。
    prev.lines[line_idx].clear();
    prev.lines[line_idx].push_str(line);
    prev.galleys[line_idx] = Arc::clone(&new_line_galley);

    let stats = HighlightStats {
        total_lines: line_count,
        reshaped_lines: 1,
        cached_lines: line_count - 1,
        patched: true,
    };
    Some((Arc::new(base), stats))
}
