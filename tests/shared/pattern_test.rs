//! 迷你正则引擎单测（`shared::pattern`）。
//!
//! 本文件的核心用例是**字符类范围 × 大小写不敏感**的交叉分支：
//! 区间两端与待匹配字符必须走同一套折叠，否则 `[A-Z]` 在忽略大小写时
//! 只能匹配到小写（大写字母恒落在 `'a'` 之下被漏掉）。
//! 该缺陷同时影响查找替换面板与侧栏跨文件搜索，故在此钉死回归。

use rustmark::shared::pattern::Regex;

/// 收集 `text` 中的全部命中（字节区间）。
///
/// `find_in` 只返回**第一个**命中，这里按「上一次的结束位置」推进 `from`
/// 逐个取完；零宽匹配时至少推进一个字节，避免原地死循环。
fn all_matches(re: &Regex, text: &str) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    let mut from = 0;
    while let Some((start, end)) = re.find_in(text, from, text.len()) {
        hits.push((start, end));
        from = if end > start { end } else { start + 1 };
    }
    hits
}

#[test]
fn case_insensitive_upper_range_matches_uppercase_letters() {
    let re = Regex::compile("[A-Z]", false).unwrap();
    // 修复前：只有端点被折叠、待匹配字符原样参与比较，于是 "ABC" 一个都命中不了。
    assert_eq!(re.find_in("ABC", 0, 3), Some((0, 1)));
    assert_eq!(
        all_matches(&re, "aBcD"),
        vec![(0, 1), (1, 2), (2, 3), (3, 4)]
    );
}

#[test]
fn case_insensitive_lower_range_matches_uppercase_letters() {
    let re = Regex::compile("[a-z]", false).unwrap();
    assert_eq!(all_matches(&re, "ABC"), vec![(0, 1), (1, 2), (2, 3)]);
}

#[test]
fn case_sensitive_range_still_distinguishes_case() {
    let upper = Regex::compile("[A-Z]", true).unwrap();
    assert_eq!(upper.find_in("abc", 0, 3), None);
    assert_eq!(upper.find_in("ABC", 0, 3), Some((0, 1)));

    let lower = Regex::compile("[a-z]", true).unwrap();
    assert_eq!(lower.find_in("ABC", 0, 3), None);
    assert_eq!(lower.find_in("abc", 0, 3), Some((0, 1)));
}

#[test]
fn catastrophic_backtracking_is_capped_by_the_step_budget() {
    // `(a+)+$` 是教科书式的指数级回溯模式。引擎在查找框键入时同步执行，
    // 若无步数预算，这类输入会把 UI 线程卡死而不报错、不入常规单测。
    let re = Regex::compile("(a+)+$", false).unwrap();
    let hostile = "a".repeat(64) + "!";

    let started = std::time::Instant::now();
    let hit = re.find_in(&hostile, 0, hostile.len());
    let elapsed = started.elapsed();

    // 预算耗尽 → 降级为「未命中」，而不是悬挂。
    assert_eq!(hit, None, "预算耗尽应判为未命中，而不是返回错误结果");
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "对抗性模式应当在毫秒级退出，实测 {elapsed:?}"
    );
}

#[test]
fn step_budget_does_not_break_ordinary_long_matches() {
    // 反例：预算必须足够宽松，不能把「普通模式 + 长文本」的正常匹配也吃掉。
    let re = Regex::compile("[a-z]+", false).unwrap();
    let text = "word ".repeat(2_000); // 10k 字符量级的一行
    assert_eq!(re.find_in(&text, 0, text.len()), Some((0, 4)));
}

#[test]
fn single_char_class_and_negated_range_are_unaffected() {
    // 单字符类走 `One` 分支（`chars_eq` 双向折叠），本就有正确行为。
    let one = Regex::compile("[A]", false).unwrap();
    assert_eq!(one.find_in("a", 0, 1), Some((0, 1)));

    // 取反区间同理：既不能被范围分支的修复带坏，也不该反向吞字符。
    let negated = Regex::compile("[^0-9]", false).unwrap();
    assert_eq!(negated.find_in("9", 0, 1), None);
    assert_eq!(negated.find_in("a", 0, 1), Some((0, 1)));
}
