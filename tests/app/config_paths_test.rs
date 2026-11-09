//! ADR-014：旧平台配置目录 → 应用目录 `data/` 的迁移契约。
//!
//! 迁移是升级路径上最容易丢用户数据的一环：草稿、设置、最近文件、崩溃报告
//! 都在旧目录里。这里的用例把三条底线钉死——**不删源、不覆盖目标、可重复执行**。

use std::path::Path;

use rustmark::app::bootstrap::migrate_between;

const MARKER: &str = ".rustmark-migrated";

/// 造一份「旧版本留下的完整配置目录」，返回根目录。
fn legacy_tree(root: &Path) {
    let base = root.join("rustmark");
    std::fs::create_dir_all(base.join(".rustmark-draft")).unwrap();
    std::fs::create_dir_all(base.join("crash")).unwrap();
    std::fs::write(base.join("preferences.toml"), "schema_version = 1").unwrap();
    std::fs::write(base.join("recent.json"), "recent-from-old-version").unwrap();
    std::fs::write(
        base.join(".rustmark-draft").join("draft-1.txt"),
        "未保存的正文",
    )
    .unwrap();
    std::fs::write(base.join("crash").join("crash-1.txt"), "旧崩溃报告").unwrap();
}

#[test]
fn migration_copies_every_kind_of_data_and_leaves_the_source_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let legacy = tmp.path().join("legacy");
    let portable = tmp.path().join("portable");
    legacy_tree(&legacy);
    std::fs::create_dir_all(&portable).unwrap();

    let copied = migrate_between(&legacy.join("rustmark"), &portable);
    assert_eq!(copied, 4, "两个普通文件 + 一个草稿 + 一份崩溃报告");

    let target = portable;
    assert_eq!(
        std::fs::read_to_string(target.join("preferences.toml")).unwrap(),
        "schema_version = 1"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("recent.json")).unwrap(),
        "recent-from-old-version"
    );
    assert_eq!(
        std::fs::read_to_string(target.join(".rustmark-draft/draft-1.txt")).unwrap(),
        "未保存的正文"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("crash/crash-1.txt")).unwrap(),
        "旧崩溃报告"
    );

    // 底线一：源文件一个都不能少。旧目录是用户既有数据，删除不可接受。
    assert!(legacy.join("rustmark/preferences.toml").exists());
    assert!(legacy.join("rustmark/.rustmark-draft/draft-1.txt").exists());
    assert!(legacy.join("rustmark/crash/crash-1.txt").exists());
    assert!(target.join(MARKER).exists(), "成功后必须留下迁移标记");
}

#[test]
fn migration_never_overwrites_a_newer_target_file() {
    let tmp = tempfile::tempdir().unwrap();
    let legacy = tmp.path().join("legacy/rustmark");
    let portable = tmp.path().join("portable");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::create_dir_all(&portable).unwrap();
    std::fs::write(legacy.join("preferences.toml"), "旧配置").unwrap();
    std::fs::write(portable.join("preferences.toml"), "用户改过的新配置").unwrap();

    let copied = migrate_between(&legacy, &portable);
    assert_eq!(copied, 0, "目标已存在就必须跳过");
    assert_eq!(
        std::fs::read_to_string(portable.join("preferences.toml")).unwrap(),
        "用户改过的新配置"
    );
}

#[test]
fn migration_is_idempotent_and_does_not_resurrect_a_deleted_config() {
    let tmp = tempfile::tempdir().unwrap();
    let legacy = tmp.path().join("legacy/rustmark");
    let portable = tmp.path().join("portable");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("preferences.toml"), "旧配置").unwrap();

    assert_eq!(migrate_between(&legacy, &portable), 1);
    assert_eq!(
        migrate_between(&legacy, &portable),
        0,
        "已有标记就别再扫旧目录"
    );

    // 用户删掉新配置想「重置」：旧配置不能因为重复迁移又跑回来。
    std::fs::remove_file(portable.join("preferences.toml")).unwrap();
    assert_eq!(migrate_between(&legacy, &portable), 0);
    assert!(!portable.join("preferences.toml").exists());
}

#[test]
fn missing_legacy_directory_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let portable = tmp.path().join("portable");
    assert_eq!(migrate_between(&tmp.path().join("nope"), &portable), 0);
    assert!(!portable.join(MARKER).exists(), "没有旧数据就不该写标记");
}
