//! 解析器：在字符序列上做**单遍**下降解析。

use super::model::{Accent, MathFont, Node, default_font_for, retext};
use super::symbols::{ENVIRONMENTS, FUNCTIONS, LIMIT_OPS, SYMBOLS};
use super::*;

/// 解析器：在字符序列上做**单遍**下降解析。
struct Parser {
    src: Vec<char>,
    pos: usize,
    /// 当前是否处于 display（行间）模式。
    ///
    /// 只影响一件事：大运算符的上下限是叠放还是放在右侧。
    /// 与 TeX 一致——行内 `$\sum_{i}$` 也是右侧，否则会撑高行距。
    display: bool,
}

/// 解析一段 LaTeX 为语法树。
pub(super) fn parse(src: &str, display: bool) -> Result<Vec<Node>, MathError> {
    let mut p = Parser {
        src: src.chars().collect(),
        pos: 0,
        display,
    };
    let nodes = p.parse_row(None)?;
    p.skip_spaces();
    if p.pos < p.src.len() {
        // 只可能是多出来的 `}` 或 `\right`——都是语法问题。
        return Err(MathError::Malformed(format!(
            "第 {} 个字符起有多余内容：{}",
            p.pos,
            p.rest_preview()
        )));
    }
    Ok(nodes)
}

impl Parser {
    /// 剩余内容的前若干字符（错误信息用）。
    fn rest_preview(&self) -> String {
        self.src[self.pos..].iter().take(16).collect()
    }

    fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn skip_spaces(&mut self) {
        while matches!(
            self.peek(),
            Some(' ') | Some('\t') | Some('\n') | Some('\r')
        ) {
            self.pos += 1;
        }
    }

    /// 解析一串节点，直到输入结束或遇到 `stop` 指定的**结束控制序列**。
    ///
    /// `stop` 为 `Some("right")` 时，`\right` 不被消费——留给 [`Self::parse_delim`]
    /// 读取它后面的定界符。返回时 `self.pos` 指向 `\right` 的 `\`。
    fn parse_row(&mut self, stop: Option<&str>) -> Result<Vec<Node>, MathError> {
        let mut out: Vec<Node> = Vec::new();
        loop {
            self.skip_spaces();
            let Some(c) = self.peek() else { break };
            match c {
                '}' => break,
                // `\\`（矩阵换行）与 `&`（列分隔）由环境解析器消费；
                // 走到这里说明它们出现在矩阵外，按语法错误处理更诚实。
                '&' => {
                    return Err(MathError::Malformed(
                        "`&` 只允许出现在矩阵 / cases 环境里".to_string(),
                    ));
                }
                '^' | '_' => {
                    self.apply_script(&mut out, c)?;
                }
                '{' => {
                    self.pos += 1;
                    let inner = self.parse_row(None)?;
                    self.expect('}')?;
                    out.push(Node::Row(inner));
                }
                '\\' => {
                    if let Some(name) = stop
                        && self.command_is(name)
                    {
                        break;
                    }
                    out.push(self.parse_command()?);
                }
                _ => {
                    self.pos += 1;
                    out.push(Node::Text {
                        // 数字与运算符用正体、字母用斜体——这是数学排版的基本观感，
                        // 不做的话 `abc` 会变成三个斜体字母挤在一起。
                        text: c.to_string(),
                        font: default_font_for(c),
                    });
                }
            }
        }
        Ok(out)
    }

    /// 当前位置是否以 `\name` 开头（`\` + 名字，且名字后不再是字母）。
    fn command_is(&self, name: &str) -> bool {
        if self.peek() != Some('\\') {
            return false;
        }
        let rest: String = self.src[self.pos + 1..].iter().take(name.len()).collect();
        if rest != name {
            return false;
        }
        // `\right` 不能匹配到 `\rightarrow`：后面必须不是字母。
        !self
            .src
            .get(self.pos + 1 + name.len())
            .is_some_and(|c| c.is_ascii_alphabetic())
    }

    fn expect(&mut self, c: char) -> Result<(), MathError> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(MathError::Malformed(format!(
                "期望 `{c}`，实际遇到 {}",
                self.peek()
                    .map_or("输入结束".to_string(), |x| format!("`{x}`"))
            )))
        }
    }

    /// 读一个命令名（`\` 之后）；非字母命令（`\\` `\,` `\{` …）返回单字符。
    fn read_command_name(&mut self) -> Result<String, MathError> {
        self.expect('\\')?;
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                name.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if name.is_empty() {
            // `\\` / `\,` / `\{` / `\%` 等：单个非字母字符。
            match self.bump() {
                Some(c) => Ok(c.to_string()),
                None => Err(MathError::Malformed("`\\` 后面没有内容".to_string())),
            }
        } else {
            Ok(name)
        }
    }

    /// 解析一个命令（含它的参数），返回对应节点。
    fn parse_command(&mut self) -> Result<Node, MathError> {
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
    fn parse_symbol_or_op(&mut self, name: &str) -> Result<Node, MathError> {
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
    fn read_group(&mut self) -> Result<Vec<Node>, MathError> {
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
    fn read_group_raw(&mut self) -> Result<String, MathError> {
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

    /// 解析到指定右界（`[n]` 里的 `]`），不消费右界。
    fn parse_until(&mut self, end: char) -> Result<Vec<Node>, MathError> {
        let mut out: Vec<Node> = Vec::new();
        loop {
            self.skip_spaces();
            let Some(c) = self.peek() else { break };
            if c == end {
                break;
            }
            match c {
                '^' | '_' => self.apply_script(&mut out, c)?,
                '{' => {
                    self.pos += 1;
                    let inner = self.parse_row(None)?;
                    self.expect('}')?;
                    out.push(Node::Row(inner));
                }
                '\\' => out.push(self.parse_command()?),
                _ => {
                    self.pos += 1;
                    out.push(Node::Text {
                        text: c.to_string(),
                        font: default_font_for(c),
                    });
                }
            }
        }
        Ok(out)
    }

    /// `\left( … \right)`：读到 `\right` 为止，两侧定界符一并记下。
    fn parse_delim(&mut self) -> Result<Node, MathError> {
        let left = self.read_delim_char()?;
        let body = self.parse_row(Some("right"))?;
        if !self.command_is("right") {
            return Err(MathError::Malformed(
                "`\\left` 没有配对的 `\\right`".to_string(),
            ));
        }
        self.pos += 1 + "right".len();
        let right = self.read_delim_char()?;
        Ok(Node::Delim { left, right, body })
    }

    /// 读一个定界符（`(` / `]` / `\{` / `|` / `.` ）。
    fn read_delim_char(&mut self) -> Result<String, MathError> {
        self.skip_spaces();
        match self.peek() {
            None => Err(MathError::Malformed("定界符缺失".to_string())),
            Some('\\') => {
                let name = self.read_command_name()?;
                match name.as_str() {
                    "lbrace" => Ok("{".to_string()),
                    "rbrace" => Ok("}".to_string()),
                    "langle" => Ok("⟨".to_string()),
                    "rangle" => Ok("⟩".to_string()),
                    "lceil" => Ok("⌈".to_string()),
                    "rceil" => Ok("⌉".to_string()),
                    "lfloor" => Ok("⌊".to_string()),
                    "rfloor" => Ok("⌋".to_string()),
                    "vert" | "mid" => Ok("|".to_string()),
                    "Vert" => Ok("‖".to_string()),
                    // `\left.` 的「隐形定界符」：不画。
                    "." => Ok(String::new()),
                    other => Err(MathError::Unsupported(format!("\\{other} 作为定界符"))),
                }
            }
            Some('.') => {
                self.pos += 1;
                Ok(String::new())
            }
            Some(c) => {
                self.pos += 1;
                Ok(c.to_string())
            }
        }
    }

    /// `\bigl(` 这类：跳过命令名后，把紧跟的定界符当普通字符输出。
    fn parse_single_delim(&mut self) -> Result<Node, MathError> {
        self.skip_spaces();
        match self.peek() {
            Some('\\') => self.parse_command(),
            Some(c) => {
                self.pos += 1;
                Ok(Node::Text {
                    text: c.to_string(),
                    font: MathFont::Upright,
                })
            }
            None => Ok(Node::Row(Vec::new())),
        }
    }

    /// 把当前格 `cell` 推进网格的**末行**，并清空 `cell`（`mem::take`）。
    ///
    /// 抽成函数是为了避开 `expect`——`src/lib.rs` 把 `clippy::expect_used`
    /// 升成了硬错误。这里的不变式其实成立（`rows` 由 `vec![Vec::new()]` 起家，
    /// 之后每次换行都是「先写末行、再 `push` 新行」的成对操作，末行恒在），
    /// 但仍用 `if let` 直取：万一不变式被破坏，退化成「丢掉当前格」，
    /// 而不是整个进程终止（`panic = "abort"` 下 panic 就是直接退出）。
    fn push_cell(rows: &mut [Vec<Vec<Node>>], cell: &mut Vec<Node>) {
        if let Some(row) = rows.last_mut() {
            row.push(std::mem::take(cell));
        }
    }

    /// `\begin{...} … \end{...}`：矩阵 / cases / aligned。
    fn parse_environment(&mut self) -> Result<Node, MathError> {
        let env = self.read_group_raw()?;
        if !ENVIRONMENTS.contains(&env.as_str()) {
            return Err(MathError::Unsupported(format!("\\begin{{{env}}}")));
        }
        let (open, close) = match env.as_str() {
            "pmatrix" => ("(", ")"),
            "bmatrix" => ("[", "]"),
            "vmatrix" => ("|", "|"),
            "Vmatrix" => ("‖", "‖"),
            "Bmatrix" => ("{", "}"),
            "cases" | "aligned" | "align" => ("", ""),
            // `matrix` 与 `smallmatrix`：无括号。
            _ => ("", ""),
        };

        let mut rows: Vec<Vec<Vec<Node>>> = vec![Vec::new()];
        let mut cell: Vec<Node> = Vec::new();
        loop {
            self.skip_spaces();
            let Some(c) = self.peek() else {
                return Err(MathError::Malformed(format!(
                    "`\\begin{{{env}}}` 缺少 `\\end`"
                )));
            };
            if c == '\\' && self.command_is("end") {
                self.pos += 1 + "end".len();
                let end = self.read_group_raw()?;
                if end != env {
                    return Err(MathError::Malformed(format!(
                        "`\\begin{{{env}}}` 与 `\\end{{{end}}}` 不匹配"
                    )));
                }
                break;
            }
            match c {
                '&' => {
                    self.pos += 1;
                    Self::push_cell(&mut rows, &mut cell);
                }
                '\\' if self.src.get(self.pos + 1) == Some(&'\\') => {
                    self.pos += 2;
                    Self::push_cell(&mut rows, &mut cell);
                    // 行末的 `\\` 会开出一行新行，后面若直接是 `\end`，
                    // 那一行会是空的——末尾统一用 `retain` 丢掉。
                    rows.push(Vec::new());
                }
                '\\' => cell.push(self.parse_command()?),
                '^' | '_' => self.apply_script(&mut cell, c)?,
                '{' => {
                    self.pos += 1;
                    let inner = self.parse_row(None)?;
                    self.expect('}')?;
                    cell.push(Node::Row(inner));
                }
                _ => {
                    self.pos += 1;
                    cell.push(Node::Text {
                        text: c.to_string(),
                        font: default_font_for(c),
                    });
                }
            }
        }
        if !cell.is_empty() {
            Self::push_cell(&mut rows, &mut cell);
        }
        // 丢掉行末多余的空行（`a\\b\\\end{...}` 会在 `\end` 前留一行空的）。
        // 至少保留一行，否则下方 `rows_h` / `col_w` 的推算会得到空网格。
        while rows.len() > 1 && rows.last().is_some_and(Vec::is_empty) {
            rows.pop();
        }
        Ok(Node::Matrix {
            open: open.to_string(),
            close: close.to_string(),
            rows,
            left_align_cols: matches!(env.as_str(), "cases" | "aligned" | "align"),
        })
    }

    /// `^` / `_`：把**紧邻的上一项**改成带上下标的节点。
    ///
    /// 连续写 `_` 与 `^` 时（`x_1^2`）第二次要**补到同一个 `Script` 上**，
    /// 而不是把「已带下标的 x」整体再套一层上标——后者会让 `1` 与 `2`
    /// 一高一低错开（嵌套的基线不同）。
    fn apply_script(&mut self, out: &mut Vec<Node>, mark: char) -> Result<(), MathError> {
        let is_sup = mark == '^';
        self.bump();
        let arg = self.read_group()?;
        // 若最近的一项是「只带了下标、还没有上标」的 Script，就地补全。
        //
        // `x_1^2` 的两次角标要落在**同一个** `Script` 上。若第二次包一层新
        // `Script`，两层基线不同，`1` 与 `2` 会一高一低错开（实测可见）。
        if let Some(Node::Script { sup, sub, .. }) = out.last_mut() {
            if is_sup && sup.is_none() {
                *sup = Some(arg);
                return Ok(());
            }
            if !is_sup && sub.is_none() {
                *sub = Some(arg);
                return Ok(());
            }
        }
        let base = out
            .pop()
            .ok_or_else(|| MathError::Malformed(format!("`{mark}` 前面没有可加角标的对象")))?;
        // 两种情形分开构造：`then_some` 会**无条件**把 `arg` 移进去，
        // 一个参数只能给一个字段（E0382）。
        let (sup, sub) = if is_sup {
            (Some(arg), None)
        } else {
            (None, Some(arg))
        };
        out.push(Node::Script {
            base: Box::new(base),
            sup,
            sub,
            stacked: false,
        });
        Ok(())
    }
}
