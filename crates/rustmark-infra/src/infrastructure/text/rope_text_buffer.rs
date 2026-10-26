//! `ropey` 版文本缓冲实现（S5-01）。
//!
//! # 为什么用 Rope
//!
//! 大文档（如 1MB 的 README）在编辑器里要做**大量局部编辑**：每次按键都在中间插入
//! 或删除几个字节。`String::insert_str` / `replace_range` 每次都要 **O(n) memcpy**
//! 整个尾部，1MB 文档上一次按键就是百万字节搬移，无法满足 `NFR-PERF`。
//! `ropey::Rope` 把文本切成树状分块，局部编辑只重写被影响的分块。
//!
//! # 单位换算是本文件的主要复杂度来源
//!
//! **领域层契约以字节为单位**（[`TextBuffer`] 全部方法用 `ByteOffset`，
//! 因为 UTF-8 字符边界只能在字节层面判定），而 **ropey 的编辑 API 以字符为单位**
//! （`Rope::insert(char_idx, ..)` / `Rope::remove(char_range)`，
//! 因为它的分块是按字符切分的）。因此每个编辑方法都要做一次
//! 「字节偏移 → 字符索引」的转换（`byte_to_char`，O(log N)）。
//! 取文本则相反，必须用**字节**版的 `byte_slice`——用字符版的 `slice` 会
//! 静默把字节区间当成字符区间，插入中文后立刻错位。
//!
//! # 与参考实现（`tests/support/text_buffer.rs` 的 `StringTextBuffer`）的关系
//!
//! 两者**必须语义等价**——`tests/infrastructure/rope_buffer_test.rs` 用同一组用例
//! 跑 ropey 实现与朴素实现作对照，确保边界行为（越界、字符边界、行数约定、CRLF）
//! 完全一致。这是本文件的核心价值：若二者行为分叉，阶段 06 换缓冲实现就会
//! 静默改变编辑器语义。
//!
//! # 绝不 panic
//!
//! `panic = "abort"` 下任何 panic 都直接终止进程。ropey 的 `byte_to_char` /
//! `line_to_byte` / `byte_slice` 等在越界时**会 panic**，因此本文件一律改用
//! `try_*` 变体（返回 `Result`）并转换为 [`TextBufferError`]。

use std::borrow::Cow;

use ropey::Rope;

use rustmark_core::domain::document::errors::TextBufferError;
use rustmark_core::domain::document::services::{BufferFactory, TextBuffer};
use rustmark_core::shared::geometry::{ByteOffset, LineCol, TextRange};

/// 基于 `ropey::Rope` 的文本缓冲区。
#[derive(Clone, Default)]
pub struct RopeTextBuffer {
    rope: Rope,
}

impl RopeTextBuffer {
    /// 由文本构造。
    pub fn new_with_text(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
        }
    }

    /// `offset` 是否为缓冲区内的 UTF-8 字符边界。
    ///
    /// ropey 1.6 **没有** `is_char_boundary`，故自行推导：
    /// `byte_to_char` 对落在字符中间的字节返回「该字节所属字符的索引」，
    /// 再用 `char_to_byte` 折回字节——若等于原偏移，说明原偏移恰在字符起始处。
    /// 越界偏移单独判掉（`byte_to_char` 在那里会 panic）。
    fn is_boundary(&self, offset: usize) -> bool {
        if offset > self.rope.len_bytes() {
            return false;
        }
        // 0 与末尾恒为边界，无需走 O(log N) 折返。
        if offset == 0 || offset == self.rope.len_bytes() {
            return true;
        }
        let char_idx = self.rope.try_byte_to_char(offset).unwrap_or(0);
        self.rope.try_char_to_byte(char_idx).unwrap_or(0) == offset
    }

    /// 把字节偏移转换为字符索引（供 ropey 的编辑 API 使用）。
    fn byte_to_char(&self, offset: usize) -> Result<usize, TextBufferError> {
        self.rope
            .try_byte_to_char(offset)
            .map_err(|_| TextBufferError::OutOfRange {
                offset,
                len: self.rope.len_bytes(),
            })
    }

    /// 校验区间合法性与两端字符边界，返回字节范围。
    ///
    /// 抽成私有方法是因为 `slice` / `remove` 的前置校验**完全一致**，
    /// 重复实现容易在边界语义上分叉。
    fn validate(&self, range: TextRange) -> Result<std::ops::Range<usize>, TextBufferError> {
        let start = range.start().value();
        let end = range.end().value();
        let len = self.rope.len_bytes();
        // `TextRange::new` 已保证 start <= end，故这里只需查 end 越界。
        if end > len {
            return Err(TextBufferError::InvalidRange { start, end, len });
        }
        if !self.is_boundary(start) {
            return Err(TextBufferError::NotCharBoundary { offset: start });
        }
        if !self.is_boundary(end) {
            return Err(TextBufferError::NotCharBoundary { offset: end });
        }
        Ok(start..end)
    }
}

impl TextBuffer for RopeTextBuffer {
    fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    fn len_lines(&self) -> usize {
        // ropey 的 `len_lines` = 换行符数 + 1，与 trait 约定一致：
        // 空文本 1 行；`"a\n"` 2 行（末行是空行）。
        self.rope.len_lines()
    }

    fn insert(&mut self, offset: ByteOffset, text: &str) -> Result<(), TextBufferError> {
        let at = offset.value();
        let len = self.rope.len_bytes();
        if at > len {
            return Err(TextBufferError::OutOfRange { offset: at, len });
        }
        if !self.is_boundary(at) {
            return Err(TextBufferError::NotCharBoundary { offset: at });
        }
        // ropey 的插入点是**字符**索引，必须换算。
        let char_idx = self.byte_to_char(at)?;
        self.rope.insert(char_idx, text);
        Ok(())
    }

    fn remove(&mut self, range: TextRange) -> Result<String, TextBufferError> {
        let bounds = self.validate(range)?;
        // 先取出再删：`byte_slice` 返回借用，删除后借用即失效，必须先转成 String。
        let removed = self.rope.byte_slice(bounds.clone()).to_string();
        // ropey 的删除区间是**字符**索引，两端都要换算。
        let start_char = self.byte_to_char(bounds.start)?;
        let end_char = self.byte_to_char(bounds.end)?;
        self.rope.remove(start_char..end_char);
        Ok(removed)
    }

    fn slice(&self, range: TextRange) -> Result<Cow<'_, str>, TextBufferError> {
        let bounds = self.validate(range)?;
        // 必须用字节版 `byte_slice`；字符版 `slice` 会把字节区间误当字符区间。
        let slice = self.rope.byte_slice(bounds);
        // `as_str()` 仅在切片**内存连续**（单块）时返回 `Some(&str)`，
        // 这正是 S5-01 要求的「优先返回 Cow 避免大文件拷贝」的零拷贝快路径；
        // 跨块时才退化为 `Cow::Owned`（一次性分配）。
        Ok(match slice.as_str() {
            Some(text) => Cow::Borrowed(text),
            None => Cow::Owned(slice.to_string()),
        })
    }

    fn snapshot(&self) -> String {
        // Rope 的 `to_string` 是一次连续分配，比逐块拼接快。
        self.rope.to_string()
    }

    fn byte_to_line_col(&self, offset: ByteOffset) -> Result<LineCol, TextBufferError> {
        let at = offset.value();
        let len = self.rope.len_bytes();
        if at > len {
            return Err(TextBufferError::OutOfRange { offset: at, len });
        }
        // `try_byte_to_line` 对「字符中间的字节」也返回其所属行（不 panic），
        // 故行列语义与朴素实现（按 `\n` 计数）一致。
        let line = self
            .rope
            .try_byte_to_line(at)
            .map_err(|_| TextBufferError::OutOfRange { offset: at, len })?;
        let line_start = self
            .rope
            .try_line_to_byte(line)
            .map_err(|_| TextBufferError::OutOfRange { offset: at, len })?;
        // 列按**字节**计，与朴素实现（`at - line_start`）保持一致。
        Ok(LineCol::new(line, at - line_start))
    }

    fn line_col_to_byte(&self, line_col: LineCol) -> Result<ByteOffset, TextBufferError> {
        let start = self.line_start_byte(line_col.line())?.value();
        let end = self.line_end_byte(line_col.line())?.value();
        // 超出该行长度时饱和到行尾（trait 文档约定）。
        let target = (start + line_col.col()).min(end);
        Ok(self.char_boundary_floor(ByteOffset::new(target)))
    }

    fn line_start_byte(&self, line: usize) -> Result<ByteOffset, TextBufferError> {
        let lines = self.rope.len_lines();
        if line >= lines {
            return Err(TextBufferError::OutOfRange {
                offset: line,
                len: lines,
            });
        }
        self.rope
            .try_line_to_byte(line)
            .map(ByteOffset::new)
            .map_err(|_| TextBufferError::OutOfRange {
                offset: line,
                len: lines,
            })
    }

    fn line_end_byte(&self, line: usize) -> Result<ByteOffset, TextBufferError> {
        let lines = self.rope.len_lines();
        if line >= lines {
            return Err(TextBufferError::OutOfRange {
                offset: line,
                len: lines,
            });
        }
        // Rope 的「行」不含行尾换行符：非末行要减去那个 `\n`；末行到文件末尾。
        // 这与朴素实现「找 `\n` 位置，找不到则到末尾」完全等价。
        let line_end = if line + 1 == lines {
            self.rope.len_bytes()
        } else {
            self.rope
                .try_line_to_byte(line + 1)
                .map(|next| next.saturating_sub(1))
                .map_err(|_| TextBufferError::OutOfRange {
                    offset: line,
                    len: lines,
                })?
        };
        Ok(ByteOffset::new(line_end))
    }

    fn char_boundary_floor(&self, offset: ByteOffset) -> ByteOffset {
        let at = offset.value().min(self.rope.len_bytes());
        if self.is_boundary(at) {
            return ByteOffset::new(at);
        }
        // `byte_to_char` 把字符中间的字节映射到该字符的起始处，
        // 再折回字节即得到「向下取整」的边界。
        let char_idx = self.rope.try_byte_to_char(at).unwrap_or(0);
        ByteOffset::new(self.rope.try_char_to_byte(char_idx).unwrap_or(0))
    }

    fn char_boundary_ceil(&self, offset: ByteOffset) -> ByteOffset {
        let at = offset.value().min(self.rope.len_bytes());
        if self.is_boundary(at) {
            return ByteOffset::new(at);
        }
        // 向下取整拿到字符起始，再取「下一个字符的起始」即为向上边界。
        let char_idx = self.rope.try_byte_to_char(at).unwrap_or(0);
        ByteOffset::new(
            self.rope
                .try_char_to_byte(char_idx + 1)
                .unwrap_or_else(|_| self.rope.len_bytes()),
        )
    }
}

impl std::fmt::Debug for RopeTextBuffer {
    /// 只输出长度而非全文——测试失败时打印 1MB 文本会淹没诊断信息。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RopeTextBuffer")
            .field("len_bytes", &self.rope.len_bytes())
            .field("len_lines", &self.rope.len_lines())
            .finish()
    }
}

/// 标准 `FromStr` 实现：与 [`RopeTextBuffer::new_with_text`] 等价且**永不失败**
/// （`Rope` 可从任意 `&str` 构造），故错误类型是 [`Infallible`]。
impl std::str::FromStr for RopeTextBuffer {
    type Err = std::convert::Infallible;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Ok(Self::new_with_text(text))
    }
}

/// 供组合根注入的 [`BufferFactory`] 实现。
///
/// 存在意义：应用层只认 `BufferFactory` trait，不直接依赖 `ropey`，
/// 故必须由基础设施层提供这一「工厂 + 实现」的配对。
#[derive(Clone, Copy, Debug, Default)]
pub struct RopeBufferFactory;

impl BufferFactory for RopeBufferFactory {
    fn create(&self, content: &str) -> Box<dyn TextBuffer + Send + Sync> {
        Box::new(RopeTextBuffer::new_with_text(content))
    }
}
