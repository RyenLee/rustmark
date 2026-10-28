//! 编辑区（`TextEdit`）每帧成本基线（问题 046：对照 `docs/logs/T.txt` 做审计与量化）。
//!
//! # 口径
//!
//! 数字**必须在 release 下取**：
//!
//! ```text
//! cargo test --release --test presentation -- --nocapture textedit_frame
//! ```
//!
//! # 为什么单独一个文件
//!
//! `highlight_perf_test.rs` 量的是**排版层**（`highlight_layout`）的成本；
//! 本文件量的是**完整编辑区一帧**的成本——`TextEdit::show` 加上我们每帧前置
//! 的那些工作（取高亮 span、算行数、克隆旧文本……），用于回答
//! 「T.txt 描述的卡顿原因在本项目里还剩多少、在哪一环」。
//!
//! 数字写入 `docs/spec/metrics.md`（唯一数字来源，禁止散落在代码注释里）。

use std::fs;
use std::sync::Arc;
use std::time::Instant;

use crate::support::clock::TestClock;
use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::markdown::queries::HighlightQuery;
use rustmark::shared::geometry::{ByteOffset, TextRange};

// ============================================================================
// 辅助
// ============================================================================

/// 合成接近 `target_bytes` 的中文 Markdown 语料（整块拼接，不会切断 UTF-8）。
///
/// 与 `highlight_perf_test.rs` 的语料同源：行数多、有列表 / 代码块 / 强调，
/// 保证「每帧前置工作」的规模与真实文档一致。
fn sample(target_bytes: usize) -> String {
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

/// 造一个已打开 `big.md` 的应用。
fn app_with_doc(target_bytes: usize) -> (tempfile::TempDir, tempfile::TempDir, EditorApp, String) {
    let ws = tempfile::tempdir().expect("工作区临时目录");
    let text = sample(target_bytes);
    fs::write(ws.path().join("big.md"), &text).expect("写样例文档");

    let cfg = tempfile::tempdir().expect("配置临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(cfg.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let root = ws.path().to_string_lossy().replace('\\', "/");
    app.scan_workspace(&root).expect("扫描应成功");
    app.open_workspace_file(&format!("{root}/big.md"))
        .expect("打开应成功");
    (ws, cfg, app, text)
}

/// 连续驱动 `n` 帧完整的应用视图（关掉预览与侧栏，只留编辑区），返回每帧毫秒。
///
/// 关掉预览 / 侧栏的理由与 `editor_view_test.rs` 相同：否则量到的是三个面板的
/// 总和，无法归因到编辑区。
fn drive_frames(app: &mut EditorApp, n: usize) -> Vec<f64> {
    let ctx = egui::Context::default();
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {}); // 装载字库
    app.ui.preview_visible = false;
    app.ui.sidebar_visible = false;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        let _ = crate::support::run_ui(&ctx, input.clone(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ui);
        });
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples
}

/// 同 [`app_with_doc`]，但装配**可控时钟**。
///
/// # 为什么击键帧必须用可控时钟
///
/// 高亮防抖按 `Clock::now()` 的差值判断「文档是否已静止」。真实 `SystemClock`
/// 下连续 100 帧的墙钟间隔只有几十毫秒（release 下每帧零点几毫秒），恰好都落在
/// 150ms 窗口内 —— 用例「碰巧」测到打字期；一旦机器慢下来（debug、CI 抖动）
/// 中途越过窗口，就会**少则一帧**触发一次全量 syntect 解析（10KB ≈ 59ms），
/// 行为随时钟漂移 = 随机失败。故此处显式推进时钟，让「窗口内」成为确定状态。
fn app_with_doc_and_clock(
    target_bytes: usize,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    EditorApp,
    Arc<TestClock>,
    String,
) {
    let ws = tempfile::tempdir().expect("工作区临时目录");
    let text = sample(target_bytes);
    fs::write(ws.path().join("big.md"), &text).expect("写样例文档");

    let cfg = tempfile::tempdir().expect("配置临时目录");
    let clock = Arc::new(TestClock::default());
    let services = Services::assemble_with_clock_and_dialogs(
        ConfigPaths::in_dir(cfg.path()),
        clock.clone(),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let root = ws.path().to_string_lossy().replace('\\', "/");
    app.scan_workspace(&root).expect("扫描应成功");
    app.open_workspace_file(&format!("{root}/big.md"))
        .expect("打开应成功");
    (ws, cfg, app, clock, text)
}

/// 驱动编辑区一帧：注入 `events`，推进时钟 8ms，返回该帧耗时（毫秒）。
///
/// 应用时钟（防抖判据）与 egui 时间（动画）同步推进；`screen` 固定 1280×800。
fn editor_frame(
    ctx: &egui::Context,
    app: &mut EditorApp,
    clock: &TestClock,
    time_s: &mut f64,
    events: Vec<egui::Event>,
) -> f64 {
    *time_s += 0.008;
    clock.advance(8);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        time: Some(*time_s),
        events,
        ..Default::default()
    };
    let start = Instant::now();
    let _ = crate::support::run_ui(ctx, input, |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(app, ui);
    });
    start.elapsed().as_secs_f64() * 1000.0
}

/// 备好编辑区帧循环：建 Context、关预览与侧栏（隔离编辑区）、装载字库、
/// 空跑一帧让测量 pass 落定、把焦点交给编辑区，再空跑一帧让焦点生效。
///
/// 返回 `(ctx, egui 时间)` 供后续帧继续推进。预热两帧不计入任何样本。
fn warm_editor(app: &mut EditorApp, clock: &TestClock) -> (egui::Context, f64) {
    let ctx = egui::Context::default();
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {}); // 装载字库
    app.ui.preview_visible = false;
    app.ui.sidebar_visible = false;

    let mut time_s = 0.0_f64;
    let _ = editor_frame(&ctx, app, clock, &mut time_s, Vec::new());
    ctx.memory_mut(|m| m.request_focus(EditorApp::editor_id()));
    let _ = editor_frame(&ctx, app, clock, &mut time_s, Vec::new());
    (ctx, time_s)
}

/// 击键帧 p95 的上限（毫秒）。
///
/// # 两档阈值，两条不同的使命
///
/// **≤ 500KB（走正常上色路径）**：守 `NFR-PERF-04` 的 **16ms 一帧**预算本身。
/// 连续打字时若防抖失效、每次击键都触发全量 syntect 解析，10KB 就是 59ms 一帧，
/// 这条断言立刻红——它同时钉住「高亮降级（OPT-011 ①）仍然生效」这个用户可感的性质。
///
/// **> 500KB（已降级为纯文本）**：给 **64ms**，语义退化为「防数量级退化」。
/// 这一档**实测超指标**（1MB 约 41ms，见 `docs/spec/metrics.md` §3.2.3）：此处不假装达标，
/// 只钉住「别更糟」。
///
/// ⚠ **2026-10-16 问题 049 修正**：本函数原先写「这笔钱花在框架里、本项目无法规避，
/// 真要把 1MB 打字做进 16ms 得自绘编辑器」——该归因已被实测证伪。1MB 击键帧里每帧
/// 真正重排的行数**恒为 1**（不是 58k 行），成本构成为「空闲帧 + 排版层 +
/// `diff_replace` + 应用层文本变更链」，其中占 58% 的应用层净开销与自绘无关。
/// 见 `docs/spec/metrics.md` §3.2.4 与 `docs/archive/stage-reports/textedit-cost-attribution.md`。
fn typing_frame_limit_ms(bytes: usize) -> f64 {
    let release = if bytes >= 1024 * 1024 { 64.0 } else { 16.0 };
    if cfg!(debug_assertions) {
        release * 8.0
    } else {
        release
    }
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
// 1. 编辑区**空闲帧**成本（滚动 / 空闲是绝大多数帧）
// ============================================================================

/// 空闲帧 p50 的上限（毫秒）。
///
/// # 阈值为什么刻意宽松
///
/// 这条断言的使命是**钉住数量级退化**，不是卡住某个具体数字——具体数字的裁定
/// 在 release 实测（`docs/spec/metrics.md` §3.2.2）。典型退化如「有人又把全文快照 /
/// 全文扫描拖回每帧路径」，在 1MB 上会立刻翻数倍，这个阈值拦得住。
///
/// 按编译档位分别给：同一段代码 debug / release 差数倍
/// （本文件实测 100KB 空闲帧 p50：debug 2.10ms / release 0.44ms，≈4.8×；
/// 超长单行排版在两个档位差 ≈8.4×，见 `highlight_perf_test.rs`）。沿用 release
/// 阈值会让日常门禁 `cargo test --all-targets`（debug）稳定误报。
fn idle_frame_limit_ms(bytes: usize) -> f64 {
    let release = if bytes >= 1024 * 1024 { 24.0 } else { 8.0 };
    if cfg!(debug_assertions) {
        release * 8.0
    } else {
        release
    }
}

/// 三档文档大小的空闲帧成本。
///
/// 「空闲帧」= 文本未变、光标未动、无输入事件——用户滚动、阅读、切面板时的帧。
/// 这一档最该便宜：egui 的 `TextEdit` 与我们的排版缓存都应命中。
///
/// 实测基线（release，2026-10-16 问题 046 修复后）：
/// 10KB p50 0.12ms ｜ 100KB p50 0.44ms ｜ 1MB p50 4.32ms，都在一帧 16ms 预算内。
#[test]
fn measure_textedit_idle_frame_at_three_sizes() {
    for (label, bytes) in [
        ("10KB", 10 * 1024),
        ("100KB", 100 * 1024),
        ("1MB", 1024 * 1024),
    ] {
        let (_ws, _cfg, mut app, text) = app_with_doc(bytes);
        // 预热两帧：首帧是 egui 的测量 pass（尺寸与稳定帧不同），且字体 /
        // 排版缓存首次填充的开销属一次性成本，不该进稳态样本。
        let _ = drive_frames(&mut app, 2);
        let samples = drive_frames(&mut app, 30);
        let p50 = pct(samples.clone(), 0.50);
        let p95 = pct(samples, 0.95);
        println!(
            "[metrics] 编辑区空闲帧 {label}（{} 字节 / {} 行）：p50={p50:.2}ms p95={p95:.2}ms",
            text.len(),
            text.split('\n').count()
        );
        // 用 p50 断言：样本只有 30 个时 p95 就是最大值，单帧抖动会让门禁随机失败。
        let limit = idle_frame_limit_ms(bytes);
        assert!(
            p50 <= limit,
            "编辑区空闲帧 {label} p50 {p50:.2}ms 越过守卫阈值 {limit:.0}ms —— \
             疑似有 O(全文) 的工作被拖回了每帧路径"
        );
    }
}

// ============================================================================
// 2. 每帧前置工作的分项归因
// ============================================================================

/// 把编辑区每帧的**非排版**工作逐项计时，回答「钱花在哪」。
///
/// 排版本身（`highlight_layout`）已有 `highlight_perf_test.rs` 覆盖，
/// 这里只量更新一版高亮 span、算行数、克隆旧文本这几项——它们都在
/// `editor_view::show` 里逐帧执行，且都不依赖 egui。
///
/// # 为什么这里没有阈值断言
///
/// 「每帧不得快照全文」这条结论**不适合**用耗时阈值钉——亚毫秒级样本的
/// 抖动比信号还大。它由 `tests/application/markdown_service_test.rs` 里的
/// 调用计数断言确定性钉死（`highlight_frame_path_never_snapshots_the_whole_document`、
/// `spell_check_frame_path_never_snapshots_the_whole_document`）。
/// 本用例只负责把**归因数字**打出来，供 `docs/spec/metrics.md` §3.2.2 引用。
#[test]
fn measure_editor_per_frame_subcosts() {
    let (_ws, _cfg, app, text) = app_with_doc(100 * 1024);
    let id = app.open_document_list()[0].id;

    let query = || HighlightQuery {
        doc_id: id,
        range: TextRange::new(ByteOffset::new(0), ByteOffset::new(text.len())),
        lang: Some("markdown".to_string()),
    };

    // 首次调用会真的做一次 syntect 全文解析（数十毫秒），不计入样本。
    let first = app
        .services
        .markdown
        .highlight(query())
        .expect("高亮应成功");
    let span_count = first.len();

    // (a) 高亮查询的**缓存命中**成本（每帧都走这条）。
    let hit = timed_ms(50, || {
        let s = app.services.markdown.highlight(query()).expect("应成功");
        std::hint::black_box(s);
    });

    // (b) 行数扫描（`split('\n').count()`，每帧一次）。
    let lines_split = timed_ms(50, || {
        std::hint::black_box(text.split('\n').count());
    });

    // (b') 候选替代写法：按**字节**数换行（UTF-8 续字节 ≥ 0x80，
    //     故字节 0x0A 的个数与 `'\n'` 字符的个数相等）。
    let lines_bytes = timed_ms(50, || {
        std::hint::black_box(text.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1);
    });

    // (c) 光标行号换算（`gutter::current_line_index` 的两种写法，光标在文末 = 最坏情况）。
    let cursor_at_end = text.len();
    let cur_chars = timed_ms(50, || {
        std::hint::black_box(text[..cursor_at_end].chars().filter(|&c| c == '\n').count());
    });
    let cur_bytes = timed_ms(50, || {
        std::hint::black_box(
            text.as_bytes()[..cursor_at_end]
                .iter()
                .filter(|&&b| b == b'\n')
                .count(),
        );
    });

    // (d) 旧文本克隆（diff 的基准，每帧一次）。
    let clone = timed_ms(50, || {
        std::hint::black_box(text.clone());
    });

    println!(
        "[metrics] 编辑区每帧前置工作（100KB / {} 行 / {} 个 span）：\
         高亮查询命中 p50={:.3}ms ｜ 行数 split p50={:.3}ms ／ 行数 bytes p50={:.3}ms ｜ \
         光标行 chars p50={:.3}ms ／ bytes p50={:.3}ms ｜ 旧文本克隆 p50={:.3}ms",
        text.split('\n').count(),
        span_count,
        pct(hit, 0.50),
        pct(lines_split, 0.50),
        pct(lines_bytes, 0.50),
        pct(cur_chars, 0.50),
        pct(cur_bytes, 0.50),
        pct(clone, 0.50),
    );
}

// ============================================================================
// 3. 编辑区**击键帧**成本（`NFR-PERF-04` 的验收口径）
// ============================================================================

/// `NFR-PERF-04`：**连续输入 100 次，取 p95**。
///
/// 这一档是用户打字时真正的帧成本，本文件里唯一直接对应指标的用例。
///
/// # 两类断言，各有各的确定性
///
/// 1. **确定性（计数）**：100 次输入期间，高亮缓存的 `misses` 必须**一次都不涨**
///    ——即全程没有任何一帧真的跑过 syntect 全文解析。这是「防抖生效」最硬的证据：
///    与耗时无关，不受机器快慢影响。
/// 2. **量化（耗时）**：p95 落在 [`typing_frame_limit_ms`] 内。对 10KB / 100KB
///    就是 `NFR-PERF-04` 的 16ms 预算；1MB 已超指标（见该函数与 §3.2.3）。
///
/// 每帧注入真实的 `Event::Text`（egui 文本输入 → `TextEdit` 改写文本 → 应用侧
/// diff / 排版 → 重绘），而不是直接改 `editor_text`：后者绕过「输入事件到文本
/// 模型」这一段，量不到用户真正等待的部分。每帧推进 8ms，模拟约 125fps 的连续
/// 人手节奏（间隔 < 150ms 防抖窗）——这正是「连续输入」的语义。
///
/// 实测基线（release，见 `docs/spec/metrics.md` §3.2.3）：
/// 10KB p95 0.64ms ｜ 100KB p95 4.57ms ｜ 1MB p95 43.14ms。
#[test]
fn measure_textedit_typing_frame_at_three_sizes() {
    for (label, bytes) in [
        ("10KB", 10 * 1024),
        ("100KB", 100 * 1024),
        ("1MB", 1024 * 1024),
    ] {
        let (_ws, _cfg, mut app, clock, text) = app_with_doc_and_clock(bytes);
        let before = text.len();
        let (ctx, mut time_s) = warm_editor(&mut app, &clock);

        // 同一运行内的空闲帧对照：把「击键帧 − 空闲帧」归因到「文本变化」这件事上。
        // ⚠ 问题 049 起，`textedit_cost_attribution_test.rs` 用八组对照做了完整归因：
        // 多出来的成本**不是**「框架重建整篇 Galley」（实测每帧只重排 1 行），而是
        // 「排版层 + `diff_replace` + 应用层文本变更链」。本行注释原先的推断已作废。
        let idle: Vec<f64> = (0..5)
            .map(|_| editor_frame(&ctx, &mut app, &clock, &mut time_s, Vec::new()))
            .collect();

        // 预热已完成（首次上色那次 miss 属于预期），此后打字期不得再有任何 miss。
        let (_, misses_before) = app.services.markdown.highlight_cache_stats();
        let samples: Vec<f64> = (0..100)
            .map(|_| {
                editor_frame(
                    &ctx,
                    &mut app,
                    &clock,
                    &mut time_s,
                    vec![egui::Event::Text("a".to_owned())],
                )
            })
            .collect();
        let (hits_after, misses_after) = app.services.markdown.highlight_cache_stats();

        // 自检：100 次 `Event::Text` 必须**真的**逐字落进文档。若焦点没落上，
        // 量到的就是空闲帧，下面的断言便成为空转（恒真）——变异验证的前提。
        let after = app.current_content().map(|c| c.len()).unwrap_or(0);
        assert_eq!(
            after,
            before + 100,
            "编辑区击键帧 {label}：100 次输入应使文档恰好增长 100 字节（实际 {before} → {after}）——\
             焦点未落在编辑区时用例会退化成空闲帧测量"
        );

        // 确定性断言：连续输入期间一次全量高亮都没有发生（防抖全程生效）。
        assert_eq!(
            misses_after,
            misses_before,
            "编辑区击键帧 {label}：连续输入 100 次期间发生了 {} 次全量高亮解析——\
             防抖（OPT-011 ①）已失效，或取全文的慢路径被拖回了帧路径",
            misses_after - misses_before
        );

        let p50 = pct(samples.clone(), 0.50);
        let p95 = pct(samples, 0.95);
        println!(
            "[metrics] 编辑区击键帧 {label}（连续输入 100 次）：p50={p50:.2}ms p95={p95:.2}ms ｜ \
             同运行空闲帧 p50={:.2}ms ｜ 高亮命中/未命中={hits_after}/{misses_after}",
            pct(idle, 0.50)
        );

        let limit = typing_frame_limit_ms(bytes);
        assert!(
            p95 <= limit,
            "编辑区击键帧 {label} p95 {p95:.2}ms 越过守卫阈值 {limit:.0}ms —— \
             疑似高亮防抖失效，或有新的 O(全文) 工作进了击键路径"
        );
    }
}

// ============================================================================
// 4. 击键帧为什么便宜：防抖**确实在生效**，而不是根本没有上色
// ============================================================================

/// 上一条用例的对照实验：打字期**一次解析都不做**（计数为证），停手后**必须做一次**。
///
/// 否则「击键帧便宜」有两种可能无法区分：(a) 防抖生效、打字期不上色（设计如此）；
/// (b) 上色链压根没跑（例如文档过大被降级、或高亮查询静默失败）。
/// 本用例用高亮缓存的 `misses` 计数排除 (b)：停手后那一帧必须让它**涨 1**——
/// 计数比耗时确定（不受机器快慢与抖动影响），也正因如此它才配当结论的依据。
///
/// 若哪天有人把防抖误删成「每帧都全量」，第 3 节用例会红；若把上色链整个
/// 断掉，本用例会红。两条用例互为对照，单独任何一条都不足以定论。
#[test]
fn typing_cheap_because_highlight_is_deferred_not_absent() {
    use rustmark::application::document::DocumentContentProvider as _;

    let (_ws, _cfg, mut app, clock, _text) = app_with_doc_and_clock(10 * 1024);
    let doc_id = app.open_document_list()[0].id;
    let (ctx, mut time_s) = warm_editor(&mut app, &clock);

    // 连续打字 30 帧（时钟每帧 +8ms，全程在 150ms 防抖窗内）。
    let (_, typing_misses_before) = app.services.markdown.highlight_cache_stats();
    let typing: Vec<f64> = (0..30)
        .map(|_| {
            editor_frame(
                &ctx,
                &mut app,
                &clock,
                &mut time_s,
                vec![egui::Event::Text("a".to_owned())],
            )
        })
        .collect();
    let typing_p50 = pct(typing, 0.50);
    let (_, typing_misses_after) = app.services.markdown.highlight_cache_stats();

    // 停手：让时间流过防抖窗，取这几帧里最贵的一帧作为「停手后上色」的证据。
    //
    // # 为什么是「每帧推进 + 取最坏」而不是「推进一次 + 单帧」
    //
    // `editor_view::show` 在把 `TextEdit` 的缓冲写回文档**之前**就发起了高亮查询，
    // 所以那次查询读到的版本比文档当前版本**落后一帧**。「静止」的计时是从
    // 「查询见到的版本」稳定下来那一刻开始的，于是停手后第一帧只是把这个差值
    // 补上（防抖照旧视为「版本变了」→ 再记一次时刻、仍不上色），要再过一帧、
    // 且这一帧与上一帧之间隔够 `interval_ms`，才会真正做那次全量解析。
    //
    // 现状下这不是缺陷——150ms 的窗口本来就吞掉一帧的量级；但回归必须照实写，
    // 否则会把它误判成「防抖失效」。取最坏帧而不指定哪一帧，是为了对
    // 「还需几帧才追上」保持不敏感。
    let mut settled_worst = 0.0_f64;
    for _ in 0..4 {
        clock.advance(300);
        settled_worst = settled_worst.max(editor_frame(
            &ctx,
            &mut app,
            &clock,
            &mut time_s,
            Vec::new(),
        ));
    }
    let settled = settled_worst;
    let (_, settled_misses) = app.services.markdown.highlight_cache_stats();

    println!(
        "[metrics] 击键期 p50={typing_p50:.3}ms（解析 {} 次）｜ 停手后最坏帧={settled:.2}ms（累计解析 {} 次）",
        typing_misses_after - typing_misses_before,
        settled_misses - typing_misses_before
    );

    assert_eq!(
        typing_misses_after,
        typing_misses_before,
        "连续打字 30 帧期间发生了 {} 次全量高亮解析——防抖未生效",
        typing_misses_after - typing_misses_before
    );
    assert!(
        settled_misses > typing_misses_after,
        "停手（越过 150ms 窗口）后没有发生任何一次全量高亮解析——\
         上色链疑似已断（高亮查询静默失败 / 文档被误降级），\
         此时第 3 节「击键帧便宜」不再有对照意义"
    );
    // 停手帧必须**不便宜于**击键帧：否则量到的很可能是空闲帧，而不是真正
    // 做了解析的那一帧。
    //
    // # 为什么阈值从 3× 降到 1×
    //
    // 原断言是「停手帧 >= 打字期 p50 的 3 倍」，依据是「停手后要做一次
    // 56ms 级的全量解析」。增量高亮（OPT-011 复议）接手后，停手帧只重解
    // 变更行——实测该帧从 56ms 级降到个位数 ms，与击键帧不再有数量级差距，
    // 3× 阈值因此失去区分力（它开始惩罚的正是我们想要的优化）。
    //
    // 主证据由上面的 `settled_misses` 计数承担：它直接证明「停手后确实
    // 发生了一次解析」，与机器快慢、防抖实现细节都无关；耗时只用于排除
    // 「量错了帧」。
    assert!(
        settled >= typing_p50.max(0.05),
        "停手帧 {settled:.2}ms 低于打字期 p50 {typing_p50:.3}ms —— \
         量到的很可能不是真正做了解析的那一帧"
    );

    // 防抖状态必须已**追上**文档版本：此后每一帧看到的都是「版本未变」，
    // 于是窗口一旦到期就上色。这直接钉住「停手后能恢复上色」的机制本身
    // （上面的耗时断言只是它的间接表现），也顺带记录了一个易被忽略的事实——
    // 高亮查询跑在文本写回之前，故它见到的版本总落后一帧，需要再走一帧才追上。
    let current_rev = app
        .services
        .registry
        .read()
        .expect("注册表锁")
        .revision_and_len(doc_id)
        .expect("文档应已打开")
        .0;
    let pace = app.services.markdown.highlight_pace_snapshot();
    assert!(
        pace.iter().any(|(_, rev, _)| *rev == current_rev),
        "防抖快照 {pace:?} 中未见文档当前版本 {current_rev} —— \
         停手后防抖仍未追上文档版本，窗口永远不会到期"
    );
}
