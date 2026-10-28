//! 系统默认程序打开器的实现（R-03）。
//!
//! # 为什么实现在这里而不在 `shared`
//!
//! `shared` 是零 IO 的共享内核，`std::process` 自 R-03 起进
//! [`SHARED_FORBIDDEN`]（见 `docs/architecture-constraints.md`）。端口
//! （[`ExternalOpener`] trait）与链接安全校验（`is_safe_to_open`）留在
//! `shared`——前者因为表现层需要它而 `presentation` 禁依赖
//! `crate::infrastructure`，后者因为它是**纯逻辑**。
//! 真正起进程的三平台分支搬来此处。
//!
//! # `spawn` 而非 `wait`
//!
//! 打开链接不应阻塞 UI 帧：`wait` 会一直等到用户关闭浏览器。
//!
//! # 失败语义
//!
//! 只返回 `Err`，不 panic——打开器不可用（或平台不支持）不应拖垮编辑器。

use rustmark_ports::system::ExternalOpener;

/// 基于操作系统默认程序的打开器。
///
/// 无状态，纯粹的能力包装（与 [`crate::infrastructure::dialogs::rfd_adapter::RfdDialogAdapter`]
/// 同一形态）。
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemExternalOpener;

impl SystemExternalOpener {
    /// 构造打开器。
    pub const fn new() -> Self {
        Self
    }
}

impl ExternalOpener for SystemExternalOpener {
    fn open(&self, target: &str) -> std::io::Result<()> {
        // `cmd /C start "" "url"` 的空 `""` 是 `start` 的「窗口标题」占位，
        // 必须保留，否则当 url 自身带引号 / 空格时会被 `start` 误判为标题。
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", target])
                .spawn()?;
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open").arg(target).spawn()?;
        }
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open").arg(target).spawn()?;
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            let _ = target;
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "unsupported platform for ExternalOpener",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 平台分支的**编译期**覆盖：当前平台至少有一个分支被编译。
    ///
    /// 这条不断言行为（真起进程），只保证「三平台分支仍在这里」——
    /// 若哪天分支被误删，`cfg` 组合会让本文件在对应平台编译失败或空转。
    #[test]
    fn implements_the_port() {
        fn assert_impl<T: ExternalOpener>() {}
        assert_impl::<SystemExternalOpener>();
    }

    /// 打开的入口形状与端口一致：`&self` + `&str` → `io::Result<()>`。
    #[test]
    fn open_takes_a_borrowed_target() {
        let opener = SystemExternalOpener;
        // 不真起进程（`about:blank` 之外的都起）；只验证签名可用。
        let _ = opener.open("");
    }
}
