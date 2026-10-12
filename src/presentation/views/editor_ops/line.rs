//! 行级变换：整行上下移动与 Markdown 列表标记识别。

/// 把光标所在行整体上移/下移一行（`dir = -1` 上移，`+1` 下移）。
///
/// 仅对单光标有意义；有选区时退化为对光标所在行操作。
/// 返回 `None` 表示已在边界（首行上移 / 末行下移），无需改动。
pub fn move_line(text: &str, cursor: usize, dir: i8) -> Option<(String, usize)> {
    if dir == 0 {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return None;
    }
    // 找到光标所在行的 [line_start, line_end] 与相邻行的范围。
    let n = chars.len();
    let mut line_start = cursor;
    while line_start > 0 && chars[line_start - 1] != '\n' {
        line_start -= 1;
    }
    let mut line_end = cursor;
    while line_end < n && chars[line_end] != '\n' {
        line_end += 1;
    }
    let has_trailing_newline = line_end < n; // line_end 指向 '\n'
    let cur_start = line_start;
    let cur_line: String = chars[cur_start..line_end].iter().collect();

    let (other_start, other_end) = if dir < 0 {
        if line_start == 0 {
            return None;
        }
        // 上一行
        let mut s = line_start;
        while s > 0 && chars[s - 1] != '\n' {
            s -= 1;
        }
        (s, line_start) // 上一段 [s, line_start) 不含其后的 '\n'
    } else {
        if !has_trailing_newline {
            return None;
        }
        let mut e = line_end + 1;
        while e < n && chars[e] != '\n' {
            e += 1;
        }
        (line_end + 1, e) // 下一行 [line_end+1, e)
    };
    let other_line: String = chars[other_start..other_end].iter().collect();

    // 组装：把两行交换（保持换行符结构不变）。
    let mut out = String::with_capacity(text.len());
    if dir < 0 {
        // 上移：当前行与上一行交换，当前行移动到「上方」。
        out.push_str(&chars[..other_start].iter().collect::<String>());
        out.push_str(&cur_line);
        out.push('\n');
        out.push_str(&other_line);
        if line_end < n {
            out.push('\n');
            out.push_str(&chars[(line_end + 1).min(n)..].iter().collect::<String>());
        }
    } else {
        // 下移：当前行与下一行交换，当前行移动到「下方」。
        out.push_str(&chars[..line_start].iter().collect::<String>());
        out.push_str(&other_line);
        out.push('\n');
        out.push_str(&cur_line);
        if other_end < n {
            out.push('\n');
            out.push_str(&chars[(other_end + 1).min(n)..].iter().collect::<String>());
        }
    }

    // 新光标：保持「当前行」内容上同偏移；交换后当前行起点为 `other_start`。
    let new_cursor = other_start + (cursor - cur_start);
    Some((out, new_cursor))
}

/// 若 `line` 以 Markdown 列表标记开头，返回 `(缩进字节长度, 标记文本)`。
///
/// 支持无序列表 `-` / `*` / `+`（后跟空格）与有序列表 `1.` / `2)` 等形式。
/// 用于「回车续列表」：在前一行是列表项时自动补出下一行标记。
pub fn list_marker(line: &str) -> Option<(usize, String)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= line.len() {
        return None;
    }
    let rest = &line[i..];
    let marker = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "));
    if marker.is_some() {
        return Some((i, line[..i + 2].to_string()));
    }
    // 有序列表：数字 + '.' 或 ')' + 空格
    //
    // 数字段必须**整段**剥出来：只剥一位的话，`10. item` 会变成
    // `0. item`，后续 `. ` 前缀匹配不上，回车就续不出 `11. `——
    // 症状是「列表写到第十条以后，回车不再补序号」。
    let digits_end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let after_num = rest[digits_end..]
        .strip_prefix(". ")
        .map(|r| (r, "."))
        .or_else(|| rest[digits_end..].strip_prefix(") ").map(|r| (r, ")")));
    if digits_end > 0
        && let Some((_, sep)) = after_num
    {
        // 分隔符位置：`after_num` 是由 `rest` 剥离得到的，故必然能找到；
        // 用 `?` 处理而非 `unwrap()`——找不到即放弃本次续行（V11-12）。
        let idx = rest.find(sep)?;
        let mlen = i + idx + 2; // 缩进 + 数字 + 分隔 + 空格
        // 递增序号
        let num: String = rest[..idx].chars().collect();
        if let Ok(n) = num.parse::<u32>() {
            return Some((mlen, format!("{}{} ", n + 1, sep)));
        }
    }
    None
}
