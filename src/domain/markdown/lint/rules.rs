//! Lint 规则的逐行判定实现，是父模块 `MarkdownLinter` 的私有子模块。
//!
//! 规则类型（`LintRule` / `LintRuleSet`）与问题模型（`LintIssue`）定义在父模块；
//! 本文件只放按行扫描的判定函数。划分依据：每条规则是一项独立的业务能力，
//! 共享的围栏状态跟踪（`fenced_lines` 等）供多条规则复用。

use super::{LintIssue, LintRule};

/// 标出每一行是否处在**围栏代码块**内（含围栏行本身）。
///
/// 围栏判定与解析器一致：以 ``` 或 ~~~ 开头的行开合；关闭围栏必须与
/// 打开它的标记同字符且长度不少于前者（这是 CommonMark 的规则，
/// 也是「围栏里写 ``` 不会被提前关掉」的保障）。
pub(super) fn fenced_lines(lines: &[&str]) -> Vec<bool> {
    let mut out = vec![false; lines.len()];
    let mut open: Option<(char, usize)> = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some((marker, len)) = open {
            out[i] = true;
            if closes_fence(line, marker, len) {
                open = None;
            }
            continue;
        }
        if let Some((marker, len)) = opens_fence(line) {
            out[i] = true;
            open = Some((marker, len));
        }
    }
    out
}

/// 该行是否开启了围栏；返回 `(标记字符, 标记长度)`。
fn opens_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|c| *c == marker).count();
    (len >= 3).then_some((marker, len))
}

/// 该行是否关闭了 `(marker, len)` 打开的围栏。
fn closes_fence(line: &str, marker: char, len: usize) -> bool {
    let trimmed = line.trim_start();
    let count = trimmed.chars().take_while(|c| *c == marker).count();
    // 关闭行除标记外只允许空白（CommonMark：信息串只出现在**开始**行）。
    count >= len && trimmed.chars().skip(count).all(char::is_whitespace)
}

/// 标题层级跳跃：`H1` 直接跳到 `H3` 会让大纲缺一级。
pub(super) fn heading_jump(lines: &[&str], fenced: &[bool]) -> Vec<LintIssue> {
    let mut out = Vec::new();
    let mut previous: Option<u8> = None;
    for (i, line) in lines.iter().enumerate() {
        if fenced[i] {
            continue;
        }
        let Some(level) = atx_level(line) else {
            continue;
        };
        if let Some(prev) = previous
            && level > prev + 1
        {
            out.push(LintIssue::new(
                LintRule::HeadingJump,
                i,
                0,
                format!(
                    "标题层级从 H{prev} 直接跳到 H{level}，中间缺了 H{}",
                    prev + 1
                ),
            ));
        }
        previous = Some(level);
    }
    out
}

/// 该行是否为 ATX 标题；返回级别（1–6）。
fn atx_level(line: &str) -> Option<u8> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    // `#` 后面必须是空白（`#tag` 不是标题，是普通文本）。
    trimmed
        .chars()
        .nth(hashes)
        .is_some_and(char::is_whitespace)
        .then_some(hashes as u8)
}

/// 同一列表里混用 `-` / `*` / `+` 会让源码看起来不统一。
///
/// 「同一列表」的判定：连续的、缩进层级相同的列表行算作同一个列表块。
/// 缩进一变就当作另一个列表（嵌套列表用不同标记在许多规范里是允许的）。
pub(super) fn list_marker_mixed(lines: &[&str], fenced: &[bool]) -> Vec<LintIssue> {
    let mut out = Vec::new();
    let mut block: Option<(usize, char)> = None; // (缩进, 首个标记)
    for (i, line) in lines.iter().enumerate() {
        if fenced[i] {
            block = None;
            continue;
        }
        let Some((indent, marker)) = list_marker(line) else {
            block = None;
            continue;
        };
        match block {
            // 缩进相同的后续行：若换了标记就报一次（只报**第一次**变化，
            // 否则一个混得乱七八糟的列表会刷出十几条）。
            Some((prev_indent, first)) if prev_indent == indent => {
                if marker != first {
                    out.push(LintIssue::new(
                        LintRule::ListMarkerMixed,
                        i,
                        indent,
                        format!("同一列表里混用了 {first} 与 {marker}，建议统一"),
                    ));
                    block = None; // 报过就不再报同一块
                }
            }
            _ => block = Some((indent, marker)),
        }
    }
    out
}

/// 该行是否以列表标记开头；返回 `(缩进列, 标记字符)`。
fn list_marker(line: &str) -> Option<(usize, char)> {
    let indent = line.chars().take_while(|c| *c == ' ').count();
    let rest = line.trim_start();
    let marker = rest.chars().next()?;
    if !matches!(marker, '-' | '*' | '+') {
        return None;
    }
    // 分隔符 `---` / `***` 不是列表。
    if rest.chars().skip(1).all(|c| c == marker) {
        return None;
    }
    rest.chars()
        .nth(1)
        .is_some_and(char::is_whitespace)
        .then_some((indent, marker))
}

/// 行尾多余空格。
///
/// **2 个空格不算问题**：那是 Markdown 的硬换行语法，报它会逼用户
/// 删掉自己刻意写的东西（markdownlint 的 `MD009` 同样豁免两空格）。
pub(super) fn trailing_whitespace(lines: &[&str], fenced: &[bool]) -> Vec<LintIssue> {
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if fenced[i] {
            continue;
        }
        let trailing = line
            .chars()
            .rev()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count();
        if trailing == 0 || trailing == 2 {
            continue;
        }
        let column = line.chars().count().saturating_sub(trailing);
        out.push(LintIssue::new(
            LintRule::TrailingWhitespace,
            i,
            column,
            format!("行尾有 {trailing} 个多余空格（硬换行用 2 个即可）"),
        ));
    }
    out
}

/// 代码围栏未闭合：**整篇**只剩这一条规则不看 `fenced`（它要找的正是没闭合的那一段）。
pub(super) fn unclosed_fence(lines: &[&str]) -> Vec<LintIssue> {
    let mut open: Option<(usize, char, usize)> = None; // (行号, 标记, 长度)
    for (i, line) in lines.iter().enumerate() {
        match open {
            None => {
                if let Some((marker, len)) = opens_fence(line) {
                    open = Some((i, marker, len));
                }
            }
            Some((_, marker, len)) => {
                if closes_fence(line, marker, len) {
                    open = None;
                }
            }
        }
    }
    open.map(|(start, marker, _)| {
        vec![LintIssue::new(
            LintRule::UnclosedFence,
            start,
            0,
            format!("第 {} 行开始的 {} 代码围栏没有闭合", start + 1, marker),
        )]
    })
    .unwrap_or_default()
}
