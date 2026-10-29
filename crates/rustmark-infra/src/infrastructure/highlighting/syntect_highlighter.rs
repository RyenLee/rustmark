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

use std::sync::OnceLock;

use syntect::easy::ScopeRangeIterator;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use rustmark_core::domain::markdown::errors::HighlightError;
use rustmark_core::domain::markdown::services::{HighlightSession, SyntaxHighlighter};
use rustmark_core::domain::markdown::value_objects::{SpanStyle, SyntaxSpan};
use rustmark_core::shared::geometry::TextRange;

use super::incremental::SyntectSession;
mod data;
mod spans;
use self::data::LANGUAGE_ALIASES;
pub(super) use spans::{classify, push_span};

/// 全局唯一的语法集（含内嵌 packdump 解压，进程内只做一次）。
///
/// 从 [`SyntectHighlighter::new`] 抽出来，是为了让同模块内的
/// [`incremental::SyntectSession`](super::incremental::SyntectSession)
/// 复用同一份实例而不是各自解压一遍。
pub(super) fn default_syntax_set() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// 把语言标记解析为 syntect 语法引用。
///
/// 查找顺序：显式别名表 → `find_syntax_by_token`（按扩展名或大小写无关名称）。
/// 后者让 `rs` / `py` / `tsx` 这类常见缩写也能命中，而不必穷举。
pub(super) fn resolve_language<'a>(
    syntaxes: &'a SyntaxSet,
    lang: &str,
) -> Option<&'a SyntaxReference> {
    let key = lang.trim().to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    if let Some((_, candidates)) = LANGUAGE_ALIASES.iter().find(|(alias, _)| *alias == key) {
        for candidate in *candidates {
            if let Some(found) = syntaxes.find_syntax_by_name(candidate) {
                return Some(found);
            }
        }
    }
    syntaxes.find_syntax_by_token(&key)
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
        Self {
            syntaxes: default_syntax_set(),
        }
    }

    /// 把语言标记解析为 syntect 语法引用。
    ///
    /// 查找顺序见 [`resolve_language`]。
    fn resolve(&self, lang: &str) -> Option<&SyntaxReference> {
        resolve_language(self.syntaxes, lang)
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

/// 预热样本清单：`(语言标记, 代表性代码)`。
///
/// 覆盖编辑区恒定的 `markdown`（`src/presentation/views/editor_view.rs:136-142`
/// 的 lang 恒为它）、测试语料涉及的 `rust`，以及高频代码围栏语言。样本里
/// `markdown` **必须带 ```rust 围栏**：syntect 的 Markdown 语法会 embed 子语法，
/// 解析该样本时顺带预热 Rust 的第②③层，与生产/测试语料一致。
///
/// 别名归并（见 [`data::LANGUAGE_ALIASES`]）：`javascript` 覆盖
/// typescript/ts/tsx（syntect 5.3 无独立 TypeScript，均归 JavaScript），
/// `bash` 覆盖 shell/sh/zsh/fish（均归 bash），`html` 覆盖 vue（无独立语法，
/// 归 HTML）。每种 1–3 行代表性代码即可：`parse_line` 的 `find_best_match`
/// 会遍历当前 context 链上的**全部** pattern 逐个 `search`，故首行解析就会
/// 编译主 context 链的绝大多数正则（第③层）；配合 `ParseState::new` 触发的
/// `lazy_contexts` 反序列化（第②层），足以把首开冷成本的主体前移。
const PREWARM_SAMPLES: &[(&str, &str)] = &[
    (
        "markdown",
        "# 标题 H1\n\n这是一段中文正文，包含 **加粗**、`行内代码` 与 [链接](https://example.com)。\n\n- 列表项一\n- 列表项二\n\n> 引用行\n\n```rust\nfn main() { println!(\"hi\"); }\n```\n\n",
    ),
    ("rust", "fn main() { let s = \"hi\"; } // note\n"),
    ("python", "def f():\n    return 'x'  # note\n"),
    // 覆盖 typescript/ts/tsx 别名（三者均归到 JavaScript 语法）。
    ("javascript", "const a = 1;\n"),
    ("json", "{\"a\": 1}\n"),
    // 覆盖 shell/sh/zsh/fish 别名（均归到 bash 语法）。
    ("bash", "echo \"hi\" # note\n"),
    // 覆盖 vue 别名（无独立语法，归到 HTML）。
    ("html", "<p class=\"a\">hi</p>\n"),
    ("css", "a { color: red; }\n"),
    ("sql", "SELECT 1;\n"),
    ("yaml", "a: 1\n"),
];

/// 预热 syntect 的 per-language lazy 层（第②层 `LazyContexts` 反序列化 +
/// 第③层正则编译），把「进程内首次真正解析」的一次性冷成本前移到启动期。
///
/// 复用 [`default_syntax_set`] 的全局 `&'static SyntaxSet`：只填充挂在
/// `SyntaxReference`/`Regex` 上的 `OnceCell`，**不 clone 任何语法集**
/// （syntect 的 `Regex::clone` 会清空已编译正则），不改变任何解析结果——
/// lazy 层是纯记忆化。由 root 启动路径在装配后、建窗口前用后台线程调用，
/// 与 eframe 建窗口（实测 ~682ms）并发跑。
///
/// 走生产同一条 `highlight` 路径：resolve → `ParseState::new`（②）→
/// `parse_line`（③）→ classify，避免「预热 A 路径但生产走 B 路径」。
/// 未知语言/解析失败按纯文本降级、尽力而为：预热失败不影响功能，只是没优化到。
pub fn prewarm_highlighting() {
    let highlighter = SyntectHighlighter::new(); // OnceLock 命中，~100ns，不重新加载
    for (lang, sample) in PREWARM_SAMPLES {
        let _ = highlighter.highlight(sample, Some(lang));
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

    /// 增量会话：编辑区连续编辑时只重解变更行（见 [`SyntectSession`]）。
    ///
    /// 返回会话而非直接开算：会话是空的，第一次 `advance` 才做全量，
    /// 调用方不必为「创建会话」多付一次全文解析。
    fn start_session(&self) -> Option<Box<dyn HighlightSession>> {
        Some(Box::new(SyntectSession::new()))
    }
}

#[cfg(test)]
mod tests {
    use rustmark_core::domain::markdown::services::SyntaxHighlighter;

    use super::{PREWARM_SAMPLES, SyntectHighlighter, prewarm_highlighting};

    /// P3 最关键回归守卫：预热只填 `OnceCell`，绝不能改变解析结果。
    ///
    /// 对 `PREWARM_SAMPLES` 每样本，先记录一次 `highlight` 结果（进程内首调会
    /// 现场反序列化 lazy 层，即**冷**结果），调 `prewarm_highlighting()`，再记录
    /// 一遍（**热**结果），断言逐片段完全一致（`Vec<SyntaxSpan>` 的 range + style），
    /// 并断言 `supports` 没变 false。这同时证明「冷/热解析输出相同」（lazy 是纯
    /// 记忆化）且「调用 prewarm 不会污染共享的 `&'static SyntaxSet`」。
    #[test]
    fn prewarm_does_not_change_highlight_results() {
        let before: Vec<_> = PREWARM_SAMPLES
            .iter()
            .map(|(lang, sample)| {
                SyntectHighlighter::new()
                    .highlight(sample, Some(lang))
                    .expect("预热前解析应成功")
            })
            .collect();

        prewarm_highlighting();

        for (i, (lang, sample)) in PREWARM_SAMPLES.iter().enumerate() {
            let after = SyntectHighlighter::new()
                .highlight(sample, Some(lang))
                .expect("预热后解析应成功");
            assert_eq!(
                before[i], after,
                "预热改变了语言「{lang}」的解析结果（逐片段 range+style 必须完全一致）"
            );
            assert!(
                SyntectHighlighter::new().supports(lang),
                "预热后语言「{lang}」的 supports() 不应变 false"
            );
        }
    }

    /// 覆盖性：预热后清单每个语言都应能被识别（抓语言标记写错 / 别名映射漏）。
    #[test]
    fn prewarm_covers_all_listed_languages() {
        prewarm_highlighting();
        let highlighter = SyntectHighlighter::new();
        for (lang, _) in PREWARM_SAMPLES {
            assert!(
                highlighter.supports(lang),
                "预热清单里的语言「{lang}」必须可被识别（标记写错或别名映射漏）"
            );
        }
    }
}
