//! `TomlPreferencesRepository` 单测（S5-06）。
//!
//! 重点是 ADR-003 的三条策略：**往返一致**、**损坏备份后回退默认**、
//! **未知字段忽略而非报错**。

use std::fs;
use std::path::PathBuf;

use rustmark::domain::preferences::aggregate::Preferences;
use rustmark::domain::preferences::repository::PreferencesRepository;
use rustmark::domain::preferences::value_objects::{EditorSettings, Theme, ThemeKind, WritingGoal};
use rustmark::infrastructure::persistence::toml_preferences_repository::TomlPreferencesRepository;

/// 测试用临时目录守卫。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let unique = format!(
            "rustmark-pref-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let path = std::env::temp_dir().join(unique.replace(['(', ')', ' '], ""));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("应能创建临时目录");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// 列出目录下的备份文件名。
    fn backups(&self) -> Vec<String> {
        fs::read_dir(&self.0)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| n.contains(".bak-"))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn repo_in(dir: &TempDir) -> TomlPreferencesRepository {
    TomlPreferencesRepository::new(dir.join("settings.toml"))
}

/// 构造一份**非默认**的偏好设置，以便验证往返确实带上了改动。
fn customized() -> Preferences {
    let theme = Theme::new(ThemeKind::Dark, "JetBrains Mono".to_string(), 16.0, 2.0);
    let editor = EditorSettings::new(
        rustmark::domain::preferences::value_objects::TabSize::Two,
        false,
        Some(60),
        true,
        false,
        true,
        800,
        0.5,
        Some(false),
        // 侧栏宽度（问题 067）：非默认值，往返必须原样带回。
        Some(420.0),
        WritingGoal::new(1200),
        rustmark::domain::preferences::value_objects::LintSettings::all_enabled(),
        9,
    );
    Preferences::new(theme, editor, Default::default()).expect("构造合法偏好设置应成功")
}

#[test]
fn missing_file_yields_defaults_without_error() {
    let dir = TempDir::new("missing");
    let repo = repo_in(&dir);

    let loaded = repo.load().expect("首次启动读不到文件不应报错");
    assert_eq!(
        loaded,
        Preferences::default_preferences(),
        "文件不存在必须返回默认配置"
    );
}

#[test]
fn save_then_load_roundtrips_exactly() {
    let dir = TempDir::new("roundtrip");
    let repo = repo_in(&dir);
    let original = customized();

    repo.save(&original).expect("保存应成功");
    let loaded = repo.load().expect("加载应成功");

    assert_eq!(loaded, original, "往返必须完全一致");
    assert_eq!(loaded.theme().font_size(), 16.0);
    assert_eq!(loaded.theme().font_family(), "JetBrains Mono");
    assert!(loaded.editor().spell_check(), "布尔值应被保留");
    assert_eq!(loaded.editor().auto_save_interval(), Some(60));
    assert_eq!(
        loaded.editor().sidebar_width(),
        Some(420.0),
        "侧栏宽度应被保留（问题 067）"
    );
}

#[test]
fn save_creates_parent_directory_and_leaves_no_temp_file() {
    let dir = TempDir::new("mkdir");
    let repo = TomlPreferencesRepository::new(dir.join("config/nested/settings.toml"));

    repo.save(&Preferences::default_preferences())
        .expect("应自动创建父目录");
    assert!(repo.path().exists());

    let leftovers: Vec<String> = fs::read_dir(repo.path().parent().unwrap())
        .map(|e| {
            e.filter_map(|x| x.ok())
                .map(|x| x.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with(".rustmark-tmp-"))
                .collect()
        })
        .unwrap_or_default();
    assert!(leftovers.is_empty(), "不应残留临时文件：{leftovers:?}");
}

#[test]
fn corrupt_file_falls_back_to_defaults_and_keeps_backup() {
    let dir = TempDir::new("corrupt");
    let repo = repo_in(&dir);
    fs::write(repo.path(), "这不是合法 TOML [[[").expect("应能写损坏内容");

    let loaded = repo.load().expect("损坏文件不得导致加载失败");
    assert_eq!(
        loaded,
        Preferences::default_preferences(),
        "按 ADR-003 应回退默认配置"
    );
    assert_eq!(dir.backups().len(), 1, "损坏文件必须被备份以便排查");
}

#[test]
fn empty_file_is_treated_as_defaults() {
    let dir = TempDir::new("empty");
    let repo = repo_in(&dir);
    fs::write(repo.path(), "  \n\t ").expect("应能写空白内容");
    assert_eq!(
        repo.load().expect("空白文件应按默认处理"),
        Preferences::default_preferences()
    );
}

#[test]
fn unknown_fields_are_ignored_not_rejected() {
    let dir = TempDir::new("unknown");
    let repo = repo_in(&dir);
    // 先写一份合法配置，再手工追加一个「未来版本才有的」顶层字段。
    repo.save(&customized()).expect("保存应成功");
    let mut raw = fs::read_to_string(repo.path()).expect("应能读回");
    raw.push_str("\nfuture_option = \"from a newer version\"\n");
    fs::write(repo.path(), raw).expect("应能写回");

    let loaded = repo
        .load()
        .expect("未知字段应被忽略，不得报错（旧版读新配置是常态）");
    assert_eq!(loaded.theme().font_size(), 16.0, "已知字段应照常读出");
    assert!(dir.backups().is_empty(), "未知字段不构成损坏，不应产生备份");
}

#[test]
fn out_of_range_values_are_rejected_and_replaced_by_defaults() {
    let dir = TempDir::new("range");
    let repo = repo_in(&dir);
    repo.save(&customized()).expect("保存应成功");

    // 手工把字号改成非法值：反序列化能成功，但取值校验必须拦下。
    let raw = fs::read_to_string(repo.path()).expect("应能读回");
    let corrupted = raw
        .replace("font_size = 16.0", "font_size = 999.0")
        .replace("font_size = 16", "font_size = 999");
    fs::write(repo.path(), corrupted).expect("应能写回");

    let loaded = repo.load().expect("取值非法不应导致加载失败");
    assert_eq!(
        loaded,
        Preferences::default_preferences(),
        "非法取值必须回退默认"
    );
    assert_eq!(dir.backups().len(), 1, "非法配置应被备份");
}

#[test]
fn reset_restores_defaults_and_persists_them() {
    let dir = TempDir::new("reset");
    let repo = repo_in(&dir);
    repo.save(&customized()).expect("保存应成功");

    let defaults = repo.reset().expect("恢复默认应成功");
    assert_eq!(defaults, Preferences::default_preferences());
    // 重写而非删除：文件应仍存在且内容为默认值。
    assert!(repo.path().exists(), "恢复默认应重写配置而非删除文件");
    assert_eq!(
        repo.load().expect("重新加载应成功"),
        Preferences::default_preferences()
    );
}

#[test]
fn save_refuses_invalid_preferences_without_touching_disk() {
    let dir = TempDir::new("invalid-save");
    let repo = repo_in(&dir);
    repo.save(&customized()).expect("先写入合法配置");
    let before = fs::read_to_string(repo.path()).expect("应能读回");

    // 构造非法偏好：字号 999 超出 8–48 区间。
    // `Preferences::new` 会拒绝它，故这里直接构造再 `save`，
    // 验证仓储层的写前校验是**最后一道防线**（不依赖构造器已校验）。
    let mut prefs = customized();
    let bad_theme = Theme::new(ThemeKind::Light, "X".to_string(), 999.0, 1.5);
    let _ = prefs.set_theme(bad_theme);

    // 若聚合允许写入非法值，仓储必须拒绝。
    let result = repo.save(&prefs);
    if prefs.validate().is_err() {
        assert!(result.is_err(), "非法配置不得写入磁盘");
        assert_eq!(
            fs::read_to_string(repo.path()).unwrap_or_default(),
            before,
            "拒绝写入时原文件必须保持不变"
        );
    } else {
        // 聚合认为合法（说明 `set_theme` 做了校验），则保存成功亦可。
        assert!(result.is_ok());
    }
}

#[test]
fn concurrent_style_repeated_saves_keep_file_valid() {
    let dir = TempDir::new("repeat-save");
    let repo = repo_in(&dir);
    // 反复覆盖写：原子写保证任一时刻磁盘上的文件都是完整可解析的。
    for i in 0..20 {
        let mut prefs = customized();
        let _ = prefs.set_editor(EditorSettings::new(
            rustmark::domain::preferences::value_objects::TabSize::Two,
            i % 2 == 0,
            Some(30 + i),
            false,
            false,
            true,
            400,
            0.5,
            Some(false),
            None,
            WritingGoal::new(0),
            rustmark::domain::preferences::value_objects::LintSettings::all_enabled(),
            7,
        ));
        repo.save(&prefs).expect("反复保存应始终成功");
    }
    assert!(repo.load().is_ok(), "最后一次写入的文件必须可解析");
    assert!(dir.backups().is_empty(), "正常保存不应产生备份");
}

// ============================================================================
// 问题 062 · 侧栏开合状态的持久化
// ============================================================================

/// **旧配置文件**（没有 `sidebar_visible` 字段）必须按默认「收起」读出，
/// 而不是让整个配置加载失败——后者会连主题与缩进一起打回默认。
///
/// 做法：先存一份当前版本的配置，再把 `sidebar_visible` 那行删掉，
/// 模拟老版本写下的文件（不手写整份 TOML，格式变了这条也不会假绿）。
#[test]
fn preferences_without_sidebar_visible_load_as_collapsed() {
    let dir = TempDir::new("sidebar-legacy");
    let repo = repo_in(&dir);

    repo.save(&Preferences::default_preferences())
        .expect("保存应成功");
    let path = dir.join("settings.toml");
    let text = fs::read_to_string(&path).expect("配置应可读");
    let stripped: String = text
        .lines()
        .filter(|line| !line.contains("sidebar_visible"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(stripped, text, "应确实删掉了 sidebar_visible 那一行");
    fs::write(&path, stripped).expect("回写应成功");

    let loaded = repo.load().expect("旧配置应能加载（缺字段不报错）");
    assert!(
        !loaded.editor().sidebar_visible(),
        "缺字段应按默认（收起）读出，而不是加载失败"
    );
    assert_eq!(
        loaded.editor().sidebar_visible_decision(),
        None,
        "缺字段应读出「没决定过」而不是 Some(false)——后者会把用户\n        锁死成「手动收起」，打开文件夹时再也不自动展开（问题 064）"
    );
    assert!(
        dir.backups().is_empty(),
        "缺字段不该触发「备份 + 回退默认」——那会丢掉其余设置"
    );
}

/// **旧配置文件**（没有 `sidebar_width` 字段）必须按「没拖过」读出
/// （`None` = 跟主题默认宽度），而不是加载失败或锁死成一个具体宽度
/// （问题 067；与上一条 `sidebar_visible` 同源）。
#[test]
fn preferences_without_sidebar_width_load_as_theme_default() {
    let dir = TempDir::new("sidebar-width-legacy");
    let repo = repo_in(&dir);

    // 先存一份**带**宽度的配置，再把那一行删掉，模拟老版本写下的文件。
    let mut prefs = Preferences::default_preferences();
    prefs
        .set_editor(
            EditorSettings::default_settings()
                .with_sidebar_visible(true)
                .with_sidebar_width(420.0),
        )
        .expect("合法设置应可写入");
    repo.save(&prefs).expect("保存应成功");
    let path = dir.join("settings.toml");
    let text = fs::read_to_string(&path).expect("配置应可读");
    let stripped: String = text
        .lines()
        .filter(|line| !line.contains("sidebar_width"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(stripped, text, "应确实删掉了 sidebar_width 那一行");
    fs::write(&path, stripped).expect("回写应成功");

    let loaded = repo.load().expect("旧配置应能加载（缺字段不报错）");
    assert_eq!(
        loaded.editor().sidebar_width(),
        None,
        "缺字段应按「没拖过」读出，而不是 Some(某个宽度)——\n        那会把用户锁死在一个他从未选过的宽度上"
    );
    assert!(
        dir.backups().is_empty(),
        "缺字段不该触发「备份 + 回退默认」——那会丢掉其余设置"
    );
}

/// 旧配置缺 `auto_save_interval` 字段时按默认「30 秒」读出，而不是让整份
/// 配置反序列化失败、触发「备份 + 回退默认」（问题 068）。
///
/// 该字段此前没有 serde 缺省：TOML 对「缺字段」直接报错，仓储的处理是
/// 备份原文件 + 整体回退默认——用户其余全部设置一起作废。
#[test]
fn preferences_without_auto_save_interval_load_with_the_default_interval() {
    let dir = TempDir::new("auto-save-legacy");
    let repo = repo_in(&dir);

    // 先存一份默认配置（含 auto_save_interval = 30），再把那一行删掉，
    // 模拟旧版本写下的文件 / 被外部工具弄丢一行的文件。
    let prefs = Preferences::default_preferences();
    repo.save(&prefs).expect("保存应成功");
    let path = dir.join("settings.toml");
    let text = fs::read_to_string(&path).expect("配置应可读");
    let stripped: String = text
        .lines()
        .filter(|line| !line.contains("auto_save_interval"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(stripped, text, "应确实删掉了 auto_save_interval 那一行");
    fs::write(&path, stripped).expect("回写应成功");

    let loaded = repo.load().expect("旧配置应能加载（缺字段不报错）");
    assert_eq!(
        loaded.editor().auto_save_interval(),
        Some(30),
        "缺字段应按默认 30 秒读出——自动保存默认开启"
    );
    assert!(
        dir.backups().is_empty(),
        "缺字段不该触发「备份 + 回退默认」——那会丢掉其余设置"
    );
}

// ============================================================================
// V11-08 · 格式检查（lint）设置的持久化
// ============================================================================

/// 关掉的规则要能原样存盘再读回——否则用户每次重启都要重关一遍。
#[test]
fn lint_rule_choices_survive_a_toml_round_trip() {
    use rustmark::domain::preferences::value_objects::{LintSettings, TabSize};

    let dir = TempDir::new("lint-roundtrip");
    let repo = repo_in(&dir);

    let theme = Theme::new(ThemeKind::Dark, "JetBrains Mono".to_string(), 16.0, 2.0);
    let editor = EditorSettings::new(
        TabSize::Four,
        true,
        None,
        false,
        true,
        true,
        400,
        0.5,
        Some(false),
        None,
        WritingGoal::new(0),
        LintSettings {
            heading_jump: false,
            list_marker_mix: true,
            trailing_whitespace: false,
            unclosed_fence: true,
        },
        5,
    );
    let prefs = Preferences::new(theme, editor, Default::default()).expect("构造合法偏好");

    repo.save(&prefs).expect("保存应成功");
    let loaded = repo.load().expect("加载应成功");
    assert!(
        !loaded.editor().lint().heading_jump,
        "关掉的规则必须原样读回"
    );
    assert!(loaded.editor().lint().list_marker_mix, "开着的规则必须保留");
    assert!(!loaded.editor().lint().trailing_whitespace);
    assert!(loaded.editor().lint().unclosed_fence);
}

/// 旧配置文件里没有 lint 字段 → 必须按「全部启用」读出，而不是让加载失败。
#[test]
fn an_old_config_without_lint_fields_loads_them_all_enabled() {
    use rustmark::domain::preferences::value_objects::LintSettings;

    let dir = TempDir::new("lint-backcompat");
    let repo = repo_in(&dir);
    repo.save(&Preferences::default_preferences())
        .expect("先写一份默认配置");
    let text = fs::read_to_string(repo.path()).expect("应能读回默认配置");
    // 抹掉 lint 相关行，模拟「升级前写下的旧配置」。
    let stripped: String = text
        .lines()
        .filter(|l| {
            !l.contains("heading_jump")
                && !l.contains("list_marker_mix")
                && !l.contains("trailing_whitespace")
                && !l.contains("unclosed_fence")
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(repo.path(), stripped).expect("写回旧格式配置");

    let loaded = repo.load().expect("旧配置必须能加载");
    assert_eq!(
        loaded.editor().lint(),
        LintSettings::all_enabled(),
        "缺 lint 字段时必须按全部启用读出"
    );
}

// ============================================================================
// V11-09 · 自定义主题语法色覆盖的持久化
// ============================================================================

/// 构造一份「自定义主题 + 指定语法覆盖」的偏好。
fn prefs_with_custom_syntax(syn_heading: Option<&str>) -> Preferences {
    use rustmark::domain::preferences::value_objects::{CustomThemePalette, ThemeKind};

    let palette = CustomThemePalette {
        syn_heading: syn_heading.map(str::to_string),
        ..Default::default()
    };
    let mut theme = Theme::new(ThemeKind::Custom, "monospace".to_string(), 14.0, 1.6);
    theme.custom = Some(palette);
    let editor = EditorSettings::new(
        rustmark::domain::preferences::value_objects::TabSize::Four,
        true,
        None,
        false,
        true,
        true,
        400,
        0.5,
        Some(false),
        None,
        WritingGoal::new(0),
        rustmark::domain::preferences::value_objects::LintSettings::all_enabled(),
        3,
    );
    Preferences::new(theme, editor, Default::default()).expect("构造合法偏好")
}

/// 语法色覆盖要能存盘再读回。
#[test]
fn custom_theme_syntax_override_survives_a_round_trip() {
    let dir = TempDir::new("syn-roundtrip");
    let repo = repo_in(&dir);
    let prefs = prefs_with_custom_syntax(Some("#123456"));

    repo.save(&prefs).expect("保存应成功");
    let loaded = repo.load().expect("加载应成功");
    let palette = loaded
        .theme()
        .custom
        .clone()
        .expect("自定义主题应带回调色板");
    assert_eq!(palette.syn_heading.as_deref(), Some("#123456"));
    assert_eq!(palette.syn_link, None, "未覆盖的语法色应保持 None");
}

/// 旧自定义主题文件里没有 `syn_*` 字段 → 必须读作 `None`（跟随主题），不得加载失败。
#[test]
fn an_old_custom_theme_without_syntax_fields_loads_as_following_theme() {
    let dir = TempDir::new("syn-backcompat");
    let repo = repo_in(&dir);
    repo.save(&prefs_with_custom_syntax(None))
        .expect("先写一份自定义主题偏好");
    let text = fs::read_to_string(repo.path()).expect("应能读回配置");
    // 抹掉所有 syn_* 行，模拟「升级前写下的旧自定义主题」。
    let stripped: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("syn_"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(repo.path(), stripped).expect("写回旧格式配置");

    let loaded = repo.load().expect("旧配置必须能加载");
    let palette = loaded
        .theme()
        .custom
        .clone()
        .expect("旧自定义主题应仍带调色板");
    assert_eq!(
        palette.syn_heading, None,
        "缺省语法色应读作 None（跟随主题）"
    );
    assert_eq!(palette.syn_table, None);
}

// ============================================================================
// 问题 038 · 打开文档上限的持久化
// ============================================================================

/// 手改配置写成 0 时，**收敛到 1** 而不是让整个配置作废。
///
/// `load()` 对校验失败的处理是「备份 + 整体回退默认」——为单个字段的 0
/// 丢掉用户全部设置是不可接受的，故收敛必须发生在 serde 入口。本用例
/// 同时验证：没有产生备份、其余字段原样保留。
#[test]
fn a_hand_edited_zero_max_open_documents_is_clamped_on_load() {
    let dir = TempDir::new("open-limit-zero");
    let repo = repo_in(&dir);
    repo.save(&customized()).expect("先写一份合法配置");

    let raw = fs::read_to_string(repo.path()).expect("应能读回");
    let hand_edited = raw.replace("max_open_documents = 9", "max_open_documents = 0");
    assert_ne!(hand_edited, raw, "替换应真的发生（夹具里的上限是 9）");
    fs::write(repo.path(), hand_edited).expect("写回手改后的配置");

    let loaded = repo.load().expect("手改 0 不得导致加载失败");
    assert_eq!(
        loaded.editor().max_open_documents(),
        1,
        "手改的 0 应收敛到 1"
    );
    assert_eq!(loaded.theme().font_size(), 16.0, "其余字段必须原样保留");
    assert_eq!(loaded.editor().auto_save_interval(), Some(60));
    assert!(dir.backups().is_empty(), "0 已收敛，不应触发备份+回退默认");
}

/// 旧配置缺 `max_open_documents` 字段时按默认值读出（不报错、不备份）。
#[test]
fn an_old_config_without_max_open_documents_uses_the_default() {
    let dir = TempDir::new("open-limit-missing");
    let repo = repo_in(&dir);
    repo.save(&customized()).expect("先写一份合法配置");

    let raw = fs::read_to_string(repo.path()).expect("应能读回");
    let stripped: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("max_open_documents"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(repo.path(), stripped).expect("写回旧格式配置");

    let loaded = repo.load().expect("旧配置必须能加载");
    assert_eq!(
        loaded.editor().max_open_documents(),
        12,
        "缺字段必须按默认值 12 读出"
    );
    assert_eq!(loaded.theme().font_size(), 16.0, "其余字段应照常读出");
    assert!(dir.backups().is_empty(), "缺字段不构成损坏，不应产生备份");
}

/// 旧配置缺 `load_remote_images` 字段时按「加载」读出（PRIV-01）。
///
/// 隐私开关的缺省值必须**向后兼容**：升级不该静默改变已有文档的呈现。
/// 反过来（缺字段读成 `false`）会让所有老用户升级后突然看不到远程图，
/// 而界面上没有任何解释。
#[test]
fn an_old_config_without_load_remote_images_keeps_loading() {
    let dir = TempDir::new("remote-images-missing");
    let repo = repo_in(&dir);
    repo.save(&customized()).expect("先写一份合法配置");

    let raw = fs::read_to_string(repo.path()).expect("应能读回");
    let stripped: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with("load_remote_images"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(stripped, raw, "应确实删掉了 load_remote_images 那一行");
    fs::write(repo.path(), stripped).expect("写回旧格式配置");

    let loaded = repo.load().expect("旧配置必须能加载");
    assert!(
        loaded.editor().load_remote_images(),
        "缺字段必须按「加载」读出——否则升级本身会静默改变已有文档的呈现"
    );
    assert_eq!(loaded.theme().font_size(), 16.0, "其余字段应照常读出");
    assert!(dir.backups().is_empty(), "缺字段不构成损坏，不应产生备份");
}

/// 关掉开关必须**写进配置文件**（不只是内存）——隐私承诺跨会话成立。
#[test]
fn load_remote_images_false_survives_a_toml_round_trip() {
    let dir = TempDir::new("remote-images-off");
    let repo = repo_in(&dir);

    let mut prefs = customized();
    let editor = EditorSettings::new(
        rustmark::domain::preferences::value_objects::TabSize::Two,
        false,
        Some(60),
        true,
        false,
        // 唯一被本条关心的一项。
        false,
        800,
        0.5,
        Some(false),
        Some(420.0),
        WritingGoal::new(1200),
        rustmark::domain::preferences::value_objects::LintSettings::all_enabled(),
        9,
    );
    prefs.set_editor(editor).expect("构造合法偏好设置应成功");
    repo.save(&prefs).expect("保存应成功");

    let raw = fs::read_to_string(repo.path()).expect("应能读回");
    assert!(
        raw.contains("load_remote_images = false"),
        "开关必须落盘（否则重启后自动恢复联网）：{raw}"
    );
    let loaded = repo.load().expect("应能加载");
    assert!(!loaded.editor().load_remote_images(), "往返后必须仍是关闭");
}
