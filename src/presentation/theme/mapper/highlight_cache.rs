//! 语法高亮 `Galley` 的线程局部两层缓存与单行排版作业构造。

use std::cell::RefCell;
use std::sync::Arc;

use egui::text::TextWrapping;

use super::*;
use crate::application::dto::SpanView;
use crate::presentation::theme::builtin::palette_signature;
use crate::shared::geometry::floor_char_boundary;

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
pub(super) struct LineLayout {
    /// 上一版的每行内容（不含 `\n`），与 `galleys` 一一对应。
    pub(super) lines: Vec<String>,
    /// 上一版每行的 Galley。
    pub(super) galleys: Vec<Arc<egui::Galley>>,
    /// 上一版所用的主题签名。
    pub(super) sig: Option<[u8; 4]>,
    /// 上一版所用的换行宽度。
    pub(super) wrap: f32,
    /// 上一版所用的查找命中签名（无命中为 0）。
    pub(super) find_sig: u64,
    /// 上一版所用的**排版度量**签名（字号 / 行高，见 [`editor_metrics_signature`]）。
    pub(super) metrics: u64,
    /// 上一版**是否带语法着色**（OPT-011 ① 防抖期间为 `false`）。
    ///
    /// 必须进缓存键：防抖期内按纯文本排了一版，停手后文本**没变**，
    /// 若只看文本就命中缓存，新的 span 永远排不进去——
    /// 表现就是「打字后颜色再也不回来」。
    pub(super) colored: bool,
    /// 是否已排版过（区分「从未排版」与「用了默认签名」）。
    pub(super) valid: bool,
}

/// 大文件阈值（FR-06）：超过后停止 span 着色，只做纯文本排版。
pub(super) const LARGE_FILE_BYTES: usize = 500 * 1024;

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
/// 整文档缓存条目：`(主题签名, 全文, 换行宽度, Galley, 查找命中签名, 是否着色, 排版度量签名)`。
pub(super) type WholeCacheEntry = (
    Option<[u8; 4]>,
    String,
    f32,
    Arc<egui::Galley>,
    u64,
    bool,
    u64,
);

/// 编辑区**排版度量**签名：字号与行高。
///
/// # 为什么要单独一个签名（不能塞进 [`palette_signature`]）
///
/// `palette_signature` 刻意只含颜色，且有测试钉住「改字号不改变配色签名」
/// （预览区靠它区分「配色变了」与「只是字号变了」，避免无谓重排）。
/// 但编辑区正相反：字号与行高**就是**排版输入——缓存若只看配色，
/// 用户在设置里改字号 / 行高后，逐行 Galley 会因「行内容没变」被整批复用，
/// 表现是「改了设置但编辑区没变」，必须重开文档才生效。
///
/// 故此处把两者量化（字号 ×100、行高 ×1000，取整避免浮点抖动）后折叠成
/// 一个 u64 并进两层缓存的键。
pub(super) fn editor_metrics_signature(theme: &Theme) -> u64 {
    let size = (theme.editor_font_size * 100.0).round() as u64;
    let line = (theme.editor_line_height * 1000.0).round() as u64;
    let mut h: u64 = 1469598103934665603;
    for v in [size, line] {
        h ^= v;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

thread_local! {
    pub(super) static WHOLE_CACHE: RefCell<Option<WholeCacheEntry>> = const { RefCell::new(None) };
    // `LineLayout::default()` / `HighlightStats::default` 非 `const fn`，故不用 `const {}` 块。
    pub(super) static LINE_LAYOUT: RefCell<LineLayout> = RefCell::new(LineLayout::default());
    pub(super) static LAST_STATS: RefCell<HighlightStats> = RefCell::new(HighlightStats::default());
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
    let metrics = editor_metrics_signature(theme);
    let lines = text.split('\n').count();
    LINE_LAYOUT.with(|c| {
        let c = c.borrow();
        if !c.valid || c.sig != sig || c.metrics != metrics || c.galleys.len() != lines {
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
pub(super) fn build_line_job(
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
