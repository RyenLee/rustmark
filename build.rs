//! 构建期架构边界校验（S2-07）。
//!
//! 构建脚本每次运行都**无条件全量扫描** `src/` 与 `tests/` 的 `use` 语句，
//! 违反分层依赖即 `panic!` 使构建失败。
//!
//! `tests/` 自 R-02 起纳入扫描：测试是**消费者**不是层，可以引用任何层的
//! 公开 API（`rustmark::…`），但禁止以 `crate::<层名>::…` 的库内部视角书写
//! （`crate::` 在集成测试里指向测试 crate 自身）。
//!
//! 此处的演进记录：初版曾在 `build.rs` 内部再加一层「路径 + mtime 指纹」缓存，
//! 实测会导致注入的违规被**静默跳过**——cargo 判定脚本无需重跑、且脚本自身缓存命中，
//! 两层条件任一漏判都会放行。扫描 100+ 文件仅耗时毫秒级，缓存收益远小于漏检风险，
//! 故移除内部缓存；是否需要重跑交由 cargo 依据 `cargo:rerun-if-changed=src` 判断。
//!
//! 逃生口：`SKIP_ARCH_CHECK=1` 可跳过（**CI 不得设置**）。
//! 人工执行：`cargo run --bin check-arch`（打印文件与行号）。

use std::path::PathBuf;

mod arch_check {
    include!("tools/arch_check.rs");
}

fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=tests");

    if std::env::var("SKIP_ARCH_CHECK").is_ok() {
        return;
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_root = manifest.join("src");
    let tests_root = manifest.join("tests");
    let violations = arch_check::scan_with_tests(&src_root, &tests_root);

    if !violations.is_empty() {
        // 报告文本与 `check-arch` bin 共用 `format_report`——两处各写一遍必然漂移。
        let mut report = arch_check::format_report(&violations);
        report.push_str("人工执行 `cargo run --bin check-arch` 可复现同一份报告。\n");
        panic!("{report}");
    }
}
