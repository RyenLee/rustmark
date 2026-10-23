//! 编码回写：保存时按原编码写回（含 BOM 语义）。
//!
//! GBK 双字节表覆盖不了的字符自动落 GB18030 四字节，UTF-16 按字节序补 BOM。

use super::gb18030::{gb18030_cp_to_pointer, gb18030_pointer_to_bytes};
use super::gbk::{GBK_COLS, GBK_ROWS, GBK_TABLE, GBK_TRAIL_LOW_COLS};
use super::{Encoding, UTF8_BOM};

/// GBK 双字节反查表（Unicode → `(首, 尾)`），首次使用时构建。
///
/// 由内嵌的 [`GBK_TABLE`] 正表翻转而来（约 2.2 万条），构建一次、常驻；
/// 相比"每字符线性扫正表"，长文档保存从 O(N×2.2万) 降到 O(N)。
pub(super) fn gbk_reverse() -> &'static std::collections::HashMap<u32, (u8, u8)> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static REVERSE: OnceLock<HashMap<u32, (u8, u8)>> = OnceLock::new();
    REVERSE.get_or_init(|| {
        let mut map = HashMap::with_capacity(GBK_ROWS * GBK_COLS);
        for row in 0..GBK_ROWS {
            for col in 0..GBK_COLS {
                let at = (row * GBK_COLS + col) * 2;
                let code = u16::from_le_bytes([GBK_TABLE[at], GBK_TABLE[at + 1]]);
                if code != 0
                    && let Some(ch) = char::from_u32(u32::from(code))
                {
                    let lead = 0x81 + row as u8;
                    let trail = if col < GBK_TRAIL_LOW_COLS {
                        0x40 + col as u8
                    } else {
                        0x80 + (col - GBK_TRAIL_LOW_COLS) as u8
                    };
                    map.insert(ch as u32, (lead, trail));
                }
            }
        }
        map
    })
}

/// 把文本按 `encoding` 编码为字节（含 BOM 语义）。
///
/// GBK 目标下，双字节表覆盖不了的字符自动落 **GB18030 四字节**
/// （GB18030 是 GBK 的超集，四字节空间线性覆盖全部 Unicode 码位），
/// 因此对任何合法 `&str` 都能无损编码。UTF-16 目标按对应字节序写入并补 BOM。
pub(crate) fn encode(text: &str, encoding: Encoding) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 4);
    match encoding {
        Encoding::Utf8 => out.extend_from_slice(text.as_bytes()),
        Encoding::Utf8Bom => {
            out.extend_from_slice(&UTF8_BOM);
            out.extend_from_slice(text.as_bytes());
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let little = encoding == Encoding::Utf16Le;
            out.extend_from_slice(if little { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
            for unit in text.encode_utf16() {
                if little {
                    out.extend_from_slice(&unit.to_le_bytes());
                } else {
                    out.extend_from_slice(&unit.to_be_bytes());
                }
            }
        }
        Encoding::Gbk => {
            for ch in text.chars() {
                let cp = ch as u32;
                if cp < 0x80 {
                    out.push(cp as u8);
                } else if ch == '€' {
                    out.push(0x80);
                } else if let Some((lead, trail)) = gbk_reverse().get(&cp) {
                    out.push(*lead);
                    out.push(*trail);
                } else if let Some(pointer) = gb18030_cp_to_pointer(cp) {
                    out.extend_from_slice(&gb18030_pointer_to_bytes(pointer));
                } else {
                    // 理论不可达（四字节空间覆盖全部码位）；防御性替换而非 panic。
                    out.extend_from_slice("\u{FFFD}".as_bytes());
                }
            }
        }
    }
    out
}
