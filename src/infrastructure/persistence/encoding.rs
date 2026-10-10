//! 文本编码探测与解码。
//!
//! # 为什么需要它
//!
//! 编辑器必须打开**任意**文件。此前 `fs_document_repository` 直接用
//! `String::from_utf8_lossy` 兜底，导致 GBK 编码的文件（Windows 上中文文本的
//! 常见历史编码）**每个字节**被替换成 U+FFFD——用户看到满屏 `�`，
//! 且一旦保存，替换字符被写回磁盘，**内容不可逆丢失**。
//!
//! # 探测顺序（由强到弱）
//!
//! 1. **UTF-8 BOM** —— 文件自己声明了编码，最可靠，直接采信。
//! 2. **UTF-16 BOM**（`FF FE` / `FE FF`）—— Windows 记事本「另存为 Unicode」的产物。
//! 3. **裸 UTF-8** —— 现代文件的绝大多数。用严格校验判定，合法即采用。
//! 4. **GBK 兜底** —— 前三条都不成立时才走。映射表内嵌（见下）。
//!
//! # 已知局限（诚实声明）
//!
//! 无 BOM 且不是合法 UTF-8 的文件，**只能"猜"是 GBK**。理论上存在
//! 一段字节同时是合法 UTF-8 又是另一编码的情形，此时会按 UTF-8 解释（第 3 步优先）。
//! 这与浏览器的编码探测行为一致，属此类方案的固有取舍——
//! 想完全消除歧义只能靠 BOM 或让用户手动指定编码。
//!
//! 另外本模块只处理 **GBK/CP936 兼容家族**：GBK 双字节 +
//! **GB18030 四字节扩展**（经 WHATWG `index-gb18030-ranges` 线性映射，
//! 见 [`gb18030_pointer_to_cp`]）。Big5、Shift_JIS 等与 GBK 无兼容关系、
//! 且无 BOM 可辨，自动探测存在原理性歧义，仍不支持。
//!
//! # 映射表的由来
//!
//! `gbk_table.bin` 由脚本生成（126 行 × 190 列，每格小端 u16，0 表示未定义），
//! 覆盖 GBK 全部双字节码位中已定义的 21791 个。内嵌而非引入 crate，
//! 是为守住项目的「零新增依赖」约束（体积代价约 47 KB）。
//!
//! `gb18030_ranges.bin` 由 `tools/gen_gb18030_ranges.py` 从 WHATWG 规范表生成
//! （207 个仿射段，每段 12 字节：起始指针 / 起始码位 / 码位跨度，各 u32 小端），
//! 已与 WHATWG 合规实现抽样 5 万例交叉验证一致。

/// 识别出的文本编码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoding {
    /// 无 BOM 的 UTF-8。
    Utf8,
    /// 带 BOM 的 UTF-8。
    Utf8Bom,
    /// UTF-16 小端（`FF FE`）。
    Utf16Le,
    /// UTF-16 大端（`FE FF`）。
    Utf16Be,
    /// GBK / CP936 双字节（含 GB18030 四字节扩展的解码与回写）。
    Gbk,
}

impl Encoding {
    /// 状态栏等处展示用的名称。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Gbk => "GBK",
        }
    }

    /// 保存时是否应写回 BOM。
    ///
    /// 只有原本带 BOM 的 UTF-8 才补回——凭空加 BOM 会干扰
    /// 依赖文本首字节的工具（如 shell 脚本的 shebang）。
    pub const fn keeps_bom(self) -> bool {
        matches!(self, Self::Utf8Bom)
    }
}

impl From<Encoding> for crate::shared::encoding::TextEncoding {
    /// 探测结果 → 共享内核的纯标签（V11-03 / DEF-05）。
    ///
    /// 映射放在**基础设施层**而非领域层：领域层不得引用带解码实现的
    /// `Encoding`，只认 `shared` 里的标签值（纯数据、零 IO）。
    fn from(value: Encoding) -> Self {
        match value {
            Encoding::Utf8 => Self::Utf8,
            Encoding::Utf8Bom => Self::Utf8Bom,
            Encoding::Utf16Le => Self::Utf16Le,
            Encoding::Utf16Be => Self::Utf16Be,
            Encoding::Gbk => Self::Gbk,
        }
    }
}

/// UTF-8 BOM。
pub const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// 解码字节流，并报告识别出的编码。
pub fn decode(bytes: &[u8]) -> (String, Encoding) {
    // 1) UTF-8 BOM：文件自带声明，直接采信。
    if let Some(rest) = bytes.strip_prefix(&UTF8_BOM) {
        // 带 BOM 却含非法字节 = 文件已损坏，此时 lossy 是合理兜底。
        return (
            String::from_utf8_lossy(rest).into_owned(),
            Encoding::Utf8Bom,
        );
    }
    // 2) UTF-16 BOM：两种字节序都要认。
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return (decode_utf16(rest, true), Encoding::Utf16Le);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return (decode_utf16(rest, false), Encoding::Utf16Be);
    }
    // 3) 裸 UTF-8 快速路径：合法即采用，绝大多数现代文件走这条。
    if let Ok(text) = std::str::from_utf8(bytes) {
        return (text.to_owned(), Encoding::Utf8);
    }
    // 4) 非法 UTF-8：先区分"**损坏的 UTF-8**"与"**真正的 GBK**"。
    //
    //    这一步不能省。若直接整体转 GBK，一份"绝大部分是合法 UTF-8、
    //    只有一两个坏字节"的文件会被**二次解码**：原本正确的中文
    //    （三字节 UTF-8 序列）被当成 GBK 双字节逐段重解，
    //    结果是满篇看似正常却全错的汉字——**比原来的替换字符更危险**，
    //    因为用户看不出问题，一保存就彻底毁掉原文。
    //
    //    判据见 `utf8_multibyte_counts`：真中文 UTF-8 的三字节序列远多于双字节，
    //    而 GBK 字节偶然凑出的合法 UTF-8 序列几乎只落在双字节区。
    let (two_byte, three_byte_plus) = utf8_multibyte_counts(bytes);
    if three_byte_plus > two_byte {
        return (String::from_utf8_lossy(bytes).into_owned(), Encoding::Utf8);
    }
    // 5) 兜底：按 GBK 解释。
    (decode_gbk(bytes), Encoding::Gbk)
}

/// 统计字节流中**合法的** UTF-8 序列条数，按长度分两组：
/// `(双字节条数, 三字节及以上条数)`。ASCII 不计入。
///
/// # 为什么这样就够区分 GBK 与 UTF-8
///
/// 关键在于两种编码"偶然合法"的概率落在**不同的长度档**上：
///
/// - **真中文 UTF-8**：汉字是 `E4–E9` 开头的**三字节**序列，
///   所以三字节档必然占绝对多数（三字节 ≫ 双字节）。
/// - **GBK 汉字**：本身是双字节，其首字节落在 `C2–DF`（UTF-8 双字节首）
///   且尾字节落在 `80–BF`（合法的续字节）时，就"碰巧"构成合法 UTF-8 双字节序列。
///   这个概率不低（粗算约四分之一），但**凑出合法三字节序列要连中两个续字节，
///   概率低得多**——所以 GBK 文本的双字节档总是多于三字节档。
///
/// 早先试图用"合法字节覆盖率"做判据，**实测不可行**：
/// GBK 中英混排文本的覆盖率能到 0.75，而损坏的 UTF-8 是 0.83，
/// 两簇贴得太近，没有可靠阈值。按长度分档则稳定得多。
fn utf8_multibyte_counts(bytes: &[u8]) -> (usize, usize) {
    let mut two = 0usize;
    let mut three_plus = 0usize;
    let mut rest = bytes;

    while !rest.is_empty() {
        let (good, error) = match std::str::from_utf8(rest) {
            Ok(text) => (text, None),
            Err(e) => {
                let valid = e.valid_up_to();
                // `valid_up_to` 的语义保证这段前缀合法；这里仍走 Result 分支而非
                // `expect`：万一被拒就按空前缀继续——循环由 `error_len` 推进，
                // 不会死循环，也不会因一次解码异常终止进程（规范 §四-15 / V11-12）。
                let prefix = std::str::from_utf8(&rest[..valid]).unwrap_or("");
                (prefix, Some(e))
            }
        };
        for ch in good.chars() {
            match ch.len_utf8() {
                2 => two += 1,
                3 | 4 => three_plus += 1,
                _ => {}
            }
        }
        match error {
            Some(e) => match e.error_len() {
                Some(skip) => {
                    let next = e.valid_up_to() + skip;
                    if next >= rest.len() {
                        break;
                    }
                    rest = &rest[next..];
                }
                // `None`：字节流在一个序列中途被截断，之后不可能再有合法序列。
                None => break,
            },
            None => break,
        }
    }
    (two, three_plus)
}

/// 仅解码、不关心编码种类时的便捷入口。
pub fn decode_to_string(bytes: &[u8]) -> String {
    decode(bytes).0
}

/// 解码 UTF-16（不含 BOM）。奇数尾字节按替换字符处理，不 panic。
fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
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

/// 解码 GBK（CP936），含 GB18030 四字节扩展。
///
/// 逐单元推进，遇到未定义或非法序列时压入一个替换字符继续——
/// 绝不停下或 panic：**部分可读远好于全篇打不开**。
fn decode_gbk(bytes: &[u8]) -> String {
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
fn decode_gb18030_four_byte(bytes: &[u8], out: &mut String) -> bool {
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
const fn lead_index(b: u8) -> u8 {
    if b >= 0x30 && b <= 0x39 {
        b - 0x30
    } else {
        b - 0x81
    }
}

/// 尾字节是否落在 GBK 合法范围内（`0x40..=0x7E` 与 `0x80..=0xFE`）。
const fn is_gbk_trail(b: u8) -> bool {
    matches!(b, 0x40..=0x7E | 0x80..=0xFE)
}

/// 内嵌的 GBK → Unicode 映射表。
///
/// 布局：`126` 行（首字节 `0x81..=0xFE`）× `190` 列
/// （尾字节 `0x40..=0x7E` 共 63 列，接 `0x80..=0xFE` 共 127 列），
/// 每格 2 字节小端 `u16`，`0` 表示未定义。
static GBK_TABLE: &[u8] = include_bytes!("gbk_table.bin");

/// 首字节的取值个数（`0x81..=0xFE`）。
const GBK_ROWS: usize = 126;
/// 每个首字节下的尾字节个数（63 + 127）。
const GBK_COLS: usize = 190;
/// 尾字节第一段的列数（`0x40..=0x7E`）。
const GBK_TRAIL_LOW_COLS: usize = 63;

/// 查表取 Unicode 码位；未定义或越过表尾返回 `None`。
fn gbk_lookup(lead: u8, trail: u8) -> Option<char> {
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

// ====================================================================
// GB18030 四字节：线性映射（WHATWG `index-gb18030-ranges`，207 个仿射段）
// ====================================================================

/// GB18030 四字节线性段表。布局见模块文档；由 `tools/gen_gb18030_ranges.py` 生成。
static GB18030_RANGES: &[u8] = include_bytes!("gb18030_ranges.bin");

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
fn gb18030_pointer_to_cp(pointer: u32) -> Option<char> {
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
fn gb18030_cp_to_pointer(cp: u32) -> Option<u32> {
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
const fn gb18030_pointer_to_bytes(pointer: u32) -> [u8; 4] {
    let b1 = 0x81 + (pointer / 12_600) as u8;
    let rest = pointer % 12_600;
    let b2 = 0x30 + (rest / 1_260) as u8;
    let rest = rest % 1_260;
    let b3 = 0x81 + (rest / 10) as u8;
    let b4 = 0x30 + (rest % 10) as u8;
    [b1, b2, b3, b4]
}

// ====================================================================
// 编码回写（保存时按原编码写回）
// ====================================================================

/// GBK 双字节反查表（Unicode → `(首, 尾)`），首次使用时构建。
///
/// 由内嵌的 [`GBK_TABLE`] 正表翻转而来（约 2.2 万条），构建一次、常驻；
/// 相比"每字符线性扫正表"，长文档保存从 O(N×2.2万) 降到 O(N)。
fn gbk_reverse() -> &'static std::collections::HashMap<u32, (u8, u8)> {
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
pub fn encode(text: &str, encoding: Encoding) -> Vec<u8> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 把字符编成 GBK 字节，供测试构造输入。
    ///
    /// ASCII 直接取字节（GBK 与之完全兼容）；其余字符在表里反查 (首, 尾)。
    /// 查不到的字符会被跳过——测试文本应只含 GBK 可表示的字符。
    fn gbk_bytes(text: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for ch in text.chars() {
            if (ch as u32) < 0x80 {
                out.push(ch as u8);
                continue;
            }
            for lead in 0x81u8..=0xFE {
                let mut found = None;
                for trail in (0x40u8..=0x7E).chain(0x80..=0xFE) {
                    if gbk_lookup(lead, trail) == Some(ch) {
                        found = Some(trail);
                        break;
                    }
                }
                if let Some(trail) = found {
                    out.push(lead);
                    out.push(trail);
                    break;
                }
            }
        }
        out
    }

    #[test]
    fn plain_utf8_is_detected_and_decoded() {
        let (text, enc) = decode("中文与 English".as_bytes());
        assert_eq!(text, "中文与 English");
        assert_eq!(enc, Encoding::Utf8);
        assert!(!enc.keeps_bom());
    }

    #[test]
    fn utf8_bom_is_stripped_and_remembered() {
        let mut bytes = UTF8_BOM.to_vec();
        bytes.extend_from_slice("带 BOM 的内容".as_bytes());
        let (text, enc) = decode(&bytes);
        assert_eq!(text, "带 BOM 的内容", "BOM 不应残留在正文里");
        assert_eq!(enc, Encoding::Utf8Bom);
        assert!(enc.keeps_bom(), "原本带 BOM 的文件保存时应补回 BOM");
    }

    /// 核心用例：GBK 文件不再产生替换字符。
    #[test]
    fn gbk_chinese_decodes_without_replacement_chars() {
        let bytes = gbk_bytes("中文测试，编辑器");
        assert!(!bytes.is_empty(), "构造的 GBK 字节不应为空（前提检查）");
        // 前提：这些字节确实不是合法 UTF-8，否则测的就不是 GBK 分支。
        assert!(
            std::str::from_utf8(&bytes).is_err(),
            "构造的字节恰好是合法 UTF-8，无法验证 GBK 分支"
        );

        let (text, enc) = decode(&bytes);
        assert_eq!(enc, Encoding::Gbk);
        assert_eq!(text, "中文测试，编辑器");
        assert!(
            !text.contains('\u{fffd}'),
            "GBK 解码不应产生替换字符，实际得到：{text}"
        );
    }

    /// 中英混排：GBK 的 ASCII 区与 UTF-8 兼容，必须原样保留。
    #[test]
    fn gbk_mixed_with_ascii_keeps_ascii_intact() {
        let bytes = gbk_bytes("文档 README 说明");
        let (text, _) = decode(&bytes);
        assert_eq!(text, "文档 README 说明");
    }

    #[test]
    fn utf16_little_endian_bom_is_decoded() {
        let text = "记事本另存为 Unicode";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let (decoded, enc) = decode(&bytes);
        assert_eq!(decoded, text);
        assert_eq!(enc, Encoding::Utf16Le);
    }

    #[test]
    fn utf16_big_endian_bom_is_decoded() {
        let text = "大端序";
        let mut bytes = vec![0xFE, 0xFF];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        let (decoded, enc) = decode(&bytes);
        assert_eq!(decoded, text);
        assert_eq!(enc, Encoding::Utf16Be);
    }

    /// 奇数长度的 UTF-16 不能 panic，且要露出替换字符提示尾部损坏。
    #[test]
    fn truncated_utf16_does_not_panic() {
        let bytes = vec![0xFF, 0xFE, 0x41, 0x00, 0x42];
        let (text, enc) = decode(&bytes);
        assert_eq!(enc, Encoding::Utf16Le);
        assert!(text.starts_with('A'));
        assert!(text.contains('\u{fffd}'), "截断的半个码元应显式提示");
    }

    /// 任意字节输入都不能 panic——这是编辑器"什么文件都要能打开"的底线。
    #[test]
    fn arbitrary_bytes_never_panic() {
        let cases: &[&[u8]] = &[
            &[],
            &[0x00],
            &[0xFF],
            &[0xFF, 0xFF, 0xFF],
            &[0x81],             // 落单的 GBK 首字节
            &[0x81, 0x20],       // 非法尾字节
            &[0xB2, 0xE2, 0x81], // 正常汉字后跟落单首字节
            &[0x80],             // CP936 欧元符号
        ];
        for case in cases {
            let (text, _) = decode(case);
            // 只要求不 panic；空输入应得到空串。
            if case.is_empty() {
                assert!(text.is_empty());
            }
        }
    }

    /// 未定义但尾字节合法的组合，整对作废且只产出**一个**替换字符。
    #[test]
    fn undefined_gbk_pair_yields_one_replacement_char() {
        // 动态找一处"尾字节合法、但组合未定义"的空洞，而非硬编码——
        // 硬编码会随映射表重新生成而失效（如 0x81 0x40 实际是「丂」）。
        let (lead, trail) = (0x81u8..=0xFE)
            .find_map(|l| {
                (0x40u8..=0x7E)
                    .chain(0x80..=0xFE)
                    .find(|&t| gbk_lookup(l, t).is_none())
                    .map(|t| (l, t))
            })
            .expect("GBK 表中应存在未定义组合（前提检查）");
        assert!(is_gbk_trail(trail), "找到的尾字节应在合法范围内");

        let (text, enc) = decode(&[lead, trail, b'A']);
        assert_eq!(enc, Encoding::Gbk);
        assert_eq!(
            text, "\u{fffd}A",
            "整对应作废成一个替换字符，且不影响其后的 ASCII"
        );
    }

    /// 表规模与实现假设一致：改了生成脚本而忘了改常量，这里会失败。
    #[test]
    fn table_size_matches_declared_dimensions() {
        assert_eq!(
            GBK_TABLE.len(),
            GBK_ROWS * GBK_COLS * 2,
            "映射表尺寸与 GBK_ROWS × GBK_COLS 不符"
        );
    }

    /// **回归防线**：大部分合法、仅零星坏字节的 UTF-8，必须仍按 UTF-8 处理。
    ///
    /// 早先的实现会直接把这类文件整体交给 GBK 解码，后果是把**原本正确的中文
    /// 二次解码**成满篇看似正常却全错的汉字——比替换字符危险得多，
    /// 因为用户看不出问题，一保存就毁掉原文。
    #[test]
    fn mostly_valid_utf8_with_few_bad_bytes_keeps_its_chinese() {
        let mut bytes = "# 中文".as_bytes().to_vec();
        bytes.extend_from_slice(&[0xFF, 0xFE, 0x80]); // 零星坏字节
        bytes.extend_from_slice("\n\ntail\n".as_bytes());

        let (text, enc) = decode(&bytes);
        assert_eq!(enc, Encoding::Utf8, "应判为（损坏的）UTF-8，而不是 GBK");
        assert!(
            text.contains("中文"),
            "坏字节之前的中文必须原样保留，实际：{text:?}"
        );
        assert!(text.contains("tail"), "坏字节之后的正文必须保留");
        assert!(
            text.contains('\u{fffd}'),
            "坏字节应显式标记为替换字符，而不是被悄悄吞掉"
        );
    }

    /// 判据分档：真 UTF-8 的三字节序列多于双字节；GBK 反之。
    ///
    /// 这是"UTF-8 还是 GBK"的分叉点，直接决定文件是否被正确打开。
    #[test]
    fn utf8_multibyte_counts_discriminates_gbk_from_utf8() {
        // 真中文 UTF-8：汉字全是三字节序列。
        let (two, three) = utf8_multibyte_counts("中文测试文档".as_bytes());
        assert_eq!((two, three), (0, 6), "六个汉字应为六个三字节序列");
        assert!(three > two, "→ 应判为 UTF-8");

        // GBK 汉字：可能凑出合法双字节，但三字节不会多于双字节。
        let gbk = gbk_bytes("中文测试文档");
        let (two, three) = utf8_multibyte_counts(&gbk);
        assert!(
            three <= two,
            "GBK 文本若出现三字节多于双字节就会被误判为 UTF-8，实际 two={two} three={three}"
        );

        // GBK 中英混排是最容易被误判的一类，单独锁住。
        let mixed = gbk_bytes("文档 README 说明");
        let (two, three) = utf8_multibyte_counts(&mixed);
        assert!(
            three <= two,
            "GBK 中英混排不得被误判为 UTF-8，实际 two={two} three={three}"
        );

        // 含中文的损坏 UTF-8：三字节序列仍占多数 → 判 UTF-8。
        let mut damaged = "# 中文".as_bytes().to_vec();
        damaged.extend_from_slice(&[0xFF, 0xFE, 0x80]);
        let (two, three) = utf8_multibyte_counts(&damaged);
        assert!(three > two, "含中文的损坏 UTF-8 应判为 UTF-8");

        // 纯 ASCII 与空输入：两个计数都是 0（它们走严格 UTF-8 分支，到不了这里）。
        assert_eq!(utf8_multibyte_counts(b"hello world"), (0, 0));
        assert_eq!(utf8_multibyte_counts(&[]), (0, 0));
    }

    // ------------------------------------------------------------------
    // GB18030 四字节扩展
    // ------------------------------------------------------------------

    /// 把字符编成 GB18030 字节：优先双字节 GBK，否则四字节。
    fn gb18030_bytes(text: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for ch in text.chars() {
            let cp = ch as u32;
            if cp < 0x80 {
                out.push(cp as u8);
                continue;
            }
            if let Some((lead, trail)) = gbk_reverse().get(&cp) {
                out.push(*lead);
                out.push(*trail);
                continue;
            }
            let pointer = gb18030_cp_to_pointer(cp)
                .unwrap_or_else(|| panic!("U+{cp:04X} 应可编码为 GB18030"));
            out.extend_from_slice(&gb18030_pointer_to_bytes(pointer));
        }
        out
    }

    /// 四字节序列正确解码（此前会产生 `\u{FFFD}` + 3 个垃圾 ASCII 字符）。
    ///
    /// 字节值由 WHATWG 合规实现独立验证（U+20000 → `95 32 82 36`）。
    #[test]
    fn gb18030_four_byte_sequence_decodes() {
        // 𠀀 (U+20000)：纯四字节文件不是合法 UTF-8，落到 GBK 分支。
        // （分支本身由下面的 `assert_eq!(enc, Encoding::Gbk)` 钉住，无需前置断言——
        //   静态字节数组传给 `str::from_utf8` 会触发 `invalid_from_utf8` lint。）
        let bytes = [0x95u8, 0x32, 0x82, 0x36];
        let (text, enc) = decode(&bytes);
        assert_eq!(enc, Encoding::Gbk);
        assert_eq!(text, "\u{20000}");
        assert!(!text.contains('\u{fffd}'), "四字节不应产生替换字符");
    }

    /// GBK 双字节与 GB18030 四字节混排：各自正确解码，互不干扰。
    ///
    /// 四字节的第 2/4 字节是 `0x30..=0x39`（ASCII 数字区）——若误按 GBK
    /// 双字节处理，这些数字会被吞进序列或泄漏到输出里。
    #[test]
    fn gb18030_four_byte_and_gbk_two_byte_interleave() {
        // "中"（GBK 双字节 D6 D0）+ 𠀀（四字节）+ "文"（GBK 双字节 CE C4）。
        let bytes = gb18030_bytes("中\u{20000}文");
        let (text, _) = decode(&bytes);
        assert_eq!(text, "中\u{20000}文");
    }

    /// 表的双向映射必须互逆：抽全空间样本，encode ∘ decode 恒等。
    #[test]
    fn gb18030_range_table_round_trips() {
        // 均匀抽样 + 边界锚点。任何一条不互逆都会让保存后的文件读回来变字。
        let mut samples = Vec::new();
        for i in 0..2000u32 {
            // Knuth 乘法散列布满全空间（模 1_587_600 个四字节指针）。
            samples.push(i.wrapping_mul(2_654_435_761) % 1_587_600);
        }
        samples.extend([0, 1, 35, 36, 18_999, 19_043, 1_237_575, 1_587_599]);
        for pointer in samples {
            match gb18030_pointer_to_cp(pointer) {
                Some(ch) => {
                    let back = gb18030_cp_to_pointer(ch as u32);
                    assert_eq!(
                        back,
                        Some(pointer),
                        "U+{:04X} 经表往返应回到指针 {pointer}",
                        ch as u32
                    );
                }
                None => continue, // 未赋值指针无需互逆
            }
        }
    }

    /// 编码回写：GBK + 四字节混合文本经 encode → decode 往返无损。
    #[test]
    fn gb18030_encode_round_trips_through_decode() {
        let text = "中文与 English 混排，含扩展汉字 𠀀 与符号 ✓。";
        let bytes = encode(text, Encoding::Gbk);
        let (decoded, enc) = decode(&bytes);
        assert_eq!(enc, Encoding::Gbk);
        assert_eq!(decoded, text, "GBK/GB18030 编码回写必须无损往返");
    }

    /// 截断的四字节序列（缺第 4 字节）不得 panic；其后的字节按 WHATWG
    /// 「作废首字节、回退重读」的语义处理——后续若凑成合法 GBK 双字节，
    /// 按双字节解码是正确行为，不算"吞"。
    #[test]
    fn truncated_gb18030_sequence_does_not_panic() {
        // 95 32 82 = 𠀀 的前三个字节，缺第 4 字节；后面跟控制字符 0x07
        // （它既当不了 GBK 尾字节，也不是数字，必然原样保留）。
        let bytes = [0x95u8, 0x32, 0x82, 0x07];
        let (text, _) = decode(&bytes);
        assert!(text.contains('\u{fffd}'), "截断序列应显式替换：{text:?}");
        assert!(
            text.ends_with('\u{07}'),
            "其后的非序列字节不得被吞：{text:?}"
        );
    }

    /// UTF-16 编码回写：小端 / 大端各自补回 BOM 且往返无损。
    #[test]
    fn utf16_encode_round_trips_with_bom() {
        let text = "记事本 Unicode";
        for enc in [Encoding::Utf16Le, Encoding::Utf16Be] {
            let bytes = encode(text, enc);
            let (decoded, detected) = decode(&bytes);
            assert_eq!(detected, enc);
            assert_eq!(decoded, text, "{enc:?} 编码回写应往返无损");
        }
    }

    /// UTF-8 BOM 编码回写：`keeps_bom` 真正接到 `encode` 上。
    #[test]
    fn utf8_bom_encode_writes_bom_back() {
        let bytes = encode("正文", Encoding::Utf8Bom);
        assert!(bytes.starts_with(&UTF8_BOM), "BOM 应被写回");
        let (decoded, detected) = decode(&bytes);
        assert_eq!(detected, Encoding::Utf8Bom);
        assert_eq!(decoded, "正文");
    }
}
