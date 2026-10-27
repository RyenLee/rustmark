//! 查找的**文本匹配纯函数**：单命中的两条字面量路径、正则路径、整批量
//! 线性扫描，以及词边界与字符边界这两个原子判定。
//!
//! 从 [`super`]（用例类型：`FindOptions` / `Finder` / `Match`）里分出来的
//! 理由：这些都是 `(&str, 选项) -> Option/Vec` 的纯计算，不持有任何状态。
//! 原先它们只能经由 `Finder::scan` 端到端覆盖——「忽略大小写字面量」有
//! **两条**实现（单次调用的 `find_literal_ignore_case` 与 DEF-01 修复引入的
//! 批量 `scan_literal_ignore_case_all`），其中一条退化时端到端用例未必能
//! 分辨是哪条在作祟。独立成模块后各条有自己的用例与变异验证。
//!
//! 两条忽略大小写路径**刻意并存**：`Finder::scan` 走批量版（O(N)，避免
//! O(N×M)），`find_from` 的逐命中路径仍走单次版。口径必须完全一致——
//! 由本模块底部的用例逐点对拍。

use crate::shared::pattern::Pattern;

use super::{FindOptions, Match};

/// 用**已编译**的 `pattern` 判断 `text` 中是否还有命中。
///
/// 与 [`super::Finder::scan`] 的区别：这里不重新解析模式串，因此正则模式的
/// 语法错误无从产生——`preview_replace_all` 拿到的 `self.pattern`
/// 必然是编译期已通过的产物。
pub(super) fn matches_with(pattern: &Pattern, text: &str, options: FindOptions) -> bool {
    find_from(pattern, options, text, 0).is_some_and(|m| !m.is_empty())
}

/// 从 `from` 起找第一个命中。
pub(super) fn find_from(
    pattern: &Pattern,
    options: FindOptions,
    text: &str,
    from: usize,
) -> Option<Match> {
    match pattern {
        Pattern::Literal(literal) => {
            let haystack = if options.case_sensitive {
                text
            } else {
                // 不区分大小写：先按小写找字节位置，再映回原文。
                // 注意 `to_lowercase()` 可能改变长度（如 `İ` → `i̇`），
                // 故这里逐字符比较而非整体小写，避免偏移错位。
                return find_literal_ignore_case(text, literal, from, options.whole_word);
            };
            find_literal(haystack, literal, from, options.whole_word)
        }
        Pattern::Regex(regex) => {
            let found = regex.find_in(text, from, text.len())?;
            if options.whole_word && !is_word_boundary(text, found.0, found.1) {
                // 跳过——继续往后找，但要防止死循环。
                let next = next_char_boundary(text, found.1);
                return find_from(pattern, options, text, next.max(from + 1));
            }
            Some(Match {
                start: found.0,
                end: found.1,
            })
        }
    }
}

/// 大小写敏感的字面量查找。
pub(super) fn find_literal(
    text: &str,
    literal: &str,
    from: usize,
    whole_word: bool,
) -> Option<Match> {
    let mut at = from;
    while at <= text.len() {
        let found = text.get(at..)?.find(literal)?;
        let start = at + found;
        let end = start + literal.len();
        if !whole_word || is_word_boundary(text, start, end) {
            return Some(Match { start, end });
        }
        // 不满足全字条件，继续往后。
        at = next_char_boundary(text, start);
    }
    None
}

/// 大小写不敏感的字面量查找。
///
/// 逐字符比较，**两侧都折叠成小写**。刻意不用
/// `text.to_lowercase().find(...)`：Unicode 小写映射会改变长度
/// （`İ` → `i̇` 是 1 个字符变 2 个），整体小写后再找会得到**错位**的字节偏移，
/// 切片时 panic 或切出乱码。
pub(super) fn find_literal_ignore_case(
    text: &str,
    literal: &str,
    from: usize,
    whole_word: bool,
) -> Option<Match> {
    let needle: Vec<char> = literal.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return None;
    }
    let haystack: Vec<(usize, char)> = text.char_indices().collect();
    for index in 0..haystack.len() {
        let start = haystack[index].0;
        if start < from {
            continue;
        }
        // 逐字符比对，两侧都折叠成小写。
        let hit = haystack[index..]
            .iter()
            .take(needle.len())
            .flat_map(|(_, ch)| ch.to_lowercase())
            .eq(needle.iter().copied());
        if !hit {
            continue;
        }
        let end = match haystack.get(index + needle.len()) {
            Some((offset, _)) => *offset,
            None => text.len(),
        };
        if whole_word && !is_word_boundary(text, start, end) {
            continue;
        }
        return Some(Match { start, end });
    }
    None
}

/// 大小写不敏感字面量查找的**单次线性扫描**版本（DEF-01 修复）。
///
/// 对照原 [`find_literal_ignore_case`]：那里每次调用都 `text.char_indices().collect()`
/// 重建整篇字符表，且 `Finder::scan` 每命中一次调一次 → 最坏 **O(N×M)**。
/// 本函数只构建一次「(原字节偏移, 字符)」序列，随后在该序列上做滑动窗口比对，
/// 整体 **O(N × |needle|)**（needle 长度固定时为 O(N)），不再随命中数退化。
///
/// 严格保持与原实现一致的比对语义：
/// - 两侧都折叠成小写再逐字符比较（不用 `to_lowercase()` 整体小写，避免
///   Unicode 小写映射改变长度导致字节偏移错位、切片 panic）；
/// - 命中后**按 `end` 字节推进**到下一个字符边界，避免重叠命中
///   （与原 `Finder::scan` 里 `at = found.end` 的推进一致）；
/// - 整词（`whole_word`）边界判定沿用 [`is_word_boundary`]；
/// - 受 `max_matches` 上限保护，达到即停。
pub(super) fn scan_literal_ignore_case_all(
    text: &str,
    literal: &str,
    whole_word: bool,
    max_matches: usize,
) -> Vec<Match> {
    // 与 [`find_literal_ignore_case`] 同款小写折叠（字符级，非整体小写）。
    let needle: Vec<char> = literal.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let haystack: Vec<(usize, char)> = text.char_indices().collect();
    if haystack.len() < needle.len() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    // 滑动窗口起点（以 haystack 下标计），命中后按 `end` 字节跳到下一字符边界。
    let mut index = 0usize;
    while index + needle.len() <= haystack.len() {
        let hit = haystack[index..]
            .iter()
            .take(needle.len())
            .flat_map(|(_, ch)| ch.to_lowercase())
            .eq(needle.iter().copied());
        if !hit {
            index += 1;
            continue;
        }
        let start = haystack[index].0;
        let end = match haystack.get(index + needle.len()) {
            Some((offset, _)) => *offset,
            None => text.len(),
        };
        if whole_word && !is_word_boundary(text, start, end) {
            // 整词边界不成立：与原实现一致，继续往后找（不按 end 跳过），
            // 因原 `find_literal_ignore_case` 对该起点 `continue` 而非返回。
            index += 1;
            continue;
        }
        matches.push(Match { start, end });
        if matches.len() >= max_matches {
            break;
        }
        // 命中后跳到 `end` 之后的第一个字符边界（与原 `at = found.end` 一致），
        // 避免同一字符参与两个重叠命中。
        index = match haystack.iter().position(|&(off, _)| off >= end) {
            Some(i) => i,
            None => haystack.len(),
        };
    }
    matches
}

/// 区间两侧是否为词边界。
///
/// 规则：起点前一个字符不是词字符，且终点后一个字符不是词字符。
/// 「词字符」取 Unicode 口径（[`char::is_alphanumeric`]），故汉字也是词字符——
/// 「中文abc」里的 `abc` 左边界**不**成立。（原注释写的「中文按非 ASCII
/// 字母数字处理」与实现不符，以本注释为准；口径由下方用例钉住。）
pub(super) fn is_word_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

/// 是否为「词字符」。
pub(super) fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// 下一个字符边界（保证不切在多字节字符中间）。
pub(super) fn next_char_boundary(text: &str, at: usize) -> usize {
    match text[at..].chars().next() {
        Some(ch) => at + ch.len_utf8(),
        None => at + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::pattern::Regex;

    /// 词边界：两侧都不能是词字符；`_` 算词字符，故 `a_b` 里的 `a` 不成立。
    ///
    /// 与侧栏搜索的整词口径（只认字母数字）**有意不同**——这条用例同时
    /// 钉住差异本身，防止哪天被「统一口径」掉。
    #[test]
    fn word_boundary_treats_underscore_as_a_word_char() {
        assert!(is_word_boundary("cat category", 0, 3), "行首 + 空格");
        assert!(!is_word_boundary("cat category", 4, 7), "右侧紧跟字母");
        assert!(!is_word_boundary("a_b", 0, 1), "`_` 是词字符");
        assert!(
            !is_word_boundary("中文abc", 6, 9),
            "汉字按 Unicode 口径也是词字符，左边界不成立"
        );
        assert!(
            is_word_boundary("中文 abc", 7, 10),
            "汉字后有空格则左边界成立"
        );
    }

    /// 词字符：字母数字与下划线算，标点和空白不算。
    #[test]
    fn word_char_is_alphanumeric_or_underscore() {
        assert!(is_word_char('a'));
        assert!(is_word_char('7'));
        assert!(is_word_char('中'), "Unicode 字母数字也算");
        assert!(is_word_char('_'));
        assert!(!is_word_char('-'));
        assert!(!is_word_char(' '));
    }

    /// 下一个字符边界：多字节字符整体跨过，越界时 +1（调用方用于强制前进）。
    #[test]
    fn next_char_boundary_skips_whole_characters() {
        assert_eq!(next_char_boundary("中文abc", 0), 3);
        assert_eq!(next_char_boundary("中文abc", 3), 6);
        assert_eq!(next_char_boundary("ab", 1), 2);
        assert_eq!(next_char_boundary("ab", 2), 3, "越界时也必须前进 1");
    }

    /// 忽略大小写的单次查找：逐字符折叠，偏移仍按**原文字节**给出。
    ///
    /// 刻意不用 `text.to_lowercase().find(...)`——Unicode 小写映射可能改变
    /// 长度（`İ` → `i̇` 是 1 个字符变 2 个），整体小写后再 `find` 会得到错位的
    /// 字节偏移，切片时 panic 或切出乱码。这条用例用等长折叠的 `Ä` / `ä`
    /// 验偏移正确（多字节字符的 `end` 必须落在字符边界上）。
    ///
    /// 已知边界（不在本用例断言范围内）：needle 的小写会**展开**成更多字符时
    /// （如 needle 为 `İ`），逐字符折叠比对不成立，返回 `None`。
    #[test]
    fn ignore_case_lookup_folds_per_character() {
        let found = find_literal_ignore_case("Hello HELLO hello", "hello", 0, false);
        assert_eq!(found, Some(Match { start: 0, end: 5 }));
        let from_6 = find_literal_ignore_case("Hello HELLO hello", "hello", 6, false);
        assert_eq!(from_6, Some(Match { start: 6, end: 11 }), "`from` 必须生效");
        // 多字节字符：`Ä` 占 2 字节，`end` 必须落在后一个 `x` 的起点（字节 3）。
        let text = "xÄx";
        assert_eq!(
            find_literal_ignore_case(text, "ä", 0, false),
            Some(Match { start: 1, end: 3 })
        );
    }

    /// 整词开关对单次查找生效：不满足边界的起点跳过，继续往后找。
    #[test]
    fn ignore_case_lookup_respects_whole_word() {
        let loose = find_literal_ignore_case("cat category cat", "cat", 0, false);
        assert_eq!(loose, Some(Match { start: 0, end: 3 }));
        let strict = find_literal_ignore_case("category cat", "cat", 0, true);
        assert_eq!(
            strict,
            Some(Match { start: 9, end: 12 }),
            "category 里的 cat 须跳过"
        );
    }

    /// 批量扫描的两条不变量：命中互不重叠，且受上限保护。
    ///
    /// 不重叠是它相对「每命中重建字符表」那条路径的关键差异——`aaa` 里找
    /// `aa` 只能命中一次，否则替换预览会把同一个字符替换两次。
    #[test]
    fn bulk_scan_is_non_overlapping_and_bounded() {
        assert_eq!(
            scan_literal_ignore_case_all("Hello hello HELLO", "hello", false, 100),
            vec![
                Match { start: 0, end: 5 },
                Match { start: 6, end: 11 },
                Match { start: 12, end: 17 }
            ]
        );
        assert_eq!(
            scan_literal_ignore_case_all("aaa", "aa", false, 100),
            vec![Match { start: 0, end: 2 }],
            "重叠命中只取第一个"
        );
        assert_eq!(
            scan_literal_ignore_case_all("aaa", "a", false, 2).len(),
            2,
            "上限达到即停"
        );
    }

    /// 两条忽略大小写路径口径一致：批量扫描 = 反复调用单条查找。
    ///
    /// 这两条实现**刻意并存**（一个走 O(N) 批量、一个走单次），是典型的
    /// 双源风险点；此处直接对拍，任一侧改了推进或边界口径都会立刻分叉。
    #[test]
    fn bulk_scan_agrees_with_repeated_single_lookups() {
        let text = "Hello hello HELLO 中文 hello";
        let bulk = scan_literal_ignore_case_all(text, "hello", false, 100);
        let mut repeated = Vec::new();
        let mut from = 0usize;
        while let Some(m) = find_literal_ignore_case(text, "hello", from, false) {
            repeated.push(m);
            from = m.end;
        }
        assert_eq!(bulk, repeated, "两条路径的命中序列必须逐项相等");
    }

    /// 正则路径的整词开关：不满足边界时**继续往后**，而不是原地返回。
    ///
    /// 少了这段递归就会把 `category` 里的 `cat` 当成命中——或者更糟，
    /// 因 `next.max(from + 1)` 写错而死循环。
    #[test]
    fn regex_lookup_skips_partial_hits_and_keeps_going() {
        let regex = Regex::compile("cat", true).expect("编译正则");
        let options = FindOptions {
            case_sensitive: true,
            whole_word: true,
            ..FindOptions::default()
        };
        let found = find_from(&Pattern::Regex(regex), options, "category cat", 0);
        assert_eq!(found, Some(Match { start: 9, end: 12 }));
    }

    /// `matches_with` 只认非空命中：零宽模式（`a*`）不得算「还有命中」。
    ///
    /// 判反了会让「全部替换」永远警告「替换后仍会有新命中」。
    #[test]
    fn matches_with_ignores_zero_width_hits() {
        let regex = Regex::compile("a*", true).expect("编译正则");
        let pattern = Pattern::Regex(regex);
        assert!(
            !matches_with(&pattern, "bbb", FindOptions::default()),
            "零宽命中不算命中"
        );
        let literal = Pattern::Literal("a".to_string());
        assert!(matches_with(&literal, "bab", FindOptions::default()));
        assert!(!matches_with(&literal, "bbb", FindOptions::default()));
    }
}
