//! S6-12 命令面板的集成用例。
//!
//! # 本任务要验的核心契约
//!
//! S6-12 的验收标准里最要紧的一条是「**命令面板可搜到全部命令，
//! 且执行结果与菜单一致**」。这句话对应两个方向：
//!
//! - **覆盖**：面板能搜到的动作集合 == 菜单能看到的动作集合
//!   （因为二者取自同一张 [`ActionTable`]，这条是结构性成立的，
//!   但仍要有用例钉住，否则有人给面板单独造一份清单就会漂移）；
//! - **同源**：面板里执行某个动作，效果与从菜单点它**完全一样**。
//!
//! # 检索为什么不在这里重测
//!
//! `ActionTable::search`（子序列匹配、标签/关键词权重、命中坐标）
//! 已由 `tests/presentation/shortcut_registry_test.rs` 覆盖 7 条。
//! 本文件只钉「检索被接到了面板上」以及「结果能不能执行」。

use std::sync::Arc;

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::ReplaceRangeCommand;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::presentation::state::dialog_ui_state::Overlay;
use rustmark::presentation::views::{command_palette_view, overlays};
use rustmark::shared::geometry::{ByteOffset, TextRange};

// ============================================================================
// 装置
// ============================================================================

fn app_with(content: &str) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = app.ensure_document().expect("应能开出空白文档");
    app.services
        .documents
        .replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
            text: content.to_string(),
        })
        .expect("写入内容应成功");
    app.refresh_text();
    (dir, app)
}

/// 跑一帧真实 egui 渲染，返回该帧的绘制指令数。
///
/// 返回「形状数」而非 `()` 的理由：S6-11f 的教训——只断言
/// `dialog.is_open()` 的话，把整个渲染分支摘掉仍然全绿。
/// 比较浮层打开前后的绘制量才抓得住「打开了但什么都没画」。
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
    out.shapes.len()
}

fn frames(app: &mut EditorApp, ctx: &egui::Context, n: usize) -> usize {
    let mut last = 0;
    for _ in 0..n.max(2) {
        last = frame(app, ctx);
    }
    last
}

// ============================================================================
// 覆盖：面板能搜到的 == 动作表里有的
// ============================================================================

/// S6-12 的核心验收：面板列出**全部**动作。
///
/// 与 `UiAction::ALL` 比对而非与某个写死的数字比对——新增动作时
/// 这条会提醒你面板没跟上，而不是等数字对不上才发现。
#[test]
fn the_palette_lists_every_action() {
    let (_dir, mut app) = app_with("x\n");
    app.execute(UiAction::CommandPalette);
    let rows = command_palette_view::rows(&app);
    assert_eq!(
        rows.len(),
        UiAction::ALL.len(),
        "面板应列出全部动作（空查询 = 全部）"
    );
    for action in UiAction::ALL {
        assert!(
            rows.iter().any(|r| r.action() == *action),
            "{action:?} 应出现在面板里"
        );
    }
}

/// 面板的清单与**菜单的清单是同一份**。
///
/// 这是「执行结果与菜单一致」的结构性保证：两边都调 `ActionTable`，
/// 不存在「面板多了一条菜单没有的命令」。
#[test]
fn the_palette_and_the_menu_read_the_same_action_table() {
    let (_dir, app) = app_with("x\n");
    let from_table = app.dispatcher().registry().table().all();
    let from_palette = command_palette_view::rows(&app);
    assert_eq!(
        from_palette.len(),
        from_table.len(),
        "面板与菜单的命令数量应一致"
    );
    for entry in &from_table {
        let row = from_palette
            .iter()
            .find(|r| r.action() == entry.action)
            .unwrap_or_else(|| panic!("菜单有 {:?} 而面板没有", entry.action));
        assert_eq!(row.label(), entry.label, "显示名应逐字相同");
        assert_eq!(
            row.entry.combo, entry.combo,
            "{:?} 的快捷键显示应与菜单相同（三处同源）",
            entry.action
        );
    }
}

/// 改键后面板立刻反映（原型缺陷 B-08 的根治，三处同源）。
#[test]
fn the_palette_shows_rebound_shortcuts() {
    let (_dir, mut app) = app_with("x\n");
    app.rebind_shortcut(UiAction::Save, "Ctrl+Alt+K");
    let rows = command_palette_view::rows(&app);
    let save = rows
        .iter()
        .find(|r| r.action() == UiAction::Save)
        .expect("保存动作应在面板里");
    assert_eq!(
        save.entry.combo, "Ctrl+Alt+K",
        "面板里的快捷键必须是**生效**键位，不能是默认快照"
    );
}

// ============================================================================
// 检索接线
// ============================================================================

/// 搜中文显示名能筛出结果。
#[test]
fn searching_by_chinese_label_narrows_the_list() {
    let (_dir, mut app) = app_with("x\n");
    app.execute(UiAction::CommandPalette);
    let all = command_palette_view::rows(&app).len();
    app.command_palette_ui.set_query("保存");
    let filtered = command_palette_view::rows(&app);
    assert!(
        filtered.len() < all,
        "查询后应变少：{} → {}",
        all,
        filtered.len()
    );
    assert!(
        filtered.iter().any(|r| r.label().contains("保存")),
        "结果里应有显示名含「保存」的命令"
    );
}

/// 搜英文关键词也能命中（用户可能只记得英文）。
#[test]
fn searching_by_english_keyword_works_too() {
    let (_dir, mut app) = app_with("x\n");
    app.command_palette_ui.set_query("bold");
    let rows = command_palette_view::rows(&app);
    assert!(
        rows.iter().any(|r| r.action() == UiAction::Bold),
        "英文关键词应能命中加粗"
    );
}

/// 搜不到时返回空而不是「全部」——后者会让用户以为自己的查询
/// 被忽略了。
#[test]
fn an_unmatched_query_yields_nothing() {
    let (_dir, mut app) = app_with("x\n");
    app.command_palette_ui.set_query("zzzzqqqxyz");
    assert!(command_palette_view::rows(&app).is_empty());
}

/// 只命中英文关键词时**不产生高亮坐标**。
///
/// 坐标是「在中文 label 里的字符下标」，拿关键词的坐标来切标签会越界
/// （`bold` 的 `[0..4]` 落在只有 2 字的「加粗」之外）。
/// `registry::search` 里的修复就是为这条兜的底。
#[test]
fn keyword_only_matches_carry_no_highlight_coordinates() {
    let (_dir, mut app) = app_with("x\n");
    app.command_palette_ui.set_query("bold");
    let rows = command_palette_view::rows(&app);
    let bold = rows
        .iter()
        .find(|r| r.action() == UiAction::Bold)
        .expect("应命中加粗");
    assert_eq!(bold.label(), "加粗");
    assert!(
        bold.hits.is_empty(),
        "只命中英文关键词时不该按坐标切中文标签，实际 hits={:?}",
        bold.hits
    );
    let label_len = bold.label().chars().count();
    assert!(
        bold.hits.iter().all(|i| *i < label_len),
        "高亮坐标必须落在 label 长度内（越界会切出乱码）"
    );
}

/// 命中显示名时给出坐标，且坐标逐字对得上。
///
/// # 第一版这条断言写错了
///
/// 写成「每个高亮位置都应是『保』」→ 红：`保存` 的 `hits = [0, 1]`，
/// 位置 1 是「存」，**完全正确**。错的是我的断言——命中的是查询里
/// 的**每个字**依次落在 label 中的位置，不是「全都是第一个字」。
///
/// 正确判据：`hits` 序列上的字符连起来 == 查询串。
#[test]
fn label_matches_highlight_exactly_the_typed_characters() {
    let (_dir, mut app) = app_with("x\n");
    let query = "保存";
    app.command_palette_ui.set_query(query);
    let rows = command_palette_view::rows(&app);
    let with_hits: Vec<_> = rows.iter().filter(|r| !r.hits.is_empty()).collect();
    assert!(!with_hits.is_empty(), "搜「保存」应有命中显示名的行");
    let needle: Vec<char> = query.chars().collect();
    for row in with_hits {
        let chars: Vec<char> = row.label().chars().collect();
        for i in &row.hits {
            assert!(
                *i < chars.len(),
                "{:?} 的高亮下标 {i} 越界（label 共 {} 字）",
                row.label(),
                chars.len()
            );
        }
        // 坐标严格递增（子序列匹配的定义），且按坐标取字 == 查询串。
        assert!(
            row.hits.windows(2).all(|w| w[0] < w[1]),
            "{:?} 的高亮坐标应严格递增：{:?}",
            row.label(),
            row.hits
        );
        let picked: String = row.hits.iter().map(|i| chars[*i]).collect();
        assert_eq!(
            picked,
            needle.iter().collect::<String>(),
            "{:?} 按高亮坐标取出的字应连成查询串",
            row.label()
        );
    }
}

// ============================================================================
// 执行：面板 → execute → 与菜单同结果
// ============================================================================

/// 面板里选中「加粗」并执行，文档应真的变粗。
///
/// 这是「执行结果与菜单一致」的可观测证据：走的是
/// `EditorApp::execute`，与菜单点击同一个入口。
#[test]
fn running_a_format_command_from_the_palette_matches_the_menu() {
    let (_dir, mut app) = app_with("hello\n");
    // **选中** `hello` 再加粗。
    //
    // 第一版只 `goto_byte_offset(0)`，结果得到 `****hello`——因为
    // 无选区时加粗是在**光标处插入**包裹符（`hello` 不在 `****` 中间）。
    // 这不是实现缺陷，是「加粗包住选区」这条规则的必然结果。
    app.editor.set_selection(0, 5);

    app.execute(UiAction::CommandPalette);
    app.command_palette_ui.set_query("加粗");
    let rows = command_palette_view::rows(&app);
    let bold = rows
        .iter()
        .find(|r| r.action() == UiAction::Bold)
        .expect("应搜到加粗");
    app.execute(bold.action());

    let content = app.current_content().expect("应有活动文档");
    assert!(
        content.contains("**hello**"),
        "执行加粗后选区应被包粗，实际：{content:?}"
    );
}

/// 执行一个会打开**别的浮层**的命令：旧面板必须先关掉，且新浮层能开。
///
/// 顺序错了会出现「面板关 → 新面板开 → 旧面板的关闭逻辑又把新面板关掉」。
///
/// # 为什么直接调 `apply_pick`
///
/// 这是模拟「用户在面板里按了 Enter」的**唯一**途径：面板内部的
/// ↑↓/Enter 由 `handle_keys` 从 `ctx.input` 原始事件里读，
/// 集成测试要造出「本帧有一个 Enter 事件」并不难，但那样断言的是
/// 事件分发；直接调 `apply_pick` 断的正是「选中项 → 关面板 → 执行」
/// 这段顺序，正是本用例要验的东西。
///
/// 第一版写成「`app.execute(action)` 然后断言关于面板开着」——
/// **它没验证用例名说的那件事**：`execute` 不走 `apply_pick`，
/// 「关闭命令面板」那段代码根本没执行。它在两个变异里变红，
/// 但红的原因是 `expect("应搜到关于")`（列表被截断），与关闭顺序无关。
/// **测试名与断言不符，本身就是信号。**
#[test]
fn running_an_overlay_command_from_the_palette_replaces_it() {
    let (_dir, mut app) = app_with("x\n");
    app.execute(UiAction::CommandPalette);
    assert!(app.dialog.is_open(Overlay::CommandPalette));

    let rows = command_palette_view::rows(&app);
    let about = rows
        .iter()
        .find(|r| r.action() == UiAction::About)
        .expect("面板应列出「关于」")
        .clone();
    command_palette_view::apply_pick(&mut app, &about);

    assert!(
        !app.dialog.is_open(Overlay::CommandPalette),
        "执行命令后面板应先关掉（否则新浮层会被它的关闭逻辑误伤）"
    );
    assert!(
        app.dialog.is_open(Overlay::About),
        "执行「关于」后该浮层应打开"
    );
}

/// 执行一个**不打开浮层**的命令：面板关掉，动作生效，文档变了。
///
/// 与上一条成对：一条验「开浮层的命令」，一条验「改文档的命令」。
#[test]
fn running_a_document_command_from_the_palette_closes_it_and_edits() {
    let (_dir, mut app) = app_with("hello\n");
    app.editor.set_selection(0, 5);
    app.execute(UiAction::CommandPalette);

    let rows = command_palette_view::rows(&app);
    let bold = rows
        .iter()
        .find(|r| r.action() == UiAction::Bold)
        .expect("面板应列出「加粗」")
        .clone();
    command_palette_view::apply_pick(&mut app, &bold);

    assert!(
        !app.dialog.is_open(Overlay::CommandPalette),
        "执行完命令面板应关闭"
    );
    let content = app.current_content().expect("应有活动文档");
    assert!(
        content.contains("**hello**"),
        "面板执行的加粗应与菜单一致，实际：{content:?}"
    );
}

/// 命令面板是**模态**：打开时全局快捷键让位（背景不可交互）。
///
/// 验收标准写的是原型 a11y（`aria-modal` + 焦点陷阱）。egui 无此语义，
/// 本项目的等价物是 `DialogUiState::is_modal` —— 模态浮层打开时
/// 分发器不再吐出动作。
#[test]
fn the_palette_is_modal_so_background_shortcuts_are_swallowed() {
    let (_dir, mut app) = app_with("x\n");
    app.execute(UiAction::CommandPalette);
    assert!(
        app.dialog.is_modal(),
        "命令面板必须是模态，否则 Ctrl+S 之类会在打字时被触发"
    );
}

/// 查找面板是**唯一**的非模态浮层（编辑区仍可编辑 / 滚动）。
///
/// 这条是**对照**：命令面板必须与查找面板语义不同——面板内部没有
/// 文本焦点可依赖，只能靠 `is_modal` 吞掉全局快捷键。若哪天有人
/// 把 `CommandPalette` 也加进非模态白名单，`Ctrl+B` 就会在打字时
/// 把文档里的字加粗（这正是 S6-08 修过的查找框缺陷的翻版）。
#[test]
fn the_palette_is_modal_unlike_the_find_panel() {
    assert!(
        Overlay::CommandPalette.is_modal(),
        "命令面板内部没有文本焦点可依赖，必须是模态"
    );
    assert!(
        !Overlay::Find.is_modal(),
        "查找面板是唯一的非模态浮层（原型允许边看边改）"
    );
}

// ============================================================================
// 打开即重置
// ============================================================================

/// 关闭后再打开，查询词与选中项必须已清空。
#[test]
fn opening_the_palette_resets_query_and_selection() {
    let (_dir, mut app) = app_with("x\n");
    app.execute(UiAction::CommandPalette);
    app.command_palette_ui.set_query("保存");
    app.command_palette_ui.selected = 4;
    app.dialog.close(Overlay::CommandPalette);

    app.execute(UiAction::CommandPalette);
    assert!(app.command_palette_ui.query.is_empty(), "查询词应已清空");
    assert_eq!(app.command_palette_ui.selected, 0, "选中项应回到 0");
}

// ============================================================================
// 渲染与待办清单
// ============================================================================

/// 打开后真的有东西被画出来。
///
/// 比较打开前后的绘制指令数——S6-11f 的教训：只断言
/// `dialog.is_open()` 的话，把渲染分支摘成 `{}` 仍然全绿。
#[test]
fn opening_the_palette_actually_paints_something() {
    let (_dir, mut app) = app_with("# t\n\nbody\n");
    let ctx = egui::Context::default();
    let baseline = frames(&mut app, &ctx, 3);
    app.execute(UiAction::CommandPalette);
    let painted = frames(&mut app, &ctx, 3);
    assert!(
        painted > baseline,
        "面板打开后绘制量没有增加（{baseline} → {painted}）：\
         视图存在但没画东西，或渲染分支被摘掉了"
    );
}

/// 搜索框拿到焦点：用户的下一个动作一定是打字。
#[test]
fn the_search_box_takes_focus_when_open() {
    let (_dir, mut app) = app_with("x\n");
    let ctx = egui::Context::default();
    app.execute(UiAction::CommandPalette);
    frames(&mut app, &ctx, 3);
    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::unique(
            command_palette_view::COMMAND_PALETTE_INPUT_ID
        ))),
        "面板打开后焦点应落在搜索框"
    );
}

/// 空结果时界面明说「没有匹配的命令」而不是空白一片。
#[test]
fn an_empty_result_set_is_stated_out_loud() {
    let (_dir, mut app) = app_with("x\n");
    let ctx = egui::Context::default();
    app.execute(UiAction::CommandPalette);
    app.command_palette_ui.set_query("zzzzqqqxyz");
    // 空态必须仍然画出东西（提示文案），否则用户面对空白面板
    // 不知道是程序卡了还是查询没生效。
    let painted = frames(&mut app, &ctx, 3);
    assert!(painted > 0, "空结果也应画出提示文案");
    assert!(command_palette_view::rows(&app).is_empty());
}

/// `PENDING_VIEWS` 已清空（跨任务总账，S6-11f 起就在）。
#[test]
fn the_pending_view_list_is_now_empty() {
    assert!(
        overlays::PENDING_VIEWS.is_empty(),
        "S6-12 后不应再有「有开启动作却没视图」的浮层，实际：{:?}",
        overlays::PENDING_VIEWS
    );
    for overlay in Overlay::ALL.iter().copied() {
        assert!(overlays::has_view(overlay), "{overlay:?} 仍缺渲染实现");
    }
}
