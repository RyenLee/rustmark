//! 与层无关的**纯文本**工具（V11-07）。
//!
//! # 为什么要单独一个模块
//!
//! 「哪个字节属于哪个英文词」这条规则被两处需要：
//! 应用层的拼写建议（按 `offset` 找词）与表现层的右键菜单（按词区间替换）。
//! 若各写一份，改了一处必然漏另一处——那正是本模块存在的理由：
//! 它是**唯一**的分词规则，与检查器（`infrastructure::spell`）同源：
//! 连续的 ASCII 字母构成一个词，非 ASCII（中文）天然是分隔符。
//!
//! 放在 `shared` 而非 `domain` / `application`：表现层不得依赖 `application`
//! 以外的层，而它同样需要这条规则（见 `.agent/architecture.md`）。

/// 返回 `offset` 所在的**英文词**的字节区间 `(start, end)`。
///
/// # 边界语义
///
/// - `offset` 落在 ASCII 字母上 → 返回包含它的整词区间；
/// - `offset` 落在空白 / 标点 / 中文上 → `None`（那里没有可建议的词）；
/// - `offset` 越界（≥ `text.len()`）→ `None`（不 panic）；
/// - `offset` 不在字符边界上 → 先**向下取到**最近的字符边界再判断，
///   保证返回的区间两端都是合法边界（`panic = "abort"` 下越界切片会直接崩进程）。
pub fn word_bounds_at(text: &str, offset: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || offset >= bytes.len() {
        return None;
    }
    let mut at = offset;
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    if !bytes[at].is_ascii_alphabetic() {
        return None;
    }
    let mut start = at;
    while start > 0 && bytes[start - 1].is_ascii_alphabetic() {
        start -= 1;
    }
    let mut end = at;
    while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
        end += 1;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 落在词中间也能扩到整词。
    #[test]
    fn an_offset_inside_a_word_expands_to_the_whole_word() {
        assert_eq!(word_bounds_at("teh quick", 1), Some((0, 3)));
        assert_eq!(word_bounds_at("teh quick", 0), Some((0, 3)));
        assert_eq!(word_bounds_at("teh quick", 2), Some((0, 3)));
    }

    /// 空白与标点上没有词。
    #[test]
    fn separators_have_no_word() {
        assert_eq!(word_bounds_at("teh quick", 3), None);
        assert_eq!(word_bounds_at("hi, there", 2), None);
        assert_eq!(word_bounds_at("", 0), None);
    }

    /// 中文不是词的一部分（与检查器的分词约定一致）。
    #[test]
    fn chinese_is_a_separator_not_part_of_a_word() {
        let text = "中文 teh 中文";
        assert_eq!(word_bounds_at(text, 7), Some((7, 10)));
        assert_eq!(word_bounds_at(text, 0), None);
    }

    /// 越界不 panic。
    #[test]
    fn out_of_range_offsets_return_none() {
        assert_eq!(word_bounds_at("abc", 3), None);
        assert_eq!(word_bounds_at("abc", 99), None);
    }
}
