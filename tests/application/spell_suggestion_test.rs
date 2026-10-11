//! V11-07 · 拼写建议的应用层编排（「你可能想输入」）。
//!
//! # 为什么这一层需要单独一组用例
//!
//! 建议算法本身（词表扫描与排序）由 `tests/infrastructure/spell_checker_test.rs`
//! 负责；本文件验的是**编排**——表现层手里只有字节偏移，
//! 是应用层负责把它扩成「一个词」再往下问：
//!
//! 1. 偏移落在词中间 → 传下去的必须是**整词**；
//! 2. 偏移落在空白 / 中文上 → 直接返回空集，不去问检查器；
//! 3. 上限 `limit` 原样透传；
//! 4. 词典不可用时返回空集（与 `check` 同策略）。
//!
//! 用 `FakeSpellChecker` 而非真词表：这里要断言的是「传下去的词对不对」，
//! 与词典内容无关——真词表还会让这些断言随词表扩充而漂移。

use std::sync::Arc;

use rustmark::application::markdown::queries::SpellSuggestQuery;
use rustmark::application::markdown::service::MarkdownAppService;
use rustmark::domain::markdown::value_objects::Misspelling;
use rustmark::shared::geometry::{ByteOffset, TextRange};
use rustmark::shared::ids::DocumentId;
use rustmark::shared::path::FilePath;

use super::fakes::{FakeDocumentContentProvider, FakeSpellChecker};

/// 造一个只装配了拼写检查器的服务（其余依赖与本用例无关）。
fn service_with(
    spell: Arc<FakeSpellChecker>,
) -> (MarkdownAppService, Arc<FakeDocumentContentProvider>) {
    let provider = Arc::new(FakeDocumentContentProvider::new());
    let service = MarkdownAppService::new(
        Arc::new(FakeNoopParser),
        Arc::new(FakeNoopHighlighter),
        provider.clone(),
        Arc::new(super::fakes::FakeClock::new(0)),
        rustmark::application::markdown::service::DebouncePolicy::new(150),
        std::sync::Arc::new(std::sync::Mutex::new(
            rustmark::application::markdown::service::RenderCache::new(8),
        )),
    )
    .with_spell_checker(spell);
    (service, provider)
}

/// 只装配拼写检查器时，解析器 / 高亮器不会被调用，故用最小实现占位。
struct FakeNoopParser;

impl rustmark::domain::markdown::services::MarkdownParser for FakeNoopParser {
    fn parse(
        &self,
        _text: &str,
    ) -> Result<
        Vec<rustmark::domain::markdown::value_objects::RenderedBlock>,
        rustmark::domain::markdown::errors::ParseError,
    > {
        Ok(Vec::new())
    }
}

/// 同上：最小高亮器占位。
struct FakeNoopHighlighter;

impl rustmark::domain::markdown::services::SyntaxHighlighter for FakeNoopHighlighter {
    fn highlight(
        &self,
        text: &str,
        _lang: Option<&str>,
    ) -> Result<
        Vec<rustmark::domain::markdown::value_objects::SyntaxSpan>,
        rustmark::domain::markdown::errors::HighlightError,
    > {
        Ok(vec![
            rustmark::domain::markdown::value_objects::SyntaxSpan::new(
                TextRange::new(ByteOffset::new(0), ByteOffset::new(text.len())),
                rustmark::domain::markdown::value_objects::SpanStyle::Plain,
            ),
        ])
    }

    fn highlight_range(
        &self,
        text: &str,
        range: TextRange,
        lang: Option<&str>,
    ) -> Result<
        Vec<rustmark::domain::markdown::value_objects::SyntaxSpan>,
        rustmark::domain::markdown::errors::HighlightError,
    > {
        let _ = range;
        self.highlight(text, lang)
    }

    fn supports(&self, _lang: &str) -> bool {
        false
    }
}

fn insert(provider: &FakeDocumentContentProvider, doc_id: DocumentId, text: &str) {
    provider.insert(
        doc_id,
        text,
        0,
        Some(FilePath::new("/d/note.md").expect("路径应合法")),
    );
}

fn miss(start: usize, end: usize) -> Misspelling {
    Misspelling::new(TextRange::new(ByteOffset::new(start), ByteOffset::new(end)))
}

/// 主路径：偏移落在词中间 → 传下去的是**整词**（不是被切开的一截）。
#[test]
fn an_offset_inside_a_word_is_expanded_to_the_whole_word() {
    let spell = Arc::new(FakeSpellChecker::suggesting(
        vec![miss(0, 3)],
        &["the", "ten"],
    ));
    let (service, provider) = service_with(spell.clone());
    let doc_id = DocumentId::new();
    insert(&provider, doc_id, "teh quick brown");

    let got = service.spell_suggestions(SpellSuggestQuery {
        doc_id,
        offset: 1,
        limit: 5,
    });
    assert_eq!(got, vec!["the".to_string(), "ten".to_string()]);
    assert_eq!(
        spell.last_suggest(),
        Some(("teh".to_string(), 5)),
        "应用层必须把整词与上限传下去"
    );
}

/// 偏移落在**词尾之后**的空白上 → 没有词可建议，且**不打扰**检查器。
#[test]
fn an_offset_on_a_separator_yields_nothing() {
    let spell = Arc::new(FakeSpellChecker::suggesting(vec![], &["the"]));
    let (service, provider) = service_with(spell.clone());
    let doc_id = DocumentId::new();
    insert(&provider, doc_id, "teh quick");

    let got = service.spell_suggestions(SpellSuggestQuery {
        doc_id,
        offset: 3,
        limit: 5,
    });
    assert!(got.is_empty(), "空白处不该有建议");
    assert_eq!(spell.last_suggest(), None, "没有词就不该去问检查器");
}

/// 中文不是英文词的一部分：offset 落在中文上 → 空集。
///
/// 这条是「中英混排不误报」的应用层落点：若把整段中文当成一个词传下去，
/// 检查器会拿它去算编辑距离，既慢又无意义。
#[test]
fn chinese_is_not_treated_as_a_word() {
    let spell = Arc::new(FakeSpellChecker::suggesting(vec![], &["the"]));
    let (service, provider) = service_with(spell.clone());
    let doc_id = DocumentId::new();
    insert(&provider, doc_id, "中文 teh 中文");

    let got = service.spell_suggestions(SpellSuggestQuery {
        doc_id,
        offset: 0,
        limit: 5,
    });
    assert!(got.is_empty());
    assert_eq!(spell.last_suggest(), None);
}

/// 上限原样透传（菜单长度由表现层决定，应用层不擅自截断或放大）。
#[test]
fn the_limit_is_passed_through_unchanged() {
    let spell = Arc::new(FakeSpellChecker::suggesting(vec![miss(0, 3)], &["the"]));
    let (service, provider) = service_with(spell.clone());
    let doc_id = DocumentId::new();
    insert(&provider, doc_id, "teh quick");

    let _ = service.spell_suggestions(SpellSuggestQuery {
        doc_id,
        offset: 0,
        limit: 2,
    });
    assert_eq!(spell.last_suggest().map(|(_, limit)| limit), Some(2));

    let _ = service.spell_suggestions(SpellSuggestQuery {
        doc_id,
        offset: 0,
        limit: 0,
    });
    // limit = 0 时根本不该去问（问了也拿不到东西）。
    assert_eq!(
        spell.last_suggest().map(|(_, limit)| limit),
        Some(2),
        "limit=0 应短路，不再调用检查器"
    );
}

/// 词典不可用时返回空集（与 `check` 同策略：宁可没有建议，也不报错）。
#[test]
fn an_unavailable_dictionary_yields_no_suggestions() {
    let (service, provider) = service_with(Arc::new(FakeSpellChecker::unavailable()));
    let doc_id = DocumentId::new();
    insert(&provider, doc_id, "teh quick");

    let got = service.spell_suggestions(SpellSuggestQuery {
        doc_id,
        offset: 0,
        limit: 5,
    });
    assert!(got.is_empty());
}

/// 文档不存在 → 空集（不 panic）。
#[test]
fn an_unknown_document_yields_nothing() {
    let (service, _provider) =
        service_with(Arc::new(FakeSpellChecker::suggesting(vec![], &["the"])));
    let got = service.spell_suggestions(SpellSuggestQuery {
        doc_id: DocumentId::new(),
        offset: 0,
        limit: 5,
    });
    assert!(got.is_empty());
}

/// 未装配检查器 → 空集（拼写检查是可选能力）。
#[test]
fn a_service_without_a_checker_yields_nothing() {
    let provider = Arc::new(FakeDocumentContentProvider::new());
    let service = MarkdownAppService::new(
        Arc::new(FakeNoopParser),
        Arc::new(FakeNoopHighlighter),
        provider.clone(),
        Arc::new(super::fakes::FakeClock::new(0)),
        rustmark::application::markdown::service::DebouncePolicy::new(150),
        std::sync::Arc::new(std::sync::Mutex::new(
            rustmark::application::markdown::service::RenderCache::new(8),
        )),
    );
    let doc_id = DocumentId::new();
    insert(&provider, doc_id, "teh quick");
    assert!(
        service
            .spell_suggestions(SpellSuggestQuery {
                doc_id,
                offset: 0,
                limit: 5,
            })
            .is_empty()
    );
}
