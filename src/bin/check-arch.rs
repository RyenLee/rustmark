//! 架构边界校验器（人工 / CI 执行）。
//!
//! ```bash
//! cargo run --bin check-arch   # 通过打印 OK 并退出 0；违规打印文件行号并退出 1
//! ```
//!
//! 与 `build.rs` 共用 `tools/arch_check.rs` 的扫描逻辑（避免两份实现漂移）——
//! 报告文本也走同一份 `format_report`，两处各写一遍格式必然漂移。
//!
//! 扫描范围自 R-02 起含 `tests/`；自 D-3 阶段 4a 起为整个 workspace
//! （shell `src/` + `crates/rustmark-core/src/` + `tests/`，`scan_project`）：
//! 测试可引用任何层的公开 API，但禁止 `crate::<层名>::…` 的库内部视角。
//!
//! # 注意：先看 `build.rs`
//!
//! `cargo run --bin check-arch` 会先编译，而 `build.rs` 在编译期就做同一遍校验
//! 并在违规时 `panic!`——所以**违规时你看到的往往是 build.rs 的 panic**，
//! 不是这个 bin 的输出。两者内容一致（同一份 `format_report`），
//! 差别只在 build.rs 额外给出「人工执行 `cargo run --bin check-arch`」的指引。
//!
//! 想只看 bin 的输出：设 `SKIP_ARCH_CHECK=1` 跳过构建期校验。

use std::path::PathBuf;
use std::process::ExitCode;

mod arch_check {
    include!("../../tools/arch_check.rs");
}

fn main() -> ExitCode {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let violations = arch_check::scan_project(&manifest);

    if violations.is_empty() {
        println!("check-arch: OK（未发现分层依赖违规）");
        return ExitCode::SUCCESS;
    }

    eprint!("{}", arch_check::format_report(&violations));
    ExitCode::FAILURE
}
