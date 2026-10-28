//! 资源下载用的 HTTP agent（从 `asset_loader` 抽出；R-07 Batch 2）。
//!
//! 只负责「怎么造一个 ureq agent、默认超时的那份怎么复用」，
//! 平台根证书的读取与缓存见 [`super::platform_certs`]。

use std::sync::OnceLock;
use std::time::Duration;

use super::platform_certs::platform_root_certs;
use super::policy::AssetLimits;

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
pub(super) fn http_agent(timeout: Duration) -> ureq::Agent {
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
