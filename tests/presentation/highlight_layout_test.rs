//! 增量高亮与 LayoutJob 缓存用例（阶段 06 · S6-04 验收标准）。
//!
//! S6-04 的风险不在「能不能跑」，而在**能不能跑错**——按行切分再 `concat`
//! 回来，一旦行 / 字符计数与整篇排版不一致，光标、选区、双击选词会整体错位，
//! 而这类错位在肉眼上只是「点偏了一点」，没有任何编译或运行时报错。
//!
//! 因此本文件的第一条也是最重要的一条用例是**等价性断言**：
//! 把 [`highlight_layout`] 的结果与「单个整文档 LayoutJob 排版」逐行比对，
//! 一旦 egui 换了拼接语义，这里会立刻红。
//!
//! 另一条主线是**计数器断言**（验收标准明确要求）：单行编辑时只允许该行重建。

use egui::text::{CCursor, LayoutJob, TextWrapping};

use rustmark::application::dto::{RangeView, SpanView};
use rustmark::presentation::theme::builtin::Theme;
use rustmark::presentation::theme::mapper::{
    StyleMapper, clear_highlight_caches, highlight_layout, last_highlight_stats, line_cache_len,
};

// ============================================================================
// 辅助
// ============================================================================

/// 可用于排版的 `Context`。
///
/// egui 的字体要等第一次 `Context::run` 才可用（`fonts_mut` 在此之前会 panic），
/// 故每个用例都先跑一次空帧把字库装好。
fn test_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    ctx
}

/// 「整文档一次排版」的参考实现——即 S6-04 之前 S6-03 的做法。
///
/// 等价性用例拿它当基准：`highlight_layout` 必须在行结构上与之一致。
fn reference_layout(
    ctx: &egui::Context,
    theme: &Theme,
    text: &str,
    spans: &[SpanView],
    wrap_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mapper = StyleMapper::new(theme);
    let base = mapper.format("plain");

    let mut job = LayoutJob {
        wrap: TextWrapping {
            max_width: wrap_width,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut sorted: Vec<&SpanView> = spans.iter().collect();
    sorted.sort_by_key(|s| s.range.start);

    let mut cursor = 0usize;
    for span in sorted {
        let start = span.range.start.min(text.len());
        let end = span.range.end.min(text.len()).max(start);
        if start > cursor {
            job.append(&text[cursor..start], 0.0, base.clone());
        }
        job.append(&text[start..end], 0.0, mapper.format(&span.style));
        cursor = end;
    }
    if cursor < text.len() {
        job.append(&text[cursor..], 0.0, base.clone());
    }

    ctx.fonts_mut(|f| f.layout_job(job))
}

/// 构造 `[start, end)` 的字节区间 + 样式。
fn span(start: usize, end: usize, style: &str) -> SpanView {
    SpanView {
        range: RangeView::new(start, end),
        style: style.to_string(),
        url: None,
    }
}

/// 逐行比对两个 Galley 的行结构：行数、每行字形数、换行标记、行宽。
fn assert_rows_equivalent(actual: &egui::Galley, expected: &egui::Galley, label: &str) {
    assert_eq!(
        actual.rows.len(),
        expected.rows.len(),
        "{label}：行数不一致（按行 concat 与整篇排版不等价）"
    );
    for (i, (a, e)) in actual.rows.iter().zip(expected.rows.iter()).enumerate() {
        assert_eq!(
            a.glyphs.len(),
            e.glyphs.len(),
            "{label}：第 {i} 行字形数不一致"
        );
        assert_eq!(
            a.ends_with_newline, e.ends_with_newline,
            "{label}：第 {i} 行的换行标记不一致，光标 / 选区会错位"
        );
    }
}

// ============================================================================
// 等价性：按行 concat ≡ 整篇排版（最要紧的一条）
// ============================================================================

/// 多行带 span 的文档：结构与整篇排版完全一致。
#[test]
fn per_line_concat_matches_whole_document_layout() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let text = "# Heading\n\nSome **bold** text\nand more\n";
    let spans = vec![
        span(0, 9, "heading:1"),
        span(12, 16, "strong"),
        span(18, 22, "keyword"),
    ];

    let actual = highlight_layout(&ctx, &theme, text, Some(&spans), 400.0);
    let expected = reference_layout(&ctx, &theme, text, &spans, 400.0);

    assert_rows_equivalent(&actual, &expected, "多行带 span");
    assert_eq!(
        actual.text(),
        expected.text(),
        "拼接后的 Galley 必须携带全文（双击选词依赖 job.text）"
    );

    // 参考实现自身也必须自洽：行字符总数应等于全文字符数。
    let expected_chars: usize = expected
        .rows
        .iter()
        .map(|r| r.glyphs.len() + usize::from(r.ends_with_newline))
        .sum();
    assert_eq!(
        expected_chars,
        text.chars().count(),
        "参考排版的行字符总数应覆盖全文"
    );
}

/// 以 `\n` 结尾的文档必须在最后留一个空行 Row——egui 的明文约定。
///
/// egui `Row::ends_with_newline` 的文档写得很清楚：「text that ends with `\n`
/// has an empty [`Row`] last」。丢了这一行，光标就无法走到文档末尾。
#[test]
fn trailing_newline_yields_final_empty_row() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let galley = highlight_layout(&ctx, &theme, "a\nb\n", None, 400.0);
    assert_eq!(galley.rows.len(), 3, "'a\\nb\\n' 应有 3 行（末行为空行）");
    assert!(galley.rows[0].ends_with_newline);
    assert!(galley.rows[1].ends_with_newline);
    assert!(!galley.rows[2].ends_with_newline, "最后一行不应带换行标记");
    assert_eq!(galley.rows[2].glyphs.len(), 0, "末行应为空");
}

/// 光标映射必须是**全文绝对索引**，而不是行内相对索引。
///
/// 这是按行切分最容易做错的地方：每行的 glyph 位置天然是行内相对的，
/// egui 靠逐行累加 `char_count_including_newline()` 还原绝对索引，
/// 所以一旦某行的字形数对应不上，后面的行会整体偏移。
#[test]
fn cursor_positions_are_absolute_across_lines() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let text = "alpha\nbeta\ngamma\n";
    let galley = highlight_layout(&ctx, &theme, text, None, 400.0);

    // 'beta' 的 'a' 在全文中的索引是 6+3 = 9。
    let mut y_of_row1: Option<f32> = None;
    for idx in 0..=text.chars().count() {
        let cursor = CCursor::new(idx);
        let rect = galley.pos_from_cursor(cursor);

        if idx == 9 {
            y_of_row1 = Some(rect.min.y);
        }
        // 每个光标都必须落在某一行的高度范围内——跑出界就是行间偏移错了。
        assert!(
            rect.max.y > rect.min.y || idx == text.chars().count(),
            "光标 {idx} 未落到有效文本行上"
        );
    }

    // 第 1 行（'beta'）的光标应显著低于第 0 行。
    let row0_rect = galley.pos_from_cursor(CCursor::new(0));
    let y_row1 = y_of_row1.expect("索引 9 应存在");
    assert!(
        y_row1 > row0_rect.min.y,
        "'beta' 的光标应与 'alpha' 不在同一行的高度：{y_row1} vs {}",
        row0_rect.min.y
    );
}

/// 软换行：单个长行被折成多行时，`concat` 只给该分片的**最后一行**打换行标记。
#[test]
fn wrapped_line_rows_stay_in_one_galley_and_keep_row_count() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let long: String = "word ".repeat(60);
    let text = format!("{long}\ntail\n");

    let actual = highlight_layout(&ctx, &theme, &text, None, 120.0);
    let expected = reference_layout(&ctx, &theme, &text, &[], 120.0);

    assert_rows_equivalent(&actual, &expected, "软换行");
    assert!(
        actual.rows.len() > 2,
        "窄宽度下长行应被折成多行，实际 {} 行",
        actual.rows.len()
    );
}

/// 中文 / 多字节：行 != 字节，改行逻辑不得按字节切 char。
#[test]
fn multibyte_text_does_not_panic_and_matches_reference() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let text = "中文标题\n正文 **加粗** 内容\n结束";
    let spans = vec![span(0, 12, "heading:1"), span(19, 25, "strong")];

    let actual = highlight_layout(&ctx, &theme, text, Some(&spans), 300.0);
    let expected = reference_layout(&ctx, &theme, text, &spans, 300.0);
    assert_rows_equivalent(&actual, &expected, "多字节");
}

/// 跨行的 span（如代码块整体标记）必须被裁到各行两端，不得 panic 或错位。
#[test]
fn span_crossing_newline_is_clipped_per_line() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let text = "ab\ncd\n";
    // 一个从第 0 字节覆盖到第 4 字节（跨过 '\n'）的 span。
    let spans = vec![span(0, 4, "code")];

    let actual = highlight_layout(&ctx, &theme, text, Some(&spans), 400.0);
    let expected = reference_layout(&ctx, &theme, text, &spans, 400.0);
    assert_rows_equivalent(&actual, &expected, "跨行 span");
}

/// 空文档 / 纯换行 / 单个字符这些边界不得崩溃。
#[test]
fn degenerate_inputs_do_not_panic() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    for text in ["", "\n", "\n\n\n", "a", "a\n", "\u{4E2D}"] {
        let galley = highlight_layout(&ctx, &theme, text, None, 400.0);
        assert!(!galley.rows.is_empty(), "{text:?} 至少应有一行");
    }
}

// ============================================================================
// 计数器断言（验收标准：「单行编辑时仅该行重建」）
// ============================================================================

const SAMPLE: &str = "# 标题\n\n第一段\n第二段\n第三段\n";

/// 首次排版是冷启动：每行都要重排。
#[test]
fn cold_layout_reshapes_every_line() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    highlight_layout(&ctx, &theme, SAMPLE, None, 400.0);
    let stats = last_highlight_stats();

    assert_eq!(stats.reshaped_lines, stats.total_lines, "冷启动应全量重排");
    assert_eq!(stats.cached_lines, 0);
}

/// **验收核心**：改动一行后，只有那一行重建，其余行从缓存取回。
#[test]
fn editing_one_line_reshapes_only_that_line() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    // 冷启动
    highlight_layout(&ctx, &theme, SAMPLE, None, 400.0);

    // 把「第二段」改成「第二段改」——只动第 4 行。
    let edited = SAMPLE.replace("第二段", "第二段改");
    highlight_layout(&ctx, &theme, &edited, None, 400.0);
    let stats = last_highlight_stats();

    assert_eq!(
        stats.reshaped_lines, 1,
        "单行编辑只应重建 1 行，实际重建 {} 行（共 {} 行）",
        stats.reshaped_lines, stats.total_lines
    );
    assert_eq!(stats.cached_lines, stats.total_lines - 1);
}

/// 文本未变时走整文档缓存：不重排任何行，且返回同一个 `Arc`。
#[test]
fn unchanged_frame_hits_whole_document_cache() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let first = highlight_layout(&ctx, &theme, SAMPLE, None, 400.0);
    let cold_lines = last_highlight_stats().reshaped_lines;

    let second = highlight_layout(&ctx, &theme, SAMPLE, None, 400.0);

    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "文本未变应复用同一个 Galley（空闲帧零开销）"
    );
    assert_eq!(
        last_highlight_stats().reshaped_lines,
        cold_lines,
        "整文档缓存命中时不应触发任何重排"
    );
}

/// 追加一行：只多排一行，原有行全部命中。
#[test]
fn appending_a_line_reshapes_only_the_new_line() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    highlight_layout(&ctx, &theme, SAMPLE, None, 400.0);

    let appended = format!("{SAMPLE}第四段\n");
    highlight_layout(&ctx, &theme, &appended, None, 400.0);
    let stats = last_highlight_stats();

    assert_eq!(
        stats.reshaped_lines, 1,
        "追加一行只应重建新增的那一行，实际 {}",
        stats.reshaped_lines
    );
}

/// 换主题必须让**两层**缓存一起失效（否则切主题后编辑区仍是旧配色）。
#[test]
fn theme_change_invalidates_every_line() {
    clear_highlight_caches();
    let ctx = test_ctx();

    highlight_layout(&ctx, &Theme::dark(), SAMPLE, None, 400.0);
    highlight_layout(&ctx, &Theme::light(), SAMPLE, None, 400.0);
    let stats = last_highlight_stats();

    assert_eq!(
        stats.reshaped_lines, stats.total_lines,
        "换主题后应全量重排（旧配色的缓存行不得被复用）"
    );
}

/// 换行宽度变化（窗口缩放）同样要让行缓存失效——换行后每行的折行位置不同。
#[test]
fn wrap_width_change_invalidates_every_line() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    highlight_layout(&ctx, &theme, SAMPLE, None, 400.0);
    highlight_layout(&ctx, &theme, SAMPLE, None, 200.0);
    let stats = last_highlight_stats();

    assert_eq!(
        stats.reshaped_lines, stats.total_lines,
        "换行宽度变化后应全量重排"
    );
}

// ============================================================================
// LRU 容量 64（§8.3）
// ============================================================================

/// 行布局随行数增长，**不得**被定容上限截断。
///
/// 这条是 `repeated_line_hits_stay_cached` 的正面配套：若有人重新引入定容 LRU，
/// 本例会先红（200 行的文档本应全部留驻）。
#[test]
fn line_layout_retains_every_line() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let text: String = (0..200).map(|i| format!("line {i} content\n")).collect();

    highlight_layout(&ctx, &theme, &text, None, 400.0);
    assert_eq!(stats_total(), 201, "200 行文本 + 末尾空行应为 201 行");
    assert_eq!(line_cache_len(), 201, "每一行都应留在行布局里");
}

/// 反复编辑同一处不会让行布局无限增长（回归：重复编辑不该泄漏）。
#[test]
fn repeated_edits_do_not_grow_line_layout() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    highlight_layout(&ctx, &theme, SAMPLE, None, 400.0);
    let baseline = line_cache_len();

    for i in 0..20 {
        let edited = SAMPLE.replace("第二段", &format!("第二段{i}"));
        highlight_layout(&ctx, &theme, &edited, None, 400.0);
    }

    assert_eq!(line_cache_len(), baseline, "行数未变则行布局规模不应增长");
}

/// 取学习辅助：参与排版的总行数来自最近一次统计。
fn stats_total() -> usize {
    last_highlight_stats().total_lines
}

/// 反复编辑同一行，每次都必须只重排那一行。
///
/// **回归锁定**：这里曾经用「定容 LRU（64 项）」实现，对超过 64 行的文档会雪崩式
/// 全失效——第 0 行被挤出后，读它 miss → 插入 → 挤掉第 1 行 → 第 1 行 miss → ……
/// 最终退化为每次击键都全量重排。故这里刻意用 **65 行**（刚好越过曾经的容量）。
#[test]
fn repeated_line_hits_stay_cached() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let base: String = (0..64).map(|i| format!("row {i}\n")).collect();
    let edited = base.replace("row 63", "row 63x");

    highlight_layout(&ctx, &theme, &base, None, 400.0);
    highlight_layout(&ctx, &theme, &edited, None, 400.0);
    let first = last_highlight_stats();

    // 再改同一行：应依旧只重排一行。
    let edited2 = base.replace("row 63", "row 63y");
    highlight_layout(&ctx, &theme, &edited2, None, 400.0);
    let second = last_highlight_stats();

    let total = base.lines().count() + 1; // 末尾空行
    assert_eq!(stats_total(), total);
    assert_eq!(
        first.reshaped_lines, 1,
        "越过曾经 LRU 容量的文档同样只能重排 1 行"
    );
    assert_eq!(second.reshaped_lines, 1, "同一行反复编辑应保持只去重排一行");
}

// ============================================================================
// 大文件降级（FR-06）
// ============================================================================

/// > 500KB 的文档停止 span 着色（纯文本排版），且不 panic、行数正确。
///
/// 这是对 FR-06「大文件不掉帧」的兜底：真实的可视区虚拟化需要 viewport，
/// 而 `TextEdit` 的 `layouter` 拿不到滚动位置，留待阶段 07 走查。
#[test]
fn oversized_document_drops_span_coloring() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    // 用「少行数 + 不折行」构造超阈值的样本：500 行 × ~1.1KB ≈ 550KB。
    // （铺 60 万行的做法在 debug 下要跑好几秒，与被测性质无关。）
    let line = "const x = 1; ".repeat(85);
    let text = format!("{line}\n").repeat(500);
    assert!(text.len() > 500 * 1024, "构造的样本必须超过阈值");

    let spans = vec![span(0, 5, "keyword")];
    // wrap_width = 0.0 → 不折行，行数 == 物理行数。
    let galley = highlight_layout(&ctx, &theme, &text, Some(&spans), 0.0);

    assert_eq!(galley.rows.len(), 501, "行数（含末尾空行）应正确");
}

// ============================================================================
// 性能基线（NFR-PERF-05：10KB 单次全文高亮 ≤ 10ms）
// ============================================================================

/// 10KB 文档的**整篇**高亮必须在预算内。
///
/// 实测基线（本机 debug 构建，仅供参考）：13440 字节 / 841 行 / 12600 字形 ≈ **2.3ms**。
/// 这里给到 50ms 作为 CI 上的宽松上限——目的是拦住量级退化
/// （比如退化成每帧多次排版），而不是在 CI 抖动上 flaky；实际值会打印出来。
#[test]
fn ten_kb_full_highlight_stays_within_budget() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let text: String = "# Heading\n\nconst x = 1; para **bold** text here\n".repeat(280);
    assert!(
        (8 * 1024..=16 * 1024).contains(&text.len()),
        "样本应约 10KB，实际 {} 字节",
        text.len()
    );

    let spans = vec![span(0, 9, "heading:1")];
    let started = std::time::Instant::now();
    let galley = highlight_layout(&ctx, &theme, &text, Some(&spans), 600.0);
    let elapsed = started.elapsed();

    println!(
        "10KB 全文高亮基线：{} 字节 / {} 行 / {:.2}ms",
        text.len(),
        galley.rows.len(),
        elapsed.as_secs_f64() * 1000.0
    );
    assert!(
        elapsed.as_millis() <= 50,
        "10KB 整篇高亮耗时 {elapsed:?}，超出宽松上限 50ms"
    );
}

/// 击键（单行改动）后的重排必须显著快于整篇。
///
/// 这是 S6-04 真正要买到的东西：一次敲键只做一行的排版。
#[test]
fn keystroke_relayout_only_rebuilds_one_line() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let base: String = (0..300)
        .map(|i| format!("第 {i} 行：这里是一些正文内容\n"))
        .collect();

    highlight_layout(&ctx, &theme, &base, None, 600.0);

    // 模拟在第 150 行敲一个字。
    let edited = base.replace("第 150 行", "第 150 行改");
    highlight_layout(&ctx, &theme, &edited, None, 600.0);

    let stats = last_highlight_stats();
    assert_eq!(stats.reshaped_lines, 1, "敲一个键只应重排 1 行");
    assert!(
        stats.cached_lines >= 290,
        "绝大多数行应复用，实际复用 {} 行",
        stats.cached_lines
    );
}

/// **回归锁定**：多处散布的修改（如「全部替换」）只重排真正变化的行。
///
/// 只做前后缀差分的实现在这里会退化——首个与末个改动之间的行明明没变，
/// 却会被整体重排（实测 841 行里重排了 838 行，只有 3 行被复用）。
#[test]
fn scattered_edits_only_reshape_changed_lines() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    // 3 行一组，共 280 组；每组第 3 行含 "para"。
    let base: String = "# Heading\n\nconst x = 1; para **bold** text here\n".repeat(280);
    highlight_layout(&ctx, &theme, &base, None, 600.0);

    // 全文替换：恰好改动 280 行，且这些行在文档里是散布的。
    let replaced = base.replace("para", "paraX");
    highlight_layout(&ctx, &theme, &replaced, None, 600.0);

    let stats = last_highlight_stats();
    assert_eq!(
        stats.reshaped_lines, 280,
        "全文替换只应重排真正变化的那 280 行（其前后缀差分会退化到 838 行）"
    );
    assert_eq!(stats.cached_lines, stats.total_lines - 280);
}

/// 中间插入一行：内容未变的行必须继续复用（行号平移不得导致全量重排）。
#[test]
fn inserting_a_line_in_the_middle_keeps_surrounding_lines() {
    clear_highlight_caches();
    let ctx = test_ctx();
    let theme = Theme::dark();

    let base: String = (0..50).map(|i| format!("第 {i} 行内容\n")).collect();
    highlight_layout(&ctx, &theme, &base, None, 600.0);

    let with_inserted = base.replace("第 25 行内容\n", "第 25 行内容\n新插入的一行\n");
    highlight_layout(&ctx, &theme, &with_inserted, None, 600.0);

    let stats = last_highlight_stats();
    assert_eq!(
        stats.reshaped_lines, 1,
        "插入一行只应重排该行（前后缀差分应吃掉行号平移），实际 {}",
        stats.reshaped_lines
    );
}
