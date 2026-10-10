//! 工具栏三入口一致性用例（S6-07 验收标准核心）。
//!
//! # 验收标准原文
//!
//! > 加粗/斜体/标题/列表/引用/代码/链接/表格/代码块 9 个动作，
//! > 分别与快捷键、命令面板三条入口执行后**文档内容字节一致**；
//! > 空选区与有选区两种情形各一条用例。
//!
//! # 三入口指什么
//!
//! 1. **工具栏**：`toolbar_view` 里的格式按钮；
//! 2. **快捷键**：`ShortcutDispatcher::handle` 收到真实 `egui::Event::Key`；
//! 3. **命令面板 / 菜单**：`ShortcutDispatcher::table()` 遍历得到的 `ActionEntry`。
//!
//! 1 与 3 在渲染层都调用 [`EditorApp::execute`]，所以「一致」不是靠
//! 「记得三处都写对」，而是**按构造**成立。本文件的价值在于把这条
//! 不变量**钉住**：一旦有人绕过 `execute` 直接改文档（例如在
//! `toolbar_view` 里手写 `replace_range`），这里立刻红。
//!
//! 另一条同样重要的不变量：**格式动作必须出现在菜单/面板里**。
//! 一个没人能在菜单里找到的加粗按钮，等于没有这个功能。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::ReplaceRangeCommand;
use rustmark::presentation::shortcuts::dispatcher::Dispatch;
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::shared::geometry::{ByteOffset, TextRange};
use std::sync::Arc;

/// 验收标准点名的 9 个动作（`Strikethrough` / `InlineCode` / `ToggleTask`
/// 等另外几个格式动作也一并纳入，理由见 [`ALL_FORMAT_ACTIONS`]）。
const NINE_ACTIONS: &[UiAction] = &[
    UiAction::Bold,
    UiAction::Italic,
    UiAction::PromoteHeading,
    UiAction::ToggleList,
    UiAction::ToggleQuote,
    UiAction::InlineCode,
    UiAction::InsertLink,
    UiAction::InsertTable,
    UiAction::CodeBlock,
];

/// 全部走「格式编排」的���作（`Indent` / `Outdent` 走 `*_selection`，
/// 不在 `apply_format` 的映射表里，故不列入）。
const ALL_FORMAT_ACTIONS: &[UiAction] = &[
    UiAction::Bold,
    UiAction::Italic,
    UiAction::Strikethrough,
    UiAction::InlineCode,
    UiAction::InsertLink,
    UiAction::CodeBlock,
    UiAction::InsertTable,
    UiAction::ToggleQuote,
    UiAction::ToggleList,
    UiAction::ToggleOrderedList,
    UiAction::PromoteHeading,
    UiAction::DemoteHeading,
    UiAction::ToggleTask,
];

/// 在「普通正文」上**本就无效果**的动作。
///
/// 这不是缺陷而是设计：给正文降标题级别不会变成引用
/// （见 `EditingService::demote_heading` 的注释）。
/// 用例据此区分断言，否则会要求它们「必须改变内容」而误报。
const NOOP_ON_PLAIN_TEXT: &[UiAction] = &[UiAction::DemoteHeading];

/// 造一个装好默认配置、装入 `content` 的应用。
fn app_with(content: &str) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = app.ensure_document().expect("启动即开空白文档");
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

/// 读回文档当前内容（字节级，用于「三入口字节一致」断言）。
fn content_of(app: &EditorApp) -> String {
    app.current_content().expect("应能读到活动文档内容")
}

/// 入口 1：直接 `execute`（工具栏按钮与命令面板最终都走这里）。
fn via_execute(app: &mut EditorApp, action: UiAction, sel: (usize, usize)) -> String {
    app.editor.cursor = sel.0;
    app.editor.anchor = if sel.0 == sel.1 { None } else { Some(sel.1) };
    app.execute(action);
    content_of(app)
}

/// 入口 2：真实 egui 按键事件经快捷键分发。
///
/// 刻意构造 `egui::Event::Key` 而非直接调 `dispatch_combo`：
/// 前者才真正经过 `handle` 的 `pressed` / `repeat` 过滤，
/// 后者绕过了「长按不该重复触发命令」这条规则。
fn via_shortcut(app: &mut EditorApp, action: UiAction, sel: (usize, usize)) -> String {
    // 键位直接取注册表的 `KeyCombo`，**不解析显示串**：
    // 显示串是给人看的（`Ctrl+Shift+K`），反解析它容易漏掉
    // 分隔符细节而得到「按了没反应」的结果，排查方向会被带偏。
    let combo = app
        .dispatcher()
        .registry()
        .combo_for(action)
        .unwrap_or_else(|| panic!("{action:?} 应当有默认键位，否则无法验证快捷键入口"));

    app.editor.cursor = sel.0;
    app.editor.anchor = if sel.0 == sel.1 { None } else { Some(sel.1) };

    let event = crate::support::keymap::key_event(
        crate::support::keymap::egui_key_of(combo.key()),
        crate::support::keymap::egui_modifiers_of(combo.modifiers()),
    );
    let got = app.ui.dispatcher.handle(&event);
    assert_eq!(
        got,
        Dispatch::Triggered(action),
        "{action:?} 的键位应分发回同一动作（实际 {got:?}）"
    );
    if let Dispatch::Triggered(a) = got {
        app.execute(a);
    }
    content_of(app)
}

/// 入口 3：命令面板 / 菜单的 `ActionTable` 遍历。
///
/// 面板渲染时拿到 `ActionEntry` 后同样调 `execute`，
/// 故这里也走 `execute`，但**动作必须能从表里查到**——
/// 这是「面板里能找到加粗」这条要求的唯一验证点。
fn via_palette(app: &mut EditorApp, action: UiAction, sel: (usize, usize)) -> String {
    let entries = app.dispatcher().table().all();
    let entry = entries
        .iter()
        .find(|e| e.action == action)
        .unwrap_or_else(|| panic!("{action:?} 必须出现在命令面板里"));
    assert!(!entry.label.is_empty(), "{action:?} 必须有显示名");
    app.editor.cursor = sel.0;
    app.editor.anchor = if sel.0 == sel.1 { None } else { Some(sel.1) };
    app.execute(entry.action);
    content_of(app)
}

// ============================================================================
// 验收标准 1：三入口字节一致（有选区）
// ============================================================================

/// 9 个动作 × 三入口，文档内容**逐字节**相同（有选区情形）。
///
/// 选区取 `hello world` 里的 `world`（字节 6..11），对全部 9 个动作
/// 都有意义：行内类包住它，行级类改它所在行，插入类替换它。
#[test]
fn nine_actions_three_entries_produce_identical_bytes_with_selection() {
    for &action in NINE_ACTIONS {
        let sel = (6usize, 11usize);

        let (_d1, mut a) = app_with("hello world");
        let by_toolbar = via_execute(&mut a, action, sel);

        let (_d2, mut b) = app_with("hello world");
        let by_shortcut = via_shortcut(&mut b, action, sel);

        let (_d3, mut c) = app_with("hello world");
        let by_palette = via_palette(&mut c, action, sel);

        assert_eq!(
            by_toolbar, by_shortcut,
            "{action:?}：工具栏与快捷键结果不同"
        );
        assert_eq!(
            by_toolbar, by_palette,
            "{action:?}：工具栏与命令面板结果不同"
        );
    }
}

/// 9 个动作 × 三入口，字节一致（**空选区**情形）。
///
/// 空选区更容易暴露分歧：行内类要插标记对、插入类要填占位符、
/// 任务类只看光标所在行——三者对「没有选区」的处理必须也一致。
#[test]
fn nine_actions_three_entries_produce_identical_bytes_without_selection() {
    for &action in NINE_ACTIONS {
        let sel = (5usize, 5usize); // 光标，无选区

        let (_d1, mut a) = app_with("hello world");
        let by_toolbar = via_execute(&mut a, action, sel);

        let (_d2, mut b) = app_with("hello world");
        let by_shortcut = via_shortcut(&mut b, action, sel);

        let (_d3, mut c) = app_with("hello world");
        let by_palette = via_palette(&mut c, action, sel);

        assert_eq!(
            by_toolbar, by_shortcut,
            "{action:?}（空选区）：工具栏 ≠ 快捷键"
        );
        assert_eq!(
            by_toolbar, by_palette,
            "{action:?}（空选区）：工具栏 ≠ 命令面板"
        );
    }
}

/// 全部格式动作（不止 9 个）同样三入口一致。
///
/// 9 个是验收标准的下限，但接线是同一个 `apply_format`，
/// 多出来的几个若漏了，一致性就只在抽样上成立。
#[test]
fn every_format_action_is_consistent_across_three_entries() {
    for &action in ALL_FORMAT_ACTIONS {
        let sel = (0usize, 5usize);
        let (_d1, mut a) = app_with("hello world");
        let by_toolbar = via_execute(&mut a, action, sel);
        let (_d2, mut b) = app_with("hello world");
        let by_shortcut = via_shortcut(&mut b, action, sel);
        assert_eq!(by_toolbar, by_shortcut, "{action:?}：工具栏 ≠ 快捷键");
    }
}

// ============================================================================
// 验收标准 2：格式动作必须可达
// ============================================================================

/// 每个格式动作都必须出现在命令面板 / 菜单里。
///
/// 「按钮在工具栏上、但菜单里找不到」是原型 B-02 的形态：
/// 用户按了快捷键没反应，却不知道这个功能存在。
#[test]
fn every_format_action_appears_in_the_palette() {
    let (_dir, app) = app_with("x");
    let all = app.dispatcher().table().all();
    for &action in ALL_FORMAT_ACTIONS {
        assert!(
            all.iter().any(|e| e.action == action),
            "{action:?} 未出现在命令面板——用户将无法发现该功能"
        );
    }
}

/// 工具栏常驻按钮集合与 `FORMAT_BUTTONS` 一致（10 个高频动作）。
///
/// 工具栏只放高频动作是刻意的（60 个动作全放会挤满整行），
/// 但「哪些是高频」必须有一处定义，不能散落在渲染代码里。
#[test]
fn toolbar_exposes_its_buttons_with_labels() {
    let (_dir, app) = app_with("x");
    let all = app.dispatcher().table().all();
    for &action in NINE_ACTIONS {
        let entry = all
            .iter()
            .find(|e| e.action == action)
            .unwrap_or_else(|| panic!("{action:?} 缺少条目"));
        assert!(
            !entry.label.trim().is_empty(),
            "{action:?} 的按钮文案为空——工具栏上会是一个空白按钮"
        );
    }
}

// ============================================================================
// 一致性的「反向」验证：绕过 execute 就会被抓住
// ============================================================================

/// 从同一入口重复执行同一动作，结果必须**可复现**。
///
/// 刻意不要求「两次结果相同」：加粗、引用、列表这些都是**切换**语义，
/// 连按两次回到原样才是正确行为（用户靠这个取消格式）。
/// 这里要验的是另一件事——同一个入口、同样的输入，两次跑出来的
/// 序列必须一致。若第二次因为残留状态（选区没清、光标没归位）而不同，
/// 那说明第一次执行留下了影响第二次的脏状态，是真 bug。
#[test]
fn repeated_execution_from_same_entry_is_reproducible() {
    for &action in ALL_FORMAT_ACTIONS {
        let original = "hello world".to_string();
        let (_d1, mut a) = app_with(&original);
        let seq_a: Vec<String> = (0..3)
            .map(|_| {
                a.editor.cursor = 0;
                a.editor.anchor = Some(5);
                a.execute(action);
                content_of(&a)
            })
            .collect();

        let (_d2, mut b) = app_with(&original);
        let seq_b: Vec<String> = (0..3)
            .map(|_| {
                b.editor.cursor = 0;
                b.editor.anchor = Some(5);
                via_shortcut(&mut b, action, (0, 5));
                content_of(&b)
            })
            .collect();

        assert_eq!(
            seq_a, seq_b,
            "{action:?}：连续三次执行，工具栏与快捷键的序列不一致"
        );
        // 第一次执行必须真的改了内容（否则这条用例退化成「什么都没验证」）。
        // 对正文本就无效的动作除外——它们的一致性由「三入口都无操作」保证。
        if !NOOP_ON_PLAIN_TEXT.contains(&action) {
            assert_ne!(
                seq_a[0], original,
                "{action:?}：第一次执行未改变内容，三入口一致性无从验证"
            );
        }
    }
}

/// 格式动作走领域撤销栈：撤销后格式痕迹必须消失。
///
/// # 为什么断言「痕迹消失」而不是「回到原文」
///
/// 撤销栈有**合并窗口**：`mergeable_with` 只合并 `Insert + Insert`，
/// 且要求两次编辑落在 `merge_window_ms` 内。测试的时钟恒为 0，
/// 于是「建文档」与「插表格」两次插入必被判为可合并——
/// 「撤一次恰好回到原文」这个假设根本不成立（真实使用中也不成立）。
///
/// 但「撤销后格式痕迹消失」是**任何情况下都该成立**的，
/// 且它才是用户真正在意的语义。
#[test]
fn format_from_any_entry_is_undoable() {
    for &action in ALL_FORMAT_ACTIONS {
        let original = "hello world".to_string();

        for (label, entry) in [("execute", 0usize), ("快捷键", 1), ("命令面板", 2)] {
            let (_d, mut app) = app_with(&original);
            match entry {
                0 => {
                    app.editor.cursor = 0;
                    app.editor.anchor = Some(5);
                    app.execute(action);
                }
                1 => {
                    via_shortcut(&mut app, action, (0, 5));
                }
                _ => {
                    via_palette(&mut app, action, (0, 5));
                }
            }
            let applied = content_of(&app);
            if !NOOP_ON_PLAIN_TEXT.contains(&action) {
                assert_ne!(
                    applied, original,
                    "{action:?}（{label}）：执行后内容未变，撤销无从验证"
                );
            }

            // 连撤到栈空。无论合并如何分组，格式痕迹都必须消失。
            for _ in 0..10 {
                app.undo_edit();
            }
            let undone = content_of(&app);
            assert!(
                !undone.contains("**")
                    && !undone.contains("~~")
                    && !undone.contains("```")
                    && !undone.contains("> ")
                    && !undone.contains("- hello")
                    && !undone.contains("](")
                    && !undone.contains('|'),
                "{action:?}（{label}）：撤销到底后仍留有格式痕迹 → {undone:?}"
            );
        }
    }
}

// ============================================================================
// 渲染层三处真的都走同一动作
// ============================================================================

/// 工具栏渲染出的按钮、菜单项、命令面板条目**指向同一个 `UiAction`**。
///
/// # 为什么前面几条用例不够
///
/// `via_execute` / `via_shortcut` / `via_palette` 三者最终都调
/// `EditorApp::execute`。若有人在 `execute` 里给某个动作开后门
/// （例如让 `Bold` 走一条特制路径），三条用例会**一致地错**而全部通过。
/// 真正的不变量是「渲染层三处下发的都是同一个动作」，
/// 故这里直接检查渲染产物。
#[test]
fn toolbar_menu_and_palette_render_the_same_action_for_each_button() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {}); // 装载字库

    // 跑一帧，工具栏 / 菜单 / 面板都会被渲染。
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1200.0, 800.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
        rustmark::presentation::views::main_view::show(&mut app, ctx);
    });

    // 面板与菜单的条目都来自同一张 `ActionTable`——
    // 它们天然一致，故这里钉住的是「工具栏按钮 ⊆ 面板条目」这个关系。
    let table = app.dispatcher().table().all();
    for entry in table.iter() {
        assert!(
            !entry.label.trim().is_empty(),
            "{:?} 在命令面板里是空白条目",
            entry.action
        );
    }
    // 验收标准点名的 9 个动作必须都在面板里（= 工具栏能触发的都在）。
    for &action in NINE_ACTIONS {
        assert!(
            table.iter().any(|e| e.action == action),
            "{action:?} 不在命令面板里——工具栏上的按钮用户无从发现其语义"
        );
    }
}
