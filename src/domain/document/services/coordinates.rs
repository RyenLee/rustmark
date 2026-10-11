//! [`EditingService`] 的坐标对齐与行区间能力：
//! UTF-8 字符边界对齐、行列 / 行区间换算、选区向整行扩展。

use crate::shared::geometry::{ByteOffset, TextRange};

use super::EditingService;

impl EditingService {
    /// 向下取到最近的 UTF-8 字符边界。
    ///
    /// 用于 IME 输入 / 中文选区（`FR-05`）：字节偏移可能落在字符中间，
    /// 直接在非边界处切片会 panic，故所有对外 API 先做对齐。
    pub fn char_floor(text: &str, offset: usize) -> usize {
        let mut i = offset.min(text.len());
        while i > 0 && !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    /// 向上取到最近的 UTF-8 字符边界（语义同 [`EditingService::char_floor`]）。
    pub fn char_ceil(text: &str, offset: usize) -> usize {
        let mut i = offset.min(text.len());
        while !text.is_char_boundary(i) {
            i += 1;
        }
        i
    }

    /// 把区间对齐到字符边界（起点向下、终点向上），避免切出半个字符。
    pub fn align_range(text: &str, range: TextRange) -> TextRange {
        TextRange::new(
            ByteOffset::new(Self::char_floor(text, range.start().value())),
            ByteOffset::new(Self::char_ceil(text, range.end().value())),
        )
    }

    /// `offset` 所在行的起始字节偏移（`offset` 恰为 `\n` 时属于该 `\n` 所在的行）。
    pub fn line_start(text: &str, offset: usize) -> usize {
        let offset = offset.min(text.len());
        match text.as_bytes()[..offset].iter().rposition(|b| *b == b'\n') {
            Some(idx) => idx + 1,
            None => 0,
        }
    }

    /// `offset` 所在行的结束字节偏移（**不含**换行符）。
    pub fn line_end(text: &str, offset: usize) -> usize {
        let offset = offset.min(text.len());
        match text.as_bytes()[offset..].iter().position(|b| *b == b'\n') {
            Some(idx) => offset + idx,
            None => text.len(),
        }
    }

    /// `offset` 所在行的区间（不含换行符）。
    pub fn line_range(text: &str, offset: usize) -> TextRange {
        let start = Self::line_start(text, offset);
        let end = Self::line_end(text, offset);
        TextRange::new(ByteOffset::new(start), ByteOffset::new(end))
    }

    /// 把区间**向外扩展**到覆盖完整的行（不含行尾换行符）。
    ///
    /// 行编辑类操作（缩进、列表前缀、整行移动）必须先调用本函数，
    /// 否则会在行中间插入内容。
    pub fn expand_to_full_lines(text: &str, range: TextRange) -> TextRange {
        let start = Self::line_start(text, range.start().value());
        let probe = if range.is_empty() {
            range.start().value()
        } else {
            range.end().value().saturating_sub(1)
        };
        let end = Self::line_end(text, probe.max(start));
        TextRange::new(ByteOffset::new(start), ByteOffset::new(end))
    }
}
