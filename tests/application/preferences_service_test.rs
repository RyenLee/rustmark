//! Preferences 用例服务测试（S4-08）。
//!
//! 覆盖：读取设置（三处共用同一份）、改主题 / 编辑器（非法值返回 Err 且不落库）、
//! 改键（阻断级冲突返回 Err 且不提交）、冲突报告、恢复出厂。

use std::sync::Arc;

use rustmark::application::dto::{
    ConflictView, CustomThemePaletteView, EditorSettingsView, SettingsView, ThemeView,
};
use rustmark::application::preferences::commands::*;
use rustmark::application::preferences::queries::*;
use rustmark::application::preferences::service::PreferencesAppService;
use rustmark::domain::preferences::aggregate::Preferences;
use rustmark::domain::preferences::value_objects::{ActionId, CustomThemePalette, KeyCombo};

use super::fakes::*;

fn build() -> (PreferencesAppService, Arc<InMemoryPreferencesRepository>) {
    let repo = Arc::new(InMemoryPreferencesRepository::new());
    let bus: Arc<dyn rustmark::shared::event::EventBus> = Arc::new(InMemoryEventBus::new());
    let svc = PreferencesAppService::new(repo.clone(), bus).unwrap();
    (svc, repo)
}

fn shortcut_for(settings: &SettingsView, action: &str) -> Option<String> {
    settings
        .shortcuts
        .iter()
        .find(|s| s.action == action)
        .map(|s| s.combo.clone())
}

#[test]
fn get_settings_returns_defaults() {
    let (svc, _repo) = build();
    let settings: SettingsView = svc.get_settings(GetSettingsQuery).unwrap();
    assert_eq!(settings.theme.kind, "system");
    assert_eq!(settings.editor.tab_size, 4);
    assert!(!settings.shortcuts.is_empty());
}

#[test]
fn set_theme_valid_persists() {
    let (svc, repo) = build();
    svc.set_theme(SetThemeCommand {
        theme: ThemeView {
            kind: "dark".to_string(),
            font_family: "serif".to_string(),
            font_size: 16.0,
            line_height: 1.5,
            custom: None,
        },
    })
    .unwrap();
    let settings: SettingsView = svc.get_settings(GetSettingsQuery).unwrap();
    assert_eq!(settings.theme.kind, "dark");
    assert!(repo.is_persisted(), "合法设置必须落盘");
}

#[test]
fn set_theme_invalid_font_size_fails_without_persist() {
    let (svc, repo) = build();
    let res = svc.set_theme(SetThemeCommand {
        theme: ThemeView {
            kind: "dark".to_string(),
            font_family: "serif".to_string(),
            font_size: 100.0, // 超出 8–48 合法区间
            line_height: 1.5,
            custom: None,
        },
    });
    assert!(res.is_err(), "非法字号必须返回 Err");
    assert!(!repo.is_persisted(), "非法设置不得落盘");
}

#[test]
fn set_editor_valid_persists() {
    let (svc, _repo) = build();
    svc.set_editor(SetEditorCommand {
        editor: EditorSettingsView {
            tab_size: 2,
            word_wrap: false,
            auto_save_interval: Some(10),
            spell_check: true,
            show_line_numbers: false,
            load_remote_images: false,
            split_ratio: 0.5,
            sidebar_visible: Some(false),
            sidebar_width: None,
            writing_goal: 500,
            lint: Default::default(),
            max_open_documents: 12,
        },
    })
    .unwrap();
    let settings: SettingsView = svc.get_settings(GetSettingsQuery).unwrap();
    assert_eq!(settings.editor.tab_size, 2);
    assert_eq!(settings.editor.writing_goal, 500, "写作目标应随设置持久化");
    assert!(
        !settings.editor.load_remote_images,
        "远程图片开关必须经 DTO → 领域 → DTO 往返（PRIV-01）：只写一半会\n         让用户关掉开关、重启后又开始联网"
    );
}

#[test]
fn set_editor_goal_beyond_max_saturates_instead_of_failing() {
    let (svc, _repo) = build();
    svc.set_editor(SetEditorCommand {
        editor: EditorSettingsView {
            tab_size: 4,
            word_wrap: true,
            auto_save_interval: None,
            spell_check: false,
            show_line_numbers: false,
            load_remote_images: true,
            split_ratio: 0.5,
            sidebar_visible: Some(false),
            sidebar_width: None,
            // 远超领域上限：必须饱和到 1_000_000，而不是让保存失败（V11-05）。
            writing_goal: u32::MAX,
            lint: Default::default(),
            max_open_documents: 12,
        },
    })
    .unwrap();
    let settings: SettingsView = svc.get_settings(GetSettingsQuery).unwrap();
    assert_eq!(
        settings.editor.writing_goal,
        rustmark::domain::preferences::value_objects::WritingGoal::MAX
    );
}

#[test]
fn set_editor_invalid_tab_size_fails() {
    let (svc, _repo) = build();
    let res = svc.set_editor(SetEditorCommand {
        editor: EditorSettingsView {
            tab_size: 3, // 非 {2,4,8}
            word_wrap: false,
            auto_save_interval: Some(10),
            spell_check: true,
            show_line_numbers: false,
            load_remote_images: true,
            split_ratio: 0.5,
            sidebar_visible: Some(false),
            sidebar_width: None,
            writing_goal: 0,
            lint: Default::default(),
            max_open_documents: 12,
        },
    });
    assert!(res.is_err());
}

#[test]
fn set_shortcut_valid_rebinds_and_persists() {
    let (svc, repo) = build();
    svc.set_shortcut(SetShortcutCommand {
        action: ActionId::Bold,
        combo: "Ctrl+X".to_string(),
    })
    .unwrap();
    let settings: SettingsView = svc.get_settings(GetSettingsQuery).unwrap();
    assert_eq!(shortcut_for(&settings, "加粗").as_deref(), Some("Ctrl+X"));
    assert!(repo.is_persisted());
}

#[test]
fn set_shortcut_duplicate_conflict_is_rejected() {
    let (svc, _repo) = build();
    // 把「打开文档」绑定到 Ctrl+S，与默认的「保存」重复 → 阻断级冲突。
    let res = svc.set_shortcut(SetShortcutCommand {
        action: ActionId::OpenDocument,
        combo: "Ctrl+S".to_string(),
    });
    assert!(res.is_err(), "阻断级冲突必须返回 Err 且不提交");
    // 绑定未被改写：打开文档仍为默认 Ctrl+O。
    let settings: SettingsView = svc.get_settings(GetSettingsQuery).unwrap();
    assert_eq!(
        shortcut_for(&settings, "打开文档").as_deref(),
        Some("Ctrl+O"),
        "冲突时不得提交改键"
    );
}

#[test]
fn get_conflicts_reports_duplicate() {
    let mut prefs = Preferences::default_preferences();
    prefs.rebind(ActionId::Bold, KeyCombo::parse("Ctrl+S").unwrap());
    let repo = Arc::new(InMemoryPreferencesRepository::new());
    let bus: Arc<dyn rustmark::shared::event::EventBus> = Arc::new(InMemoryEventBus::new());
    let svc = PreferencesAppService::with_preferences(repo, bus, prefs);

    let conflicts: Vec<ConflictView> = svc.get_conflicts(GetConflictsQuery).unwrap();
    assert!(
        conflicts.iter().any(|c| c.combo == "Ctrl+S"),
        "应报告 Ctrl+S 的重复冲突"
    );
}

#[test]
fn reset_returns_to_default() {
    let (svc, _repo) = build();
    svc.set_theme(SetThemeCommand {
        theme: ThemeView {
            kind: "dark".to_string(),
            font_family: "serif".to_string(),
            font_size: 16.0,
            line_height: 1.5,
            custom: None,
        },
    })
    .unwrap();
    svc.reset(ResetPreferencesCommand).unwrap();
    let settings: SettingsView = svc.get_settings(GetSettingsQuery).unwrap();
    assert_eq!(settings.theme.kind, "system");
}

#[test]
fn set_theme_custom_roundtrips_palette() {
    let (svc, _repo) = build();
    let palette = CustomThemePaletteView {
        bg_primary: "#0A0A0A".to_string(),
        bg_secondary: "#1A1A1A".to_string(),
        text_primary: "#EEEEEE".to_string(),
        text_secondary: "#AAAAAA".to_string(),
        accent: "#33AAFF".to_string(),
        code_bg: "#222222".to_string(),
        border: "#333333".to_string(),
        selection_bg: "#44BBFF".to_string(),
        // V11-09：语法色是可选的，这里只给一项覆盖，其余跟随主题。
        syn_heading: Some("#FF8800".to_string()),
        ..Default::default()
    };
    svc.set_theme(SetThemeCommand {
        theme: ThemeView {
            kind: "custom".to_string(),
            font_family: "monospace".to_string(),
            font_size: 14.0,
            line_height: 1.6,
            custom: Some(palette.clone()),
        },
    })
    .unwrap();
    let settings = svc.get_settings(GetSettingsQuery).unwrap();
    assert_eq!(settings.theme.kind, "custom");
    let got = settings.theme.custom.expect("回读应带调色板");
    assert_eq!(got.bg_primary, "#0A0A0A");
    assert_eq!(got.accent, "#33AAFF");
    assert_eq!(
        got.syn_heading.as_deref(),
        Some("#FF8800"),
        "语法色覆盖应原样回读"
    );
    assert_eq!(got.syn_link, None, "未覆盖的语法色应保持 None（跟随主题）");
}

/// DTO 的默认调色板**派生自**领域默认，不是另抄一份。
///
/// 两边各写 8 个 hex 字面量时，「改一处忘另一处」会让「首次切到自定义」
/// 拿到的配色取决于走哪条路径（领域聚合补齐的 vs 视图默认构造的）——
/// 差几个色号肉眼几乎看不出来，是最难排查的那种不一致。
///
/// 断言的是「两边逐字段相等」而不是具体色号：色号可以改，双源不行。
#[test]
fn dto_default_palette_is_derived_from_the_domain_default() {
    assert_eq!(
        CustomThemePaletteView::default(),
        CustomThemePaletteView::from(CustomThemePalette::default()),
        "DTO 默认调色板必须与领域默认逐字段一致（不得各抄一份）"
    );
}

#[test]
fn set_theme_custom_without_palette_still_defaults() {
    let (svc, _repo) = build();
    // 选 custom 但没给调色板：不应崩溃，回读 kind 为 custom 且带默认调色板。
    svc.set_theme(SetThemeCommand {
        theme: ThemeView {
            kind: "custom".to_string(),
            font_family: "monospace".to_string(),
            font_size: 14.0,
            line_height: 1.6,
            custom: None,
        },
    })
    .unwrap();
    let settings = svc.get_settings(GetSettingsQuery).unwrap();
    assert_eq!(settings.theme.kind, "custom");
    let got = settings.theme.custom.expect("默认调色板应被补齐");
    assert_eq!(got.bg_primary, "#14161B");
}

#[test]
fn set_theme_light_keeps_custom_palette_through_to_theme() {
    // #039 回归：从 custom 切到 light/dark 时，`to_theme` 不得静默丢弃自定义调色板。
    // 旧实现只在 `kind == Custom` 时附上调色板，于是带调色板的 light 主题经
    // `set_theme → to_theme` 重建领域对象后调色板被清空，等同于无声改了用户设置。
    // 这里带调色板地写入 light，再经 `get_settings → to_theme_view` 回读，
    // 调色板必须仍在。
    let (svc, _repo) = build();
    let palette = CustomThemePaletteView {
        bg_primary: "#0A0A0A".to_string(),
        bg_secondary: "#1A1A1A".to_string(),
        text_primary: "#EEEEEE".to_string(),
        text_secondary: "#AAAAAA".to_string(),
        accent: "#33AAFF".to_string(),
        code_bg: "#222222".to_string(),
        border: "#333333".to_string(),
        selection_bg: "#44BBFF".to_string(),
        ..Default::default()
    };
    svc.set_theme(SetThemeCommand {
        theme: ThemeView {
            kind: "light".to_string(),
            font_family: "monospace".to_string(),
            font_size: 14.0,
            line_height: 1.6,
            custom: Some(palette.clone()),
        },
    })
    .unwrap();
    let settings = svc.get_settings(GetSettingsQuery).unwrap();
    assert_eq!(settings.theme.kind, "light");
    let got = settings
        .theme
        .custom
        .expect("切到 light 后自定义调色板不应被丢弃");
    assert_eq!(got.accent, "#33AAFF");
}

/// 兜底设置必须与默认设置对齐自动保存间隔（问题 068）。
///
/// `fallback()` 供「运行期读配置失败」时渲染兜底界面使用。旧值 `None`
/// 等于「关闭自动保存」——读失败本只该意味着「读不到用户的定制」，
/// 不该把崩溃恢复保险一并关掉。
#[test]
fn fallback_settings_keep_autosave_enabled() {
    let view = SettingsView::fallback();
    assert_eq!(
        view.editor.auto_save_interval,
        Some(30),
        "兜底设置的自动保存间隔应与默认设置（30 秒）一致"
    );
}
