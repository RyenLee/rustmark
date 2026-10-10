//! 架构边界校验器（人工 / CI 执行）。
//!
//! ```bash
//! cargo run --bin check-arch   # 通过打印 OK 并退出 0；违规打印文件行号并退出 1
//! ```
//!
//! 与 `build.rs` 共用 `tools/arch_check.rs` 的扫描逻辑（避免两份实现漂移）。

use std::path::PathBuf;
use std::process::ExitCode;

mod arch_check {
    include!("../../tools/arch_check.rs");
}

fn main() -> ExitCode {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let violations = arch_check::scan(&src_root);

    if violations.is_empty() {
        println!("check-arch: OK（未发现分层依赖违规）");
        return ExitCode::SUCCESS;
    }

    eprintln!("架构边界校验失败，共 {} 处违规：", violations.len());
    for v in &violations {
        eprintln!("  {}:{} —— {}", v.file, v.line, v.detail);
    }
    eprintln!("\n规则见 .agent/architecture.md");
    ExitCode::FAILURE
}
