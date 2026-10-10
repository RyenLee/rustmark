//! HTML 导出器（S5-09）。
//!
//! # 流程顺序不可调换（原型缺陷 2.4 的根治点）
//!
//! ```text
//! Markdown → pulldown 渲染正文 → 拼接 TOC → **白名单清洗** → 包裹骨架 → 写盘
//! ```
//!
//! 关键点：**TOC 在清洗之前拼接**。若先清洗再拼 TOC，则 TOC 里的标题文本
//! 是未经清洗的原始内容，标题含 `<script>` 时就会形成注入入口。
//! 而拼接后再统一清洗，TOC 与正文享受同一套白名单保护。
//!
//! 另一个关键点：**骨架在清洗之外**。文档骨架（`<meta charset>`、`<style>`、
//! `<title>`）由本模块生成，是可信常量；而清洗器按设计会丢弃 `style`/`meta`
//! （二者都在 `DROP_TAGS` 中，防的是用户注入的样式与重定向）。
//! 若把骨架一并送进清洗器，导出的 HTML 会失去 charset 声明与全部样式。
//! 故清洗的边界严格取在「正文 + TOC」这段不可信内容上。
//!
//! 配合领域层的 `TocEntry::needs_escape` 标记（构造时判定）与
//! [`TocBuilder::verify`] 兜底，可确保标题**恰好转义一次**——
//! 不会因「渲染时才想起转义」而出现双重转义或漏转义。
//!
//! # 为什么自己拼 HTML 文档骨架
//!
//! 导出的 HTML 要在浏览器里独立打开，故需要 `<meta charset>`、`<title>`、
//! 基础样式。用字符串模板拼接而非引入模板引擎，是为了守住体积预算（§8.1）。
//!
//! # 公式 / 图表的渲染资源（P2 遗留修复）
//!
//! 正文含公式 / Mermaid 时，骨架在**清洗边界之外**按需注入 CDN 渲染资源
//! （KaTeX / Mermaid）与一段带守卫的渲染脚本——资源不可达时内容保持
//! 源码形态，与未注入时行为一致。不含公式 / 图表的普通文档**不注入任何
//! 外链**，保持导出件「零脚本」的安全性质。策略详见 [`runtime_resources`]。

use std::path::PathBuf;

use pulldown_cmark::{Options, html};

use crate::domain::export::aggregate::ExportJob;
use crate::domain::export::errors::ExportError;
use crate::domain::export::services::{Exporter, TocBuilder};
use crate::domain::export::value_objects::{ExportFormat, TocEntry};
use crate::infrastructure::persistence::recent_files_store::atomic_write;

use super::html_sanitize::sanitize_html;

/// 导出用的 pulldown 选项。
///
/// 与 [`crate::infrastructure::parsing::pulldown_parser`] 的选项集**保持一致**：
/// 若预览支持表格而导出不支持，用户会看到「预览有、导出没有」的不一致。
fn export_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_GFM
        // 与解析器 `parser_options` 保持一致：缺这一项时 pulldown 会把 `$...$`
        // 当普通文本，导出的 HTML 里就没有 `math` 标记了。
        | Options::ENABLE_MATH
}

/// 渲染 Markdown 正文为 HTML 片段（不含文档骨架）。
pub fn render_fragment(markdown: &str) -> String {
    let parser = pulldown_cmark::Parser::new_ext(markdown, export_options());
    let mut buffer = String::new();
    html::push_html(&mut buffer, parser);
    buffer
}

/// 生成目录 HTML。
///
/// 标题文本经 [`TocBuilder::escape_title`] 转义（消费领域层的 `needs_escape` 标记），
/// 因此含 `<` `&` 的标题不会破坏 HTML 结构。
pub fn render_toc(entries: &[TocEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    // 用 `div` 而非语义更贴切的 `nav`：`nav` 不在清洗白名单内，
    // 会被拆壳并连同 `class="toc"` 一起丢掉（原型白名单同样没有 `nav`）。
    let mut out = String::from("<div class=\"toc\">\n");
    for entry in entries {
        out.push_str(&format!(
            "<a class=\"toc-item toc-level-{}\" href=\"#{}\">{}</a>\n",
            entry.level(),
            entry.anchor(),
            TocBuilder::escape_title(entry),
        ));
    }
    out.push_str("</div>\n");
    out
}

/// 页面样式（内联，避免额外文件依赖）。
///
/// 打印友好：`@media print` 隐藏导航并避免分页截断。
///
/// # 为什么字体族里要显式列出中文字体
///
/// 浏览器虽有自动回落，但只列西文时，缺少默认 CJK 字体的环境会把中文
/// 交给某个不含汉字的字体，最终渲染成方框。显式声明可避免这种不确定性。
/// 同时 `pre` / `code` 要补中文**等宽**字体：否则代码块里的中文注释
/// 会用比例字体渲染，与代码对不齐。
const PAGE_STYLE: &str = "\
body{font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,\
Microsoft YaHei,PingFang SC,Hiragino Sans GB,Noto Sans CJK SC,sans-serif;\
line-height:1.7;max-width:52em;margin:2em auto;padding:0 1em;color:#222}
h1,h2,h3,h4,h5,h6{line-height:1.3;margin-top:1.6em}
pre{background:#f6f8fa;padding:1em;overflow:auto;border-radius:4px}
code{background:#f6f8fa;padding:.15em .35em;border-radius:3px;font-size:.9em}
pre code{background:none;padding:0}
code,pre{font-family:ui-monospace,SFMono-Regular,Consolas,Menlo,Monaco,SimSun,\
Noto Sans Mono CJK SC,monospace}
blockquote{margin:1em 0;padding-left:1em;border-left:4px solid #ddd;color:#555}
table{border-collapse:collapse;width:100%;margin:1em 0}
th,td{border:1px solid #ddd;padding:.5em .75em;text-align:left}
th{background:#f6f8fa}
img{max-width:100%;height:auto}
.toc{border:1px solid #ddd;border-radius:4px;padding:1em 1.25em;margin-bottom:2em;\
background:#fafbfc}
.toc-item{display:block;text-decoration:none;color:#0366d6}
.toc-level-2{padding-left:1.25em}
.toc-level-3{padding-left:2.5em}
.toc-level-4{padding-left:3.75em}
.toc-level-5,.toc-level-6{padding-left:5em}
@media print{.toc{display:none}body{max-width:none;margin:0}h1,h2,h3{page-break-after:avoid}\
pre,table,img{page-break-inside:avoid}}
";

/// 组装完整 HTML 文档。
///
/// 正文含公式 / Mermaid 图表时，`<head>` 注入对应的渲染资源（见
/// [`runtime_resources`]）：没有这类内容时**不注入任何外链**，
/// 普通文档的导出件保持「零脚本」性质。
fn wrap_document(title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n\
<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n\
<title>{title}</title>\n\
<style>\n{style}\n</style>\n\
{resources}\
</head>\n<body>\n{body}\n</body>\n</html>\n",
        title = escape_text(title),
        style = PAGE_STYLE,
        resources = runtime_resources(body),
        body = body,
    )
}

/// 公式 / 图表渲染资源的 CDN 地址（按需注入）。
///
/// # 外链策略（本项实施时的裁定）
///
/// 采用**公共 CDN + 优雅降级**：浏览器打开导出件时若能联网，
/// 公式与图表即渲染为成品；断网或 CDN 不可达时脚本缺失，
/// `render_math_and_diagrams` 里的守卫判断直接跳过——
/// 内容保持源码形态，与未注入时的行为完全一致，正文阅读不受影响。
/// 不内嵌离线资源包（KaTeX 字体 + Mermaid 合计数 MB，会击穿体积预算）；
/// 需要完全离线的场景由用户导出 Markdown / 自行部署后替换链接。
///
/// 版本在实施时经 CDN 实测可达（三个 dist 资源均 HTTP 200）；
/// 升级版本时须同样实测。
const KATEX_VERSION: &str = "0.16.47";
const MERMAID_VERSION: &str = "10.9.8";

/// 正文需要时拼出 `<head>` 注入块；不需要时返回空串。
///
/// 检测用**类名**而非解析 HTML：pulldown 的输出格式是固定的
/// （`<span class="math math-inline|math-display">` 与
/// `<code class="language-mermaid">`），字符串探测足够且零成本。
fn runtime_resources(body: &str) -> String {
    let has_math = body.contains("class=\"math");
    let has_mermaid = body.contains("language-mermaid");
    if !has_math && !has_mermaid {
        return String::new();
    }
    let mut out = String::new();
    if has_math {
        out.push_str(&format!(
            "<link rel=\"stylesheet\" href=\"https://cdn.jsdelivr.net/npm/katex@{v}/dist/katex.min.css\">\n\
             <script defer src=\"https://cdn.jsdelivr.net/npm/katex@{v}/dist/katex.min.js\"></script>\n",
            v = KATEX_VERSION
        ));
    }
    if has_mermaid {
        out.push_str(&format!(
            "<script defer src=\"https://cdn.jsdelivr.net/npm/mermaid@{v}/dist/mermaid.min.js\"></script>\n",
            v = MERMAID_VERSION
        ));
    }
    out.push_str("<script defer>\n");
    out.push_str(render_math_and_diagrams());
    out.push_str("</script>\n");
    out
}

/// 注入的渲染脚本（`defer` 保证在 `DOMContentLoaded` 前执行完毕）。
///
/// 每一步都有「资源缺失就跳过」的守卫：CDN 不可达时公式 / 图表
/// 保持源码形态——与未注入资源时行为一致，绝不因渲染失败丢内容。
fn render_math_and_diagrams() -> &'static str {
    r#"
window.addEventListener('DOMContentLoaded', function () {
  if (window.katex) {
    document.querySelectorAll('.math-inline, .math-display').forEach(function (el) {
      try {
        katex.render(el.textContent, el, {
          displayMode: el.classList.contains('math-display'),
          throwOnError: false
        });
      } catch (e) { /* 公式非法：保留源码形态 */ }
    });
  }
  var charts = document.querySelectorAll('pre > code.language-mermaid');
  if (charts.length && window.mermaid) {
    charts.forEach(function (code) {
      var div = document.createElement('div');
      div.className = 'mermaid';
      div.textContent = code.textContent;
      code.parentElement.replaceWith(div);
    });
    mermaid.initialize({ startOnLoad: true });
  }
});
"#
}

/// 组装**打印就绪**的 HTML 文档（供 S5-10 系统打印管线消费）。
///
/// 与 [`build_document`] 的唯一区别是**不做白名单清洗**：
/// 产物写到本地临时文件、交给打印引擎，不会在浏览器里打开，
/// 故没有脚本注入面。这一点必须在注释里写明，否则后人会
/// 「为了安全」给这里也加上清洗——而清洗会连本导出器生成的
/// `<style>` 一起剥掉，打印出来就是无样式的裸文档。
pub fn build_printable_document(title: &str, body: &str) -> String {
    wrap_document(title, body)
}

/// 文本转义（用于标题等文本节点）。
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// HTML 导出器。
#[derive(Clone, Copy, Debug, Default)]
pub struct PulldownHtmlExporter;

impl PulldownHtmlExporter {
    /// 构造导出器。
    pub fn new() -> Self {
        Self
    }

    /// 由 Markdown 源码构建完整 HTML 文档。
    ///
    /// 单独暴露以便单测直接断言中间产物，无需落盘。
    pub fn build_document(&self, markdown: &str, job: &ExportJob, toc: &[TocEntry]) -> String {
        let title = job.destination().file_name().to_string();
        let body = render_fragment(markdown);
        let with_toc = if job.needs_toc() {
            format!("{body}\n{}", render_toc(toc))
        } else {
            body
        };
        // 只清洗**正文 + TOC**（不可信来源），骨架在清洗之外。
        let safe_body = sanitize_html(&with_toc);
        wrap_document(&title, &safe_body)
    }
}

impl Exporter for PulldownHtmlExporter {
    fn export(&self, content: &str, job: &ExportJob) -> Result<usize, ExportError> {
        // 只处理 HTML 与 Markdown 两种「纯文本」格式；
        // PDF 与图片由各自导出器负责。
        match job.format() {
            ExportFormat::Html => {}
            ExportFormat::Markdown => {
                // Markdown 导出是原文拷贝，不经 HTML 管线。
                let target = to_disk_path(job.destination().as_str());
                return atomic_write(&target, content.as_bytes())
                    .map(|()| content.len())
                    .map_err(|e| ExportError::io_unavailable(format!("写入 Markdown 失败：{e}")));
            }
            other => {
                return Err(ExportError::io_unavailable(format!(
                    "{} 导出不由 HTML 导出器处理",
                    other.label()
                )));
            }
        }

        // TOC 条目从正文标题提取；此处的转义标记由领域层 `TocEntry::new` 判定。
        let toc = collect_toc(content);

        // 兜底防线：若有条目漏标 `needs_escape`，在写盘前拦下（原型缺陷 2.4）。
        TocBuilder::verify(&toc)?;

        let document = self.build_document(content, job, &toc);
        // `build_document` 内部已完成「正文 + TOC」的整体清洗，
        // 此处**不可**再清洗一次：那会把刚生成的 `<meta charset>` 与
        // `<style>`（`style` 属DROP_TAGS）一并剥掉，导出成没有样式的裸文档。

        let target = to_disk_path(job.destination().as_str());
        atomic_write(&target, document.as_bytes())
            .map(|()| document.len())
            .map_err(|e| ExportError::io_unavailable(format!("写入 HTML 失败：{e}")))
    }
}

/// 把领域路径转为磁盘路径（`FilePath` 内部用 `/`，Windows 亦接受）。
fn to_disk_path(path: &str) -> PathBuf {
    PathBuf::from(path)
}

/// 从 Markdown 源码提取标题，构建目录条目。
///
/// 逐行匹配 ATX 标题（`#`~`######`），跳过代码围栏内的 `#`（那是内容不是标题）。
///
/// 公开以便单测直接断言标题收集规则——它是 TOC 正确性的第一道关口，
/// 一旦漏收或误收，用户看到的目录就是错的。
pub fn collect_toc(markdown: &str) -> Vec<TocEntry> {
    let mut entries = Vec::new();
    let mut in_fence = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        // 围栏代码块的开关行（``` 或 ~~~）。
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        // 1–6 级且`#` 后须有空格（否则是 `#hashtag` 这类普通文本）。
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            let title = trimmed[hashes..].trim().trim_end_matches('#').trim();
            if !title.is_empty() {
                entries.push(TocEntry::new(hashes as u8, title));
            }
        }
    }
    entries
}
