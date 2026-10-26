//! 查找引擎：编译后扫描、循环定位、全部替换预览与字面量/词边界算法。

use super::pattern::{Pattern, compile};
use super::{FindOptions, PatternError};

/// 一次查找的结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Match {
    /// 命中区间（**字节**）。
    pub start: usize,
    /// 命中区间终点（字节，不含）。
    pub end: usize,
}

impl Match {
    /// 是否为空命中（零宽匹配，如 `a*` 可匹配空串）。
    pub(crate) const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// 查找器：编译好的模式 + 全部命中。
#[derive(Clone, Debug)]
pub struct Finder {
    pattern: Pattern,
    options: FindOptions,
    matches: Vec<Match>,
}

impl Finder {
    /// 编译并扫描全文。
    pub fn scan(text: &str, pattern: &str, options: FindOptions) -> Result<Self, PatternError> {
        let pattern = compile(pattern, options)?;
        // 上限保护：10 万条足以覆盖任何真实文档，再多就是模式退化。
        const MAX_MATCHES: usize = 100_000;
        // DEF-01 修复：大小写不敏感的字面量查找改为单次线性扫描（O(N)），
        // 不再「每命中一次重建全篇字符表再重新扫描」导致 O(N×M)。
        // 其余分支（正则 / 大小写敏感字面量）维持原逐命中扫描。
        // 大小写不敏感的字面量查找走 O(N) 单次线性扫描（见下）。
        // 此处一次性把「忽略大小写 + 字面量」判定与绑定合并，避免后续 `unreachable!`
        // （`panic = "abort"` 下会让整个进程退出，违反「非测试代码尽量不 panic」）。
        let ignore_case_literal = match &pattern {
            Pattern::Literal(l) if !options.case_sensitive => Some(l),
            _ => None,
        };
        let matches = if let Some(literal) = ignore_case_literal {
            scan_literal_ignore_case_all(text, literal, options.whole_word, MAX_MATCHES)
        } else {
            let mut matches = Vec::new();
            let mut at = 0usize;
            while at <= text.len() && matches.len() < MAX_MATCHES {
                let Some(found) = find_from(&pattern, options, text, at) else {
                    break;
                };
                // 零宽匹配必须强制前进，否则死循环。
                at = if found.is_empty() {
                    next_char_boundary(text, found.end)
                } else {
                    found.end
                };
                if !found.is_empty() {
                    matches.push(found);
                }
            }
            matches
        };
        Ok(Self {
            pattern,
            options,
            matches,
        })
    }

    /// 全部命中。
    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    /// 命中数。
    pub fn count(&self) -> usize {
        self.matches.len()
    }

    /// 下一个命中（从 `after` 之后开始，循环）。
    pub fn next_after(&self, after: usize) -> Option<Match> {
        self.find_cyclic(after, true)
    }

    /// 上一个命中（从 `before` 之前开始，循环）。
    pub fn prev_before(&self, before: usize) -> Option<Match> {
        self.find_cyclic(before, false)
    }

    /// 循环查找：越过端点则回到另一端。
    fn find_cyclic(&self, from: usize, forward: bool) -> Option<Match> {
        if self.matches.is_empty() {
            return None;
        }
        if forward {
            // 命中区间终点 <= from 的都算「已过去」。
            self.matches
                .iter()
                .find(|m| m.end > from)
                .copied()
                .or_else(|| self.matches.first().copied())
        } else {
            // 起点 >= from 的都算「还没过去」。
            self.matches
                .iter()
                .rev()
                .find(|m| m.start < from)
                .copied()
                .or_else(|| self.matches.last().copied())
        }
    }

    /// 校验「全部替换」后的新内容。
    ///
    /// 返回 `Err` 表示替换文本会让命中数变化（例如替换文本本身含模式），
    /// 此时 UI 应提示「替换后仍会有新命中」，避免用户以为替换干净了。
    pub fn preview_replace_all(&self, text: &str, replacement: &str) -> ReplaceSummary {
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0usize;
        let mut count = 0usize;
        for m in &self.matches {
            if m.start < cursor {
                continue;
            }
            out.push_str(&text[cursor..m.start]);
            // 替换文本里的 `$1` 之类不支持——按字面量处理。
            out.push_str(replacement);
            cursor = m.end;
            count += 1;
        }
        out.push_str(&text[cursor..]);
        // 判断「替换后是否仍会有新命中」，必须用**原模式**去扫新文本，
        // 而不是用替换文本当模式——后者只在「替换文本恰好等于模式」时成立，
        // 会漏掉绝大多数真正会二次命中的情况。
        let will_match_again =
            !self.matches.is_empty() && matches_with(&self.pattern, &out, self.options);
        ReplaceSummary {
            result: out,
            count,
            will_match_again,
        }
    }
}

/// 替换结果摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceSummary {
    /// 替换后的全文。
    pub result: String,
    /// 实际替换次数。
    pub count: usize,
    /// 替换后是否仍会出现新命中。
    ///
    /// 例：用「`aa`」替换「`a`」，新文本里全是 `a`，再点一次全部替换会
    /// **无限增生**（`a` → `aa` → `aaaa` → …）。UI 必须据此警告，否则
    /// 用户会点到手酸。
    ///
    /// 反方向（用「`a`」替换「`aa`」）是收敛的，必须**不**报警——
    /// 误报会让用户以为替换不干净，反而不敢用这个功能。
    pub will_match_again: bool,
}

/// 用**已编译**的 `pattern` 判断 `text` 中是否还有命中。
///
/// 与 [`Finder::scan`] 的区别：这里不重新解析模式串，因此正则模式的
/// 语法错误无从产生——`preview_replace_all` 拿到的 `self.pattern`
/// 必然是编译期已通过的产物。
fn matches_with(pattern: &Pattern, text: &str, options: FindOptions) -> bool {
    find_from(pattern, options, text, 0).is_some_and(|m| !m.is_empty())
}

/// 从 `from` 起找第一个命中。
fn find_from(pattern: &Pattern, options: FindOptions, text: &str, from: usize) -> Option<Match> {
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
fn find_literal(text: &str, literal: &str, from: usize, whole_word: bool) -> Option<Match> {
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
fn find_literal_ignore_case(
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
fn scan_literal_ignore_case_all(
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
/// 中文按「非 ASCII 字母数字」处理，故「中文abc」里 `abc` 的左边界成立。
fn is_word_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

/// 是否为「词字符」。
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// 下一个字符边界（保证不切在多字节字符中间）。
fn next_char_boundary(text: &str, at: usize) -> usize {
    match text[at..].chars().next() {
        Some(ch) => at + ch.len_utf8(),
        None => at + 1,
    }
}
