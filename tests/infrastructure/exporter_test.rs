//! `PulldownHtmlExporter` 单测（S5-09）。
//!
//! 两条主线：
//! 1. **导出产物正确性**：完整文档结构、正文渲染、选项下传；
//! 2. **原型缺陷 2.4 的根治验证**：TOC 标题含 `<` / `&` 时的转义，
//!    以及「TOC 拼接在清洗之前」这一顺序要求。

use std::fs;
use std::path::PathBuf;

use rustmark::domain::export::aggregate::ExportJob;
use rustmark::domain::export::errors::ExportError;
use rustmark::domain::export::services::Exporter;
use rustmark::domain::export::value_objects::{ExportFormat, ExportOptions, PageSize};
use rustmark::infrastructure::export::pulldown_html_exporter::{
    PulldownHtmlExporter, collect_toc, render_fragment, render_toc,
};
use rustmark::shared::ids::{DocumentId, JobId};
use rustmark::shared::path::FilePath;

/// 测试用临时目录守卫。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let unique = format!(
            "rustmark-exp-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let path = std::env::temp_dir().join(unique.replace(['(', ')', ' '], ""));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("应能创建临时目录");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 构造一个 HTML 导出任务。
fn html_job(dir: &TempDir, name: &str, include_toc: bool) -> (ExportJob, PathBuf) {
    let target = dir.join(name);
    let path = FilePath::new(target.to_string_lossy().replace('\\', "/")).expect("路径应合法");
    let options = ExportOptions::new(include_toc, false, "default", PageSize::A4, true);
    let job = ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        path,
        ExportFormat::Html,
        options,
    );
    (job, target)
}

#[test]
fn export_writes_complete_html_document() {
    let dir = TempDir::new("doc");
    let exporter = PulldownHtmlExporter::new();
    let (job, target) = html_job(&dir, "out.html", false);

    exporter
        .export("# 标题\n\n正文内容。", &job)
        .expect("导出应成功");

    let html = fs::read_to_string(&target).expect("应能读回导出文件");
    assert!(html.starts_with("<!DOCTYPE html>"), "必须是完整 HTML 文档");
    assert!(html.contains("<meta charset=\"utf-8\">"), "必须声明 UTF-8");
    assert!(html.contains("</html>"), "必须有结束标签");
    assert!(html.contains("标题"), "正文内容应存在");

    // 字体族须**显式**列出中文字体：只列西文时，缺少默认 CJK 字体的环境
    // 会把中文交给不含汉字的字体，最终渲染成方框。
    assert!(
        html.contains("Microsoft YaHei") && html.contains("PingFang SC"),
        "CSS 字体族应显式包含中文字体：{html}"
    );
    // 代码块与行内代码要补中文**等宽**字体，否则中文注释与代码对不齐。
    assert!(
        html.contains("code,pre{font-family:"),
        "code / pre 应显式声明等宽字体族：{html}"
    );
}

#[test]
fn exported_html_preserves_markdown_structure() {
    let dir = TempDir::new("structure");
    let exporter = PulldownHtmlExporter::new();
    let (job, target) = html_job(&dir, "struct.html", false);

    let markdown = "# 一级标题\n\n段落 **粗体**\n\n- 列表项\n\n```rust\nfn main() {}\n```\n";
    exporter.export(markdown, &job).expect("导出应成功");
    let html = fs::read_to_string(&target).expect("应能读回");

    assert!(html.contains("<h1>"), "标题应渲染为 h1：{html}");
    assert!(html.contains("<strong>粗体</strong>"), "粗体应渲染");
    assert!(html.contains("<li>列表项</li>"), "列表应渲染");
    assert!(html.contains("<code"), "代码块应渲染");
}

#[test]
fn toc_is_included_only_when_requested() {
    let dir = TempDir::new("toc-toggle");
    let exporter = PulldownHtmlExporter::new();

    let (with_toc, with_target) = html_job(&dir, "with.html", true);
    exporter
        .export("# 章\n\n正文", &with_toc)
        .expect("导出应成功");
    let html = fs::read_to_string(&with_target).expect("应能读回");
    assert!(html.contains("class=\"toc\""), "勾选目录时应生成 TOC");
    assert!(html.contains("href=\"#章\""), "TOC 应含锚点链接：{html}");

    let (without_toc, without_target) = html_job(&dir, "without.html", false);
    exporter
        .export("# 章\n\n正文", &without_toc)
        .expect("导出应成功");
    let html = fs::read_to_string(&without_target).expect("应能读回");
    assert!(!html.contains("class=\"toc\""), "未勾选目录时不应生成 TOC");
}

#[test]
fn markdown_format_copies_source_verbatim() {
    let dir = TempDir::new("md-copy");
    let exporter = PulldownHtmlExporter::new();
    let target = dir.join("copy.md");
    let path = FilePath::new(target.to_string_lossy().replace('\\', "/")).expect("路径应合法");
    let job = ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        path,
        ExportFormat::Markdown,
        ExportOptions::default_options(),
    );

    let source = "# 原样保留\n\n**不加粗**\n";
    exporter.export(source, &job).expect("导出应成功");
    assert_eq!(
        fs::read_to_string(&target).expect("应能读回"),
        source,
        "Markdown 导出必须是原文拷贝，不经 HTML 管线"
    );
}

#[test]
fn pdf_format_is_rejected_by_html_exporter() {
    let dir = TempDir::new("pdf-reject");
    let exporter = PulldownHtmlExporter::new();
    let target = dir.join("x.pdf");
    let path = FilePath::new(target.to_string_lossy().replace('\\', "/")).expect("路径应合法");
    let job = ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        path,
        ExportFormat::Pdf,
        ExportOptions::default_options(),
    );

    let error = exporter
        .export("# x", &job)
        .expect_err("PDF 不应由 HTML 导出器处理");
    assert!(
        matches!(error, ExportError::IoUnavailable { .. }),
        "应返回资源不可用错误，实际：{error:?}"
    );
}

// ---------------------------------------------------------------------------
// 原型缺陷 2.4：TOC 转义
// ---------------------------------------------------------------------------

/// 标题含 `<` 与 `&` 时，TOC 中的标题文本必须被转义，且**恰好一次**。
#[test]
fn toc_titles_with_special_chars_are_escaped() {
    let entries = collect_toc("# a < b & c\n");
    assert_eq!(entries.len(), 1);

    let html = render_toc(&entries);
    // `<` 与 `&` 必须已转义——否则会破坏 HTML 结构。
    assert!(html.contains("&lt;"), "`<` 必须转义：{html}");
    assert!(html.contains("&amp;"), "`&` 必须转义：{html}");
    assert!(!html.contains("a < b"), "不得出现未转义的裸 `<`：{html}");
    // 「恰好一次」：不应出现 `&amp;lt;` 这类双重转义。
    assert!(!html.contains("&amp;lt;"), "不得双重转义：{html}");
}

#[test]
fn toc_entries_from_headings_respect_levels_and_fences() {
    let markdown = "\
# 一级

正文

```text
# 这是代码不是标题
```

## 二级

### 三级

####### 七级不是标题（超过 6 级）

#hashtag不是标题

结尾
";
    let entries = collect_toc(markdown);
    let titles: Vec<(&str, u8)> = entries.iter().map(|e| (e.title(), e.level())).collect();
    assert_eq!(
        titles,
        vec![("一级", 1), ("二级", 2), ("三级", 3)],
        "只应收集 1–6 级 ATX 标题，且必须跳过代码围栏内的内容"
    );
}

#[test]
fn export_with_dangerous_heading_is_still_safe() {
    let dir = TempDir::new("evil-heading");
    let exporter = PulldownHtmlExporter::new();
    let (job, target) = html_job(&dir, "evil.html", true);

    // 标题里同时含原始 HTML 标签、事件属性与 javascript 链接。
    let markdown = "# <img src=x onerror=alert(1)>\n\n正文\n";
    exporter.export(markdown, &job).expect("导出应成功");
    let html = fs::read_to_string(&target).expect("应能读回");

    // 原始标签被拆壳：`src` 因不是安全 URL 被剥除，`onerror` 不在属性白名单被剥除，
    // 故标题里只剩一个干净的 `<img>`。
    assert!(
        html.contains("<h1><img></h1>"),
        "标题中的裸标签必须被清洗为无属性的空元素：{html}"
    );
    // 事件属性不得作为**属性**存活。注意 TOC 里的标题文本
    // `&lt;img src=x onerror=alert(1)&gt;` 里含有 `onerror=` 字样，
    // 但它已在转义文本中，不是属性——故断言针对「未被转义的属性赋值」。
    assert!(
        !html.contains(" onerror=") || html.contains("&lt;img src=x onerror=alert(1)&gt;"),
        "正文/标题中不得留下未转义的事件属性赋值：{html}"
    );
    // 真正的判定：所有 `on*` 事件属性都必须在标签上下文里被剥除。
    // 逐个标签检查其属性串，确保没有 `on*=value` 形态存活。
    for tag_body in html.split('<').skip(1) {
        let Some(name_end) = tag_body.find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        else {
            continue;
        };
        let attrs = &tag_body[name_end..];
        let attrs = attrs.split('>').next().unwrap_or("");
        for attr in attrs.split_whitespace() {
            assert!(
                !attr.to_lowercase().starts_with("on"),
                "标签属性中不得存在事件处理器：{attr}（原文：{html}）"
            );
        }
    }
    // TOC 中的标题文本应仍以**转义**形式可见。
    assert!(
        html.contains("&lt;img src=x onerror=alert(1)&gt;"),
        "标题文本应以转义形式保留：{html}"
    );
    // 锚点由 `slugify` 生成，只含 `alnum`/`-`/`_`，无法逃逸出属性上下文。
    assert!(
        html.contains("href=\"#"),
        "断言前提：应存在 TOC 锚点链接，否则说明 TOC 未生成"
    );
    let anchor_line = html
        .lines()
        // 只取真正以 `<a class="toc-item` 开头的行——`<style>` 里也有
        // `.toc-item{...}` 的 CSS 规则，不能用 `contains` 匹配。
        .find(|l| l.starts_with("<a class=\"toc-item"))
        .expect("应存在 TOC 条目行");
    assert_eq!(
        anchor_line.matches('<').count(),
        2,
        "TOC 条目行只应有两个 `<`（a 的开闭），实际：{anchor_line}"
    );
}

#[test]
fn export_of_script_bearing_document_is_neutralized() {
    let dir = TempDir::new("script");
    let exporter = PulldownHtmlExporter::new();
    let (job, target) = html_job(&dir, "script.html", false);

    let markdown = "正文\n\n<script>alert(1)</script>\n\n<img src=x onerror=alert(2)>\n";
    exporter.export(markdown, &job).expect("导出应成功");
    let html = fs::read_to_string(&target).expect("应能读回");

    assert!(!html.contains("<script"), "script 标签必须被丢弃：{html}");
    assert!(!html.contains("onerror"), "事件属性必须被剥除：{html}");
}

#[test]
fn fragment_rendering_does_not_include_document_skeleton() {
    let fragment = render_fragment("**仅正文**");
    assert!(fragment.contains("<strong>仅正文</strong>"));
    assert!(!fragment.contains("<!DOCTYPE"), "片段渲染不应包含文档骨架");
    assert!(!fragment.contains("<html"), "片段渲染不应包含 html 标签");
}

#[test]
fn export_creates_missing_parent_directories() {
    let dir = TempDir::new("mkdir");
    let exporter = PulldownHtmlExporter::new();
    let target = dir.join("deep/nested/out.html");
    let path = FilePath::new(target.to_string_lossy().replace('\\', "/")).expect("路径应合法");
    let job = ExportJob::new(
        JobId::new(),
        DocumentId::new(),
        path,
        ExportFormat::Html,
        ExportOptions::default_options(),
    );

    exporter.export("# 深目录", &job).expect("应自动创建父目录");
    assert!(target.exists(), "导出文件应存在于新建的目录中");
}

#[test]
fn export_leaves_no_temp_file_behind() {
    let dir = TempDir::new("tmp");
    let exporter = PulldownHtmlExporter::new();
    let (job, _target) = html_job(&dir, "clean.html", true);
    exporter.export("# 标题", &job).expect("导出应成功");

    let leftovers: Vec<String> = fs::read_dir(&dir.0)
        .expect("应能列出目录")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with(".rustmark-tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "不应残留临时文件：{leftovers:?}");
}

// ============================================================================
// 数学公式 / Mermaid 图表（P1）
// ============================================================================

#[test]
fn math_survives_html_export() {
    // 回归防护：导出器的 `Options` 若漏了 `ENABLE_MATH`，pulldown 会把 `$...$`
    // 当普通文本——预览里有公式、导出没有，且**不会报任何错**。
    let fragment = render_fragment("质能方程 $E = mc^2$ 很有名。\n");
    assert!(
        fragment.contains("math-inline"),
        "导出 HTML 应保留行内公式标记：{fragment}"
    );
    assert!(
        fragment.contains("E = mc^2"),
        "公式源码应写入导出结果：{fragment}"
    );

    let display = render_fragment("$$\n\\int_0^1 x^2 dx\n$$\n");
    assert!(
        display.contains("math-display"),
        "导出 HTML 应保留块级公式标记：{display}"
    );
}

#[test]
fn math_marks_survive_sanitization() {
    // `html_sanitize` 的 `DROP_TAGS` 含 `math`（针对手写的原生 `<math>`）。
    // pulldown 输出的是 `<span class="math ...">`，必须确认清洗后仍存活——
    // 这正是「公式在导出后静默消失」的真实风险点。
    let cleaned = rustmark::infrastructure::export::html_sanitize::sanitize_html(
        r#"<p><span class="math math-inline">E = mc^2</span></p>"#,
    );
    assert!(
        cleaned.contains("math-inline"),
        "清洗不应丢掉 pulldown 的公式 span：{cleaned}"
    );
}

#[test]
fn mermaid_fence_is_exported_as_code_block() {
    // 导出侧不做图表渲染（交给浏览器端 Mermaid），但源码必须完整保留。
    let fragment = render_fragment("```mermaid\ngraph TD;\n  A-->B;\n```\n");
    assert!(
        fragment.contains("graph TD;"),
        "图表源码应完整导出：{fragment}"
    );
    assert!(
        fragment.contains("mermaid"),
        "围栏语言标注应保留（供前端选择渲染器）：{fragment}"
    );
}

// ============================================================================
// 公式 / 图表渲染资源注入（P2 遗留：导出件开箱即渲染）
// ============================================================================

#[test]
fn math_in_export_injects_katex_resources() {
    let dir = TempDir::new("katex");
    let exporter = PulldownHtmlExporter::new();
    let (job, target) = html_job(&dir, "math.html", false);

    exporter
        .export("质能方程 $E = mc^2$ 很有名。", &job)
        .expect("导出应成功");
    let html = fs::read_to_string(&target).expect("应能读回导出文件");

    assert!(
        html.contains("katex.min.css") && html.contains("katex.min.js"),
        "含公式的导出件应注入 KaTeX 资源"
    );
    assert!(
        html.contains("querySelectorAll('.math-inline, .math-display')"),
        "渲染脚本应按 pulldown 的类名定位公式元素"
    );
    assert!(
        html.contains("throwOnError: false"),
        "公式渲染失败不得中断整页脚本"
    );
}

#[test]
fn mermaid_in_export_injects_mermaid_resources() {
    let dir = TempDir::new("mermaid");
    let exporter = PulldownHtmlExporter::new();
    let (job, target) = html_job(&dir, "mermaid.html", false);

    exporter
        .export("```mermaid\ngraph TD;\n  A-->B;\n```", &job)
        .expect("导出应成功");
    let html = fs::read_to_string(&target).expect("应能读回导出文件");

    assert!(
        html.contains("mermaid.min.js"),
        "含图表的导出件应注入 Mermaid 资源"
    );
    assert!(
        html.contains("code.language-mermaid"),
        "渲染脚本应把 mermaid 代码块替换为图表容器"
    );
}

#[test]
fn plain_export_has_no_external_resources() {
    // 普通文档不得被注入任何外链脚本：导出件的「零脚本」性质是安全边界
    // （清洗器拦住了用户注入的 script，导出器自己不能又开一道门）。
    let dir = TempDir::new("plain");
    let exporter = PulldownHtmlExporter::new();
    let (job, target) = html_job(&dir, "plain.html", false);

    exporter
        .export("# 纯文本\n\n没有公式也没有图表。", &job)
        .expect("导出应成功");
    let html = fs::read_to_string(&target).expect("应能读回导出文件");

    assert!(
        !html.contains("<script"),
        "不含公式/图表的导出件不应有任何脚本：{html}"
    );
    assert!(!html.contains("cdn.jsdelivr.net"), "不应有 CDN 外链");
}
