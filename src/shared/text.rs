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

/// 文本的逻辑行数（`split('\n')` 口径：末尾换行会多出一行空行）。
///
/// # 为什么按**字节**数换行
///
/// `\n` 是 ASCII（0x0A），UTF-8 的续字节恒 ≥ 0x80——因此字节 `0x0A` 的个数
/// 与 `'\n'` 字符的个数**恒等**。直接扫字节省掉了逐码位的 UTF-8 解码，
/// 实测（release，100KB 中文 Markdown）为 `split('\n').count()` 的约 **1.8 倍速**，
/// 1MB 文档上从 ~0.5ms 降到 ~0.27ms（`docs/metrics.md` §3.2.2）。
///
/// # 与 `split('\n').count()` 的等价性
///
/// 设文本含 `k` 个 `'\n'`：`split('\n')` 恒产出 `k + 1` 段（含首尾空段），
/// 故返回值 `k + 1` 与之逐例相等——`""` → 1、`"a"` → 1、`"a\n"` → 2、`"a\nb"` → 2。
/// 也因此**永远 ≥ 1**，调用方不必再 `.max(1)`。
///
/// # 为什么放在 `shared`
///
/// 编辑区（每帧算 `desired_rows` 与行号栏宽度）、行号栏、状态栏、文档统计
/// 四处都要这个数。各写一份 `split('\n').count()` 是四份会各自漂移的实现，
/// 而 `shared` 是各层唯一共用的依赖点（同 [`word_bounds_at`] 的理由）。
///
/// # 为什么值得单独抽出来（问题 046）
///
/// 编辑区那处是**每帧**调用，属热路径：实测 100KB 文档每次 0.048ms、
/// 1MB 文档约 0.27–0.5ms。这类「每帧的 O(全文) 构造」在问题 043 已定过纪律
/// （新增每帧查询先问：构造是 O(1) 还是 O(全文)？），能便宜就便宜。
pub fn line_count(text: &str) -> usize {
    byte_line_breaks(text.as_bytes()) + 1
}

/// 给定字节偏移，返回它所在的行号（0-based）。
///
/// `offset` 先**向下取到字符边界**再数——光标字节偏移可能落在多字节字符中间
/// （程序化设值只按长度夹紧），直接切片会 panic，`panic = "abort"` 下即整窗口消失
/// （实测崩溃日志 crash-20261011005255：3887 落在「右」的 3885..3888 中间）。
pub fn line_index_at(text: &str, offset: usize) -> usize {
    let clamped = offset.min(text.len());
    let clamped = crate::shared::geometry::floor_char_boundary(text, clamped);
    byte_line_breaks(&text.as_bytes()[..clamped])
}

/// 数字节切片里 `\n` 的个数（两者共用的唯一实现）。
fn byte_line_breaks(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&b| b == b'\n').count()
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

    /// 行数与 `split('\n').count()` 逐例相等（等价性是这次改写的前提）。
    #[test]
    fn line_count_matches_split_semantics() {
        for case in [
            "",
            "a",
            "a\n",
            "a\nb",
            "a\nb\n",
            "\n",
            "\n\n",
            "中文\n中文\n",
        ] {
            assert_eq!(
                line_count(case),
                case.split('\n').count(),
                "行数口径必须与 split('\\n') 一致：{case:?}"
            );
            assert!(line_count(case) >= 1, "行数恒 ≥ 1：{case:?}");
        }
    }

    /// 多字节字符不会让字节扫描数错。
    #[test]
    fn line_count_is_utf8_safe() {
        // 每个汉字 3 字节，其续字节 ≥ 0x80，不会被误判成 '\n'。
        assert_eq!(line_count("中文中文"), 1);
        assert_eq!(line_count("中文\n中文"), 2);
    }

    /// 行号换算与「按字符数」一致，且落在多字节字符中间不 panic。
    #[test]
    fn line_index_agrees_with_char_counting() {
        let text = "一\n二\n三";
        for offset in 0..=text.len() {
            // 期望值自己也要先落到字符边界，否则切片本身就会 panic。
            let at = crate::shared::geometry::floor_char_boundary(text, offset);
            let expected = text[..at].chars().filter(|&c| c == '\n').count();
            assert_eq!(line_index_at(text, offset), expected, "offset={offset}");
        }
        // 越界夹紧，不 panic。
        assert_eq!(line_index_at(text, 9999), 2);
    }
}
