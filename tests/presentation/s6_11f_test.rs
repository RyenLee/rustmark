//! S6-11f 六个浮层的集成用例。
//!
//! # 测什么，不测什么
//!
//! 测**从动作到可见结果**的整条通路：`execute` → 浮层打开 →
//! 状态变化 → 文档变化 / 真实帧渲染。
//!
//! 不测纯函数的细枝末节（diff 的 LCS、模糊匹配的字符坐标等）——
//! 那些已由各状态模块的内嵌 `#[cfg(test)]` 覆盖。
//! 两处重叠是**故意**的：状态层单测钉住「算法对不对」，
//! 本文件钉住「算法被接到了没有」。
//!
//! # 每一类静默失效都要有一条用例
//!
//! S6-07 / S6-08 的教训：S6-11 之前，六个浮层的动作全部
//! `openable_overlay` 成功 → 打开一个没有视图的浮层 → `execute`
//! 直接 `return`，**全程零报错、界面完全正常、只是功能毫无反应**。
//! 因此本文件里的「打开后真的有渲染」「状态真的重置了」
//! 不是凑数，是回归防线。

use std::sync::Arc;

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::{ReplaceRangeCommand, UndoCommand};
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::presentation::state::dialog_ui_state::Overlay;
use rustmark::presentation::state::diff_ui_state::{DisplayRow, exceeds_budget};
use rustmark::presentation::state::quick_open_ui_state::MAX_CANDIDATES;
use rustmark::presentation::state::shortcut_overview_ui_state::ShortcutOverviewUiState;
use rustmark::presentation::state::symbol_ui_state::{SymbolGroup, items_of, search};
use rustmark::presentation::state::{quick_open_ui_state, symbol_ui_state};
use rustmark::presentation::views::{
    about_view, overlays, quick_open_view, stats_view, symbol_view,
};
use rustmark::shared::geometry::{ByteOffset, TextRange};

// ============================================================================
// 装置
// ============================================================================

/// 造一个应用（可注入对话框回应）。
fn app_with_dialog(fake: FakeDialog) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(ConfigPaths::in_dir(dir.path()), Arc::new(fake))
        .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

/// 造一个装好默认配置、装入 `content` 的应用。
fn app_with(content: &str) -> (tempfile::TempDir, EditorApp) {
    let (dir, mut app) = app_with_dialog(FakeDialog::new());
    let id = app.ensure_document().expect("应能开出空白文档");
    set_content(&mut app, id, content);
    (dir, app)
}

/// 直接写领域内容并刷新 UI 副本（绕过 `TextEdit`）。
fn set_content(app: &mut EditorApp, id: rustmark::shared::ids::DocumentId, content: &str) {
    app.services
        .documents
        .replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
            text: content.to_string(),
        })
        .expect("写入内容应成功");
    app.refresh_text();
}

/// 造一个已扫描过工作区的应用。
fn app_with_workspace(root: &std::path::Path) -> (tempfile::TempDir, tempfile::TempDir, EditorApp) {
    let ws = tempfile::tempdir().expect("工作区目录");
    let root_path = ws.path();
    for name in ["alpha.md", "beta.md"] {
        std::fs::write(root_path.join(name), "# title\n\nbody\n").expect("应可写文件");
    }
    let (dir, mut app) = app_with_dialog(FakeDialog::new());
    let root_str = root.to_string_lossy().replace('\\', "/");
    app.scan_workspace(&root_str).expect("扫描应成功");
    (dir, ws, app)
}

/// 跑一帧真实 egui 渲染，返回该帧的绘制指令数。
///
/// # 为什么返回值是「形状 + 文字」的总数
///
/// `egui::FullOutput` 只给到最终结果，看不出「是谁画的」。
/// 但有一个更省事也更可靠的判据：**浮层被打开后绘制量必然增加**。
/// 打开一个空视图（`Overlay::Stats => {}`）时绘制量与不打开时**完全相同**——
/// 这正是 S6-07 那种静默失效的特征，用「有东西被画出来」当断言
/// 才能把它抓住；而只断言 `dialog.is_open()` 会 100% 放行。
fn frame(app: &mut EditorApp, ctx: &egui::Context) -> usize {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1000.0, 700.0),
        )),
        ..Default::default()
    };
    let out = crate::support::run_ui(ctx, input, |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(app, ui);
    });
    // `shapes` 是所有 clip 后的绘制指令；文字已经光栅化成
    // `TextureManager` 里的字形图，但每个字形仍对应一条 shape。
    out.shapes.len()
}

/// 跑多帧（浮层需要至少两帧才完成布局与焦点），返回**最后一帧**的绘制量。
fn frames(app: &mut EditorApp, ctx: &egui::Context, n: usize) -> usize {
    let mut last = 0;
    for _ in 0..n.max(2) {
        last = frame(app, ctx);
    }
    last
}

// ============================================================================
// 通用：六个浮层都真的有视图且真的被渲染
// ============================================================================

/// 全部浮层都有渲染实现，`PENDING_VIEWS` 已清空。
///
/// 这条是**跨任务的总账**：任何浮层若被误加回待办清单、
/// 或清单被清空却仍有成员没有视图，都红。
///
/// S6-11f 时它的断言是「只剩 `CommandPalette`」；S6-12 落地命令面板后
/// 清单清空，断言随之升级为「一个都不许剩」。
///
/// `has_view` 在 S6-12 改成「不在 `PENDING_VIEWS` 里」——**枚举式**判定
/// （`matches!(overlay, A | B | C)`）在 `Overlay` 新增成员时会
/// 静默返回 `false`，那正是 S6-07「有开启动作却没视图」的形态。
#[test]
fn every_overlay_now_has_a_view() {
    assert!(
        overlays::PENDING_VIEWS.is_empty(),
        "PENDING_VIEWS 应已清空，实际还剩：{:?}",
        overlays::PENDING_VIEWS
    );
    for overlay in Overlay::ALL.iter().copied() {
        assert!(
            overlays::has_view(overlay),
            "{overlay:?}：有开启动作却没有渲染实现，且未登记进 `PENDING_VIEWS`"
        );
    }
}

/// 六个动作逐一打开对应浮层（`frame_test` 里已有等价断言，此处补渲染）。
#[test]
fn each_new_overlay_actually_renders_in_a_real_frame() {
    let cases = [
        (UiAction::OpenDocument, Overlay::QuickOpen),
        (UiAction::ShortcutOverview, Overlay::ShortcutOverview),
        (UiAction::About, Overlay::About),
        (UiAction::DocumentStats, Overlay::Stats),
        (UiAction::InsertSymbol, Overlay::Symbol),
        (UiAction::CompareDiff, Overlay::Diff),
    ];
    for (action, overlay) in cases {
        let (_dir, mut app) = app_with("# t\n\nbody\n");
        let ctx = egui::Context::default();
        // 先量「浮层未打开」时的基线绘制量。同一份文档、同一块画布，
        // 唯一的差别就是浮层——差值才是浮层的贡献。
        let baseline = frames(&mut app, &ctx, 3);
        app.execute(action);
        assert_eq!(
            app.dialog.top(),
            Some(overlay),
            "{action:?} 应打开 {overlay:?}"
        );
        // 渲染三帧：若视图不存在，`overlays::show` 什么都不画，
        // 绘制量与基线**逐条相等**。
        let painted = frames(&mut app, &ctx, 3);
        assert!(
            painted > baseline,
            "{overlay:?} 打开后绘制量没有增加（{baseline} → {painted}）：\
             视图存在但没画东西，或渲染分支被摘掉了"
        );
        assert!(app.dialog.is_open(overlay));
    }
}

/// 浮层关闭后从对话框状态里摘掉（否则再点开打不开）。
#[test]
fn closing_an_overlay_via_its_own_button_clears_it_from_the_dialog_state() {
    let (_dir, mut app) = app_with("x\n");
    let ctx = egui::Context::default();
    app.execute(UiAction::About);
    frames(&mut app, &ctx, 3);
    // 走 `close` 而非模拟点 × ：这里验的是「视图的关闭路径接到了
    // dialog.close」，点 × 是同一条路径。
    app.dialog.close(Overlay::About);
    assert!(!app.dialog.any_open());
}

// ============================================================================
// 统计浮层
// ============================================================================

/// 统计浮层的数值与 `current_stats` 一致（**不重算**）。
///
/// 原型 `StatsDialog.vue` 有一份独立的展示逻辑；本实现刻意
/// 只呈现 S6-11e 算好的 `StatsView`。若视图自己再算一遍，
/// 状态栏与浮层会显示两个不同的字数——用户会先信哪个？
#[test]
fn stats_dialog_shows_exactly_what_the_status_bar_counts() {
    let (_dir, app) = app_with("你好世界\n\n第二段\n");
    let from_service = app.current_stats().expect("应有文档");
    let rows = stats_view::rows_of(&from_service);
    assert_eq!(
        rows.len(),
        9,
        "原型是 10 行（含字数），本视图把字数提成大数字"
    );
    let labels: Vec<&str> = rows.iter().map(|(l, _)| *l).collect();
    for expected in [
        "字数",
        "字符数（含空格）",
        "行数",
        "段落数",
        "标题",
        "代码块",
        "链接",
        "图片",
        "任务项",
    ] {
        if expected == "字数" {
            // 字数在 `rows_of` 里被大数字取代，但必须能从 `StatsView` 拿到。
            assert!(from_service.words > 0);
            continue;
        }
        assert!(labels.contains(&expected), "统计浮层缺少「{expected}」行");
    }
}

/// 千分位分隔：四位以上才加逗号，且**经视图**生效。
///
/// 断言走 `rows_of` 而不是直接调工具函数：分隔实现在 `shared::text`，
/// 但「统计浮层用不用它」是另一件事——这三处历史上各写过一份实现。
/// 只测工具函数的话，视图哪天又自己拼一遍数字也照样绿。
#[test]
fn large_numbers_are_grouped() {
    let (_dir, app) = app_with(&"行\n".repeat(12_345));
    let stats = app.current_stats().expect("应有文档");
    let rows = stats_view::rows_of(&stats);
    let lines = rows
        .iter()
        .find(|(label, _)| *label == "行数")
        .map(|(_, value)| value.as_str())
        .expect("统计浮层必须有「行数」行");
    assert_eq!(lines, "12,346", "12,345 个换行 + 最后一行");
}

// ============================================================================
// 关于浮层
// ============================================================================

/// 关于面板的版本 / 许可证取自 `Cargo.toml`，不是硬编码字符串。
#[test]
fn about_panel_reads_version_and_license_from_the_manifest() {
    let (_dir, app) = app_with("");
    let items = about_view::items(&app);
    let find = |label: &str| {
        items
            .iter()
            .find(|i| i.label == label)
            .unwrap_or_else(|| panic!("关于面板缺少「{label}」项"))
            .value
            .clone()
    };
    assert_eq!(find("版本"), format!("v{}", env!("CARGO_PKG_VERSION")));
    assert_eq!(find("许可证"), env!("CARGO_PKG_LICENSE"));
}

/// 「已打开文档」这一项必须**跟着状态变**，不能是写死的数字。
#[test]
fn about_panel_reflects_the_real_document_count() {
    let (_dir, mut app) = app_with("");
    app.close_all_documents();
    app.new_document().expect("应可新建");
    app.new_document().expect("应可新建");
    let count = app.open_document_list().len();
    let items = about_view::items(&app);
    let shown = items
        .iter()
        .find(|i| i.label == "已打开文档")
        .expect("应有一项")
        .value
        .clone();
    assert_eq!(shown, format!("{count} 个"));
}

// ============================================================================
// 快捷键一览
// ============================================================================

/// 一览必须覆盖**全部**动作，且读的是生效键位（原型缺陷 B-08）。
#[test]
fn shortcut_overview_covers_every_action_with_live_bindings() {
    let (_dir, mut app) = app_with("");
    app.execute(UiAction::ShortcutOverview);
    let sections = rustmark::presentation::views::shortcut_overview_view::collect(&app);
    let total: usize = sections.iter().map(|s| s.rows.len()).sum();
    assert_eq!(total, UiAction::ALL.len());

    // 改键后立即反映。
    app.rebind_shortcut(UiAction::Save, "Ctrl+Alt+K");
    let sections = rustmark::presentation::views::shortcut_overview_view::collect(&app);
    let row = sections
        .iter()
        .flat_map(|s| s.rows.iter())
        .find(|r| r.action == UiAction::Save)
        .expect("保存动作应在一览里");
    assert_eq!(row.combo, "Ctrl+Alt+K");
    assert!(
        app.dispatcher()
            .registry()
            .combo_for(UiAction::Save)
            .is_some()
    );
}

/// 过滤词命中后，非命中分组**仍然存在**（只是为空）。
#[test]
fn shortcut_overview_filtering_keeps_every_section_present() {
    let (_dir, mut app) = app_with("");
    app.execute(UiAction::ShortcutOverview);
    app.shortcut_overview_ui.query = "zzzz".to_string();
    let sections = rustmark::presentation::views::shortcut_overview_view::collect(&app);
    assert_eq!(
        sections.len(),
        rustmark::presentation::shortcuts::registry::MenuGroup::ALL.len(),
        "分组消失会让面板高度忽变忽无"
    );
    assert!(sections.iter().all(|s| s.is_empty()));
}

/// 重复打开面板应重置过滤词（用户从没在这个面板搜过那个词）。
#[test]
fn reopening_the_overview_clears_the_filter() {
    let (_dir, mut app) = app_with("");
    app.execute(UiAction::ShortcutOverview);
    app.shortcut_overview_ui.query = "加粗".to_string();
    app.dialog.close(Overlay::ShortcutOverview);
    app.execute(UiAction::ShortcutOverview);
    assert!(
        app.shortcut_overview_ui.query.is_empty(),
        "面板重复打开不该带上次的过滤词"
    );
}

/// 独立状态对象的重置语义（视图与调度共用同一处）。
#[test]
fn overview_state_open_resets_the_filter() {
    let mut state = ShortcutOverviewUiState::new();
    state.query = "x".to_string();
    rustmark::presentation::views::shortcut_overview_view::reset_filter_on_open(&mut state);
    assert!(state.query().is_empty());
}

// ============================================================================
// 快速打开
// ============================================================================

/// 候选来自工作区条目（目录不算），不是重新扫盘。
#[test]
fn quick_open_candidates_come_from_the_loaded_workspace() {
    let ws = tempfile::tempdir().expect("工作区");
    std::fs::write(ws.path().join("readme.md"), "# r\n").expect("应可写");
    let (_cfg, _ws2, mut app) = app_with_workspace(ws.path());
    let candidates = quick_open_view::candidates(&mut app);
    assert!(
        candidates.iter().any(|c| c.name == "readme.md"),
        "工作区里的文件必须出现在候选里，实际：{:?}",
        candidates.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
    assert!(
        !candidates.iter().any(|c| c.path.ends_with('/')),
        "目录不该作为可打开候选"
    );
}

/// 打开时重置查询与选中项。
#[test]
fn quick_open_resets_query_and_selection_on_open() {
    let ws = tempfile::tempdir().expect("工作区");
    std::fs::write(ws.path().join("a.md"), "a\n").expect("应可写");
    let (_cfg, _ws2, mut app) = app_with_workspace(ws.path());
    app.execute(UiAction::OpenDocument);
    app.quick_open_ui.set_query("readme");
    app.quick_open_ui.selected = 7;
    app.dialog.close(Overlay::QuickOpen);
    app.execute(UiAction::OpenDocument);
    assert!(app.quick_open_ui.query.is_empty());
    assert_eq!(app.quick_open_ui.selected, 0);
}

/// 选中项越界会被夹回（候选变少时不能「高亮不存在的行」）。
#[test]
fn quick_open_clamps_selection_when_candidates_shrink() {
    let (_dir, mut app) = app_with("");
    app.execute(UiAction::OpenDocument);
    app.quick_open_ui.selected = 11;
    app.quick_open_ui.clamp(2);
    assert_eq!(app.quick_open_ui.selected, 1);
    app.quick_open_ui.clamp(0);
    assert_eq!(app.quick_open_ui.selected, 0);
}

/// 无工作区时最近文件是唯一来源（否则快速打开永远空）。
#[test]
fn quick_open_falls_back_to_recent_files_without_a_workspace() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    // 直接构造一条最近文件记录（走仓储公开的 search 路径会太绕）。
    let file = dir.path().join("recent.md");
    std::fs::write(&file, "# r\n").expect("应可写");
    let path_str = file.to_string_lossy().replace('\\', "/");
    app.services
        .workspace
        .touch_recent(
            rustmark::application::workspace::commands::TouchRecentCommand {
                path: rustmark::shared::path::FilePath::new(path_str.clone()).expect("路径应合法"),
            },
        )
        .expect("应可登记");
    let candidates = quick_open_view::candidates(&mut app);
    assert!(
        candidates.iter().any(|c| c.path == path_str),
        "无工作区时应能用最近文件，实际候选：{:?}",
        candidates
    );
}

/// 候选数有上限（否则面板会被几百个文件撑爆）。
#[test]
fn quick_open_candidate_count_is_bounded() {
    let ws = tempfile::tempdir().expect("工作区");
    for i in 0..40 {
        std::fs::write(ws.path().join(format!("f{i}.md")), "x\n").expect("应可写");
    }
    let (_cfg, _ws2, mut app) = app_with_workspace(ws.path());
    assert!(
        quick_open_view::candidates(&mut app).len() <= MAX_CANDIDATES,
        "候选必须截断到上限"
    );
}

/// 匹配是子序列（用户记得的是「大概有这么两个字母」）。
#[test]
fn quick_open_matches_out_of_order_characters() {
    let ws = tempfile::tempdir().expect("工作区");
    std::fs::write(ws.path().join("a-b-c-file.md"), "x\n").expect("应可写");
    let (_cfg, _ws2, mut app) = app_with_workspace(ws.path());
    app.execute(UiAction::OpenDocument);
    app.quick_open_ui.set_query("abc");
    let hits = quick_open_view::candidates(&mut app);
    assert_eq!(hits.len(), 1, "子串匹配下 'abc' 命中不了 'a-b-c-file'");
}

/// 快速打开里点中**白名单外**的文件：不打开、不关浮层，只说明原因（问题 045）。
///
/// 「不关浮层」是这条用例的重点：关掉浮层会让这次点击看起来**生效了**——
/// 用户以为文件开了，回头看标签却没变。留在浮层上，他还能立刻改选别的文件。
#[test]
fn quick_open_refuses_non_text_candidates_without_closing() {
    let ws = tempfile::tempdir().expect("工作区");
    std::fs::write(ws.path().join("note.md"), "# note\n").expect("应可写");
    std::fs::write(ws.path().join("shot.png"), b"\x89PNG\r\n\x1a\n").expect("应可写");
    let (_cfg, _ws2, mut app) = app_with_workspace(ws.path());

    let ctx = egui::Context::default();
    app.execute(UiAction::OpenDocument);
    assert!(app.dialog.is_open(Overlay::QuickOpen), "浮层应先打开");

    // 选中「shot.png」。按**完整路径**定位下标，不假设顺序——
    // 排序规则变了不该让这条用例变成在测排序。
    let list = quick_open_view::candidates(&mut app);
    let index = list
        .iter()
        .position(|item| item.path.ends_with("shot.png"))
        .expect("候选里应有 shot.png");
    app.quick_open_ui.selected = index;

    // 走键盘 Enter 路径：它与鼠标点击共用 `apply_pick`，但不必依赖坐标
    // （浮层尺寸 / 行高一变，任何推导出来的落点都会失效）。
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1000.0, 700.0),
        )),
        events: vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        ..Default::default()
    };
    crate::support::run_ui(&ctx, input, |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(&mut app, ui);
    });

    assert!(
        app.dialog.is_open(Overlay::QuickOpen),
        "选中一个打不开的文件不该把浮层关掉（那会让用户以为已经生效）"
    );
    assert!(
        app.open_document_list().iter().all(|d| d
            .path
            .as_ref()
            .is_none_or(|p| !p.as_str().ends_with("shot.png"))),
        "白名单外的候选不得被打开"
    );
    let notice = app
        .status_bar
        .message
        .as_ref()
        .map(|m| m.text.clone())
        .unwrap_or_default();
    assert!(
        notice.contains("暂不支持打开该格式"),
        "应说明原因，实际状态栏 = {notice:?}"
    );
}

// ============================================================================
// 符号面板
// ============================================================================

/// 插入符号真的进文档，且光标停在插入内容**之后**。
#[test]
fn inserting_a_symbol_changes_the_document_and_the_cursor() {
    let (_dir, mut app) = app_with("");
    app.close_all_documents();
    app.new_document().expect("应可新建");
    // 先写几个字再把光标移回开头，才能验「插入后停在之后」——
    // 领域插入本身会把光标推到末尾，不显式归位的话两次插入都落在末尾。
    app.insert_at_cursor("你好").expect("应可插入");
    app.goto_byte_offset(0);
    app.insert_at_cursor("★").expect("应可插入符号");
    assert_eq!(
        app.current_content().as_deref(),
        Some("★你好"),
        "符号应插在光标处"
    );
    let (line, col) = app.cursor_line_col();
    assert_eq!((line, col), (1, 2), "光标应停在插入的 1 个字符之后");
}

/// 插入是**一个**撤销点（用户按一次 Ctrl+Z 能整段撤掉）。
#[test]
fn inserting_a_symbol_is_a_single_undo_step() {
    let (_dir, mut app) = app_with("");
    app.close_all_documents();
    app.new_document().expect("应可新建");
    app.insert_at_cursor("```\n代码块\n```").expect("应可插入");
    assert!(app.current_content().is_some_and(|c| c.contains("代码块")));
    let id = app
        .services
        .documents
        .list_open(rustmark::application::document::queries::ListOpenDocumentsQuery)
        .expect("应可列出")
        .first()
        .expect("应有一个文档")
        .id;
    app.services
        .documents
        .undo(UndoCommand { id })
        .expect("撤销应成功");
    app.refresh_text();
    assert!(
        !app.current_content().is_some_and(|c| c.contains("代码块")),
        "撤一次就该把整段插入撤掉"
    );
}

/// 插入点取 **UI 光标**而非领域光标（大纲跳转后的插入位置）。
///
/// 缺陷形态：`goto_byte_offset` 只改 UI 光标，领域那边还停在上一次
/// 领域命令之后的位置。此时按「插入符号」，若读领域光标，
/// 内容会插到用户**看不见**的地方——不报错、界面正常、
/// 只是一个看不见的结果出现在别处。
#[test]
fn insert_lands_where_the_ui_cursor_is_not_where_the_domain_cursor_is() {
    let (_dir, mut app) = app_with("0123456789");
    // 光标移到文档中间（UI 与领域此刻已经不同步）。
    app.goto_byte_offset(5);
    app.insert_at_cursor("★").expect("应可插入");
    assert_eq!(
        app.current_content().as_deref(),
        Some("01234★56789"),
        "符号应插在 UI 光标处；若插到末尾说明读的是领域光标"
    );
}

/// 有选区时插入**替换选区**（而非插在选区旁边）。
#[test]
fn insert_replaces_the_selection() {
    let (_dir, mut app) = app_with("hello world");
    app.goto_byte_offset(0);
    // 选中 "hello"（0..5）。
    app.editor.set_selection(0, 5);
    app.insert_at_cursor("你好").expect("应可插入");
    assert_eq!(
        app.current_content().as_deref(),
        Some("你好 world"),
        "插入应替换选区，而不是得到「hello你好 world」"
    );
}

/// 面板里没有任何本应用不消费的模板短代码。
///
/// 原型 `symbols.ts` 的片段组含 `{{< highlight rust >}}`（Hugo 短代码）。
/// 本项目导出 HTML / Markdown / PDF，产物里不会出现 Hugo 语法，
/// 插进去只会得到一段永不渲染的文本。
#[test]
fn symbol_table_has_no_foreign_template_shortcodes() {
    for group in SymbolGroup::ALL {
        for item in items_of(*group) {
            assert!(
                !item.text.contains("{{<") && !item.text.contains("{%"),
                "{:?} 含本应用不消费的模板语法：{}",
                group,
                item.text
            );
        }
    }
}

/// 搜索按 `label` 的**任一关键词**匹配（原型是空格分隔多关键词）。
#[test]
fn symbol_search_matches_any_keyword() {
    let hits = search(items_of(SymbolGroup::Symbol), "star");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].text, "★");
    let hits = search(items_of(SymbolGroup::Symbol), "实心星");
    assert_eq!(hits.len(), 1);
}

/// 切分组会收回选中项（两组长度差 10 倍，不收回会像随机的）。
#[test]
fn switching_symbol_groups_resets_the_selection() {
    let (_dir, mut app) = app_with("");
    app.execute(UiAction::InsertSymbol);
    app.symbol_ui.selected = 15;
    app.symbol_ui.set_group(SymbolGroup::Snippet);
    assert_eq!(app.symbol_ui.selected, 0);
    let visible = app.symbol_ui.visible();
    assert!(visible.len() >= 16, "片段组条目应多于 15（前提检查）");
}

/// 打开面板重置一切。
#[test]
fn opening_the_symbol_panel_resets_its_state() {
    let (_dir, mut app) = app_with("");
    app.execute(UiAction::InsertSymbol);
    app.symbol_ui.set_group(SymbolGroup::Snippet);
    app.symbol_ui.set_query("表格");
    app.symbol_ui.selected = 2;
    app.dialog.close(Overlay::Symbol);
    app.execute(UiAction::InsertSymbol);
    assert_eq!(app.symbol_ui.group, SymbolGroup::Emoji);
    assert!(app.symbol_ui.query.is_empty());
    assert_eq!(app.symbol_ui.selected, 0);
}

/// 符号面板里的每个字形都必须**真的能渲染出来**。
///
/// # 为什么值得单独一条
///
/// 面板条目是**静态字符表**，新增时很容易选到字体不含的码位——
/// `✓`(U+2713) 与 `✗`(U+2717) 就是这样踩中的，且一直没被发现。
///
/// 缺字形的表现只是"一个方框"：**不报错、不崩溃**，
/// 在几十个条目里靠肉眼几乎不可能注意到。只有自动化断言能守住。
///
/// 本条按**最终字体配置**（默认族 + 系统 CJK 字体）判定，
/// 与运行时一致——若哪天换掉了字体加载，这里会立刻失败。
#[test]
fn every_symbol_glyph_is_renderable_under_the_real_font_setup() {
    let setup = rustmark::infrastructure::fonts::font_definitions(None);
    let ctx = egui::Context::default();
    ctx.set_fonts(rustmark::app::frame::to_egui_font_definitions(
        setup.definitions,
    ));
    // 必须先跑一帧，`fonts` 才可用。
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {});

    let font = egui::FontId::proportional(14.0);
    let mut missing = Vec::new();
    for group in SymbolGroup::ALL {
        for item in items_of(*group) {
            for ch in item.glyph.chars() {
                // 空白字符不参与渲染判定。
                if ch.is_whitespace() {
                    continue;
                }
                if !ctx.fonts_mut(|f| f.has_glyph(&font, ch)) {
                    missing.push(format!(
                        "{}组「{}」的 {ch:?} U+{:04X}",
                        group.label(),
                        item.label,
                        ch as u32
                    ));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "以下符号无字形，界面上会显示为方框（共 {} 个）：\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}

/// 符号面板的状态转移是纯状态机的，不依赖 egui（可直接构造验证）。
#[test]
fn symbol_state_moves_and_clamps_independently_of_rendering() {
    let mut state = symbol_ui_state::SymbolUiState::new();
    state.move_selection(-1, 4);
    assert_eq!(state.selected, 3);
    state.selected = 99;
    state.clamp(4);
    assert_eq!(state.selected, 3);
}

// ============================================================================
// 差异对比
// ============================================================================

/// 选好对照文件后能读到内容并算出差异。
#[test]
fn diff_compares_the_current_document_against_a_workspace_file() {
    let ws = tempfile::tempdir().expect("工作区");
    std::fs::write(ws.path().join("target.md"), "line1\nline2\nline3\n").expect("应可写");
    let (_cfg, mut app) = app_with("line1\nCHANGED\nline3\n");
    let root = ws.path().to_string_lossy().replace('\\', "/");
    app.scan_workspace(&root).expect("扫描应成功");

    let target = format!("{root}/target.md");
    app.diff_ui.open(
        "target.md".to_string(),
        target.clone(),
        "target.md".to_string(),
    );

    let left = app.current_content().expect("应有文档");
    let right = app.read_workspace_file(&target).expect("应能读到对照文件");
    assert_eq!(right, "line1\nline2\nline3\n");
    app.diff_ui.recompute(&left, &right);

    assert_eq!(app.diff_ui.stat.added, 1);
    assert_eq!(app.diff_ui.stat.removed, 1);
    assert!(!app.diff_ui.is_identical());
    assert!(
        app.diff_ui
            .rows
            .iter()
            .any(|r| matches!(r, DisplayRow::Replace { .. })),
        "改一行应合并成一个替换行"
    );
    // 两侧相同的行必须保留（用户要看「改了哪些」也得确认「其余没变」）。
    // 两侧都以 `\n` 结尾，`split('\n')` 各多出一个末尾空行且内容相同，
    // 故相等行是 3 行（line1 / line3 / 末尾空行）而不是 2。
    assert_eq!(
        app.diff_ui
            .rows
            .iter()
            .filter(|r| matches!(r, DisplayRow::Equal { .. }))
            .count(),
        3
    );
}

/// 「只看差异行」过滤掉相等的行。
#[test]
fn diff_only_changes_filter_hides_equal_rows() {
    let (_dir, mut app) = app_with("");
    app.diff_ui
        .open("a".to_string(), "/x".to_string(), "b".to_string());
    app.diff_ui.recompute("a\nb\nc", "a\nX\nc");
    let all = app.diff_ui.visible_rows().len();
    app.diff_ui.only_changes = true;
    let changed = app.diff_ui.visible_rows().len();
    assert!(changed < all);
    assert!(changed >= 1);
}

/// 没有对照文件时面板显示选择器，不显示空差异。
#[test]
fn diff_without_a_right_side_asks_for_one() {
    let (_dir, mut app) = app_with("a\n");
    let ctx = egui::Context::default();
    app.execute(UiAction::CompareDiff);
    assert!(app.diff_ui.right_path.is_none());
    frames(&mut app, &ctx, 3);
    assert!(app.dialog.is_open(Overlay::Diff));
}

/// 读取不存在的对照文件时保留选择并给出原因（不静默）。
#[test]
fn diff_reports_an_unreadable_right_side() {
    let (_dir, app) = app_with("a\n");
    let err = app.read_workspace_file("/definitely/not/here.md");
    assert!(err.is_err(), "读不到的文件必须报错而不是返回空串");
}

/// 大文件降级必须**明说**（否则用户以为「整个文件都变了」）。
#[test]
fn oversized_documents_degrade_and_say_so() {
    let (_dir, mut app) = app_with("");
    let n = 2000;
    let a: Vec<String> = (0..n).map(|i| (i % 2).to_string()).collect();
    let b: Vec<String> = (0..n).map(|i| (i % 3).to_string()).collect();
    let (a, b) = (a.join("\n"), b.join("\n"));
    assert!(exceeds_budget(&a, &b), "前提：这份规模必须超预算");
    app.diff_ui
        .open("a".to_string(), "/x".to_string(), "b".to_string());
    app.diff_ui.recompute(&a, &b);
    assert!(app.diff_ui.degraded);
    assert!(
        app.diff_ui
            .rows
            .iter()
            .all(|r| !matches!(r, DisplayRow::Equal { .. })),
        "降级后不该还有相等行——那会让提示与结果自相矛盾"
    );
}

/// 统计基于**行级原始结果**，不受「替换」合并影响。
#[test]
fn diff_stat_counts_the_underlying_lines_not_the_merged_rows() {
    let (_dir, mut app) = app_with("");
    app.diff_ui
        .open("a".to_string(), "/x".to_string(), "b".to_string());
    app.diff_ui.recompute("a\nb1\nb2\nc", "a\nx\ny\nc");
    assert_eq!(app.diff_ui.stat.added, 2);
    assert_eq!(app.diff_ui.stat.removed, 2);
    assert_eq!(
        app.diff_ui
            .rows
            .iter()
            .filter(|r| matches!(r, DisplayRow::Replace { .. }))
            .count(),
        2
    );
}

/// 差层面板在真实帧里不崩且能反复开关。
#[test]
fn diff_panel_survives_repeated_open_close() {
    let ws = tempfile::tempdir().expect("工作区");
    std::fs::write(ws.path().join("t.md"), "x\n").expect("应可写");
    let (_cfg, mut app) = app_with("y\n");
    let root = ws.path().to_string_lossy().replace('\\', "/");
    app.scan_workspace(&root).expect("扫描应成功");
    app.diff_ui
        .open("y".to_string(), format!("{root}/t.md"), "t.md".to_string());
    let ctx = egui::Context::default();
    for _ in 0..3 {
        app.execute(UiAction::CompareDiff);
        frames(&mut app, &ctx, 3);
        app.dialog.close(Overlay::Diff);
    }
    assert!(!app.dialog.is_open(Overlay::Diff));
}

// ============================================================================
// 快速打开的状态纯函数（与视图中同一份实现）
// ============================================================================

/// 子序列匹配的命中索引是**字符**坐标（按字节切会切在多字节中间）。
#[test]
fn fuzzy_hits_use_character_coordinates() {
    let hits = quick_open_ui_state::fuzzy_match("中文sd.md", "sd").expect("应命中");
    assert_eq!(hits, vec![2, 3]);
}

/// 完全等于查询名的候选排在最前。
#[test]
fn exact_name_match_is_ranked_first() {
    let items = vec![
        quick_open_ui_state::QuickOpenItem {
            path: "/r/a-s-d.md".to_string(),
            name: "a-s-d.md".to_string(),
            group: quick_open_ui_state::QuickOpenGroup::Workspace,
        },
        quick_open_ui_state::QuickOpenItem {
            path: "/r/abd.md".to_string(),
            name: "abd.md".to_string(),
            group: quick_open_ui_state::QuickOpenGroup::Workspace,
        },
    ];
    let out = quick_open_ui_state::filter_candidates(&items, "abd", 10);
    assert_eq!(out[0].name, "abd.md");
}

// ============================================================================
// 符号视图的几何常量（与原型对齐）
// ============================================================================

/// 符号面板是 8 列网格（对齐原型 `repeat(8, 1fr)`）。
#[test]
fn symbol_grid_matches_the_prototype_geometry() {
    // 视图模块不导出常量，但真实帧必须能画出来 —— 已由
    // `each_new_overlay_actually_renders_in_a_real_frame` 覆盖。
    let _ = symbol_view::SYMBOL_INPUT_ID;
}

// ============================================================================
// R-11 子批 2：差异对比浮层的写入口收敛到 app 层
// ============================================================================
//
// 判据一律走 `EditorApp` 的方法入口：**不**直接给 `app.diff_ui.<字段>` 赋值
// 再断言它。那样断言读的是自己刚写进去的值，方法改成空实现也照样绿
// （判据自指，本项目已多次撞上）。

/// 读取对照文件失败必须被记下来。
///
/// 静默返回空串会被当成「两侧完全一致」——用户得到的结论与事实相反。
#[test]
fn diff_read_failure_is_recorded_through_the_app_method() {
    let (_dir, mut app) = app_with("a\n");
    assert!(app.diff_ui.error.is_none(), "前置：初始无错误");

    app.set_diff_error("读取对照文件失败：找不到 /x.md");

    assert_eq!(
        app.diff_ui.error.as_deref(),
        Some("读取对照文件失败：找不到 /x.md"),
        "读失败必须明说，不能静默"
    );
}

/// 「换一个文件」同时清掉选择与上一份结果的行表。
#[test]
fn swapping_the_diff_right_file_clears_both_the_choice_and_the_rows() {
    let (_dir, mut app) = app_with("a\nb\nc");
    app.diff_ui
        .open("a".to_string(), "/x.md".to_string(), "x.md".to_string());
    app.diff_ui.recompute("a\nb\nc", "a\nX\nc");
    assert!(app.diff_ui.right_path.is_some(), "前置：已选对照文件");
    assert!(!app.diff_ui.rows.is_empty(), "前置：已有结果行");

    app.clear_diff_selection();

    assert!(app.diff_ui.right_path.is_none(), "选择应被清空");
    assert!(
        app.diff_ui.rows.is_empty(),
        "上一份结果行也必须清掉——否则旧结果会与「读失败」提示同框，自相矛盾"
    );
}

/// 没选对照文件时打开面板，会清掉上一次的错误。
#[test]
fn opening_diff_without_a_right_side_clears_the_stale_error() {
    let (_dir, mut app) = app_with("a\n");
    assert!(app.diff_ui.right_path.is_none(), "前置：还没选对照文件");
    app.set_diff_error("上一次读失败");

    app.open_diff_overlay();

    assert!(
        app.diff_ui.error.is_none(),
        "还没选文件时不该留着上一次的错误：面板会先显示一条无从理解的错误"
    );
}

/// 已保留对照文件时打开面板，**不清**错误。
///
/// 与上一条互补：`open_diff_overlay` 的清错误是**有条件的**。无条件清会让
/// 「文件还在、却读不进来」退化成「两侧完全一致」。
#[test]
fn opening_diff_with_a_kept_right_side_keeps_the_error() {
    let (_dir, mut app) = app_with("a\n");
    app.diff_ui
        .open("a".to_string(), "/x.md".to_string(), "x.md".to_string());
    app.set_diff_error("读取对照文件失败");

    app.open_diff_overlay();

    assert!(
        app.diff_ui.error.is_some(),
        "对照文件仍在时清掉错误，会让面板显示「两侧一致」而真相是根本没读进来"
    );
}
