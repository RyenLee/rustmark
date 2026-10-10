//! 应用入口。
//!
//! 当前为阶段 02 的骨架状态：窗口尚未接线（S6-01），入口仅转调组合根。

fn main() -> anyhow::Result<()> {
    // 必须是 `main` 的第一句：晚一行就会漏掉那一部分的启动耗时。
    // 未设 `RUSTMARK_STARTUP_TIMING=1` 时该调用是空操作（见模块文档）。
    rustmark::app::startup_timing::mark_process_start();
    rustmark::app::run()
}
