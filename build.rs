//! 构建期架构边界校验（S2-07）。
//!
//! 构建脚本每次运行都**无条件全量扫描**整个 workspace 的源码——
//! shell `src/`、`crates/rustmark-core/src/`（D-3 阶段 4a 拆分）、
//! `crates/rustmark-actions/src/`（V11-57，整根判 Free）、
//! `crates/rustmark-icons/src/`（V11-58，`lib.rs` 自然判 Free）、
//! `crates/rustmark-ui/src/`（V11-62，`presentation/` 前缀判子层）、
//! `crates/rustmark-render/src/`（R-04 升 crate，整根判 Free）、
//! `crates/rustmark-shared/src/`（TD-04，`shared/` 前缀判共享内核）、
//! `crates/rustmark-infra/src/`（V11-64，`infrastructure/` 前缀判基础设施层）、
//! `crates/rustmark-ports/src/`（R-10 项 A，整根判 Ports）、
//! `crates/rustmark-assembly/src/`（R-10 项 B，整根判 App）
//! 与 `tests/` 的 `use` 语句，违反分层依赖即 `panic!` 使构建失败。
//!
//! 重跑条件（`cargo:rerun-if-changed`）由 `arch_check::src_root_paths`
//! **派生**，与扫描根同源：早先两份清单分开维护，render / ports / assembly
//! 三个根漏登记，只改它们时本地构建不重跑校验，违规要等测试或 CI 才暴露。
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

// V11-65：`[lints] workspace = true` 是包级声明，覆盖全部目标（含本 build script），
// 而原先 lib.rs 顶部的 attribute 只作用于 lib 目标。本文件的 `panic!` 是 S2-07
// 架构守卫的**失败机制本身**（发现分层违规即使构建失败），属「确有不变式保障
// 的位置」，按规范 §四-15 的逐处裁决惯例在此豁免；`unwrap`/`expect` 本文件零使用，
// 不一并放宽。
#![allow(clippy::panic)]

use std::path::PathBuf;

mod arch_check {
    include!("tools/arch_check.rs");
}

/// 把 `assets/logo/rustmark.ico` 编进 exe 资源段（仅 Windows 目标）。
///
/// 失败不 `panic`：图标是体验项，不该让构建失败。`NotAttempted`（机器上没有
/// `rc.exe`）与 `Failed` 都只打印 `cargo:warning`，构建照常继续——但会显式
/// 说出「这一环没生效」，而不是静默降级成默认图标。
#[cfg(windows)]
fn embed_icon() {
    println!("cargo:rerun-if-changed=assets/logo/rustmark.rc");
    println!("cargo:rerun-if-changed=assets/logo/rustmark.ico");
    match embed_resource::compile("assets/logo/rustmark.rc", embed_resource::NONE) {
        embed_resource::CompilationResult::Ok => {}
        other => {
            println!("cargo:warning=窗口图标未嵌入（{other:?}），exe 将沿用系统默认图标");
        }
    }
}

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Windows exe 的静态图标（资源管理器 / 固定到任务栏看到的那一张）。
    // 运行期 `with_icon` 只管窗口与 alt-tab，二者缺任一环都会露出系统默认图标。
    #[cfg(windows)]
    embed_icon();

    // 重跑清单与扫描根同源：新增或搬迁 crate 只需改 `src_roots` 一处。
    // 两处各列一遍曾让 render / ports / assembly 漏登记，只改它们时
    // 本地构建沿用旧校验结论（违规被静默跳过）。
    for root in arch_check::src_root_paths(&manifest) {
        let rel = root
            .strip_prefix(&manifest)
            .unwrap_or(root.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        println!("cargo:rerun-if-changed={rel}");
    }
    println!("cargo:rerun-if-changed=tests");

    if std::env::var("SKIP_ARCH_CHECK").is_ok() {
        return;
    }

    let violations = arch_check::scan_project(&manifest);

    if !violations.is_empty() {
        // 报告文本与 `check-arch` bin 共用 `format_report`——两处各写一遍必然漂移。
        let mut report = arch_check::format_report(&violations);
        report.push_str("人工执行 `cargo run --bin check-arch` 可复现同一份报告。\n");
        panic!("{report}");
    }
}
