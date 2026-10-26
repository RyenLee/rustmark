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
//!
//! # 模块结构
//!
//! - `policy`：标签 / 属性 / URL 白名单与空元素判定；
//! - `tokenizer`：手写词法分析器与记号类型；
//! - `writer`：属性白名单过滤与回写。
//!
//! 本 facade 只保留 `sanitize_html` 的栈式主循环。

mod policy;
mod tokenizer;
mod writer;

pub use policy::is_safe_url;

use policy::{ALLOWED_TAGS, DROP_TAGS, is_void_element};
use tokenizer::{Token, Tokenizer};
use writer::{filter_attributes, render_attributes};

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
/// use rustmark_infra::infrastructure::export::html_sanitize::sanitize_html;
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
