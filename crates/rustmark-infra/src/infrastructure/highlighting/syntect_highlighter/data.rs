//! `syntect_highlighter` 的数据表与选择器缓存（从主文件抽出；R-07 Batch 1）。

use rustmark_core::domain::markdown::value_objects::SpanStyle;
use std::str::FromStr;
use std::sync::OnceLock;
use syntect::highlighting::ScopeSelectors;

/// 原型 `CodeDialog.vue` 的 20 种语言 ↔ syntect 语法名映射。
///
/// # 为什么必须显式映射（而不是直接 `find_syntax_by_token`）
///
/// 三条实测得来的理由：
/// 1. 原型的 `cpp` / `csharp` 是自定义 key，syntect 里的名字是 `C++` / `C#`；
/// 2. `shell` 在 syntect 中对应两个语法（bash / POSIX sh），需要给出优先级；
/// 3. **syntect 5.3 的默认语法集里根本没有独立的 TypeScript**（实测 75 个语法，
///    `find_syntax_by_name("TypeScript")` 与 `find_syntax_by_token("typescript")`
///    都返回 `None`）。TypeScript 是 JavaScript 的超集，退回 JS 是业界通行做法，
///    若不做这层映射，TS 代码块会静默退化成纯文本。
///
/// 另有 `vue` 同样没有独立语法，退回 HTML（Vue 模板即 HTML）。
///
/// # 顺序敏感性
///
/// 候选按顺序尝试，命中即止。`Vue` 之所以放在 `HTML` 前面，
/// 是为了将来syntect 补上 Vue 语法时无需改本表即可生效。
pub(super) const LANGUAGE_ALIASES: &[(&str, &[&str])] = &[
    ("rust", &["Rust"]),
    // syntect 5.3 无 TypeScript；JS 是其真子集，语法高亮可接受。
    // `ts` / `tsx` 也不在默认语法集的扩展名里（因为压根没有 TS 语法），
    // 必须显式给出，否则最常见的 TS 简写会静默退化成纯文本。
    ("typescript", &["TypeScript", "JavaScript"]),
    ("ts", &["TypeScript", "JavaScript"]),
    ("tsx", &["TypeScript", "JavaScript"]),
    ("javascript", &["JavaScript", "JS"]),
    ("vue", &["Vue", "HTML"]),
    ("html", &["HTML"]),
    ("css", &["CSS"]),
    ("json", &["JSON"]),
    ("python", &["Python"]),
    ("go", &["Go"]),
    ("java", &["Java"]),
    ("c", &["C"]),
    ("cpp", &["C++"]),
    ("csharp", &["C#"]),
    ("php", &["PHP"]),
    ("ruby", &["Ruby"]),
    (
        "shell",
        &["Bourne Again Shell (bash)", "Shell-Unix-Generic"],
    ),
    ("bash", &["Bourne Again Shell (bash)"]),
    ("sh", &["Bourne Again Shell (bash)", "Shell-Unix-Generic"]),
    ("zsh", &["Bourne Again Shell (bash)"]),
    ("fish", &["Bourne Again Shell (bash)"]),
    ("sql", &["SQL"]),
    ("yaml", &["YAML"]),
    ("markdown", &["Markdown"]),
    ("diff", &["Diff"]),
    ("text", &["Plain Text"]),
];

/// scope 选择器 → [`SpanStyle`] 的映射表。
///
/// # 顺序即优先级（不可随意重排）
///
/// TextMate 的选择器是**前缀匹配**：`keyword` 会同时匹配 `keyword.operator`。
/// 若把 `keyword` 放在 `keyword.operator` 之前，所有运算符都会被判成关键字。
/// 故「更具体的选择器必须排在更宽泛的前面」。
///
/// 同理 `comment` / `string` 必须最先判定：字符串里可以出现关键字，
/// 注释里可以出现任何东西，若让它们被内层 scope 抢走就会高亮错误。
///
/// # 为什么含`punctuation.*` 与 `entity.name.tag.*`
///
/// 这些不是为「代码语法」加的，而是为**标记类语言**（HTML / Markdown / Diff）
/// 加的。实测（见 `tests/infrastructure/highlighter_test.rs`）这三种语言的
/// 有用信息几乎全部落在标点 scope 上：
/// - HTML：标签名是 `entity.name.tag.block.any.html`，属性分隔是
///   `punctuation.separator.key-value.html`；
/// - Markdown：标题是 `entity.name.section`，粗体/行内代码是
///   `punctuation.definition.bold.begin` / `punctuation.definition.raw.begin`；
/// - Diff：增删行只有 `punctuation.definition.inserted` / `.deleted`。
///
/// 若只映射「代码类」scope，这三种语言会输出**零片段**（实测），
/// 用户看到的就是「代码块没高亮」——功能上等同于坏了。
const STYLE_SELECTORS: &[(&str, SpanStyle)] = &[
    ("comment", SpanStyle::Comment),
    ("string", SpanStyle::String),
    ("constant.numeric", SpanStyle::Number),
    // 必须在 `keyword` 之前：`keyword.operator` 是 `keyword` 的子 scope。
    ("keyword.operator", SpanStyle::Operator),
    ("storage.type", SpanStyle::Keyword),
    ("keyword", SpanStyle::Keyword),
    ("entity.name.function", SpanStyle::Function),
    ("support.function", SpanStyle::Function),
    ("meta.function-call", SpanStyle::Function),
    ("entity.name.type", SpanStyle::Type),
    ("entity.name.class", SpanStyle::Type),
    ("support.type", SpanStyle::Type),
    ("support.class", SpanStyle::Type),
    // --- 标记类语言（HTML / Markdown / Diff）---
    // 必须排在 `variable` 之前：HTML 属性值会落在 `variable` 之类的宽泛 scope 上。
    (
        "punctuation.definition.inserted",
        SpanStyle::TaskMarker { done: true },
    ),
    ("punctuation.definition.deleted", SpanStyle::Strikethrough),
    ("punctuation.definition.bold", SpanStyle::Strong),
    ("punctuation.definition.italic", SpanStyle::Emphasis),
    ("punctuation.definition.raw", SpanStyle::Code),
    ("entity.name.tag", SpanStyle::Type),
    ("entity.name.section", SpanStyle::Heading { level: 1 }),
    ("entity.other.attribute-name", SpanStyle::Variable),
    ("variable", SpanStyle::Variable),
];

/// 编译并缓存选择器表。
///
/// `ScopeSelectors::from_str` 需要解析 scope 字符串并注册到全局 scope 仓库，
/// 放在 `OnceLock` 里只做一次——高亮是逐帧调用的路径，不能每次重新解析。
pub(super) fn style_selectors() -> &'static [(SpanStyle, ScopeSelectors)] {
    static CACHE: OnceLock<Vec<(SpanStyle, ScopeSelectors)>> = OnceLock::new();
    CACHE.get_or_init(|| {
        STYLE_SELECTORS
            .iter()
            .filter_map(|(pattern, style)| {
                ScopeSelectors::from_str(pattern)
                    .ok()
                    .map(|parsed| (*style, parsed))
            })
            .collect()
    })
}
