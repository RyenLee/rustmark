//! 调度层用例（S6-01）：动作执行、浮层映射、主题轮换。
//!
//! `eframe::App::update` 需要真实窗口，本文件只测**可脱离窗口验证**的部分：
//! 动作 → 用例/UI 状态的映射。这类映射正是「三入口一致」的兑现点，
//! 也是最容易在后续子任务接线时被改坏的地方，所以先钉住。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::shortcuts::registry::{ShortcutRegistry, UiAction};
use rustmark::presentation::state::app_ui_state::{SPLIT_MAX, SPLIT_MIN, SidebarTab};
use rustmark::presentation::state::dialog_ui_state::Overlay;
use rustmark::presentation::theme::builtin::BuiltinTheme;
use std::sync::Arc;

/// 造一个装好默认配置的应用根状态。
fn app() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

#[test]
fn startup_applies_persisted_shortcuts_to_dispatcher() {
    let (_dir, app) = app();
    // 分发器必须**不是**默认表，而是来自配置的那一份——
    // 两者相同只在「用户从未改键」时成立，所以这条只验证接线而非内容。
    let display = app.dispatcher().registry().display_for(UiAction::Save);
    assert!(
        !display.is_empty(),
        "保存动作必须有键位显示，否则菜单里是空白"
    );
}

#[test]
fn layout_actions_toggle_independent_flags() {
    let (_dir, mut app) = app();
    let before = (
        app.ui.sidebar_visible,
        app.ui.preview_visible,
        app.ui.focus_mode,
    );

    app.execute(UiAction::ToggleSidebar);
    app.execute(UiAction::TogglePreview);
    app.execute(UiAction::ToggleFocusMode);

    assert_ne!(app.ui.sidebar_visible, before.0, "侧栏开关应翻转");
    assert_ne!(app.ui.preview_visible, before.1, "预览开关应翻转");
    assert!(app.ui.focus_mode, "专注模式应进入");

    // 再点一次应各自回到原值——三个开关互不干扰。
    app.execute(UiAction::ToggleSidebar);
    app.execute(UiAction::TogglePreview);
    app.execute(UiAction::ToggleFocusMode);
    assert_eq!(
        (
            app.ui.sidebar_visible,
            app.ui.preview_visible,
            app.ui.focus_mode
        ),
        before,
        "三个布局开关必须各自独立可逆"
    );
}

/// 侧栏开合跨重启保留（问题 062）：默认收起 → 展开并落盘 → 重新装配
/// （同一份配置）仍是展开；收起并落盘 → 仍是收起。
///
/// 这条只管「值确实进了配置文件、启动时确实读回来了」；「落盘发生在哪一帧」
/// 由 `tests/presentation/sidebar_toggle_test.rs` 的真帧用例钉住。
#[test]
fn sidebar_visibility_round_trips_through_preferences() {
    let (dir, mut app) = app();
    assert!(
        !app.ui.sidebar_visible,
        "首次启动（无配置）应默认收起：内容区优先"
    );

    app.execute(UiAction::ToggleSidebar);
    assert!(app.ui.sidebar_visible);
    app.persist_sidebar_visibility();

    let restarted = EditorApp::new(
        Services::assemble_with_dialogs(
            ConfigPaths::in_dir(dir.path()),
            Arc::new(FakeDialog::new()),
        )
        .expect("装配应成功"),
        false,
    );
    assert!(
        restarted.ui.sidebar_visible,
        "上次是展开，重启后应仍是展开（记住上次的开合状态）"
    );

    // 反向：收起后重启仍是收起。
    let mut reopened = EditorApp::new(
        Services::assemble_with_dialogs(
            ConfigPaths::in_dir(dir.path()),
            Arc::new(FakeDialog::new()),
        )
        .expect("装配应成功"),
        false,
    );
    reopened.execute(UiAction::ToggleSidebar);
    reopened.persist_sidebar_visibility();
    assert!(!reopened.ui.sidebar_visible);

    let collapsed = EditorApp::new(
        Services::assemble_with_dialogs(
            ConfigPaths::in_dir(dir.path()),
            Arc::new(FakeDialog::new()),
        )
        .expect("装配应成功"),
        false,
    );
    assert!(
        !collapsed.ui.sidebar_visible,
        "上次收起过，重启后应仍是收起"
    );
}

#[test]
fn every_overlay_opening_action_actually_opens_its_overlay() {
    let (_dir, mut app) = app();
    for overlay in Overlay::ALL {
        let Some(action) = overlay.opened_by() else {
            continue;
        };
        app.dialog.close_all();
        app.execute(action);
        assert_eq!(
            app.dialog.top(),
            Some(*overlay),
            "动作 {:?} 应打开浮层 {:?}",
            action,
            overlay.title()
        );
    }
}

#[test]
fn close_overlay_closes_top_overlay_first() {
    let (_dir, mut app) = app();
    app.execute(UiAction::Find);
    app.execute(UiAction::OpenSettings);
    assert_eq!(app.dialog.top(), Some(Overlay::Settings));

    app.execute(UiAction::CloseOverlay);
    assert_eq!(
        app.dialog.top(),
        Some(Overlay::Find),
        "Esc 一次只应关掉最上面那层"
    );

    app.execute(UiAction::CloseOverlay);
    assert!(!app.dialog.any_open(), "再按一次应把最后一层也关掉");
}

#[test]
fn close_overlay_does_not_silently_exit_focus_mode() {
    let (_dir, mut app) = app();
    app.execute(UiAction::ToggleFocusMode);
    assert!(app.ui.focus_mode, "应已进入专注模式");

    app.execute(UiAction::Find);
    app.execute(UiAction::CloseOverlay);

    assert!(
        app.ui.focus_mode,
        "关浮层的那次 Esc 不应顺带退出专注模式——一次按键做两件事几乎总是 bug"
    );

    // 浮层都关完后再按，才退出专注模式。
    app.execute(UiAction::CloseOverlay);
    assert!(!app.ui.focus_mode, "无浮层可关时 Esc 才退出专注模式");
}

#[test]
fn toggle_outline_switches_sidebar_tab() {
    let (_dir, mut app) = app();
    assert_eq!(app.ui.sidebar_tab, SidebarTab::Files);
    // 默认收起（问题 061）：本用例钉的是「标签切换与收起」语义，先展开
    // 到可见基线。
    app.ui.sidebar_visible = true;
    assert!(app.ui.sidebar_visible);

    app.execute(UiAction::ToggleOutline);
    assert_eq!(app.ui.sidebar_tab, SidebarTab::Outline);
    assert!(app.ui.sidebar_visible);

    // 再次点击同一标签应**收起侧栏**（标签选择保持不变）。
    // 这正是「标签栏点不动」缺陷的来源：若第二次点击只是
    // 「把 tab 设成它自己」，用户会觉得按钮坏了。
    app.execute(UiAction::ToggleOutline);
    assert_eq!(
        app.ui.sidebar_tab,
        SidebarTab::Outline,
        "收起时不应改变已选标签——否则用户切回来要重新选一次"
    );
    assert!(
        !app.ui.sidebar_visible,
        "再次点击同一标签应收起侧栏，否则标签栏看起来点不动"
    );
}

#[test]
fn theme_toggle_swaps_dark_and_light_directly() {
    let (_dir, mut app) = app();
    // 首次跟随系统；本例 system_dark=false → 解析为浅色（生效态 = 浅色）。
    assert_eq!(app.theme().kind, BuiltinTheme::Light);
    assert_eq!(app.settings().theme.kind, "system");

    // 一键切换 = 直接深浅互换，两向都只要一次点击（#039：原来浅色点回深色要点两次）。
    // 从「生效浅色」点一下 → 深色。
    app.execute(UiAction::ToggleTheme);
    assert_eq!(app.settings().theme.kind, "dark");
    assert_eq!(app.theme().kind, BuiltinTheme::Dark);

    // 再点一下 → 浅色（不再是回到 system）。
    app.execute(UiAction::ToggleTheme);
    assert_eq!(app.settings().theme.kind, "light");
    assert_eq!(app.theme().kind, BuiltinTheme::Light);

    // 再点一下 → 深色，循环就是纯粹的 dark↔light。
    app.execute(UiAction::ToggleTheme);
    assert_eq!(app.settings().theme.kind, "dark");
    assert_eq!(app.theme().kind, BuiltinTheme::Dark);
}

#[test]
fn theme_toggle_from_system_resolves_by_effective_mode() {
    // 从「跟随系统」出发、系统偏好为浅色时，第一下切换得到深色：
    // 取反的是「当前生效的明暗」，而非切换到某个固定序列的下一态。
    let (_dir, mut app) = app();
    assert_eq!(app.settings().theme.kind, "system");
    app.execute(UiAction::ToggleTheme);
    assert_eq!(
        app.settings().theme.kind,
        "dark",
        "跟随系统解析为浅色时，点一下应得到深色"
    );
}

#[test]
fn theme_toggle_never_panics_on_unwired_actions() {
    let (_dir, mut app) = app();
    // 逐个执行**全部**动作：尚未接线的必须安静无事，
    // 一旦 panic，用户按下任何未接线快捷键都会直接丢掉窗口。
    for action in UiAction::ALL {
        app.execute(*action);
    }
    assert!(
        UiAction::ALL.len() > 50,
        "动作表规模异常，测试可能漏掉了新枚举"
    );
}

#[test]
fn split_ratio_stays_within_bounds_after_layout_actions() {
    let (_dir, mut app) = app();
    for _ in 0..10 {
        app.execute(UiAction::TogglePreview);
    }
    assert!(
        (SPLIT_MIN..=SPLIT_MAX).contains(&app.ui.split_ratio),
        "分割比例越界会算出负宽度面板，实际：{}",
        app.ui.split_ratio
    );
}

#[test]
fn custom_shortcut_registry_is_used_instead_of_hardcoded_defaults() {
    // 把「保存」改绑到一个默认未被占用的键，重建后必须生效。
    // 这条锁住的是「接线」：若 `EditorApp::new` 忽略了配置里的shortcuts，
    // 用户改的键位在重启后全部失效，而界面不会报任何错。
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let paths = ConfigPaths::in_dir(dir.path());
    let services = Services::assemble(paths.clone()).expect("装配应成功");

    let mut theme = app_settings_theme(&services);
    theme.kind = "dark".to_string();
    services
        .preferences
        .set_theme(
            rustmark::application::preferences::commands::SetThemeCommand {
                theme: theme.clone(),
            },
        )
        .expect("应可写配置");
    let combo = rustmark::presentation::shortcuts::registry::normalize("Ctrl+Alt+Shift+P")
        .expect("应可归一化");
    services
        .preferences
        .set_shortcut(
            rustmark::application::preferences::commands::SetShortcutCommand {
                action: rustmark::domain::preferences::value_objects::ActionId::Save,
                combo: combo.display(),
            },
        )
        .expect("应可改键");

    // 重新装配（模拟重启）——这是唯一能证明「改键真的落盘了」的方式。
    drop(services);
    let reborn = Services::assemble(paths).expect("应可重新装配");
    let app = EditorApp::new(reborn, false);
    assert_eq!(
        app.dispatcher().registry().display_for(UiAction::Save),
        "Ctrl+Alt+Shift+P",
        "重启后必须用持久化的键位，而不是默认值"
    );
}

/// 从服务里读出当前主题设置（测试辅助）。
fn app_settings_theme(services: &Services) -> rustmark::application::dto::ThemeView {
    services
        .preferences
        .get_settings(rustmark::application::preferences::queries::GetSettingsQuery)
        .expect("应可读设置")
        .theme
}

/// 证明 `ShortcutRegistry` 的默认构造与「有配置」构造确实不同，
/// 否则上面那条「重启后用持久化键位」的用例可能是**假通过**。
#[test]
fn persisted_registry_differs_from_defaults_when_user_rebound() {
    let custom = ShortcutRegistry::from_settings(&[rustmark::application::dto::ShortcutView {
        key: "save".to_string(),
        action: "保存".to_string(),
        combo: "Ctrl+Alt+Shift+P".to_string(),
    }]);
    assert_eq!(custom.display_for(UiAction::Save), "Ctrl+Alt+Shift+P");
    assert_eq!(
        ShortcutRegistry::with_defaults().display_for(UiAction::Save),
        "Ctrl+S"
    );
}
