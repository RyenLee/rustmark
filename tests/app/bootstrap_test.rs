//! 依赖装配用例（S6-01）。

use std::path::PathBuf;

use rustmark::app::bootstrap::{ConfigPaths, PREVIEW_DEBOUNCE_MS, Services};
use rustmark::application::document::commands::NewDocumentCommand;
use rustmark::application::document::queries::GetContentQuery;
use rustmark::application::markdown::queries::RenderPreviewQuery;
use rustmark::application::markdown::service::RENDER_CACHE_CAPACITY;
use rustmark::application::preferences::queries::GetSettingsQuery;
use rustmark::application::workspace::commands::ScanTreeCommand;
use rustmark::shared::path::FilePath;

/// 在临时目录里装配一套服务。
fn assemble() -> (tempfile::TempDir, Services) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble(ConfigPaths::in_dir(dir.path())).expect("装配应成功");
    (dir, services)
}

#[test]
fn bootstrap_succeeds_with_fresh_config_directory() {
    let (_dir, services) = assemble();
    // 装配成功本身就说明：配置不存在时能落回默认（TomlPreferencesRepository
    // 对缺失文件返回默认，而不是 Err）。
    let settings = services
        .preferences
        .get_settings(GetSettingsQuery)
        .expect("应可读出设置");
    assert!(
        !settings.shortcuts.is_empty(),
        "默认配置应带出完整快捷键列表，否则三处显示会空掉"
    );
}

#[test]
fn fresh_install_defaults_to_following_system_theme() {
    let (_dir, services) = assemble();
    let settings = services
        .preferences
        .get_settings(GetSettingsQuery)
        .expect("应可读出设置");
    assert_eq!(
        settings.theme.kind, "system",
        "首次运行应跟随系统，而不是替用户选死深色或浅色"
    );
}

#[test]
fn previews_debounce_is_150ms() {
    // FR-01 写死 150ms。装配层若传错值，表现层看不出问题——
    // 只会「预览好像有点延迟」，没人会去查DebouncePolicy。
    assert_eq!(PREVIEW_DEBOUNCE_MS, 150);
}

#[test]
fn document_and_markdown_services_share_one_registry() {
    let (_dir, mut services) = assemble();
    let id = services
        .documents
        .new_document(NewDocumentCommand)
        .expect("应可新建文档");

    // 关键：Markdown 服务必须**看得到**刚建的文档。
    // 若 `provider()` 在错误的时机提取（装配顺序错了），预览会永远返回
    //「文档未打开」，而文档服务本身完全正常——这类错误只有跨服务用例能发现，
    // 且症状是「预览区永远是空的」，极难回溯到装配顺序。
    let rendered = services
        .markdown
        .render_preview(RenderPreviewQuery {
            doc_id: id,
            version: 0,
        })
        .expect("Markdown 服务必须能看到文档服务新建的文档");

    let content = services
        .documents
        .get_content(GetContentQuery { id, range: None })
        .expect("应可读内容");
    assert!(
        content.is_empty(),
        "新建文档的初始内容应为空，测试前提不成立"
    );
    assert!(
        !rendered.truncated,
        "空文档不应被判定为截断（截断阈值远大于 0 字节）"
    );
}

#[test]
fn workspace_scan_on_missing_root_reports_error_not_panic() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble(ConfigPaths::in_dir(dir.path())).expect("装配应成功");
    let missing = dir.path().join("does-not-exist");
    let result = services.workspace.scan_tree(ScanTreeCommand {
        root: FilePath::new(missing.to_string_lossy().replace('\\', "/")).expect("路径应合法"),
        max_depth: 3,
    });
    assert!(result.is_err(), "扫描不存在的目录必须报错");
}

#[test]
fn config_paths_detect_appends_rustmark_directory() {
    let paths = ConfigPaths::detect();
    assert!(
        paths
            .preferences
            .ends_with(PathBuf::from("rustmark").join("preferences.toml")),
        "配置应落在 rustmark 子目录下，实际：{}",
        paths.preferences.display()
    );
    assert!(
        paths
            .recent
            .ends_with(PathBuf::from("rustmark").join("recent.json")),
        "最近文件应与配置同目录，实际：{}",
        paths.recent.display()
    );
}

#[test]
fn corrupt_config_does_not_block_startup_and_leaves_a_backup() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    // 语法非法的 TOML。按 ADR-003：备份原文件 + 回退默认，**不阻断启动**。
    // 阻断启动是更差的结果——用户会因此打不开编辑器，
    // 而这只是一份配置坏了，文档本身安然无恙。
    std::fs::write(dir.path().join("preferences.toml"), "这不是合法 TOML [[[")
        .expect("应可写入损坏配置");

    let services = Services::assemble(ConfigPaths::in_dir(dir.path()))
        .expect("损坏配置不得阻止启动（ADR-003）");

    let settings = services
        .preferences
        .get_settings(GetSettingsQuery)
        .expect("回退后应可读出默认设置");
    assert_eq!(settings.theme.kind, "system", "应回退到默认配置");

    // 备份必须真的落盘：否则用户报障时我们无从查起，
    // 而「悄悄重置」这件事用户是看不见的。
    let has_backup = std::fs::read_dir(dir.path())
        .expect("应可列目录")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("preferences.toml.bak-")
        });
    assert!(has_backup, "损坏配置必须被备份，原文件要留痕");
}

#[test]
fn render_cache_capacity_is_reported_consistently() {
    // 缓存容量写在 bootstrap 里，预览服务又持有一份。
    // 若两者不一致，症状是「切标签页开始重新解析」——极难定位。
    assert_eq!(RENDER_CACHE_CAPACITY, 32);
}
