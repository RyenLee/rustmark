//! UTF-16 两种字节序的解码（BOM 已由父模块剥离）。

/// 解码 UTF-16（不含 BOM）。奇数尾字节按替换字符处理，不 panic。
pub(super) fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    // `as_chunks` 同时给出"完整的码元"与"落单的尾字节"，
    // 不必再手工算余数。
    let (pairs, remainder) = bytes.as_chunks::<2>();
    let mut units: Vec<u16> = pairs
        .iter()
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes(*pair)
            } else {
                u16::from_be_bytes(*pair)
            }
        })
        .collect();
    // 截断的半个码元：补一个替换字符，让用户看出文件尾部不完整。
    if !remainder.is_empty() {
        units.push(char::REPLACEMENT_CHARACTER as u16);
    }
    String::from_utf16_lossy(&units)
}
