//! 应用入口。
//!
//! 当前为阶段 02 的骨架状态：窗口尚未接线（S6-01），入口仅转调组合根。

// Windows 图形子系统：release 构建不创建控制台窗口（用户反馈：双击 exe
// 附带黑色终端）。debug 构建保留 console——`RUST_LOG` 的日志输出、启动
// 耗时打印都要经它，开发期丢不得。其余平台（macOS/Linux）GUI 程序不经
// console 子系统启动，无此现象，该属性在非 Windows 下是纯 no-op。
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

fn main() -> anyhow::Result<()> {
    // 必须是 `main` 的第一句：晚一行就会漏掉那一部分的启动耗时。
    // 未设 `RUSTMARK_STARTUP_TIMING=1` 时该调用是空操作（见模块文档）。
    rustmark::app::startup_timing::mark_process_start();
    // F-2：注册日志后端。此前 `log` 的 12 个告警调用点（锁中毒恢复、
    // 未知语言降级、跳过符号链接等）无后端接收，全部静默丢弃。
    // 默认级别 warn：`RUST_LOG` 未设时告警与错误可见，排障时
    // `RUST_LOG=debug` 开全量。初始化失败（理论上只会因重复注册）不阻断
    // 启动——日志是排障手段，不是功能。
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();
    rustmark::app::run()
}
