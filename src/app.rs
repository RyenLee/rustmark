//! 组合根（Composition Root）：唯一允许依赖全部层的位置。
//!
//! - [`bootstrap`]：依赖装配
//! - [`crash_log`]：崩溃报告写盘（S9-08，无遥测，用户手动提交）
//! - [`startup_diagnosis`]：启动期致命失败的可操作诊断（P1 #3，GPU 初始化失败）
//! - [`dialog_runner`]：原生对话框的后台执行（S6-11f）
//! - [`frame`]：eframe 调度与启动期骨架

pub mod bootstrap;
pub mod crash_log;
pub mod dialog_runner;
pub mod frame;
pub mod startup_diagnosis;
pub mod startup_timing;

use bootstrap::{ConfigPaths, Services};

/// 启动应用。
///
/// # 错误为什么用 `anyhow`而不是 `eframe::Error`
///
/// 装配失败有三类原因（配置损坏、语法集缺失、窗口创建失败），
/// 它们的处置方式完全不同：前两者应打印可读提示让用户改配置，
/// 后者需要把 `eframe` / `wgpu` 的技术性错误翻成用户能动手的诊断
/// （见 [`startup_diagnosis`]）。此处统一用 `anyhow` 向上传递，
/// 由 [`main`] 决定怎么呈现，避免在这里替调用方决定。
///
/// # 窗口初始化失败为什么额外落盘
///
/// `eframe::run_native` 的失败是**普通 `Err`，不是 panic**（依据与源码位置见
/// [`startup_diagnosis`] 的模块文档），所以 `crash_log` 的 panic hook 收不到它。
/// 而 Windows release 没有控制台（`src/main.rs` 的 `windows_subsystem`），
/// 只打 stderr 等于用户看到「双击没反应」。故这里在返回错误前先写一份诊断文件。
///
/// S8-03 将在此接入启动优化（延迟加载、首次启动引导）。
pub fn run() -> anyhow::Result<()> {
    // panic hook 必须在**一切逻辑之前**安装（含装配）：
    // 装配期 panic（配置目录不可写等）同样要留下崩溃报告，
    // 晚一行就漏掉那一部分的诊断信息。
    let crash_dir = ConfigPaths::detect().crash_dir();
    crash_log::install_hook(crash_dir.clone());
    // 装配先于窗口创建：装配失败时**不该**先闪一个空窗口再报错。
    // 代价是启动多等几十毫秒（语法集加载），换来「启动失败 = 什么都没发生」，
    // 这个取舍在S8-03 会用后台线程细化。
    //
    // 注意这里能失败**只有读不到配置文件**（权限/路径异常）；
    // 配置内容损坏由 `TomlPreferencesRepository` 按 ADR-003 备份后回退默认，
    // 不会走到这条错误路径——用户不該因为一个语法错误就打不开编辑器。
    let services = Services::bootstrap().map_err(|e| {
        anyhow::anyhow!(
            "启动失败：无法读取应用配置。\n\
             默认位置：Windows %APPDATA%\\rustmark\\preferences.toml，\
             macOS ~/Library/Application Support/rustmark/preferences.toml，\
             Linux ~/.config/rustmark/preferences.toml。\n\
             请确认该文件可读（若损坏，删掉它会用默认配置重建，原文件会留一份 .bak）。\n\
             原始错误：{e}"
        )
    })?;
    startup_timing::mark("装配完成");

    // P3：预热 syntect per-language lazy 层，与 eframe 建窗口（实测 ~682ms，
    // 占冷启动 99.3%）并发跑。此时装配已完成（worker 已 spawn、语法集第①层
    // 已加载），预热 ②③ 约 0.1–0.3s，几乎必然在窗口就绪前完成。
    // 独立后台线程：不占 worker 命令队列、不干扰 parse_count/in_flight 统计、
    // 不碰 core/domain trait；spawn 失败只降级为不预热（无回归，只是没优化到）。
    match std::thread::Builder::new()
        .name("rustmark-prewarm".to_string())
        .spawn(|| {
            let started = std::time::Instant::now();
            crate::infrastructure::highlighting::syntect_highlighter::prewarm_highlighting();
            startup_timing::mark("预热完成");
            log::debug!("语法高亮预热完成：{:?}", started.elapsed());
        }) {
        Ok(_) => {}
        Err(e) => log::warn!("语法高亮预热线程启动失败，已跳过（不影响功能）：{e}"),
    }

    frame::run(services).map_err(|e| {
        // 窗口 / 渲染器初始化失败（P1 #3）：这是**普通 `Err`，不是 panic**，
        // panic hook 收不到，必须在这里把 eframe 的技术性错误翻成可操作诊断
        // 并自己落盘。分类与文案是纯函数（见 `startup_diagnosis`），
        // 故「Linux 无 Vulkan 驱动时该看到什么」有单测钉住。
        let raw = e.to_string();
        let diagnosis =
            startup_diagnosis::diagnose(&raw, startup_diagnosis::HostPlatform::current());
        // 落盘先于返回：Windows release 无控制台，文件是唯一可见通道；
        // 写不进去也不能覆盖原始诊断，只补一条日志。
        match crash_log::write_startup_failure(&crash_dir, &diagnosis) {
            Ok(path) => log::error!("启动失败，诊断已写入 {}", path.display()),
            Err(write_err) => log::error!("启动失败，诊断落盘亦失败（{write_err}）"),
        }
        anyhow::anyhow!("{diagnosis}")
    })?;
    Ok(())
}
