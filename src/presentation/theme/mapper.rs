//! 样式映射：领域语义 → egui 呈现（阶段 06 · S6-10）。
//!
//! # 职责边界（这是 S5-03 那个决策的兑现）
//!
//! S5-03 决定**不用** syntect 的主题，改用领域自有的 `SpanStyle` 枚举，
//! 因为「颜色」属于表现层，不该泄漏进领域。本模块就是那个边界的具体实现：
//!
//! ```text
//! syntect scope ──► SpanStyle（领域，无颜色）
//!                        │  应用层转成字符串
//!                        ▼
//!                 style字符串 ──► **本模块** ──► egui TextFormat
//! ```
//!
//! 好处：换主题只改 [`Theme`]，不需要碰 syntect、不需要重新解析、
//! 也不需要把主题包塞进二进制。
//!
//! # 字符串契约
//!
//! 映射的输入是**字符串**（[`crate::application::dto::SpanView::style`]），
//! 而不是领域类型——因为表现层不得依赖 `domain`。
//! 取值来自 `MarkdownAppService::span_style_str`，共 21 种；
//! 本模块的 [`StyleMapper::format`] 是它们的**唯一**解释处。

use std::cell::RefCell;
use std::sync::Arc;

use egui::text::TextWrapping;
use egui::{Color32, TextFormat};

use super::builtin::{Theme, palette_signature};

use crate::application::dto::{RangeView, SpanView};
use crate::shared::geometry::floor_char_boundary;

/// 编辑区查找命中高亮区间（**文档字节**坐标）。
///
/// 由表现层在绘制编辑区前从 [`crate::presentation::state::find_replace_ui_state::FindReplaceUiState`]
/// 的命中列表换算而来，传给 [`highlight_layout_with_find`]，由其落到逐行 `Galley`
/// 的背景上。`current` 标记「当前命中」，使用更醒目的背景色
/// （[`Theme::find_current_bg`]），其余命中用 [`Theme::find_bg`]——
/// 与原型 `tk-find` / `tk-find-current` 的观感一致。
#[derive(Clone, Copy, Debug)]
pub struct FindMark {
    /// 命中起点（字节，含）。
    pub start: usize,
    /// 命中终点（字节，不含）。
    pub end: usize,
    /// 是否为当前命中（更醒目的背景）。
    pub current: bool,
}

/// 单个波浪线片段的几何（**纯函数产物**，供无头测试断言）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Squiggle {
    /// 片段左上角。
    pub rect: egui::Rect,
    /// 锯齿相位。相邻片段必须错开相位，否则连成一条实线而不是波浪线。
    pub phase: f32,
}

/// 拼写错词标记（字节区间 + 原词）。
///
/// 与 [`FindMark`] 同构：都只带区间不带内容，供排版层每帧比对。
#[derive(Clone, Copy, Debug)]
pub struct SpellMark {
    /// 错词起点（字节，含）。
    pub start: usize,
    /// 错词终点（字节，不含）。
    pub end: usize,
}

/// 波浪线的振幅与波长（像素）。
///
/// 取值参考主流编辑器的观感：振幅 1.5px 在 14–16px 字号下清晰可辨又不至于
/// 抢过正文；波长 4px 约等于「一个字符宽的三分之一」，密集但不糊成实线。
const SQUIGGLE_AMPLITUDE: f32 = 1.5;
const SQUIGGLE_WAVELENGTH: f32 = 4.0;

/// 由错词区间生成波浪线片段几何（**纯函数**）。
///
/// # 为什么是纯函数
///
/// 波浪线是「错词区间 × 排版结果」的乘积，天然只能在有 `Galley` 的地方算。
/// 但「区间 → 锯齿片段序列」这段逻辑本身与主题、字体无关，抽出来才能在
/// 无头环境断言：跨行错词是否被拆成两段、零宽/越界区间是否被丢弃、
/// 相邻片段的相位是否错开——这些恰恰是肉眼很难发现的缺陷。
///
/// `galley` 须由与 `text` 同源的排版结果而来，否则字节↔像素映射会错位。
pub fn squiggles_for(
    galley: &egui::Galley,
    text: &str,
    marks: &[SpellMark],
    origin: egui::Vec2,
) -> Vec<Squiggle> {
    let mut out = Vec::new();
    for (i, m) in marks.iter().enumerate() {
        // 越界区间直接跳过：调用方（应用层）已按文档内容给界，
        // 但纯函数不该假设输入一定合法——`panic="abort"` 下越界切片会直接崩进程。
        if m.start >= m.end || m.end > text.len() || !text.is_char_boundary(m.start) {
            continue;
        }
        // `CCursor` 用**字符索引**而非字节偏移，故先换算。
        // 区间两端已验证落在字符边界上，`chars().count()` 的 O(n) 代价可接受——
        // 错词数量通常个位数。
        let start_char = text[..m.start].chars().count();
        let end_char = text[..m.end].chars().count();
        let start_rect = galley.pos_from_cursor(egui::text::CCursor::new(start_char));
        let end_rect = galley.pos_from_cursor(egui::text::CCursor::new(end_char));

        for (seg, rect) in row_segments(galley, start_rect, end_rect)
            .into_iter()
            .enumerate()
        {
            let rect = rect.translate(origin);
            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                continue;
            }
            out.push(Squiggle {
                rect,
                // 跨行片段之间也要错开相位：按片段序号推相位，
                // 否则第二行的波浪会与第一行垂直对齐，看着像下划线。
                phase: (i + seg) as f32 * SQUIGGLE_WAVELENGTH,
            });
        }
    }
    out
}

/// 把错词区间拆成「每行一段」的矩形列表（处理软换行导致的跨行）。
///
/// # 为什么需要拆
///
/// `pos_from_cursor` 只给出**光标所在那一行**的矩形。一个错词若跨了软换行，
/// 直接拿首尾矩形会得到一个横跨两行的巨大矩形——波浪线会横穿整段正文。
///
/// 本版本 egui 的 [`egui::text::Row`] 不暴露字符区间，故按**行的几何**切分：
/// 用首尾光标矩形的纵向位置定位起止行，中间行取整行宽度。
fn row_segments(
    galley: &egui::Galley,
    start_rect: egui::Rect,
    end_rect: egui::Rect,
) -> Vec<egui::Rect> {
    // 定位起止行在 `rows` 中的下标（按纵向位置匹配）。
    //
    // # 为什么必须借`rows` 而不能直接用光标矩形
    //
    // `pos_from_cursor` 返回的是**零宽的光标矩形**（`[[x, top] - [x, bottom]]`，
    // 宽恒为 0）。直接用它俩拼`from_min_max(start.left_center(), end.right_center())`
    // 得到的矩形**高度也是 0**，会被后续的零面积过滤整条丢掉——
    // 这正是「一个片段都没产出」的原因。波浪线需要的是**行的纵向范围**，
    // 故必须回到 `galley.rows` 取真实行高。
    let row_top = |y: f32| -> Option<usize> {
        galley
            .rows
            .iter()
            .position(|r| y >= r.pos.y - 0.5 && y < r.pos.y + r.row.size.y + 0.5)
    };
    let single = |sr: egui::Rect, er: egui::Rect| {
        vec![egui::Rect::from_min_max(
            egui::pos2(sr.left(), sr.top()),
            egui::pos2(er.left(), er.bottom()),
        )]
    };

    let Some(first) = row_top(start_rect.top()) else {
        return single(start_rect, end_rect);
    };
    let Some(last) = row_top(end_rect.top()) else {
        return single(start_rect, end_rect);
    };

    let mut out = Vec::new();
    for idx in first..=last.min(galley.rows.len().saturating_sub(1)) {
        let placed = &galley.rows[idx];
        let row_rect = placed.rect();
        let x0 = if idx == first {
            start_rect.left_center().x
        } else {
            row_rect.left()
        };
        let x1 = if idx == last {
            end_rect.right_center().x
        } else {
            row_rect.right()
        };
        let seg = egui::Rect::from_min_max(
            egui::pos2(x0, row_rect.top()),
            egui::pos2(x1, row_rect.bottom()),
        );
        // 退化段（零宽 / 越界）跳过，避免画出无意义的点。
        if seg.width() > 0.0 {
            out.push(seg);
        }
    }
    // 兜底：行定位失败时退回单段——宁可位置不够准，也不要整条错词没有波浪线。
    // 注意纵向范围仍取自光标矩形的 top/bottom（光标矩形是零宽但**高度非零**，
    // 正是行高；只有横向是退化的）。
    if out.is_empty() {
        out.push(egui::Rect::from_min_max(
            egui::pos2(start_rect.left(), start_rect.top()),
            egui::pos2(end_rect.left(), end_rect.bottom()),
        ));
    }
    out
}

/// 绘制锯齿波浪线（给定片段几何）。
///
/// 用 `Shape::closed_line` 逐段连出锯齿而非 `hline`：实线波浪线在深色主题下
/// 视觉噪声远低于锯齿线，这也是原生编辑器的通行做法。
pub fn paint_squiggles(painter: &egui::Painter, squiggles: &[Squiggle], color: egui::Color32) {
    let stroke = egui::Stroke::new(1.0_f32, color);
    for sq in squiggles {
        let rect = sq.rect;
        // 贴在文字基线下方一点，避免与字形下缘重叠。
        let y = rect.bottom() - 1.0;
        let segments = ((rect.width() / SQUIGGLE_WAVELENGTH).ceil() as usize).max(1);
        let step = rect.width() / segments as f32;
        let mut pts: Vec<egui::Pos2> = Vec::with_capacity(segments + 1);
        for k in 0..=segments {
            let x = rect.left() + step * k as f32;
            // 用全局 x 参与相位计算，保证相邻片段的波形连续（不会在边界处错相）。
            let phase_x = x + sq.phase;
            let t = (phase_x / SQUIGGLE_WAVELENGTH) * std::f32::consts::TAU;
            pts.push(egui::pos2(x, y + t.sin() * SQUIGGLE_AMPLITUDE));
        }
        painter.add(egui::Shape::line(pts, stroke));
    }
}

/// 单行高亮统计：供单元测试断言「仅变更行重建」与调试面板显示命中率。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HighlightStats {
    /// 本次参与排版的行数。
    pub total_lines: usize,
    /// 本次真正重新排版的行数（缓存未命中）。
    pub reshaped_lines: usize,
    /// 本次命中行缓存的行数。
    pub cached_lines: usize,
}

/// 上一版交付成功的逐行排版结果（增量重排的基础）。
#[derive(Default)]
struct LineLayout {
    /// 上一版的每行内容（不含 `\n`），与 `galleys` 一一对应。
    lines: Vec<String>,
    /// 上一版每行的 Galley。
    galleys: Vec<Arc<egui::Galley>>,
    /// 上一版所用的主题签名。
    sig: Option<[u8; 4]>,
    /// 上一版所用的换行宽度。
    wrap: f32,
    /// 上一版所用的查找命中签名（无命中为 0）。
    find_sig: u64,
    /// 上一版**是否带语法着色**（OPT-011 ① 防抖期间为 `false`）。
    ///
    /// 必须进缓存键：防抖期内按纯文本排了一版，停手后文本**没变**，
    /// 若只看文本就命中缓存，新的 span 永远排不进去——
    /// 表现就是「打字后颜色再也不回来」。
    colored: bool,
    /// 是否已排版过（区分「从未排版」与「用了默认签名」）。
    valid: bool,
}

/// 大文件阈值（FR-06）：超过后停止 span 着色，只做纯文本排版。
const LARGE_FILE_BYTES: usize = 500 * 1024;

// 语法高亮 `Galley` 的线程局部缓存（S6-04）。
//
// 分两层：
// 1. **整文档缓存** `WHOLE_CACHE`：键为 `(主题签名, 全文, 换行宽度)`。
//    文本 / 主题 / 换行未变时直接返回，**空闲帧零开销**（避免每帧全量重排）。
// 2. **行布局** `LINE_LAYOUT`：保留上一版每一行的 `Galley`。文本变化时用
//    前后缀比对定位变更行，**只有真正变的行重新排版**，其余行直接复用。
//
// # 为什么不是「定容 LRU」
//
// 阶段文档 §8.3 原本设想 `LayoutJob` 定容 LRU（上限 64）。实测下来此路不通：
// `TextEdit` 的 `layouter` 每帧要拿到**整篇**的结果，访问模式是「按文档顺序请求全部行」，
// 而**任何容量小于行数的缓存面对全顺序扫描，命中率都是 0**——
// 第 0 行被挤出后，读第 0 行 miss → 插入 → 把第 1 行挤掉 → 第 1 行 miss → 雪崩，
// 退化为每次击键全量重排，等于 S6-04 白做（`repeated_line_hits_stay_cached`
// 这条用例就是用来钉死这个回归的）。
//
// 改成「显式保留上一版全部行」后：
// - 命中率与文档大小无关，永远只重排变更行；
// - 内存并不翻倍——`Galley::concat` 内部克隆的是 `Arc<Row>`，
//   行数据（glyph / mesh）与本就保留的整篇 Galley **共享**，每行只多一个薄外壳。
//
// 这是 S6-04「增量高亮」的落点。> 500KB 走降级分支：跳过 span 着色。
// 真正的「只高亮可视区」需要 viewport，而 `layouter` 接口拿不到滚动位置，
// 留待阶段 07 真机走查时再做。
/// 整文档缓存条目：`(主题签名, 全文, 换行宽度, Galley, 查找命中签名, 是否着色)`。
type WholeCacheEntry = (Option<[u8; 4]>, String, f32, Arc<egui::Galley>, u64, bool);

thread_local! {
    static WHOLE_CACHE: RefCell<Option<WholeCacheEntry>> = const { RefCell::new(None) };
    // `LineLayout::default()` / `HighlightStats::default` 非 `const fn`，故不用 `const {}` 块。
    static LINE_LAYOUT: RefCell<LineLayout> = RefCell::new(LineLayout::default());
    static LAST_STATS: RefCell<HighlightStats> = RefCell::new(HighlightStats::default());
}

/// 读取最近一次排版统计（调试面板用）。
pub fn last_highlight_stats() -> HighlightStats {
    LAST_STATS.with(|s| *s.borrow())
}

/// 当前保留的行排版条目数（调试面板显示命中率 / 规模用）。
pub fn line_cache_len() -> usize {
    LINE_LAYOUT.with(|c| c.borrow().galleys.len())
}

/// 每行**顶部**的 y 坐标（点，相对整篇 `Galley` 顶端；末元素为总高）。
///
/// S6-06 双向滚动同步要用它做「像素 ↔ 源码偏移」换算：
/// 编辑区的 `ScrollArea` 只知道滚了多少像素，而滚动同步的用例
/// （`MapScrollQuery`）要的是**字节偏移**，中间缺的正是这张表。
///
/// 数值直接来自 `LINE_LAYOUT` 里已缓存的逐行 `Galley`：
/// `Galley::concat` 就是按 `rect.height()` 逐行下移的，
/// 故累加行高与真正的行位置**完全一致**（含折行，折行会让该行变高）。
/// 空闲帧只是一次 O(行数) 的累加，不触发任何重新排版。
///
/// 返回 `None` 的情形：从未排版过、换了主题、或行数与当前文本不符
/// （例如本帧还没走到 `highlight_layout`）。调用方按「本帧无法换算」处理，
/// 下一帧自然恢复——绝不能凭空猜一个行高，那会让同步**稳定地**错位。
pub fn line_tops(theme: &Theme, text: &str) -> Option<Vec<f32>> {
    let sig = palette_signature(theme);
    let lines = text.split('\n').count();
    LINE_LAYOUT.with(|c| {
        let c = c.borrow();
        if !c.valid || c.sig != sig || c.galleys.len() != lines {
            return None;
        }
        let mut tops = Vec::with_capacity(lines + 1);
        let mut y = 0.0_f32;
        for g in &c.galleys {
            tops.push(y);
            y += g.rect.height();
        }
        tops.push(y);
        Some(tops)
    })
}

/// 清空全部高亮缓存（单元测试隔离用；切主题 / 换文档由签名与整文档键自然失效）。
pub fn clear_highlight_caches() {
    WHOLE_CACHE.with(|c| *c.borrow_mut() = None);
    LINE_LAYOUT.with(|c| *c.borrow_mut() = LineLayout::default());
    LAST_STATS.with(|s| *s.borrow_mut() = HighlightStats::default());
}

/// 构建单行 `LayoutJob`（无尾随换行；换行由 `concat` 统一补）。
///
/// `spans` 必须是**行内字节区间**且已按起点升序（调用方按此约定传入）；
/// `bg_marks` 是行内查找命中区间 `(start, end, current)`。两者合并：
/// 取边界并集后逐段决定前景色（语法样式）与是否叠加查找命中背景。
fn build_line_job(
    theme: &Theme,
    line: &str,
    spans: &[SpanView],
    bg_marks: &[(usize, usize, bool)],
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let mapper = StyleMapper::new(theme);
    let base = mapper.format("plain");
    let mut job = egui::text::LayoutJob {
        text: line.to_string(),
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

    // 语法样式区间与查找命中背景合并：取两者边界的并集，逐段决定格式。
    //
    // **所有边界必须先吸附到字符边界（DEF-04）**：span 与查找命中的坐标都来自
    // 外部（解析器 / 查找器），与 `line` 未必同源——文本已改而 span 停在上一版时，
    // 边界会落到多字节字符的中间，下面的 `&line[lo..hi]` 就会 panic。
    // 本项目 `panic = "abort"`，一次 panic 即窗口消失，故一律向下吸附：
    // 最坏只是某一段少上色，绝不崩。
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
        // 前景色：覆盖 [lo, hi) 的语法 span，否则基础色。
        let mut fmt = spans
            .iter()
            .find(|s| s.range.start <= lo && lo < s.range.end)
            .map(|s| mapper.format(&s.style))
            .unwrap_or_else(|| base.clone());
        // 查找命中背景：该段落在任一命中区间内则叠加背景。
        if let Some(&(_, _, current)) = bg_marks.iter().find(|&&(s, e, _)| s <= lo && lo < e) {
            fmt.background = if current {
                theme.find_current_bg
            } else {
                theme.find_bg
            };
        }
        job.append(&line[lo..hi], 0.0, fmt);
    }
    job
}

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

    // 1) 整文档缓存：文本 / 主题 / 换行 / 查找命中 / 着色状态未变 → 直接返回。
    if let Some((cached_sig, cached_text, cached_wrap, galley, cached_find, cached_colored)) =
        WHOLE_CACHE.with(|c| c.borrow().clone())
        && cached_sig == sig
        && cached_text == text
        && cached_wrap == wrap_width
        && cached_find == find_sig
        && cached_colored == colored
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
        ));
    });
    LAST_STATS.with(|s| *s.borrow_mut() = stats);

    combined
}

/// 语义样式 → egui 文本格式。
///
/// 未知样式**不报错**，退回普通文本色：新增一个 `SpanStyle` 变体时，
/// 表现层最多「不高亮」，不会「崩溃」或「整块变黑」。
#[derive(Clone, Copy, Debug)]
pub struct StyleMapper<'a> {
    theme: &'a Theme,
}

/// 标题级别 → 字号倍率。
///
/// 与原型 `PreviewPane.vue` 的观感对齐：`h1` 最大，逐级递减，
/// `h6` 仍略大于正文（否则大纲里的六级标题在预览里全都一样大）。
const fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.80,
        2 => 1.50,
        3 => 1.30,
        4 => 1.15,
        5 => 1.05,
        _ => 1.0,
    }
}

impl<'a> StyleMapper<'a> {
    /// 以主题构造。
    pub const fn new(theme: &'a Theme) -> Self {
        Self { theme }
    }

    /// 样式字符串 → 文本格式。
    ///
    /// `heading` 的级别信息编码在字符串里（`heading:3`），
    /// 由应用层的 `SpanStyle::Heading { level }` 格式化而来。
    ///
    /// 字号通过 [`FontId::size`] 表达——`TextFormat` 没有「相对字号」字段，
    /// 只能给出绝对磅值，故以 `body` 基准乘 [`heading_scale`]。
    pub fn format(&self, style: &str) -> TextFormat {
        let body_size = self.theme.editor_font_size;
        let base = TextFormat {
            color: self.theme.text_primary,
            font_id: egui::FontId::monospace(body_size),
            ..Default::default()
        };

        match style {
            "plain" | "" => base,

            // —— Markdown 语义 ——
            s if s == "heading" || s.starts_with("heading:") => {
                let level = s
                    .strip_prefix("heading:")
                    .and_then(|n| n.parse::<u8>().ok())
                    .unwrap_or(1);
                TextFormat {
                    color: self.theme.syn_heading,
                    font_id: egui::FontId::monospace(body_size * heading_scale(level)),
                    ..Default::default()
                }
            }
            "strong" => TextFormat {
                color: self.theme.syn_bold,
                ..base
            },
            "emphasis" => TextFormat {
                color: self.theme.syn_italic,
                italics: true,
                ..base
            },
            "strikethrough" => TextFormat {
                color: self.theme.text_secondary,
                strikethrough: hairline(self.theme.text_secondary),
                ..base
            },
            "code" => TextFormat {
                color: self.theme.syn_code,
                background: self.theme.code_bg,
                ..base
            },
            "link" => TextFormat {
                color: self.theme.syn_link,
                underline: hairline(self.theme.syn_link),
                ..base
            },
            "image" => TextFormat {
                color: self.theme.syn_meta,
                italics: true,
                ..base
            },
            "quote" => TextFormat {
                color: self.theme.syn_quote,
                italics: true,
                ..base
            },
            "list_marker" | "list-marker" => TextFormat {
                color: self.theme.syn_marker,
                ..base
            },
            "table_border" | "table-border" => TextFormat {
                color: self.theme.syn_table,
                ..base
            },
            s if s == "task_marker" || s.starts_with("task_marker:") => TextFormat {
                color: self.theme.accent,
                ..base
            },

            // —— 代码语法（来自 syntect scope 映射）——
            "keyword" | "operator" => TextFormat {
                color: self.theme.syn_italic,
                ..base
            },
            "string" => TextFormat {
                color: self.theme.success,
                ..base
            },
            "comment" => TextFormat {
                color: self.theme.text_secondary,
                italics: true,
                ..base
            },
            "number" => TextFormat {
                color: self.theme.warning,
                ..base
            },
            "function" => TextFormat {
                color: self.theme.syn_code,
                ..base
            },
            "type" => TextFormat {
                color: self.theme.syn_code,
                ..base
            },
            "variable" | "identifier" => TextFormat {
                color: self.theme.syn_other,
                ..base
            },
            "punctuation" | "meta" | "escape" => TextFormat {
                color: self.theme.syn_meta,
                ..base
            },

            // 未知样式：退回正文色，不报错（见类型文档）。
            _ => base,
        }
    }

    /// 样式 → 颜色（只要颜色时用；等价于 `format(..).color`）。
    pub fn color(&self, style: &str) -> Color32 {
        self.format(style).color
    }

    /// 标题级别的字号倍率（供预览区排版）。
    pub fn heading_scale(level: u8) -> f32 {
        heading_scale(level)
    }

    /// 预览区某类块的前景色。
    ///
    /// `kind` 里带后缀的形式（如 `raw:html`）按前缀匹配。
    pub fn block_color(&self, kind: &str) -> Color32 {
        if kind == "heading" || kind.starts_with("heading:") {
            self.theme.syn_heading
        } else if kind.starts_with("quote") {
            self.theme.syn_quote
        } else {
            self.theme.text_primary
        }
    }

    /// 标题样式字符串 → 级别（供预览区决定排版）。
    ///
    /// 块级字符串只有 `"heading"`（`BlockView::kind` 不带级别），
    /// 片段级才有 `heading:N`。
    pub fn heading_level_of(style: &str) -> Option<u8> {
        if style == "heading" {
            return Some(1);
        }
        style
            .strip_prefix("heading:")
            .and_then(|n| n.parse::<u8>().ok())
    }
}

/// 把主题写进 egui 的全局样式（字体、背景、控件配色）。
///
/// 这是「切换主题无需重启」的落点：egui 的 `Context::style_mut` 是全局的，
/// 每帧应用一次当前主题即可，窗口不必重建。
///
/// # 为什么从 `Visuals::dark()` / `Visuals::light()` 起步
///
/// 早先的实现是在「上一帧的 `visuals`」上逐字段覆盖。问题在于 `ctx.style()`
/// 的基线是 egui 默认的 `Visuals::dark()`：只要有一个字段忘了覆盖，深色默认值
/// 就会被带进浅色主题。`text_cursor` 正是这样一个字段——它的默认 caret 是浅蓝
/// `rgb(192, 222, 255)`，落在白色画布上几乎不可见，用户看到的就是「聚焦后
/// 看不见光标」。同类字段还有 `override_text_color`、`window_stroke` 等。
///
/// 改为按明暗模式取基线后，两个方向都不会再互相污染，可读性也不再依赖
/// 「记得把每个字段都覆盖一遍」这种易碎前提。
pub fn apply(ctx: &egui::Context, theme: &Theme) {
    let mut style = (*ctx.style()).clone();

    let mut visuals = if theme.is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    visuals.dark_mode = theme.is_dark();
    visuals.panel_fill = theme.bg_secondary;
    visuals.window_fill = theme.bg_secondary;
    visuals.extreme_bg_color = theme.bg_primary;
    visuals.faint_bg_color = theme.bg_tertiary;
    visuals.code_bg_color = theme.code_bg;
    visuals.hyperlink_color = theme.accent;

    // 选区：底色 + **选区内的文字色**。
    //
    // egui 会把被选中文字的顶点色强制改写成 `selection.stroke.color`
    // （见 `text_selection::visuals::paint_text_selection`）。若这里给强调色，
    // 选中文字就变成「强调色压在强调色底上」——浅色主题下几乎糊成一片。
    // 故选区文字固定用正文色，保证**选中态**同样清晰可读。
    visuals.selection.bg_fill = theme.selection_bg;
    visuals.selection.stroke = hairline(theme.text_primary);

    // 光标：显式按主题指定，不再依赖 egui 的「深色模式专用」默认值。
    visuals.text_cursor.stroke = egui::Stroke::new(2.0_f32, theme.accent);

    // 次要文字色显式给出，避免默认的「对正文色二次降亮」把次要文字压到不可读。
    visuals.weak_text_color = Some(theme.text_secondary);

    // 控件圆角统一到原型的 `--radius-btn`（2026-10 精修后为 6px）。
    let button_radius =
        egui::CornerRadius::same(theme.metrics.radius_button.round().clamp(0.0, 255.0) as u8);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = button_radius;
    }

    visuals.widgets.noninteractive.bg_stroke = hairline(theme.border);
    visuals.widgets.noninteractive.weak_bg_fill = theme.bg_tertiary;
    visuals.widgets.noninteractive.bg_fill = theme.bg_tertiary;
    visuals.widgets.noninteractive.fg_stroke = hairline(theme.text_secondary);
    visuals.widgets.inactive.bg_fill = theme.bg_tertiary;
    visuals.widgets.inactive.weak_bg_fill = theme.bg_tertiary;
    visuals.widgets.inactive.bg_stroke = hairline(theme.border);
    visuals.widgets.inactive.fg_stroke = hairline(theme.text_primary);
    visuals.widgets.hovered.bg_fill = theme.bg_tertiary;
    visuals.widgets.hovered.weak_bg_fill = theme.accent_soft;
    visuals.widgets.hovered.bg_stroke = hairline(theme.accent);
    visuals.widgets.hovered.fg_stroke = hairline(theme.text_primary);
    visuals.widgets.active.bg_fill = theme.accent_soft;
    visuals.widgets.active.weak_bg_fill = theme.accent_soft;
    visuals.widgets.active.bg_stroke = hairline(theme.accent);
    visuals.widgets.active.fg_stroke = hairline(theme.text_primary);
    visuals.widgets.open.bg_fill = theme.bg_tertiary;
    visuals.widgets.open.bg_stroke = hairline(theme.border);
    visuals.widgets.open.fg_stroke = hairline(theme.text_primary);
    visuals.error_fg_color = theme.error;
    visuals.warn_fg_color = theme.warning;

    style.visuals = visuals;

    // 间距：原型栅格以 4px 为基准（`--sp-1`）。把 egui 的控件间距与按钮内边距
    // 收敛到同一套节奏，避免桌面端比原型「松/紧一档」。
    // 两个值都从 [`Metrics`] 的间距刻度取——全局默认值与各视图的显式留白
    // 必须是同一个来源，否则「统一间距规范」只管得住显式调用、管不住默认值。
    let m = &theme.metrics;
    style.spacing.item_spacing = egui::vec2(m.space_md, m.space_xs);
    style.spacing.button_padding = egui::vec2(m.space_md, m.space_xs);

    // 字号：编辑区用等宽，其余用 UI 字体。
    for (font, size) in [
        (egui::TextStyle::Monospace, theme.editor_font_size),
        (egui::TextStyle::Body, theme.metrics.preview_font_size),
        (egui::TextStyle::Button, theme.metrics.ui_font_size),
        (
            egui::TextStyle::Small,
            (theme.metrics.ui_font_size - 1.0).max(8.0),
        ),
        (
            egui::TextStyle::Heading,
            theme.metrics.preview_font_size * 1.4,
        ),
    ] {
        // `Style::text_styles` 的值是 `FontId`（字段 `size` / `family`），
        // 不是独立结构体。保留 egui 调校过的其它设置，只改字号与字体族。
        let mut font_id = style.text_styles.get(&font).cloned().unwrap_or_default();
        font_id.size = size;
        if font == egui::TextStyle::Monospace {
            font_id.family = egui::FontFamily::Monospace;
        }
        style.text_styles.insert(font, font_id);
    }

    ctx.set_style(style);
}

/// 1px 实线描边（发丝线）。
///
/// 全文件统一走这里，避免每个 `Stroke::new(1.0, ...)` 都触发一次
/// `f32`/`f64` 字面量推断的回退警告。
fn hairline(color: egui::Color32) -> egui::Stroke {
    egui::Stroke::new(1.0_f32, color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Color32;
    use egui::text::{LayoutJob, LayoutSection};

    /// 可用于排版的 `Context`（先跑空帧装好字库，否则 `fonts_mut` 会 panic）。
    fn test_ctx() -> egui::Context {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        ctx
    }

    /// 取某段覆盖的文本（egui 0.33 的 `LayoutSection` 用 `byte_range` 索引 `job.text`）。
    fn section_text<'a>(job: &'a LayoutJob, s: &LayoutSection) -> &'a str {
        job.text.get(s.byte_range.clone()).unwrap_or("")
    }

    /// 查找命中段必须叠加背景，非命中段保持透明。
    #[test]
    fn build_line_job_applies_find_background() {
        let theme = Theme::dark();
        // "hello world" 中命中字节 6..11（"world"）。
        let job = build_line_job(&theme, "hello world", &[], &[(6, 11, false)], 0.0);
        let world = job
            .sections
            .iter()
            .find(|s| section_text(&job, s) == "world")
            .expect("应有 world 段");
        assert_ne!(
            world.format.background,
            Color32::TRANSPARENT,
            "查找命中段必须叠加背景"
        );
        let hello = job
            .sections
            .iter()
            .find(|s| section_text(&job, s) == "hello ")
            .expect("应有 hello 段");
        assert_eq!(
            hello.format.background,
            Color32::TRANSPARENT,
            "非命中段不应有背景"
        );
    }

    /// 当前命中背景应区别于普通命中（更醒目）。
    #[test]
    fn current_mark_uses_distinct_background() {
        let theme = Theme::dark();
        let plain = build_line_job(&theme, "hello world", &[], &[(6, 11, false)], 0.0);
        let current = build_line_job(&theme, "hello world", &[], &[(6, 11, true)], 0.0);
        let plain_bg = plain
            .sections
            .iter()
            .find(|s| section_text(&plain, s) == "world")
            .unwrap()
            .format
            .background;
        let current_bg = current
            .sections
            .iter()
            .find(|s| section_text(&current, s) == "world")
            .unwrap()
            .format
            .background;
        assert_ne!(plain_bg, current_bg, "当前命中背景应区别于普通命中");
    }

    /// 语法前景色与查找背景可共存：命中段若同时是语法 span，颜色不丢。
    #[test]
    fn find_background_does_not_clobber_syntax_foreground() {
        let theme = Theme::dark();
        let spans = vec![SpanView {
            range: RangeView::new(6, 11),
            style: "strong".to_string(),
        }];
        let job = build_line_job(&theme, "hello world", &spans, &[(6, 11, false)], 0.0);
        let world = job
            .sections
            .iter()
            .find(|s| section_text(&job, s) == "world")
            .expect("应有 world 段");
        assert_ne!(
            world.format.background,
            Color32::TRANSPARENT,
            "命中段应有背景"
        );
        assert_eq!(
            world.format.color, theme.syn_bold,
            "命中段不应丢失语法前景色"
        );
    }

    /// `find_sig` 必须并入缓存键：不同命中集要失效重建，相同命中集要命中整文档缓存。
    #[test]
    fn highlight_layout_keys_on_find_signature() {
        clear_highlight_caches();
        let ctx = test_ctx();
        let theme = Theme::dark();
        let text = "the cat sat on the mat";
        let marks = vec![FindMark {
            start: 4,
            end: 7,
            current: true,
        }];

        let none = highlight_layout_with_find(&ctx, &theme, text, None, 400.0, None);
        let with = highlight_layout_with_find(&ctx, &theme, text, None, 400.0, Some(&marks));
        assert!(
            !std::sync::Arc::ptr_eq(&none, &with),
            "有无查找命中必须失效重建（否则旧高亮残留）"
        );
        let with_again = highlight_layout_with_find(&ctx, &theme, text, None, 400.0, Some(&marks));
        assert!(
            std::sync::Arc::ptr_eq(&with, &with_again),
            "相同命中集应命中整文档缓存（空闲帧零开销）"
        );
    }

    /// DEF-04：span 边界落在多字节字符中间时不得 panic（`panic = "abort"`）。
    ///
    /// 生产路径每帧重新高亮，span 与文本天然同源，故这个缺陷从未触发；
    /// 但「同源」只是约定不是保证——缓存版本错位、手写区间都会打破它。
    /// 表现层是最后一道防线：宁可少上一段色，也不能崩。
    #[test]
    fn build_line_job_survives_non_char_boundary_spans() {
        let theme = Theme::dark();
        // 中=0..3、文=3..6、a=6、b=7、c=8 → 1 与 5 都在字符内部。
        let line = "中文abc";
        assert!(
            !line.is_char_boundary(1) && !line.is_char_boundary(5),
            "用例前提：这两个偏移必须是非字符边界"
        );
        let spans = vec![SpanView {
            range: RangeView::new(1, 5),
            style: "strong".to_string(),
        }];
        let job = build_line_job(&theme, line, &spans, &[], 0.0);
        // 走到这里即未 panic；再断言文本被完整覆盖（少上色可以，丢字符不行）。
        let covered: String = job.sections.iter().map(|s| section_text(&job, s)).collect();
        assert_eq!(covered, line, "吸附边界后不得丢字符");
    }

    /// 查找命中区间同样可能被算到字符中间（DEF-04 的同源风险）。
    #[test]
    fn build_line_job_survives_non_char_boundary_find_marks() {
        let theme = Theme::dark();
        let line = "中文abc";
        let job = build_line_job(&theme, line, &[], &[(1, 5, true)], 0.0);
        let covered: String = job.sections.iter().map(|s| section_text(&job, s)).collect();
        assert_eq!(covered, line, "吸附边界后不得丢字符");
    }

    /// 端到端：span 来自「文本的上一版」（长度已变）时，整篇排版不得 panic。
    #[test]
    fn highlight_layout_survives_spans_from_older_text() {
        clear_highlight_caches();
        let ctx = test_ctx();
        let theme = Theme::dark();
        let older = "列表项一";
        // 新版更长 → 旧 span 的坐标与文本整体错位。
        let newer = "列表项壹拾贰";
        let spans = vec![SpanView {
            range: RangeView::new(1, older.len()),
            style: "strong".to_string(),
        }];
        let g = highlight_layout(&ctx, &theme, newer, Some(&spans), 900.0);
        assert!(g.rect.height() > 0.0, "排版应产出非空结果");
    }

    /// DEF-05（回归）：`highlight_layout*` 的产出不得被 `Galley::is_empty()` 判为空。
    ///
    /// `Galley::concat` 只把传入的 `job` 原样搬到合并结果上，而下游两处都以
    /// `galley.is_empty()`（≡ `job.sections.is_empty()`）作为「整篇为空、不画」的判据：
    /// `egui::Painter::galley` 与 `epaint::Tessellator::tessellate_text`。
    /// 于是「逐行 Galley 排得完全正确、编辑区却一个字都不画」是可能的——
    /// 只要 `sections` 被留空，`Shape::galley` 根本进不了 `PaintList`。
    ///
    /// 这条用例直接把该前提钉死：文本非空 ⇒ 整篇 Galley 非空。
    /// 它不依赖 egui 渲染，因此比 `tests/presentation/editor_view_test.rs`
    /// 更早、更准地定位到这一类回归。
    #[test]
    fn highlight_layout_output_is_not_empty_for_non_empty_text() {
        clear_highlight_caches();
        let ctx = test_ctx();
        let text = "# 标题\n\n正文一行。\n第二行 `code`";
        let spans = vec![SpanView {
            range: RangeView::new(0, 5),
            style: "heading:1".to_string(),
        }];

        // 两个主题都要过：该缺陷与配色无关，任何主题下都是白板。
        for theme in [Theme::dark(), Theme::light()] {
            let g = highlight_layout(&ctx, &theme, text, Some(&spans), 900.0);
            assert!(
                !g.is_empty(),
                "整篇 Galley 被判为空 → Painter::galley 会跳过绘制（编辑区白板）"
            );
            assert_eq!(
                g.text(),
                text,
                "整篇 Galley 必须携带全文（选中/查找依赖它）"
            );
            assert!(g.rect.height() > 0.0, "排版应产出非零高度");
        }

        // 不带 span（防抖期 / 大文件降级）走的也是同一条拼接路径。
        clear_highlight_caches();
        let plain = highlight_layout(&ctx, &Theme::dark(), "纯文本", None, 900.0);
        assert!(!plain.is_empty(), "无 span 的降级路径同样不得被判为空");
        assert_eq!(plain.text(), "纯文本");

        // 带查找命中（编辑区 Ctrl+F 高亮）的路径同理。
        clear_highlight_caches();
        let marks = vec![FindMark {
            start: 0,
            end: 3,
            current: true,
        }];
        let found =
            highlight_layout_with_find(&ctx, &Theme::dark(), "纯文本", None, 900.0, Some(&marks));
        assert!(!found.is_empty(), "带查找命中的路径同样不得被判为空");
    }
}
