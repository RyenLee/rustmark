//! 零依赖编码原语：最小 base64 解码、百分号解码。
//!
//! （HTTP 切体原语 `split_http_body` 已随手写 `TcpStream` 下载一起退役：
//! 远程下载改走 `ureq`，响应体由其直接给出，见 `asset_loader::load_remote`。）

/// 最小 base64 解码器。
///
/// 不引依赖的理由：base64 逻辑只有约 40 行，而 `base64` crate 会
/// 带来额外体积；更重要的是解码失败必须能被**测试覆盖**，
/// 自己实现才能对畸形输入的每条分支加断言。
pub(super) fn decode_base64(input: &str) -> Option<Vec<u8>> {
    fn value_of(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a') as u32 + 26,
            b'0'..=b'9' => (c - b'0') as u32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }

    let filtered: Vec<u8> = input
        .bytes()
        // base64 里换行与空白是合法的（原型正则也允许 `\s`）。
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if !filtered.len().is_multiple_of(4) {
        return None;
    }

    let mut out = Vec::with_capacity(filtered.len() / 4 * 3);
    for chunk in filtered.chunks(4) {
        let mut buffer = 0u32;
        let mut padding = 0usize;
        for (i, byte) in chunk.iter().enumerate() {
            if *byte == b'=' {
                padding += 1;
                // base64 每4 字节一组最多补2 个 `=`；`====` 之类的输入非法。
                if padding > 2 {
                    return None;
                }
                continue;
            }
            // 填充符只能出现在末尾。
            if padding > 0 {
                return None;
            }
            buffer |= value_of(*byte)? << (18 - 6 * i);
        }
        out.push((buffer >> 16) as u8);
        if padding < 2 {
            out.push((buffer >> 8) as u8);
        }
        if padding < 1 {
            out.push(buffer as u8);
        }
    }
    Some(out)
}

/// 最小百分号解码器。
pub(super) fn percent_decode(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    Some(out)
}
