//! 坐标系换算（字符索引 ↔ 字节偏移）与插入点邻近文本工具。

/// 把字符索引转换为字节偏移。
///
/// `char_idx` 超过文本长度时返回 `text.len()`（夹到末尾，不越界）。
pub fn char_to_byte(text: &str, char_idx: usize) -> usize {
    // 第 `char_idx` 个字符的起始字节；越界则夹到文末。
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(byte, _)| byte)
}

/// 把字节偏移转换为字符索引（夹到 `[0, 字符总数]`）。
pub fn byte_to_char(text: &str, byte_idx: usize) -> usize {
    if byte_idx == 0 {
        return 0;
    }
    let clamped = byte_idx.min(text.len());
    text[..clamped].chars().count()
}

/// 括号配对：若用户刚输入一个开括号，返回应自动补出的闭括号字符。
pub fn matching_close(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '<' => Some('>'),
        _ => None,
    }
}

/// 返回 `byte` 之前的完整行文本（即上一个 `\n` 之后到 `byte` 为止）。
///
/// 用于「回车续列表」时判断前一行是否为列表项。
pub fn line_before(text: &str, byte: usize) -> &str {
    let clamped = byte.min(text.len());
    let start = text[..clamped].rfind('\n').map(|i| i + 1).unwrap_or(0);
    &text[start..clamped]
}
