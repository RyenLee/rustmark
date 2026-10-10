//! `html_sanitize` 单测（S5-09）。
//!
//! **必须通过原型 `REMAINING-WORK.md` §6.5 的 8 条真实安全用例**
//! （原型修复前 3 条 FAIL，说明这些是实际被利用过的绕过路径，
//! 而非假想威胁）。此外覆盖 TOC 转义（原型缺陷 2.4 的根治点）。

use rustmark::infrastructure::export::html_sanitize::{is_safe_url, sanitize_html};

// ---------------------------------------------------------------------------
// 原型 8 条安全用例
// ---------------------------------------------------------------------------

/// 用例 1：`<svg><script>` 必须**整棵子树**丢弃，不得留下 script。
#[test]
fn case1_svg_script_subtree_is_dropped() {
    let out = sanitize_html("<svg><script>alert(1)</script></svg>");
    assert_eq!(out, "", "svg 子树必须整体丢弃，实际：{out}");
    assert!(!out.contains("script"), "不得残留任何 script 痕迹");
    assert!(!out.contains("alert"), "不得残留脚本文本");
}

/// 用例 2：`<foo><img onerror>` —— 未知标签拆壳后，子节点**仍须清洗**。
///
/// 这是原型真实踩过的坑：拆壳时若直接 `continue` 跳过清洗，
/// `onerror` 就会存活（实测修复前输出为 `onerror="alert(1)"` 保留）。
#[test]
fn case2_unwrapped_unknown_tag_children_are_resanitized() {
    let out = sanitize_html("<foo><img src=x onerror=alert(1)></foo>");
    assert!(
        !out.contains("onerror"),
        "拆壳后的子节点必须被重新清洗，实际：{out}"
    );
    assert!(!out.contains("alert"), "不得残留脚本文本，实际：{out}");
    // 与原型 REMAINING-WORK §6.5 表格一致：期望结果是 `<img>`。
    assert!(out.contains("<img"), "img 标签本身应保留，实际：{out}");
    // 裸 `x` 既非锚点也非相对路径（无 `./` 前缀），按 `safeUrl` 规则被剥除——
    // 这与原型行为一致，故此处断言 src 已被剥除。
    assert!(
        !out.contains("src="),
        "裸 `src=x` 按 safeUrl 规则应被剥除（与原型一致），实际：{out}"
    );
    // 相对路径带 `./` 前缀时应保留。
    let kept = sanitize_html("<foo><img src=\"./a.png\" onerror=alert(1)></foo>");
    assert!(kept.contains("./a.png"), "相对路径应保留，实际：{kept}");
}

/// 用例 3：`<math><mtext><img onerror>` —— math 子树整体丢弃。
#[test]
fn case3_math_namespace_subtree_is_dropped() {
    let out = sanitize_html("<math><mtext><img src=x onerror=alert(1)></mtext></math>");
    assert_eq!(out, "", "math 子树必须整体丢弃，实际：{out}");
    assert!(!out.contains("onerror"), "不得残留 onerror");
}

/// 用例 4：`javascript:` 协议（含大小写混淆）必须被剥除。
#[test]
fn case4_javascript_scheme_is_stripped() {
    for payload in [
        "<a href=\"javascript:alert(1)\">x</a>",
        "<a href=\"JaVaScRiPt:alert(1)\">x</a>",
        "<a href=\"  javascript:alert(1)\">x</a>",
    ] {
        let out = sanitize_html(payload);
        assert!(
            !out.to_lowercase().contains("javascript"),
            "`javascript:` 必须被剥除（输入 {payload}），实际：{out}"
        );
    }
}

/// 用例 5：全部 `on*` 事件属性必须被剥除。
#[test]
fn case5_all_event_handler_attributes_are_stripped() {
    let out = sanitize_html("<img src=x onerror=alert(1) onload=alert(2) onclick=alert(3)>");
    for attr in ["onerror", "onload", "onclick"] {
        assert!(!out.contains(attr), "{attr} 必须被剥除，实际：{out}");
    }
    // 其它非白名单属性同样剥除。
    let out = sanitize_html("<div class=\"ok\" data-evil=\"x\" formaction=\"y\">t</div>");
    assert!(out.contains("class=\"ok\""), "白名单属性应保留");
    assert!(!out.contains("data-evil"), "非白名单属性应剥除");
    assert!(!out.contains("formaction"), "非白名单属性应剥除");
}

/// 用例 6：`style` 属性必须被剥除（可用于 CSS 注入与数据外泄）。
#[test]
fn case6_style_attribute_is_stripped() {
    let out = sanitize_html("<p style=\"background:url(javascript:alert(1))\">文本</p>");
    assert!(!out.contains("style"), "style 属性必须被剥除，实际：{out}");
    assert!(out.contains("文本"), "文本内容应保留");
}

/// 用例 7：位图 data URI **保留**（不能误杀合法内联图片）。
#[test]
fn case7_bitmap_data_uri_is_preserved() {
    let out = sanitize_html("<img src=\"data:image/png;base64,iVBORw0KGgo=\" alt=\"图\">");
    assert!(
        out.contains("data:image/png;base64,"),
        "合法位图 data URI 必须保留，实际：{out}"
    );
    // 各种白名单类型都应放行。
    for mime in ["png", "jpeg", "jpg", "gif", "webp", "bmp"] {
        assert!(
            is_safe_url(&format!("data:image/{mime};base64,AAAA")),
            "image/{mime} 应被放行"
        );
    }
}

/// 用例 8：`data:text/html` 必须被剥除（XSS 向量）。
#[test]
fn case8_data_text_html_is_stripped() {
    let out = sanitize_html("<a href=\"data:text/html;base64,PHNjcmlwdD4=\">x</a>");
    assert!(
        !out.contains("data:text/html"),
        "`data:text/html` 必须被剥除，实际：{out}"
    );
    // 任何非位图的 data: 一律拒绝。
    for payload in [
        "data:text/html,<script>alert(1)</script>",
        "data:application/javascript,x",
        "data:image/svg+xml;base64,AAAA",
        "data:image/pngx;base64,AAAA",
    ] {
        assert!(!is_safe_url(payload), "`{payload}` 必须被判为不安全");
    }
}

// ---------------------------------------------------------------------------
// URL 白名单
// ---------------------------------------------------------------------------

#[test]
fn url_whitelist_allows_normal_links() {
    for url in [
        "https://example.com",
        "http://example.com",
        "mailto:a@b.com",
        "tel:+123456",
        "#anchor",
        "/absolute/path",
        "./relative",
        "../parent",
        "",
    ] {
        assert!(is_safe_url(url), "`{url}` 应被放行");
    }
}

#[test]
fn url_whitelist_rejects_dangerous_schemes() {
    for url in [
        "javascript:alert(1)",
        "JaVaScRiPt:alert(1)",
        "vbscript:msgbox(1)",
        "file:///etc/passwd",
        "data:text/html;base64,x",
    ] {
        assert!(!is_safe_url(url), "`{url}` 应被拒绝");
    }
}

/// 实体绕过：`java&#115;cript:` 还原后仍是 `javascript:`。
#[test]
fn entity_encoded_javascript_is_still_rejected() {
    let out = sanitize_html("<a href=\"java&#115;cript:alert(1)\">x</a>");
    assert!(
        !out.to_lowercase().contains("javascript"),
        "实体编码不得绕过协议检查，实际：{out}"
    );
}

// ---------------------------------------------------------------------------
// 标签分类处理
// ---------------------------------------------------------------------------

#[test]
fn drop_tags_are_removed_with_subtree() {
    // 只测**非空元素**的 DROP 标签——它们才有「子树」可言。
    for tag in [
        "script", "style", "iframe", "object", "form", "svg", "math", "video", "audio", "canvas",
        "template", "noscript", "picture", "portal", "dialog",
    ] {
        let payload = format!("<{tag}>内部内容</{tag}>");
        let out = sanitize_html(&payload);
        assert_eq!(out, "", "<{tag}> 子树必须整体丢弃，实际：{out}");
    }
}

#[test]
fn void_drop_tags_are_removed_without_a_subtree() {
    // `embed` / `source` / `track` 是 HTML **空元素**，本就没有子树。
    // 浏览器的 DOMParser（即原型所用）会把 `<embed>文字</embed>` 解析成
    // 「embed 元素 + 兄弟文本节点」，因此正确输出是保留文字、只丢标签。
    // 本实现按同一语义处理；若反过来「吞掉后续全部内容」，
    // 就等于让一个空元素否决掉整篇文档（真实缺陷：曾使导出 HTML 变成空文件）。
    for tag in ["embed", "source", "track", "link", "meta"] {
        let payload = format!("<{tag}>内部内容</{tag}>");
        let out = sanitize_html(&payload);
        assert_eq!(
            out, "内部内容",
            "<{tag}> 是空元素，应只丢标签、保留后续文本"
        );
    }
    // 未闭合的空元素同样不得吞掉后文。
    assert_eq!(
        sanitize_html("<meta charset=\"utf-8\"><p>正文</p>"),
        "<p>正文</p>"
    );
}

#[test]
fn unknown_tags_are_unwrapped_keeping_text() {
    assert_eq!(
        sanitize_html("<foo>可见文本</foo>"),
        "可见文本",
        "未知标签应拆壳保留文本"
    );
    // `custom-element` 是未知标签应拆壳；但其内部的 `strong` 是**白名单标签**，
    // 必须连标签一起保留（这正是「拆壳只丢壳、不丢内容」的语义）。
    assert_eq!(
        sanitize_html("<custom-element><strong>粗</strong></custom-element>"),
        "<strong>粗</strong>"
    );
    // 注意 `<b>` 不在白名单内（与原型一致，白名单只有 `strong`），
    // 故它会被拆壳成纯文本——这不是缺陷，而是与原型对齐的取舍。
    assert_eq!(
        sanitize_html("<b>粗</b>"),
        "粗",
        "`<b>` 不在白名单，按原型规则拆壳"
    );
}

#[test]
fn allowed_tags_survive_unchanged() {
    let out = sanitize_html("<h1>标题</h1><p>段落<strong>粗体</strong></p>");
    assert!(out.contains("<h1>"), "允许标签应保留：{out}");
    assert!(out.contains("<strong>"), "允许标签应保留：{out}");
    assert!(out.contains("段落"), "文本应保留");
}

#[test]
fn comments_and_declarations_are_discarded() {
    // 注释里可能藏 `<script>`，必须整体丢弃。
    let out = sanitize_html("<!-- <script>alert(1)</script> --><p>正文</p>");
    assert!(!out.contains("script"), "注释必须被丢弃，实际：{out}");
    assert!(out.contains("正文"), "注释外的正文应保留");
    assert_eq!(sanitize_html("<!DOCTYPE html><p>x</p>"), "<p>x</p>");
}

/// 嵌套的 DROP 标签必须按深度配对，不能被内部同名标签提前"闭合"。
#[test]
fn nested_drop_tags_require_matching_close() {
    let out = sanitize_html("<svg><svg><script>alert(1)</script></svg></svg><p>正常</p>");
    assert!(!out.contains("alert"), "嵌套 svg 内的脚本文本必须被丢弃");
    assert!(out.contains("正常"), "svg 之后的正常内容应保留");
}

#[test]
fn unclosed_drop_tag_swallows_remainder() {
    // `<svg>` 未闭合：其后内容都在 svg 子树内，必须一并丢弃。
    let out = sanitize_html("<p>前</p><svg><p>后</p>");
    assert!(out.contains("前"), "svg 之前的内容保留");
    assert!(!out.contains("后"), "svg 之后的内容应被丢弃，实际：{out}");
}

// ---------------------------------------------------------------------------
// 输出良构性
// ---------------------------------------------------------------------------

#[test]
fn output_is_well_formed_by_closing_open_tags() {
    // 未闭合的允许标签应被补齐，输出仍是良构 HTML。
    let out = sanitize_html("<div><p>未闭合");
    assert_eq!(out, "<div><p>未闭合</p></div>");
}

#[test]
fn stray_end_tags_are_ignored() {
    assert_eq!(sanitize_html("</p>孤立结束标签"), "孤立结束标签");
    assert_eq!(sanitize_html("文字</div>"), "文字");
}

#[test]
fn self_closing_forms_are_handled() {
    let out = sanitize_html("<br /><hr/>");
    assert!(out.contains("<br"), "自闭合 br 应保留：{out}");
    assert!(out.contains("<hr"), "自闭合 hr 应保留：{out}");
}

#[test]
fn a_target_blank_gets_noopener() {
    let out = sanitize_html("<a href=\"https://x.com\" target=\"_blank\">链接</a>");
    assert!(
        out.contains("noopener"),
        "target=_blank 必须补 noopener 防反向标签页劫持，实际：{out}"
    );
}

#[test]
fn attribute_values_cannot_break_out_of_quotes() {
    // 属性值里的引号必须被转义，否则可逃逸出属性上下文注入新属性。
    let out = sanitize_html("<a href=\"x\" title=\"a&quot; onmouseover=&quot;alert(1)\">t</a>");
    assert!(
        !out.contains("onmouseover=\""),
        "属性值内的引号必须被转义，实际：{out}"
    );
}

#[test]
fn plain_text_is_untouched() {
    assert_eq!(sanitize_html("纯文本内容"), "纯文本内容");
    assert_eq!(sanitize_html(""), "");
    assert_eq!(
        sanitize_html("中文与 English 混排，标点。"),
        "中文与 English 混排，标点。"
    );
}

#[test]
fn realistic_markdown_export_output_is_preserved() {
    // 正常导出内容应基本原样通过——清洗器不能误伤合法结构。
    let html = "<h1>标题</h1>\n<p>段落 <em>斜体</em> <strong>粗体</strong> <code>代码</code></p>\n<ul><li>项一</li><li>项二</li></ul>\n<table><thead><tr><th>列</th></tr></thead></table>";
    let out = sanitize_html(html);
    for expected in [
        "<h1>", "<em>", "<strong>", "<code>", "<ul>", "<li>", "<table>", "<th>",
    ] {
        assert!(out.contains(expected), "合法结构 {expected} 被误伤：{out}");
    }
}

#[test]
fn void_drop_tags_do_not_swallow_the_rest_of_the_document() {
    // `meta` / `link` 同时是 DROP_TAGS 与 HTML 空元素，没有结束标签。
    // 若进入丢弃模式，`</meta>` 永不出现，后续内容会被整篇丢弃
    // （真实缺陷：曾导致导出的 HTML 变成空文件）。
    assert_eq!(
        sanitize_html("<meta charset=\"utf-8\"><p>正文</p>"),
        "<p>正文</p>",
        "空元素 DROP 标签不得开启丢弃模式"
    );
    assert_eq!(
        sanitize_html("<p>前</p><link rel=\"x\"><p>后</p>"),
        "<p>前</p><p>后</p>",
        "`link` 之后的内容必须保留"
    );
    // 非空元素的 DROP 行为不受影响。
    assert_eq!(
        sanitize_html("<svg><p>被丢弃</p></svg><p>保留</p>"),
        "<p>保留</p>"
    );
}

#[test]
fn unclosed_non_void_drop_tag_still_swallows_remainder() {
    // 对照用例：非空的 DROP 标签未闭合时**确实应该**吞掉后续内容
    // （保守策略：宁可少显示，也不能漏掉一个可执行标签）。
    assert_eq!(sanitize_html("<script>alert(1)"), "");
    assert_eq!(
        sanitize_html("<style>p{color:red}</style><p>正文</p>"),
        "<p>正文</p>"
    );
}
