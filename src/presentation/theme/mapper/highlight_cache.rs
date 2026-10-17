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
///
/// # 所有权约定：每次排版把它**整条移出**，用完写回（问题 052）
///
/// [`super::highlight_layout::highlight_layout_with_find`] 用 `mem::take` 取走本结构
/// （而不是 `lines.clone()` + `galleys.clone()`），比对与构建完成后把新一版写回。
/// 理由：这份数据在下一行就要被整体替换，克隆出来的旧表只用于逐行比对、比完即弃；
/// 1MB / 58k 行文档下每次击键白白付 58k 次小堆分配（微基准 2.47ms）与 58k 次
/// `Arc` 克隆（0.44ms）。
///
/// 代价是本结构在排版期间短暂处于 `valid = false` 的默认态。排版不会被重入
/// （`line_tops` 只从 UI 侧调用，与 layouter 不在同一调用栈），故无风险。
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
    /// 上一版所用的字体图集尺寸令牌（见 [`atlas_token`]）。
    ///
    /// egui 重建 `Fonts` 时只清自己的 galley 缓存，本结构跨帧持有的
    /// `Arc<Galley>` 全部失效；令牌变了就必须整篇重排重新取字形坐标。
    pub(super) atlas: u64,
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
// 1. **整文档缓存** `WHOLE_CACHE`：键为 `(主题签名, 全文, 换行宽度, 查找命中,
//    着色状态, 排版度量, 图集令牌)`——其中「全文」取自整篇 Galley 的 `job.text`，
//    不再另存一份（问题 052，见 [`WholeCacheEntry`]）。
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
/// 整文档缓存条目：`(主题签名, 换行宽度, Galley, 查找命中签名, 是否着色, 排版度量签名, 图集令牌)`。
///
/// # 为什么**不**单独存一份全文（问题 052）
///
/// 缓存键需要「全文未变」这个判据。这里刻意不存 `String`——拼回来的整篇
/// `Galley` 本身就携带着全文（`job.text`；双击选词依赖它，见 [`egui::Galley::text`]），
/// 故 `galley.text() == text` 与「另存一份再比」**完全等价**（O(1) 取引用 +
/// 逐字节精确比较），却省掉三笔开销：
///
/// - 每帧（含空闲帧）一次 1MB 复制——早先的写法是 `c.borrow().clone()`，
///   它复制的是**整条条目**（`Ref<T>` 不实现 `Clone`，克隆的是 `T` 本身）；
/// - 每击键一次 `text.to_string()`（新文本回写缓存）；
/// - 1MB 常驻内存（同一份文本在 `WHOLE_CACHE` 与整篇 Galley 里各一份）。
///
/// 也刻意**不用哈希**当键：哈希碰撞会让「新文本误命中旧条目」→ 残留旧字形的
/// 颜色与坐标，而本项目的确定性纪律偏保守（同 [`atlas_token`] 的取舍）。
pub(super) type WholeCacheEntry = (Option<[u8; 4]>, f32, Arc<egui::Galley>, u64, bool, u64, u64);

/// 编辑区**排版度量**签名：字号 / 行高 / 字体族。
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
///
/// # 字体族也必须进签名（S6-06 字体选择）
///
/// 同理适用于字体族：用户在设置里换了编辑区字体后，`set_fonts` 重建字体，
/// 但逐行 Galley 的缓存键（配色 / 文本 / 行宽 / 度量）**一切照旧** →
/// 缓存命中 → 整篇继续用旧字体的字形——表现为「换了字体但编辑区没变」，
/// 与字号那条同源。字体族的字节折叠进同一签名即可（空串也有稳定哈希）。
pub(super) fn editor_metrics_signature(theme: &Theme) -> u64 {
    let size = (theme.editor_font_size * 100.0).round() as u64;
    let line = (theme.editor_line_height * 1000.0).round() as u64;
    let mut h: u64 = 1469598103934665603;
    for v in [size, line] {
        h ^= v;
        h = h.wrapping_mul(1099511628211);
    }
    for b in theme.editor_font_family.as_bytes() {
        h ^= u64::from(*b);
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

/// 字体图集尺寸令牌：`Fonts::font_image_size()`（`[宽, 高]`）折叠成 u64。
///
/// # 为什么两层缓存键里必须有它（V11-17）
///
/// egui 的 `Fonts::begin_pass` 会**整体重建 `Fonts` 与字体图集**，触发条件
/// 有三个：`max_texture_side` 变、`text_alpha_from_coverage` 变（该值已被
/// `theme::mapper::apply` 钉死，见那儿的长注释）、或**图集填充率超过 80%**。
/// 重建只 flush egui 自己的 galley 缓存，本模块跨帧持有的 `Arc<Galley>`
/// 不会被告知——其字形 `uv_rect` 全部失效，而缓存键（配色签名 / 文本 /
/// 换行宽度 / 度量）一切照旧 → 缓存命中 → 拿旧坐标采样新图集 →
/// **中文渲染成错码**（V11-17：切主题后编辑区乱码、预览区却正常）。
///
/// 图集尺寸是重建的**可靠公开探针**：图集高度按 32→64→…→2048 翻倍增长
/// （epaint `TextureAtlas::resize_to_min_height`），填充率 >80% 时高度
/// 必然已顶到 2048，重建后归 32 → 尺寸必然变化 → 缓存失效 → 整篇重排，
/// 重新向 egui 取一遍字形坐标即恢复。日常编辑不引入新字形时尺寸不变，
/// 增量命中不受影响（一生顶多多重排几次）。
pub fn atlas_token(ctx: &egui::Context) -> u64 {
    let [w, h] = ctx.fonts(|f| f.font_image_size());
    ((w as u64) << 32) | (h as u64)
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
///
/// 刻意**不**校验字体图集令牌（[`atlas_token`]）：本函数只消费行**高**，
/// 而图集重建（egui `Fonts::begin_pass`）不改变任何字体度量与行高，
/// 旧表对滚动换算依然成立；令牌该管的是字形坐标（见 `highlight_layout`）。
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
    // `text` 必须留空：下面所有文本都经 `LayoutJob::append` 写入，而 `append`
    // 会把它推进 `text` 并据此计算 `byte_range`。egui 0.37 起 `Fonts::layout_job`
    // 会做 `debug_sanity_check`——要求各 `section` 从字节 0 起、无缝隙地铺满
    // `job.text`；若这里预填 `line`，首个 section 会从 `line.len()` 开始，
    // 断言即失败（0.33 无此断言，故旧代码侥幸可用）。
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
    // 空行（line 为空）必须保留一个「空 section」，否则 egui 会把该行排成 0 高度：
    // 见 epaint `text_layout.rs` —— `Paragraph::empty_paragraph_height` 只在
    // `layout_section` 处理到**非空** section 时才被置为 `line_height`；纯空 job
    // 不触发任何 `layout_section`（没有 section 可迭代），空段落高度维持默认 0.0，
    // 于是整行高度为 0。拼回整文档 `Galley` 后，连续回车产生的空行统统不可见，
    // 行号栏又按 0 高度空行定位 → 与正文错位。这正是 V11-16「连续 N 次回车只显示
    // 一个空行、须再打字符才换行」的真正根因——与 IME / 回车事件无关，是自定义
    // 逐行排版器对空行高度处理的缺失。补一个空 section 即可让 egui 正确赋予行高。
    if job.sections.is_empty() {
        job.append(line, 0.0, base);
    }
    job
}
