//! 清洗策略：标签 / 属性 / URL 白名单与空元素判定。

use crate::infrastructure::image_policy::is_allowed_image_subtype;

/// 允许保留的标签集合。
pub(super) const ALLOWED_TAGS: &[&str] = &[
    "p",
    "br",
    "hr",
    "span",
    "div",
    "pre",
    "code",
    "em",
    "strong",
    "del",
    "s",
    "sup",
    "sub",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "blockquote",
    "a",
    "img",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
    "input",
    "dl",
    "dt",
    "dd",
    "kbd",
    "mark",
];

/// 连同子树一起丢弃的标签。
///
/// `svg` / `math` / `video` / `audio` / `source` 等必须丢弃而非「拆壳保留文本」——
/// 它们的子树常用外部命名空间，拆壳会让未检查的节点逃逸清洗。
pub(super) const DROP_TAGS: &[&str] = &[
    "script", "style", "iframe", "object", "embed", "form", "link", "meta", "template", "noscript",
    "svg", "math", "video", "audio", "canvas", "source", "track", "picture", "portal", "dialog",
];

/// 需要做 URL 安全校验的属性。
pub(super) const URI_ATTRS: &[&str] = &["href", "src"];

/// 允许保留的属性集合。
pub(super) const ALLOWED_ATTRS: &[&str] = &[
    "class", "id", "alt", "title", "lang", "colspan", "rowspan", "align", "start", "type",
    "checked", "disabled", "target", "rel", "width", "height", "dir",
];

/// 合法 data URI：仅位图的 base64 内联图。
///
/// 注意 `data:text/html` **不在**此列——它是 XSS 向量，必须剥除。
const SAFE_DATA_IMAGE: &str = "data:image/";

/// 允许的 URL 协议前缀。
const SAFE_URL_SCHEMES: &[&str] = &["http:", "https:", "mailto:", "tel:"];

/// HTML 空元素：没有结束标签，内容永远为空。
///
/// 这些元素若被压入开放栈，清洗器会在收尾时补出 `<img></img>` 这类
/// 非法标记——虽不产生 XSS，但会让导出的 HTML 不合法、且在部分
/// 解析器里引发怪异行为。
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// 该标签是否为 HTML 空元素。
pub(super) fn is_void_element(name: &str) -> bool {
    VOID_ELEMENTS.contains(&name)
}

/// 判定 URL 是否安全。
///
/// 与原型 `safeUrl` 语义一致：空值放行（`<a href>`）、锚点与相对路径放行、
/// 位图 data URI 放行，其余只放行白名单协议。
pub fn is_safe_url(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_lowercase();

    // 位图 data URI：必须**精确**匹配 `data:image/< subtype>;base64,`。
    if lower.starts_with(SAFE_DATA_IMAGE) {
        return is_safe_data_image(&lower);
    }
    // 任何其它 `data:` 一律拒绝（含 `data:text/html`）。
    if lower.starts_with("data:") {
        return false;
    }
    // 锚点与相对路径。
    if lower.starts_with('#')
        || lower.starts_with('/')
        || lower.starts_with("./")
        || lower.starts_with("../")
    {
        return true;
    }
    SAFE_URL_SCHEMES
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

/// 判定是否为合法的内联位图 data URI。
///
/// 只看前缀部分：`data:image/<type>;base64,` 且 `<type>` 在白名单内。
/// 载荷部分只要求非空——完整校验 base64 正则意义有限（浏览器会解码），
/// 真正的防线是**类型白名单**：`data:image/svg+xml` 已被排除在外，
/// 因为 SVG 内部可以携带脚本。
fn is_safe_data_image(lower: &str) -> bool {
    let Some(rest) = lower.strip_prefix(SAFE_DATA_IMAGE) else {
        return false;
    };
    let Some((mime, payload)) = rest.split_once(";base64,") else {
        return false;
    };
    // 必须精确等于某个白名单类型（`image/png` 不能匹配 `image/pngx`）。
    // 白名单取 `image_policy` 的真源，与预览侧的加载器共用同一张表——
    // 各持一份时，同一张图会在「预览放行 / 导出被剥」之间悄悄不一致。
    is_allowed_image_subtype(mime) && !payload.trim().is_empty()
}
