//! 模块外置测试。
use super::*;

/// 拖放分类：白名单内的格式（markdown 家族 / txt / csv）应打开，其余应拒绝。
#[test]
fn classify_dropped_splits_by_extension() {
    let paths = vec![
        "/d/docs/笔记.md".to_string(),
        "/d/docs/readme.markdown".to_string(),
        "/d/docs/plain.txt".to_string(),
        "/d/docs/table.csv".to_string(),
        "/d/docs/photo.png".to_string(),
        "/d/docs/report.pdf".to_string(),
        "/d/docs/archive.zip".to_string(),
    ];
    let (open, reject) = classify_dropped(&TextFormats::builtin(), &paths);
    assert_eq!(
        open,
        vec![
            "/d/docs/笔记.md".to_string(),
            "/d/docs/readme.markdown".to_string(),
            "/d/docs/plain.txt".to_string(),
            "/d/docs/table.csv".to_string(),
        ],
        "markdown / markdown / txt / csv 都应被识别为可打开（与文件树守卫同一份白名单）"
    );
    assert_eq!(reject.len(), 3, "png/pdf/zip 应被拒绝");
}

/// 大小写不敏感：`.MD` 也应接受。
#[test]
fn classify_dropped_is_case_insensitive() {
    let (open, reject) = classify_dropped(
        &TextFormats::builtin(),
        &["/d/A.MD".to_string(), "/d/B.TXT".to_string()],
    );
    assert_eq!(open.len(), 2);
    assert!(reject.is_empty());
}

/// 空输入应不产生任何分组。
#[test]
fn classify_dropped_empty_is_noop() {
    let (open, reject) = classify_dropped(&TextFormats::builtin(), &[]);
    assert!(open.is_empty() && reject.is_empty());
}

/// 分类用的是**传入的**白名单，不是内置常量——拖放与守卫同源。
#[test]
fn classify_dropped_follows_the_injected_whitelist() {
    let custom = TextFormats::builtin().extended_with(["log"]);
    let paths = vec!["/d/app.log".to_string(), "/d/a.md".to_string()];
    let (open, _) = classify_dropped(&custom, &paths);
    assert_eq!(open.len(), 2, "追加的 .log 应随白名单一起放行");
    let (open, reject) = classify_dropped(&TextFormats::builtin(), &paths);
    assert_eq!(open.len(), 1);
    assert_eq!(reject.len(), 1);
}
