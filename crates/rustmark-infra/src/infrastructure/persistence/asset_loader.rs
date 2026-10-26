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
//! 这条规则的真源是 `infrastructure::image_policy`，导出消毒器
//! （`html_sanitize::is_safe_url`）与之**共用同一张表**——各持一份时，
//! 同一张图会在「预览放行 / 导出被剥」之间悄悄不一致，且两边都不报错。
//!
//! # 体积：硬约束而非提示（原型缺陷 2.1 的根治）
//!
//! 原型对超过 `IMAGE_WARN_BYTES`（512 KB）的图片**只提示但依然插入**，
//! 结果是 localStorage 配额超限、整篇文档存不下去——提示没有约束力。
//! 本实现改为**硬约束**：按来源分层——data URI 超过
//! [`AssetLimits::max_inline_bytes`]（512 KiB）、本地/远程超过
//! [`AssetLimits::max_local_bytes`]（8 MiB）即返回 [`AssetError::TooLarge`]，
//! 并带上可操作提示。
//!
//! # 为什么解码失败不 `panic`
//!
//! `panic = "abort"` 下任何 `unwrap()` 都直接终止进程，而图片解码是
//! **外部输入驱动的**：损坏的 PNG、伪装成 PNG 的文本、被截断的网络响应
//! 都会走到解码路径。编辑器绝不能因为一张坏图而退出。
//!
//! # 模块结构
//!
//! facade 保留 `AssetLoader` 与三种来源的加载流程；
//! 限制 / 错误 / MIME 推断在 `policy`，零依赖编码原语在 `coding`。

mod coding;
mod policy;

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use coding::{decode_base64, percent_decode};
use policy::{ALLOWED_MIME, alt_of, mime_from_extension};

pub use policy::{AssetError, AssetLimits, LoadedAsset};

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
                max_local_bytes: 8 * 1024 * 1024,
                download_timeout: AssetLimits::DEFAULT_DOWNLOAD_TIMEOUT,
            },
        }
    }

    /// 用自定义限制构造。
    pub const fn with_limits(limits: AssetLimits) -> Self {
        Self { limits }
    }

    /// 当前的 data URI 内联上限。
    pub const fn max_inline_bytes(&self) -> usize {
        self.limits.max_inline_bytes
    }

    /// 当前的本地 / 远程图片上限。
    pub const fn max_local_bytes(&self) -> usize {
        self.limits.max_local_bytes
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

        // 分层上限：data URI 是文本内联（512 KiB），本地与远程是资源文件（8 MiB）。
        let (asset, limit) = if let Some(rest) = trimmed.strip_prefix("data:") {
            (self.load_data_uri(rest)?, self.limits.max_inline_bytes)
        } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            (self.load_remote(trimmed)?, self.limits.max_local_bytes)
        } else if let Some(scheme_end) = trimmed.find("://") {
            // 带 scheme 但不是 http/https/data（如 ftp://、file://）：
            // 明确报错，而不是把 `x://…/a.png` 当成本地路径去 `fs::read`。
            let scheme = &trimmed[..scheme_end];
            return Err(AssetError::DownloadFailed {
                url: trimmed.to_string(),
                reason: format!("不支持的协议「{scheme}」：仅支持 http/https 与本地路径"),
            });
        } else {
            (
                self.load_local(trimmed, base_dir)?,
                self.limits.max_local_bytes,
            )
        };

        // 体积硬约束放在最后统一判定：三条来源都要过这一关，
        // 避免某个分支忘了检查（这正是原型「只提示不约束」的教训）。
        if asset.len() > limit {
            return Err(AssetError::TooLarge {
                actual: asset.len(),
                limit,
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

        // 先看元数据再读：超限文件若先 `fs::read` 进内存再拒绝，
        // 一张数百 MB 的图会先造成内存尖峰（远程路径已是边读边判）。
        //
        // 这是**乐观预检**（评估报告 E-22）：`metadata` 与下面的 `fs::read`
        // 之间存在 TOCTOU 窗口，文件可能在两次 IO 之间被换成超大文件。
        // 这里刻意不做「读完再二次 stat」的收紧——那会多一次 IO 且窗口只是
        // 变窄而非关闭。真正的上界由外层 `load()` 在拿到字节后按
        // `asset.len() > limit` 再判一次（内联 data URI 路径同理），
        // 故竞态最坏只让一次超限分配漏到读后校验，不会放过超限内容。
        if let Ok(meta) = std::fs::metadata(&path) {
            let too_large = usize::try_from(meta.len())
                .map(|len| len > self.limits.max_local_bytes)
                .unwrap_or(true);
            if too_large {
                return Err(AssetError::TooLarge {
                    actual: usize::try_from(meta.len()).unwrap_or(usize::MAX),
                    limit: self.limits.max_local_bytes,
                });
            }
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

    /// 下载远程图片（http / https 同一条通路）。
    ///
    /// 历史注记：首版手写 `TcpStream` + 裸 HTTP/1.0（S5-11 当时为体积拒绝
    /// 一切 HTTP 客户端 crate），代价是 https 直接报错「未内置 TLS」——而
    /// 现实文档里的图片地址绝大多数是 https（缺陷 025）。现改用 `ureq`
    /// （纯 Rust TLS：三平台行为一致、不链系统 OpenSSL），TLS / 重定向 /
    /// chunked 等 HTTP 细节全部交出去；本函数只保留三条
    /// **安全与资源边界**——超时、体积硬上限、MIME 白名单。
    fn load_remote(&self, url: &str) -> Result<LoadedAsset, AssetError> {
        let agent = http_agent(self.limits.download_timeout);
        let response = agent
            .get(url)
            .header("User-Agent", "rustmark")
            .call()
            .map_err(|e| AssetError::DownloadFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?;

        if !response.status().is_success() {
            return Err(AssetError::DownloadFailed {
                url: url.to_string(),
                reason: format!("HTTP 状态码 {}", response.status().as_u16()),
            });
        }

        // MIME 优先取响应头；缺失时按**最终 URL**（重定向后）的扩展名推断，
        // 判定次序与白名单和旧实现一致。
        let final_path = ureq::ResponseExt::get_uri(&response).path().to_string();
        let mime = response
            .body()
            .mime_type()
            .map(str::to_ascii_lowercase)
            .filter(|v| ALLOWED_MIME.contains(&v.as_str()))
            .or_else(|| {
                mime_from_extension(Path::new(&final_path))
                    .filter(|m| ALLOWED_MIME.contains(m))
                    .map(str::to_string)
            })
            .ok_or_else(|| AssetError::DisallowedMime {
                mime: "unknown".to_string(),
            })?;

        // 边读边判上限：恶意/超大响应不应先全量进内存。
        let mut reader = response.into_body().into_reader();
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.len() > self.limits.max_local_bytes {
                        return Err(AssetError::TooLarge {
                            actual: bytes.len(),
                            limit: self.limits.max_local_bytes,
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

        if bytes.is_empty() {
            return Err(AssetError::DownloadFailed {
                url: url.to_string(),
                reason: "响应体为空".to_string(),
            });
        }

        Ok(LoadedAsset {
            bytes,
            mime,
            alt: alt_of(&final_path),
        })
    }
}

/// 进程级共享的 HTTP agent（默认超时那一份）。
///
/// 见 [`http_agent`]。
static DEFAULT_AGENT: OnceLock<ureq::Agent> = OnceLock::new();

/// 取用于下载的 HTTP agent。
///
/// # 为什么要复用
///
/// 早先每次下载都 `Agent::new_with_config`：连接池、DNS 缓存、TLS 会话
/// 一律从头开始，一张远程图一次完整握手。含 N 张远程图的文档要付 N 次
/// 握手成本——而它们指向的常常是同一个图床主机。`ureq::Agent` 内部是
/// 连接池 + 引用计数，`clone` 极廉价且可跨线程共享（后台下载线程人手一份
/// 也不会互相打架），故缓存一份复用。
///
/// # 为什么只对默认超时复用
///
/// 超时是烧进 agent 配置的，而 agent 是进程级共享的：若把任意超时都塞进
/// 同一个单例，A 处配的 30 秒会悄悄作用到 B 处那张本该 10 秒就放弃的图上。
/// 故只有超时等于 [`AssetLimits::DEFAULT_DOWNLOAD_TIMEOUT`] 的加载器共用
/// 单例（生产路径全部如此）；自定义超时退回按次构造——宁可少一次复用，
/// 也不能让限额配置互相串味。
fn http_agent(timeout: Duration) -> ureq::Agent {
    if timeout != AssetLimits::DEFAULT_DOWNLOAD_TIMEOUT {
        return build_agent(timeout);
    }
    DEFAULT_AGENT
        .get_or_init(|| build_agent(AssetLimits::DEFAULT_DOWNLOAD_TIMEOUT))
        // Agent 的 clone 是廉价的句柄复制（内部引用计数），连接仍在池里。
        .clone()
}

/// 按给定超时新建一个 agent（含平台根证书与 TLS 配置）。
fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            // REL-03：exe 不再内置 Mozilla 根包，根证书改由平台信任库提供
            // （见 [`platform_root_certs`]）。不显式注入会走 ureq 的
            // `RootCerts::WebPki` 默认值——该 feature 已关，运行期 panic。
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(platform_root_certs())
                    .build(),
            )
            .timeout_global(Some(timeout))
            .build(),
    )
}

/// 进程级缓存：平台信任库只读一次。
///
/// 读信任库在部分平台要解析数百 KB 的证书包，逐张图片重读纯属浪费；
/// 根集本身在进程生命周期内不会变（改信任库需要重启应用，可接受）。
static PLATFORM_ROOTS: std::sync::OnceLock<ureq::tls::RootCerts> = std::sync::OnceLock::new();

/// 平台信任库里的根证书（REL-03：替代此前内置的 `webpki-roots`）。
///
/// # 为什么改读平台
///
/// 内置 Mozilla 根包是 exe 里最后一块**非必需**的大数据（约 400 KB 裸 DER，
/// 压不进 `.text` 的压缩收益），而系统信任库本来就在维护同一份清单，还能
/// 覆盖企业自建 CA / TLS 代理。关掉 ureq 的 `rustls-webpki-roots` 后，
/// `RootCerts::WebPki` 会在建连时 panic（见 ureq `tls/rustls.rs`），
/// 故这里**必须**显式给出根集。
///
/// # 读不到时怎么办
///
/// 极简容器可能没有 `/etc/ssl/certs`、`SSL_CERT_FILE` 也未设置。此时返回
/// **空根集**：https 下载以「证书校验失败」收场并落到
/// [`AssetError::DownloadFailed`]，编辑器其余功能（含本地图片）不受影响。
/// 只留一条 `warn`，绝不 panic——`panic = "abort"` 下那等于整个进程退出。
fn platform_root_certs() -> ureq::tls::RootCerts {
    PLATFORM_ROOTS
        .get_or_init(|| {
            let native = rustls_native_certs::load_native_certs();
            let certs: Vec<ureq::tls::Certificate<'static>> = native
                .certs
                .iter()
                .map(|der| ureq::tls::Certificate::from_der(der.as_ref()).to_owned())
                .collect();
            if certs.is_empty() {
                log::warn!(
                    "平台信任库未读到任何根证书（{} 个错误），https 图片将无法通过证书校验",
                    native.errors.len()
                );
            } else {
                log::debug!(
                    "已从平台信任库装载 {} 张根证书（{} 个读取错误）",
                    certs.len(),
                    native.errors.len()
                );
            }
            ureq::tls::RootCerts::new_with_certs(&certs)
        })
        .clone()
}
