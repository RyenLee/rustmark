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
        800,
        0.5,
        WritingGoal::new(1200),
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
            400,
            0.5,
            WritingGoal::new(0),
        ));
        repo.save(&prefs).expect("反复保存应始终成功");
    }
    assert!(repo.load().is_ok(), "最后一次写入的文件必须可解析");
    assert!(dir.backups().is_empty(), "正常保存不应产生备份");
}
