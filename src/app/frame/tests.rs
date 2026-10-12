//! 模块外置测试。
use super::*;

/// 拖放分类：markdown 家族与 txt 应打开，其它扩展名应拒绝。
#[test]
fn classify_dropped_splits_by_extension() {
    let paths = vec![
        "/d/docs/笔记.md".to_string(),
        "/d/docs/readme.markdown".to_string(),
        "/d/docs/plain.txt".to_string(),
        "/d/docs/photo.png".to_string(),
        "/d/docs/report.pdf".to_string(),
        "/d/docs/archive.zip".to_string(),
    ];
    let (open, reject) = classify_dropped(&paths);
    assert_eq!(
        open,
        vec![
            "/d/docs/笔记.md".to_string(),
            "/d/docs/readme.markdown".to_string(),
            "/d/docs/plain.txt".to_string(),
        ],
        "markdown / markdown / txt 都应被识别为可打开"
    );
    assert_eq!(reject.len(), 3, "png/pdf/zip 应被拒绝");
}

/// 大小写不敏感：`.MD` 也应接受。
#[test]
fn classify_dropped_is_case_insensitive() {
    let (open, reject) = classify_dropped(&["/d/A.MD".to_string(), "/d/B.TXT".to_string()]);
    assert_eq!(open.len(), 2);
    assert!(reject.is_empty());
}

/// 空输入应不产生任何分组。
#[test]
fn classify_dropped_empty_is_noop() {
    let (open, reject) = classify_dropped(&[]);
    assert!(open.is_empty() && reject.is_empty());
}
