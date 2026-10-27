//! 文本编码**标签**（共享内核，V11-03 / DEF-05）。
//!
//! # 为何放在 shared
//!
//! 编码由**基础设施层**探测（要读字节、查 GB18030 映射表，属 IO），
//! 却要在**状态栏**（表现层）展示。而：
//!
//! - 表现层禁止 `use crate::infrastructure::*`（`docs/architecture-constraints.md` §4）；
//! - 领域层禁止触碰 IO 类型（`.agent/conventions.md` §六）。
//!
//! 于是把「这是什么编码」抽成一个**纯值**（不含任何解码逻辑）放进共享内核：
//! 三层都能引用，且不产生任何反向依赖。
//!
//! # 与基础设施层 `Encoding` 的关系
//!
//! `infrastructure::persistence::encoding::Encoding` 带解码 / 编码实现
//! （BOM 判定、UTF-16 双字节、GB18030 四字节映射）；本枚举只描述结果，
//! 由基础设施层在探测完成后做**一次映射**（`From<Encoding>`）。
//! 两者一一对应，不新增编码种类。

/// 文本编码标签（纯展示值，无解码能力）。
///
/// 用于状态栏显示与文档元信息传递。`None`（在调用方侧）表示
/// 「尚未落盘 / 未探测」——新建未保存的文档没有编码可言。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TextEncoding {
    /// 无 BOM 的 UTF-8：新建文档的落盘默认值。
    #[default]
    Utf8,
    /// 带 BOM 的 UTF-8。
    Utf8Bom,
    /// UTF-16 小端（`FF FE`）。
    Utf16Le,
    /// UTF-16 大端（`FE FF`）。
    Utf16Be,
    /// GBK / CP936（含 GB18030 四字节扩展的读写）。
    Gbk,
}

impl TextEncoding {
    /// 展示名（状态栏用）。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Gbk => "GBK",
        }
    }

    /// 悬停提示：说明编码是自动识别的、且保存时会写回原编码。
    pub const fn hint(self) -> &'static str {
        "由文件内容自动识别；保存时写回原编码"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_honest_for_every_variant() {
        assert_eq!(TextEncoding::Utf8.label(), "UTF-8");
        assert_eq!(TextEncoding::Utf8Bom.label(), "UTF-8 BOM");
        assert_eq!(TextEncoding::Utf16Le.label(), "UTF-16 LE");
        assert_eq!(TextEncoding::Utf16Be.label(), "UTF-16 BE");
        assert_eq!(TextEncoding::Gbk.label(), "GBK");
    }

    #[test]
    fn default_is_utf8_without_bom() {
        // 新建文档落盘走 UTF-8 无 BOM：凭空加 BOM 会干扰依赖首字节的工具。
        assert_eq!(TextEncoding::default(), TextEncoding::Utf8);
    }

    #[test]
    fn hint_mentions_write_back() {
        assert!(TextEncoding::Gbk.hint().contains("写回原编码"));
    }
}
