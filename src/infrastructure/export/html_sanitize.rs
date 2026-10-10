//! HTML 白名单清洗（S5-09）。
//!
//! # 为什么导出必须清洗
//!
//! Markdown 允许内嵌任意原始 HTML，而导出的 HTML 会被用户在浏览器里打开。
//! 一篇含 `<script>` 或 `<img onerror=...>` 的文档就是一次脚本注入。
//! 原型在浏览器里用 `DOMParser` 实现清洗（`markdown-editor-prototype/src/lib/sanitize.ts`），
//! 但浏览器没有对应物，故本文件用**手写词法分析**实现同样的规则。
//!
//! # 三类标签的三种处理（这是全部安全性的来源）
//!
//! | 类别 | 处理 | 理由 |
//! |---|---|---|
//! | `DROP_TAGS` | 连同**子树**丢弃 | `svg` / `math` 等的子树常用外部命名空间（`<svg><script>`），拆壳会让未检查的节点逃逸清洗 |
//! | `ALLOWED_TAGS` | 保留，属性按白名单过滤 | 正常内容 |
//! | 其余（未知标签） | **拆壳保留文本** | `<foo>文字</foo>` 应留下「文字」；但搬出的子节点必须**逐个重新清洗** |
//!
//! 第三类是原型真实踩过的坑：`<foo><img src=x onerror=alert(1)></foo>` 若拆壳时
//! 直接跳过清洗，`onerror` 就会存活。本实现用**栈 + 递归下降**保证每个节点都被清洗。
//!
//! # 为什么不用 HTML 解析库
//!
//! 引入解析库会带来体积风险（§8.1 预算 10 MB），而本清洗器只需处理
//! pulldown-cmark 产出的**规整** HTML。词法分析器对畸形输入采取
//! 「无法识别就原样转义」的保守策略——宁可多显示一点文本，
//! 也不能漏掉一个可执行标签。
//!
//! # URL 白名单（`safeUrl`）
//!
//! 关键约束：**不能放行整个 `data:`**。`data:text/html;base64,` 是 XSS 向量。
//! 故只精确放行位图 data URI（`png|jpeg|jpg|gif|webp|bmp`），
//! 这与原型的正则 `^data:image\/(png|jpeg|jpg|gif|webp|bmp);base64,[a-z0-9+/=\s]*$` 一致。

/// 允许保留的标签集合。
const ALLOWED_TAGS: &[&str] = &[
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
const DROP_TAGS: &[&str] = &[
    "script", "style", "iframe", "object", "embed", "form", "link", "meta", "template", "noscript",
    "svg", "math", "video", "audio", "canvas", "source", "track", "picture", "portal", "dialog",
];

/// 需要做 URL 安全校验的属性。
const URI_ATTRS: &[&str] = &["href", "src"];

/// 允许保留的属性集合。
const ALLOWED_ATTRS: &[&str] = &[
    "class", "id", "alt", "title", "lang", "colspan", "rowspan", "align", "start", "type",
    "checked", "disabled", "target", "rel", "width", "height", "dir",
];

/// 合法 data URI：仅位图的 base64 内联图。
///
/// 注意 `data:text/html` **不在**此列——它是 XSS 向量，必须剥除。
const SAFE_DATA_IMAGE: &str = "data:image/";

/// 内联图片允许的 MIME 子类型。
const SAFE_DATA_IMAGE_TYPES: &[&str] = &["png", "jpeg", "jpg", "gif", "webp", "bmp"];

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
fn is_void_element(name: &str) -> bool {
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
    SAFE_DATA_IMAGE_TYPES.contains(&mime) && !payload.trim().is_empty()
}

/// 单个属性。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Attribute {
    name: String,
    value: String,
}

/// HTML 词法分析器：把输入切成「文本 / 开始标签 / 结束标签」三类记号。
///
/// 只做词法不做树构建——树结构由 [`sanitize_html`] 用栈维护。
/// 这样可保证「拆壳后重新清洗子节点」自然发生：每个记号都会被
/// 独立处理一次，不存在「搬移后忘记清洗」的可能。
struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
}

/// 词法记号。
enum Token {
    /// 原样文本（已由上游 HTML 转义，或来自 Markdown 纯文本）。
    Text(String),
    /// 开始标签；`self_closing` 表示 `<br />` 这类。
    Start {
        name: String,
        attrs: Vec<Attribute>,
        self_closing: bool,
    },
    /// 结束标签。
    End(String),
    /// 注释 / 声明 / CDATA：整体丢弃（其中也可能藏 `<script>`）。
    Ignorable,
}

impl<'a> Tokenizer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    /// 取出下一个记号；输入耗尽时返回 `None`。
    fn next_token(&mut self) -> Option<Token> {
        let bytes = self.input.as_bytes();
        if self.pos >= bytes.len() {
            return None;
        }
        // 找下一个 `<`；其前的部分都是纯文本。
        let start = self.pos;
        match self.input[self.pos..].find('<') {
            Some(offset) => {
                let lt = self.pos + offset;
                if lt > start {
                    self.pos = lt;
                    return Some(Token::Text(self.input[start..lt].to_string()));
                }
            }
            None => {
                self.pos = bytes.len();
                return Some(Token::Text(self.input[start..].to_string()));
            }
        }

        // 从 `<` 开始识别标签类型。
        let rest = &self.input[self.pos..];
        if rest.starts_with("<!--") {
            // 注释：找到 `-->`；未闭合则丢弃到末尾。
            self.pos = match rest.find("-->") {
                Some(offset) => self.pos + offset + 3,
                None => self.input.len(),
            };
            return Some(Token::Ignorable);
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            // 声明 / 处理指令：丢弃到下一个 `>`。
            self.pos = match rest.find('>') {
                Some(offset) => self.pos + offset + 1,
                None => self.input.len(),
            };
            return Some(Token::Ignorable);
        }

        let closing = rest.starts_with("</");
        let body_start = if closing { 2 } else { 1 };
        let body = &rest[body_start..];
        // 标签名：到空白、`>` 或 `/` 为止。
        let name_end = body
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(body.len());
        let raw_name = &body[..name_end];
        if raw_name.is_empty() {
            // `<` 后面不是标签名（如 `a < b`）——按文本处理，避免死循环。
            self.pos += 1;
            return Some(Token::Text("<".to_string()));
        }
        let name = raw_name.to_lowercase();

        if closing {
            // 结束标签：找到 `>` 即可（属性在结束标签上无意义）。
            self.pos = match rest.find('>') {
                Some(offset) => self.pos + offset + 1,
                None => self.input.len(),
            };
            return Some(Token::End(name));
        }

        // 开始标签：逐个解析属性，直到 `>` 或 `/>`。
        let attrs_start = self.pos + body_start + name_end;
        let (attrs, self_closing, consumed) = self.parse_attrs(attrs_start);
        self.pos = consumed;
        Some(Token::Start {
            name,
            attrs,
            self_closing,
        })
    }

    /// 从 `start` 起解析属性列表，返回（属性、自闭合标记、新位置）。
    fn parse_attrs(&self, start: usize) -> (Vec<Attribute>, bool, usize) {
        let bytes = self.input.as_bytes();
        let mut attrs = Vec::new();
        let mut i = start;
        let mut self_closing = false;

        while i < bytes.len() {
            // 跳过空白。
            while i < bytes.len() && (bytes[i] as char).is_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            if bytes[i] == b'>' {
                i += 1;
                break;
            }
            if bytes[i] == b'/' {
                self_closing = true;
                i += 1;
                continue;
            }

            // 属性名：到 `=`、空白、`>` 或 `/` 为止。
            let name_start = i;
            while i < bytes.len()
                && bytes[i] != b'='
                && bytes[i] != b'>'
                && bytes[i] != b'/'
                && !(bytes[i] as char).is_whitespace()
            {
                i += 1;
            }
            let name = self.input[name_start..i].to_lowercase();
            if name.is_empty() {
                i += 1;
                continue;
            }

            // 跳过 `=` 与其后的空白。
            while i < bytes.len() && (bytes[i] as char).is_whitespace() {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b'=' {
                // 无值的属性（如 `disabled`）：值记为空串。
                attrs.push(Attribute {
                    name,
                    value: String::new(),
                });
                continue;
            }
            i += 1; // 跳过 '='
            while i < bytes.len() && (bytes[i] as char).is_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }

            // 带引号 / 不带引号的属性值。
            let value = if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                i += 1;
                let value_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                let value = &self.input[value_start..i.min(bytes.len())];
                if i < bytes.len() {
                    i += 1; // 跳过收尾引号
                }
                value
            } else {
                let value_start = i;
                while i < bytes.len() && bytes[i] != b'>' && !(bytes[i] as char).is_whitespace() {
                    i += 1;
                }
                &self.input[value_start..i]
            };
            attrs.push(Attribute {
                name,
                value: unescape_html(value),
            });
        }
        (attrs, self_closing, i)
    }
}

/// 还原 HTML 实体的**最小**部分：仅 `&amp;` `&lt;` `&gt;` `&quot;` `&#39;`。
///
/// 只为让 URL 判定不受实体绕过影响（如 `java&#115;cript:`），
/// 不做完整实体表——完整还原反而会引入新的解析歧义。
fn unescape_html(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
}

/// 按白名单过滤标签的属性，并强制 `a[target=_blank]` 补 `rel=noopener`。
///
/// 返回 `None` 表示该元素应被丢弃（如 `<a>` 的 `href` 不安全时
/// 剥除属性即可，不必丢整个元素；但 `<img src=javascript:>` 的 `src`
/// 被剥后图片自然失效，同样保留元素是安全的）。
fn filter_attributes(tag: &str, attrs: &[Attribute]) -> Vec<Attribute> {
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
fn render_attributes(attrs: &[Attribute]) -> String {
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

/// 对 HTML 做白名单清洗，返回可安全嵌入的片段。
///
/// 处理流程（每个记号独立处理，天然保证「拆壳后重新清洗」）：
/// 1. `DROP_TAGS`：进入丢弃模式，直到配对的结束标签——期间**所有**内容都不输出；
/// 2. `ALLOWED_TAGS`：输出，属性按白名单过滤；
/// 3. 未知标签：只输出其**文本子树**（拆壳），子节点在后续记号中被正常处理。
///
/// # 示例
///
/// ```
/// use rustmark::infrastructure::export::html_sanitize::sanitize_html;
///
/// // 事件属性被剥除。
/// assert_eq!(sanitize_html("<img src=x onerror=alert(1)>"), "<img>");
/// // 未知标签拆壳，但内部 img 仍被清洗。
/// assert_eq!(sanitize_html("<foo><img src=x onerror=alert(1)></foo>"), "<img>");
/// // DROP_TAGS 连子树一起丢弃。
/// assert_eq!(sanitize_html("<svg><script>alert(1)</script></svg>"), "");
/// ```
pub fn sanitize_html(html: &str) -> String {
    let mut tokenizer = Tokenizer::new(html);
    let mut out = String::with_capacity(html.len());

    // 丢弃模式栈：记录正在被丢弃的标签名与其嵌套深度。
    // 非空时**一切内容都不输出**（文本、允许标签、未知标签统统跳过）。
    let mut drop_stack: Vec<String> = Vec::new();

    // 开放标签栈：用于补齐未闭合的允许标签，保证输出是良构 HTML。
    let mut open_stack: Vec<String> = Vec::new();

    while let Some(token) = tokenizer.next_token() {
        // 丢弃模式：只跟踪嵌套深度，不输出任何内容。
        if !drop_stack.is_empty() {
            match &token {
                // 嵌套的同名 DROP 标签：入栈，深度 +1。
                Token::Start {
                    name,
                    self_closing: false,
                    ..
                } if DROP_TAGS.contains(&name.as_str()) => drop_stack.push(name.clone()),
                // 只有闭合**栈顶**那个才出栈：否则 `<svg><svg></svg>` 的第一个
                // `</svg>` 就会提前退出丢弃模式，让后续内容漏进输出。
                Token::End(name) if drop_stack.last() == Some(name) => {
                    drop_stack.pop();
                }
                _ => {}
            }
            continue;
        }

        match token {
            Token::Text(text) => out.push_str(&text),
            Token::Ignorable => {}
            Token::Start {
                name,
                attrs,
                self_closing,
            } => {
                if DROP_TAGS.contains(&name.as_str()) {
                    // 自闭合的 DROP 标签（如 `<svg />`）无需进入丢弃模式。
                    // **空元素同理**：`meta` / `link` / `source` / `track` / `embed`
                    // 既在 DROP_TAGS 又是 HTML 空元素，根本没有结束标签——
                    // 若照样入栈，`</meta>` 永远不会出现，后续**整篇文档**
                    // 都会被丢弃（实测：导出的 HTML 变成空文件）。
                    if !self_closing && !is_void_element(&name) {
                        drop_stack.push(name);
                    }
                    continue;
                }
                if ALLOWED_TAGS.contains(&name.as_str()) {
                    let safe_attrs = filter_attributes(&name, &attrs);
                    out.push('<');
                    out.push_str(&name);
                    out.push_str(&render_attributes(&safe_attrs));
                    if self_closing {
                        out.push_str(" />");
                    } else {
                        out.push('>');
                        // 空元素（void）在 HTML 中没有结束标签，
                        // 不能压入开放栈——否则会凭空生成 `</img>` 这类非法标记。
                        if !is_void_element(&name) {
                            open_stack.push(name);
                        }
                    }
                    continue;
                }
                // 未知标签：**拆壳**——不输出标签本身，但保留其文本子树。
                // 子节点会在后续记号中被独立清洗，故 `on*` 属性无处逃逸。
            }
            Token::End(name) => {
                // 丢弃模式下已统一处理；此处只关心允许标签与未知标签的结束。
                if DROP_TAGS.contains(&name.as_str()) {
                    // 未配对的 DROP 结束标签：忽略。
                    continue;
                }
                if ALLOWED_TAGS.contains(&name.as_str()) {
                    // 只闭合**最近开启的同名标签**，避免错乱嵌套导致结构破坏。
                    if let Some(position) = open_stack.iter().rposition(|open| *open == name) {
                        // 先闭合其上方的标签，保证输出良构。
                        while open_stack.len() > position {
                            if let Some(open) = open_stack.pop() {
                                out.push_str(&format!("</{open}>"));
                            }
                        }
                    }
                    continue;
                }
                // 未知标签的结束标签：拆壳，无需输出。
            }
        }
    }

    // 补齐仍未闭合的标签，保证输出是良构 HTML。
    while let Some(open) = open_stack.pop() {
        out.push_str(&format!("</{open}>"));
    }
    out
}
