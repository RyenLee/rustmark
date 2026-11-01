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
//! # 两道内容校验（评审 P2 #6 / #7）
//!
//! 白名单只回答「这个**类型**能不能放行」，不回答「这份内容真是该类型吗」。
//! 两个口子必须单独堵：
//!
//! - **符号链接越界（#6）**：`../` 的文本归一看不出
//!   `img.png -> /etc/passwd`。故读取前把 base 与目标都 `canonicalize`，
//!   要求目标真实路径仍在 base 子树内（`ensure_within_base_real`）。
//! - **远程内容伪装（#7）**：`Content-Type` 与 URL 扩展名都由对端控制，
//!   与字节无关。故边读边核对魔数（`policy::bytes_match_declared_mime`），
//!   声明与实际不同族即拒。
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
//! 限制 / 错误 / MIME 推断在 `policy`，零依赖编码原语在 `coding`，
//! HTTP agent 与平台根证书分别在 `agent` / `platform_certs`。

#[cfg(feature = "remote-images")]
mod agent;
mod coding;
#[cfg(feature = "remote-images")]
mod platform_certs;
mod policy;

// `Read` 仅 `load_remote` 的边读边判上限用到；`remote-images` 关闭时整条门控掉，
// 否则 `clippy -D warnings` 会报 unused import。
#[cfg(feature = "remote-images")]
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

#[cfg(feature = "remote-images")]
use agent::http_agent;
use coding::{decode_base64, percent_decode};
use policy::{ALLOWED_MIME, alt_of, mime_from_extension};
#[cfg(feature = "remote-images")]
use policy::{MAGIC_WINDOW, bytes_match_declared_mime};

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
    ///
    /// # 安全边界（F-02）
    ///
    /// 本地图片解析分两种信任语境，由 `base_dir` 区分：
    ///
    /// - **有文档上下文**（`base_dir = Some(doc_dir)`，即已打开 / 已保存的文档）：
    ///   文档内容可能来自不可信来源（下载的 `.md`、别人发的文件），其内嵌的
    ///   `![](…)` 绝不能读走文档目录外的任意文件。故绝对路径与 `../` 越界引用
    ///   一律拒绝，相对路径强制收束进 `doc_dir` 子树。
    /// - **无文档上下文**（`base_dir = None`，即未保存文档 / 用户刚用对话框选图）：
    ///   路径来自本次会话的受信用户操作（见「插入图片」对未命名文档插绝对路径的
    ///   契约），允许绝对路径直接读取；相对路径因无基准目录无法解析，明确报错。
    fn load_local(&self, raw: &str, base_dir: Option<&Path>) -> Result<LoadedAsset, AssetError> {
        let candidate = Path::new(raw);
        let path = match base_dir {
            Some(base) => {
                // 有文档上下文：收束进文档目录，绝对路径与 `../` 越界都拒。
                if candidate.is_absolute() {
                    return Err(AssetError::UnresolvedPath {
                        raw: raw.to_string(),
                        reason: "不允许绝对路径，图片必须相对当前文档引用".to_string(),
                    });
                }
                // 第一道：**文本分量**归一（拦 `../` 与盘符）。
                let lexical = Self::resolve_within_base(base, candidate).ok_or_else(|| {
                    AssetError::UnresolvedPath {
                        raw: raw.to_string(),
                        reason: "图片路径越过文档目录（../ 越界）".to_string(),
                    }
                })?;
                // 第二道：**真实路径**校验（评审 P2 #6）。文本归一看不出
                // `evil.png -> /etc/passwd` 这种符号链接，必须在读取前把
                // base 与目标都 canonicalize 后再比子树。
                Self::ensure_within_base_real(base, &lexical).ok_or_else(|| {
                    AssetError::UnresolvedPath {
                        raw: raw.to_string(),
                        reason: "图片路径解析后越过文档目录（符号链接指向目录外）".to_string(),
                    }
                })?
            }
            None => {
                // 无文档上下文：受信的用户选图路径，允许绝对路径直接读取。
                if candidate.is_absolute() {
                    candidate.to_path_buf()
                } else {
                    return Err(AssetError::UnresolvedPath {
                        raw: raw.to_string(),
                        reason: "相对路径需要先打开文档".to_string(),
                    });
                }
            }
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

    /// 把相对引用解析进 `base` 目录内，越界（`../` 逃出 base）即返回 `None`。
    ///
    /// 仅做**文本分量**归一（不解析符号链接），足以拦住 `../` 与绝对路径这两类
    /// 把读取范围扩大到文档目录之外的攻击面（F-02）。
    ///
    /// **符号链接不在这里管**：本函数是纯函数（不碰磁盘），而「链接指向哪里」
    /// 只有真实路径才知道。那道防线是 [`Self::ensure_within_base_real`]，
    /// 由 `load_local` 在读取前紧跟着调用——两个函数合起来才完整，
    /// 少任何一个都留一个口子。
    fn resolve_within_base(base: &Path, rel: &Path) -> Option<PathBuf> {
        let mut out = base.to_path_buf();
        let mut depth: usize = 0;
        for comp in rel.components() {
            match comp {
                // 绝对路径 / 带盘符前缀：调用方 `is_absolute()` 已拒过，这里再兜一层。
                Component::Prefix(_) | Component::RootDir => return None,
                Component::CurDir => {}
                Component::ParentDir => {
                    if depth == 0 {
                        return None; // 试图逃出 base
                    }
                    depth -= 1;
                    out.pop();
                }
                Component::Normal(c) => {
                    out.push(c);
                    depth += 1;
                }
            }
        }
        Some(out)
    }

    /// 真实路径校验：目标解析符号链接后必须仍位于 `base` 子树内（评审 P2 #6）。
    ///
    /// `resolve_within_base` 只看文本分量，`docs/img.png` 与
    /// `docs/img.png → /etc/passwd` 在它眼里一模一样——后者能把「打开一份
    /// 别人发来的 .md」变成「读取本机任意文件，并把内容画进预览区」。故在
    /// **真正读取之前**把两边都 canonicalize 再比子树。
    ///
    /// 返回**真实路径**（canonical）：后续 `metadata` / `read` 都走它，
    /// 消除了「校验完再被换成链接」的一部分窗口。
    ///
    /// 边界处理：
    /// - `base` 自身 canonicalize 失败（不存在 / 无权限）→ 拒绝：
    ///   无法确认基准在哪，任何放行都是猜测；
    /// - 目标 canonicalize 成功 → 必须 `starts_with(base_canon)`，
    ///   两边同源（Windows 上都带 `\\?\` 前缀）故可直接比较，不必剥前缀；
    /// - 目标 `NotFound` → 真实路径无从解析，退一步校验**最近的已存在祖先**
    ///   仍在 base 内，然后原样返回词法路径，让随后的 `fs::read` 给出
    ///   用户看得懂的「读取失败」而不是含糊的「路径非法」；
    /// - 其它 IO 错误（权限等）→ 拒绝，不猜。
    fn ensure_within_base_real(base: &Path, resolved: &Path) -> Option<PathBuf> {
        let base_canon = std::fs::canonicalize(base).ok()?;
        match std::fs::canonicalize(resolved) {
            Ok(real) => real.starts_with(&base_canon).then_some(real),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let ancestor = Self::nearest_existing_ancestor(resolved)?;
                let real_ancestor = std::fs::canonicalize(ancestor).ok()?;
                real_ancestor
                    .starts_with(&base_canon)
                    .then(|| resolved.to_path_buf())
            }
            Err(_) => None,
        }
    }

    /// 取 `path` 自身或其最近的**已存在**祖先（含符号链接跟随语义）。
    ///
    /// 仅用于「目标尚不存在」时的兜底校验：即便目标还没落盘，它所在的目录
    /// 也必须真实位于 base 内，否则 `fs::read` 一旦因竞态成功就越界了。
    fn nearest_existing_ancestor(path: &Path) -> Option<&Path> {
        let mut current = path;
        loop {
            if current.exists() {
                return Some(current);
            }
            current = current.parent()?;
        }
    }

    /// 下载远程图片（http / https 同一条通路）。
    ///
    /// 历史注记：首版手写 `TcpStream` + 裸 HTTP/1.0（S5-11 当时为体积拒绝
    /// 一切 HTTP 客户端 crate），代价是 https 直接报错「未内置 TLS」——而
    /// 现实文档里的图片地址绝大多数是 https（缺陷 025）。现改用 `ureq`
    /// （纯 Rust TLS：三平台行为一致、不链系统 OpenSSL），TLS / 重定向 /
    /// chunked 等 HTTP 细节全部交出去；本函数只保留三条
    /// **安全与资源边界**——超时、体积硬上限、MIME 白名单。
    ///
    /// `remote-images` feature 关闭时，本函数被替换为直接报错的替身
    /// （见下方 `#[cfg(not(feature = "remote-images"))]` 定义）。
    #[cfg(feature = "remote-images")]
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

        // 流式读取：边读边判**体积上限**、边读边判**字节头**（评审 P2 #7）。
        //
        // 为什么必须在读的时候就判魔数：声明 MIME 只有响应头与 URL 扩展名两个
        // 来源，二者完全由服务端 / 文档作者控制。若无字节校验，一份
        // `Content-Type: image/png` 的 HTML 错误页、被反代改写的响应、或任意
        // 二进制都会被当作「合法 png」交给解码器。攒够 [`MAGIC_WINDOW`] 字节
        // 立即判定：既不需要二次下载，也不需要把整份内容缓冲完才发现不对。
        let mut reader = response.into_body().into_reader();
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut magic_verified = false;
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
                    if !magic_verified && bytes.len() >= MAGIC_WINDOW {
                        Self::verify_magic(url, &mime, &bytes)?;
                        magic_verified = true;
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

        if !magic_verified {
            // 全文短于签名窗口：任何合法位图都不可能这么短（最小的 BMP 头
            // 也有 14 字节），故按「内容不是图片」处理，而不是放行半截数据。
            return Err(AssetError::DownloadFailed {
                url: url.to_string(),
                reason: format!("响应体仅 {} 字节，不足以构成 {mime} 图片", bytes.len()),
            });
        }

        Ok(LoadedAsset {
            bytes,
            mime,
            alt: alt_of(&final_path),
        })
    }

    /// 校验响应字节头与**声明** MIME 同族（评审 P2 #7）。
    ///
    /// 拒绝时用 `DisallowedMime`：它已是「这个类型我们不接受」的既有语义出口
    /// （上层统一按「该资源不可用 → 占位图」降级），只是 `mime` 里补上
    /// 「声明 ≠ 实际」的信息，便于用户与日志定位。
    #[cfg(feature = "remote-images")]
    fn verify_magic(url: &str, mime: &str, head: &[u8]) -> Result<(), AssetError> {
        if bytes_match_declared_mime(mime, head) {
            return Ok(());
        }
        log::warn!("远程图片字节头与声明类型不符，已拒绝：{url}（声明 {mime}）");
        Err(AssetError::DisallowedMime {
            mime: format!("{mime}（响应字节不是该格式）"),
        })
    }

    /// 未启用 `remote-images` 时的替身：明确报错，而不是把 `https://…`
    /// 当成本地路径去 `fs::read`。调用方（预览取图）会降级为占位图。
    #[cfg(not(feature = "remote-images"))]
    fn load_remote(&self, url: &str) -> Result<LoadedAsset, AssetError> {
        Err(AssetError::DownloadFailed {
            url: url.to_string(),
            reason: "本构建未启用远程图片（remote-images feature 关闭）".to_string(),
        })
    }
}

#[cfg(test)]
mod tests;
