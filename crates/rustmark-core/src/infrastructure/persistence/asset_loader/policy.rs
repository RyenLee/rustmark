//! 资源加载策略：限制参数、错误类型、已加载资源模型与 MIME / alt 推断。

use std::path::Path;
use std::time::Duration;

use crate::shared::error::AppError;

/// 资源加载的限制参数。
///
/// 体积上限**按来源分层**：data URI 是写进 Markdown 文本的内联数据，
/// 过大既撑爆文档也拖慢解析，故保持 512 KB；本地文件与远程图片没有
/// 文本膨胀问题，按普通照片的典型体积（2–5 MB）放到 8 MiB。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetLimits {
    /// data URI 允许内联的最大字节数（512 KiB）。
    pub max_inline_bytes: usize,
    /// 本地文件与远程下载的体积上限（8 MiB）。
    pub max_local_bytes: usize,
    /// 远程下载超时（10 秒）。
    ///
    /// 首版用同步下载，故必须有超时：否则一个无响应的服务器会把
    /// 整个预览线程挂住，用户看到的是「编辑器卡死了」。
    pub download_timeout: Duration,
}

impl Default for AssetLimits {
    fn default() -> Self {
        Self {
            max_inline_bytes: 512 * 1024,
            max_local_bytes: 8 * 1024 * 1024,
            download_timeout: Duration::from_secs(10),
        }
    }
}

/// 允许内联的图片 MIME 白名单。
///
/// **不含 `svg+xml`**：SVG 是 XML，可以内嵌`<script>`，
/// 内联进预览等于直接执行脚本。
pub(super) const ALLOWED_MIME: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/jpg",
    "image/gif",
    "image/webp",
    "image/bmp",
];

/// 资源加载错误。
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    /// 超过体积上限。
    ///
    /// 携带上限值与实际值，便于 UI 直接显示「当前 2.1 MB，上限 512 KB」。
    #[error(
        "图片体积 {actual} 字节超过上限 {limit} 字节；请压缩或缩小尺寸后重试，或改用相对路径引用"
    )]
    TooLarge {
        /// 实际字节数。
        actual: usize,
        /// [`AssetLimits::max_inline_bytes`] 的当前取值。
        limit: usize,
    },

    /// 不在白名单内的 data URI 类型（如 `data:text/html`）。
    #[error("不支持的内联资源类型：{mime}；出于安全考虑仅允许位图（png/jpeg/gif/webp/bmp）")]
    DisallowedMime {
        /// data URI 声明的 MIME 类型。
        mime: String,
    },

    /// data URI 格式非法或 base64 解码失败。
    #[error("内联资源格式非法：{reason}")]
    MalformedDataUri {
        /// 具体失败原因（格式残缺 / base64 解码错误等）。
        reason: String,
    },

    /// 远程下载失败。
    #[error("下载图片失败（{url}）：{reason}")]
    DownloadFailed {
        /// 下载目标 URL。
        url: String,
        /// 传输层失败原因（超时 / 连接被拒等）。
        reason: String,
    },

    /// 本地文件读取失败。
    #[error("读取图片失败（{path}）：{reason}")]
    ReadFailed {
        /// 读取目标路径。
        path: String,
        /// IO 层失败原因。
        reason: String,
    },

    /// 本地路径无法解析（基准文档缺失等）。
    #[error("无法解析图片路径「{raw}」：{reason}")]
    UnresolvedPath {
        /// 源码里出现的原始路径文本。
        raw: String,
        /// 解析失败原因（基准文档缺失 / 相对路径越界等）。
        reason: String,
    },
}

impl From<AssetError> for AppError {
    fn from(e: AssetError) -> Self {
        Self::Io {
            message: e.to_string(),
        }
    }
}

/// 已加载的图片资源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedAsset {
    /// 原始字节。
    pub bytes: Vec<u8>,
    /// MIME 类型（已归一为小写）。
    pub mime: String,
    /// 展示用的替代文本（文件名去扩展名）。
    pub alt: String,
}

impl LoadedAsset {
    /// 字节长度。
    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }

    /// 是否为空（恒为 false——空图片在加载阶段已被拒绝）。
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// 由扩展名推断 MIME 类型。
pub(super) fn mime_from_extension(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

/// 取文件名去扩展名作为 alt 文本（对齐原型 `altOf`）。
pub(super) fn alt_of(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "image".to_string())
}
