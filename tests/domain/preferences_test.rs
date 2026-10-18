//! Preferences 上下文单测（S3-12）。
//!
//! 前 5 条是**原型缺陷 1.1 的免疫用例**：原型的快捷键存在「四处真相」与
//! 「匹配层拒绝纯单键」的问题，这里把规则固化为可单测的纯函数。

use rustmark::domain::preferences::aggregate::Preferences;
use rustmark::domain::preferences::errors::PreferencesError;
use rustmark::domain::preferences::value_objects::{
    ActionId, ConflictKind, EditorSettings, Key, KeyCombo, LintSettings, Modifiers, NamedKey,
    ShortcutBinding, ShortcutTable, TabSize, Theme, ThemeKind, WritingGoal,
};

fn combo(raw: &str) -> KeyCombo {
    KeyCombo::parse(raw).expect("测试用组合应合法")
}

// ---------- 原型缺陷免疫用例 ----------

/// ① `Ctrl+S` 与 `Ctrl+Shift+S` **不得**被判为冲突（原型把 Shift 当成了可选修饰键）。
#[test]
fn ctrl_s_and_ctrl_shift_s_are_not_conflicting() {
    let table = ShortcutTable::new(vec![
        ShortcutBinding::parse(ActionId::Save, "Ctrl+S").unwrap(),
        ShortcutBinding::parse(ActionId::SaveAs, "Ctrl+Shift+S").unwrap(),
    ]);
    assert_ne!(combo("Ctrl+S"), combo("Ctrl+Shift+S"), "两者本就是不同组合");
    assert!(
        !table.conflicts().iter().any(|c| c.kind().is_blocking()),
        "不得报告阻断级冲突：{:?}",
        table.conflicts()
    );
}

/// ② 同一组合绑定到两个动作 → 必须报告 `Duplicate` 冲突（原型缺陷 D）。
#[test]
fn duplicate_binding_reports_conflict() {
    let mut table = ShortcutTable::new(Vec::new());
    table.rebind(ActionId::Save, combo("Ctrl+S"));
    table.rebind(ActionId::Export, combo("Ctrl+S"));

    let conflicts = table.conflicts();
    let duplicate = conflicts
        .iter()
        .find(|c| c.kind() == ConflictKind::Duplicate)
        .expect("应报告重复冲突");
    assert_eq!(duplicate.combo(), combo("Ctrl+S"));
    assert_eq!(duplicate.actions().len(), 2);
    assert!(!table.is_usable(), "存在阻断级冲突时不应判定为可用");
}

/// ③ `Escape` 与 `F1`–`F12` **必须允许**绑定（原型在匹配层拒绝纯单键）。
#[test]
fn escape_and_function_keys_are_bindable() {
    let mut table = ShortcutTable::new(Vec::new());
    table.rebind(
        ActionId::CommandPalette,
        KeyCombo::plain(Key::Named(NamedKey::Escape)),
    );
    table.rebind(
        ActionId::ToggleOutline,
        KeyCombo::plain(Key::Named(NamedKey::F5)),
    );

    assert_eq!(
        table.action_for(combo("Escape")),
        Some(ActionId::CommandPalette)
    );
    assert_eq!(table.action_for(combo("F5")), Some(ActionId::ToggleOutline));
    assert!(
        !table.conflicts().iter().any(|c| c.kind().is_blocking()),
        "无修饰键的功能键不得被判为冲突：{:?}",
        table.conflicts()
    );
}

/// ④ 组合归一化：`Ctrl+Shift+S` 与 `Shift+Ctrl+s` 必须判等。
#[test]
fn combo_normalization_makes_writing_order_irrelevant() {
    assert_eq!(combo("Ctrl+Shift+S"), combo("Shift+Ctrl+s"));
    assert_eq!(combo("ctrl+shift+S"), combo("Ctrl+Shift+s"));
    assert_eq!(combo("Meta+S"), combo("Cmd+S"));
    assert_eq!(combo("Alt+S"), combo("Option+S"));
    assert_ne!(combo("Ctrl+S"), combo("Ctrl+Shift+S"));
}

/// ⑤ 显示文本**唯一来源**：菜单 / 命令面板 / 快捷键一览共用 `display_all()`。
#[test]
fn display_all_is_the_single_source_of_truth() {
    let mut table = ShortcutTable::new(Vec::new());
    table.rebind(ActionId::Save, combo("ctrl+shift+s"));

    let shown: Vec<String> = table
        .display_all()
        .iter()
        .map(|(_, text)| text.clone())
        .collect();
    assert_eq!(
        shown,
        vec!["Ctrl+Shift+S".to_string()],
        "显示必须是规范化形式"
    );
    assert_eq!(
        table.binding_for(ActionId::Save).map(|c| c.display()),
        Some("Ctrl+Shift+S".to_string())
    );
}

// ---------- 其它规则 ----------

#[test]
fn bare_letter_key_reports_text_input_clash_only_as_warning() {
    let mut table = ShortcutTable::new(Vec::new());
    table.rebind(ActionId::Bold, KeyCombo::plain(Key::Char('B')));

    let conflicts = table.conflicts();
    assert!(
        conflicts
            .iter()
            .any(|c| c.kind() == ConflictKind::TextInputClash)
    );
    assert!(table.is_usable(), "文本输入冲突只是提示，不应阻断");
}

#[test]
fn reserved_combo_is_reported() {
    let mut table = ShortcutTable::new(Vec::new());
    table.rebind(ActionId::CloseTab, combo("Meta+W"));
    assert!(
        table
            .conflicts()
            .iter()
            .any(|c| c.kind() == ConflictKind::Reserved)
    );
}

#[test]
fn default_bindings_have_no_blocking_conflicts() {
    let table = ShortcutTable::default_bindings().expect("默认绑定应可解析");
    assert!(
        !table.conflicts().iter().any(|c| c.kind().is_blocking()),
        "默认绑定不得自带冲突：{:?}",
        table.conflicts()
    );
    assert!(
        ActionId::all()
            .iter()
            .all(|a| table.binding_for(*a).is_some()),
        "每个动作都应有默认绑定"
    );
}

#[test]
fn invalid_combo_string_is_rejected() {
    assert!(matches!(
        KeyCombo::parse("Ctrl+"),
        Err(PreferencesError::InvalidCombo { .. })
    ));
    // 修饰键可以出现在任意位置（`Ctrl+S+Alt` 合法）；出现两个主键才应被拒绝。
    assert!(KeyCombo::parse("Ctrl+S+Alt").is_ok());
    assert!(KeyCombo::parse("Ctrl+S+A").is_err(), "多个主键应被拒绝");
    assert!(KeyCombo::parse("").is_err());
}

#[test]
fn rebind_replaces_existing_binding() {
    let mut table = ShortcutTable::default_bindings().unwrap();
    table.rebind(ActionId::Save, combo("Ctrl+Alt+S"));
    assert_eq!(table.binding_for(ActionId::Save), Some(combo("Ctrl+Alt+S")));
    assert_eq!(
        table
            .bindings()
            .iter()
            .filter(|b| b.action() == ActionId::Save)
            .count(),
        1,
        "重绑定不得产生重复条目"
    );
}

#[test]
fn tab_size_maps_to_indent_unit() {
    assert_eq!(TabSize::Two.indent_unit(), "  ");
    assert_eq!(TabSize::Four.indent_unit(), "    ");
    assert_eq!(TabSize::Eight.indent_unit(), "        ");
    assert_eq!(TabSize::from_spaces(4), Some(TabSize::Four));
    assert_eq!(TabSize::from_spaces(3), None);
}

#[test]
fn preferences_rejects_invalid_editor_settings() {
    let mut preferences = Preferences::default_preferences();
    let invalid = EditorSettings::new(
        TabSize::Four,
        true,
        Some(0),
        false,
        false,
        400,
        0.5,
        Some(false),
        WritingGoal::new(0),
        LintSettings::all_enabled(),
        12,
    );
    assert!(matches!(
        preferences.set_editor(invalid),
        Err(PreferencesError::InvalidSetting { .. })
    ));
}

#[test]
fn preferences_rejects_invalid_theme() {
    let mut preferences = Preferences::default_preferences();
    let invalid = Theme::new(ThemeKind::Dark, "", 14.0, 1.6);
    assert!(matches!(
        preferences.set_theme(invalid),
        Err(PreferencesError::InvalidSetting { .. })
    ));
    let too_large = Theme::new(ThemeKind::Dark, "system-ui", 200.0, 1.6);
    assert!(preferences.set_theme(too_large).is_err());
}

#[test]
fn preferences_indent_unit_derives_from_tab_size() {
    let mut preferences = Preferences::default_preferences();
    preferences
        .set_editor(EditorSettings::new(
            TabSize::Eight,
            true,
            None,
            false,
            false,
            400,
            0.5,
            Some(false),
            WritingGoal::new(0),
            LintSettings::all_enabled(),
            12,
        ))
        .unwrap();
    assert_eq!(preferences.indent_unit(), "        ");
}

#[test]
fn modifiers_equality_ignores_construction_order() {
    let a = Modifiers::new(true, false, true, false);
    let b = Modifiers::new(true, false, true, false);
    assert_eq!(a, b);
    assert!(Modifiers::NONE.is_empty());
    assert!(!a.is_empty());
}

#[test]
fn rebind_str_accepts_string_form() {
    let mut preferences = Preferences::default_preferences();
    preferences
        .rebind_str(ActionId::Save, "Ctrl+Alt+S")
        .unwrap();
    assert_eq!(
        preferences.shortcuts().binding_for(ActionId::Save),
        Some(combo("Ctrl+Alt+S"))
    );
    assert!(preferences.rebind_str(ActionId::Save, "!!!").is_err());
}

// ===========================================================================
// 写作目标 WritingGoal（V11-05）
// ===========================================================================

#[test]
fn writing_goal_zero_means_unset() {
    let goal = WritingGoal::new(0);
    assert!(
        !goal.is_set(),
        "0 = 未设置，是合法的「无目标」而非「目标 0 字」"
    );
    assert_eq!(goal.progress_percent(123), None, "未设目标没有进度百分比");
    assert!(!goal.is_reached(123), "没有目标就谈不上达成");
}

#[test]
fn writing_goal_saturates_above_max_instead_of_panicking() {
    let goal = WritingGoal::new(u32::MAX);
    assert_eq!(
        goal.words(),
        WritingGoal::MAX,
        "超过上限必须饱和到上限，而不是让设置保存失败"
    );
}

#[test]
fn writing_goal_progress_never_exceeds_100_percent() {
    let goal = WritingGoal::new(100);
    assert_eq!(goal.progress_percent(0), Some(0));
    assert_eq!(goal.progress_percent(50), Some(50));
    assert_eq!(goal.progress_percent(100), Some(100));
    assert_eq!(
        goal.progress_percent(137),
        Some(100),
        "超 100% 一律按 100 显示"
    );
    assert!(goal.is_reached(100));
    assert!(goal.is_reached(137));
}

#[test]
fn editor_settings_round_trip_through_toml_persists_writing_goal() {
    // 通过完整偏好聚合验证：写一份带目标的设置，应能经 TOML 往返带回。
    let mut preferences = Preferences::default_preferences();
    preferences
        .set_editor(EditorSettings::new(
            TabSize::Four,
            true,
            Some(30),
            false,
            false,
            400,
            0.5,
            Some(false),
            WritingGoal::new(1500),
            LintSettings::all_enabled(),
            12,
        ))
        .unwrap();
    // 目标值经由 `EditorSettings` 携带，落到 `Preferences` 里。
    assert_eq!(preferences.editor().writing_goal().words(), 1500);
}

/// 打开文档上限默认 12（问题 038）：低于常见「同时对照几份文档」的
/// 工作量会碍事，高于此值后标签条本就靠横向滚动，再大只是更难认。
#[test]
fn max_open_documents_defaults_to_twelve() {
    assert_eq!(
        EditorSettings::default_settings().max_open_documents(),
        12,
        "默认上限应为 12"
    );
}

/// 侧栏开合默认**收起**（问题 062），且可被 `with_sidebar_visible` 改写。
///
/// 默认值与 `AppUiState::new` 的初始值必须一致：两处都是「第一次打开时
/// 侧栏是收起的」，不一致的表现是启动瞬间闪一下侧栏再收起。
///
/// 问题 064 起默认值是**三态**的：`sidebar_visible()`（折叠成布尔）为
/// `false`，但 `sidebar_visible_decision()`（原样）是 `None`——「没决定过」
/// 与「决定过收起」必须可区分，前者允许打开文件夹时自动展开。
#[test]
fn sidebar_visibility_defaults_to_collapsed() {
    assert!(
        !EditorSettings::default_settings().sidebar_visible(),
        "默认侧栏应为收起（内容区优先）"
    );
    assert_eq!(
        EditorSettings::default_settings().sidebar_visible_decision(),
        None,
        "默认应是「没决定过」而不是「决定过收起」——否则打开文件夹时\n        永远等不到自动展开（问题 064）"
    );
    let opened = EditorSettings::default_settings().with_sidebar_visible(true);
    assert!(opened.sidebar_visible(), "改成展开后应读出展开");
    assert_eq!(
        opened.sidebar_visible_decision(),
        Some(true),
        "`with_sidebar_visible` 落成的必须是明确决定 Some(true)，不能留 None"
    );
    // 其余字段不受影响（值对象不可变，`with_*` 只动一个字段）。
    assert_eq!(
        opened.split_ratio(),
        EditorSettings::default_settings().split_ratio()
    );
    assert_eq!(
        opened.max_open_documents(),
        EditorSettings::default_settings().max_open_documents()
    );
}

/// 上限 0 是非法值（一个文档都打不开 = 应用不可用），必须被拒绝。
#[test]
fn max_open_documents_zero_is_invalid() {
    let zero = EditorSettings::new(
        TabSize::Four,
        true,
        None,
        false,
        false,
        400,
        0.5,
        Some(false),
        WritingGoal::new(0),
        LintSettings::all_enabled(),
        0,
    );
    assert!(
        !zero.is_valid(),
        "上限 0 必须判非法——否则「一个文档都打不开」会被当成合法配置"
    );
    let mut preferences = Preferences::default_preferences();
    assert!(
        preferences.set_editor(zero).is_err(),
        "非法的上限不得写进偏好"
    );
}
