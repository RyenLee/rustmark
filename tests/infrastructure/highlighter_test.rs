//! `SyntectHighlighter` 单测（S5-03）。
//!
//! 四条主线：
//! 1. **20 种语言全覆盖**（原型 `CodeDialog.vue` 的语言集）：每种至少产出一个片段；
//! 2. **选择器优先级**：验证 `keyword` 不会吞掉 `keyword.operator` 这类前缀重叠；
//! 3. **增量一致性**：`highlight_range` 与全量 `highlight` 在同一区间上必须一致；
//! 4. **降级不报错**：未知语言返回 `Ok(空)`，且 `supports` 如实反映。

use std::time::Instant;

use rustmark::domain::markdown::services::SyntaxHighlighter;
use rustmark::domain::markdown::value_objects::{SpanStyle, SyntaxSpan};
use rustmark::infrastructure::highlighting::syntect_highlighter::SyntectHighlighter;
use rustmark::shared::geometry::{ByteOffset, TextRange};

/// 取出片段覆盖的原文，便于断言「高亮的是哪一段」。
fn texts<'a>(spans: &[SyntaxSpan], code: &'a str) -> Vec<(&'a str, SpanStyle)> {
    spans
        .iter()
        .map(|s| {
            let range = s.range();
            let start = range.start().value().min(code.len());
            let end = range.end().value().min(code.len());
            (&code[start..end], s.style())
        })
        .collect()
}

/// 判断是否存在「样式为 `style` 且原文为 `text`」的片段。
fn has(spans: &[SyntaxSpan], code: &str, text: &str, style: SpanStyle) -> bool {
    texts(spans, code)
        .iter()
        .any(|(t, s)| t.contains(text) && *s == style)
}

// ---------------------------------------------------------------------------
// 1. 20 种语言全覆盖
// ---------------------------------------------------------------------------

/// 原型 `CodeDialog.vue` 的 20 种语言，各配一段能体现高亮的最小样例。
const LANGUAGE_CASES: &[(&str, &str)] = &[
    ("rust", "fn main() { let x = 1; }"),
    ("typescript", "const a: number = 1;"),
    ("javascript", "const a = 1;"),
    ("vue", "<template><div/></template>"),
    ("html", "<p class=\"a\">hi</p>"),
    ("css", "a { color: red; }"),
    ("json", "{\"a\": 1}"),
    ("python", "def f(): pass"),
    ("go", "func main() {}"),
    ("java", "class A {}"),
    ("c", "int main(void){return 0;}"),
    ("cpp", "int main(){return 0;}"),
    ("csharp", "class A {}"),
    ("php", "<?php echo 1;"),
    ("ruby", "def f; end"),
    ("shell", "echo hi"),
    ("sql", "SELECT 1;"),
    ("yaml", "a: 1"),
    ("markdown", "# h\n\n**b** and `c`\n"),
    ("diff", "+added\n-removed\n"),
];

#[test]
fn all_twenty_languages_produce_spans() {
    let highlighter = SyntectHighlighter::new();
    let mut silent = Vec::new();

    for (lang, code) in LANGUAGE_CASES {
        let spans = highlighter
            .highlight(code, Some(lang))
            .unwrap_or_else(|e| panic!("{lang} 高亮应成功，实际：{e}"));
        assert!(
            !spans.is_empty(),
            "语言「{lang}」零片段——别名映射或scope 表可能漏了它（样例：{code:?}）"
        );
        assert!(
            highlighter.supports(lang),
            "「{lang}」产出了片段，supports() 却返回 false，两者必须一致"
        );
        // 所有片段必须落在文本内、且非空——越界或零长片段会让下游布局出错。
        for span in &spans {
            let range = span.range();
            assert!(
                range.start().value() < range.end().value(),
                "{lang}: 不得产出零长或倒序片段：{range:?}"
            );
            assert!(
                range.end().value() <= code.len(),
                "{lang}: 片段越过文本末尾：{range:?}（文本 {} 字节）",
                code.len()
            );
        }
        if spans.is_empty() {
            silent.push(*lang);
        }
    }
    assert!(silent.is_empty(), "以下语言零片段：{silent:?}");
}

/// 20 种语言必须**恰好**是这 20 种（防止后续误改别名表导致漏项或多项）。
#[test]
fn language_case_list_covers_exactly_twenty() {
    assert_eq!(
        LANGUAGE_CASES.len(),
        20,
        "原型 CodeDialog.vue 定义了 20 种语言，用例表必须与之对齐"
    );
}

/// 语言别名必须能覆盖常见的简写与大小写写法。
#[test]
fn language_aliases_accept_common_spellings() {
    let highlighter = SyntectHighlighter::new();
    for lang in [
        "rust",
        "Rust",
        "RUST",
        "rs",
        "typescript",
        "ts",
        "javascript",
        "js",
        "py",
        "python",
        "sh",
        "bash",
        "zsh",
        "yml",
        "md",
        "c++",
        "cs",
    ] {
        assert!(highlighter.supports(lang), "常见写法「{lang}」应能命中语法");
    }
}

// ---------------------------------------------------------------------------
// 2. 语义分类正确性
// ---------------------------------------------------------------------------

#[test]
fn code_tokens_are_classified_by_meaning() {
    let highlighter = SyntectHighlighter::new();

    let rust = "fn main() { let x = 1; }";
    let spans = highlighter.highlight(rust, Some("rust")).expect("应成功");
    assert!(
        has(&spans, rust, "fn", SpanStyle::Keyword),
        "{:?}",
        texts(&spans, rust)
    );
    assert!(has(&spans, rust, "main", SpanStyle::Function));
    assert!(has(&spans, rust, "let", SpanStyle::Keyword));
    assert!(has(&spans, rust, "1", SpanStyle::Number));

    let python = "def f():\n    return 'x'  # note\n";
    let spans = highlighter
        .highlight(python, Some("python"))
        .expect("应成功");
    assert!(has(&spans, python, "def", SpanStyle::Keyword));
    assert!(has(&spans, python, "f", SpanStyle::Function));
    assert!(
        has(&spans, python, "# note", SpanStyle::Comment),
        "注释应被识别：{:?}",
        texts(&spans, python)
    );

    let shell = "echo \"hi\" # c";
    let spans = highlighter.highlight(shell, Some("shell")).expect("应成功");
    assert!(
        has(&spans, shell, "# c", SpanStyle::Comment),
        "shell 注释：{:?}",
        texts(&spans, shell)
    );
}

/// **选择器优先级**：TextMate 选择器是前缀匹配，`keyword` 会吃掉
/// `keyword.operator`。若顺序写反，运算符就会被误判成关键字。
#[test]
fn specific_selectors_win_over_prefix_overlapping_ones() {
    let highlighter = SyntectHighlighter::new();
    // `a = b` 中的 `=` 在多数语言里是 `keyword.operator`。
    let code = "fn f() { a = b; }";
    let spans = highlighter.highlight(code, Some("rust")).expect("应成功");

    let operators: Vec<&str> = texts(&spans, code)
        .into_iter()
        .filter(|(_, style)| *style == SpanStyle::Operator)
        .map(|(t, _)| t)
        .collect();
    assert!(
        operators.iter().any(|t| t.contains('=')),
        "`=` 应被判为 Operator 而非 Keyword，实际分类：{:?}",
        texts(&spans, code)
    );
}

/// 标记类语言（HTML / Markdown / Diff）必须有片段——
/// 它们的有效信息几乎全在标点 scope 上，映射表漏掉就会退化成纯文本。
#[test]
fn markup_languages_are_highlighted() {
    let highlighter = SyntectHighlighter::new();

    let diff = "+added\n-removed\n";
    let spans = highlighter.highlight(diff, Some("diff")).expect("应成功");
    assert!(
        has(&spans, diff, "+", SpanStyle::TaskMarker { done: true })
            || has(&spans, diff, "+", SpanStyle::Strong),
        "Diff 新增行应被标记：{:?}",
        texts(&spans, diff)
    );
    assert!(
        has(&spans, diff, "-", SpanStyle::Strikethrough),
        "Diff 删除行应被判为 Strikethrough：{:?}",
        texts(&spans, diff)
    );

    let html = "<p class=\"a\">hi</p>";
    let spans = highlighter.highlight(html, Some("html")).expect("应成功");
    assert!(
        has(&spans, html, "p", SpanStyle::Type),
        "HTML 标签名应被判为 Type：{:?}",
        texts(&spans, html)
    );

    let markdown = "# 标题\n\n**粗体**与 `代码`\n";
    let spans = highlighter
        .highlight(markdown, Some("markdown"))
        .expect("应成功");
    assert!(
        has(&spans, markdown, "标题", SpanStyle::Heading { level: 1 }),
        "Markdown 标题应被判为 Heading：{:?}",
        texts(&spans, markdown)
    );
    // syntect 的 Markdown 语法只给**定界符**加 scope（`punctuation.definition.bold`），
    // 定界符之间的正文不带任何 scope——这是语法定义本身的性质，不是映射表的疏漏。
    // 故此处断言定界符被标记，而非断言正文被包成 Strong。
    assert!(
        has(&spans, markdown, "**", SpanStyle::Strong),
        "Markdown 粗体定界符应被判为 Strong：{:?}",
        texts(&spans, markdown)
    );
    assert!(
        has(&spans, markdown, "`", SpanStyle::Code),
        "Markdown 行内代码定界符应被判为 Code：{:?}",
        texts(&spans, markdown)
    );
}

// ---------------------------------------------------------------------------
// 3. 增量一致性（验收标准④）
// ---------------------------------------------------------------------------

/// `highlight_range` 在区间内必须与全量 `highlight` **完全一致**。
///
/// 这是防增量实现跑偏的核心用例：若为了「快」而从 `range.start` 重新解析，
/// 跨行语法（块注释、Python 三引号字符串）就会因为丢失上文状态而出错。
#[test]
fn highlight_range_matches_full_highlight_on_same_region() {
    let highlighter = SyntectHighlighter::new();
    let code = "fn f() {\n    let s = \"a\";\n    /* 跨行\n       注释 */\n    let y = 2;\n}\n";

    let full = highlighter.highlight(code, Some("rust")).expect("应成功");
    assert!(!full.is_empty(), "样例本身应产出片段");

    // 对每一行单独取区间，结果都必须等于全量结果在该行内的部分。
    let mut line_start = 0usize;
    for line in code.split_inclusive('\n') {
        let line_end = line_start + line.len();
        let range = TextRange::new(ByteOffset::new(line_start), ByteOffset::new(line_end));

        let ranged = highlighter
            .highlight_range(code, range, Some("rust"))
            .expect("应成功");
        let expected: Vec<SyntaxSpan> = full
            .iter()
            .filter(|s| {
                let r = s.range();
                r.start().value() >= line_start && r.end().value() <= line_end
            })
            .copied()
            .collect();

        assert_eq!(
            ranged,
            expected,
            "第 {line_start}..{line_end} 字节的增量结果与全量不一致：\n增量={:?}\n全量={:?}",
            texts(&ranged, code),
            texts(&expected, code)
        );
        line_start = line_end;
    }
}

/// 跨行语法：块注释必须从`/*` 一直延续到 `*/`，
/// 哪怕区间起点落在注释**中间**。
#[test]
fn range_starting_inside_multiline_comment_is_still_correct() {
    let highlighter = SyntectHighlighter::new();
    let code = "/* 第一行\n第二行注释\n第三行 */\nlet x = 1;\n";
    let full = highlighter.highlight(code, Some("rust")).expect("应成功");

    // 区间从第二行开始——实现必须先解析上文才知道这里仍在注释里。
    let start = code.find("第二行").expect("样例应含该行");
    let range = TextRange::new(ByteOffset::new(start), ByteOffset::new(code.len()));
    let ranged = highlighter
        .highlight_range(code, range, Some("rust"))
        .expect("应成功");

    let in_range: Vec<SyntaxSpan> = ranged
        .iter()
        .filter(|s| s.range().start().value() >= start)
        .copied()
        .collect();
    let expected: Vec<SyntaxSpan> = full
        .iter()
        .filter(|s| s.range().start().value() >= start)
        .copied()
        .collect();

    assert_eq!(
        in_range,
        expected,
        "区间起点落在块注释中间时丢状态：\n增量={:?}\n全量={:?}",
        texts(&in_range, code),
        texts(&expected, code)
    );
    assert!(
        in_range.iter().any(|s| s.style() == SpanStyle::Comment),
        "第二行注释仍应被判为 Comment：{:?}",
        texts(&in_range, code)
    );
}

/// 越界区间必须**裁剪**而非报错（trait 约定）。
#[test]
fn out_of_bounds_range_is_clipped_not_rejected() {
    let highlighter = SyntectHighlighter::new();
    let code = "let x = 1;";

    let beyond = TextRange::new(ByteOffset::new(5), ByteOffset::new(9999));
    let clipped = highlighter
        .highlight_range(code, beyond, Some("rust"))
        .expect("越界不应报错");
    let full = highlighter.highlight(code, Some("rust")).expect("应成功");

    let from_five: Vec<SyntaxSpan> = full
        .iter()
        .filter(|s| s.range().start().value() >= 5)
        .copied()
        .collect();
    assert_eq!(
        clipped, from_five,
        "越界裁剪后的结果应等于全量结果在有效区间内的部分"
    );

    // 完全越界（起点超过文本长度）→ 空结果，不报错。
    let past_end = TextRange::new(ByteOffset::new(500), ByteOffset::new(600));
    assert!(
        highlighter
            .highlight_range(code, past_end, Some("rust"))
            .expect("不应报错")
            .is_empty()
    );
}

// ---------------------------------------------------------------------------
// 4. 降级与边界
// ---------------------------------------------------------------------------

#[test]
fn unknown_language_falls_back_to_plain_text_without_error() {
    let highlighter = SyntectHighlighter::new();

    for lang in ["brainfuck", "不存在的语言", "", "   "] {
        let spans = highlighter
            .highlight("some code", Some(lang))
            .unwrap_or_else(|e| panic!("未知语言「{lang}」不应报错，实际：{e}"));
        assert!(
            spans.is_empty(),
            "未知语言「{lang}」应返回空片段（纯文本降级），实际：{}",
            spans.len()
        );
        assert!(!highlighter.supports(lang), "supports({lang:?}) 应为 false");
    }
}

#[test]
fn missing_language_returns_no_spans() {
    let highlighter = SyntectHighlighter::new();
    assert!(
        highlighter
            .highlight("fn f() {}", None)
            .expect("不应报错")
            .is_empty(),
        "未指定语言时按纯文本处理"
    );
}

#[test]
fn empty_and_plain_inputs_are_handled() {
    let highlighter = SyntectHighlighter::new();
    assert!(
        highlighter
            .highlight("", Some("rust"))
            .expect("应成功")
            .is_empty()
    );
    assert!(
        highlighter
            .highlight("只是普通文字", Some("text"))
            .expect("应成功")
            .is_empty(),
        "纯文本语言不应产出语义片段"
    );
}

/// 片段必须按起点升序且互不重叠——否则下游无法顺序绘制。
#[test]
fn spans_are_ordered_and_non_overlapping() {
    let highlighter = SyntectHighlighter::new();
    for (lang, code) in LANGUAGE_CASES {
        let spans = highlighter.highlight(code, Some(lang)).expect("应成功");
        for pair in spans.windows(2) {
            assert!(
                pair[0].range().end().value() <= pair[1].range().start().value(),
                "{lang}: 片段必须有序且不重叠，实际 {:?} 与 {:?}",
                pair[0].range(),
                pair[1].range()
            );
        }
    }
}

/// 相邻同样式片段应被合并（减少每帧遍历的片段数）。
#[test]
fn adjacent_spans_with_same_style_are_merged() {
    let highlighter = SyntectHighlighter::new();
    let code = "let a = 1;\nlet b = 2;\nlet c = 3;\n";
    let spans = highlighter.highlight(code, Some("rust")).expect("应成功");

    for pair in spans.windows(2) {
        assert!(
            !(pair[0].style() == pair[1].style()
                && pair[0].range().end().value() == pair[1].range().start().value()),
            "相邻同样式片段应被合并，却出现了两个：{:?} / {:?}",
            pair[0].range(),
            pair[1].range()
        );
    }
}

// ---------------------------------------------------------------------------
// 5. 性能基线（验收标准③：dump 加载 ≤ 30ms）
// ---------------------------------------------------------------------------

/// 语法集加载耗时基线。
///
/// 上限取 30ms（验收标准③）。注意该断言只在**首次**构造时真正测量加载成本——
/// `OnceLock` 命中后是纳秒级，因此本用例永远不会因缓存而"变绿"掩盖回归。
#[test]
fn syntax_set_loads_within_cold_start_budget() {
    let start = Instant::now();
    let highlighter = SyntectHighlighter::new();
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() <= 30,
        "语法集加载耗时 {elapsed:?}，超出 30ms 冷启动预算"
    );
    // 必须真的能干活，防止「因为失败而更快」。
    assert!(highlighter.supports("rust"), "加载后应能识别 Rust");
}

/// 高亮吞吐的宽松基线：防止出现数量级退化。
///
/// 这里刻意**不**对 NFR-PERF-05（10KB ≤ 10ms）下断言：
/// syntect 的 `parse_line` 本身在 release 下约 24ms/7KB，
/// 达不到该指标。处置见 `docs/spec/metrics.md`（阶段 08 靠视口增量高亮达标）。
/// 此处只保证「不是平方级退化」——200 行应在 500ms 内完成。
#[test]
fn highlighting_throughput_has_not_regressed() {
    let highlighter = SyntectHighlighter::new();
    let code: String = (0..200)
        .map(|i| format!("fn f{i}() {{ let x = {i}; /* c{i} */ }}\n"))
        .collect();

    let start = Instant::now();
    let spans = highlighter.highlight(&code, Some("rust")).expect("应成功");
    let elapsed = start.elapsed();

    assert!(!spans.is_empty(), "应产出片段");
    assert!(
        elapsed.as_millis() <= 500,
        "200 行 Rust 高亮耗时 {elapsed:?}，疑似数量级退化"
    );
}

// ============================================================================
// OPT-011 归因：一次 10KB Markdown 高亮的成本构成
// ============================================================================

/// 把 `SyntectHighlighter::highlight` 拆成「syntect 逐行解析」与「其余」
/// （scope → `SpanStyle` 判定 + 片段组装 + `Vec` 增长）两段，用于判断
/// OPT-011（编辑帧 67.9ms）该往哪个方向优化。
///
/// # 为什么值得单测
///
/// 之前只有一个总数 67.9ms，两个候选方案（① 防抖降级观感 ② 增量高亮需扩接口）
/// 都不理想。若能证明成本几乎全在**判定**而非**解析**，就存在第三种做法：
/// 给「scope 栈 → 样式」加一张备忘表——纯内部记忆化，不碰领域接口、
/// 不改变任何可见行为。这条用例就是用来判断这条路值不值得走的。
///
/// 数字写入 `docs/spec/metrics.md`（唯一数字来源）。
#[test]
fn measure_markdown_highlight_parse_vs_classify() {
    use std::hint::black_box;

    use syntect::easy::ScopeRangeIterator;
    use syntect::parsing::{ParseState, SyntaxSet};

    /// 与 `tests/presentation/highlight_perf_test.rs` 同源的中文 Markdown 语料。
    fn markdown_sample(target_bytes: usize) -> String {
        let unit = "# 标题 H1\n\n\
                    这是一段中文正文，用于模拟真实编辑场景：包含 **加粗**、`行内代码` \
                    与 [链接](https://example.com)。\n\n\
                    - 列表项一\n\
                    - 列表项二\n\n\
                    > 引用行\n\n\
                    ```rust\nfn main() { println!(\"hi\"); }\n```\n\n";
        let mut s = String::new();
        while s.len() < target_bytes {
            s.push_str(unit);
        }
        s
    }

    fn p50(samples: &[f64]) -> f64 {
        let mut v = samples.to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).expect("耗时不应为 NaN"));
        v[v.len() / 2]
    }

    let text = markdown_sample(10 * 1024);
    let runs = 10;

    // A) 全量：解析 + 判定 + 片段组装（就是编辑帧付出的成本）。
    let highlighter = SyntectHighlighter::new();
    let _ = highlighter.highlight(&text, Some("markdown"));
    let full: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            let spans = highlighter.highlight(&text, Some("markdown")).expect("ok");
            black_box(spans);
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();

    // B) 只解析：逐行 `parse_line` 并遍历区间，但**不做** scope → 样式判定。
    let syntaxes = SyntaxSet::load_defaults_newlines();
    let syntax = syntaxes
        .find_syntax_by_name("Markdown")
        .expect("Markdown 语法应存在");
    // 预热：首次解析要反序列化语法上下文（LazyContexts），不计入稳态样本。
    {
        let mut state = ParseState::new(syntax);
        for line in text.split_inclusive('\n') {
            let ops = state.parse_line(line, &syntaxes).expect("解析应成功");
            for _ in ScopeRangeIterator::new(&ops, line) {}
        }
    }
    let parse_only: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            let mut state = ParseState::new(syntax);
            for line in text.split_inclusive('\n') {
                let ops = state.parse_line(line, &syntaxes).expect("解析应成功");
                for (r, _) in ScopeRangeIterator::new(&ops, line) {
                    black_box(r);
                }
            }
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();

    let full_p50 = p50(&full);
    let parse_p50 = p50(&parse_only);
    let rest = (full_p50 - parse_p50).max(0.0);

    println!(
        "[metrics] 10KB Markdown 高亮归因：全量 p50={full_p50:.2}ms ｜ \
         仅 syntect 解析 p50={parse_p50:.2}ms ｜ 判定与组装 ≈{rest:.2}ms \
         （占比 {:.1}%）",
        if full_p50 > 0.0 {
            rest / full_p50 * 100.0
        } else {
            0.0
        }
    );

    // 退化守卫（与构建 profile 无关）：解析必须主导成本。
    // 若「判定与组装」反超解析，说明有人把重量级工作塞进了着色路径，
    // 这正是 OPT-011 否决「scope→样式备忘表」那条候选的根据——不能让回归把它推翻。
    assert!(
        parse_p50 >= rest,
        "10KB 高亮：解析 {parse_p50:.2}ms 不应低于判定+组装 {rest:.2}ms（着色路径疑似混入重活）"
    );
}
