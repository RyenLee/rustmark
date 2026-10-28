//! 平台信任库根证书（从 `asset_loader` 抽出；R-07 Batch 2）。
//!
//! REL-03：exe 不再内置 Mozilla 根包，根证书改由平台信任库提供。
//! 本模块只做「读一次、缓存、克隆句柄」，不关心 HTTP 细节。

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
/// [`super::policy::AssetError::DownloadFailed`]，编辑器其余功能（含本地图片）
/// 不受影响。只留一条 `warn`，绝不 panic——`panic = "abort"` 下那等于整个
/// 进程退出。
pub(super) fn platform_root_certs() -> ureq::tls::RootCerts {
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
