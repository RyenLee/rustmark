//! 英文词表（vendored数据，非依赖）。
//!
//! # 为什么自带词表而不引`hunspell` / `words` crate
//!
//! ADR-001 要求安装包≤ 10 MB，且本项目坚持「不引入未确认依赖」。
//! `hunspell` 需要系统 C 库（跨平台编译麻烦），`words` crate 体积可观，
//! 二者都超出「路线①零依赖」的边界。自带词表是**数据**而非依赖，不增加编译期成本。
//!
//! # 词表来源与取舍
//!
//! 词表为**高频英文词**（约 3000 词，按使用频率精选），而非全量词典。
//! 这是刻意的取舍：小词表 + 「必须有近邻词才判错」的门槛（见
//! [`super::simple_checker`]）= **低误报**。全量词典会把
//! `Kubernetes`、`Zhihu` 这类专有名词全标红，用户会直接关掉该功能——
//! 误报比漏报更伤体验。
//!
//! 词表以空白分隔的 `const str` 内嵌，`include_str!` 一个纯文本资源文件，
//! 便于后续替换为更大的词表而无需改代码。

/// 词表资源文件（纯文本，空白分隔，全小写）。
const WORDS_RAW: &str = include_str!("words.txt");

/// 解析后的词表（`OnceLock`保证只构建一次，且免去每帧`HashSet` 重建）。
///
/// 用 `OnceLock` 而非 `lazy_static` / `once_cell`：标准库自带，无依赖。
fn words() -> &'static std::collections::HashSet<&'static str> {
    static SET: std::sync::OnceLock<std::collections::HashSet<&'static str>> =
        std::sync::OnceLock::new();
    SET.get_or_init(|| WORDS_RAW.split_whitespace().collect())
}

/// 词表是否可用。
///
/// 现在恒为 `true`（词表内嵌且 `include_str!` 在编译期校验存在），
/// 但保留该函数是为了让 [`crate::domain::markdown::services::SpellChecker`]
/// 的「可用性」契约有真实落点，而非永远返回 `true` 的死代码。
pub fn is_available() -> bool {
    !words().is_empty()
}

/// 遍历全部词表词（构建近邻索引用）。
pub(crate) fn iter_words() -> impl Iterator<Item = &'static str> {
    words().iter().copied()
}

/// 单词是否在词表中（大小写不敏感）。
pub(crate) fn contains(word: &str) -> bool {
    words().contains(word)
}

/// 词表词数（诊断 / 测试用）。
pub fn word_count() -> usize {
    words().len()
}
