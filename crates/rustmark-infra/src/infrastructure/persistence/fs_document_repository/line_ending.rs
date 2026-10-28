//! 文件换行风格（从 `fs_document_repository` 抽出；R-07 Batch 2）。
//!
//! 只做「探测 / 平台默认」的判定，不含任何 IO；读写流程仍在门面
//! [`super`] 侧，保持单一真源。

/// 文件的换行风格。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LineEnding {
    /// `\n`（Unix / 默认为新文件）。
    #[default]
    Lf,
    /// `\r\n`（Windows）。
    CrLf,
}

impl LineEnding {
    /// 按当前平台推断新建文件应使用的换行。
    ///
    /// Windows 记事本等工具对纯 LF 支持不佳，故新建文件用 `CRLF` 更友好。
    pub(crate) const fn platform_default() -> Self {
        if cfg!(windows) { Self::CrLf } else { Self::Lf }
    }

    /// 由**原始字节**探测换行风格（出现 `\r\n` 即为 CRLF）。
    ///
    /// 必须传原始字节而非已解码文本：门面的 `decode` 会把 CRLF 归一为 LF，
    /// 若拿解码结果来探测，CRLF 文件永远会被误判成 LF，
    /// 于是「读时归一 + 写时还原」里的还原步骤失效——用户的 Windows 文件
    /// 会在我们保存一次后被整体改成 LF（git diff 噪声）。
    pub(crate) fn detect_bytes(bytes: &[u8]) -> Self {
        if bytes.windows(2).any(|pair| pair == b"\r\n") {
            Self::CrLf
        } else {
            Self::Lf
        }
    }
}
