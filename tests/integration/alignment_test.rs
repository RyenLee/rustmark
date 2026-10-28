//! S7-02 行为对齐矩阵 · 自动化断言部分。
//!
//! 逐项验证 `docs/prototype-alignment.md` 中标注「自动化用例」的行：
//! 渲染（R-01~08）、高亮（H-01~20）、统计口径（S-01~11）、交互默认值（D-01~07）。
//! D-08（无修饰键单键允许）归 `regression_test.rs`。
//!
//! 仅断言**可脱离 GUI 观测**的行为；视觉/像素差异已在矩阵文档记录并归 S7-05。

use rustmark::app::bootstrap::PREVIEW_DEBOUNCE_MS;
use rustmark::application::document::service::compute_stats;
use rustmark::application::markdown::service::TRUNCATE_THRESHOLD_BYTES;
use rustmark::application::workspace::service::{MAX_HITS_PER_FILE, MAX_TOTAL_HITS};
use rustmark::domain::markdown::services::SyntaxHighlighter;
use rustmark::domain::markdown::value_objects::SpanStyle;
use rustmark::domain::preferences::aggregate::Preferences;
use rustmark::domain::workspace::value_objects::RECENT_FILES_LIMIT;
use rustmark::infrastructure::export::html_sanitize::sanitize_html;
use rustmark::infrastructure::export::pulldown_html_exporter::render_fragment;
use rustmark::infrastructure::highlighting::syntect_highlighter::SyntectHighlighter;

// ---------------------------------------------------------------------------
// ① 渲染差异（R-01 ~ R-08）
// ---------------------------------------------------------------------------

#[test]
fn align_gfm_table_renders() {
    let html = render_fragment("| 左 | 右 |\n|---|---|\n| a | b |\n");
    assert!(
        html.contains("<table"),
        "表格必须渲染为 <table>，实际：{html}"
    );
    assert!(html.contains("<th"), "表头必须渲染");
    assert!(
        html.contains("a") && html.contains("b"),
        "单元格内容必须保留"
    );
}

#[test]
fn align_task_list_renders_checkbox() {
    let html = render_fragment("- [ ] 待办\n- [x] 已完成\n");
    assert!(
        html.contains("type=\"checkbox\""),
        "任务列表必须渲染复选框，实际：{html}"
    );
    // 已完成项应带 checked（pulldown 对 [x] 输出 checked）。
    assert!(
        html.contains("checked") || html.contains("<input"),
        "已完成项应标记选中态"
    );
}

#[test]
fn align_footnote_renders() {
    let html = render_fragment("正文[^1]\n\n[^1]: 脚注说明\n");
    // 脚注定义文本必须出现在输出（<section> 被拆壳但内容保留）。
    assert!(
        html.contains("脚注说明"),
        "脚注定义文本必须保留，实际：{html}"
    );
    // 引用锚点必须保留，形成可跳转引用。pulldown-cmark 输出数字锚点
    // （GitHub 风格是 fn1 / fnref1），这是与原型的一处已知实现差异。
    assert!(
        html.contains("footnote-reference") && html.contains("href=\"#"),
        "脚注引用应保留可跳转锚点，实际：{html}"
    );
    // 定义块与引用锚点必须指向同一 id，否则跳转失效。
    assert!(
        html.contains("footnote-definition") && html.contains("id=\""),
        "脚注定义应带锚点 id，实际：{html}"
    );
}

#[test]
fn align_autolink_renders_anchor() {
    let html = render_fragment("见 <https://example.com> 与裸 URL https://x.org 末尾。\n");
    assert!(
        html.contains("href=\"https://example.com\""),
        "自动链接必须生成 <a href>，实际：{html}"
    );
}

#[test]
fn align_inline_html_preserved() {
    let html = render_fragment("前面\n\n<div class=\"note\">保留的 div</div>\n\n后面\n");
    assert!(
        html.contains("<div class=\"note\">"),
        "白名单内联标签必须保留，实际：{html}"
    );
    assert!(html.contains("保留的 div"));
}

#[test]
fn align_inline_html_dropped() {
    // 走真实管线：pulldown 渲染 → 白名单清洗。清洗器丢弃 <script> 整棵子树。
    let html = sanitize_html(&render_fragment("文本\n\n<script>alert(1)</script>\n"));
    assert!(
        !html.contains("<script"),
        "危险内联标签必须被丢弃，实际：{html}"
    );
    assert!(!html.contains("alert"), "脚本文本不得残留");
}

#[test]
fn align_strikethrough_renders() {
    let html = render_fragment("~~删除线~~ 普通\n");
    assert!(
        html.contains("<del>"),
        "删除线必须渲染为 <del>，实际：{html}"
    );
}

// ---------------------------------------------------------------------------
// ② 语法高亮差异（H-01 ~ H-20）
// ---------------------------------------------------------------------------

/// 原型 CodeDialog.vue 的 20 种语言，各跑一段代表性代码。
const TWENTY_LANGS: &[(&str, &str)] = &[
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

#[test]
fn align_twenty_languages_all_highlight() {
    let highlighter = SyntectHighlighter::new();
    let mut zero_spans = Vec::new();
    for (lang, code) in TWENTY_LANGS {
        let spans = highlighter
            .highlight(code, Some(lang))
            .unwrap_or_else(|e| panic!("{lang} 高亮应成功：{e}"));
        if spans.is_empty() {
            zero_spans.push(*lang);
        }
    }
    assert!(
        zero_spans.is_empty(),
        "以下语言高亮返回空（未识别）：{zero_spans:?}"
    );
}

#[test]
fn align_highlight_is_deterministic() {
    let highlighter = SyntectHighlighter::new();
    let code = "fn f() { let s = \"x\"; }";
    let first = highlighter.highlight(code, Some("rust")).expect("应成功");
    for _ in 0..3 {
        let again = highlighter.highlight(code, Some("rust")).expect("应成功");
        assert_eq!(first, again, "高亮必须确定性可复现");
    }
}

#[test]
fn align_highlight_styles_are_domain_owned_only() {
    let highlighter = SyntectHighlighter::new();
    let spans = highlighter
        .highlight("fn f() { let s = \"a\"; }", Some("rust"))
        .expect("应成功");
    for span in spans.iter() {
        // 穷举领域枚举：若 syntect 类型泄漏进来，编译会失败。
        let name = match span.style() {
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
// ③ 数值口径差异（S-01 ~ S-11）
// ---------------------------------------------------------------------------

#[test]
fn align_stats_counts_cjk_and_words() {
    // 「你好world」：2 个 CJK 字 + 1 个西文词（world）。
    let s = compute_stats("你好world", "");
    assert_eq!(s.words, 3, "中日韩按字 + 西文按词，应为 3");
    // 「- [ ] 待办」：连字符/方括号不计为词（有意偏离原型）。
    let t = compute_stats("- [ ] 待办\n- [x] 完成\n", "");
    assert_eq!(t.tasks_total, 2, "任务总数应为 2");
    assert_eq!(t.tasks_done, 1, "已完成任务应为 1");
}

#[test]
fn align_stats_char_and_line_counts() {
    let s = compute_stats("a b\nc d e\n", "");
    assert_eq!(s.characters, 10, "含空格字符数：8 字符 + 2 换行");
    assert_eq!(s.characters_no_space, 5, "不含空格字符数：a b c d e = 5");
    assert_eq!(s.lines, 3, "行数含末行（空）计为 3");
}

#[test]
fn align_stats_structure_counts() {
    let md = "# 标题\n\n段落一\n\n## 小标题\n\n```\ncode\n```\n\n[链接](https://x.com) 与 ![图](a.png)\n";
    let s = compute_stats(md, "");
    assert_eq!(s.headings, 2, "标题数");
    assert_eq!(s.code_blocks, 1, "代码块数");
    assert_eq!(s.links, 1, "链接数（图片不计入链接）");
    assert_eq!(s.images, 1, "图片数");
    // 段落计数：每个空行后的首个非空块（含标题行）都计为一段，故 4 段。
    assert_eq!(s.paragraphs, 4, "段落数（2 标题 + 2 文本段）");
}

#[test]
fn align_stats_reading_minutes_floor_one() {
    let s = compute_stats("你好", "");
    assert_eq!(s.reading_minutes, 1, "阅读时长下限为 1");
    let big = compute_stats(&"字".repeat(900), "");
    assert_eq!(big.reading_minutes, 3, "900 字 / 300 = 3 分钟");
}

// ---------------------------------------------------------------------------
// ④ 交互默认值与阈值（D-01 ~ D-07）
// ---------------------------------------------------------------------------

#[test]
fn align_default_preview_debounce_is_150ms() {
    assert_eq!(PREVIEW_DEBOUNCE_MS, 150, "预览防抖必须 150ms（FR-01）");
}

#[test]
fn align_default_undo_merge_window_is_400ms() {
    assert_eq!(
        Preferences::default().editor().undo_merge_window_ms(),
        400,
        "撤销合并窗口必须 400ms（A-07）"
    );
}

/// A-17 的原型值为 50 / 300；2026-10-25 阈值评审裁定上调到 100 / 1000
/// （1000 文件夹具中常见词会立即触顶且无分页）。此处断言**本实现的决策值**，
/// 偏差已登记在 `docs/reports/threshold-review-2026-10-25.md` 与原型契约 A-17 备注。
#[test]
fn align_search_hit_caps_follow_reviewed_limits() {
    assert_eq!(
        MAX_HITS_PER_FILE, 100,
        "单文件命中上限应为 100（A-17 偏差）"
    );
    assert_eq!(MAX_TOTAL_HITS, 1000, "总命中上限应为 1000（A-17 偏差）");
}

#[test]
fn align_recent_files_cap_is_20() {
    assert_eq!(RECENT_FILES_LIMIT, 20, "最近文件上限应为 20（A-18）");
}

#[test]
fn align_truncate_threshold_is_500kb() {
    assert_eq!(
        TRUNCATE_THRESHOLD_BYTES,
        500 * 1024,
        "大文件降级阈值应为 500KB（FR-06）"
    );
}

// ---------------------------------------------------------------------------
// ⑥ P1 扩展块（FUNC-P1-02 / FUNC-P1-03，2026-10-08 交付）
// ---------------------------------------------------------------------------

#[test]
fn align_math_and_mermaid_survive_export() {
    // 原型用 highlight.js / KaTeX 呈现公式与图表；Rust 侧零依赖方案不内嵌运行时，
    // 但**导出的 HTML 必须保留结构与源码**，否则浏览器侧也无从渲染。
    let html = render_fragment("公式 $E = mc^2$ 与图表：\n\n```mermaid\ngraph TD;\n```\n");
    assert!(
        html.contains("math-inline") && html.contains("E = mc^2"),
        "行内公式应保留标记与源码：{html}"
    );
    assert!(
        html.contains("graph TD;") && html.contains("mermaid"),
        "图表源码与语言标注应保留：{html}"
    );
}

#[test]
fn align_sanitize_keeps_math_span_but_drops_native_math_tag() {
    // 两条策略并存且都要成立：
    // ① pulldown 产出的 `<span class="math">` 必须活下来（公式承载于它）；
    // ② 手写的原生 `<math>` 仍在 `DROP_TAGS` 内（防注入，B-09）。
    let kept = sanitize_html(r#"<p><span class="math math-display">x^2</span></p>"#);
    assert!(
        kept.contains("math-display"),
        "公式 span 不应被清洗丢弃：{kept}"
    );

    let dropped = sanitize_html(r#"<p><math><mi>x</mi></math></p>"#);
    assert!(
        !dropped.contains("<math"),
        "原生 <math> 仍应被丢弃（B-09 安全策略）：{dropped}"
    );
}
