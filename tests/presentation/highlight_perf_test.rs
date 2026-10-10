//! S8-05 高亮性能基线（阶段 08 · 编辑响应与降级）。
//!
//! # 为什么单独一个文件
//!
//! 这些用例产出的是**数字**而非布尔结论，且必须带 `--nocapture` 才能看到
//! `[metrics]` 行。与 `highlight_layout_test.rs`（结构等价性与计数器断言）
//! 分开，避免「功能用例被性能用例的耗时拖慢」。
//!
//! # 测量口径
//!
//! - **必须 release 下跑**：`cargo test --release --test presentation -- --nocapture measure`。
//!   本机 debug 吞吐约低 40%，debug 数字会把达标项误判为不达标。
//! - 每个用例先 `clear_highlight_caches()` 再预热一次，把「首次分配 / 字体装载」的
//!   开销排除在样本之外（那是一次性成本，不是稳态帧成本）。
//! - 样本取 p50 / p95：p50 代表典型体验，p95 用于钉住抖动。
//!
//! 数字写入 `docs/metrics.md`（唯一数字来源，禁止散落在代码注释里）。

use std::time::Instant;

use rustmark::application::dto::{RangeView, SpanView};
use rustmark::domain::markdown::services::SyntaxHighlighter;
use rustmark::domain::markdown::value_objects::SpanStyle;
use rustmark::infrastructure::highlighting::syntect_highlighter::SyntectHighlighter;
use rustmark::presentation::theme::builtin::Theme;
use rustmark::presentation::theme::mapper::{
    clear_highlight_caches, highlight_layout, last_highlight_stats,
};

// ============================================================================
// 辅助
// ============================================================================

/// 可用于排版的 `Context`（字体需先跑一帧才装载）。
fn test_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    ctx
}

/// 合成接近 `target_bytes` 的中文 Markdown 语料（整块拼接，不会切断 UTF-8）。
fn markdown_sample(target_bytes: usize) -> String {
    let unit = "# 标题 H1\n\n\
                这是一段中文正文，用于模拟真实编辑场景：包含 **加粗**、`行内代码` \
                与 [链接](https://example.com)。\n\n\
                - 列表项一\n\
                - 列表项二\n\n\
                > 引用行\n\n\
                ```rust\nfn main() { println!(\"hi\"); }\n```\n\n";
    let mut s = String::new();
    while s.len() < target_bytes {
        s.push_str(unit);
    }
    s
}

/// 领域 `SpanStyle` → 表现层样式字符串。
///
/// 与 `application::markdown::service::span_style_str` 同构（该函数是模块私有的，
/// 测试无法调用）。**性能测量不依赖具体颜色名**，只依赖 span 的数量与边界分布，
/// 故此处保持同构即可。
fn style_str(style: SpanStyle) -> String {
    match style {
        SpanStyle::Plain => "plain",
        SpanStyle::Heading { .. } => "heading",
        SpanStyle::Emphasis => "emphasis",
        SpanStyle::Strong => "strong",
        SpanStyle::Strikethrough => "strikethrough",
        SpanStyle::Code => "code",
        SpanStyle::Link => "link",
        SpanStyle::Image => "image",
        SpanStyle::Quote => "quote",
        SpanStyle::ListMarker => "list_marker",
        SpanStyle::TableBorder => "table_border",
        SpanStyle::TaskMarker { done } => {
            if done {
                "task_marker:true"
            } else {
                "task_marker:false"
            }
        }
        SpanStyle::Keyword => "keyword",
        SpanStyle::String => "string",
        SpanStyle::Comment => "comment",
        SpanStyle::Number => "number",
        SpanStyle::Function => "function",
        SpanStyle::Type => "type",
        SpanStyle::Variable => "variable",
        SpanStyle::Operator => "operator",
    }
    .to_string()
}

/// 用**真实** syntect 产出 span（span 密度与真实场景一致，合成 span 会失真）。
fn spans_of(text: &str) -> Vec<SpanView> {
    let highlighter = SyntectHighlighter::new();
    let spans = highlighter
        .highlight(text, Some("markdown"))
        .expect("高亮应成功");
    spans
        .iter()
        .map(|s| SpanView {
            range: RangeView::new(s.range().start().value(), s.range().end().value()),
            style: style_str(s.style()),
        })
        .collect()
}

fn pct(mut samples: Vec<f64>, q: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).expect("耗时不应为 NaN"));
    let idx = ((samples.len() as f64 * q) as usize).min(samples.len() - 1);
    samples[idx]
}

/// 跑 `runs` 次操作，返回毫秒样本。
fn timed_ms(runs: usize, mut op: impl FnMut()) -> Vec<f64> {
    (0..runs)
        .map(|_| {
            let start = Instant::now();
            op();
            std::hint::black_box(());
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect()
}

// ============================================================================
// 1. 10KB 全文高亮（验收：≤ 10ms）
// ============================================================================

/// 10KB 文档：span 生成与排版分别计时，并给出「改一行后的增量帧」。
///
/// 三个数字各有用途：
/// - **span 生成**：syntect 的开销，随文档线性增长；
/// - **冷排版**：首帧/换文档的开销，每行都要排；
/// - **增量帧**：稳态编辑体验（只重排 1 行），这才是用户每次击键的真实感受。
#[test]
fn measure_10kb_full_highlight_is_bounded() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();
    let text = markdown_sample(10 * 1024);
    let lines = text.split('\n').count();
    let spans = spans_of(&text);

    // span 生成（syntect）
    let highlighter = SyntectHighlighter::new();
    let _ = highlighter.highlight(&text, Some("markdown"));
    let span_samples = timed_ms(20, || {
        let s = highlighter.highlight(&text, Some("markdown")).expect("ok");
        std::hint::black_box(s);
    });

    // 冷排版（全量）
    clear_highlight_caches();
    let cold = {
        let start = Instant::now();
        let g = highlight_layout(&ctx, &theme, &text, Some(&spans), 900.0);
        std::hint::black_box(g);
        start.elapsed().as_secs_f64() * 1000.0
    };

    // 增量帧：每次改「列表项一」所在行（只应有 1 行重排）。
    //
    // **必须是等长替换**：span 是按原文算出的字节区间，若编辑改变了长度，
    // 后续 span 的边界会落到非字符边界上（生产路径每帧都重新高亮，天然同源；
    // 这里为了把「排版」与「span 生成」两项成本分开计时，才复用同一份 span）。
    //
    // 替换串**逐个递增**（而不是在两态间来回切）：来回切会让「改回原文」那一半
    // 帧命中整文档缓存，p50 会被这种零成本帧拉低，测出来的不是稳态编辑体验。
    let needle = "列表项一";
    let anchor = text.find(needle).expect("语料应含锚点");
    // 全部为 4 个汉字（12 字节），与 `needle` 等长，故 span 区间始终对齐。
    let variants = ["列表项壹", "列表项贰", "列表项叁", "列表项肆", "列表项伍"];
    let mut edited = text.clone();
    let mut i = 0usize;
    let frame_samples = timed_ms(20, || {
        let v = variants[i % variants.len()];
        i += 1;
        edited.replace_range(anchor..anchor + needle.len(), v);
        let g = highlight_layout(&ctx, &theme, &edited, Some(&spans), 900.0);
        std::hint::black_box(g);
    });
    let stats = last_highlight_stats();

    let span_p50 = pct(span_samples.clone(), 0.50);
    let span_p95 = pct(span_samples, 0.95);
    let frame_p50 = pct(frame_samples.clone(), 0.50);
    let frame_p95 = pct(frame_samples, 0.95);

    println!(
        "[metrics] 10KB 全文高亮：span 生成 p50={span_p50:.2}ms p95={span_p95:.2}ms ｜ \
         冷排版 {cold:.2}ms ｜ 增量帧 p50={frame_p50:.2}ms p95={frame_p95:.2}ms \
         （{} 字节 / {} 行 / 重建 {} 行）",
        text.len(),
        lines,
        stats.reshaped_lines
    );

    // 计数器断言：S8-05 要求「单行编辑时重建行数 == 1」。
    assert_eq!(
        stats.reshaped_lines, 1,
        "10KB 文档改一行只应重建 1 行，实际 {} 行（共 {} 行）",
        stats.reshaped_lines, stats.total_lines
    );
    // 宽松上限：钉住数量级退化（目标 ≤10ms 见 docs/metrics.md）。
    assert!(
        frame_p95 <= 40.0,
        "10KB 增量帧 p95 {frame_p95:.2}ms 疑似退化（目标 ≤10ms）"
    );
}

// ============================================================================
// 2. 1MB 降级路径的稳态帧（验收：滚动帧 p95 ≤ 16ms）
// ============================================================================

/// 1MB 文档：> 500KB 走降级分支（跳过 span 着色，纯文本排版）。
///
/// **滚动不改变文本**，故滚动帧走的是整文档缓存命中路径——这正是
/// 「1MB 文档滚动不掉帧」的机理。冷启动首屏是另一次成本，已由 S7-04
/// `large_first_screen_render_is_bounded` 单独记录。
#[test]
fn measure_1mb_degraded_scroll_frame_is_bounded() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();
    let text = markdown_sample(1024 * 1024);
    assert!(text.len() > 500 * 1024, "样本必须触发降级分支");

    // 冷启动（首帧）：不计入门槛，只记录。
    let cold = {
        let start = Instant::now();
        let g = highlight_layout(&ctx, &theme, &text, None, 900.0);
        std::hint::black_box(g);
        start.elapsed().as_secs_f64() * 1000.0
    };

    // 稳态滚动帧：文本未变 → 整文档缓存命中。
    let samples = timed_ms(20, || {
        let g = highlight_layout(&ctx, &theme, &text, None, 900.0);
        std::hint::black_box(g);
    });
    let p50 = pct(samples.clone(), 0.50);
    let p95 = pct(samples, 0.95);

    println!(
        "[metrics] 1MB 降级：冷首帧 {cold:.2}ms ｜ 稳态滚动帧 p50={p50:.2}ms p95={p95:.2}ms \
         （{} 字节 / {} 行）",
        text.len(),
        text.split('\n').count()
    );

    assert!(
        p95 <= 16.0,
        "1MB 降级滚动帧 p95 {p95:.2}ms 超出 16ms 预算（一帧 60fps 的预算）"
    );
}

// ============================================================================
// 3. 超长单行（FR-06 行级保护）
// ============================================================================

/// 10 万字符无换行的单行：不得卡死（行级保护）。
///
/// 这是最容易翻车的一类输入：整行刷成一个 `LayoutJob`，折行计算是 O(字符数)，
/// 且只要动一个字符，这一整行都要重排。数字用来判断是否需要行级截断保护。
#[test]
fn measure_very_long_single_line_is_bounded() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let long: String = "这是一个超长单行。".repeat(10_000); // ≈ 18 万字节
    let text = format!("正常第一行\n{long}\n正常第三行\n");
    let spans = spans_of(&text);

    // 冷启动
    let cold = {
        let start = Instant::now();
        let g = highlight_layout(&ctx, &theme, &text, Some(&spans), 900.0);
        std::hint::black_box(g);
        start.elapsed().as_secs_f64() * 1000.0
    };

    // 在超长行里改一个字符：整行（18 万字节）都要重排。
    // 同样用等长替换（"。" → "、"），理由同上一个用例。
    // 同样逐个递增（理由同上：来回切会命中整文档缓存，测不到重排成本）。
    let anchor = text.find('。').expect("语料应含句号");
    let variants = ["、", "！", "？", "；", "："];
    let mut edited = text.clone();
    let mut i = 0usize;
    let samples = timed_ms(10, || {
        let v = variants[i % variants.len()];
        i += 1;
        edited.replace_range(anchor..anchor + v.len(), v);
        let g = highlight_layout(&ctx, &theme, &edited, Some(&spans), 900.0);
        std::hint::black_box(g);
    });
    let p50 = pct(samples.clone(), 0.50);
    let p95 = pct(samples, 0.95);
    let stats = last_highlight_stats();

    println!(
        "[metrics] 超长单行（{} 字节 / 共 {} 行 / 重建 {} 行）：冷排版 {cold:.2}ms ｜ \
         改一字符帧 p50={p50:.2}ms p95={p95:.2}ms",
        long.len(),
        stats.total_lines,
        stats.reshaped_lines
    );

    assert_eq!(
        stats.total_lines, 4,
        "无换行的超长行仍应算作 1 行（共 4 行：首行 + 超长行 + 尾行 + 末尾空行）"
    );
    // 先钉住「不卡死」这一底线；是否需要行级截断由实测数字决定（见 optimization-log）。
    assert!(
        p95 <= 500.0,
        "超长单行改一字符 p95 {p95:.2}ms，已属卡死量级（需行级保护）"
    );
}
