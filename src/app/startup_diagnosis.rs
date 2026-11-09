//! 启动期致命失败的可操作诊断（架构评审 2026-11-01 · P1 #3）。
//!
//! # 要解决的问题
//!
//! Linux 目标只启用 wgpu 的 `vulkan` 后端（`Cargo.toml` 的
//! `cfg(not(any(windows, macos)))` 段，为控制发行体积而放弃了 gles 回退）。
//! 代价是**本机没有 Vulkan 驱动 / loader** 时窗口无法创建——老核显、精简容器、
//! 云桌面、部分虚拟机模板都会遇到。
//!
//! 这类机器上用户看到的是这样一句话（`eframe::Error` 的 `Display`）：
//!
//! ```text
//! WGPU error: No suitable graphics adapter found; noop support not compiled in,
//! vulkan drivers/libraries could not be loaded, metal support not compiled in, …
//! ```
//!
//! 它对排障者有用（"drivers/libraries could not be loaded" 直指驱动），
//! 但对用户**不可操作**：既没说「装什么包」，也没说「怎么自检」。
//!
//! # 失败路径的实测依据（不猜）
//!
//! 结论：**不是 panic**，是 `eframe::run_native` 返回 `Err`。三步链路：
//!
//! 1. `wgpu::Instance::new` 只在「当前目标平台一个后端 feature 都没编进来」时
//!    panic（wgpu-30.0.1 `src/api/instance.rs`），那是 DEF-03 的场景，已由按平台
//!    feature 修掉；**驱动缺失不走这条**——`wgpu-core` 只是 `log::debug!` 记一句
//!    backend 初始化失败，然后把它排除在 `active_backends` 之外
//!    （wgpu-core-30.0.1 `src/instance.rs` 的 `try_add_hal`）。
//! 2. `request_adapter` 于是返回 `Err(RequestAdapterError::NotFound { … })`，
//!    `Display` 由 wgpu-types-30.0.1 `src/adapter.rs` 逐个后端解释原因，其中
//!    「编译进来了、也被请求了、但实例没建起来」的措辞是
//!    `drivers/libraries could not be loaded`。
//! 3. egui-wgpu 把它包成 `WgpuError::RequestAdapterError`，eframe 再包成
//!    `eframe::Error::Wgpu`（`egui-wgpu-0.37.0/src/lib.rs`、`eframe-0.37.0/src/lib.rs`），
//!    经事件循环的 `return_result` 从 `run_native` 返回
//!    （`eframe-0.37.0/src/native/run.rs` 的 `handle_event_result`）。
//!
//! 因此：**panic hook 收不到这类失败**（没有 panic），`crash_log` 的 hook 帮不上忙，
//! 必须在 `app::run()` 里显式接住 `Err` 并自己落盘——这也是为什么本模块与
//! [`super::crash_log`] 并存而不是合并。
//!
//! # 为什么这些函数是纯函数
//!
//! [`classify`] 与 [`diagnose`] 都不碰 IO，平台以 [`HostPlatform`] 参数传入
//! 而不是内部 `cfg!(target_os)`。理由是本项目的短板：**开发机是 Windows、
//! 无 Linux 真机**。参数化之后，「Linux 该看到什么文案」可以在 Windows 上
//! 被单测钉住，剩下的只是「真机跑一遍确认文案确实出现在屏幕上」。

/// 启动失败的归因类别。
///
/// 分类粒度刻意只分三档：诊断文案要给出**动作**，而「装驱动 / 换会话 /
/// 看 DISPLAY」这三类动作是互斥的；再细分只会让文案更长而更少被读完。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupFailure {
    /// 一个可用的图形适配器都拿不到——P1 #3 的目标场景（Linux 无 Vulkan 驱动）。
    NoGpuAdapter,
    /// 适配器拿得到，但 wgpu 初始化（设备创建 / 表面格式）失败。
    GpuInit,
    /// 与图形栈无关的失败（窗口系统、事件循环等）。
    Other,
}

/// 宿主平台。
///
/// 做成枚举而不是在文案里直接 `cfg!(target_os = …)`，是为了让**所有平台**的
/// 文案都能在任意宿主上被单测覆盖（见模块文档「为什么这些函数是纯函数」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostPlatform {
    /// Windows：wgpu 只启用 `dx12`。
    Windows,
    /// macOS：wgpu 只启用 `metal`。
    MacOs,
    /// Linux 及其它类 Unix：wgpu 只启用 `vulkan`。
    Linux,
    /// 其余平台（暂无官方产物，文案走通用分支）。
    Other,
}

impl HostPlatform {
    /// 当前编译目标的平台。纯 `cfg!` 判定，无运行期查询。
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }

    /// 平台名（`StartupFailure` 报告里的「平台」字段已由 `crash_log` 填写，
    /// 这里只用于文案里的自称，如「在 Linux 上」）。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::MacOs => "macOS",
            Self::Linux => "Linux",
            Self::Other => "本平台",
        }
    }
}

/// `eframe::Error` 的 `Display` 里 wgpu 分支的固定前缀
/// （`eframe-0.37.0/src/lib.rs`：`Self::Wgpu(err) => write!(f, "WGPU error: {err}")`）。
///
/// 前缀是 eframe 自己写的字面量，比 wgpu 内部错误文案稳定得多，
/// 故「是不是图形栈的问题」用前缀判，「是不是一个适配器都没有」再用细则判。
const WGPU_ERROR_PREFIX: &str = "WGPU error:";

/// 「一个适配器都拿不到」的判据。
///
/// - `No suitable graphics adapter found`：wgpu `RequestAdapterError::NotFound`
///   的 `Display` 首句（wgpu-types-30.0.1/src/adapter.rs），也是本项目
///   「Linux 无 Vulkan 驱动」的实测措辞。
/// - `Adapter selection failed`：egui-wgpu 的自定义选择器分支
///   （`WgpuError::CustomNativeAdapterSelectionError`）。本项目未设置
///   `native_adapter_selector`，留作防止将来接入时分类退化。
///
/// 升级 wgpu / egui-wgpu 时若文案变了，[`classify`] 会退回 [`StartupFailure::GpuInit`]
/// 分支（仍有驱动更新建议），不会退回「与图形无关」——这条降级路径是刻意设计的。
const NO_ADAPTER_MARKERS: &[&str] = &[
    "No suitable graphics adapter found",
    "Adapter selection failed",
];

/// 判定失败类别（纯函数：同样输入永远同样输出，无 IO、无 cfg）。
pub fn classify(error_display: &str) -> StartupFailure {
    if NO_ADAPTER_MARKERS
        .iter()
        .any(|marker| error_display.contains(marker))
    {
        StartupFailure::NoGpuAdapter
    } else if error_display.contains(WGPU_ERROR_PREFIX) {
        StartupFailure::GpuInit
    } else {
        StartupFailure::Other
    }
}

/// 生成给用户看的完整诊断文本（纯函数）。
///
/// 输出是**多行纯文本**：直接进 stderr，也直接进 `crash/` 下的诊断文件
/// （Windows release 无控制台，文件是唯一可见通道）。最后一段永远带
/// `原始错误`，让用户提交 Issue 时不必再复述。
pub fn diagnose(error_display: &str, platform: HostPlatform) -> String {
    let failure = classify(error_display);
    let headline = match failure {
        StartupFailure::NoGpuAdapter => "rustmark 启动失败：找不到可用的图形适配器（GPU）",
        StartupFailure::GpuInit => "rustmark 启动失败：GPU 初始化失败",
        StartupFailure::Other => "rustmark 启动失败",
    };
    let advice = advice_for(failure, platform);
    format!(
        "{headline}\n\
         \n\
         {advice}\n\
         \n\
         原始错误（提交 Issue 时请附上）：\n\
         {error_display}\n\
         \n\
         本诊断同时写入数据目录下的 crash/：\n\
         默认是应用目录下的 data/crash/；若应用目录不可写（例如装在\n\
         Program Files、macOS .app 或 Linux AppImage），则回退到平台配置目录\n\
         （Windows %APPDATA%\\rustmark\\crash\\，\
         macOS ~/Library/Application Support/rustmark/crash/，\
         Linux ~/.config/rustmark/crash/）。\n\
         报告只含版本 / 平台 / 本条错误，不含任何文档内容。\n"
    )
}

/// 各「失败类别 × 平台」的处置建议（纯函数，单独拆出便于逐组合断言）。
fn advice_for(failure: StartupFailure, platform: HostPlatform) -> &'static str {
    match (failure, platform) {
        (StartupFailure::NoGpuAdapter, HostPlatform::Linux) => LINUX_NO_ADAPTER,
        (StartupFailure::NoGpuAdapter, HostPlatform::Windows) => WINDOWS_NO_ADAPTER,
        (StartupFailure::NoGpuAdapter, HostPlatform::MacOs) => MACOS_NO_ADAPTER,
        (StartupFailure::NoGpuAdapter, HostPlatform::Other) => GENERIC_NO_ADAPTER,
        (StartupFailure::GpuInit, _) => GPU_INIT_FAILED,
        (StartupFailure::Other, HostPlatform::Linux) => LINUX_NO_DISPLAY,
        (StartupFailure::Other, _) => OTHER_FAILURE,
    }
}

/// Linux 无 Vulkan 驱动（P1 #3 的主场景）。
const LINUX_NO_ADAPTER: &str = "\
rustmark 在 Linux 上只使用 **Vulkan** 后端：为了控制发行体积，OpenGL / GLES
回退没有编入（见 Cargo.toml 中 wgpu 的按平台声明）。因此本机必须能加载
Vulkan loader 与驱动，否则窗口建不出来。

按顺序自检：

  1) 装 Vulkan loader 与驱动（按发行版选一条）：
       Debian / Ubuntu：sudo apt install mesa-vulkan-drivers libvulkan1 vulkan-tools
       Fedora / RHEL  ：sudo dnf install mesa-vulkan-drivers vulkan-loader vulkan-tools
       Arch           ：sudo pacman -S vulkan-icd-loader vulkan-tools
                        驱动按显卡另装 vulkan-intel / vulkan-radeon / vulkan-nvidia

  2) 确认能枚举到设备（有 GPU0 一段即通过）：
       vulkaninfo --summary
     若报 \"Cannot create Vulkan instance\"，说明 loader 或驱动仍未就位。

  3) 虚拟机 / 云桌面 / WSL2：需在宿主侧开启 3D 加速并安装 guest Vulkan 驱动；
     RDP 与部分 VNC 协议不提供 Vulkan 设备，请在本机桌面会话下运行。

  4) 只想确认「后端 feature 有没有编进去」：cargo test --test startup_smoke
     注意该守卫只证明编译配置正确，**不**证明本机驱动可用。";

/// Windows 无 dx12 适配器。
const WINDOWS_NO_ADAPTER: &str = "\
rustmark 在 Windows 上只使用 Direct3D 12（dx12）后端，需要显卡驱动支持 D3D12。

按顺序自检：

  1) 更新显卡驱动：优先到厂商官网下载（Intel / AMD / NVIDIA），
     Windows Update 分发的驱动往往落后一两个大版本。
  2) 确认系统版本：D3D12 需要 Windows 10（1903 及以上）或 Windows 11。
  3) 虚拟机（Hyper-V / VMware / VirtualBox）：开启 3D 加速并安装增强工具；
     未开启时通常只有 WARP 软件适配器，wgpu 默认不会选它。
  4) 远程桌面（RDP）会话可能拿不到独立显卡适配器，请在本机会话下运行。";

/// macOS 无 Metal 适配器。
const MACOS_NO_ADAPTER: &str = "\
rustmark 在 macOS 上只使用 Metal 后端。

按顺序自检：

  1) 需要 macOS 10.13 及以上，且显卡支持 Metal（2012 年后的机型基本都支持）。
  2) 虚拟机需开启 Metal / 3D 加速（Parallels、VMware Fusion 等）。
  3) 非标准显卡 / 黑苹果：用「系统信息 → 图形卡」确认 Metal 支持为「支持」。";

/// 平台未知时的通用建议。
const GENERIC_NO_ADAPTER: &str = "\
rustmark 通过 wgpu 渲染，本平台没有取得可用的图形适配器。

处置：确认已安装厂商显卡驱动、有可用的图形会话，并更新到最新驱动后重试。";

/// 有适配器但初始化失败（设备创建 / 表面格式）。
const GPU_INIT_FAILED: &str = "\
适配器找得到，但 wgpu 初始化失败（设备创建 / 表面配置）。常见成因是驱动版本过旧，
或远程会话的合成器限制了可用的表面格式。

处置：
  1) 把显卡驱动更新到厂商最新版；
  2) 若在远程桌面 / 虚拟机 / 容器里，改到本机桌面会话运行；
  3) 仍有问题请附下面的原始错误提交 Issue。";

/// Linux 上「与图形无关」的失败：GUI 程序被塞进了没有图形会话的环境。
const LINUX_NO_DISPLAY: &str = "\
这不是驱动问题，而是**没有图形会话**。常见于 ssh、cron、systemd 服务里直接启动
GUI 程序：`DISPLAY` 与 `WAYLAND_DISPLAY` 都没设置时，窗口系统无法初始化。

处置：
  1) 在带桌面会话的终端里运行 rustmark；
  2) 远程使用时用 ssh -X / -Y 转发，或改用 VNC 等提供图形会话的方式；
  3) 需要更多线索：以 RUST_LOG=debug 重跑并查看 stderr。";

/// 其余平台的通用失败建议。
const OTHER_FAILURE: &str = "\
启动失败与图形栈无关，多为窗口系统 / 事件循环问题。

处置：在带图形会话的终端里运行；需要更多线索时以 RUST_LOG=debug 重跑，
并把 stderr 输出与下面的原始错误一起提交 Issue。";

#[cfg(test)]
mod tests {
    //! 本模块的单测是**唯一**能在 Windows 开发机上验证 Linux 文案的手段，
    //! 故按「分类判定」与「逐平台文案」两组钉住，不追求穷举。

    use super::*;

    /// 实测会出现的那句话（wgpu-types-30.0.1 的 `NotFound` Display 首句 +
    /// 我们这台 Linux 上 vulkan 的解释项）。
    const LINUX_REAL_WORLD: &str = "WGPU error: No suitable graphics adapter found; \
         noop support not compiled in, vulkan drivers/libraries could not be loaded, \
         metal not requested, dx12 support not compiled in, gl not requested";

    #[test]
    fn adapter_missing_is_classified_as_no_adapter() {
        assert_eq!(
            classify(LINUX_REAL_WORLD),
            StartupFailure::NoGpuAdapter,
            "Linux 无 Vulkan 驱动的实测文案必须落到 NoGpuAdapter 分支"
        );
    }

    #[test]
    fn other_wgpu_errors_fall_back_to_gpu_init() {
        // 文案变了（wgpu 升级）也不许退回「与图形无关」。
        assert_eq!(
            classify("WGPU error: There was no valid format for the surface at all."),
            StartupFailure::GpuInit
        );
    }

    #[test]
    fn non_gpu_errors_are_other() {
        assert_eq!(
            classify("winit EventLoopError: neither WAYLAND_DISPLAY nor DISPLAY is set"),
            StartupFailure::Other
        );
        assert_eq!(classify(""), StartupFailure::Other);
    }

    /// `diagnose` 的三段式骨架：标题 → 建议 → 原始错误。缺任何一段，
    /// 用户提交 Issue 时就少了线索。
    #[test]
    fn diagnosis_carries_headline_advice_and_raw_error() {
        let text = diagnose(LINUX_REAL_WORLD, HostPlatform::Linux);
        assert!(text.starts_with("rustmark 启动失败"), "缺标题：{text}");
        assert!(text.contains("原始错误"), "缺原始错误段：{text}");
        assert!(
            text.contains(LINUX_REAL_WORLD),
            "原始错误必须逐字保留（不转述）：{text}"
        );
        assert!(text.contains("crash/"), "必须告诉用户诊断落在哪里：{text}");
    }

    /// P1 #3 的核心断言：Linux 文案必须给出**可执行**的两条——装哪个包、
    /// 用 `vulkaninfo` 自检。换掉任何一条都算回归。
    #[test]
    fn linux_advice_names_the_package_and_the_self_check() {
        let text = diagnose(LINUX_REAL_WORLD, HostPlatform::Linux);
        assert!(
            text.contains("mesa-vulkan-drivers"),
            "Linux 建议必须点名要装的包：{text}"
        );
        assert!(
            text.contains("vulkaninfo"),
            "Linux 建议必须给出自检命令：{text}"
        );
        assert!(
            text.contains("libvulkan1"),
            "Linux 建议必须点名 loader 包：{text}"
        );
    }

    /// Windows 文案不与 Linux 混淆：不出现 apt / vulkaninfo，给出驱动更新路径。
    #[test]
    fn windows_advice_is_about_drivers_not_packages() {
        let text = diagnose(
            "WGPU error: No suitable graphics adapter found; vulkan not requested, \
             dx12 found no adapters",
            HostPlatform::Windows,
        );
        assert!(
            text.contains("驱动") && text.contains("Direct3D 12"),
            "Windows 建议应指向显卡驱动与 dx12：{text}"
        );
        assert!(
            !text.contains("mesa-vulkan-drivers") && !text.contains("vulkaninfo"),
            "Windows 不应出现 Linux 的包名与命令：{text}"
        );
    }

    /// macOS 段落存在且指向 Metal（防止将来 match 漏臂退回通用文案）。
    #[test]
    fn macos_advice_points_at_metal() {
        let text = diagnose(
            "WGPU error: No suitable graphics adapter found; metal found no adapters",
            HostPlatform::MacOs,
        );
        assert!(text.contains("Metal"), "macOS 建议必须指向 Metal：{text}");
    }

    /// Linux 上的非 GPU 失败要指向 DISPLAY，而不是让用户去装 Vulkan 驱动。
    #[test]
    fn linux_display_failure_does_not_blame_the_driver() {
        let text = diagnose(
            "winit EventLoopError: neither WAYLAND_DISPLAY nor DISPLAY is set",
            HostPlatform::Linux,
        );
        assert!(text.contains("DISPLAY"), "应指向 DISPLAY：{text}");
        assert!(
            !text.contains("mesa-vulkan-drivers"),
            "没有图形会话时不该让用户装驱动：{text}"
        );
    }

    /// 平台判定与编译目标一致（本机是 Windows，断言 Windows 分支）。
    #[test]
    fn current_platform_matches_target() {
        let expected = if cfg!(target_os = "windows") {
            HostPlatform::Windows
        } else if cfg!(target_os = "macos") {
            HostPlatform::MacOs
        } else if cfg!(target_os = "linux") {
            HostPlatform::Linux
        } else {
            HostPlatform::Other
        };
        assert_eq!(HostPlatform::current(), expected);
    }
}
