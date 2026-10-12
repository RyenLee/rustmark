//! [`Parser`] 的矩阵 / 环境解析：`\begin{...} … \end{...}`。

use super::*;

impl Parser {
    /// 把当前格 `cell` 推进网格的**末行**，并清空 `cell`（`mem::take`）。
    ///
    /// 抽成函数是为了避开 `expect`——`src/lib.rs` 把 `clippy::expect_used`
    /// 升成了硬错误。这里的不变式其实成立（`rows` 由 `vec![Vec::new()]` 起家，
    /// 之后每次换行都是「先写末行、再 `push` 新行」的成对操作，末行恒在），
    /// 但仍用 `if let` 直取：万一不变式被破坏，退化成「丢掉当前格」，
    /// 而不是整个进程终止（`panic = "abort"` 下 panic 就是直接退出）。
    pub(super) fn push_cell(rows: &mut [Vec<Vec<Node>>], cell: &mut Vec<Node>) {
        if let Some(row) = rows.last_mut() {
            row.push(std::mem::take(cell));
        }
    }

    /// `\begin{...} … \end{...}`：矩阵 / cases / aligned。
    pub(super) fn parse_environment(&mut self) -> Result<Node, MathError> {
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
}
