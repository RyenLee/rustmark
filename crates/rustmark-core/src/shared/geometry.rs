//! 文本坐标值对象：字节偏移、文本区间、行列坐标。
//!
//! 约定：
//! - 一切偏移以 **字节** 为单位（与 UTF-8 文本、`Rope` 一致），不是字符数；
//! - [`TextRange`] 为**左闭右开**区间 `[start, end)`；
//! - [`LineCol`] 内部为 **0-based**，行列的 1-based 展示由表现层负责。

use std::ops::{Add, Sub};

/// 文本中的字节偏移。
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Debug)]
pub struct ByteOffset(usize);

impl ByteOffset {
    /// 构造字节偏移。
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    /// 取出内部 `usize` 值。
    pub const fn value(self) -> usize {
        self.0
    }
}

impl Add<usize> for ByteOffset {
    type Output = Self;
    fn add(self, rhs: usize) -> Self {
        Self(self.0 + rhs)
    }
}

impl Sub<usize> for ByteOffset {
    type Output = Self;
    /// 减法在越界时**饱和到 0**，不 panic（`panic = "abort"` 下 panic 即进程终止）。
    fn sub(self, rhs: usize) -> Self {
        Self(self.0.saturating_sub(rhs))
    }
}

impl Sub<Self> for ByteOffset {
    type Output = usize;
    /// 两个偏移之差，`self < rhs` 时饱和到 0。
    fn sub(self, rhs: Self) -> usize {
        self.0.saturating_sub(rhs.0)
    }
}

/// 文本区间（左闭右开）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct TextRange {
    start: ByteOffset,
    end: ByteOffset,
}

impl TextRange {
    /// 构造区间。
    ///
    /// `start > end` 时**自动归一化**（交换两端），因此反向选区无需调用方处理。
    pub fn new(start: ByteOffset, end: ByteOffset) -> Self {
        if start.value() <= end.value() {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }

    /// 构造空区间（起点即终点）。
    pub fn empty(at: ByteOffset) -> Self {
        Self { start: at, end: at }
    }

    /// 区间起点。
    pub const fn start(self) -> ByteOffset {
        self.start
    }

    /// 区间终点（不含）。
    pub const fn end(self) -> ByteOffset {
        self.end
    }

    /// 区间长度（字节），空区间为 0。
    pub fn len(self) -> usize {
        self.end.value().saturating_sub(self.start.value())
    }

    /// 是否为空区间。
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// `offset` 是否落在区间内（含起点、不含终点）。
    pub fn contains(self, offset: ByteOffset) -> bool {
        offset.value() >= self.start.value() && offset.value() < self.end.value()
    }

    /// 两区间是否有非空的交集。
    ///
    /// 注意：**相邻且端点相接的区间不算相交**（`[0,4)` 与 `[4,8)` 不相交）。
    pub fn intersects(self, other: Self) -> bool {
        self.start.value() < other.end.value() && other.start.value() < self.end.value()
    }

    /// 将本区间裁剪到 `limit` 之内；无交集时返回 `limit` 起点的空区间。
    pub fn clamp(self, limit: Self) -> Self {
        let start = self.start.value().max(limit.start.value());
        let end = self.end.value().min(limit.end.value());
        if start <= end {
            Self::new(ByteOffset::new(start), ByteOffset::new(end))
        } else {
            Self::empty(ByteOffset::new(limit.start.value()))
        }
    }

    /// 以 `usize` 区间形式取出（便于与 `Rope` / 切片 API 对接）。
    pub const fn as_usize_range(self) -> std::ops::Range<usize> {
        self.start.value()..self.end.value()
    }
}

/// 行列坐标，内部为 0-based。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct LineCol {
    line: usize,
    col: usize,
}

impl LineCol {
    /// 构造 0-based 行列坐标。
    pub const fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }

    /// 行号（0-based）。
    pub const fn line(self) -> usize {
        self.line
    }

    /// 列号（0-based）。
    pub const fn col(self) -> usize {
        self.col
    }

    /// 转为面向用户的 1-based 行号。
    pub const fn display_line(self) -> usize {
        self.line + 1
    }

    /// 转为面向用户的 1-based 列号。
    pub const fn display_col(self) -> usize {
        self.col + 1
    }
}

// ============================================================================
// 字符边界安全（DEF-04）
// ============================================================================

/// 向下取 `at` 之前**最近的字符边界**（`at` 越界时取 `text.len()`）。
///
/// # 为什么只向下、不向上
///
/// 本项目 `panic = "abort"`，一次越界切片就是窗口直接消失，没有回退余地。
/// 向上取整（`ceil_char_boundary`）会让偏移比实际**大**——用户数着自己的光标，
/// 会发现「明明在第 3 个字，状态栏却说第 4 列」。差一格且偏小，比崩掉好。
pub fn floor_char_boundary(text: &str, at: usize) -> usize {
    // `str::floor_char_boundary` 在 `at > len` 时 panic，故先裁剪。
    text.floor_char_boundary(at.min(text.len()))
}

/// 把字节区间裁剪并**吸附**到 `text` 的字符边界，返回可直接切片的区间。
///
/// `None` 表示吸附后区间为空（越界、反向、或两端落在同一个字符内部），
/// 调用方跳过即可。
///
/// # 为什么必须有（DEF-04）
///
/// 高亮 span 的坐标由解析器给出，与待绘制的文本**未必同源**：文本已被编辑而
/// span 还停在上一版、缓存键漏掉了版本、或调用方手写了区间，都会让边界落到某个
/// 多字节 UTF-8 字符的中间。此时 `&text[start..end]` 直接 panic，而
/// `panic = "abort"` 下 panic 即进程终止。
///
/// 表现层是最后一道防线：**宁可少上一段色，也不能崩**。
pub fn clamp_to_char_boundaries(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let start = floor_char_boundary(text, start);
    let end = floor_char_boundary(text, end);
    (start < end).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 落在多字节字符中间的边界必须向下吸附，吸附后可直接切片（不 panic）。
    #[test]
    fn clamp_snaps_mid_char_boundaries() {
        // "中" 占 3 字节：偏移 1 / 2 都在它内部，应吸到 0。
        let text = "中文abc";
        assert_eq!(floor_char_boundary(text, 1), 0);
        assert_eq!(floor_char_boundary(text, 2), 0);
        assert_eq!(floor_char_boundary(text, 3), 3);
        // 端点吸附后区间仍可切片，且不越过字符内部。
        let clamped = clamp_to_char_boundaries(text, 1, 5);
        assert!(clamped.is_some(), "非空源区间吸附后仍应非空");
        let (s, e) = clamped.unwrap_or((0, 0));
        assert_eq!((s, e), (0, 3));
        assert_eq!(&text[s..e], "中");
    }

    /// 越界、反向、整段落在同一字符内部时返回 `None`（调用方跳过，不 panic）。
    #[test]
    fn clamp_returns_none_when_nothing_to_slice() {
        let text = "中文abc";
        assert_eq!(clamp_to_char_boundaries(text, 1, 2), None, "同一字符内部");
        assert_eq!(clamp_to_char_boundaries(text, 5, 3), None, "反向区间");
        assert_eq!(
            clamp_to_char_boundaries(text, 100, 200),
            None,
            "整体越界（吸附后两端均为 len）"
        );
    }

    /// 越界偏移吸附到 `text.len()`，不得越出（否则切片仍会 panic）。
    #[test]
    fn floor_never_exceeds_len() {
        let text = "中文";
        assert_eq!(floor_char_boundary(text, 999), text.len());
        assert_eq!(floor_char_boundary("", 999), 0, "空串不得 panic");
    }

    /// 吸附后的两端一定满足 `is_char_boundary`，这是「可直接切片」的充要条件。
    #[test]
    fn clamp_output_is_always_sliceable() {
        let text = "中a文b\nc";
        for start in 0..=text.len() + 2 {
            for end in 0..=text.len() + 2 {
                if let Some((s, e)) = clamp_to_char_boundaries(text, start, end) {
                    assert!(text.is_char_boundary(s) && text.is_char_boundary(e));
                    let _ = &text[s..e]; // 不 panic 即为通过
                }
            }
        }
    }
}
