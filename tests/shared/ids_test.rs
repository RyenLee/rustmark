//! `shared::ids` 单元测试（S2-04）。

use rustmark::shared::ids::{AssetId, DocumentId, JobId, WorkspaceId};

#[test]
fn new_ids_are_distinct() {
    let a = DocumentId::new();
    let b = DocumentId::new();
    assert_ne!(a, b, "同类型连续生成的 ID 必须不同");
}

#[test]
fn same_value_ids_are_equal() {
    let a = DocumentId::from_u64(42);
    let b = DocumentId::from_u64(42);
    assert_eq!(a, b);
    assert_eq!(a.as_u64(), 42);
}

#[test]
fn roundtrip_through_u64() {
    let id = WorkspaceId::new();
    assert_eq!(WorkspaceId::from_u64(id.as_u64()), id);
}

#[test]
fn display_includes_type_name() {
    let id = AssetId::from_u64(7);
    let text = id.to_string();
    assert!(
        text.starts_with("AssetId"),
        "Display 应含类型名，实际：{text}"
    );
    assert!(text.contains("7"));
}

#[test]
fn debug_matches_display() {
    let id = JobId::from_u64(3);
    assert_eq!(format!("{id}"), format!("{id:?}"));
}

#[test]
fn ids_are_copy_and_hashable() {
    use std::collections::HashSet;
    let a = DocumentId::from_u64(1);
    let b = a; // Copy
    let mut set = HashSet::new();
    set.insert(a);
    assert!(set.contains(&b));
    assert!(!set.contains(&DocumentId::from_u64(2)));
}
