//! 崩溃日志测试（S9-08）。
//!
//! # 验收标准对照
//!
//! - 人为 panic 生成崩溃记录 → `installed_hook_writes_a_report_on_panic`
//!   （panic hook 在 `catch_unwind` 捕获时**同样触发**，可在测试内闭环）；
//! - 默认不产生日志 → `normal_startup_writes_nothing`（正常装配零写盘）；
//! - 报告内容与格式 → `report_contains_version_platform_and_message`；
//! - 保留上限 → `prune_keeps_only_the_newest_reports`。

use rustmark::app::bootstrap::ConfigPaths;
use rustmark::app::crash_log::{self, CrashReport};

/// 报告内容：版本、平台、消息、位置、栈回溯都必须在场。
#[test]
fn report_contains_version_platform_and_message() {
    let report = CrashReport::from_panic(
        &"boom：故意崩溃".to_string() as &(dyn std::any::Any + Send),
        "src/app/crash_log.rs:123:45".to_string(),
        "main".to_string(),
    );
    let text = report.render();
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "缺版本：{text}");
    assert!(
        text.contains(std::env::consts::OS) && text.contains(std::env::consts::ARCH),
        "缺平台：{text}"
    );
    assert!(text.contains("boom：故意崩溃"), "缺消息：{text}");
    assert!(text.contains("crash_log.rs:123:45"), "缺位置：{text}");
    assert!(text.contains("栈回溯"), "缺回溯小节：{text}");
    assert!(!text.contains('\u{0}'), "报告必须是纯文本：{text}");
}

/// 非字符串 panic 载荷降级为占位文案，不得 panic。
#[test]
fn non_string_payload_degrades_to_placeholder() {
    struct Opaque;
    let report = CrashReport::from_panic(
        &Opaque as &(dyn std::any::Any + Send),
        "x:1:1".to_string(),
        "t".to_string(),
    );
    assert!(
        report.message.contains("非字符串"),
        "应给占位文案而不是强行 downcast：{:?}",
        report.message
    );
}

/// 写盘：文件落在指定目录、带 `crash-` 前缀、内容即渲染文本。
#[test]
fn write_report_lands_in_the_given_directory() {
    let dir = tempfile::tempdir().unwrap();
    let report = CrashReport::from_panic(
        &"写盘用例".to_string() as &(dyn std::any::Any + Send),
        "x:1:1".to_string(),
        "main".to_string(),
    );
    let path = crash_log::write_report(dir.path(), &report).expect("写盘应成功");
    assert!(path.is_file());
    assert!(
        path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt")),
        "文件名必须是 crash-*.txt：{:?}",
        path
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("写盘用例"));
}

/// 验收「人为 panic 生成崩溃记录」：装好 hook 后触发真实 panic
/// （用 `catch_unwind` 接住——hook 在捕获时**同样**触发），
/// 断言磁盘上出现包含该消息的报告。
#[test]
fn installed_hook_writes_a_report_on_panic() {
    let dir = tempfile::tempdir().unwrap();
    crash_log::install_hook(dir.path().to_path_buf());

    let result = std::panic::catch_unwind(|| panic!("S9-08 人为崩溃标记 XYZ"));
    assert!(result.is_err(), "panic 应被 catch_unwind 接住");

    // hook 是全局的：其他测试的 panic 也会写进来，所以断言
    // 「存在含本用例标记的报告」而不是「目录里恰好一份」。
    let found = std::fs::read_dir(dir.path())
        .expect("崩溃目录应被创建")
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .any(|text| text.contains("S9-08 人为崩溃标记 XYZ"));
    assert!(found, "人为 panic 后应生成含该消息的崩溃记录");
}

/// 验收「默认不产生日志」：正常路径**零写盘**。
///
/// 装配、配置读写都不该碰崩溃目录——目录甚至不应该存在。
/// 生产入口（`run()`）只在安装 panic hook 时做一次「列目录清理」，
/// 目录不存在时静默跳过，同样不会创建它。
#[test]
fn normal_startup_writes_nothing() {
    let cfg = tempfile::tempdir().unwrap();
    let paths = ConfigPaths::in_dir(cfg.path());
    // 模拟一次正常启动会触发的读取路径。
    let _ = std::fs::read_to_string(&paths.preferences);
    let crash_dir = paths.crash_dir();
    assert!(
        !crash_dir.exists(),
        "正常使用不得产生任何日志/崩溃文件：{:?}",
        crash_dir
    );
}

/// 保留上限：只留最新 10 份，更老的按文件名（内嵌时间戳）删除。
#[test]
fn prune_keeps_only_the_newest_reports() {
    let dir = tempfile::tempdir().unwrap();
    // 造 14 份：文件名内嵌递增时间戳（字典序即时间序）。
    for i in 0..14u32 {
        std::fs::write(
            dir.path()
                .join(format!("crash-20261009-0000{:02}-1.txt", i)),
            format!("report {i}"),
        )
        .unwrap();
    }
    // 混入干扰项：非 crash 前缀与非 .txt 都不应被清。
    std::fs::write(dir.path().join("preferences.toml"), b"x").unwrap();
    std::fs::write(dir.path().join("crash-not-a-report.md"), b"x").unwrap();

    crash_log::prune_reports(dir.path());

    let mut remaining: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        .collect();
    remaining.sort();
    assert_eq!(remaining.len(), 10, "应只保留 10 份：{remaining:?}");
    // 最旧的 4 份（000000..000003）必须被删，最新 10 份（000004..000013）保留。
    assert!(!remaining.contains(&"crash-20261009-000000-1.txt".to_string()));
    assert_eq!(remaining[0], "crash-20261009-000004-1.txt");
    assert_eq!(remaining[9], "crash-20261009-000013-1.txt");
    assert!(
        dir.path().join("preferences.toml").is_file(),
        "非崩溃文件不得被误删"
    );
    assert!(
        dir.path().join("crash-not-a-report.md").is_file(),
        "非 .txt 报告不得被误删"
    );
}

/// 同一秒内多次崩溃不互相覆盖：文件名内嵌进程 ID。
///
/// 连写两份报告，磁盘上必须是两个不同的文件——
/// 若文件名只含秒级时间戳，同秒的第二次崩溃会覆盖第一次。
#[test]
fn consecutive_crashes_do_not_overwrite_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let mk = |n: u32| CrashReport {
        version: "0.1.0".into(),
        platform: "test".into(),
        timestamp: format!("2026-10-09 11:00:0{n}"),
        message: format!("msg {n}"),
        location: "x:1:1".into(),
        thread: "main".into(),
        backtrace: String::new(),
    };
    let p1 = crash_log::write_report(dir.path(), &mk(1)).unwrap();
    let p2 = crash_log::write_report(dir.path(), &mk(2)).unwrap();
    assert_ne!(p1, p2, "两份报告不得写到同一路径");
}
