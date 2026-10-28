//! 增量高亮的纯函数：变更区间计算与片段平移（从 `incremental.rs` 抽出；R-07 Batch 1）。

use rustmark_core::domain::markdown::value_objects::SyntaxSpan;
use rustmark_core::shared::geometry::{ByteOffset, TextRange};

/// 用「公共前缀 + 公共后缀」求新文本坐标下的变更区间。
///
/// # 为什么不用真正的 diff
///
/// 高亮只需要「哪一段可能影响语法状态」，不需要最小编辑脚本。公共前后缀
/// 给出的是**包含所有变更的最小连续区间**：区间外逐字节相同，正是增量
/// 恢复所需的全部前提；区间内哪怕有未变的片段，也只是多解几行。
///
/// 多光标 / 撤销这类「两处都变」的编辑会得到一个横跨两处的区间——最坏
/// 退化为全量，正确性不受影响。
///
/// # UTF-8 边界
///
/// 前缀/后缀是按**字节**比对的，可能落在多字节字符中间。此时向前/向后
/// 收缩到最近的 char boundary，把这个字符整体算进变更区（区间只能变大，
/// 不会切在字符中间导致后续切片 panic）。
pub(crate) fn changed_range(old: &str, new: &str) -> TextRange {
    let old_bytes = old.as_bytes();
    let new_bytes = new.as_bytes();

    let mut prefix = 0usize;
    let prefix_limit = old_bytes.len().min(new_bytes.len());
    while prefix < prefix_limit && old_bytes[prefix] == new_bytes[prefix] {
        prefix += 1;
    }
    while prefix > 0 && !new.is_char_boundary(prefix) {
        prefix -= 1;
    }

    let mut suffix = 0usize;
    let suffix_limit = (old_bytes.len() - prefix).min(new_bytes.len() - prefix);
    while suffix < suffix_limit
        && old_bytes[old_bytes.len() - 1 - suffix] == new_bytes[new_bytes.len() - 1 - suffix]
    {
        suffix += 1;
    }
    while suffix > 0 && !new.is_char_boundary(new_bytes.len() - suffix) {
        suffix -= 1;
    }

    TextRange::new(
        ByteOffset::new(prefix),
        ByteOffset::new(new_bytes.len() - suffix),
    )
}

/// 追加一个整体平移 `delta` 的旧片段。
///
/// 不复用 [`push_span`] 的合并逻辑：收敛点是行首，新旧片段分属不同行，
/// 跨收敛边界的合并会破坏「增量与全量逐片段一致」的验收标准。
pub(crate) fn push_shifted(spans: &mut Vec<SyntaxSpan>, span: SyntaxSpan, delta: i64) {
    let start = span.range().start().value() as i64 + delta;
    let end = span.range().end().value() as i64 + delta;
    if start < 0 || end < start {
        return;
    }
    spans.push(SyntaxSpan::new(
        TextRange::new(
            ByteOffset::new(start as usize),
            ByteOffset::new(end as usize),
        ),
        span.style(),
    ));
}
