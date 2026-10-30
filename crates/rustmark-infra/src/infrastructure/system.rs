//! 系统默认程序打开器的实现（R-03）。
//!
//! # 为什么实现在这里而不在 `shared`
//!
//! `shared` 是零 IO 的共享内核，`std::process` 自 R-03 起进
//! [`SHARED_FORBIDDEN`]（见 `docs/spec/architecture-constraints.md`）。端口
//! （[`ExternalOpener`] trait）与目标分类（[`classify_open_target`]）留在
//! `rustmark-ports`——前者因为表现层需要它而 `presentation` 禁依赖
//! `crate::infrastructure`，后者因为它是**纯逻辑**。
//! 真正起进程的三平台分支搬来此处。
//!
//! # 绝不经 shell（C-1）
//!
//! 旧实现是 `cmd /C start "" <target>`：`Command::args` 在 Windows 上对
//! 不含空格的参数**不加引号**，而 `cmd.exe` 会把整行再解析一遍。于是文档里
//! 的 `[x](https://a.com&calc.exe)` 里的 `&` 成了 cmd 的命令分隔符，
//! 后半截被当作第二条命令执行（CWE-78，RCE）。另外 `is_safe_to_open` 旧
//! 口径把「一切不含 `://` 的串」当成本地文件，`[x](C:\x\evil.exe)` 会被
//! `start` 直接执行。
//!
//! 现在两处都换掉了：
//! - **动作**按 [`classify_open_target`] 分派（远程 / 本地文档 / 仅定位 / 拒绝）；
//! - **启动**在 Windows 走 `rundll32.exe url.dll,FileProtocolHandler`，
//!   参数经 argv 直达，`&`、`|`、`^`、`%VAR%` 全部失去语法含义；
//! - **本地可执行文件永不执行**，只让文件管理器定位。
//!
//! # `spawn` 而非 `wait`
//!
//! 打开链接不应阻塞 UI 帧：`wait` 会一直等到用户关闭浏览器。
//!
//! # 失败语义
//!
//! 只返回 `Err`，不 panic——打开器不可用（或平台不支持）不应拖垮编辑器。

use rustmark_ports::system::{ExternalOpener, OpenTarget, classify_open_target};

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
        match classify_open_target(target) {
            // 锚点由表现层处理（`sync.request_click`）；走到这里视为 no-op。
            OpenTarget::Anchor => Ok(()),
            OpenTarget::Reject => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "拒绝打开：该链接目标属于不安全类型（协议 / UNC / 可执行文件）",
            )),
            // 远程 URL 与本地可读文档：交系统协议处理器 / 关联程序打开。
            OpenTarget::Remote | OpenTarget::LocalDocument => launch(target),
            // 本地 exe / 脚本 / html / pdf：只定位，不执行。
            OpenTarget::RevealOnly => reveal(target),
        }
    }
}

/// 启动 URL / 可打开文档。**任何平台分支都不得经过 shell**。
fn launch(target: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        launch_command(target).spawn()?;
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

/// 只在文件管理器中定位，不执行目标。
fn reveal(target: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        // 注意：explorer.exe 即使成功也常返回非 0；这里只 spawn，不看退出码。
        let path = local_path_of(target);
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{path}"))
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(local_path_of(target))
            .spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        // 没有统一的「定位」协议：退化为打开父目录。
        let path = local_path_of(target);
        let parent = std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        std::process::Command::new("xdg-open").arg(parent).spawn()?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = target;
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "unsupported platform for reveal",
        ));
    }
    Ok(())
}

/// Windows 启动命令的纯构造（与 [`launch`] 分离，便于单测断言参数）。
///
/// `rundll32.exe url.dll,FileProtocolHandler <target>` 是系统自带的协议 /
/// 关联程序处理器：参数经 argv 直达，不经过 `cmd.exe`。第一个参数是
/// `url.dll,FileProtocolHandler`（rundll32 的「dll,入口点」固定形态），
/// 目标整体作为单个 argv 传给入口点。
#[cfg(target_os = "windows")]
pub fn launch_command(target: &str) -> std::process::Command {
    let mut command = std::process::Command::new("rundll32.exe");
    command.args(["url.dll,FileProtocolHandler", target]);
    command
}

/// `file:///C:/x` → `C:/x`（去掉 `file://` 后残留的前导 `/` 与盘符错位）。
fn local_path_of(target: &str) -> String {
    let p = target.strip_prefix("file://").unwrap_or(target);
    match p.strip_prefix('/') {
        Some(rest) if rest.len() > 1 && rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        _ => p.to_string(),
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

    /// 不安全 / 空目标在分类阶段就被拒绝，**不起任何进程**。
    #[test]
    fn open_rejects_unsafe_targets_without_spawning() {
        let opener = SystemExternalOpener;
        for target in ["", "javascript:alert(1)", "//host/share", "\\\\host\\share"] {
            let error = opener.open(target).expect_err("应拒绝打开");
            assert_eq!(
                error.kind(),
                std::io::ErrorKind::InvalidInput,
                "拒绝 {target:?} 时错误类型不符"
            );
        }
    }

    /// 锚点走到打开器是 no-op：不报错，也不惊动系统。
    #[test]
    fn anchor_is_a_no_op() {
        assert!(SystemExternalOpener.open("#section").is_ok());
    }

    /// `file://` 本地路径的规范化（`file:///C:/x` → `C:/x`）。
    #[test]
    fn local_path_strips_file_scheme_and_leading_slash() {
        assert_eq!(local_path_of("file:///C:/x.png"), "C:/x.png");
        assert_eq!(local_path_of("file:///D:/a/b.pdf"), "D:/a/b.pdf");
        // 非盘符形态（unix 绝对路径）只剥 `file://`。
        assert_eq!(local_path_of("file:///tmp/x.pdf"), "/tmp/x.pdf");
        // 相对路径原样返回。
        assert_eq!(local_path_of("./evil.exe"), "./evil.exe");
    }

    /// Windows 启动命令的形状：**必须**是 `rundll32.exe` + `url.dll,FileProtocolHandler`，
    /// 且参数里不得出现 `cmd` / `/C`。
    ///
    /// 只构造命令、不 `spawn`——`&calc.exe` 这类 payload 若真的起进程，
    /// 在 CI 上会弹计算器。
    #[cfg(target_os = "windows")]
    #[test]
    fn launch_never_goes_through_cmd() {
        let command = super::launch_command("https://a.com&calc.exe");
        assert_eq!(command.get_program().to_string_lossy(), "rundll32.exe");
        let args: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "url.dll,FileProtocolHandler".to_string(),
                "https://a.com&calc.exe".to_string()
            ]
        );
        assert!(
            !args
                .iter()
                .any(|a| a.eq_ignore_ascii_case("cmd") || a == "/C"),
            "启动命令不得经过 cmd: {args:?}"
        );
    }
}
