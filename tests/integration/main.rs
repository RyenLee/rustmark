//! 基础设施 golden 集成测试（S5-12）。
//!
//! # 为什么要 golden（快照）而不是逐条断言
//!
//! 解析 / 高亮 / 导出的输出是**结构化且冗长**的。逐条断言会写成
//! 「断言块数= 17、块 3 是 H2、块 7 的区间是 120..145」——
//! 语料一改，这些魔法数字全要跟着改，且没人能看出改动是否合理。
//! 快照把整个输出一次性固化，diff 直接呈现「变了什么」。
//!
//! # 快照更新纪律
//!
//! 快照变了**不等于**代码错了。必须先人工审阅 diff 确认变化符合预期，
//! 再用 `UPDATE_GOLDEN=1 cargo test --test integration` 显式更新，
//! 并在 PR 描述里写明变更原因（见 `CONTRIBUTING.md`）。
//! 绝不能习惯性 `--accept` 后不看内容。

// V11-65：workspace lints 为包级声明，覆盖本测试 crate（原先仅 lib 目标
// 受 lib.rs attribute 管辖）。规范 §七-20 允许测试代码 unwrap/expect/
// panic，同 lib.rs 的 `cfg_attr(test, allow)` 惯例在此豁免。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::path::PathBuf;
use std::time::Instant;

use rustmark::domain::export::aggregate::ExportJob;
use rustmark::domain::export::services::Exporter;
use rustmark::domain::export::value_objects::{ExportFormat, ExportOptions};
use rustmark::domain::markdown::services::{MarkdownParser, SyntaxHighlighter};
use rustmark::domain::markdown::value_objects::SpanStyle;
use rustmark::infrastructure::export::pulldown_html_exporter::PulldownHtmlExporter;
use rustmark::infrastructure::highlighting::syntect_highlighter::SyntectHighlighter;
use rustmark::infrastructure::parsing::pulldown_parser::PulldownMarkdownParser;
use rustmark::shared::ids::{DocumentId, JobId};
use rustmark::shared::path::FilePath;

#[path = "../common/golden_fixture.rs"]
mod golden_fixture;

#[path = "../support.rs"]
mod support;

// 应用层测试替身（S8-05 编辑区高亮帧测量需要注入 provider / clock）。
// 该文件自带一份 `mod support`，与上面的声明重复加载同一路径；
// 两份实例互不干涉，已在 `fakes.rs` 顶部统一 allow `duplicate_mod`。
#[path = "../common/fakes.rs"]
mod fakes;

// S7-02 行为对齐矩阵 · S7-03 原型缺陷回归（自动化断言）。
#[path = "alignment_test.rs"]
mod alignment_test;

#[path = "regression_test.rs"]
mod regression_test;

// S7-04 大文件与边界场景验证。
#[path = "large_file_test.rs"]
mod large_file_test;

// S7-06 异常退出与草稿恢复（原子保存控制流 / 自动保存计时逻辑 / 草稿数据层）。
#[path = "crash_recovery_test.rs"]
mod crash_recovery_test;

// S8-06 预览防抖（headless 驱动真实帧循环数重解析次数）。
#[path = "preview_debounce_test.rs"]
mod preview_debounce_test;

// S?-P2 拖拽打开文件（headless 模拟 `RawInput.dropped_files` 验证端到端打开）。
#[path = "drag_open_test.rs"]
mod drag_open_test;

/// golden 语料目录。
fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join("golden")
}

/// 读取一份golden 语料。
fn read_golden(name: &str) -> String {
    let path = golden_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("应能读取语料 {}：{e}", path.display()))
}

/// 快照文件的落盘位置。
fn snapshot_path(case: &str) -> PathBuf {
    golden_dir().join(format!("{case}.expected"))
}

/// 比较实际输出与快照。
///
/// - `UPDATE_GOLDEN=1` 时**重写**快照并返回（用于有意更新）；
/// - 相等时通过；
/// - 不等时打印 diff 片段并失败——**绝不自动接受**。
fn assert_snapshot(case: &str, actual: &str) {
    let path = snapshot_path(case);

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::write(&path, actual).expect("应能写入快照");
        eprintln!("[golden] 已更新 {case}");
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "缺少快照 {}：{e}\n请先审阅输出，再用 `UPDATE_GOLDEN=1` 生成。",
            path.display()
        )
    });

    if expected != actual {
        // 打印首个差异行，避免整篇刷屏。
        let mut diff = Vec::new();
        for (i, (e, a)) in expected.lines().zip(actual.lines()).enumerate() {
            if e != a {
                diff.push(format!("  第 {} 行:\n    期望: {e}\n    实际: {a}", i + 1));
                if diff.len() >= 5 {
                    break;
                }
            }
        }
        // 行数不同也算差异
        let expected_lines = expected.lines().count();
        let actual_lines = actual.lines().count();
        if expected_lines != actual_lines {
            diff.push(format!(
                "  行数不同：期望 {expected_lines}，实际 {actual_lines}"
            ));
        }
        panic!("快照 {case} 不匹配：\n{}", diff.join("\n"));
    }
}

/// 把解析结果渲染成稳定的文本快照。
fn render_blocks_snapshot(markdown: &str) -> String {
    let parser = PulldownMarkdownParser::new();
    let blocks = parser.parse(markdown).expect("解析应成功");

    let mut out = String::new();
    out.push_str(&format!("块总数: {}\n", blocks.len()));
    for block in blocks.iter() {
        let range = block.source_range();
        out.push_str(&format!(
            "[#{:?}] kind={:?} range={}..{} | {:?}\n",
            block.id(),
            block.kind(),
            range.start().value(),
            range.end().value(),
            block.content().chars().take(40).collect::<String>()
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// 解析 golden
// ---------------------------------------------------------------------------

#[test]
fn gfm_full_corpus_parses_to_stable_snapshot() {
    let markdown = read_golden("gfm-full.md");
    assert_snapshot("gfm-full-blocks", &render_blocks_snapshot(&markdown));
}

#[test]
fn chinese_corpus_parses_to_stable_snapshot() {
    let markdown = read_golden("chinese-mixed.md");
    assert_snapshot("chinese-mixed-blocks", &render_blocks_snapshot(&markdown));
}

/// 块区间的不变量。
///
/// # 注意：**不要**断言「所有块区间互不重叠」
///
/// 解析器输出的是**树**：`Quote` 块里含 `Paragraph`、外层 `List` 里含嵌套 `List`，
/// 因此父子块的区间**本来就重叠**（实测 gfm 语料里 `Quote 35..231` 含
/// `Paragraph 37..231`）。这不是缺陷——`BlockMapper::blocks_in_range`
/// 用「有交集」判定重渲染范围，父子同时命中恰恰是正确的。
///
/// 真正必须成立的不变量是：区间在文档内、不倒序、非空、且两端都是 UTF-8 字符边界
/// （后者是字节偏移实现最容易出错的地方——中文占 3 字节，切在中间会 panic）。
#[test]
fn all_block_ranges_are_within_bounds_and_on_char_boundaries() {
    for name in ["gfm-full.md", "chinese-mixed.md"] {
        let markdown = read_golden(name);
        let parser = PulldownMarkdownParser::new();
        let blocks = parser.parse(&markdown).expect("解析应成功");
        let len = markdown.len();

        for block in blocks.iter() {
            let range = block.source_range();
            let start = range.start().value();
            let end = range.end().value();

            assert!(start <= end, "{name}: 区间倒序 {start}..{end}");
            assert!(
                end <= len,
                "{name}: 区间越过文档末尾（{len}）：{start}..{end}"
            );
            assert!(
                start < end,
                "{name}: 零长区间 {start}..{end}（kind={:?}）",
                block.kind()
            );
            assert!(
                markdown.is_char_boundary(start),
                "{name}: 起点 {start} 不是字符边界（中文占 3 字节，切错会 panic）"
            );
            assert!(
                markdown.is_char_boundary(end),
                "{name}: 终点 {end} 不是字符边界"
            );
            // 区间可切片本身即证明两端都是合法边界。
            let _ = &markdown[start..end];
        }
    }
}

/// 块内容与区间的一致性。
///
/// # 为什么比较前要 `trim_end`
///
/// `source_range` 会**裁掉尾随换行**（这是刻意的：pulldown 的 End 偏移含块后
/// `\n`，不裁会让相邻块区间重叠，进而让 `BlockMapper::blocks_in_range`
/// 把无关块纳入重渲染）。而 `content` 仍保留那个换行。
/// 因此「content 一定短于切片」这个不变量要在去掉尾部空白后成立。
#[test]
fn block_content_comes_from_within_its_range() {
    for name in ["gfm-full.md", "chinese-mixed.md"] {
        let markdown = read_golden(name);
        let parser = PulldownMarkdownParser::new();
        let blocks = parser.parse(&markdown).expect("解析应成功");

        for block in blocks.iter() {
            let content = block.content();
            if content.trim().is_empty() {
                continue;
            }
            let range = block.source_range();
            let slice = &markdown[range.start().value()..range.end().value()];
            let content_chars = content.trim_end().chars().count();
            assert!(
                content_chars <= slice.chars().count(),
                "{name}: 块内容({content_chars} 字符)长于其源码区间({} 字符)，区间可能算错\n内容={content:?}\n切片={slice:?}",
                slice.chars().count()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 高亮 golden
// ---------------------------------------------------------------------------

/// 把高亮结果渲染成稳定快照。
fn render_highlight_snapshot(code: &str, lang: &str) -> String {
    let highlighter = SyntectHighlighter::new();
    let spans = highlighter
        .highlight(code, Some(lang))
        .unwrap_or_else(|e| panic!("{lang} 高亮应成功：{e}"));

    let mut out = String::new();
    out.push_str(&format!("片段数: {}\n", spans.len()));
    for span in spans.iter() {
        let range = span.range();
        let start = range.start().value().min(code.len());
        let end = range.end().value().min(code.len());
        out.push_str(&format!(
            "{}..{} {:?} {:?}\n",
            start,
            end,
            span.style(),
            &code[start..end]
        ));
    }
    out
}

#[test]
fn highlight_output_is_stable_across_languages() {
    // 覆盖 20 种语言中最有代表性的若干种（完整 20 种覆盖见 highlighter_test.rs）。
    let cases: &[(&str, &str, &str)] = &[
        (
            "rust",
            "rust",
            "fn main() {\n    let x: i32 = 1; // 注释\n    println!(\"{}\", x);\n}\n",
        ),
        (
            "python",
            "python",
            "def f(x):\n    '''doc'''\n    return x + 1  # 加一\n",
        ),
        (
            "javascript",
            "javascript",
            "const a = `t${1}`;\n/* 块注释 */\nconsole.log(a);\n",
        ),
        ("json", "json", "{\"a\": [1, 2.5, null], \"b\": true}\n"),
        ("yaml", "yaml", "key: value\nlist:\n  - a\n  - b\n"),
        ("html", "html", "<div class=\"a\">text</div>\n"),
        ("diff", "diff", "+added\n-removed\n context\n"),
        ("markdown", "markdown", "# 标题\n\n**粗体**与 `代码`\n"),
        (
            "shell",
            "shell",
            "if [ -f \"$1\" ]; then\n  echo \"# not a comment\"\nfi\n",
        ),
        (
            "cpp",
            "cpp",
            "template<typename T>\nT max(T a, T b) { return a > b ? a : b; }\n",
        ),
    ];

    for (case, lang, code) in cases {
        assert_snapshot(
            &format!("highlight-{case}"),
            &render_highlight_snapshot(code, lang),
        );
    }
}

/// 同一输入两次高亮必须完全一致（无隐藏状态 / 无哈希序不确定性）。
#[test]
fn highlighting_is_deterministic() {
    let highlighter = SyntectHighlighter::new();
    let code = "fn f() { let s = \"x\"; /* c */ }\n";

    let first = highlighter.highlight(code, Some("rust")).expect("应成功");
    for _ in 0..5 {
        let again = highlighter.highlight(code, Some("rust")).expect("应成功");
        assert_eq!(first, again, "高亮结果必须确定性可复现");
    }

    // 全新实例（命中 OnceLock 缓存）也必须一致。
    let fresh = SyntectHighlighter::new();
    assert_eq!(
        first,
        fresh.highlight(code, Some("rust")).expect("应成功"),
        "不同实例间结果也必须一致"
    );
}

/// 高亮结果的 span 样式必须来自领域层自有枚举，不得泄漏 syntect 类型。
#[test]
fn highlight_styles_are_domain_owned_only() {
    let highlighter = SyntectHighlighter::new();
    let spans = highlighter
        .highlight("fn f() { let s = \"a\"; }", Some("rust"))
        .expect("应成功");

    // 显式匹配穷举领域枚举——若日后syntect 类型混进来，这里会编译失败。
    for span in spans.iter() {
        let style = span.style();
        let name = match style {
            SpanStyle::Plain => "Plain",
            SpanStyle::Heading { .. } => "Heading",
            SpanStyle::Emphasis => "Emphasis",
            SpanStyle::Strong => "Strong",
            SpanStyle::Strikethrough => "Strikethrough",
            SpanStyle::Code => "Code",
            SpanStyle::Link => "Link",
            SpanStyle::Image => "Image",
            SpanStyle::Quote => "Quote",
            SpanStyle::ListMarker => "ListMarker",
            SpanStyle::TableBorder => "TableBorder",
            SpanStyle::TaskMarker { .. } => "TaskMarker",
            SpanStyle::Keyword => "Keyword",
            SpanStyle::String => "String",
            SpanStyle::Comment => "Comment",
            SpanStyle::Number => "Number",
            SpanStyle::Function => "Function",
            SpanStyle::Type => "Type",
            SpanStyle::Variable => "Variable",
            SpanStyle::Operator => "Operator",
        };
        assert!(!name.is_empty());
    }
}

// ---------------------------------------------------------------------------
// HTML 导出 golden
// ---------------------------------------------------------------------------

/// 导出到内存并返回 HTML 文本（通过临时文件，因为导出器只写文件）。
fn export_html(markdown: &str, case: &str, include_toc: bool) -> String {
    let dir = std::env::temp_dir().join("rustmark-golden-export");
    std::fs::create_dir_all(&dir).expect("应能创建临时目录");
    let target = dir.join(format!("{case}.html"));

    let path = FilePath::new(target.to_string_lossy().replace('\\', "/")).expect("路径应合法");
    let job = ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        path,
        ExportFormat::Html,
        ExportOptions::new(include_toc, true, "default", Default::default(), true),
    );
    PulldownHtmlExporter::new()
        .export(markdown, &job)
        .unwrap_or_else(|e| panic!("导出应成功：{e}"));

    std::fs::read_to_string(&target).expect("应能读回导出文件")
}

/// 导出结果的快照需要**抹掉**不确定的部分：
/// `<title>` 含文件名（固定），但样式块可能随 `PAGE_STYLE` 改动而变，
/// 故快照只取「结构 + 正文 + TOC」三部分，把样式单独断言。
fn normalize_export(html: &str) -> String {
    let mut out = String::new();
    if html.starts_with("<!DOCTYPE html>") {
        out.push_str("[DOCTYPE ok]\n");
    }
    if html.contains("<meta charset=\"utf-8\">") {
        out.push_str("[charset ok]\n");
    }
    out.push_str("[含 style 块]\n");

    // 截取 <body> 部分
    if let Some(start) = html.find("<body>") {
        out.push_str(&html[start..]);
    } else {
        out.push_str("[缺少 body]");
    }
    out
}

#[test]
fn html_export_of_gfm_corpus_is_stable_snapshot() {
    let markdown = read_golden("gfm-full.md");
    let html = export_html(&markdown, "gfm-full", false);
    assert_snapshot("gfm-full-export", &normalize_export(&html));
}

#[test]
fn html_export_with_toc_is_stable_snapshot() {
    let markdown = read_golden("chinese-mixed.md");
    let html = export_html(&markdown, "chinese-toc", true);
    assert_snapshot("chinese-mixed-export-toc", &normalize_export(&html));
}

/// 导出必须**保留样式**——这条专门防止有人为了「安全」把骨架也送进清洗器。
#[test]
fn exported_html_keeps_charset_and_style() {
    let html = export_html("# 标题\n\n正文", "style-check", false);
    assert!(html.contains("<style>"), "导出必须保留内联样式块");
    assert!(html.contains("font-family"), "样式块内容不应为空");
    assert!(
        html.contains("<meta charset=\"utf-8\">"),
        "导出必须保留 UTF-8 声明"
    );
}

// ---------------------------------------------------------------------------
// 性能基线（写入 docs/metrics.md 的数字来源）
// ---------------------------------------------------------------------------

#[test]
fn measure_10kb_parse_time() {
    // 用中文语料拼到约 10KB。
    let base = read_golden("chinese-mixed.md");
    let mut markdown = base.clone();
    while markdown.len() < 10 * 1024 {
        markdown.push('\n');
        markdown.push_str(&base);
    }
    let markdown: String = markdown.chars().take(markdown.len()).collect();

    let parser = PulldownMarkdownParser::new();
    // 预热一次，避免把首次分配的耗时算进去。
    let _ = parser.parse(&markdown).expect("应成功");

    let mut samples = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        let blocks = parser.parse(&markdown).expect("应成功");
        let elapsed = start.elapsed();
        std::hint::black_box(blocks);
        samples.push(elapsed.as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).expect("耗时不应为 NaN"));
    let p50 = samples[samples.len() / 2];
    let p95 = samples[(samples.len() as f64 * 0.95) as usize];

    println!(
        "[metrics] 10KB 解析：p50={p50:.2}ms p95={p95:.2}ms（{} 字节）",
        markdown.len()
    );
    // 宽松上限：防止数量级退化（目标 5ms 见 docs/metrics.md）。
    assert!(
        p95 <= 50.0,
        "10KB 解析 p95 {p95:.2}ms 疑似退化（目标 ≤5ms）"
    );
}

#[test]
fn measure_large_file_parse_time() {
    let markdown = golden_fixture::load_or_generate_large();
    assert!(markdown.len() >= 1024 * 1024, "样本应达 1MB");

    let parser = PulldownMarkdownParser::new();
    let _ = parser.parse(&markdown).expect("应成功");

    let mut samples = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let blocks = parser.parse(&markdown).expect("应成功");
        let elapsed = start.elapsed();
        std::hint::black_box(blocks);
        samples.push(elapsed.as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).expect("耗时不应为 NaN"));
    let p50 = samples[samples.len() / 2];

    println!(
        "[metrics] 1MB 解析：p50={p50:.2}ms（{} 字节）",
        markdown.len()
    );
    assert!(p50 <= 2000.0, "1MB 解析 p50 {p50:.2}ms 疑似退化");
}

#[test]
fn measure_syntax_set_load_time() {
    let start = Instant::now();
    let highlighter = SyntectHighlighter::new();
    let load = start.elapsed();

    let start = Instant::now();
    let spans = highlighter
        .highlight("fn main() {}", Some("rust"))
        .expect("应成功");
    let first = start.elapsed();
    std::hint::black_box(spans);

    println!("[metrics] 语法集冷加载：{load:?}；首次高亮：{first:?}");

    assert!(load.as_millis() <= 30, "语法集加载 {load:?} 超出 30ms 预算");
}

#[test]
fn measure_twenty_language_highlight_time() {
    // 原型 CodeDialog.vue 的 20 种语言，各跑一段代表性代码。
    let cases: &[(&str, &str)] = &[
        ("rust", "fn main() {}"),
        ("typescript", "const a: number = 1;"),
        ("javascript", "const a = 1;"),
        ("vue", "<template><div/></template>"),
        ("html", "<p class=\"a\">x</p>"),
        ("css", "a { color: red; }"),
        ("json", "{\"a\":1}"),
        ("python", "def f(): pass"),
        ("go", "func main() {}"),
        ("java", "class A {}"),
        ("c", "int main(){return 0;}"),
        ("cpp", "int main(){return 0;}"),
        ("csharp", "class A {}"),
        ("php", "<?php echo 1;"),
        ("ruby", "def f; end"),
        ("shell", "echo hi"),
        ("sql", "SELECT 1;"),
        ("yaml", "a: 1"),
        ("markdown", "# h"),
        ("diff", "+a\n-b"),
    ];
    let highlighter = SyntectHighlighter::new();

    // **冷**首调：每个语法首次使用时，syntect 要现场反序列化它的
    // 上下文（`LazyContexts`）。这 20 个语法的冷启动合计在数百毫秒量级，
    // 是「用户第一次滚到某个代码块」的延迟，不等同于稳态高亮耗时。
    let cold_start = Instant::now();
    let mut zero_spans = Vec::new();
    for (lang, code) in cases {
        let spans = highlighter.highlight(code, Some(lang)).expect("应成功");
        if spans.is_empty() {
            zero_spans.push(*lang);
        }
    }
    let cold = cold_start.elapsed();

    // **热**调用：上下文已缓存，这才是逐帧重绘的真实成本。
    let warm_start = Instant::now();
    for _ in 0..10 {
        for (lang, code) in cases {
            let spans = highlighter.highlight(code, Some(lang)).expect("应成功");
            std::hint::black_box(spans);
        }
    }
    let warm_total = warm_start.elapsed();
    let warm_per_lang = warm_total.as_micros() / (cases.len() as u128 * 10);

    println!(
        "[metrics] 20 语言：冷首调合计 {cold:?}；热调用 {warm_per_lang}µs/语言（{warm_total:?} / 200 次）"
    );

    assert!(zero_spans.is_empty(), "以下语言零片段：{zero_spans:?}");
    // 热路径才是交互相关的那一个：单语言应远低于一帧预算（16ms）。
    assert!(
        warm_per_lang <= 1000,
        "热路径单语言 {warm_per_lang}µs，超出 1ms（帧预算 16ms）"
    );
}

/// 编辑区「每帧请求全文 span」的真实成本（S8-05，OPT-011 ① 后重测）。
///
/// 四种帧各测一次，因为它们的成本差着三个数量级：
///
/// - **冷**：首次见该文档，无缓存（打开 / 切换文档时付一次）；
/// - **空闲帧**：文档未变（滚动 / 空闲 / 切换面板）→ 命中高亮缓存；
/// - **打字帧**：内容刚变、仍在 150ms 防抖窗口内 → **不进高亮器**（OPT-011 ①）；
/// - **补算帧**：停手满 150ms 后 → 一次性上色，这是防抖的代价所在。
///
/// 数字写入 `docs/metrics.md`。
#[test]
fn measure_editor_frame_highlight_cost() {
    use std::sync::{Arc, Mutex};

    use rustmark::application::markdown::queries::HighlightQuery;
    use rustmark::application::markdown::service::{
        DebouncePolicy, MarkdownAppService, RENDER_CACHE_CAPACITY, RenderCache,
    };
    use rustmark::shared::geometry::{ByteOffset, TextRange};

    let base = "# 标题 H1\n\n这是一段中文正文，包含 **加粗**、`行内代码` 与 [链接](https://example.com)。\n\n- 列表项一\n- 列表项二\n\n> 引用行\n\n```rust\nfn main() { println!(\"hi\"); }\n```\n\n";
    let mut text = String::new();
    while text.len() < 10 * 1024 {
        text.push_str(base);
    }

    let provider = Arc::new(fakes::FakeDocumentContentProvider::new());
    let doc_id = DocumentId::new();
    provider.insert(doc_id, &text, 0, Some(FilePath::new("/d/a.md").unwrap()));

    let clock = Arc::new(fakes::FakeClock::new(0));
    let service = MarkdownAppService::new(
        Arc::new(PulldownMarkdownParser::new()),
        Arc::new(SyntectHighlighter::new()),
        provider.clone(),
        clock.clone(),
        DebouncePolicy::new(150),
        Arc::new(Mutex::new(RenderCache::new(RENDER_CACHE_CAPACITY))),
    );

    let hq = |len: usize| HighlightQuery {
        doc_id,
        range: TextRange::new(ByteOffset::new(0), ByteOffset::new(len)),
        lang: Some("markdown".to_string()),
    };

    // 1) 冷启动：首次见该文档（打开 / 切换文档），必须立刻上色。
    let cold = {
        let start = Instant::now();
        let v = service.highlight(hq(text.len())).expect("高亮应成功");
        assert!(!v.is_empty(), "首次见该文档不得被防抖拦下");
        std::hint::black_box(v);
        start.elapsed().as_secs_f64() * 1000.0
    };

    // 2) 空闲帧：文档未变，连测 20 帧。
    clock.set(1_000);
    let mut idle = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        let v = service.highlight(hq(text.len())).expect("高亮应成功");
        std::hint::black_box(v);
        idle.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    // 3) 打字帧：每 60ms 一个字符，共 20 个（模拟连续输入）。
    let mut typing = Vec::new();
    let mut last_edit_ms = 1_000u64;
    for i in 0..20 {
        let mut next = text.replace("列表项一", &format!("列表项{i}"));
        next.push('！');
        last_edit_ms += 60;
        clock.set(last_edit_ms);
        provider.insert(
            doc_id,
            &next,
            (i + 1) as u64,
            Some(FilePath::new("/d/a.md").unwrap()),
        );
        let start = Instant::now();
        let v = service.highlight(hq(next.len())).expect("高亮应成功");
        assert!(v.is_empty(), "打字期内应不上色（第 {i} 帧）");
        std::hint::black_box(v);
        typing.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    // 4) 补算帧：停手满 150ms 后一次性上色（防抖的代价，只付一次）。
    clock.set(last_edit_ms + 200);
    let settle = {
        let start = Instant::now();
        let v = service.highlight(hq(text.len() + 20)).expect("高亮应成功");
        assert!(!v.is_empty(), "停手后必须补上颜色");
        std::hint::black_box(v);
        start.elapsed().as_secs_f64() * 1000.0
    };

    // 补算之后再来的帧应全部命中缓存。
    let mut after = Vec::new();
    for t in [last_edit_ms + 300, last_edit_ms + 400, last_edit_ms + 500] {
        clock.set(t);
        let start = Instant::now();
        let v = service.highlight(hq(text.len() + 20)).expect("高亮应成功");
        std::hint::black_box(v);
        after.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    let pct = |mut v: Vec<f64>, q: f64| -> f64 {
        v.sort_by(|a, b| a.partial_cmp(b).expect("NaN"));
        v[((v.len() as f64 * q) as usize).min(v.len() - 1)]
    };
    let idle_p50 = pct(idle.clone(), 0.50);
    let idle_p95 = pct(idle.clone(), 0.95);
    let typing_p50 = pct(typing.clone(), 0.50);
    let typing_p95 = pct(typing.clone(), 0.95);
    let after_p95 = pct(after.clone(), 0.95);

    println!(
        "[metrics] 编辑区高亮（10KB / {} 字节）：冷 {cold:.2}ms ｜ 空闲帧 p50={idle_p50:.2}ms \
         p95={idle_p95:.2}ms ｜ 打字帧 p50={typing_p50:.2}ms p95={typing_p95:.2}ms ｜ \
         停手补算 {settle:.2}ms（一次性）｜ 补算后帧 p95={after_p95:.2}ms",
        text.len()
    );

    // 空闲帧必须命中缓存（优化前每帧都是「冷」的成本）。
    assert!(
        idle_p95 <= 5.0,
        "空闲帧 p95 {idle_p95:.2}ms 疑似未命中高亮缓存（应与冷启动 {cold:.2}ms 同量级才对）"
    );
    assert!(
        idle_p95 < cold,
        "空闲帧 {idle_p95:.2}ms 未比冷启动 {cold:.2}ms 更快，缓存未生效"
    );
    // OPT-011 ① 的核心收益：打字帧必须落在帧预算内（此前 67.9ms，超预算 4.2 倍）。
    assert!(
        typing_p95 <= 16.0,
        "打字帧 p95 {typing_p95:.2}ms 超 16ms 帧预算，高亮防抖未生效"
    );
    // 补算只允许发生在停手后，且一次性——补算后的帧必须回到缓存量级。
    assert!(
        after_p95 <= 5.0,
        "停手补算后 p95 {after_p95:.2}ms 仍居高，说明每帧都在重算（防抖退化成节流）"
    );
}
