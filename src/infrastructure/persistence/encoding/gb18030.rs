//! GB18030 四字节：线性映射（WHATWG `index-gb18030-ranges`，207 个仿射段）。
//!
//! 提供指针 ↔ Unicode 码位的双向查表，供 GBK 解码与编码回写共用。

/// GB18030 四字节线性段表。布局见模块文档；由 `tools/gen_gb18030_ranges.py` 生成。
static GB18030_RANGES: &[u8] = include_bytes!("../gb18030_ranges.bin");

/// 读取第 `index` 个线性段 `(起始指针, 起始码位, 码位跨度)`。
const fn gb18030_run(index: usize) -> (u32, u32, u32) {
    let at = index * 12;
    (
        u32::from_le_bytes([
            GB18030_RANGES[at],
            GB18030_RANGES[at + 1],
            GB18030_RANGES[at + 2],
            GB18030_RANGES[at + 3],
        ]),
        u32::from_le_bytes([
            GB18030_RANGES[at + 4],
            GB18030_RANGES[at + 5],
            GB18030_RANGES[at + 6],
            GB18030_RANGES[at + 7],
        ]),
        u32::from_le_bytes([
            GB18030_RANGES[at + 8],
            GB18030_RANGES[at + 9],
            GB18030_RANGES[at + 10],
            GB18030_RANGES[at + 11],
        ]),
    )
}

/// 线性段条数（由表长推导，避免双处维护常量）。
const fn gb18030_run_count() -> usize {
    GB18030_RANGES.len() / 12
}

/// 四字节指针（线性序号）→ Unicode 码位。
///
/// 算法与 WHATWG Encoding Standard `index-gb18030-ranges code point for pointer`
/// 一致：取「起始指针 ≤ pointer」的最后一个线性段，`cp = 起始码位 + 差值`；
/// 差值越过该段的码位跨度（如两段之间的排除区）则视为未赋值。
/// 返回 `None` 的情况：未赋值指针、或映射到代理区（`char::from_u32` 拒绝）。
pub(super) fn gb18030_pointer_to_cp(pointer: u32) -> Option<char> {
    let count = gb18030_run_count();
    // 二分找最后一个 `起始指针 <= pointer` 的段。
    let mut low = 0usize;
    let mut high = count;
    let mut found = None;
    while low < high {
        let mid = low + (high - low) / 2;
        if gb18030_run(mid).0 <= pointer {
            found = Some(mid);
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    let (start_ptr, start_cp, span) = gb18030_run(found?);
    let delta = pointer.checked_sub(start_ptr)?;
    if delta >= span {
        return None; // 落在排除区（如指针 39420..188999 的大空洞）。
    }
    char::from_u32(start_cp + delta)
}

/// Unicode 码位 → 四字节指针（[`gb18030_pointer_to_cp`] 的逆映射）。
///
/// 用**同一张表**反查：找最后一个 `起始码位 ≤ cp` 的段，其可行域为
/// 「码位跨度与指针跨度的较小者」（两段交界处被排除的码位——如 U+00A4——
/// 已由双字节 GBK 覆盖，四字节不再编码）。代理区不在任何段的可写域内。
pub(super) fn gb18030_cp_to_pointer(cp: u32) -> Option<u32> {
    let count = gb18030_run_count();
    let mut low = 0usize;
    let mut high = count;
    let mut found = None;
    while low < high {
        let mid = low + (high - low) / 2;
        if gb18030_run(mid).1 <= cp {
            found = Some(mid);
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    let index = found?;
    let (start_ptr, start_cp, cp_span) = gb18030_run(index);
    let delta = cp.checked_sub(start_cp)?;
    // 可行域 = min(码位跨度, 指针跨度)。指针跨度取下一段的起始指针差
    // （末段的指针跨度等于码位跨度）。
    let ptr_span = if index + 1 < count {
        gb18030_run(index + 1).0 - start_ptr
    } else {
        cp_span
    };
    if delta >= cp_span.min(ptr_span) {
        return None;
    }
    Some(start_ptr + delta)
}

/// 四字节指针 → 4 字节序列。
pub(super) const fn gb18030_pointer_to_bytes(pointer: u32) -> [u8; 4] {
    let b1 = 0x81 + (pointer / 12_600) as u8;
    let rest = pointer % 12_600;
    let b2 = 0x30 + (rest / 1_260) as u8;
    let rest = rest % 1_260;
    let b3 = 0x81 + (rest / 10) as u8;
    let b4 = 0x30 + (rest % 10) as u8;
    [b1, b2, b3, b4]
}
