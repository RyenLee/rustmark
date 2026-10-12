//! [`Parser`] 的命令解析方法：`parse_command` / 符号查表 / 参数读取。

use super::*;

impl Parser {
    /// 解析一个命令（含它的参数），返回对应节点。
    pub(super) fn parse_command(&mut self) -> Result<Node, MathError> {
        let name = self.read_command_name()?;
        match name.as_str() {
            // —— 文本 / 字体族 ——
            "text" | "textrm" | "textnormal" | "mbox" => {
                let raw = self.read_group_raw()?;
                Ok(Node::Text {
                    text: raw.replace('~', "\u{a0}"),
                    font: MathFont::Upright,
                })
            }
            "mathrm" | "mathbf" | "boldsymbol" | "operatorname" => {
                let body = self.read_group()?;
                Ok(Node::Row(body))
            }
            "mathbb" | "Bbb" => {
                let body = self.read_group()?;
                Ok(Node::Row(retext(body, MathFont::Blackboard)))
            }
            "mathcal" | "mathscr" | "mathfrak" => {
                let body = self.read_group()?;
                Ok(Node::Row(retext(body, MathFont::Calligraphic)))
            }
            "mathit" => {
                let body = self.read_group()?;
                Ok(Node::Row(retext(body, MathFont::Italic)))
            }
            // —— 结构 ——
            "frac" | "dfrac" | "tfrac" => {
                let num = self.read_group()?;
                let den = self.read_group()?;
                Ok(Node::Frac {
                    num,
                    den,
                    big: name != "tfrac",
                })
            }
            "sqrt" => {
                // 可选的 `[n]` 次根。
                let index = if self.peek() == Some('[') {
                    self.bump();
                    let idx = self.parse_until(']')?;
                    self.expect(']')?;
                    Some(idx)
                } else {
                    None
                };
                let body = self.read_group()?;
                Ok(Node::Sqrt { index, body })
            }
            "left" => self.parse_delim(),
            "begin" => self.parse_environment(),
            "overline" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Bar,
                    body,
                })
            }
            "hat" | "widehat" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Hat,
                    body,
                })
            }
            "bar" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Bar,
                    body,
                })
            }
            "vec" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Vec,
                    body,
                })
            }
            "dot" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Dot,
                    body,
                })
            }
            "tilde" => {
                let body = self.read_group()?;
                Ok(Node::Accent {
                    kind: Accent::Tilde,
                    body,
                })
            }
            "displaystyle" | "textstyle" | "limits" | "nolimits" => Ok(Node::Row(Vec::new())),
            // 拉伸类命令（`\bigl` …）在本渲染器里没有对应的伸缩语义，
            // 直接忽略：它们只影响括号大小的审美，不影响可读性。
            "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" | "Biggl" | "Biggr" | "big"
            | "Big" | "bigg" | "Bigg" => {
                // 后面跟的定界符要照常输出。
                self.parse_single_delim()
            }
            "quad" => Ok(Node::Space(1.0)),
            "qquad" => Ok(Node::Space(2.0)),
            "," | ";" | ":" | " " | "!" => Ok(Node::Space(if name == "!" { -0.15 } else { 0.22 })),
            "\\" => Err(MathError::Malformed(
                "`\\\\`（换行）只允许出现在矩阵环境里".to_string(),
            )),
            _ => self.parse_symbol_or_op(&name),
        }
    }

    /// 符号表 / 大运算符 / 函数名三种查表，都查不到就是子集外。
    pub(super) fn parse_symbol_or_op(&mut self, name: &str) -> Result<Node, MathError> {
        if LIMIT_OPS.contains(&name) || matches!(name, "int" | "iint" | "iiint" | "oint") {
            let glyph = SYMBOLS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, g)| (*g).to_string())
                .ok_or_else(|| MathError::Unsupported(format!("\\{name}")))?;
            return Ok(Node::BigOp {
                glyph,
                limits: self.display && LIMIT_OPS.contains(&name),
            });
        }
        if FUNCTIONS.contains(&name) {
            return Ok(Node::Fn(name.to_string()));
        }
        if let Some((_, glyph)) = SYMBOLS.iter().find(|(n, _)| *n == name) {
            return Ok(Node::Text {
                text: (*glyph).to_string(),
                font: MathFont::Upright,
            });
        }
        Err(MathError::Unsupported(format!("\\{name}")))
    }

    /// 读一个必选参数 `{...}`（也接受单字符形式的 `\frac12`）。
    pub(super) fn read_group(&mut self) -> Result<Vec<Node>, MathError> {
        self.skip_spaces();
        match self.peek() {
            Some('{') => {
                self.pos += 1;
                let inner = self.parse_row(None)?;
                self.expect('}')?;
                Ok(inner)
            }
            Some('\\') => Ok(vec![self.parse_command()?]),
            Some(c) if !matches!(c, ']' | ')' | '}' | '&') => {
                self.pos += 1;
                Ok(vec![Node::Text {
                    text: c.to_string(),
                    font: default_font_for(c),
                }])
            }
            _ => Err(MathError::Malformed("命令缺少参数".to_string())),
        }
    }

    /// 读 `{...}` 的**原始文本**（`\text{}` 用：里面不解析数学命令）。
    pub(super) fn read_group_raw(&mut self) -> Result<String, MathError> {
        self.skip_spaces();
        self.expect('{')?;
        let mut depth = 1usize;
        let mut out = String::new();
        while let Some(c) = self.bump() {
            match c {
                '{' => {
                    depth += 1;
                    out.push(c);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push(c);
                }
                // `\` 在 `\text{}` 里只用于转义，直接吞掉反斜杠保留字符。
                '\\' => {
                    if let Some(next) = self.bump() {
                        out.push(next);
                    }
                }
                _ => out.push(c),
            }
        }
        Err(MathError::Malformed("`{` 没有闭合".to_string()))
    }
}
