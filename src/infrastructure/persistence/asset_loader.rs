//! 图片与资源加载（S5-11）。
//!
//! # 三种来源，一个出口
//!
//! Markdown 里的图片有三种写法，必须统一成「拿到字节 + 知道 MIME」：
//! 1. **本地相对路径** `![](images/a.png)` —— 相对**当前文档**解析；
//! 2. **远程 URL** `![](https://…/a.png)` —— 首版同步下载并带超时；
//! 3. **内联 data URI** `![](data:image/png;base64,…)`。
//!
//! # 安全：只放行位图data URI（ADR-002 正面经验）
//!
//! `data:text/html;base64,…` 是 XSS 向量——它能在页面里执行脚本。
//! 故白名单**精确到 MIME 子类型**，只放行 `png / jpeg / jpg / gif / webp / bmp`，
//! 且**排除 `svg+xml`**（SVG 内部可以携带 `<script>`）。
//! 这条规则与 `html_sanitize::is_safe_url` 保持一致，两处必须同源。
//!
//! # 体积：硬约束而非提示（原型缺陷 2.1 的根治）
//!
//! 原型对超过 `IMAGE_WARN_BYTES`（512 KB）的图片**只提示但依然插入**，
//! 结果是 localStorage 配额超限、整篇文档存不下去——提示没有约束力。
//! 本实现改为**硬约束**：超过 [`AssetLimits::max_inline_bytes`] 直接返回
//! [`AssetError::TooLarge`]，并带上「建议改用相对路径」的可操作提示。
//!
//! # 为什么解码失败不 `panic`
//!
//! `panic = "abort"` 下任何 `unwrap()` 都直接终止进程，而图片解码是
//! **外部输入驱动的**：损坏的 PNG、伪装成 PNG 的文本、被截断的网络响应
//! 都会走到解码路径。编辑器绝不能因为一张坏图而退出。

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::shared::error::AppError;

/// 资源加载的限制参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetLimits {
    /// 允许内联的最大字节数（默认 512 KB，与原型的 `IMAGE_WARN_BYTES` 同值）。
    pub max_inline_bytes: usize,
    /// 远程下载超时（默认 5秒）。
    ///
    /// 首版用同步下载，故必须有超时：否则一个无响应的服务器会把
    /// 整个预览线程挂住，用户看到的是「编辑器卡死了」。
    pub download_timeout: Duration,
}

impl Default for AssetLimits {
    fn default() -> Self {
        Self {
            max_inline_bytes: 512 * 1024,
            download_timeout: Duration::from_secs(5),
        }
    }
}

/// 允许内联的图片 MIME 白名单。
///
/// **不含 `svg+xml`**：SVG 是 XML，可以内嵌`<script>`，
/// 内联进预览等于直接执行脚本。
const ALLOWED_MIME: &[&str] = &[
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
    #[error("图片体积 {actual} 字节超过上限 {limit} 字节；请改用相对路径引用，或压缩后再插入")]
    TooLarge { actual: usize, limit: usize },

    /// 不在白名单内的 data URI 类型（如 `data:text/html`）。
    #[error("不支持的内联资源类型：{mime}；出于安全考虑仅允许位图（png/jpeg/gif/webp/bmp）")]
    DisallowedMime { mime: String },

    /// data URI 格式非法或 base64 解码失败。
    #[error("内联资源格式非法：{reason}")]
    MalformedDataUri { reason: String },

    /// 远程下载失败。
    #[error("下载图片失败（{url}）：{reason}")]
    DownloadFailed { url: String, reason: String },

    /// 本地文件读取失败。
    #[error("读取图片失败（{path}）：{reason}")]
    ReadFailed { path: String, reason: String },

    /// 本地路径无法解析（基准文档缺失等）。
    #[error("无法解析图片路径「{raw}」：{reason}")]
    UnresolvedPath { raw: String, reason: String },
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
    /// 是否为位图（恒为 true——非位图在加载阶段已被拒绝）。
    pub const fn is_bitmap(&self) -> bool {
        true
    }

    /// 字节长度。
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// 是否为空（恒为 false——空图片在加载阶段已被拒绝）。
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// 由扩展名推断 MIME 类型。
fn mime_from_extension(path: &Path) -> Option<&'static str> {
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
fn alt_of(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "image".to_string())
}

/// 图片与资源加载器。
#[derive(Debug, Clone, Copy)]
pub struct AssetLoader {
    limits: AssetLimits,
}

impl Default for AssetLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetLoader {
    /// 用默认限制构造。
    pub const fn new() -> Self {
        Self {
            limits: AssetLimits {
                max_inline_bytes: 512 * 1024,
                download_timeout: Duration::from_secs(5),
            },
        }
    }

    /// 用自定义限制构造。
    pub const fn with_limits(limits: AssetLimits) -> Self {
        Self { limits }
    }

    /// 当前的体积上限。
    pub const fn max_inline_bytes(&self) -> usize {
        self.limits.max_inline_bytes
    }

    /// 判定是否为受支持的图片引用（本地路径 / 远程 URL / data URI）。
    ///
    /// 「看起来像路径」不等于「可加载」：`ftp://x/y.png` 扩展名是 `.png`，
    /// 但没有任何加载器支持 ftp，直接返回 true 会让上层把它当成可用图片，
    /// 随后在 `load` 里报错——错误点离真正的原因太远。
    /// 故此处对**带 scheme 的引用**显式只认 `http` / `https` / `data`。
    pub fn is_image_reference(raw: &str) -> bool {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return false;
        }
        if let Some(scheme_end) = trimmed.find("://") {
            let scheme = trimmed[..scheme_end].to_ascii_lowercase();
            return matches!(scheme.as_str(), "http" | "https");
        }
        if trimmed.starts_with("data:") {
            return true;
        }
        mime_from_extension(Path::new(trimmed)).is_some()
    }

    /// 加载图片（自动识别三种来源）。
    ///
    /// `base_dir` 用于解析相对路径，为 `None` 时相对路径无法解析而报错。
    pub fn load(&self, raw: &str, base_dir: Option<&Path>) -> Result<LoadedAsset, AssetError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(AssetError::UnresolvedPath {
                raw: raw.to_string(),
                reason: "引用为空".to_string(),
            });
        }

        let asset = if let Some(rest) = trimmed.strip_prefix("data:") {
            self.load_data_uri(rest)?
        } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            self.load_remote(trimmed)?
        } else if let Some(scheme_end) = trimmed.find("://") {
            // 带 scheme 但不是 http/https/data（如 ftp://、file://）：
            // 明确报错，而不是把 `x://…/a.png` 当成本地路径去 `fs::read`。
            let scheme = &trimmed[..scheme_end];
            return Err(AssetError::DownloadFailed {
                url: trimmed.to_string(),
                reason: format!("不支持的协议「{scheme}」：仅支持 http/https 与本地路径"),
            });
        } else {
            self.load_local(trimmed, base_dir)?
        };

        // 体积硬约束放在最后统一判定：三条来源都要过这一关，
        // 避免某个分支忘了检查（这正是原型「只提示不约束」的教训）。
        if asset.len() > self.limits.max_inline_bytes {
            return Err(AssetError::TooLarge {
                actual: asset.len(),
                limit: self.limits.max_inline_bytes,
            });
        }
        Ok(asset)
    }

    /// 解析 `data:` URI（`rest` 是不含 `data:` 前缀的部分）。
    fn load_data_uri(&self, rest: &str) -> Result<LoadedAsset, AssetError> {
        // 形如 `image/png;base64,AAAA`；也允许省略 `;base64`（纯文本数据）。
        let (meta, payload) = rest
            .split_once(',')
            .ok_or_else(|| AssetError::MalformedDataUri {
                reason: "缺少 `,` 分隔符".to_string(),
            })?;

        let mut parts = meta.split(';');
        let mime = parts.next().unwrap_or_default().trim().to_ascii_lowercase();

        // 白名单精确到子类型；`svg+xml` 不在其中。
        if !ALLOWED_MIME.contains(&mime.as_str()) {
            return Err(AssetError::DisallowedMime { mime });
        }

        let is_base64 = parts.any(|p| p.trim().eq_ignore_ascii_case("base64"));
        let bytes = if is_base64 {
            decode_base64(payload.trim()).ok_or_else(|| AssetError::MalformedDataUri {
                reason: "base64 解码失败".to_string(),
            })?
        } else {
            // 未标注 base64：按 URL 百分号编码解析。
            percent_decode(payload).ok_or_else(|| AssetError::MalformedDataUri {
                reason: "百分号编码非法".to_string(),
            })?
        };

        if bytes.is_empty() {
            return Err(AssetError::MalformedDataUri {
                reason: "解码后为空".to_string(),
            });
        }

        Ok(LoadedAsset {
            bytes,
            mime,
            alt: alt_of("inline"),
        })
    }

    /// 读取本地图片。
    fn load_local(&self, raw: &str, base_dir: Option<&Path>) -> Result<LoadedAsset, AssetError> {
        let candidate = PathBuf::from(raw);
        let path = if candidate.is_absolute() {
            candidate
        } else {
            // 相对路径必须相对**当前文档**解析；没有基准目录就是无法解析，
            // 此时给出明确错误而非静默失败。
            let base = base_dir.ok_or_else(|| AssetError::UnresolvedPath {
                raw: raw.to_string(),
                reason: "相对路径需要先打开文档".to_string(),
            })?;
            base.join(candidate)
        };

        let mime = mime_from_extension(&path)
            .ok_or_else(|| AssetError::UnresolvedPath {
                raw: raw.to_string(),
                reason: "不支持的图片扩展名".to_string(),
            })?
            .to_string();

        // SVG 即使扩展名合法也不内联——见 ALLOWED_MIME 的说明。
        if !ALLOWED_MIME.contains(&mime.as_str()) {
            return Err(AssetError::DisallowedMime { mime });
        }

        let bytes = std::fs::read(&path).map_err(|e| AssetError::ReadFailed {
            path: path.to_string_lossy().to_string(),
            reason: e.to_string(),
        })?;
        if bytes.is_empty() {
            return Err(AssetError::ReadFailed {
                path: path.to_string_lossy().to_string(),
                reason: "文件为空".to_string(),
            });
        }

        Ok(LoadedAsset {
            bytes,
            mime,
            alt: alt_of(raw),
        })
    }

    /// 下载远程图片。
    ///
    /// 首版用系统能力做同步下载，故只能支持 `http(s)` 且必须能超时。
    /// 这里**不引入 HTTP 客户端 crate**：一个最小实现只用到
    /// `TcpStream`，而 `reqwest` 之类的完整客户端会显著增加体积
    /// （且带一堆 TLS 依赖）。真正的异步下载留给阶段 08。
    fn load_remote(&self, url: &str) -> Result<LoadedAsset, AssetError> {
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .ok_or_else(|| AssetError::DownloadFailed {
                url: url.to_string(),
                reason: "仅支持 http/https".to_string(),
            })?;

        let (authority, path) = match rest.find('/') {
            Some(slash) => rest.split_at(slash),
            None => (rest, "/"),
        };
        // 只支持 http（明文）——TLS 需要外部 crate，
        // 静默降级到 http 会让用户以为是安全的，故直接报错提示。
        if url.starts_with("https://") {
            return Err(AssetError::DownloadFailed {
                url: url.to_string(),
                reason: "当前版本未内置 TLS，请改用本地图片或 http 地址".to_string(),
            });
        }

        let mut stream =
            std::net::TcpStream::connect(authority).map_err(|e| AssetError::DownloadFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?;
        stream
            .set_read_timeout(Some(self.limits.download_timeout))
            .map_err(|e| AssetError::DownloadFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?;
        stream
            .set_write_timeout(Some(self.limits.download_timeout))
            .map_err(|e| AssetError::DownloadFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?;

        let request = format!(
            "GET {path} HTTP/1.0\r\nHost: {authority}\r\nConnection: close\r\nUser-Agent: rustmark\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| AssetError::DownloadFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?;

        let mut response = Vec::new();
        // 边读边判上限：恶意/超大响应不应先全量进内存。
        let mut chunk = [0u8; 8192];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    response.extend_from_slice(&chunk[..n]);
                    if response.len() > self.limits.max_inline_bytes * 2 {
                        return Err(AssetError::TooLarge {
                            actual: response.len(),
                            limit: self.limits.max_inline_bytes,
                        });
                    }
                }
                Err(e) => {
                    return Err(AssetError::DownloadFailed {
                        url: url.to_string(),
                        reason: e.to_string(),
                    });
                }
            }
        }

        let body = split_http_body(&response).ok_or_else(|| AssetError::DownloadFailed {
            url: url.to_string(),
            reason: "响应格式非法".to_string(),
        })?;

        // MIME 优先取响应头；缺失时按路径扩展名推断。
        let mime = String::from_utf8_lossy(&response)
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("content-type:"))
            .and_then(|l| l.split(':').nth(1))
            .map(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
            })
            .filter(|v| ALLOWED_MIME.contains(&v.as_str()))
            .or_else(|| {
                mime_from_extension(Path::new(path))
                    .filter(|m| ALLOWED_MIME.contains(m))
                    .map(str::to_string)
            })
            .ok_or_else(|| AssetError::DisallowedMime {
                mime: "unknown".to_string(),
            })?;

        if body.is_empty() {
            return Err(AssetError::DownloadFailed {
                url: url.to_string(),
                reason: "响应体为空".to_string(),
            });
        }

        Ok(LoadedAsset {
            bytes: body.to_vec(),
            mime,
            alt: alt_of(path),
        })
    }
}

/// 从 HTTP 响应中切出响应体（跳过状态行与头部）。
fn split_http_body(response: &[u8]) -> Option<&[u8]> {
    const SEP: &[u8] = b"\r\n\r\n";
    let index = response.windows(SEP.len()).position(|w| w == SEP)?;
    Some(&response[index + SEP.len()..])
}

/// 最小 base64 解码器。
///
/// 不引依赖的理由：base64 逻辑只有约 40 行，而 `base64` crate 会
/// 带来额外体积；更重要的是解码失败必须能被**测试覆盖**，
/// 自己实现才能对畸形输入的每条分支加断言。
fn decode_base64(input: &str) -> Option<Vec<u8>> {
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
fn percent_decode(input: &str) -> Option<Vec<u8>> {
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
