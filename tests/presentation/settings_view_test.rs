//! 设置对话框功能用例（S6-11 验收标准的前两条）。
//!
//! # 验收标准原文
//!
//! > 逐项对照原型：设置「取消」回滚全部未确认修改、
//! > 「恢复默认」只重置当前分区而非全部 13 个字段（原型缺陷 3.6 根治）；
//! > 导出四类各产出一次真实文件并人工打开确认；
//! > 文件树右键菜单可用；大纲点击跳转与层级调整可用；
//! > 全局搜索结果分组与跳转可用；
//! > 状态栏各项数值与原型 `StatsDialog` 口径一致。
//!
//! 本文件覆盖设置部分，另有一条**真实帧**用例确认面板确实被渲染
//! （钉住「动作能打开浮层但没有视图」这类静默失效）。
//!
//! # 为什么回滚断言要同时查三处
//!
//! 主题 / 编辑器 / 快捷键是三条**独立**的写入路径。历史上曾出现「回滚了主题
//! 但忘了快捷键」——用户看到的是「按了取消，字号回来了但 Ctrl+B 不加粗了」。
//! 只断言其中一处会漏掉这类部分回滚。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::preferences::commands::{ResetSectionCommand, SettingsSection};
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::presentation::state::dialog_ui_state::Overlay;
use rustmark::presentation::state::settings_ui_state::SettingsTab;
use rustmark::presentation::views::settings_view::FONT_INPUT_ID;
use rustmark::presentation::views::{main_view, overlays};
use std::sync::Arc;

/// 造一个装好默认配置的应用（不预置内容，设置用例不看文档）。
fn app() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

/// 当前生效的某动作键位（`None` = 未绑定）。
fn combo_of(app: &EditorApp, action: UiAction) -> Option<String> {
    app.dispatcher()
        .registry()
        .combo_for(action)
        .map(|combo| combo.display())
}

/// 「取消」把三类修改**全部**回滚。
///
/// 验收标准原文是「取消回滚全部未确认修改」——三类路径必须都覆盖。
#[test]
fn cancel_rolls_back_theme_editor_and_shortcuts() {
    let (_dir, mut app) = app();

    // 先改一个非默认值，否则「回滚到默认」与「回滚到初始」无法区分。
    app.apply_theme_field(|t| t.font_size = 14.0);
    app.apply_editor_field(|e| e.tab_size = 2);
    app.rebind_shortcut(UiAction::Bold, "Ctrl+Alt+B");
    app.execute(UiAction::OpenSettings);
    let before = app.settings();
    let bold_before = combo_of(&app, UiAction::Bold);

    app.apply_theme_field(|t| t.font_size = 20.0);
    app.apply_theme_field(|t| t.kind = "light".to_string());
    app.apply_editor_field(|e| e.tab_size = 8);
    app.apply_editor_field(|e| e.word_wrap = !before.editor.word_wrap);
    app.rebind_shortcut(UiAction::Bold, "Ctrl+Shift+Alt+B");

    let during = app.settings();
    assert_eq!(during.theme.font_size, 20.0, "实时预览应立即生效");
    assert_eq!(during.editor.tab_size, 8);
    assert_ne!(
        combo_of(&app, UiAction::Bold),
        bold_before,
        "改键应立即反映到注册表"
    );

    app.cancel_settings();

    let after = app.settings();
    assert_eq!(after.theme.font_size, before.theme.font_size, "字号应回滚");
    assert_eq!(after.theme.kind, before.theme.kind, "主题应回滚");
    assert_eq!(after.editor.tab_size, before.editor.tab_size, "缩进应回滚");
    assert_eq!(
        after.editor.word_wrap, before.editor.word_wrap,
        "自动换行应回滚"
    );
    assert_eq!(
        combo_of(&app, UiAction::Bold),
        bold_before,
        "快捷键应回滚，且**注册表**也要跟着回滚（否则界面显示与实际生效不一致）"
    );
}

/// 回滚发生在「打开设置之后」，打开之前的修改不应被回滚。
///
/// 快照取自 `open_settings`（打开那一刻）而不是「会话开始」——
/// 否则用户在设置外改过的值会被一次无关的「取消」抹掉。
#[test]
fn cancel_only_rolls_back_changes_made_after_opening() {
    let (_dir, mut app) = app();
    // 设置**外**的修改：菜单里换主题。
    app.execute(UiAction::ToggleTheme);
    let outside = app.settings().theme.kind;

    app.execute(UiAction::OpenSettings);
    app.apply_editor_field(|e| e.tab_size = 8);
    app.cancel_settings();

    assert_eq!(
        app.settings().theme.kind,
        outside,
        "设置打开**之前**改的主题不应被这次取消回滚"
    );
    assert_ne!(app.settings().editor.tab_size, 8);
}

/// 「恢复本组默认」只重置当前分区。
///
/// 这是原型缺陷 3.6 的根治点：原型调的是全量 `reset()`，
/// 一次打回 13 个字段，用户只想改字体却连主题与缩进都被重置。
#[test]
fn reset_section_only_touches_the_current_section() {
    let (_dir, mut app) = app();

    // 三个分区全部改成非默认值。
    app.apply_theme_field(|t| t.font_size = 20.0);
    app.apply_editor_field(|e| e.tab_size = 8);
    app.rebind_shortcut(UiAction::Bold, "Ctrl+Alt+B");
    app.execute(UiAction::OpenSettings);

    app.settings_ui.section = SettingsTab::Appearance;
    app.reset_current_settings_section();

    let after = app.settings();
    assert_eq!(after.theme.font_size, 14.0, "外观分区应回到默认字号");
    assert_eq!(
        after.editor.tab_size, 8,
        "编辑器分区**必须**保持用户设的值——这是本用例的全部意义"
    );
    assert_eq!(
        combo_of(&app, UiAction::Bold),
        Some("Ctrl+Alt+B".to_string()),
        "快捷键分区**必须**保持用户设的键位"
    );
}

/// 「编辑器」分区重置不影响主题与快捷键。
#[test]
fn resetting_editor_section_leaves_theme_and_shortcuts_alone() {
    let (_dir, mut app) = app();
    app.apply_theme_field(|t| t.font_size = 20.0);
    app.apply_editor_field(|e| e.tab_size = 8);
    app.rebind_shortcut(UiAction::Bold, "Ctrl+Alt+B");
    app.execute(UiAction::OpenSettings);

    app.settings_ui.section = SettingsTab::Editor;
    app.reset_current_settings_section();

    let after = app.settings();
    assert_eq!(after.editor.tab_size, 4, "编辑器分区应回到默认");
    assert_eq!(after.theme.font_size, 20.0, "主题不受影响");
    assert_eq!(
        combo_of(&app, UiAction::Bold),
        Some("Ctrl+Alt+B".to_string()),
        "快捷键不受影响"
    );
}

/// 「保存」确认修改（不回滚）。
#[test]
fn confirm_keeps_the_changes() {
    let (_dir, mut app) = app();
    app.execute(UiAction::OpenSettings);
    app.apply_theme_field(|t| t.font_size = 20.0);
    app.apply_editor_field(|e| e.tab_size = 8);

    app.confirm_settings();

    let after = app.settings();
    assert_eq!(after.theme.font_size, 20.0, "保存应保留改动");
    assert_eq!(after.editor.tab_size, 8);
    assert!(
        app.settings_ui.snapshot.is_none(),
        "关闭后应丢弃快照，否则下次打开会拿旧快照当基准"
    );
}

/// 「保存」与「取消」的差别只有一处：回滚与否。
///
/// 写成两条独立用例（上面的 `confirm_keeps_the_changes` 与
/// `cancel_rolls_back_theme_editor_and_shortcuts`）之外再钉这一条，
/// 是为了防止将来有人把「保存」也实现成回滚——那会让设置页完全无法使用，
/// 而症状是「改了没反应」，很容易被误判成领域不接收变更。
#[test]
fn save_and_cancel_differ_only_in_rollback() {
    let mut results = Vec::new();
    for confirm in [true, false] {
        let (_dir, mut app) = app();
        app.execute(UiAction::OpenSettings);
        app.apply_theme_field(|t| t.font_size = 20.0);
        if confirm {
            app.confirm_settings();
        } else {
            app.cancel_settings();
        }
        results.push(app.settings().theme.font_size);
    }
    assert_eq!(
        results,
        vec![20.0, 14.0],
        "保存=保留 / 取消=回滚；两者若相同说明其中一条写错了"
    );
}

/// 「关于」分区不提供「恢复本组默认」。
///
/// 它没有任何设置项，挂一个点了没反应的按钮比不挂更糟
/// （用户会以为程序坏了）。
#[test]
fn about_tab_has_no_reset_button() {
    let mut visible = Vec::new();
    for tab in SettingsTab::ALL {
        let (_dir, mut app) = app();
        app.execute(UiAction::OpenSettings);
        app.settings_ui.section = *tab;
        visible.push((*tab, app.settings_ui.can_reset_section()));
    }
    let about = visible
        .iter()
        .find(|(t, _)| *t == SettingsTab::About)
        .unwrap();
    assert!(!about.1, "「关于」不该有「恢复本组默认」");
    let appearance = visible
        .iter()
        .find(|(t, _)| *t == SettingsTab::Appearance)
        .unwrap();
    assert!(appearance.1, "「外观」应有「恢复本组默认」");
}

/// 「快捷键」分区用专属按钮（而非分区重置），且重置后注册表同步。
#[test]
fn reset_all_shortcuts_restores_defaults_and_registry() {
    let (_dir, mut app) = app();
    let original = combo_of(&app, UiAction::Bold);
    app.rebind_shortcut(UiAction::Bold, "Ctrl+Alt+B");
    assert_eq!(combo_of(&app, UiAction::Bold), Some("Ctrl+Alt+B".into()));

    app.reset_all_shortcuts();

    assert_eq!(
        combo_of(&app, UiAction::Bold),
        original,
        "恢复默认后**注册表**也要同步，否则界面显示与实际生效不一致"
    );
    assert_eq!(
        app.settings()
            .shortcuts
            .iter()
            .find(|s| s.key == "bold")
            .map(|s| s.combo.clone()),
        original,
        "持久化配置也应恢复默认"
    );
}

/// 改键后注册表立即生效（否则界面写着新键、按下去走旧动作）。
#[test]
fn rebinding_shortcut_updates_the_registry_immediately() {
    let (_dir, mut app) = app();
    app.rebind_shortcut(UiAction::Bold, "Ctrl+Alt+B");

    assert_eq!(
        combo_of(&app, UiAction::Bold),
        Some("Ctrl+Alt+B".into()),
        "改键后注册表必须立刻反映新键位"
    );
    assert!(
        app.settings()
            .shortcuts
            .iter()
            .any(|s| s.key == "bold" && s.combo == "Ctrl+Alt+B"),
        "改键必须落盘，否则重启后失效"
    );
}

/// 有阻断级冲突的改键被拒绝，且**原键位不变**。
///
/// 原型缺陷：静默写入后被注册顺序在前的同名动作吞掉，界面无任何提示。
#[test]
fn conflicting_rebind_is_rejected_and_leaves_the_original_intact() {
    let (_dir, mut app) = app();
    let save = combo_of(&app, UiAction::Save).expect("Save 应有默认键位");
    let bold_before = combo_of(&app, UiAction::Bold);

    // 直接抢 Save 的键位。
    app.rebind_shortcut(UiAction::Bold, &save);

    assert_eq!(
        combo_of(&app, UiAction::Bold),
        bold_before,
        "冲突的改键必须被拒绝，原键位保持不变"
    );
    assert_eq!(
        combo_of(&app, UiAction::Save),
        Some(save.clone()),
        "被抢的动作也不该被动"
    );
    assert!(
        app.settings_ui.error.is_some(),
        "冲突必须有可见提示（原型是静默吞掉）"
    );
}

/// 非法字号被领域拒绝时给出可见错误，且**不**改动配置。
#[test]
fn invalid_font_size_is_rejected_with_a_visible_error() {
    let (_dir, mut app) = app();
    app.execute(UiAction::OpenSettings);
    let before = app.settings().theme.font_size;

    app.apply_theme_field(|t| t.font_size = 999.0);

    assert_eq!(
        app.settings().theme.font_size,
        before,
        "非法字号不得写进配置"
    );
    assert!(
        app.settings_ui.error.is_some(),
        "非法取值必须给出可见提示，否则用户以为拖了滑块没反应"
    );
}

/// 「恢复本组默认」在「关于」页被忽略（而不是误重置别的分区）。
#[test]
fn reset_is_ignored_on_the_about_tab() {
    let (_dir, mut app) = app();
    app.apply_editor_field(|e| e.tab_size = 8);
    app.execute(UiAction::OpenSettings);
    app.settings_ui.section = SettingsTab::About;

    app.reset_current_settings_section();

    assert_eq!(
        app.settings().editor.tab_size,
        8,
        "在「关于」页触发重置不应误伤编辑器分区"
    );
}

/// 应用层 `reset_section` 直接调用（不经 UI）也保持分区隔离。
///
/// UI 层的 `can_reset_section` 只是「按钮是否显示」，
/// 真正的隔离由应用/领域保证——这条用例钉的是后者。
#[test]
fn reset_section_command_isolates_partitions_at_the_service_level() {
    let (_dir, mut app) = app();
    app.apply_theme_field(|t| t.font_size = 20.0);
    app.apply_editor_field(|e| e.tab_size = 8);

    app.services
        .preferences
        .reset_section(ResetSectionCommand {
            section: SettingsSection::Shortcuts,
        })
        .expect("重置快捷键分区应成功");

    let after = app.settings();
    assert_eq!(after.theme.font_size, 20.0, "重置快捷键不应动主题");
    assert_eq!(after.editor.tab_size, 8, "重置快捷键不应动编辑器设置");
}

/// 打开设置浮层会记录快照；关闭后快照被丢弃。
#[test]
fn opening_records_a_snapshot_and_closing_discards_it() {
    let (_dir, mut app) = app();
    assert!(app.settings_ui.snapshot.is_none());
    app.execute(UiAction::OpenSettings);
    assert!(app.settings_ui.snapshot.is_some());
    app.confirm_settings();
    assert!(app.settings_ui.snapshot.is_none());
}

/// 设置浮层真的被渲染（真实帧）。
///
/// 钉的是 S6-07 / S6-08 那类**静默失效**：动作能打开浮层、浮层在栈里、
/// 但没有视图渲染它——不报错、界面正常、功能毫无反应。
/// 跑一帧完整界面。
fn frame(app: &mut EditorApp, ctx: &egui::Context) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1000.0, 800.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
        main_view::show(app, ctx);
    });
}

fn frames(app: &mut EditorApp, ctx: &egui::Context, n: usize) {
    for _ in 0..n.max(2) {
        frame(app, ctx);
    }
}

/// 设置浮层真的被渲染（真实帧）。
///
/// 钉的是 S6-07 / S6-08 那类**静默失效**：动作能打开浮层、浮层在栈里、
/// 但没有视图渲染它——不报错、界面正常、功能毫无反应。
///
/// 判据是「焦点落到了字体族输入框」：egui 0.33 没有文本回读接口，
/// 而焦点能落在一个控件上就证明整棵控件树被布局过了。
/// 若 `overlays::show` 漏了 `Settings` 分支，控件从未被布局，焦点也就无处可落。
#[test]
fn opening_settings_actually_renders_it_in_a_real_frame() {
    let (_dir, mut app) = app();
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 2);
    assert!(!app.dialog.is_open(Overlay::Settings), "初始不应打开设置");

    app.execute(UiAction::OpenSettings);
    assert!(app.dialog.is_open(Overlay::Settings));
    frames(&mut app, &ctx, 2);

    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::new(FONT_INPUT_ID))),
        "设置面板未渲染：字体族输入框拿不到焦点（`overlays::show` 可能没接上设置分支）"
    );
}

/// Esc 等价于「取消」：回滚 + 关面板。
///
/// 三条关闭路径（取消按钮 / 标题栏 × / Esc）必须一致。
/// 这条锁的是 S6-11 修掉的一个缺陷：`CloseOverlay` 曾直接
/// `dialog.close_top`，于是 Esc **只关面板不回滚**——
/// 用户改了字号按 Esc，面板没了但字号没回去，且没有任何提示。
#[test]
fn escape_behaves_like_cancel() {
    let (_dir, mut app) = app();
    app.execute(UiAction::OpenSettings);
    app.apply_theme_field(|t| t.font_size = 20.0);

    app.execute(UiAction::CloseOverlay);

    assert!(!app.dialog.is_open(Overlay::Settings), "Esc 应关闭设置浮层");
    assert_eq!(
        app.settings().theme.font_size,
        14.0,
        "Esc 应回滚未确认的修改（否则用户以为取消了但设置没撤）"
    );
}

/// 「保存」等价于确认：保留 + 关面板。
#[test]
fn confirm_closes_the_overlay() {
    let (_dir, mut app) = app();
    app.execute(UiAction::OpenSettings);
    app.apply_theme_field(|t| t.font_size = 20.0);

    app.confirm_settings();

    assert!(!app.dialog.is_open(Overlay::Settings), "保存应关闭浮层");
    assert_eq!(app.settings().theme.font_size, 20.0, "保存应保留改动");
}

/// 浮层是**模态**：打开时背景不可交互（验收标准的 a11y 条款）。
#[test]
fn settings_is_modal_so_shortcuts_do_not_reach_the_editor() {
    let (_dir, mut app) = app();
    app.execute(UiAction::OpenSettings);
    assert!(
        app.dialog.is_modal(),
        "设置应对话框必须是模态：否则按 Ctrl+B 会去改文档而不是在对话框里操作"
    );
    app.confirm_settings();
    assert!(!app.dialog.is_modal(), "关闭后应恢复非模态");
}

/// 设置浮层有视图实现（与 `PENDING_VIEWS` 清单互斥）。
#[test]
fn settings_overlay_has_a_view() {
    assert!(
        overlays::has_view(Overlay::Settings),
        "设置浮层已实现视图，应从 `PENDING_VIEWS` 中移除"
    );
    assert!(
        !overlays::PENDING_VIEWS.contains(&Overlay::Settings),
        "已实现的浮层必须从待办清单里删掉，否则清单会骗人"
    );
}

/// 自定义主题的调色板编辑器（含 V11-09 语法色折叠区）必须能在真实帧里渲染，
/// 且语法色覆盖能经设置写入路径落到设置里。
///
/// 判据选「写入后再读回」而不是「有没有画出某个控件」：egui 0.33 无文本回读，
/// 而「设置真的被记下」才是这个编辑器的职责。
#[test]
fn custom_theme_palette_editor_renders_and_records_syntax_override() {
    let (_dir, mut app) = app();
    // 切到自定义：服务应补齐一份默认调色板。
    app.apply_theme_field(|t| t.kind = "custom".to_string());
    assert!(
        app.settings().theme.custom.is_some(),
        "切到自定义应补齐默认调色板"
    );

    // 走与取色器同一条写入路径：覆盖一个语法色。
    app.apply_theme_field(|t| {
        if let Some(p) = t.custom.as_mut() {
            p.syn_heading = Some("#00FF00".to_string());
        }
    });
    assert_eq!(
        app.settings()
            .theme
            .custom
            .as_ref()
            .and_then(|p| p.syn_heading.as_deref()),
        Some("#00FF00"),
        "语法色覆盖应写进设置"
    );

    // 真实帧渲染「外观」页（含折叠区），不得 panic。
    app.settings_ui.section = SettingsTab::Appearance;
    app.settings_ui.open(app.settings_snapshot());
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 2);
    assert_eq!(app.settings().theme.kind, "custom");
}

// ============================================================================
// 设置窗口高度不得逐帧增长（回归：点开设置后浮窗「爆炸」到超出屏幕）
// ============================================================================
//
// # 这个缺陷的形态
//
// `settings_view::body` 曾在 `ui.horizontal_top(...)` 里直接调 `ui.separator()`。
// egui 的竖直分隔线取 `available_size_before_wrap().y` 作高度，而设置对话框是
// egui 的 `Window`——**按内容自动定高**。两者构成正反馈：窗口越高 → 可用高度越大
// → 分隔线越高 → 内容越高 → 下一帧窗口更高。实测每帧 +~150pt，四帧内从 411
// 涨到 1066，把「保存 / 取消」推到屏幕之外（用户看到的就是「设置浮窗显示异常」）。
//
// # 为什么断言「高度不增长」而不是断言具体数值
//
// 具体数值随主题字号 / 分区内容变化，写死就会在正常改动下变红。这里只钉住
// 那条**反馈环**：连跑若干帧后高度必须与第一帧稳定帧一致。
#[test]
fn settings_window_height_does_not_grow_every_frame() {
    let (_dir, mut app) = app();
    app.settings_ui.section = SettingsTab::Appearance;
    app.dialog.open(Overlay::Settings);

    let id = egui::Id::new("rustmark.overlay.settings");
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 900.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run(input.clone(), |_| {}); // 装载字库

    let mut heights = Vec::new();
    for _ in 0..8 {
        let _ = ctx.run(input.clone(), |ctx| {
            rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
            main_view::show(&mut app, ctx);
        });
        heights.push(
            ctx.memory(|m| m.area_rect(id))
                .map(|r| r.height())
                .unwrap_or(0.0),
        );
    }

    // 稳定帧：跳过首帧的测量 pass。
    let settled = heights[1];
    for (frame, h) in heights.iter().enumerate().skip(2) {
        assert!(
            (h - settled).abs() < 1.0,
            "设置窗口高度逐帧增长（第 {frame} 帧 {h:.0}，第 1 帧 {settled:.0}，\
             全部 {heights:?}）——多半是水平布局里用了 `ui.separator()`，\
             它与「窗口按内容自动定高」形成正反馈"
        );
    }
    assert!(
        settled <= 900.0,
        "设置窗口高度 {settled:.0} 超出屏幕高度 900，底部按钮会被切掉"
    );
}
