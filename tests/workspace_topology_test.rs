//! workspace 结构守卫（V11-57 建立）：契约 crate 的**依赖面**断言。
//!
//! 与 CI 的拓扑 diff（`docs/workspace-topology.expected.txt`）是一对：
//! 那条在远端兜底「任何依赖边变更必须是显式评审事件」，本文件在本地门禁里
//! 钉住单个 crate 的依赖面规格——改 `Cargo.toml` 的人先撞到的是这里的红，
//! 而不是推到远端才红。
//!
//! 判据用 **manifest 文本断言**而非 spawn `cargo tree`：后者要在测试里跑
//! cargo（慢、锁 target 目录、依赖解析状态），而「依赖面规格」本质是一句
//! 可静态判定的事实——没有依赖段就是零依赖。

// V11-65：workspace lints 为包级声明，覆盖本测试 crate。规范 §七-20 允许
// 测试代码 unwrap/expect/panic，同 arch_violation_fixture.rs 的惯例在此豁免。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

/// `rustmark-actions` 必须零依赖（V11-57 验收第 1 条的机械化）。
///
/// 契约 crate 一旦获得任何依赖（normal / dev / build / target-specific），
/// 「零依赖词汇表」的定位即失效——而这类变更在 `Cargo.toml` 的 diff 里
/// 只是一行，评审极易顺手带过。本条把验收标准里的 `cargo tree -p
/// rustmark-actions` 零依赖变成本地门禁：要加依赖，先来改这条断言——
/// 那是显式评审，不是静默漂移。
#[test]
fn actions_crate_declares_no_dependencies() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates")
        .join("rustmark-actions")
        .join("Cargo.toml");
    let text = std::fs::read_to_string(&path).expect("actions crate 的 Cargo.toml 应可读");
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('#') {
            continue; // 注释里谈「依赖」不构成依赖声明
        }
        assert!(
            !line.contains("dependencies"),
            "rustmark-actions 是零依赖契约 crate，Cargo.toml 不得出现任何依赖段\
             （normal / dev / build / target-specific 均不可）：{line}"
        );
    }
}
