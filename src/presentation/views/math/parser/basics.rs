//! [`Parser`] 的基础工具方法：peek / bump / skip / expect / 命令名识别。

use super::*;

impl Parser {
    /// 剩余内容的前若干字符（错误信息用）。
    pub(super) fn rest_preview(&self) -> String {
        self.src[self.pos..].iter().take(16).collect()
    }

    pub(super) fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    pub(super) fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    pub(super) fn skip_spaces(&mut self) {
        while matches!(
            self.peek(),
            Some(' ') | Some('\t') | Some('\n') | Some('\r')
        ) {
            self.pos += 1;
        }
    }

    /// 当前位置是否以 `\name` 开头（`\` + 名字，且名字后不再是字母）。
    pub(super) fn command_is(&self, name: &str) -> bool {
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

    pub(super) fn expect(&mut self, c: char) -> Result<(), MathError> {
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
    pub(super) fn read_command_name(&mut self) -> Result<String, MathError> {
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
}
