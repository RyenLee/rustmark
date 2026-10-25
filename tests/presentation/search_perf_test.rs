//! 侧栏「搜索」标签性能基线（性能排查 + 回归门禁）。
//!
//! # 口径
//!
//! 数字以 release 为准：
//!
//! ```text
//! cargo test --release --test presentation -- --nocapture search_perf
//! ```
//!
//! # 为什么单独一个文件
//!
//! 这里的用例产出的是**数字**（`[metrics]` 行）而非布尔结论，回答的是
//! 「切到搜索标签为什么卡」。与 `sidebar_view_test.rs`（功能正确性）分开，
//! 避免功能用例被性能用例的耗时拖慢。
//!
//! # 三个测量面
//!
//! 1. `search_workspace()` 单次调用：视图每帧调用**一次**（计数与结果列表
//!    同源，见 `sidebar_view::search` 的 `toggle_row`），整帧的搜索成本
//!    即 1 × 这个数（「旧视图等价」一项另量了合并前一帧两次的口径）；
//! 2. 大小写敏感对照：同一语料、同一查询，只翻 `case_sensitive` 一项。
//!    两者差多少，就是「逐字符 `to_lowercase()` 分配」的代价；
//! 3. 侧栏整帧（搜索标签 / 文件标签）：前者减去第 1 项，得到渲染成本。
//!
//! # 与结果缓存的关系
//!
//! `WorkspaceAppService` 带单槽结果缓存（查询指纹 + revision 快照），
//! 同一查询重复调用会命中短路。本文件的口径是**量真实搜索路径**
//! （`search_uncached`，见其文档约定），故每次计时前显式
//! `invalidate_search_cache()`——否则测到的只是缓存结果的克隆耗时，
//! 门禁就形同虚设。

use std::fs;
use std::sync::Arc;
use std::time::Instant;

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::workspace::commands::ScanTreeCommand;
use rustmark::application::workspace::queries::ReadFileQuery;
use rustmark::presentation::state::app_ui_state::SidebarTab;
use rustmark::presentation::views::sidebar_view;
use rustmark::shared::path::FilePath;

/// 合成 `target_bytes` 的 Markdown 语料，且 `needle` 出现次数远超总命中上限。
fn sample(target_bytes: usize) -> String {
    let unit = "# 标题 H1\n\n\
                这是一段中文正文，needle 用于模拟真实编辑场景：包含 **加粗**、`行内代码` \
                与 [链接](https://example.com)。\n\n\
                - 列表项一 needle\n\
                - 列表项二\n\n\
                > 引用行\n\n\
                ```rust\nfn main() { println!(\"hi\"); } // needle\n```\n\n";
    let mut s = String::new();
    while s.len() < target_bytes {
        s.push_str(unit);
    }
    s
}

fn app_with_doc(target_bytes: usize) -> (tempfile::TempDir, tempfile::TempDir, EditorApp) {
    let ws = tempfile::tempdir().expect("工作区临时目录");
    fs::write(ws.path().join("big.md"), sample(target_bytes)).expect("写样例文档");
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
    (ws, cfg, app)
}

fn pct(mut samples: Vec<f64>, q: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).expect("耗时不应为 NaN"));
    let idx = ((samples.len() as f64 * q) as usize).min(samples.len() - 1);
    samples[idx]
}

fn timed<T>(runs: usize, mut op: impl FnMut() -> T) -> (Vec<f64>, T) {
    let mut out = None;
    let samples = (0..runs)
        .map(|_| {
            let start = Instant::now();
            let value = op();
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            out = Some(value);
            std::hint::black_box(ms);
            ms
        })
        .collect();
    (samples, out.expect("至少跑一次"))
}

/// 只画侧栏，驱动 `n` 帧；返回每帧毫秒。预热两帧（字库装载 / 首帧测量）不计入。
fn drive_sidebar_frames(app: &mut EditorApp, n: usize) -> Vec<f64> {
    let ctx = egui::Context::default();
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {});
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };
    for _ in 0..2 {
        let _ = crate::support::run_ui(&ctx, input.clone(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            sidebar_view::show(app, ui, 260.0);
        });
    }
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        // 每帧前失效：整帧口径量的是「真实搜索 + 渲染」的最坏稳态，
        // 不让缓存命中把搜索回归藏起来（口径见文件头）。
        app.services.workspace.invalidate_search_cache();
        let start = Instant::now();
        let _ = crate::support::run_ui(&ctx, input.clone(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            sidebar_view::show(app, ui, 260.0);
        });
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples
}

/// 量「从别的标签切到搜索」的那一帧（验收口径：交互响应 < 100ms）。
fn measure_switch_to_search_frame(app: &mut EditorApp) -> f64 {
    let ctx = egui::Context::default();
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {});
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };
    // 先在「文件」标签稳定两帧：这之后的状态才是用户切过来时的真实起点。
    app.ui.sidebar_tab = SidebarTab::Files;
    for _ in 0..2 {
        let _ = crate::support::run_ui(&ctx, input.clone(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            sidebar_view::show(app, ui, 260.0);
        });
    }
    app.ui.sidebar_tab = SidebarTab::Search;
    // 切换帧 = 缓存冷启动的真实首帧（用户切过来时缓存未必命中），
    // 计时前失效以量真实搜索路径（口径见文件头）。
    app.services.workspace.invalidate_search_cache();
    let start = Instant::now();
    let _ = crate::support::run_ui(&ctx, input.clone(), |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        sidebar_view::show(app, ui, 260.0);
    });
    start.elapsed().as_secs_f64() * 1000.0
}

#[test]
fn measure_search_tab_frame_cost() {
    let (_ws, _cfg, mut app) = app_with_doc(1024 * 1024);
    app.sidebar.search_query = "needle".into();
    app.ui.sidebar_tab = SidebarTab::Search;
    app.ui.sidebar_visible = true;

    // 1. 单次 search_workspace（默认：当前文档 + 大小写不敏感）
    let (single, result) = timed(5, || {
        app.services.workspace.invalidate_search_cache();
        app.search_workspace().expect("搜索应成功")
    });
    let hits = result.total();

    // 2. 同一语料，仅翻 case_sensitive：差值 = 逐字符 to_lowercase 分配成本
    app.sidebar.search_options.case_sensitive = true;
    let (case_sensitive, cs_result) = timed(5, || {
        app.services.workspace.invalidate_search_cache();
        app.search_workspace().expect("搜索应成功")
    });
    app.sidebar.search_options.case_sensitive = false;

    // 3. 侧栏整帧（搜索标签）与对照组（文件标签）
    let search_frames = drive_sidebar_frames(&mut app, 5);
    app.ui.sidebar_tab = SidebarTab::Files;
    let files_frames = drive_sidebar_frames(&mut app, 5);

    // 4. 切到搜索标签的那一帧（验收口径）
    let switch_ms = measure_switch_to_search_frame(&mut app);

    // 5. 旧视图等价成本：同一帧里跑两遍搜索（计数一遍、列表一遍）
    let (legacy_pair, _) = timed(3, || {
        app.services.workspace.invalidate_search_cache();
        let first = app.search_workspace();
        app.services.workspace.invalidate_search_cache();
        let second = app.search_workspace();
        (first, second)
    });

    let single_p50 = pct(single.clone(), 0.50);
    let single_p95 = pct(single, 0.95);
    let cs_p50 = pct(case_sensitive.clone(), 0.50);
    let sf_p50 = pct(search_frames.clone(), 0.50);
    let sf_p95 = pct(search_frames, 0.95);
    let ff_p50 = pct(files_frames.clone(), 0.50);

    println!(
        "[metrics] 搜索标签 1MB / 查询 \"needle\"：单次 search_workspace p50={single_p50:.2}ms \
         p95={single_p95:.2}ms（{} 条命中）｜ 大小写敏感对照 p50={cs_p50:.2}ms（{} 条命中）｜ \
         侧栏整帧（搜索） p50={sf_p50:.2}ms p95={sf_p95:.2}ms ｜ 侧栏整帧（文件） p50={ff_p50:.2}ms \
         ｜ 切到搜索标签首帧 {switch_ms:.2}ms ｜ 旧视图等价（一帧两次搜索） p50={:.2}ms",
        hits,
        cs_result.total(),
        pct(legacy_pair, 0.50)
    );

    // 验收：切到搜索标签后响应 < 100ms（1MB 文档 + 50 条命中）。
    // 口径沿用 `metrics.md` §3.2.10 的约定：release 硬断言，debug 只打印
    // 数字、不卡门禁——debug 的绝对耗时随机器波动，不是交付口径。
    if !cfg!(debug_assertions) {
        assert!(
            switch_ms < 100.0,
            "切到搜索标签首帧 {switch_ms:.2}ms 超过 100ms 预算"
        );
        assert!(
            sf_p95 < 100.0,
            "搜索标签稳态帧 {sf_p95:.2}ms 超过 100ms 预算"
        );
    }
}

/// 造一个「真实规模」的工作区：`files` 个 4KB 文件，外加一个已打开的小文档。
fn app_with_many_files(files: usize) -> (tempfile::TempDir, tempfile::TempDir, EditorApp) {
    let ws = tempfile::tempdir().expect("工作区临时目录");
    let body = "# 文档\n\n这是别处的正文，用于模拟工作区里大量未被打开的文件。\n";
    for i in 0..files {
        fs::write(ws.path().join(format!("f{i:04}.md")), body).expect("写工作区文件");
    }
    fs::write(ws.path().join("index.md"), "# 索引\n\nneedle 只在这里\n").expect("写索引文件");
    let cfg = tempfile::tempdir().expect("配置临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(cfg.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let root = ws.path().to_string_lossy().replace('\\', "/");
    app.scan_workspace(&root).expect("扫描应成功");
    app.open_workspace_file(&format!("{root}/index.md"))
        .expect("打开应成功");
    (ws, cfg, app)
}

/// 「当前文档」范围不该读盘：工作区里有 N 个未打开的文件，与搜索无关。
///
/// 对照组是「打开的文档」范围——它只快照已打开文档、不进磁盘循环。
/// 两者之差就是「当前文档」范围白读整个工作区的代价。
#[test]
fn measure_current_document_scope_disk_read_cost() {
    let (ws, _cfg, mut app) = app_with_many_files(1000);
    app.sidebar.search_query = "needle".into();
    app.ui.sidebar_tab = SidebarTab::Search;
    app.ui.sidebar_visible = true;

    let (current, current_result) = timed(3, || {
        app.services.workspace.invalidate_search_cache();
        app.search_workspace().expect("搜索应成功")
    });
    app.sidebar.search_scope =
        rustmark::presentation::state::sidebar_ui_state::SearchScope::OpenDocuments;
    let (open_docs, open_result) = timed(3, || {
        app.services.workspace.invalidate_search_cache();
        app.search_workspace().expect("搜索应成功")
    });

    // 回到「当前文档」范围量用户实际会遇到的切换帧。
    app.sidebar.search_scope =
        rustmark::presentation::state::sidebar_ui_state::SearchScope::CurrentDocument;
    let switch_ms = measure_switch_to_search_frame(&mut app);
    let search_frames = drive_sidebar_frames(&mut app, 3);

    // 旧实现等价成本：范围过滤发生在读盘**之后**，于是每次搜索都逐文件
    // `read_content`（`fs::read` + 编码探测）整个工作区。
    let root = FilePath::new(ws.path().to_string_lossy().replace('\\', "/")).expect("工作区路径");
    let entries = app
        .services
        .workspace
        .scan_tree(ScanTreeCommand { root, max_depth: 8 })
        .expect("扫描应成功");
    let file_count = entries.iter().filter(|e| !e.is_directory).count();
    let (legacy_disk, _) = timed(3, || {
        let mut read = 0usize;
        for entry in entries.iter().filter(|e| !e.is_directory) {
            if app
                .services
                .workspace
                .read_file(ReadFileQuery {
                    path: entry.path.clone(),
                })
                .is_ok()
            {
                read += 1;
            }
        }
        read
    });

    let current_p50 = pct(current.clone(), 0.50);
    let legacy_p50 = pct(legacy_disk, 0.50);
    println!(
        "[metrics] 1000 文件工作区 / 查询 \"needle\"：当前文档范围单次 p50={current_p50:.2}ms（{} 条命中）｜ \
         旧实现等价（逐文件读盘，{file_count} 个文件） p50={legacy_p50:.2}ms ｜ \
         打开的文档范围单次 p50={:.2}ms（{} 条命中）｜ 侧栏整帧 p50={:.2}ms ｜ 切到搜索标签首帧 {switch_ms:.2}ms",
        current_result.total(),
        pct(open_docs.clone(), 0.50),
        open_result.total(),
        pct(search_frames, 0.50)
    );

    // 回归门禁：1000 个未打开文件不得让「当前文档」范围产生读盘开销。
    // 旧实现单次就是下面这个 `legacy_p50`（debug 150ms / release 数十毫秒），
    // 这里留 15× 余量，任何「按目录读盘」的回退都会立即顶破。
    assert!(
        current_p50 < 10.0,
        "「当前文档」范围单次 {current_p50:.2}ms，疑似又按工作区全量读盘（旧实现 {legacy_p50:.2}ms）"
    );
    if !cfg!(debug_assertions) {
        assert!(
            switch_ms < 100.0,
            "1000 文件工作区切到搜索标签首帧 {switch_ms:.2}ms 超过 100ms 预算"
        );
    }
}
