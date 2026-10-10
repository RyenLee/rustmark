//! `FilePath` 值对象单测（S3-A）。

use rustmark::shared::path::{FilePath, PathError};

#[test]
fn backslashes_are_normalized_to_forward_slashes() {
    let path = FilePath::new(r"C:\Users\docs\a.md").unwrap();
    assert_eq!(path.as_str(), "C:/Users/docs/a.md");
}

#[test]
fn redundant_segments_and_dot_are_collapsed() {
    let path = FilePath::new("/a//b/./c").unwrap();
    assert_eq!(path.as_str(), "/a/b/c");
}

#[test]
fn traversal_is_rejected() {
    assert!(matches!(
        FilePath::new("/a/../b"),
        Err(PathError::Traversal)
    ));
    assert!(matches!(FilePath::new(".."), Err(PathError::Traversal)));
}

#[test]
fn empty_and_null_paths_are_rejected() {
    assert!(matches!(FilePath::new(""), Err(PathError::Empty)));
    assert!(matches!(FilePath::new("/"), Err(PathError::Empty)));
    assert!(matches!(FilePath::new("a\0b"), Err(PathError::NullByte)));
}

#[test]
fn file_name_and_extension() {
    let path = FilePath::new("/docs/readme.md").unwrap();
    assert_eq!(path.file_name(), "readme.md");
    assert_eq!(path.extension(), Some("md"));

    let no_ext = FilePath::new("/docs/Makefile").unwrap();
    assert_eq!(no_ext.extension(), None);

    let dotfile = FilePath::new("/docs/.gitignore").unwrap();
    assert_eq!(dotfile.extension(), None);
}

#[test]
fn parent_returns_containing_directory() {
    let path = FilePath::new("/docs/sub/a.md").unwrap();
    assert_eq!(
        path.parent().map(|p| p.as_str().to_string()),
        Some("/docs/sub".to_string())
    );

    let top = FilePath::new("/a.md").unwrap();
    assert_eq!(top.parent(), None, "根下第一层没有父目录");
}

#[test]
fn join_validates_single_segment() {
    let dir = FilePath::new("/docs").unwrap();
    assert_eq!(dir.join("a.md").unwrap().as_str(), "/docs/a.md");
    assert!(dir.join("").is_err());
    assert!(dir.join("sub/a.md").is_err());
    assert!(dir.join("..").is_err());
}

#[test]
fn is_under_detects_descendants() {
    let root = FilePath::new("/ws").unwrap();
    let child = FilePath::new("/ws/docs/a.md").unwrap();
    let sibling = FilePath::new("/wsx/a.md").unwrap();

    assert!(child.is_under(&root));
    assert!(!root.is_under(&root), "自身不算自己的后代");
    assert!(!sibling.is_under(&root), "同前缀但不是子目录的情况必须排除");
}

#[test]
fn try_from_conversions() {
    let from_string = FilePath::try_from("/a/b.md".to_string()).unwrap();
    let from_str = FilePath::try_from("/a/b.md").unwrap();
    assert_eq!(from_string, from_str);
}

#[test]
fn display_matches_normalized_string() {
    let path = FilePath::new("/a/b.md").unwrap();
    assert_eq!(path.to_string(), "/a/b.md");
}
