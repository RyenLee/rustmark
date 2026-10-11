//! V11-07 · 拼写建议算法（「你可能想输入」）。
//!
//! # 与判错的区别（这是本文件存在的前提）
//!
//! 判错用**删除变体索引**在 O(词长) 内问「有没有距离 ≤ 1 的近邻」；
//! 建议则要**真的列出**候选，索引做不到，故走「长度过滤 + 全词表距离计算」。
//! 两条路径的最大距离刻意取不同值：判错 = 1（放宽会把整篇技术文档标红），
//! 建议 = 2（放宽只是多几个候选，用户看不到想选的词顶多放弃替换）。
//!
//! 钉住四件事：
//! 1. 常见错词能给出**正确**的候选（这是功能的全部价值）；
//! 2. 正确词**没有**建议（`is_known` 优先，不给没错的人添乱）；
//! 3. 排序**稳定**（按距离再按字典序，否则菜单会抖动）；
//! 4. 上限与空输入等边界。

use rustmark::domain::markdown::services::SpellChecker;
use rustmark::infrastructure::spell::VendoredSpellChecker;
use rustmark::infrastructure::spell::simple_checker::suggest_candidates;

/// 造一个检查器。
fn checker() -> VendoredSpellChecker {
    VendoredSpellChecker::new()
}

// ---------------------------------------------------------------------------
// ① 召回：候选必须是「用户真正想打的那个词」
// ---------------------------------------------------------------------------

/// 三个最经典的错词都要能给出正确建议。
#[test]
fn common_typos_get_the_right_suggestion() {
    let c = checker();
    for (typo, expected) in [
        ("teh", "the"),
        ("recieve", "receive"),
        ("seperate", "separate"),
    ] {
        let got = c.suggest(typo, 5);
        assert!(
            got.iter().any(|s| s == expected),
            "{typo} 的建议里应有 {expected}，实际 {got:?}"
        );
    }
}

/// 易位错词（`teh`）必须把 `the` 排进**前三条**。
///
/// 纯 Levenshtein 会把 `teh → the` 记成 2（换两个字符），让它掉到一堆
/// 距离 1 的无关词后面；把「相邻易位记 1 步」之后它才能出现在用户看得见的位置。
#[test]
fn a_transposition_typo_ranks_the_intended_word_near_the_top() {
    let c = checker();
    let got = c.suggest("teh", 5);
    let rank = got.iter().position(|s| s == "the");
    assert!(
        rank.is_some_and(|i| i < 3),
        "the 应排在前三条，实际 {got:?}"
    );
}

/// 距离 2 的候选照样给——这正是「建议比判错宽松一档」的意义。
#[test]
fn distance_two_candidates_are_still_offered() {
    // `seperete` 与 `separate` 差两处且不相邻（既非易位也非单字符增删），
    // 判错链（门槛 1）抓不到它，建议链（门槛 2）应当给得出来。
    let got = suggest_candidates("seperete", 12);
    assert!(
        got.iter().any(|s| s == "separate"),
        "距离 2 也应有建议，实际 {got:?}"
    );
}

// ---------------------------------------------------------------------------
// ② 误报防线：没错的地方不给建议
// ---------------------------------------------------------------------------

/// 正确的词（含屈折形态）不该有建议——给没错的人弹菜单是骚扰。
#[test]
fn correct_words_have_no_suggestions() {
    let c = checker();
    for word in ["the", "receive", "colors", "dropped", "running", "colour"] {
        assert!(
            c.suggest(word, 5).is_empty(),
            "{word} 是正确拼写，不该有建议，实际 {:?}",
            c.suggest(word, 5)
        );
    }
}

/// 中文 / 空串 / 非字母输入都返回空集（不 panic、不产生噪声）。
#[test]
fn non_words_have_no_suggestions() {
    let c = checker();
    assert!(c.suggest("", 5).is_empty());
    assert!(c.suggest("中文", 5).is_empty());
    assert!(c.suggest("123", 5).is_empty());
}

/// 词表扩充（V11-07）不得引入新的误判：
/// 一批与新增词相邻的正确词仍必须干净。
///
/// 判错的代价远大于漏建议，词表每扩充一次都可能让「正确但不在词表里的词」
/// 因为多了近邻而被标红。这组断言就是那道闸。
#[test]
fn the_expanded_dictionary_does_not_create_new_false_positives() {
    let c = checker();
    let clean = "the colour of the honour is a favourite feature of this behaviour";
    let flagged: Vec<&str> = c
        .check(clean)
        .expect("检查不应失败")
        .iter()
        .map(|m| &clean[m.range.start().value()..m.range.end().value()])
        .collect();
    assert!(
        flagged.is_empty(),
        "词表扩充后不应出现新误报，实际标红 {flagged:?}"
    );
}

// ---------------------------------------------------------------------------
// ③ 排序稳定性与上限
// ---------------------------------------------------------------------------

/// 同样输入两次结果必须一致（菜单不该抖动）。
#[test]
fn suggestions_are_stable_across_calls() {
    let c = checker();
    assert_eq!(c.suggest("recieve", 5), c.suggest("recieve", 5));
}

/// `limit` 真的生效。
#[test]
fn the_limit_caps_the_result() {
    let c = checker();
    let all = suggest_candidates("recieve", 20);
    assert!(all.len() > 1, "这个错词应有多条候选，实际 {all:?}");
    assert_eq!(c.suggest("recieve", 1).len(), 1, "limit=1 只应返回一条");
}

/// `limit = 0` → 空集（不 panic）。
#[test]
fn a_zero_limit_yields_nothing() {
    assert!(suggest_candidates("teh", 0).is_empty());
    assert!(checker().suggest("teh", 0).is_empty());
}

/// 大写输入按小写处理（菜单给的是原形，表现层负责还原大小写）。
#[test]
fn uppercase_input_is_normalized() {
    let c = checker();
    assert_eq!(c.suggest("TEH", 5), c.suggest("teh", 5));
}

// ---------------------------------------------------------------------------
// ④ 性能：建议只在右键时算一次，但仍不能太慢
// ---------------------------------------------------------------------------

/// 单次建议在毫秒级（菜单弹出的那一刻算，用户不该感觉到顿挫）。
#[test]
fn a_single_suggestion_lookup_is_fast() {
    let c = checker();
    let start = std::time::Instant::now();
    for _ in 0..20 {
        let _ = c.suggest("recieve", 5);
    }
    let per_call = start.elapsed().as_millis() / 20;
    assert!(
        per_call < 50,
        "单次建议耗时 {per_call}ms，超出「点右键即出」的预算"
    );
}
