//! 拼写检查器单测（路线①零依赖）。
//!
//! 断言分三类：
//! 1. **召回**：常见错词必须被抓出；
//! 2. **误报防线**：正确英文、中文、标识符、缩写一律不标红——
//!    对辅助提示功能而言，误报比漏报更致命；
//! 3. **坐标契约**：区间必须落在字符边界上且与原文对齐。

use rustmark::domain::markdown::services::SpellChecker;
use rustmark::infrastructure::spell::VendoredSpellChecker;
use rustmark::infrastructure::spell::dictionary;
use rustmark::infrastructure::spell::simple_checker::{check_english, edit_distance};

fn flagged<'a>(checker: &VendoredSpellChecker, text: &'a str) -> Vec<&'a str> {
    checker
        .check(text)
        .expect("检查不应失败")
        .iter()
        .map(|m| &text[m.range.start().value()..m.range.end().value()])
        .collect()
}

// ---------------------------------------------------------------------------
// ① 召回
// ---------------------------------------------------------------------------

#[test]
fn catches_common_typos() {
    let c = VendoredSpellChecker::new();
    let got = flagged(&c, "teh recieve seperate");
    assert_eq!(got, vec!["teh", "recieve", "seperate"]);
}

#[test]
fn catches_typo_that_is_missing_a_letter() {
    // 少字母形态：靠「候选本身 ∈ 删除变体索引」抓到（`mesage` ← `message`）。
    let c = VendoredSpellChecker::new();
    assert_eq!(
        flagged(&c, "I recieve the mesage"),
        vec!["recieve", "mesage"],
        "少字母与替换两类错词都应被抓出"
    );
}

#[test]
fn british_spelling_is_in_dictionary() {
    // `colour` 与 `color` 编辑距离为 2，超出「近邻门槛=1」，故若不在词表会被误判。
    // 这类英式/美式变体是**确定正确**的词，必须显式收进词表而非依赖门槛兜住。
    let c = VendoredSpellChecker::new();
    assert_eq!(
        flagged(&c, "the colour of the honour"),
        Vec::<&str>::new(),
        "英式拼写不应被误判"
    );
}

#[test]
fn inflected_forms_of_correct_words_are_not_flagged() {
    // 这是词干回退的核心价值：没有它，`colors` / `dropped` / `running`
    // 这类**完全正确**的词会因找不到近邻而全部标红（实测误报 53 个）。
    let c = VendoredSpellChecker::new();
    let text = "colors dropped running boxes studies tried moving";
    assert_eq!(
        flagged(&c, text),
        Vec::<&str>::new(),
        "屈折形态不应被误判：{text:?}"
    );
}

#[test]
fn whole_sentence_of_correct_english_is_clean() {
    let c = VendoredSpellChecker::new();
    let text = "The quick brown fox jumps over the lazy dog.";
    assert_eq!(flagged(&c, text), Vec::<&str>::new());
}

// ---------------------------------------------------------------------------
// ② 误报防线
// ---------------------------------------------------------------------------

#[test]
fn chinese_text_is_never_flagged() {
    // 中文按字切分、无空格，本就不属拼写范畴。
    let c = VendoredSpellChecker::new();
    assert_eq!(flagged(&c, "中文没有空格所以不检查"), Vec::<&str>::new());
    assert_eq!(
        flagged(&c, "混合 mixed 文本 text 混排"),
        Vec::<&str>::new(),
        "中英混排中的英文正确词不应被误判"
    );
}

#[test]
fn identifiers_and_acronyms_are_skipped() {
    let c = VendoredSpellChecker::new();
    // 内部大写 = 驼峰标识符；全大写 = 缩写。
    assert_eq!(flagged(&c, "camelCase ParseError"), Vec::<&str>::new());
    assert_eq!(flagged(&c, "API HTTP UTF8 JSON"), Vec::<&str>::new());
}

#[test]
fn short_tokens_are_skipped() {
    let c = VendoredSpellChecker::new();
    assert_eq!(flagged(&c, "a an is of it to"), Vec::<&str>::new());
}

#[test]
fn capitalized_words_are_skipped_to_avoid_proper_noun_false_positives() {
    // 刻意的漏判：句首 / 专有名词无法与小写形态区分，放过优于误报。
    let c = VendoredSpellChecker::new();
    assert_eq!(flagged(&c, "Teh quick Rust"), Vec::<&str>::new());
}

// ---------------------------------------------------------------------------
// ③ 坐标契约与纯函数
// ---------------------------------------------------------------------------

#[test]
fn ranges_are_sorted_and_aligned_with_source() {
    let c = VendoredSpellChecker::new();
    let text = "teh recieve seperate";
    let marks = c.check(text).expect("检查不应失败");
    let mut prev_end = 0usize;
    for m in &marks {
        let (s, e) = (m.range.start().value(), m.range.end().value());
        assert!(s >= prev_end, "区间应按起点升序且不重叠：{s} < {prev_end}");
        assert!(s < e, "区间起点须小于终点");
        assert!(text.is_char_boundary(s) && text.is_char_boundary(e));
        assert!(
            text[s..e].chars().all(|ch| ch.is_ascii_alphabetic()),
            "区间应只覆盖字母，实际 {:?}",
            &text[s..e]
        );
        prev_end = e;
    }
}

#[test]
fn multibyte_text_offsets_stay_on_char_boundaries() {
    // 中文在前、错词在后：字节偏移必须跳过中文的 3 字节 UTF-8 序列。
    let c = VendoredSpellChecker::new();
    let text = "中文前缀 teh 后缀";
    let marks = c.check(text).expect("检查不应失败");
    assert_eq!(marks.len(), 1);
    let (s, e) = (marks[0].range.start().value(), marks[0].range.end().value());
    assert_eq!(&text[s..e], "teh");
}

#[test]
fn empty_and_whitespace_input_is_noop() {
    let c = VendoredSpellChecker::new();
    assert_eq!(flagged(&c, ""), Vec::<&str>::new());
    assert_eq!(flagged(&c, "   \n\t  "), Vec::<&str>::new());
}

#[test]
fn tokenizer_isolated_from_typo_predicate() {
    // 用「全判错」的极端实现验证分词与跳过规则本身：
    // 若这里多出候选词，说明 normalize_candidate 的跳过规则有漏洞。
    let text = "The camelCase API of 中文 is ok";
    let all: Vec<&str> = check_english(text, |_| true)
        .iter()
        .map(|m| &text[m.range.start().value()..m.range.end().value()])
        .collect();
    // 只应剩下小写、无内部大写、长度 >= 3 的词：`camelCase`（内部大写）、
    // `API`（全大写）、`The`（首字母大写）、`of`（太短）、`is`（太短）、
    // `ok`（太短）全部跳过；中文不产出候选。
    assert_eq!(all, Vec::<&str>::new(), "跳过规则应滤掉全部候选");
}

#[test]
fn edit_distance_matches_known_values() {
    // 注意 `teh` → `the` 的 **Levenshtein 距离是 2**（末尾两字符互换属Damerau 距离 1）。
    // 本函数是标准 Levenshtein 实现，故此处断言 2；
    // 而检查器的删除变体索引对「易位」同样命中（两者删同一位置后相等），
    // 该能力由 `catches_common_typos` 间接覆盖。
    assert_eq!(edit_distance("teh", "the"), 2);
    // `colour` → `color` 仅需删掉 `u`，故 Levenshtein 距离是 1，
    // 正好落在近邻门槛内——但仍**显式收进词表**，因为依赖门槛兜住「正确词」
    // 是脆弱的：门槛一改，正确词就会变红。
    assert_eq!(edit_distance("colour", "color"), 1);
    assert_eq!(edit_distance("", "abc"), 3);
    assert_eq!(edit_distance("abc", "abc"), 0);
    assert_eq!(edit_distance("kitten", "sitting"), 3);
}

#[test]
fn near_neighbor_gap_is_exactly_one() {
    // 记录算法的已知边界：距离 2 的错词抓不到。
    // 这条断言的作用是「把设计取舍钉死」，让后人改动门槛时必然看到它。
    let c = VendoredSpellChecker::new();
    assert!(
        !flagged(&c, "occurance").contains(&"occurance"),
        "距离 2 的 occurance 当前应漏判（门槛=1），若将来放宽门槛请更新本断言"
    );
}

#[test]
fn dictionary_is_loadable_and_non_trivial() {
    assert!(dictionary::is_available(), "词表应可用");
    assert!(
        dictionary::word_count() > 2000,
        "词表过小会让近邻门槛形同虚设，实际 {}",
        dictionary::word_count()
    );
}

#[test]
fn large_document_is_checked_within_budget() {
    // 约 900KB：与真实大文档同量级。debug 下阈值放宽，release 下应远低于此。
    let c = VendoredSpellChecker::new();
    let big = "the quick brown fox jumps over the lazy dog. ".repeat(20_000);
    let t = std::time::Instant::now();
    let n = c.check(&big).expect("检查不应失败").len();
    let elapsed = t.elapsed();
    assert_eq!(n, 0, "全是正确单词，不应有标记");
    assert!(
        elapsed.as_millis() < 2000,
        "900KB 文档检查耗时 {}ms，超出预算",
        elapsed.as_millis()
    );
}
