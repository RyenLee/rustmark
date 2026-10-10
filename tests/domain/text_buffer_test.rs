//! `StringTextBuffer` 参考实现单测（S3-02）。
//!
//! 本套用例是**对照基线**：阶段 05 的 `RopeTextBuffer` 必须能跑通同样一组用例，
//! 以保证 ropey 实现在越界、字符边界、CRLF 上的语义与朴素实现一致。

use rustmark::domain::document::errors::TextBufferError;
use rustmark::domain::document::services::TextBuffer;
use rustmark::shared::geometry::{ByteOffset, LineCol, TextRange};

use crate::support::text_buffer::StringTextBuffer;

#[test]
fn lengths_are_consistent() {
    let buffer = StringTextBuffer::from_str("中文ab");
    assert_eq!(buffer.len_bytes(), 8);
    assert_eq!(buffer.len_chars(), 4);
    assert_eq!(buffer.len_lines(), 1);
}

#[test]
fn empty_text_counts_as_one_line() {
    let buffer = StringTextBuffer::new();
    assert_eq!(buffer.len_lines(), 1);
    assert_eq!(buffer.len_bytes(), 0);
}

#[test]
fn trailing_newline_creates_last_empty_line() {
    let buffer = StringTextBuffer::from_str("a\n");
    assert_eq!(buffer.len_lines(), 2);
}

#[test]
fn insert_at_end_is_allowed_and_at_beyond_is_rejected() {
    let mut buffer = StringTextBuffer::from_str("abc");
    buffer.insert(ByteOffset::new(3), "!").unwrap();
    assert_eq!(buffer.snapshot(), "abc!");

    let result = buffer.insert(ByteOffset::new(99), "!");
    assert!(matches!(result, Err(TextBufferError::OutOfRange { .. })));
}

#[test]
fn insert_at_non_char_boundary_is_rejected() {
    let mut buffer = StringTextBuffer::from_str("中文");
    let result = buffer.insert(ByteOffset::new(1), "!");
    assert!(matches!(
        result,
        Err(TextBufferError::NotCharBoundary { .. })
    ));
    assert_eq!(buffer.snapshot(), "中文", "失败的插入不得修改缓冲区");
}

#[test]
fn remove_returns_removed_text() {
    let mut buffer = StringTextBuffer::from_str("hello world");
    let removed = buffer
        .remove(TextRange::new(ByteOffset::new(5), ByteOffset::new(11)))
        .unwrap();
    assert_eq!(removed, " world");
    assert_eq!(buffer.snapshot(), "hello");
}

#[test]
fn remove_invalid_range_is_rejected() {
    let mut buffer = StringTextBuffer::from_str("abc");
    let result = buffer.remove(TextRange::new(ByteOffset::new(2), ByteOffset::new(99)));
    assert!(matches!(result, Err(TextBufferError::InvalidRange { .. })));
}

#[test]
fn slice_borrows_without_copy() {
    let buffer = StringTextBuffer::from_str("hello");
    let sliced = buffer
        .slice(TextRange::new(ByteOffset::new(0), ByteOffset::new(5)))
        .unwrap();
    assert_eq!(sliced.as_ref(), "hello");
}

#[test]
fn byte_to_line_col_roundtrip() {
    let buffer = StringTextBuffer::from_str("abc\ndef\nghi");
    let line_col = buffer.byte_to_line_col(ByteOffset::new(6)).unwrap();
    assert_eq!(line_col, LineCol::new(1, 2));

    let back = buffer.line_col_to_byte(line_col).unwrap();
    assert_eq!(back.value(), 6);
}

#[test]
fn line_col_to_byte_clamps_to_line_end() {
    let buffer = StringTextBuffer::from_str("abc\ndef");
    let byte = buffer.line_col_to_byte(LineCol::new(0, 99)).unwrap();
    assert_eq!(byte.value(), 3, "列号越界应裁剪到行尾");
}

#[test]
fn line_start_and_end_bytes() {
    let buffer = StringTextBuffer::from_str("ab\ncd\n");
    assert_eq!(buffer.line_start_byte(0).unwrap().value(), 0);
    assert_eq!(buffer.line_end_byte(0).unwrap().value(), 2);
    assert_eq!(buffer.line_start_byte(1).unwrap().value(), 3);
    assert_eq!(buffer.line_end_byte(1).unwrap().value(), 5);
}

#[test]
fn line_start_byte_beyond_last_is_rejected() {
    let buffer = StringTextBuffer::from_str("ab\ncd");
    assert!(buffer.line_start_byte(9).is_err());
}

#[test]
fn char_boundary_floor_and_ceil() {
    let buffer = StringTextBuffer::from_str("中文");
    assert_eq!(buffer.char_boundary_floor(ByteOffset::new(1)).value(), 0);
    assert_eq!(buffer.char_boundary_ceil(ByteOffset::new(1)).value(), 3);
    assert_eq!(buffer.char_boundary_floor(ByteOffset::new(99)).value(), 6);
    assert_eq!(buffer.char_boundary_floor(ByteOffset::new(3)).value(), 3);
}

#[test]
fn crlf_is_handled_as_single_break() {
    let buffer = StringTextBuffer::from_str("a\r\nb");
    assert_eq!(buffer.len_lines(), 2);
    assert_eq!(buffer.line_start_byte(1).unwrap().value(), 3);
}
