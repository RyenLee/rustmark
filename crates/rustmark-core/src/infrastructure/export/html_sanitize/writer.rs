//! 属性过滤与回写：白名单过滤、`target=_blank` 强制 `rel=noopener`、属性渲染。

use super::policy::{ALLOWED_ATTRS, URI_ATTRS, is_safe_url};
use super::tokenizer::Attribute;

/// 按白名单过滤标签的属性，并强制 `a[target=_blank]` 补 `rel=noopener`。
///
/// 返回 `None` 表示该元素应被丢弃（如 `<a>` 的 `href` 不安全时
/// 剥除属性即可，不必丢整个元素；但 `<img src=javascript:>` 的 `src`
/// 被剥后图片自然失效，同样保留元素是安全的）。
pub(super) fn filter_attributes(tag: &str, attrs: &[Attribute]) -> Vec<Attribute> {
    let mut out = Vec::new();
    for attr in attrs {
        if URI_ATTRS.contains(&attr.name.as_str()) {
            if is_safe_url(&attr.value) {
                out.push(attr.clone());
            }
            // 不安全则**剥除属性**（而非保留原值）。
            continue;
        }
        if ALLOWED_ATTRS.contains(&attr.name.as_str()) {
            out.push(attr.clone());
        }
        // 其余属性（含全部 `on*` 事件属性）一律丢弃。
    }
    // 反向标签页劫持：target=_blank 必须配 noopener。
    if tag == "a"
        && let Some(target) = attrs.iter().find(|a| a.name == "target")
        && target.value.eq_ignore_ascii_case("_blank")
    {
        out.retain(|a| a.name != "rel");
        out.push(Attribute {
            name: "rel".to_string(),
            value: "noopener noreferrer".to_string(),
        });
    }
    out
}

/// 把属性列表渲染回 HTML。
pub(super) fn render_attributes(attrs: &[Attribute]) -> String {
    let mut out = String::new();
    for attr in attrs {
        out.push(' ');
        out.push_str(&attr.name);
        if !attr.value.is_empty() {
            out.push_str("=\"");
            // 属性值内的引号必须转义，否则可逃逸出属性上下文。
            out.push_str(
                &attr
                    .value
                    .replace('&', "&amp;")
                    .replace('"', "&quot;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;"),
            );
            out.push('"');
        }
    }
    out
}
