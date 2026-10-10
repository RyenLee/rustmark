//! 内存 `String` 版 [`TextBuffer`] 实现（S3-02 验收要求的「参考实现」）。
//!
//! 用途有二：
//! 1. 阶段 03 起为领域层单测提供零依赖的缓冲区；
//! 2. 阶段 05 的 `RopeTextBuffer` 以**同一套用例**跑一遍，作为对照基线，
//!    确保 ropey 实现在边界语义（越界、字符边界、CRLF）上与朴素实现一致。

// 测试支撑代码：为后续阶段与更多用例预留的完整 fake API，未被当前用例全部引用属正常。
#![allow(dead_code)]

use std::borrow::Cow;

use rustmark::domain::document::errors::TextBufferError;
use rustmark::domain::document::services::TextBuffer;
use rustmark::shared::geometry::{ByteOffset, LineCol, TextRange};

/// 朴素 `String` 文本缓冲区。
#[derive(Clone, Default, Debug)]
pub struct StringTextBuffer {
    text: String,
}

impl StringTextBuffer {
    /// 构造空缓冲区。
    pub fn new() -> Self {
        Self {
            text: String::new(),
        }
    }

    /// 由文本构造。
    pub fn from_str(text: &str) -> Self {
        Self {
            text: text.to_string(),
        }
    }
}

impl TextBuffer for StringTextBuffer {
    fn len_bytes(&self) -> usize {
        self.text.len()
    }

    fn len_chars(&self) -> usize {
        self.text.chars().count()
    }

    fn len_lines(&self) -> usize {
        self.text.split('\n').count()
    }

    fn insert(&mut self, offset: ByteOffset, text: &str) -> Result<(), TextBufferError> {
        let at = offset.value();
        if at > self.text.len() {
            return Err(TextBufferError::OutOfRange {
                offset: at,
                len: self.text.len(),
            });
        }
        if !self.text.is_char_boundary(at) {
            return Err(TextBufferError::NotCharBoundary { offset: at });
        }
        self.text.insert_str(at, text);
        Ok(())
    }

    fn remove(&mut self, range: TextRange) -> Result<String, TextBufferError> {
        let start = range.start().value();
        let end = range.end().value();
        if start > end || end > self.text.len() {
            return Err(TextBufferError::InvalidRange {
                start,
                end,
                len: self.text.len(),
            });
        }
        if !self.text.is_char_boundary(start) || !self.text.is_char_boundary(end) {
            let offset = if self.text.is_char_boundary(start) {
                end
            } else {
                start
            };
            return Err(TextBufferError::NotCharBoundary { offset });
        }
        let removed = self.text[start..end].to_string();
        self.text.replace_range(start..end, "");
        Ok(removed)
    }

    fn slice(&self, range: TextRange) -> Result<Cow<'_, str>, TextBufferError> {
        let start = range.start().value();
        let end = range.end().value();
        if start > end || end > self.text.len() {
            return Err(TextBufferError::InvalidRange {
                start,
                end,
                len: self.text.len(),
            });
        }
        if !self.text.is_char_boundary(start) || !self.text.is_char_boundary(end) {
            let offset = if self.text.is_char_boundary(start) {
                end
            } else {
                start
            };
            return Err(TextBufferError::NotCharBoundary { offset });
        }
        Ok(Cow::Borrowed(&self.text[start..end]))
    }

    fn snapshot(&self) -> String {
        self.text.clone()
    }

    fn byte_to_line_col(&self, offset: ByteOffset) -> Result<LineCol, TextBufferError> {
        let at = offset.value();
        if at > self.text.len() {
            return Err(TextBufferError::OutOfRange {
                offset: at,
                len: self.text.len(),
            });
        }
        let prefix = &self.text.as_bytes()[..at];
        let line = prefix.iter().filter(|b| **b == b'\n').count();
        let line_start = prefix
            .iter()
            .rposition(|b| *b == b'\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        Ok(LineCol::new(line, at - line_start))
    }

    fn line_col_to_byte(&self, line_col: LineCol) -> Result<ByteOffset, TextBufferError> {
        let start = self.line_start_byte(line_col.line())?.value();
        let end = self.line_end_byte(line_col.line())?.value();
        let target = (start + line_col.col()).min(end);
        Ok(ByteOffset::new(
            self.char_boundary_floor(ByteOffset::new(target)).value(),
        ))
    }

    fn line_start_byte(&self, line: usize) -> Result<ByteOffset, TextBufferError> {
        if line == 0 {
            return Ok(ByteOffset::new(0));
        }
        let mut seen = 0;
        for (idx, byte) in self.text.as_bytes().iter().enumerate() {
            if *byte == b'\n' {
                seen += 1;
                if seen == line {
                    return Ok(ByteOffset::new(idx + 1));
                }
            }
        }
        Err(TextBufferError::OutOfRange {
            offset: line,
            len: self.len_lines(),
        })
    }

    fn line_end_byte(&self, line: usize) -> Result<ByteOffset, TextBufferError> {
        let start = self.line_start_byte(line)?.value();
        let end = self.text.as_bytes()[start..]
            .iter()
            .position(|b| *b == b'\n')
            .map(|idx| start + idx)
            .unwrap_or(self.text.len());
        Ok(ByteOffset::new(end))
    }

    fn char_boundary_floor(&self, offset: ByteOffset) -> ByteOffset {
        let mut at = offset.value().min(self.text.len());
        while at > 0 && !self.text.is_char_boundary(at) {
            at -= 1;
        }
        ByteOffset::new(at)
    }

    fn char_boundary_ceil(&self, offset: ByteOffset) -> ByteOffset {
        let mut at = offset.value().min(self.text.len());
        while !self.text.is_char_boundary(at) {
            at += 1;
        }
        ByteOffset::new(at)
    }
}

/// 便于构造领域聚合的辅助函数：把文本装进内存缓冲区。
pub fn buffer_of(text: &str) -> Box<dyn TextBuffer + Send + Sync> {
    Box::new(StringTextBuffer::from_str(text))
}
