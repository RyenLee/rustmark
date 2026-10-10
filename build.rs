//! 构建期架构边界校验（S2-07）。
//!
//! 构建脚本每次运行都**无条件全量扫描** `src/` 的 `use` 语句，违反分层依赖即
//! `panic!` 使构建失败。
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

    if std::env::var("SKIP_ARCH_CHECK").is_ok() {
        return;
    }

    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let violations = arch_check::scan(&src_root);

    if !violations.is_empty() {
        let mut report = String::from("\n架构边界校验失败（分层依赖违规）：\n");
        for v in &violations {
            report.push_str(&format!("  {}:{} —— {}\n", v.file, v.line, v.detail));
        }
        report
            .push_str("\n规则见 .agent/architecture.md；人工执行 `cargo run --bin check-arch`。\n");
        panic!("{report}");
    }
}
