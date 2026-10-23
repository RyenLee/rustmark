//! 手写 HTML 词法分析器：把输入切成「文本 / 开始标签 / 结束标签」三类记号。

/// 单个属性。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Attribute {
    pub(super) name: String,
    pub(super) value: String,
}

/// HTML 词法分析器：把输入切成「文本 / 开始标签 / 结束标签」三类记号。
///
/// 只做词法不做树构建——树结构由 [`sanitize_html`](super::sanitize_html) 用栈维护。
/// 这样可保证「拆壳后重新清洗子节点」自然发生：每个记号都会被
/// 独立处理一次，不存在「搬移后忘记清洗」的可能。
pub(super) struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
}

/// 词法记号。
pub(super) enum Token {
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
    pub(super) fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    /// 取出下一个记号；输入耗尽时返回 `None`。
    pub(super) fn next_token(&mut self) -> Option<Token> {
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
