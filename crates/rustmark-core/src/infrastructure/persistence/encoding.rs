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
//! 见 `gb18030::gb18030_pointer_to_cp`）。Big5、Shift_JIS 等与 GBK 无兼容关系、
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
//!
//! # 模块结构
//!
//! facade 保留编码标签、探测入口与 UTF-8 分档判据；各编解码实现按技术能力拆分：
//! - `utf16`：UTF-16 两字节序解码；
//! - `gbk`：GBK 双字节解码与内嵌映射表；
//! - `gb18030`：四字节线性段双向查表；
//! - `writer`：按原编码回写与 GBK 反查表。

mod gb18030;
mod gbk;
mod utf16;
mod writer;

use gbk::decode_gbk;
use utf16::decode_utf16;

pub(crate) use writer::encode;

// 仅 facade 内联单测需要直接访问底层查表函数。

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
    /// 保存时是否应写回 BOM。
    ///
    /// 只有原本带 BOM 的 UTF-8 才补回——凭空加 BOM 会干扰
    /// 依赖文本首字节的工具（如 shell 脚本的 shebang）。
    #[cfg(test)]
    pub(crate) const fn keeps_bom(self) -> bool {
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
pub(crate) const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

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

#[cfg(test)]
mod tests;
