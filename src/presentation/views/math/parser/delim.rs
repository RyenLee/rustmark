//! [`Parser`] 的定界符解析：`\left` / `\right` / `\bigl` 等。

use super::*;

impl Parser {
    /// `\left( … \right)`：读到 `\right` 为止，两侧定界符一并记下。
    pub(super) fn parse_delim(&mut self) -> Result<Node, MathError> {
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
    pub(super) fn read_delim_char(&mut self) -> Result<String, MathError> {
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
    pub(super) fn parse_single_delim(&mut self) -> Result<Node, MathError> {
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
}
