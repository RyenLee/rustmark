//! GBK（CP936）双字节解码与内嵌映射表；
//! 遇到 `0x30..=0x39` 尾字节时转交 GB18030 四字节解码。

use super::gb18030::gb18030_pointer_to_cp;

/// 解码 GBK（CP936），含 GB18030 四字节扩展。
///
/// 逐单元推进，遇到未定义或非法序列时压入一个替换字符继续——
/// 绝不停下或 panic：**部分可读远好于全篇打不开**。
pub(super) fn decode_gbk(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let lead = bytes[i];

        // ASCII 区与 GBK 完全兼容。
        if lead < 0x80 {
            out.push(char::from(lead));
            i += 1;
            continue;
        }
        // CP936 把单字节 0x80 定义为欧元符号。
        if lead == 0x80 {
            out.push('€');
            i += 1;
            continue;
        }

        // 只有 `0x81..=0xFE` 才是合法的双字节首字节（`0xFF` 永远非法）。
        // 这个判断必须放在"取尾字节"**之前**：否则 0xFF 会吞掉紧随其后的
        // 一个正常 ASCII 字符，静默吃掉用户内容中的一个字符。
        if (0x81..=0xFE).contains(&lead)
            && let Some(&trail) = bytes.get(i + 1)
        {
            // 第二字节落在 0x30..=0x39：这不是合法的 GBK 双字节尾字节
            // （GBK 尾字节从 0x40 起），而是 **GB18030 四字节序列**的标志。
            // GBK 与 GB18030 正是靠这一位互斥兼容的——不必先猜编码家族。
            if (0x30..=0x39).contains(&trail) {
                if decode_gb18030_four_byte(&bytes[i..], &mut out) {
                    // 命中（含"已赋值指针为空"的显式替换）：整个四字节序列已消费。
                    i += 4;
                } else {
                    // 形状不完整 / 非法：只作废首字节，不吞后面的内容。
                    out.push(char::REPLACEMENT_CHARACTER);
                    i += 1;
                }
                continue;
            }
            if let Some(ch) = gbk_lookup(lead, trail) {
                out.push(ch);
                i += 2;
            } else if is_gbk_trail(trail) {
                // 合法尾字节、但该组合在 GBK 中未定义：整对作废。
                out.push(char::REPLACEMENT_CHARACTER);
                i += 2;
            } else {
                // 尾字节本身非法：只作废首字节，**不吞掉**下一个字节——
                // 它很可能是一个正常的 ASCII 字符。
                out.push(char::REPLACEMENT_CHARACTER);
                i += 1;
            }
            continue;
        }

        // 非法首字节（如 0xFF），或末尾落单的首字节。
        out.push(char::REPLACEMENT_CHARACTER);
        i += 1;
    }
    out
}

/// 解码一个 GB18030 四字节序列（`bytes` 从首字节开始）。
///
/// 命中且已映射时把字符压入 `out` 并返回 `true`（调用方推进 4 字节）；
/// 形状不完整（缺第 3/4 字节）或落在排除区时**不写 `out`**、返回 `false`
/// （调用方只作废首字节——四字节序列里混着的 ASCII 数字不得被吞掉）。
pub(super) fn decode_gb18030_four_byte(bytes: &[u8], out: &mut String) -> bool {
    let Some(&b2) = bytes.get(1) else {
        return false;
    };
    let (Some(&b3), Some(&b4)) = (bytes.get(2), bytes.get(3)) else {
        // 序列在文件末尾被截断：交给调用方按单字节作废。
        return false;
    };
    // 四字节形状：[0x81-0xFE, 0x30-0x39, 0x81-0xFE, 0x30-0x39]。
    if !(0x30..=0x39).contains(&b2) || !(0x81..=0xFE).contains(&b3) || !(0x30..=0x39).contains(&b4)
    {
        return false;
    }
    // 四字节线性指针：`首*12600 + 二*1260 + 三*10 + 四`（各字节先转段内序号）。
    let pointer = u32::from(bytes[0] - 0x81) * 12_600
        + u32::from(lead_index(b2)) * 1_260
        + u32::from(lead_index(b3)) * 10
        + u32::from(b4 - 0x30);
    match gb18030_pointer_to_cp(pointer) {
        Some(ch) => {
            out.push(ch);
            true
        }
        // 线性段内的未赋值指针（如代理区）：显式替换，而不是回退猜双字节。
        None => {
            out.push(char::REPLACEMENT_CHARACTER);
            true
        }
    }
}

/// `0x30..=0x39` 与 `0x81..=0xFE` 的内部线性序号（构造四字节指针用）。
pub(super) const fn lead_index(b: u8) -> u8 {
    if b >= 0x30 && b <= 0x39 {
        b - 0x30
    } else {
        b - 0x81
    }
}

/// 尾字节是否落在 GBK 合法范围内（`0x40..=0x7E` 与 `0x80..=0xFE`）。
pub(super) const fn is_gbk_trail(b: u8) -> bool {
    matches!(b, 0x40..=0x7E | 0x80..=0xFE)
}

/// 内嵌的 GBK → Unicode 映射表。
///
/// 布局：`126` 行（首字节 `0x81..=0xFE`）× `190` 列
/// （尾字节 `0x40..=0x7E` 共 63 列，接 `0x80..=0xFE` 共 127 列），
/// 每格 2 字节小端 `u16`，`0` 表示未定义。
pub(super) static GBK_TABLE: &[u8] = include_bytes!("../gbk_table.bin");

/// 首字节的取值个数（`0x81..=0xFE`）。
pub(super) const GBK_ROWS: usize = 126;
/// 每个首字节下的尾字节个数（63 + 127）。
pub(super) const GBK_COLS: usize = 190;
/// 尾字节第一段的列数（`0x40..=0x7E`）。
pub(super) const GBK_TRAIL_LOW_COLS: usize = 63;

/// 查表取 Unicode 码位；未定义或越过表尾返回 `None`。
pub(super) fn gbk_lookup(lead: u8, trail: u8) -> Option<char> {
    if !(0x81..=0xFE).contains(&lead) {
        return None;
    }
    let col = match trail {
        0x40..=0x7E => (trail - 0x40) as usize,
        0x80..=0xFE => (trail - 0x80) as usize + GBK_TRAIL_LOW_COLS,
        _ => return None,
    };
    let row = (lead - 0x81) as usize;
    let at = (row * GBK_COLS + col) * 2;
    debug_assert!(row < GBK_ROWS, "行号越界说明首字节范围判断有误");
    let code = u16::from_le_bytes([GBK_TABLE[at], GBK_TABLE[at + 1]]);
    if code == 0 {
        None
    } else {
        char::from_u32(u32::from(code))
    }
}
