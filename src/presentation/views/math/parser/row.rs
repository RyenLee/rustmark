//! [`Parser`] 的行解析方法：`parse_row` / `parse_until` / `apply_script`。

use super::*;

impl Parser {
    /// 解析一串节点，直到输入结束或遇到 `stop` 指定的**结束控制序列**。
    ///
    /// `stop` 为 `Some("right")` 时，`\right` 不被消费——留给 [`Self::parse_delim`]
    /// 读取它后面的定界符。返回时 `self.pos` 指向 `\right` 的 `\`。
    pub(super) fn parse_row(&mut self, stop: Option<&str>) -> Result<Vec<Node>, MathError> {
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

    /// 解析到指定右界（`[n]` 里的 `]`），不消费右界。
    pub(super) fn parse_until(&mut self, end: char) -> Result<Vec<Node>, MathError> {
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

    /// `^` / `_`：把**紧邻的上一项**改成带上下标的节点。
    ///
    /// 连续写 `_` 与 `^` 时（`x_1^2`）第二次要**补到同一个 `Script` 上**，
    /// 而不是把「已带下标的 x」整体再套一层上标——后者会让 `1` 与 `2`
    /// 一高一低错开（嵌套的基线不同）。
    pub(super) fn apply_script(
        &mut self,
        out: &mut Vec<Node>,
        mark: char,
    ) -> Result<(), MathError> {
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
