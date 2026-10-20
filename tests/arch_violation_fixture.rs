//! 架构边界校验器自身的验证（S2-07 建立，R-01 重写后扩为「形状覆盖」）。
//!
//! # 为什么要这么多用例
//!
//! 校验器本身**不产生编译错误**：它写错一个 `use`、漏一个括号、少认一个宏，
//! 后果都是「该拦的没拦」——而工程照样编译通过、测试照样全绿。
//! 唯一能让这类缺陷现形的办法就是**注入违规**。
//!
//! 所以本文件的核心不是「跑一遍真实工程」，而是**把各种逃逸形状逐个钉死**：
//! 旧实现的漏洞全是「某个具体形状逃过去了」，那就把形状做成用例。
//!
//! # 三类用例
//!
//! 1. **零违规守卫**：真实工程必须干净；
//! 2. **注入必报**：每种逃逸形状都必须被检出；
//! 3. **注入不报**：每种「看起来像违规但其实不是」的情形必须放过。

use std::path::{Path, PathBuf};

mod arch_check {
    include!("../tools/arch_check.rs");
}

fn write_fixture(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// 写入豁免登记表（`docs/architecture-constraints.md` 的登记区段）。
///
/// 登记表是**豁免指令的另一半**：代码里的 `// arch-allow: X` 只在登记表里
/// 出现过的对象上才有效。没登记的豁免会被报「豁免未登记」——
/// 否则「随手写个豁免注释就能放行」等于没有豁免机制。
fn write_registry(root: &Path, rows: &[(&str, &str)]) {
    let manifest = root.parent().unwrap();
    let path = manifest.join("docs").join("architecture-constraints.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut text = String::from("# 架构约束\n\n<!-- arch-allow:registry -->\n");
    text.push_str("| 文件 | 对象 | 理由 |\n|---|---|---|\n");
    for (f, n) in rows {
        text.push_str(&format!("| `{f}` | `{n}` | 测试登记 |\n"));
    }
    text.push_str("<!-- /arch-allow:registry -->\n");
    std::fs::write(path, text).unwrap();
}

fn described(vs: &[arch_check::Violation]) -> Vec<String> {
    vs.iter()
        .map(|v| format!("{}:{} {}", v.file, v.line, v.detail))
        .collect()
}

// ============================================================================
// 一、零违规守卫
// ============================================================================

#[test]
fn current_project_has_no_violations() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let violations = arch_check::scan(&src_root);
    assert!(
        violations.is_empty(),
        "当前工程不应存在分层违规，实际：{:?}",
        described(&violations)
    );
}

/// 现状守卫：真实工程里的 UI State 一个领域文档类型都不引。
///
/// 与 `current_project_has_no_violations` 重复是有意的——那条会因
/// **任何**层的任何违规而红，这条红了能立刻指认是 `state/` 的问题。
#[test]
fn ui_state_modules_import_no_domain_document() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let state_violations: Vec<_> = arch_check::scan(&src_root)
        .into_iter()
        .filter(|v| v.file.starts_with("presentation/state/"))
        .collect();
    assert!(
        state_violations.is_empty(),
        "UI State 只放视图状态，业务数据来自 DTO。实际违规：{:?}",
        described(&state_violations)
    );
}

// ============================================================================
// 一之补、R-02：`tests/` 纳入扫描
// ============================================================================

/// 真实工程连 `tests/` 一起扫也必须干净（R-02 的存量守卫）。
///
/// # 为什么单独一条
///
/// `current_project_has_no_violations` 只扫 `src/`。R-02 把 `tests/` 纳进
/// 扫描范围后，**存量是否干净必须单独守**——否则将来有人在 `tests/` 写下
/// 库内部视角的引用，没有任何一条用例会红。
#[test]
fn tests_directory_has_no_violations() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let violations = arch_check::scan_with_tests(&manifest.join("src"), &manifest.join("tests"));
    assert!(
        violations.is_empty(),
        "连 tests/ 一起扫也不应有违规，实际：{:?}",
        described(&violations)
    );
}

/// **R-02 的点名判据**：`tests/` 里注入 `use crate::domain::x::Y;`
/// → `scan_with_tests` 必须报，而 `scan` 不报。
///
/// # 为什么两个都要断言
///
/// 只断言「`scan_with_tests` 报」不够：那可能是 `scan` 自己就误报了
/// （比如把 `tests/` 也当 src 扫了）。`scan` 不报这条断言钉住「两个入口
/// 的范围差异」正是 R-02 引入的那个参数，而不是别处行为变化。
#[test]
fn injected_crate_layer_path_in_tests_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = dir.path().join("src");
    let tests_root = dir.path().join("tests");
    write_fixture(&src_root, "lib.rs", "//! x\n");
    write_fixture(
        &tests_root,
        "domain/main.rs",
        "//! x\nuse crate::domain::x::Y;\n",
    );

    let src_only = arch_check::scan(&src_root);
    assert!(
        src_only.is_empty(),
        "scan 只扫 src/，不应看到 tests/ 的违规，实际：{:?}",
        described(&src_only)
    );

    let with_tests = arch_check::scan_with_tests(&src_root, &tests_root);
    assert_eq!(
        with_tests.len(),
        1,
        "scan_with_tests 应检出 tests/ 里那一处，实际：{:?}",
        described(&with_tests)
    );
    let v = &with_tests[0];
    assert!(v.file.starts_with("tests/"), "文件应带 tests/ 前缀：{v:?}");
    assert!(v.file.ends_with("domain/main.rs"), "文件路径不对：{v:?}");
    assert_eq!(v.line, 2, "应报告具体行号：{v:?}");
    assert!(
        v.detail.contains("crate::domain"),
        "文案应点名禁用的前缀：{v:?}"
    );
    // 文案要能指认「这是库内部视角」——否则读者不知道正确写法是什么。
    assert!(
        v.detail.contains("rustmark::"),
        "文案应给出正确写法（公开 API rustmark::…）：{v:?}"
    );
}

/// 测试**自身**的共享模块必须放行（`tests/support/` 的组织方式）。
///
/// # 为什么这条是关键护栏
///
/// `TESTS_FORBIDDEN` 只列库层名（`crate::domain` / `crate::app` / …），
/// 天然放行 `crate::support` / `crate::golden_fixture` 这类**测试自己**的
/// 模块。若哪天有人「图省事」把名单改成禁整个 `crate::`，
/// 全仓 243 处 `crate::support::…` 会同时误报——这条用例当场红。
#[test]
fn tests_may_reference_their_own_support_modules() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = dir.path().join("src");
    let tests_root = dir.path().join("tests");
    write_fixture(&src_root, "lib.rs", "//! x\n");
    write_fixture(
        &tests_root,
        "app/save_all_test.rs",
        "//! x\nuse crate::support::fake_dialog::FakeDialog;\n\
         use crate::golden_fixture::load_or_generate_large;\n",
    );

    let violations = arch_check::scan_with_tests(&src_root, &tests_root);
    assert!(
        violations.is_empty(),
        "测试自身的 support / fixture 模块不是库内部视角，应放行。实际：{:?}",
        described(&violations)
    );
}

/// `tests/` 下**每个**库层名都要拦（不是只拦 `domain`）。
///
/// 名单漏一项的后果是「只有 `domain` 被守，其它五个库层名在测试侧
/// 畅通无阻」——而这种放宽不产生编译错误、也没人会注意到。
#[test]
fn every_library_layer_name_is_forbidden_in_tests() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = dir.path().join("src");
    let tests_root = dir.path().join("tests");
    write_fixture(&src_root, "lib.rs", "//! x\n");
    let banned = [
        "crate::app",
        "crate::domain",
        "crate::application",
        "crate::infrastructure",
        "crate::shared",
        "crate::presentation",
    ];
    for (i, prefix) in banned.iter().enumerate() {
        write_fixture(
            &tests_root,
            &format!("app/case_{i}.rs"),
            &format!("//! x\nuse {prefix}::x::Y;\n"),
        );
    }

    let violations = arch_check::scan_with_tests(&src_root, &tests_root);
    assert_eq!(
        violations.len(),
        banned.len(),
        "{} 个库层名应全部报出，实际：{:?}",
        banned.len(),
        described(&violations)
    );
    for (prefix, v) in banned.iter().zip(&violations) {
        assert!(v.detail.contains(prefix), "应报 {prefix}，实际报的是 {v:?}");
    }
}

/// 表现层的「更窄豁免前缀」**不**适用于 `tests/` 侧。
///
/// # 为什么单独一条
///
/// `allowed_for` 给表现层放了 `crate::app::frame`（54 个文件依赖它，
/// 见 `tools/arch_check.rs` 的说明）。若哪天有人把 `Layer::Tests` 并进
/// 那个 `match` 分支（「测试也要用 EditorApp 嘛」），`crate::app::frame`
/// 在测试侧就会被放行——而那正是「测试绕进组合根私事」的口子。
/// 变异验证钉住这一条：把 `Layer::Tests` 并进豁免分支 → 本用例红。
#[test]
fn the_narrow_allow_prefix_does_not_apply_to_tests() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = dir.path().join("src");
    let tests_root = dir.path().join("tests");
    write_fixture(&src_root, "lib.rs", "//! x\n");
    write_fixture(
        &tests_root,
        "app/frame_test.rs",
        "//! x\nuse crate::app::frame::EditorApp;\n",
    );

    let violations = arch_check::scan_with_tests(&src_root, &tests_root);
    assert_eq!(
        violations.len(),
        1,
        "crate::app::frame 的豁免只属于表现层，测试侧不放行。实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("crate::app"),
        "{:?}",
        violations[0]
    );
}

/// `tests/` 下**非 `use` 位置**的限定路径也要拦。///
/// 词法级判定的既有能力（R-01 重写后 `fn f(d: crate::domain::x::Y)`
/// 在 `src/` 里会被报）必须对测试侧同样成立——否则测试侧就是
/// 「只在 `use` 那一行守」的半套规则。
#[test]
fn qualified_path_outside_use_is_detected_in_tests() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = dir.path().join("src");
    let tests_root = dir.path().join("tests");
    write_fixture(&src_root, "lib.rs", "//! x\n");
    write_fixture(
        &tests_root,
        "app/qualified_test.rs",
        "//! x\nfn f(d: crate::domain::x::Y) {}\n",
    );

    let violations = arch_check::scan_with_tests(&src_root, &tests_root);
    assert_eq!(
        violations.len(),
        1,
        "非 use 位置的限定路径也应被检出，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("crate::domain"),
        "{:?}",
        violations[0]
    );
}

/// `tests/` 下的字符串字面量与注释里的 `crate::domain` **不是**违规。
///
/// # 为什么必须单独一条
///
/// `tests/arch_violation_fixture.rs` 自己就有 41 处 `crate::…`，
/// 全部在**字符串字面量**里（本文件注入用的测试数据）。词法阶段剔除
/// 字符串/注释是 R-01 的能力，但「tests/ 纳入扫描」让这条能力第一次
/// 在真实工程里被大规模检验——fixture 文件自身就是最大的那块试金石。
/// 若这条红了，说明词法对测试侧失效，全仓测试会同时挂。
#[test]
fn crate_layer_paths_inside_strings_and_comments_are_not_violations_in_tests() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = dir.path().join("src");
    let tests_root = dir.path().join("tests");
    write_fixture(&src_root, "lib.rs", "//! x\n");
    write_fixture(
        &tests_root,
        "app/literals_test.rs",
        "//! 文档注释里提一句 crate::domain 不算引用\n\
         fn f() {\n    \
         let s = \"use crate::domain::x::Y;\";\n    \
         let t = \"// use crate::app::bootstrap::X\";\n\
         }\n",
    );

    let violations = arch_check::scan_with_tests(&src_root, &tests_root);
    assert!(
        violations.is_empty(),
        "字符串与注释里的路径不是依赖，应放行。实际：{:?}",
        described(&violations)
    );
}

/// `tests/` 下的帧预约规则**依然生效**（T11 / ADR-009 不因测试而豁免）。
///
/// # 为什么测试也要守
///
/// 帧预约唯一出口是与层无关的通用规则（记忆里「对**所有**文件生效，
/// 含组合根」）。测试里直接写 `request_repaint_after` 一样会造出
/// 「看起来在测、其实绕过 scheduler」的假绿。实测 `tests/` 下该标识符
/// 只出现在 fixture 的字符串字面量里，故存量干净。
#[test]
fn repaint_rule_still_applies_inside_tests() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = dir.path().join("src");
    let tests_root = dir.path().join("tests");
    write_fixture(&src_root, "lib.rs", "//! x\n");
    write_fixture(
        &tests_root,
        "app/frame_test.rs",
        "//! x\nfn f(ctx: &egui::Context) {\n    \
         ctx.request_repaint_after(std::time::Duration::from_millis(10));\n}\n",
    );

    let violations = arch_check::scan_with_tests(&src_root, &tests_root);
    assert_eq!(
        violations.len(),
        1,
        "tests/ 下的直接预约同样应被拦，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("request_repaint_after"),
        "{:?}",
        violations[0]
    );
}

// ============================================================================
// 一之补、R-03：`shared` 禁 `std::process`
// ============================================================================

/// `shared` 里出现 `std::process` 必报（R-03 的新禁令）。
///
/// # 为什么这条禁令存在
///
/// `shared` 自称「零外部依赖」，但 `system.rs` 曾在此用 `Command` 起进程，
/// 文件头自认「`std::process` 不在禁止清单，故在此落位」——那是**绕规则
/// 落位**：规则说这里不该有 IO，就改规则迁就代码。R-03 把执行端移到
/// `infrastructure::system`，此处补禁令防回退。
///
/// 端口（trait）与纯逻辑校验可以留在 `shared`——它们不产生 IO，
/// 这条禁令只拦**进程调用**本身。
#[test]
fn std_process_is_forbidden_in_shared() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "shared/system.rs",
        "//! x\nuse std::process::Command;\nfn open(t: &str) {\n    \
         Command::new(\"cmd\").arg(t).spawn().ok();\n}\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "shared 里的 std::process 应被拦，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("std::process"),
        "{:?}",
        violations[0]
    );
}

/// 端口 trait 与纯校验留在 `shared` **不**触发禁令。
///
/// # 为什么必须单独一条
///
/// 若禁令写成「`shared` 不许出现 `process` 字样」这类宽口径，
/// `ExternalOpener` trait 的文档注释（解释为什么拆开）会被误伤；
/// 更糟的是写成禁 `Command` 的话，`infrastructure` 侧也会被拖累。
/// 这条钉住「只拦 `std::process` 路径，不拦端口定义」。
#[test]
fn the_port_trait_may_live_in_shared() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "shared/system.rs",
        "//! 端口留在 shared，实现移到 infrastructure。\n\
         pub trait ExternalOpener: Send + Sync {\n    \
         fn open(&self, target: &str) -> std::io::Result<()>;\n}\n\
         pub fn is_safe_to_open(url: &str) -> bool { !url.is_empty() }\n",
    );

    let violations = arch_check::scan(&root);
    assert!(
        violations.is_empty(),
        "端口 trait 与纯校验不是 IO，应放行。实际：{:?}",
        described(&violations)
    );
}

/// **R-03 点名的变异验证**：把预览区的调用改回直连 `shared::system::open_external`
/// → 该自由函数若留在 `shared` 就会带 `std::process`，被新禁令拦下。
///
/// # 这条测的是什么
///
/// 方案原文的变异判据是「改回直连 → `cargo check` 报 shared 不得依赖
/// `std::process`」。这里用 fixture 复现同一形状：`shared` 里既留端口
/// **又**留一个用 `Command` 的自由函数封装 → 必报。
/// 它守的是「不许把实现搬回 shared」这条底线，而不是某个具体符号名。
#[test]
fn a_free_function_wrapper_in_shared_is_still_banned() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "shared/system.rs",
        "//! x\npub trait ExternalOpener: Send + Sync {\n    \
         fn open(&self, t: &str) -> std::io::Result<()>;\n}\n\
         /// 薄封装：留个Compat 出口。\n\
         pub fn open_external(t: &str) -> std::io::Result<()> {\n    \
         std::process::Command::new(\"cmd\").arg(t).spawn()\n}\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "即使包成薄封装，shared 里的进程调用仍应被拦。实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("std::process"),
        "{:?}",
        violations[0]
    );
}

/// `infrastructure` 里的 `std::process` **不**受影响。
///
/// R-03 把实现搬到 `infrastructure::system`，那里本来就要起进程。
/// 若禁令误扩到 infrastructure（比如有人把 `std::process` 加进
/// `INFRASTRUCTURE_FORBIDDEN`），这条当场红。
#[test]
fn std_process_is_allowed_in_infrastructure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "infrastructure/system.rs",
        "//! x\npub struct SystemExternalOpener;\nimpl SystemExternalOpener {\n    \
         pub fn open(t: &str) -> std::io::Result<()> {\n        \
         std::process::Command::new(\"cmd\").arg(t).spawn()\n    }\n}\n",
    );

    let violations = arch_check::scan(&root);
    assert!(
        violations.is_empty(),
        "infrastructure 是执行端的归宿，应放行。实际：{:?}",
        described(&violations)
    );
}

// ============================================================================
// 二、注入必报：use 声明的各种逃逸形状
// ============================================================================

#[test]
fn injected_domain_violation_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(&root, "domain/document/aggregate.rs", "//! x\nuse egui;\n");

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "应检出 1 处违规，实际：{}",
        violations.len()
    );
    assert!(violations[0].detail.contains("domain 层禁止依赖 egui"));
    assert_eq!(violations[0].line, 2, "应报告具体行号");
    assert!(violations[0].file.contains("aggregate.rs"));
}

#[test]
fn trailing_comment_after_use_still_detected() {
    // 回归用例：`use egui; // 注释` 曾因解析器先去分号后去注释而漏检。
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nuse egui; // 需要 egui 的类型\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "带行尾注释的 use 同样应被检出，实际：{}",
        violations.len()
    );
}

#[test]
fn injected_presentation_violation_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/main_view.rs",
        "//! x\nuse crate::domain::document::aggregate::Document;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].detail.contains("crate::domain"));
}

#[test]
fn injected_std_fs_in_domain_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/repository.rs",
        "//! x\nuse std::fs;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].detail.contains("std::fs"));
}

// ============================================================================
// 三、注入必报：R-01 修掉的 8 类逃逸形状
// ============================================================================

/// `pub use` 曾整类逃逸——旧实现只认「行首 `use `」。
#[test]
fn pub_use_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\npub use egui::Context;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "pub use 同样必须被检出：{:?}",
        described(&violations)
    );
    assert!(violations[0].detail.contains("egui"));
}

/// `pub(crate) use` 与 `pub use` 同理。
#[test]
fn pub_crate_use_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\npub(crate) use egui::Context;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "pub(crate) use 必须被检出：{:?}",
        described(&violations)
    );
}

/// 嵌套花括号曾整类逃逸——旧实现按整行取首段，`crate::{domain::x::Y}` 落到
/// 「crate」就断了。
#[test]
fn nested_brace_use_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/main_view.rs",
        "//! x\nuse crate::{domain::document::aggregate::Document};\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "嵌套花括号必须被检出：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("crate::domain"),
        "应展开成完整路径：{}",
        violations[0].detail
    );
}

/// 非 `use` 位置的限定路径也是跨层依赖——`fn f(d: crate::domain::x::Y)`。
#[test]
fn qualified_path_in_signature_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/main_view.rs",
        "//! x\nfn f(d: crate::domain::document::aggregate::Document) {}\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "签名里的限定路径必须被检出：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("引用"),
        "来源应标注为「引用」而非「use」：{}",
        violations[0].detail
    );
}

/// 函数体内的 `use` 也是依赖声明。
#[test]
fn use_inside_function_body_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nfn f() { use egui::Context; }\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "函数体内的 use 必须被检出：{:?}",
        described(&violations)
    );
}

/// 别名曾整类逃逸——旧实现取整行首段，`use egui as e;` 里首段是 `egui`
/// 却被 `as` 之后的噪声干扰。
#[test]
fn aliased_use_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nuse egui as e;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "别名 use 必须被检出：{:?}",
        described(&violations)
    );
    assert!(violations[0].detail.contains("egui"));
}

/// 跨行 `use` 曾整类逃逸——旧实现按行处理，多行声明只有第一行被看到。
#[test]
fn multiline_use_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nuse egui::{\n    Context,\n    Frame,\n};\n",
    );

    let violations = arch_check::scan(&root);
    assert!(
        !violations.is_empty(),
        "跨行 use 必须被检出：{:?}",
        described(&violations)
    );
}

/// 层根文件 `src/domain.rs` 曾整类逃逸——旧实现按路径首段判定，
/// 首段是 `"domain.rs"` 而不是 `"domain"`。
#[test]
fn layer_root_file_is_judged_by_its_layer() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(&root, "domain.rs", "//! x\nuse egui;\n");

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "层根文件必须按所属层判定：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.starts_with("domain 层"),
        "应报为 domain 层：{}",
        violations[0].detail
    );
}

/// `src/app/` 整层曾逃逸**所有**规则——包括帧预约唯一出口这条。
///
/// 这是历史上最严重的一条：`layer_of("app/...")` 返回 `""`，而扫描函数
/// 在**所有**检查之前就 `continue` 掉了 → 组合根里随手写
/// `ctx.request_repaint_after(...)` 不会被拦，「定时功能静默失效」的老坑
/// 可以原样复发。
#[test]
fn app_layer_is_not_exempt_from_the_repaint_rule() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "app/frame/files.rs",
        "//! x\nfn f(ctx: &egui::Context) {\n    ctx.request_repaint_after(std::time::Duration::from_millis(10));\n}\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "组合根里的帧预约也必须被拦：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("request_repaint_after"),
        "应报帧预约违规：{}",
        violations[0].detail
    );
    assert_eq!(violations[0].line, 3, "应报告实际行号");
}

/// 帧预约的唯一出口本身当然合法。
#[test]
fn the_repaint_exit_itself_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "app/frame/scheduler.rs",
        "//! x\nfn f(ctx: &egui::Context) {\n    ctx.request_repaint_after(std::time::Duration::from_millis(10));\n}\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "唯一出口处的预约不应被拦"
    );
}

/// `crate::app` 曾完全没有被任何层禁止——组合根可以被下层反向依赖。
#[test]
fn crate_app_is_forbidden_in_the_lower_layers() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nuse crate::app::frame::EditorApp;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "领域层反向依赖组合根必须被拦：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("crate::app"),
        "应报 crate::app：{}",
        violations[0].detail
    );
}

// ============================================================================
// 四、注入必报：打印宏家族
// ============================================================================

#[test]
fn println_in_domain_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/export/aggregate.rs",
        "//! x\nprintln!(\"hi\");\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].detail.contains("println!"));
}

/// `print!`（无 ln）曾漏网——旧实现只查 `println!` 字面量。
#[test]
fn print_without_ln_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/export/aggregate.rs",
        "//! x\nprint!(\"hi\");\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "print! 必须被检出：{:?}",
        described(&violations)
    );
    assert!(violations[0].detail.contains("print!"));
}

/// `eprintln!` 曾被当成 `println!` 报错——文案不准会让人去错地方找。
#[test]
fn eprintln_reports_its_own_macro_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/export/aggregate.rs",
        "//! x\neprintln!(\"hi\");\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(violations.len(), 1);
    assert!(
        violations[0].detail.contains("eprintln!"),
        "文案应带实际宏名：{}",
        violations[0].detail
    );
}

// ============================================================================
// 五、注入不报：注释、字符串里的路径不是依赖
// ============================================================================

/// 行尾注释里的 `println!` 曾被误报。
#[test]
fn println_inside_a_comment_is_not_a_violation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/export/aggregate.rs",
        "//! x\n// 这里本可以用 println!(\"hi\") 打印\nfn f() {}\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "注释里的 println! 不该被报：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// 块注释里的 `use` 曾被误报（且嵌套块注释会把后面的代码整段吞掉）。
#[test]
fn use_inside_a_block_comment_is_not_a_violation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\n/*\nuse egui;\n*/\nfn f() {}\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "块注释里的 use 不该被报：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// 嵌套块注释：内层 `/* */` 不能让外层提前结束。
#[test]
fn nested_block_comment_does_not_leak_code() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\n/* 外层 /* 内层 */ 仍是外层 */\nfn f() {}\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "嵌套块注释之后的代码不该被误判：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// 字符串字面量里的路径不是依赖。
#[test]
fn path_inside_a_string_literal_is_not_a_violation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nconst S: &str = \"use egui;\";\nfn f() {}\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "字符串里的路径不该被报：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// 文档注释里提到 `crate::domain` 不是依赖。
#[test]
fn path_inside_a_doc_comment_is_not_a_violation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/main_view.rs",
        "//! x\n/// 参见 [`crate::domain::document::aggregate::Document`]。\nfn f() {}\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "文档注释里的路径不该被报：{:?}",
        described(&arch_check::scan(&root))
    );
}

// ============================================================================
// 六、路径段前缀：不能误伤同前缀的其它 crate
// ============================================================================

/// `crate::domainfoo` 不是 `crate::domain`——字符串前缀匹配会误伤。
#[test]
fn similar_prefix_crate_is_not_matched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/main_view.rs",
        "//! x\nuse crate::domainfoo::Thing;\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "同前缀但不同段的 crate 不该被误伤：{:?}",
        described(&arch_check::scan(&root))
    );
}

// ============================================================================
// 七、组合根与表现层的边界
// ============================================================================

#[test]
fn app_layer_may_use_ui_and_infra() {
    // 组合根是唯一允许依赖全部层的位置（含 main.rs / lib.rs / app/）
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "app/bootstrap.rs",
        "//! x\nuse egui;\nuse crate::infrastructure::text::rope_text_buffer::RopeTextBuffer;\n",
    );

    assert!(arch_check::scan(&root).is_empty(), "组合根不应被判违规");
}

/// 表现层从 `crate::app::frame` 取状态句柄是**被认可的下行通道**。
///
/// 现实：54 个表现层文件都这么干。规则若一刀切禁掉 `crate::app`，
/// 就会变成「一条没人能遵守的规则」——例外随之蔓延成惯例，
/// 连真正要拦的反向依赖也一起淹掉。
#[test]
fn presentation_may_reach_the_app_frame_handle() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/main_view.rs",
        "//! x\nuse crate::app::frame::EditorApp;\nuse crate::app::frame::PendingDialog;\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "表现层取 app::frame 状态句柄不应被判违规：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// 但表现层**不许**碰组合根的其它部分（启动编排、崩溃日志等）。
#[test]
fn presentation_may_not_reach_the_rest_of_the_composition_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/main_view.rs",
        "//! x\nuse crate::app::bootstrap::run;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "表现层伸手到组合根其它部分必须被拦：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("crate::app"),
        "应报 crate::app：{}",
        violations[0].detail
    );
}

// ============================================================================
// 八、豁免机制的三态
// ============================================================================

/// 豁免必须**登记**——否则「随手写个豁免注释就能放行」等于没有豁免机制。
#[test]
fn arch_allow_must_be_registered() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nuse egui; // arch-allow: egui\n",
    );
    // 故意**不**写登记表。

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "未登记的豁免必须被报：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("豁免未登记"),
        "应报「豁免未登记」：{}",
        violations[0].detail
    );
}

/// 豁免只豁免**写明的那个对象**——旧实现是「整行放行」，
/// 写 `arch-allow: crate::domain` 能把同行的 `use egui;` 一起放行。
#[test]
fn arch_allow_only_covers_the_named_object() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nuse egui; // arch-allow: crate::domain\n",
    );
    write_registry(&root, &[("domain/document/aggregate.rs", "crate::domain")]);

    let violations = arch_check::scan(&root);
    assert!(
        violations
            .iter()
            .any(|v| v.detail.contains("禁止依赖 egui")),
        "豁免写的是 crate::domain，放行不了 egui：{:?}",
        described(&violations)
    );
}

/// 登记齐全的豁免应当完全放行。
#[test]
fn arch_allow_comment_suppresses_single_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nuse egui; // arch-allow: egui（已在 architecture.md 登记）\n",
    );
    write_registry(&root, &[("domain/document/aggregate.rs", "egui")]);

    assert!(
        arch_check::scan(&root).is_empty(),
        "带 arch-allow 注释的行应被豁免：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// 过时豁免必须被报出来——否则豁免会随代码演进不断堆积，
/// 最后没人敢动。
#[test]
fn stale_arch_allow_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nfn f() {} // arch-allow: egui\n",
    );
    write_registry(&root, &[("domain/document/aggregate.rs", "egui")]);

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "没有违规可豁免的豁免必须被报：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("豁免无效"),
        "应报「豁免无效」：{}",
        violations[0].detail
    );
}

/// 豁免理由里提到 `architecture.md` 不能被当成第二个豁免对象。
#[test]
fn arch_allow_reason_text_is_not_a_second_object() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nuse egui; // arch-allow: egui（登记处见 architecture.md）\n",
    );
    write_registry(&root, &[("domain/document/aggregate.rs", "egui")]);

    assert!(
        arch_check::scan(&root).is_empty(),
        "理由里的文件名不该变成豁免对象：{:?}",
        described(&arch_check::scan(&root))
    );
}

// ============================================================================
// 九、扫描本身的性质
// ============================================================================

/// 扫描必须**确定**且**对内容敏感**。
///
/// 旧实现里有 `signature()`（路径 + mtime 指纹）用于跳过重复扫描——
/// 但 build.rs 早已移除内部缓存，那段代码成了死代码，且与
/// 「已移除内部缓存」的文档记载自相矛盾。删掉它之后，
/// 这里守住的是「同一份输入永远得到同一份输出」这条更本质的性质。
#[test]
fn scan_is_deterministic_and_content_sensitive() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(&root, "shared/ids.rs", "//! x\n");

    let first = arch_check::scan(&root);
    let second = arch_check::scan(&root);
    assert_eq!(
        described(&first),
        described(&second),
        "同一份输入应得到同一份输出"
    );

    write_fixture(&root, "shared/ids.rs", "//! x\nuse egui;\n");
    let third = arch_check::scan(&root);
    assert_ne!(
        described(&first),
        described(&third),
        "源文件变更后扫描结果必须变化"
    );
}

/// 报告文本是**单一来源**：`build.rs` 与 `check-arch` bin 共用
/// `format_report`，两处各写一遍格式必然漂移。
#[test]
fn the_report_is_produced_by_a_single_source() {
    let violations = vec![arch_check::Violation {
        file: "domain/document/aggregate.rs".to_string(),
        line: 7,
        detail: "domain 层禁止依赖 egui".to_string(),
    }];
    let report = arch_check::format_report(&violations);
    assert!(report.contains("domain/document/aggregate.rs:7"));
    assert!(report.contains("domain 层禁止依赖 egui"));
    assert!(report.contains("docs/architecture-constraints.md"));
}

// ============================================================================
// 十、S6-12：presentation/state 子层
//
// 词法层是「哪些东西会被判成路径」的唯一真源。它一旦出错，后面所有
// 判定都建立在错误的地基上——而这类错误**不产生编译错误**。
// 所以词法层本身也要有直接的用例。

/// 空白**不产生 token**。
///
/// 这条不是洁癖：`use` 与被导入项之间的空格一旦变成 token，
/// `parse_use_tree` 读到的就是「`use` 后面跟了个非标识符」，
/// 整条 `use` 声明会**静默漏检**。
#[test]
fn whitespace_does_not_become_a_token() {
    let dump = arch_check::lex_dump("use egui;\n");
    assert_eq!(
        dump, "1:Ident(use)\n1:Ident(egui)\n1:Semi()\n",
        "空白不应出现在 token 流里：{dump}"
    );
}

/// 换行必须计入行号——否则所有违规都报「第 1 行」，行号形同虚设。
#[test]
fn newlines_advance_the_line_counter() {
    let dump = arch_check::lex_dump("fn a() {}\nfn b() {}\n");
    assert!(
        dump.contains("2:Ident(b)"),
        "第二行的标识符应报第 2 行：{dump}"
    );
}

/// 注释与字符串里的内容**不产生 token**。
#[test]
fn comments_and_strings_produce_no_tokens() {
    let dump = arch_check::lex_dump(
        "//! 文档\n// 行注释\n/* 块注释 */\nfn f() { let s = \"use egui;\"; }\n",
    );
    assert!(
        !dump.contains("egui"),
        "注释与字符串里的路径不该被词法：{dump}"
    );
    assert!(
        !dump.contains("Ident(use)"),
        "字符串里的 use 不该被词法：{dump}"
    );
}

/// 块注释的嵌套深度正确——内层 `/* */` 不能让外层提前结束。
#[test]
fn nested_block_comments_end_at_the_outer_close() {
    let dump = arch_check::lex_dump("/* a /* b */ c */\nfn f() {}\n");
    assert!(
        dump.contains("1:Ident(fn)") || dump.contains("2:Ident(fn)"),
        "外层块注释之后的代码应被词法：{dump}"
    );
}

/// 豁免指令只在**普通注释**里生效——文档注释里提到标记串不算豁免。
#[test]
fn doc_comments_do_not_produce_allow_directives() {
    let dump = arch_check::lex_dump("/// 参见 arch-allow: egui 的用法\nfn f() {}\n");
    assert!(!dump.contains("allow@"), "文档注释不该产生豁免指令：{dump}");
}

/// 子层判定：`state/` 走自己的层名，父层禁令**叠加**其上。
///
/// # 这里有个必须说清的事实
///
/// 最初想造一个「父层允许、子层禁止」的对照物（试过 `std::fs`，
/// 它在表现层根本合法 → 两边都不报 0 条）。转而想造一个
/// 「只被子层拦下」的路径（`crate::domain::document::x`）——
/// **做不到**：`crate::domain::document` 是 `crate::domain` 的前缀，
/// 凡是匹配它的路径必然也匹配父层。
///
/// 结论：本条子层规则在当前名单下**不新增任何拦截**，
/// 它买到的两样东西是——
/// 1. 报错文案指明「是 UI State 这一层」并附上「业务数据来自 DTO」；
/// 2. 若将来 `PRESENTATION_FORBIDDEN` 被放宽，这条独立规则仍在原地。
///
/// 所以本用例断言的是**归属与叠加**，不是「拦截数比父层多」。
#[test]
fn the_state_subdirectory_is_judged_by_its_own_layer_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    let banned = "use crate::domain::document::aggregate::Document;\n";

    write_fixture(&root, "presentation/state/app_ui_state.rs", banned);
    write_fixture(&root, "presentation/views/editor_view.rs", banned);

    let violations = arch_check::scan(&root);
    let state_hits: Vec<_> = violations
        .iter()
        .filter(|v| v.detail.starts_with("presentation/state 层"))
        .collect();
    // 注意：不能用 `contains("presentation 层")` 来筛父层那条——
    // 那是子层串的**子串**，会把子层一起捞进来。改判「层名后紧跟空格
    // 且不是 state」：这里用 `starts_with` 锚定整条 detail 的开头。
    let view_hits: Vec<_> = violations
        .iter()
        .filter(|v| v.detail.starts_with("presentation 层"))
        .collect();
    assert_eq!(
        state_hits.len(),
        2,
        "`state/` 叠加父层 + 子层，应报 2 条：{:?}",
        described(&violations)
    );
    assert!(
        state_hits
            .iter()
            .all(|v| v.file.contains("app_ui_state.rs")),
        "子层那两条都应指向 state 文件：{:?}",
        described(&violations)
    );
    // 子层那两条里，只有一条带 DTO 说明（父层那条不该带）。
    assert_eq!(
        state_hits
            .iter()
            .filter(|v| v.detail.contains("DTO"))
            .count(),
        1,
        "只有子层自己的禁令才附 DTO 说明：{:?}",
        described(&violations)
    );
    assert_eq!(
        view_hits.len(),
        1,
        "`views/` 仍按父层名报告（不应被子层规则吞掉）：{:?}",
        described(&violations)
    );
    assert!(view_hits[0].file.contains("editor_view.rs"));
}

/// 子层叠加而非替换：父层名单在 `state/` 同样生效。
///
/// 用 `crate::infrastructure`（在父层名单里、不在子层名单里）。
#[test]
fn the_state_subdirectory_still_obeys_the_parent_layer_rules() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/state/app_ui_state.rs",
        "use crate::infrastructure::text::rope_text_buffer::RopeTextBuffer;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(violations.len(), 1, "父层禁令在子层同样生效");
    assert!(
        violations[0].detail.contains("crate::infrastructure"),
        "应由父层规则而非子层规则拦下：{}",
        violations[0].detail
    );
    // 父层那条**不该**带子层的 DTO 说明（那句与基础设施依赖无关）。
    assert!(
        !violations[0].detail.contains("DTO"),
        "父层禁令不应附加子层专属的说明：{}",
        violations[0].detail
    );
}

/// 子层那条违规必须同时报出**两条**：父层的 `crate::domain`
/// 与子层的 `crate::domain::document`。
///
/// 这正是「叠加」的可见证据——若子层写成替换父层，
/// 这里的违规数会从 2 掉到 1。
#[test]
fn a_state_violation_reports_both_the_parent_and_the_sub_layer_rule() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/state/app_ui_state.rs",
        "use crate::domain::document::aggregate::Document;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        2,
        "应同时命中父层与子层两条禁令，实际：{}",
        violations.len()
    );
    let described: Vec<String> = violations.iter().map(|v| v.detail.clone()).collect();
    // 注意两条的 detail **都以 `presentation/state 层` 开头**——
    // `{layer}` 是完整子层名，父层那条也不例外。区分依据是**被禁的路径**：
    // 父层禁 `crate::domain`（精确匹配），子层禁 `crate::domain::document`。
    assert!(
        described
            .iter()
            .any(|d| d.contains("层禁止依赖 crate::domain（use")),
        "应含父层那条（精确禁 `crate::domain`）：{described:?}"
    );
    assert!(
        described
            .iter()
            .any(|d| d.contains("层禁止依赖 crate::domain::document（use")),
        "应含子层那条（精确禁 `crate::domain::document`）：{described:?}"
    );
}

/// 违规信息必须带「为什么」，否则读者会当成随手定的规则去绕。
#[test]
fn the_state_violation_explains_why() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/state/app_ui_state.rs",
        "use crate::domain::document::aggregate::Document;\n",
    );

    let violations = arch_check::scan(&root);
    // 只看子层那一条（另一条是父层的，不该有 DTO 说明）。
    let sub = violations
        .iter()
        .find(|v| v.detail.contains("DTO"))
        .expect("子层违规应带 DTO 说明");
    assert!(
        sub.detail.contains("DTO") && sub.detail.contains("presentation/state"),
        "子层说明应同时指明层名与理由：{}",
        sub.detail
    );
}

// ============================================================================
// 十一、词法层
// ============================================================================

/// 豁免指令要区分「独立成行」与「行尾」——窗口形状取决于这个区分。
///
/// 词法层是唯一能便宜地看见这个区分的地方：扫描级用例只能看「罩没罩住」，
/// 看不出「为什么没罩」。debug 输出里带上了形状标记，窗口问题不用猜。
#[test]
fn allow_directives_record_whether_they_are_own_line() {
    let dump = arch_check::lex_dump("// arch-allow: egui\nuse egui; // arch-allow: egui\n");
    assert!(
        dump.contains("allow@1:egui(own_line)"),
        "独立成行的注释应记 own_line：{dump}"
    );
    assert!(
        dump.contains("allow@2:egui(trailing)"),
        "行尾注释应记 trailing：{dump}"
    );
}

// ============================================================================
// 十二、豁免窗口：行尾豁免只罩本条语句，独立行豁免额外罩下一行
// ============================================================================

/// 行尾豁免曾顺「起点前一行也认」漏到下一行，把一条**独立的**违规压掉。
///
/// 形状：第 1 行行尾写豁免，第 2 行是另一条同类 `use`——它没有任何豁免，
/// 必须照常报。这是「判据无牙齿」的又一变体：豁免写在第 1 行，看着像只
/// 豁免了第 1 行，实际把第 2 行一起吞了。
#[test]
fn a_trailing_allow_does_not_cover_the_next_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "use egui; // arch-allow: egui\nuse egui;\n",
    );
    write_registry(&root, &[("domain/document/aggregate.rs", "egui")]);

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "第 2 行的 use 没有任何豁免，必须照常报：{:?}",
        described(&violations)
    );
    assert_eq!(
        violations[0].line,
        2,
        "应报第 2 行：{:?}",
        described(&violations)
    );
}

/// 同上，但下一行是**非 use 位置**的限定路径——漏抑它同样放掉一个真依赖。
#[test]
fn a_trailing_allow_does_not_cover_the_next_reference() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "use egui; // arch-allow: egui\nfn f(x: egui::Context) {}\n",
    );
    write_registry(&root, &[("domain/document/aggregate.rs", "egui")]);

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "下一行的引用没有任何豁免，必须照常报：{:?}",
        described(&violations)
    );
    assert_eq!(violations[0].line, 2);
    assert!(
        violations[0].detail.contains("引用"),
        "应报为引用：{:?}",
        described(&violations)
    );
}

/// 独立成行的豁免写在 `use` 上面——这是它**该**罩的形状，不能收严收坏。
#[test]
fn an_own_line_allow_covers_the_statement_below() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "// arch-allow: egui\nuse egui;\n",
    );
    write_registry(&root, &[("domain/document/aggregate.rs", "egui")]);

    assert!(
        arch_check::scan(&root).is_empty(),
        "独立成行的豁免应罩住紧邻的 use：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// 独立成行的豁免 + 多行 use：窗口起点是 `use` 所在行，也要罩住。
#[test]
fn an_own_line_allow_covers_a_multiline_use_below() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "// arch-allow: egui\nuse egui::{\n    Context,\n};\n",
    );
    write_registry(&root, &[("domain/document/aggregate.rs", "egui")]);

    assert!(
        arch_check::scan(&root).is_empty(),
        "独立成行的豁免应罩住紧邻的多行 use：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// 豁免写在多行 use 组内（与代码同行）——本条语句照常罩。
#[test]
fn an_allow_inside_a_use_group_covers_that_statement() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "use egui::{ // arch-allow: egui\n    Context,\n};\n",
    );
    write_registry(&root, &[("domain/document/aggregate.rs", "egui")]);

    assert!(
        arch_check::scan(&root).is_empty(),
        "组内的行尾豁免应罩住本条语句：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// 组内的行尾豁免**不越过本条语句**：后面的另一条 use 照常报。
#[test]
fn an_allow_inside_a_use_group_does_not_leak_past_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "use egui::{ // arch-allow: egui\n    Context,\n};\nuse egui::Frame;\n",
    );
    write_registry(&root, &[("domain/document/aggregate.rs", "egui")]);

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "第 4 行的 use 不在豁免窗口里：{:?}",
        described(&violations)
    );
    assert_eq!(violations[0].line, 4);
}

/// 独立行豁免只罩**紧邻**的下一行：隔一条语句不罩，
/// 且那条够不着的豁免自身要报「无效」。
#[test]
fn an_own_line_allow_does_not_skip_a_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "// arch-allow: egui\nfn f() {}\nuse egui;\n",
    );
    write_registry(&root, &[("domain/document/aggregate.rs", "egui")]);

    let violations = arch_check::scan(&root);
    assert!(
        violations
            .iter()
            .any(|v| v.line == 3 && v.detail.contains("禁止依赖 egui")),
        "隔一行的 use 必须照常报：{:?}",
        described(&violations)
    );
    assert!(
        violations
            .iter()
            .any(|v| v.line == 1 && v.detail.contains("豁免无效")),
        "罩不住任何违规的豁免要报无效：{:?}",
        described(&violations)
    );
}

// ============================================================================
// 十三、自由位置是白名单，其它顶层名字是覆盖缺口
// ============================================================================

/// 白名单内的自由位置不报「未知层」：crate 入口与 `src/bin/` 工具。
///
/// 与下几条对照——收严成「默认未知」会误伤这三个约定好的位置。
#[test]
fn the_free_positions_are_a_whitelist() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(&root, "main.rs", "use egui;\n");
    write_fixture(&root, "lib.rs", "use egui;\n");
    write_fixture(&root, "bin/check-arch.rs", "use egui;\n");

    assert!(
        arch_check::scan(&root).is_empty(),
        "crate 入口与 bin 工具是约定好的自由位置：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// 未登记的顶层目录曾**静默**逃过全部层禁令——`layer_of` 的默认分支
/// 曾是 `Free`，任何新目录（含打错的层名）都不产生任何报告。
#[test]
fn an_unknown_top_level_directory_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(&root, "utils/helpers.rs", "use egui;\n");

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "未登记的顶层目录必须报出来：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("未知层"),
        "应报「未知层」：{:?}",
        described(&violations)
    );
}

/// 未登记的顶层文件同理（`src/foo.rs`）。
#[test]
fn an_unknown_top_level_file_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(&root, "foo.rs", "fn f() {}\n");

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "未登记的顶层文件必须报出来：{:?}",
        described(&violations)
    );
    assert!(violations[0].detail.contains("未知层"));
}

/// 未知层的文件**仍然**受帧预约规则约束——那条与层无关。
///
/// 这条钉的是检查顺序：未知层检查若插到帧预约循环**之前**，
/// 这里的预约违规会被漏掉（正是 `app_layer_is_not_exempt_from_the_repaint_rule`
/// 同类错误的另一个入口）。
#[test]
fn an_unknown_file_still_obeys_the_repaint_rule() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "utils/helpers.rs",
        "fn f(ctx: &egui::Context) {\n    ctx.request_repaint_after(std::time::Duration::from_millis(10));\n}\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        2,
        "应同时报未知层与帧预约：{:?}",
        described(&violations)
    );
    assert!(
        violations.iter().any(|v| v.detail.contains("未知层")),
        "应报未知层：{:?}",
        described(&violations)
    );
    assert!(
        violations
            .iter()
            .any(|v| v.detail.contains("request_repaint_after")),
        "帧预约规则与层无关，未知层同样生效：{:?}",
        described(&violations)
    );
}

/// 组合根 / 自由文件里的 `arch-allow` 也曾被**静默忽略**——豁免体检跑在
/// 「无层禁令提前返回」的后面。三态表（未登记 / 无效 / 对象不符）应当对
/// 所有文件成立，包括没有层禁令的位置。
#[test]
fn a_stale_allow_in_a_layer_without_bans_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(&root, "app/bootstrap.rs", "fn f() {} // arch-allow: egui\n");
    write_registry(&root, &[("app/bootstrap.rs", "egui")]);

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "组合根里的无效豁免必须被报：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("豁免无效"),
        "应报「豁免无效」：{:?}",
        described(&violations)
    );
}

// ============================================================================
// 十四、extern crate 与「不设豁免」的规则
// ============================================================================

/// `extern crate egui;`（2015 版式，`#[macro_use]` 场景仍会出现）与 `use`
/// 同等对待——旧实现只认 `use`，这条整类漏检。
#[test]
fn extern_crate_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "extern crate egui;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "extern crate 必须被检出：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("extern crate"),
        "来源应标注为 extern crate：{:?}",
        described(&violations)
    );
}

/// 带别名的 `extern crate egui as e;`——别名不改变依赖对象。
#[test]
fn aliased_extern_crate_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "extern crate egui as e;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "别名 extern crate 必须被检出：{:?}",
        described(&violations)
    );
    assert!(violations[0].detail.contains("egui"));
}

/// `extern "C" { … }` 不是依赖声明：字符串不产生 token，`extern` 后面
/// 跟不到 `crate` 标识符。这条防的是把 `extern` 一律当依赖声明的新误报。
#[test]
fn an_extern_block_is_not_a_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "extern \"C\" {\n    fn foo();\n}\nfn f() {}\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "extern 块不是依赖声明：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// `extern "C" fn` 同理。
#[test]
fn an_extern_fn_is_not_a_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "extern \"C\" fn foo() {}\nfn f() {}\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "extern fn 不是依赖声明：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// 帧预约规则**不设豁免**：给它写豁免，要被指出「这条规则没有豁免」，
/// 而不是报「该行没有依赖它的违规」——后者会让人去挪注释位置，
/// 而正确动作是删掉那处预约。
#[test]
fn the_repaint_rule_has_no_exemption() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "app/frame/files.rs",
        "fn f(ctx: &egui::Context) {\n    ctx.request_repaint_after(std::time::Duration::from_millis(10)); // arch-allow: request_repaint_after\n}\n",
    );
    write_registry(&root, &[("app/frame/files.rs", "request_repaint_after")]);

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        2,
        "应报帧预约违规 + 无效豁免：{:?}",
        described(&violations)
    );
    assert!(
        violations
            .iter()
            .any(|v| v.detail.contains("request_repaint_after") && !v.detail.contains("arch-allow")),
        "帧预约违规照常报：{:?}",
        described(&violations)
    );
    let stale = violations
        .iter()
        .find(|v| v.detail.contains("arch-allow"))
        .expect("应有一条豁免体检结果");
    assert!(
        stale.detail.contains("不设豁免"),
        "文案应指明这条规则没有豁免，而不是说「该行没有违规」：{}",
        stale.detail
    );
}
