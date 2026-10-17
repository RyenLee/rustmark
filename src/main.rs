//! 应用入口。
//!
//! 当前为阶段 02 的骨架状态：窗口尚未接线（S6-01），入口仅转调组合根。

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
