//! 语法高亮适配器（S5-03）。
//!
//! # 为什么**不用** syntect 的主题（Theme）
//!
//! syntect 的经典用法是 `HighlightLines` + `Theme`：把文本切成带颜色
//! `(Style, &str)` 的片段。那套模型的前提是「渲染器直接吃 syntect 的颜色」，
//! 而本项目的领域层 [`SpanStyle`] 是**自有枚举**（Keyword / String / Comment…），
//! 具体配色由表现层（egui）按主题决定。
//!
//! 若沿用 `HighlightLines`，就等于把「颜色」这个表现层概念泄漏进领域层，
//! 还得额外内嵌主题 packdump（数百 KB）。故本实现改走
//! **scope 路径**：`ParseState::parse_line` + `ScopeStack` +
//! `ScopeSelectors`——只取「这段文本是什么语义」，不取「它是什么颜色」。
//! 这既让 infrastructure 保持唯一出口，也让 `default-themes` feature
//! 可以整个关掉，直接省下主题包体积（S5-03 的体积硬指标）。
//!
//! # 为什么 `highlight_range` 仍然从文首开始解析
//!
//! 语法是**有状态**的：Python 的三引号字符串、JS 的模板串、绝大多数语言的
//! 块注释都会跨行延续。因此若只从 `range.start` 开始解析，得到的语法上下文
//! 是错的（`range` 恰好落在块注释中间时，整段都会被当成注释）。
//!
//! 所以实现方式是：**顺序解析全文以维护正确状态，但只输出落在 `range` 内的片段**。
//! 这保证了验收标准④——「增量 `highlight_range` 与全量 `highlight`
//! 在相同行上产出一致结果」。代价是单次调用仍需扫描到 `range.end`，
//! 但换来的是**正确性**；真正的增量收益留给阶段 08（缓存 `ParseState`）。
//!
//! # 未知语言不报错
//!
//! 代码围栏里写 ```brainfuck 不该让整个预览失败。按验收标准⑤，
//! 未知语言返回 `Ok(空 spans)`，由上层降级为纯文本。

use std::str::FromStr;
use std::sync::OnceLock;

use syntect::easy::ScopeRangeIterator;
use syntect::highlighting::ScopeSelectors;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxSet};

use crate::domain::markdown::errors::HighlightError;
use crate::domain::markdown::services::SyntaxHighlighter;
use crate::domain::markdown::value_objects::{SpanStyle, SyntaxSpan};
use crate::shared::geometry::{ByteOffset, TextRange};

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
const LANGUAGE_ALIASES: &[(&str, &[&str])] = &[
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
fn style_selectors() -> &'static [(SpanStyle, ScopeSelectors)] {
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

/// 基于 syntect 的语法高亮器。
pub struct SyntectHighlighter {
    syntaxes: &'static SyntaxSet,
}

impl SyntectHighlighter {
    /// 构造高亮器（语法集全局只加载一次）。
    ///
    /// # 为什么用 `OnceLock` 而非每次 `new` 都加载
    ///
    /// `load_defaults_newlines()` 需要解压内嵌的 packdump，是启动期最贵的
    /// 单项操作之一（验收标准③要求 ≤ 30 ms）。本项目多处会各自构造高亮器
    /// （预览、导出、大纲），若每次都重新加载，30ms 会变成 90ms。
    ///
    /// # 关于「加载失败会不会 panic」
    ///
    /// syntect 的 `load_defaults_newlines()` 内部对 `include_bytes!` 的内嵌数据
    /// 直接 `unwrap`。那是**编译期常量**，加载失败等价于「产物损坏」，
    /// 不存在运行时可恢复的路径，因此这里不额外包一层 `Result`。
    pub fn new() -> Self {
        static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
        let syntaxes = SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines);
        Self { syntaxes }
    }

    /// 把语言标记解析为 syntect 语法引用。
    ///
    /// 查找顺序：显式别名表 → `find_syntax_by_token`（按扩展名或大小写无关名称）。
    /// 后者让 `rs` / `py` / `tsx` 这类常见缩写也能命中，而不必穷举。
    fn resolve(&self, lang: &str) -> Option<&syntect::parsing::SyntaxReference> {
        let key = lang.trim().to_ascii_lowercase();
        if key.is_empty() {
            return None;
        }
        if let Some((_, candidates)) = LANGUAGE_ALIASES.iter().find(|(alias, _)| *alias == key) {
            for candidate in *candidates {
                if let Some(found) = self.syntaxes.find_syntax_by_name(candidate) {
                    return Some(found);
                }
            }
        }
        self.syntaxes.find_syntax_by_token(&key)
    }

    /// 核心实现：顺序解析全文，只输出与 `clip` 相交的片段。
    ///
    /// `clip` 为 `None` 时输出全部片段。
    fn highlight_clipped(
        &self,
        text: &str,
        lang: Option<&str>,
        clip: Option<TextRange>,
    ) -> Result<Vec<SyntaxSpan>, HighlightError> {
        // `lang` 缺省或未知 → 不高亮。**不报错**，由上层降级为纯文本。
        let Some(lang) = lang else {
            return Ok(Vec::new());
        };
        let Some(syntax) = self.resolve(lang) else {
            log::warn!("未知语言「{lang}」，按纯文本处理");
            return Ok(Vec::new());
        };
        if text.is_empty() {
            return Ok(Vec::new());
        }

        // 裁剪区间：越界时**裁剪**而非报错（trait 约定）。
        let clip = clip.map(|range| {
            let start = range.start().value().min(text.len());
            let end = range.end().value().min(text.len()).max(start);
            (start, end)
        });

        let mut state = ParseState::new(syntax);
        let mut stack = ScopeStack::new();
        let mut spans: Vec<SyntaxSpan> = Vec::new();
        let mut line_start = 0usize;
        // 上一区间的 scope 栈快照与判定结果。
        //
        // 一行代码常被切成十几个区间，而相邻区间的 scope 栈往往**完全相同**
        // （例如 HTML 里连续十几个标点都停在同一个栈上）。`classify` 要遍历
        // 十余个选择器逐个 `does_match`，是本实现最热的一处；
        // 用栈快照做记忆化后，重复判定从「每区间 14 次匹配」降为「每区间 1 次短比较」。
        let mut cached_scopes: Vec<Scope> = Vec::new();
        let mut cached_style = SpanStyle::Plain;

        // 编辑器内部统一用 `\n`（`FsDocumentRepository::decode` 已归一 CRLF），
        // 故用 `load_defaults_newlines` 的语法集并按 `\n` 切分。
        for line in text.split_inclusive('\n') {
            let line_len = line.len();
            let ops = state
                .parse_line(line, self.syntaxes)
                .map_err(|e| HighlightError::new(format!("解析 {lang} 语法失败：{e}")))?;

            for (range_in_line, op) in ScopeRangeIterator::new(&ops, line) {
                if !matches!(op, syntect::parsing::ScopeStackOp::Noop) && stack.apply(op).is_err() {
                    // 栈失衡（语法定义异常）时停止本文件高亮，
                    // 已产出的片段仍然有效，不整体丢弃。
                    log::warn!("语言「{lang}」的 scope 栈失衡，提前结束高亮");
                    return Ok(spans);
                }
                // 空区间（相邻 op 位置相同）会产生，必须跳过，
                // 否则会产出零长片段，让下游的布局拿到无意义区间。
                if range_in_line.is_empty() {
                    continue;
                }
                let start = line_start + range_in_line.start;
                let end = line_start + range_in_line.end;

                if let Some((clip_start, clip_end)) = clip
                    && (end <= clip_start || start >= clip_end)
                {
                    continue;
                }
                let style = if cached_scopes.as_slice() == stack.as_slice() {
                    cached_style
                } else {
                    let resolved = classify(&stack);
                    cached_scopes.clear();
                    cached_scopes.extend_from_slice(stack.as_slice());
                    cached_style = resolved;
                    resolved
                };
                if style == SpanStyle::Plain {
                    // 无语义 → 不产出片段。渲染器本就按原文绘制，
                    // 输出 Plain 片段只会让片段数无谓膨胀。
                    continue;
                }
                push_span(&mut spans, start, end, style, line_start);
            }
            line_start += line_len;
        }
        Ok(spans)
    }
}

impl Default for SyntectHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntaxHighlighter for SyntectHighlighter {
    fn highlight(&self, text: &str, lang: Option<&str>) -> Result<Vec<SyntaxSpan>, HighlightError> {
        self.highlight_clipped(text, lang, None)
    }

    fn highlight_range(
        &self,
        text: &str,
        range: TextRange,
        lang: Option<&str>,
    ) -> Result<Vec<SyntaxSpan>, HighlightError> {
        self.highlight_clipped(text, lang, Some(range))
    }

    fn supports(&self, lang: &str) -> bool {
        self.resolve(lang).is_some()
    }
}

/// 按 scope 栈判定语义样式。
///
/// 遍历 [`style_selectors`]，取**第一个**命中的选择器。
/// 正确性依赖 [`STYLE_SELECTORS`] 的顺序（具体优先），
/// 这与 syntect 自己按主题顺序应用样式的方式一致，且顺序可被测试锁定。
fn classify(stack: &ScopeStack) -> SpanStyle {
    let scopes = stack.as_slice();
    for (style, selector) in style_selectors() {
        if selector.does_match(scopes).is_some() {
            return *style;
        }
    }
    SpanStyle::Plain
}

/// 追加片段，并合并**同一行内**相邻且同样式的区间。
///
/// # 为什么合并不能跨行
///
/// syntect 是逐行解析的，跨行的块注释会表现为「每行一个片段」。
/// 若把它们合并成一个横跨多行的片段，就会与
/// [`SyntaxHighlighter::highlight_range`] 的结果**不一致**：
/// 全量高亮得到 `[30..54]` 一个片段，而只请求第 3 行时得到 `[30..40]`——
/// 验收标准④（增量与全量一致）直接失败。
/// 因此合并只在同一行内进行，跨行片段保持各行独立。
///
/// 这也是编辑器渲染器更需要的形状：egui 逐行布局，跨行片段反而要额外拆分。
///
/// # 合并的价值
///
/// 重绘是逐帧按片段遍历的，一个 200 行的大纲若产生上千个零散片段
/// 会明显增加每帧开销；而连续同语义的片段本就是冗余的。
fn push_span(
    spans: &mut Vec<SyntaxSpan>,
    start: usize,
    end: usize,
    style: SpanStyle,
    line_start: usize,
) {
    if let Some(last) = spans.last_mut()
        && last.style() == style
        && last.range().end().value() == start
        // 上一片段必须结束在**当前行之内**，才允许合并。
        && last.range().end().value() > line_start
    {
        *last = SyntaxSpan::new(
            TextRange::new(last.range().start(), ByteOffset::new(end)),
            style,
        );
        return;
    }
    spans.push(SyntaxSpan::new(
        TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
        style,
    ));
}
