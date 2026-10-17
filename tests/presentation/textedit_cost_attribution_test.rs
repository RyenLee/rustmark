//! 编辑区 1MB 击键帧成本归因（问题 049）。
//!
//! # 要裁决的争议
//!
//! `docs/metrics.md` §3.2.3 结论 3 把 1MB 档击键帧的约 39ms（击键帧 42ms −
//! 空闲帧 3.25ms）记在「egui 文本一变即重建 58,514 行 `Galley`」上，并据此判定
//! 「要做进 16ms 只能自绘编辑器」。但同一节的结论 1 写的是「只重建改动那一行，
//! 增量设计重建行数 == 1」，而 `mapper::highlight_layout_with_find` 的大文件降级
//! 分支也明确注明「跳过 span 着色，**仍逐行缓存**」——三处互相矛盾。
//! 本用例用测量裁决，不靠推理。
//!
//! # 三组对照
//!
//! 同为 1MB 中文 Markdown、同样注入**真实的** `Event::Text`、同样每帧推进 8ms：
//!
//! | 组 | 被测对象 | 回答的问题 |
//! |---|---|---|
//! | E1 | 真实 `EditorApp`（当前实现，含我们的逐行 layouter） | 指标口径下每帧到底几毫秒 |
//! | E2 | 直接调 `mapper::highlight_layout_with_find`（文本变一字节） | 「拆行 + 差分 + 重排 1 行 + concat」自己花了多少 |
//! | E2b | 同上但文本不变（整文档缓存命中） | 一次调用的底价是多少（把簿记成本与调用开销分开） |
//! | E3 | 裸 `TextEdit` + **egui 默认 layouter** | 交给上游（含 0.37 的按段落缓存）要多少 |
//! | E7 | 裸 `TextEdit` + **我们的 layouter** | 同样的宿主，换成自家排版层净增多少 |
//! | E8 | 直接调 `editor_ops::diff_replace`（文末追加 / 文档中点替换） | 应用层文本变更链里最大的一块嫌疑占多少 |
//!
//! # 第二轮（049 续）：把 E1 的「余下」拆到代码行
//!
//! 第一轮把击键帧拆成 3 块后仍余 17.68ms（见
//! `docs/archive/stage-reports/textedit-cost-attribution.md` §5 遗留清单）。
//! 读代码后锁定的嫌疑全部是**每帧或每击键一次的 `O(全文)` 操作**，
//! 且都在应用层 / 领域层——**自绘编辑器照样要付**：
//!
//! | 组 | 被测对象 | 位置 |
//! |---|---|---|
//! | A2 | 状态栏消融（同一份代码，只切 `status_bar_visible`） | `status_bar_view::show` |
//! | S1 | 整篇 `String` 克隆 | `editor_view.rs` 的 `prev.clone()`；`aggregate.rs` 的 `snapshot_at` |
//! | S2 | `format!` 重建整篇最终串 | `editor_view.rs` 第 5 步 |
//! | S3 | `line_start_offsets`（2 遍全文扫描 + 58k 项 `Vec`） | `scroll_sync.rs`，经 `cursor_line_col` 每帧一次 |
//! | S4 | `line_count`（1 遍全文扫描） | `shared::text`，编辑区与状态栏各一次 |
//! | S5 | 领域 `replace_range` 整条链路（含撤销快照） | `service/editing.rs` → `aggregate.rs::apply` |
//! | S5b | 同上但 10KB 文档 | 用长度斜率判定其中 `O(全文)` 成分 |
//! | S6 | `get_stats` 的 miss 帧（整篇克隆 + 全文按行统计） | `service/queries.rs`，状态栏每击键一次 |
//!
//! 判读规则：
//!
//! - `E2` 若占 `E1 − E3` 的**大头** ⇒ 瓶颈在本项目自己的缓存簿记里，与 egui 无关，
//!   自绘解决不了（自绘照样要写这些，且更长）。
//! - `E3` 若与 `E1` 同量级 ⇒ egui 侧那笔成本确实存在，且与我们的排版层无关。
//! - 两条同时成立 ⇒ 「1MB 只能靠自绘」的结论**不成立**。
//!
//! # E3 为什么是「不自绘、交给上游」的忠实对照
//!
//! 裸 `TextEdit` 的默认 layouter 走 `LayoutJob::simple`，其 `break_on_newline = true`、
//! `wrap.max_rows = usize::MAX`（默认值，见 `epaint-0.37.0/src/text/text_layout_types.rs`
//! 的 `simple` 与 `TextWrapping::default`），且文本含 `\n`——恰好满足 epaint 0.37
//! `galley_cache::should_cache_each_paragraph_individually` 的三个条件。故上游会
//! **按段落分别缓存、只重排改动段**（`layout_each_paragraph_individually`），
//! 即这条路径量的正是「升级路线 / 交给上游」的上限。
//!
//! 反观本项目：`highlight_layout` 自己把整篇拆成**单行 job**，单行不含 `\n`，
//! 该条件恒为 `false`——上游这份按段落缓存从未生效过（等价效果由我们自己的
//! `LINE_LAYOUT` 逐行 LRU 提供）。
//!
//! # 口径
//!
//! 数字**必须在 release 下取**（debug 下文本处理慢数倍，结论会失真）：
//!
//! ```text
//! cargo test --release --test presentation -- --nocapture profile_1mb_typing_frame
//! ```
//!
//! # 关于重复的辅助函数
//!
//! E1 的驱动辅助（`sample` / `app_with_doc_and_clock` / `editor_frame` / `warm_editor`）
//! 与 `textedit_frame_perf_test.rs` 同名同源，此处**刻意各留一份**：那条基线用例
//! 已通过验收，本次是诊断，不去动它。若日后两处都要演进，再提为 `tests/support/` 模块。

use std::fs;
use std::sync::Arc;
use std::time::Instant;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::ReplaceRangeCommand;
use rustmark::presentation::theme::mapper;
use rustmark::presentation::views::scroll_sync;
use rustmark::shared::geometry::{ByteOffset, TextRange};

use crate::support::clock::TestClock;
use crate::support::fake_dialog::FakeDialog;

/// 编辑区视口（与既有基线用例一致）。
const SCREEN: [f32; 2] = [1280.0, 800.0];
/// 每帧推进的毫秒数（约 125fps 的连续输入节奏）。
const FRAME_MS: u64 = 8;
/// 1MB 档的语料规模。
const MIB: usize = 1024 * 1024;

/// 帧数按编译档位缩放。
///
/// **数字一律以 release 为准**；debug 下 1MB 的单帧贵近一个数量级，若照搬
/// release 的样本数，本用例会把日常门禁（`cargo test --all-targets`）拖慢一两分钟。
/// 缩放后 debug 只承担守卫职责（钉住「每帧重建行数仍是个位数」），不产出可比数字。
fn frames_for(n: usize) -> usize {
    if cfg!(debug_assertions) {
        (n / 5).max(5)
    } else {
        n
    }
}

// ============================================================================
// 辅助（与 textedit_frame_perf_test.rs 同源，见文件头「关于重复的辅助函数」）
// ============================================================================

/// 合成接近 `target_bytes` 的中文 Markdown 语料（整块拼接，不会切断 UTF-8）。
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

/// 造一个已打开 `big.md` 的应用，装配**可控时钟**（理由见基线用例的同名函数）。
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
fn editor_frame(
    ctx: &egui::Context,
    app: &mut EditorApp,
    clock: &TestClock,
    time_s: &mut f64,
    events: Vec<egui::Event>,
) -> f64 {
    *time_s += FRAME_MS as f64 / 1000.0;
    clock.advance(FRAME_MS);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(SCREEN[0], SCREEN[1]),
        )),
        time: Some(*time_s),
        events,
        ..Default::default()
    };
    let start = Instant::now();
    let _ = crate::support::run_ui(ctx, input, |ui| {
        mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(app, ui);
    });
    start.elapsed().as_secs_f64() * 1000.0
}

/// 建 Context、关预览与侧栏（隔离编辑区）、装载字库、预热两帧、把焦点交给编辑区。
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

fn pct(mut samples: Vec<f64>, q: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).expect("耗时不应为 NaN"));
    let idx = ((samples.len() as f64 * q) as usize).min(samples.len() - 1);
    samples[idx]
}

// ============================================================================
// E1：现状（真实 EditorApp + 我们的逐行 layouter）
// ============================================================================

/// 1MB 击键帧的耗时，以及**每帧真正重建了几行**（`HighlightStats::reshaped_lines`）。
///
/// 返回 `(每帧毫秒, 每帧重建行数)`。
///
/// 后者是这个用例里最硬的一条证据：若它恒为个位数，则「文本一变即重建 58,514 行
/// `Galley`」的说法当场被证伪——耗时再大也不是从「重建 58k 行」来的。
fn e1_current_impl() -> (Vec<f64>, Vec<usize>, f64) {
    let (_ws, _cfg, mut app, clock, text) = app_with_doc_and_clock(MIB);
    let (ctx, mut time_s) = warm_editor(&mut app, &clock);

    // 同一运行内的空闲帧对照（1MB 档的击键帧差值要减掉它）。
    let idle: Vec<f64> = (0..5)
        .map(|_| editor_frame(&ctx, &mut app, &clock, &mut time_s, Vec::new()))
        .collect();

    let frames = frames_for(100);
    let mut reshaped = Vec::with_capacity(frames);
    let samples: Vec<f64> = (0..frames)
        .map(|_| {
            let ms = editor_frame(
                &ctx,
                &mut app,
                &clock,
                &mut time_s,
                vec![egui::Event::Text("a".to_owned())],
            );
            // `last_highlight_stats()` 是本帧 layouter 那次 `highlight_layout_with_find`
            // 留下的统计；文本每帧都变 → 整文档缓存必 miss → 每帧都会被更新。
            reshaped.push(mapper::last_highlight_stats().reshaped_lines);
            ms
        })
        .collect();

    // 自检：100 次输入必须真的逐字落进文档，否则量到的是空闲帧（断言会空转）。
    let after = app.current_content().map(|c| c.len()).unwrap_or(0);
    assert_eq!(
        after,
        text.len() + frames,
        "E1：{frames} 次 `Event::Text` 应使文档恰好增长 {frames} 字节（实际 {} → {}）——\
         焦点未落在编辑区时本组会退化成空闲帧测量",
        text.len(),
        after
    );

    (samples, reshaped, pct(idle, 0.50))
}

// ============================================================================
// E2：排版层单独调用（拆行 + 差分 + 重排 1 行 + concat）
// ============================================================================

/// 只量**本项目排版层**：以 `seed` 为基准连续追加字符，每追加一个测一次
/// `highlight_layout_with_find`。
///
/// 形状与连续打字完全一致：每次调用相对上一次只多一个字节，故内部走
/// 「整文档缓存 miss → 拆行 → 差分映射 → 只重排受影响的行 → `concat` 拼回」。
///
/// `seed` 必须是 E1 结束时的文档内容——这样预热一次就能把两层缓存对齐到
/// `(文本, 换行宽度)`，样本里不再混入一次性整篇重建。
///
/// 返回 `(变更帧样本, 命中帧样本)`。后者是同一文本重复调用的**整文档缓存命中**
/// 路径（函数开头直接 `return` 缓存里的 `Arc`），用来把「变更帧的成本」与
/// 「一个函数调用的底价」分开——否则无法判断 9ms 是花在缓存簿记上还是调用本身。
fn e2_layouter_only(
    ctx: &egui::Context,
    theme: &rustmark::presentation::theme::builtin::Theme,
    seed: &str,
) -> (Vec<f64>, Vec<f64>) {
    // 与编辑器视口同量级的换行宽度；预热一次后，缓存即以它为键。
    let wrap = 1000.0_f32;
    let mut cur = seed.to_owned();
    // 预热（不计时）：把两层缓存对齐到 `(cur, wrap)`。
    let _ = mapper::highlight_layout_with_find(ctx, theme, &cur, None, wrap, None);

    let n = frames_for(20);
    let mut reshaped = Vec::with_capacity(n);
    let changed: Vec<f64> = (0..n)
        .map(|_| {
            cur.push('b'); // 一次「击键」
            let start = Instant::now();
            let g = mapper::highlight_layout_with_find(ctx, theme, &cur, None, wrap, None);
            std::hint::black_box(&g);
            reshaped.push(mapper::last_highlight_stats().reshaped_lines);
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();

    // E2b：文本不变 → 整文档缓存命中（对照）。
    let hit: Vec<f64> = (0..n)
        .map(|_| {
            let start = Instant::now();
            let g = mapper::highlight_layout_with_find(ctx, theme, &cur, None, wrap, None);
            std::hint::black_box(&g);
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();

    let mut distinct = reshaped.clone();
    distinct.sort_unstable();
    distinct.dedup();
    println!(
        "[metrics] E2 排版层单次增量（1MB 纯文本）：重建行数 {distinct:?}（共 {} 次调用）",
        reshaped.len()
    );
    (changed, hit)
}

// ============================================================================
// E3 / E7：裸 TextEdit —— 换 layouter，宿主不变
// ============================================================================

/// 裸 `TextEdit` 一帧：注入 `events`，推进 egui 时间 8ms，返回耗时（毫秒）。
///
/// 结构对齐真实编辑区（`ScrollArea::vertical` + 撑满视口的 `TextEdit`），但宿主是
/// 裸控件——没有 `EditorApp` 的每帧前置工作、diff 回写、行号栏与工具栏。
/// `custom` 为 `None` 时用 egui 默认 layouter（E3）；给出主题则挂本项目排版层（E7）。
///
/// E3 与 E7 的**唯一**差别就是 layouter，因此 `E7 − E3` 干净地隔离出
/// 「自家逐行排版相对上游按段落排版的净开销」。
fn bare_frame(
    ctx: &egui::Context,
    buf: &mut String,
    id: egui::Id,
    rows: usize,
    time_s: &mut f64,
    events: Vec<egui::Event>,
    custom: Option<&rustmark::presentation::theme::builtin::Theme>,
) -> f64 {
    *time_s += FRAME_MS as f64 / 1000.0;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(SCREEN[0], SCREEN[1]),
        )),
        time: Some(*time_s),
        events,
        ..Default::default()
    };
    let start = Instant::now();
    let _ = crate::support::run_ui(ctx, input, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let viewport_h = ui.clip_rect().height();
                // 自定义 layouter 的闭包不能借用 `app`（与 `editor_view::show` 同因），
                // 故这里把主题克隆进闭包。
                let mut layouter = custom.map(|theme| {
                    let theme = theme.clone();
                    move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_arg: f32| {
                        mapper::highlight_layout_with_find(
                            ui.ctx(),
                            &theme,
                            buf.as_str(),
                            None,
                            wrap_arg,
                            None,
                        )
                    }
                });
                let mut te = egui::TextEdit::multiline(buf)
                    .id(id)
                    .font(egui::FontId::monospace(14.0))
                    .desired_rows(rows)
                    .desired_width(ui.available_width())
                    .min_size(egui::vec2(0.0, viewport_h))
                    .code_editor();
                if let Some(l) = layouter.as_mut() {
                    te = te.layouter(
                        l as &mut dyn FnMut(
                            &egui::Ui,
                            &dyn egui::TextBuffer,
                            f32,
                        ) -> Arc<egui::Galley>,
                    );
                }
                te.show(ui);
            });
    });
    start.elapsed().as_secs_f64() * 1000.0
}

/// 同一份文本、同一个裸宿主，只换 layouter，返回每帧毫秒。
///
/// `custom = None` ⇒ 上游默认路径（分段缓存）；`Some(theme)` ⇒ 本项目逐行排版层。
fn bare_textedit_samples(
    text: &str,
    rows: usize,
    frames: usize,
    custom: Option<&rustmark::presentation::theme::builtin::Theme>,
) -> Vec<f64> {
    let ctx = egui::Context::default();
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {}); // 装载字库
    let id = egui::Id::unique("bare-textedit-probe");
    let mut buf = text.to_owned();
    let mut time_s = 0.0_f64;

    // 预热两帧：首帧是 egui 的测量 pass；随后把焦点交给控件（否则注入的
    // `Event::Text` 落不到它身上，量到的会是空转帧）。
    let _ = bare_frame(&ctx, &mut buf, id, rows, &mut time_s, Vec::new(), custom);
    ctx.memory_mut(|m| m.request_focus(id));
    let _ = bare_frame(&ctx, &mut buf, id, rows, &mut time_s, Vec::new(), custom);

    let before = buf.len();
    let samples: Vec<f64> = (0..frames)
        .map(|_| {
            bare_frame(
                &ctx,
                &mut buf,
                id,
                rows,
                &mut time_s,
                vec![egui::Event::Text("a".to_owned())],
                custom,
            )
        })
        .collect();

    // 自检：焦点必须真的落上（否则这一组量的不是「输入」）。
    assert_eq!(
        buf.len(),
        before + frames,
        "裸 TextEdit：`Event::Text` 未落进控件（{} → {}）——焦点未生效，样本无效",
        before,
        buf.len()
    );
    samples
}

// ============================================================================
// 归因
// ============================================================================

/// 五组对照，产出 1MB 击键帧的成本归属。
#[test]
fn profile_1mb_typing_frame_cost_attribution() {
    let big = sample(MIB);
    let rows = big.split('\n').count();

    // ---- E1：现状 ----
    let (e1, reshaped, e1_idle) = e1_current_impl();
    let e1_p50 = pct(e1.clone(), 0.50);
    let e1_p95 = pct(e1, 0.95);
    let worst_reshaped = reshaped.iter().copied().max().unwrap_or(0);
    println!(
        "[metrics] E1 现状（EditorApp + 逐行 layouter）1MB：p50={e1_p50:.2}ms p95={e1_p95:.2}ms ｜ \
         空闲帧 p50={e1_idle:.2}ms ｜ 每帧重建行数 max={worst_reshaped}（共 {} 帧）",
        reshaped.len()
    );

    // 硬证据：文本每帧都在变，但重建的行数必须是「个位数」。
    // 这条断言钉住「逐行增量确实生效」，并证伪「每帧重建 58k 行」。
    assert!(
        worst_reshaped <= 4,
        "1MB 击键帧有帧重建了 {worst_reshaped} 行（应为个位数）——\
         逐行增量缓存（LINE_LAYOUT）疑似失效，文本一变就整篇重排"
    );

    // ---- E2 / E2b：排版层单独调用 ----
    // 复用真实装配以取得主题，seed 取 E1 结束时的文档内容，让差分路径直接命中。
    let (_ws, _cfg, mut app, clock, _text) = app_with_doc_and_clock(MIB);
    let (ctx, mut time_s) = warm_editor(&mut app, &clock);
    let _ = editor_frame(&ctx, &mut app, &clock, &mut time_s, Vec::new());
    let seed = app.current_content().unwrap_or_default();
    let theme = app.ui.theme.clone();
    let (e2, e2_hit) = e2_layouter_only(&ctx, &theme, &seed);
    let e2_p50 = pct(e2.clone(), 0.50);
    let e2_p95 = pct(e2, 0.95);
    let e2b_p50 = pct(e2_hit, 0.50);
    println!(
        "[metrics] E2 排版层单次增量（1MB 纯文本）：p50={e2_p50:.2}ms p95={e2_p95:.2}ms ｜ \
         E2b 整文档缓存命中 p50={e2b_p50:.3}ms"
    );

    // ---- E3 / E7：裸 TextEdit，宿主相同、只换 layouter ----
    let bare_frames = frames_for(20);
    let e3 = bare_textedit_samples(&big, rows, bare_frames, None);
    let e3_p50 = pct(e3.clone(), 0.50);
    let e3_p95 = pct(e3, 0.95);
    let e7 = bare_textedit_samples(&big, rows, bare_frames, Some(&theme));
    let e7_p50 = pct(e7.clone(), 0.50);
    let e7_p95 = pct(e7, 0.95);
    println!(
        "[metrics] 裸 TextEdit 1MB（同一宿主、同一文本，只换 layouter）：\
         上游默认 layouter p50={e3_p50:.2}ms p95={e3_p95:.2}ms ｜ \
         本项目逐行 layouter p50={e7_p50:.2}ms p95={e7_p95:.2}ms"
    );

    // ---- E8：应用层头号嫌疑 —— `editor_ops::diff_replace` ----
    //
    // `editor_view::show` 在 `response.changed()` 分支里每帧调它一次。
    // 旧实现（049 第一轮量得 6.93ms）先 `old.chars().collect::<Vec<char>>()`
    // 再 `new.chars().collect()`——**1MB 文本 = 两次 35 万字符的 `Vec<char>`
    // （约 2.8MB 分配 + 两遍 UTF-8 解码）**，末尾还要两次 O(k) 的 `char_to_byte`。
    // 与问题 046「为两个整数付 O(全文)」同一性质。
    //
    // 049 已把它改为**字节层**求前后缀（不分配、不解码）。新实现的成本不再与
    // 文档长度成正比、而与**改动位置**相关：前缀越长、那次 `byte_to_char`
    // 的字符计数越贵。故这里同时量两个位置：
    //
    // - **E8 文末追加**（前缀 = 全文，最坏位置，也是用户连续打字的主场景）；
    // - **E8b 文档中点替换**（前缀 = 半篇），用来看位置敏感度有多大。
    let mut edited_tail = big.clone();
    edited_tail.push('a');
    // 样本数**固定 20**，不随 `frames_for` 缩放：单项探针很快（release 0.3ms），
    // 而 debug 下 `frames_for(20)` 只给 5 个样本，p50 会在阈值附近抖动
    // （实测过一次 1.87 → 越过 2.0 的假红）。
    const E8_SAMPLES: usize = 20;
    let e8: Vec<f64> = (0..E8_SAMPLES)
        .map(|_| {
            let start = Instant::now();
            let e = rustmark::presentation::views::editor_ops::diff_replace(&big, &edited_tail);
            std::hint::black_box(&e);
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    let e8_p50 = pct(e8, 0.50);

    let mid = big.len() / 2;
    let mid = (0..=mid)
        .rev()
        .find(|&i| big.is_char_boundary(i))
        .unwrap_or(0);
    let mut edited_mid = big.clone();
    edited_mid.replace_range(mid..mid, "x");
    let e8b_p50 = bench_p50(E8_SAMPLES, || {
        let e = rustmark::presentation::views::editor_ops::diff_replace(&big, &edited_mid);
        std::hint::black_box(&e);
    });
    println!(
        "[metrics] E8 `editor_ops::diff_replace`（1MB，字节层实现）：\
         文末追加（前缀=全文，最坏）p50={e8_p50:.2}ms ｜ 文档中点替换 p50={e8b_p50:.2}ms"
    );
    // 硬证据：字节层实现必须远离旧实现的量级——否则说明它退化回了
    // 「无论改哪里都整篇过一遍」。这是**量级**守卫，不是精确耗时断言；
    // 阈值按档位给（debug 无内联无优化，实测 1.2 / 1.9ms，同机 release 0.24 / 0.33），
    // 与 `textedit_frame_perf_test::typing_frame_limit_ms` 同一套口径。
    // 旧实现（字符层）release 6.93ms ⇒ debug 约 55ms，两侧都远超对应上限，
    // 故退化必然被抓。
    let e8_limit = if cfg!(debug_assertions) { 16.0 } else { 2.0 };
    assert!(
        e8_p50 < e8_limit && e8b_p50 < e8_limit,
        "diff_replace 字节层实现退化：文末 {e8_p50:.2}ms / 中点 {e8b_p50:.2}ms \
         （上限 {e8_limit:.0}ms；释放档旧字符层实现为 6.93ms——超过上限说明\
         又出现了 O(全文) 的分配或解码）"
    );

    // ---- 归因判读 ----
    let delta = e1_p50 - e1_idle;
    println!(
        "[metrics] 归因（p50，ms）：E1 现状 {e1_p50:.2} = 空闲帧 {e1_idle:.2} + 文本变更 {delta:.2}；\
         文本变更中「本项目排版层」(E2) {e2_p50:.2}、`diff_replace`(E8) {e8_p50:.2}、\
         余下（`TextEdit` 自身 + `prev` 克隆 + 最终串重建 + 领域回写）{:.2}；\
         裸宿主口径：自家 layouter 比上游贵 {:.2}（E7−E3）、应用层净开销 {:.2}（E1−E7）",
        delta - e2_p50 - e8_p50,
        e7_p50 - e3_p50,
        e1_p50 - e7_p50
    );
}

// ============================================================================
// 第二轮（049 续）：状态栏消融 + 单项 O(全文) 探针
// ============================================================================

/// 对同一闭包取 `n` 次样本的 p50（毫秒）。
///
/// 单项探针统一走它，保证与 E1/E2 同口径（p50、release 取数）。
fn bench_p50(n: usize, mut f: impl FnMut()) -> f64 {
    pct(
        (0..n)
            .map(|_| {
                let start = Instant::now();
                f();
                start.elapsed().as_secs_f64() * 1000.0
            })
            .collect(),
        0.50,
    )
}

/// 1MB 档击键帧实测，`status_bar` 决定状态栏是否参与本帧绘制。
///
/// 这是**消融实验**：两次运行除了状态栏可见性外代码路径完全相同，故
/// 「开 − 关」把状态栏每帧成本干净地隔离出来（含它内部那几笔 `O(全文)`
/// 查询）。返回 `(击键帧样本, 空闲帧 p50)`。
fn typing_frames_with_status_bar(bytes: usize, status_bar: bool) -> (Vec<f64>, f64) {
    let (_ws, _cfg, mut app, clock, text) = app_with_doc_and_clock(bytes);
    let (ctx, mut time_s) = warm_editor(&mut app, &clock);
    app.ui.status_bar_visible = status_bar;
    // 可见性变化会改变主体区高度 → 先跑两帧让布局稳定，再开始采样。
    let _ = editor_frame(&ctx, &mut app, &clock, &mut time_s, Vec::new());
    let _ = editor_frame(&ctx, &mut app, &clock, &mut time_s, Vec::new());

    let idle: Vec<f64> = (0..5)
        .map(|_| editor_frame(&ctx, &mut app, &clock, &mut time_s, Vec::new()))
        .collect();

    let frames = frames_for(30);
    let samples: Vec<f64> = (0..frames)
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

    // 自检：样本必须真的含输入（否则量到的是空闲帧）。
    let after = app.current_content().map(|c| c.len()).unwrap_or(0);
    assert_eq!(
        after,
        text.len() + frames,
        "A2：{frames} 次 `Event::Text` 应使文档恰好增长 {frames} 字节（实际 {} → {}）",
        text.len(),
        after
    );
    (samples, pct(idle, 0.50))
}

/// 领域替换链路的 p50：`replace_range` → `Document::apply`（含撤销快照）。
///
/// 走**真实应用服务**而不是合成字符串操作：这条链路上还有别的 `O(全文)`
/// 动作（编辑前快照、事件载荷），合成探针测不到它们。
fn domain_replace_range_p50(bytes: usize, n: usize) -> f64 {
    let (_ws, _cfg, mut app, clock, _text) = app_with_doc_and_clock(bytes);
    let id = app.active_doc_for_probe();
    let len = app.current_content().map(|c| c.len()).unwrap_or(0);
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        // 推进时钟：撤销合并窗口是 400ms，这里按真实击键节奏（8ms）走，
        // 即「连续打字」的真实合并行为，而非每次都产生新撤销点。
        clock.advance(FRAME_MS);
        let at = len + i;
        let start = Instant::now();
        let _ = app.services.documents.replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(at), ByteOffset::new(at)),
            text: "a".to_owned(),
        });
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    pct(samples, 0.50)
}

/// 击键帧 `get_stats` 的 **miss 路径**：先编辑（`revision` +1）再查统计。
///
/// 状态栏每帧查两次统计，缓存键是 `(id, revision)`（`queries.rs::get_stats`）；
/// 击键帧 `revision` 变了必然 miss → `compute_stats(doc.text())`，即
/// **整篇克隆 + 全文按行统计**。它正是「状态栏消融」量到的那笔钱，
/// 但 `compute_stats` 是私有函数，只能这样经真实链路测。
fn stats_after_edit_p50(bytes: usize, n: usize) -> f64 {
    let (_ws, _cfg, mut app, clock, _text) = app_with_doc_and_clock(bytes);
    let id = app.active_doc_for_probe();
    let len = app.current_content().map(|c| c.len()).unwrap_or(0);
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        clock.advance(FRAME_MS);
        let at = len + i;
        let _ = app.services.documents.replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(at), ByteOffset::new(at)),
            text: "a".to_owned(),
        });
        // 只计查询本身：编辑（0.29ms 量级）已在上一条探针里单独量过。
        let start = Instant::now();
        let _ = app.current_stats();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    pct(samples, 0.50)
}

/// 1MB 档：把 E1 归因里「余下的 17.68ms」拆到具体代码行。
#[test]
fn profile_1mb_app_layer_whole_text_costs() {
    let big = sample(MIB);
    let n = frames_for(20);

    // ---- A2：状态栏消融 ----
    let (on, on_idle) = typing_frames_with_status_bar(MIB, true);
    let (off, off_idle) = typing_frames_with_status_bar(MIB, false);
    let on_p50 = pct(on, 0.50);
    let off_p50 = pct(off, 0.50);
    println!(
        "[metrics] A2 状态栏消融（1MB 击键帧 p50）：开 {on_p50:.2}ms（空闲 {on_idle:.2}）｜\
         关 {off_p50:.2}ms（空闲 {off_idle:.2}）｜状态栏每帧 ≈ {:.2}ms",
        on_p50 - off_p50
    );

    // ---- S1：整篇克隆（`prev.clone()` 与撤销快照同源）----
    let s1 = bench_p50(n, || {
        let c = big.clone();
        std::hint::black_box(&c);
    });

    // ---- S2：`format!` 重建整篇最终串（模拟打字到中部）----
    // 取中点并**退回字符边界**：1MB 中文里 `len/2` 有一半概率落在字符中间，
    // 直接切片会 panic（`panic = "abort"` 下即终止）。
    let mut cut = big.len() / 2;
    while !big.is_char_boundary(cut) {
        cut -= 1;
    }
    let s2 = bench_p50(n, || {
        let t = format!("{}{}{}", &big[..cut], "a", &big[cut..]);
        std::hint::black_box(&t);
    });
    // 真实击键路径的另一半成本：egui 在 `&mut String` 上原地插入一个字符。
    let s2b = bench_p50(n, || {
        let mut t = big.clone();
        t.insert(cut, 'a');
        std::hint::black_box(&t);
    });

    // ---- S3 / S4：每帧全文扫描 ----
    let s3 = bench_p50(n, || {
        let v = scroll_sync::line_start_offsets(&big);
        std::hint::black_box(&v);
    });
    let s4 = bench_p50(n, || {
        let c = rustmark::shared::text::line_count(&big);
        std::hint::black_box(&c);
    });

    // ---- S5 / S5b / S6：领域替换链路与统计查询（长度斜率）----
    let s5 = domain_replace_range_p50(MIB, n);
    let s5b = domain_replace_range_p50(10 * 1024, n);
    let s6 = stats_after_edit_p50(MIB, n);

    println!(
        "[metrics] 单项探针（1MB，p50）：S1 整篇克隆 {s1:.2}ms ｜ S2 `format!` 重建 {s2:.2}ms ｜ \
         S2b 原地 insert {s2b:.2}ms ｜ S3 `line_start_offsets` {s3:.2}ms ｜ S4 `line_count` {s4:.2}ms"
    );
    println!(
        "[metrics] S5 领域 `replace_range`：1MB {s5:.2}ms ｜ 10KB {s5b:.2}ms ｜ \
         每 MB 增量 {:.2}ms（提示其中与文档长度成正比的部分）",
        s5 - s5b
    );
    println!(
        "[metrics] S6 `get_stats` miss 帧（= 单次整篇统计）：1MB {s6:.2}ms —— \
         建议一（问题 039）落地前，A2 量到的状态栏「击键帧 − 空闲帧」差值≈{:.2}ms 主要由它解释；\
         防抖后该差值应降为接近 0（打字期不再每击键重算，仅停手满窗口才付一次 {s6:.2}ms）",
        (on_p50 - on_idle) - (off_p50 - off_idle)
    );
    println!(
        "[metrics] 换算：击键帧内至少 2 次整篇克隆（`prev.clone()` + 撤销快照 {s1:.2}ms 各）\
         + 1 次最终串重建（{s2:.2}ms）≈ {:.2}ms —— 与 egui / 自绘无关；\
         整篇统计 {s6:.2}ms 现已被防抖摊销，不再计入每击键帧",
        s1 * 2.0 + s2
    );

    // 建议一（问题 039）落地后：状态栏统计已防抖，打字期不再每击键整篇重算
    // （`compute_stats` 在 1MB 上 ≈ 9.5ms，见 `docs/metrics.md` §3.2.5）。
    // 故「开 / 关状态栏」的击键帧 p50 差值应从「> 2ms」降为「接近 0」
    // （只余 O(1) 的节拍器比对 + 一次缓存克隆）。这条断言钉住「防抖确实生效、
    // 热点已消除」，而非相反——若它显著 > 0，说明 `compute_stats` 仍在每击键跑。
    //
    // 该探针在 **release** 口径下才有意义：debug 下 `bench_p50` 仅取 5 个样本，
    // 1MB 击键帧方差极大，且与 540 个其他测试并发跑时的时间抖动会把它推过阈值，
    // 造成与逻辑无关的偶发失败。故断言只在 release 生效；debug 仍打印数字供肉眼核对。
    #[cfg(not(debug_assertions))]
    assert!(
        on_p50 - off_p50 < 3.0,
        "A2：防抖后状态栏每帧增量应为接近 0，实测 {:.2}ms（开 {on_p50:.2} / 关 {off_p50:.2}）——\
         若显著 > 0，说明 `compute_stats` 仍在每击键跑，建议一防抖失效",
        on_p50 - off_p50
    );
}
