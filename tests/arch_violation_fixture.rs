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

// V11-65：workspace lints 为包级声明，覆盖本测试 crate（原先仅 lib 目标
// 受 lib.rs attribute 管辖）。规范 §七-20 允许测试代码 unwrap/expect/
// panic，同 lib.rs 的 `cfg_attr(test, allow)` 惯例在此豁免。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::path::{Path, PathBuf};

mod arch_check {
    include!("../tools/arch_check.rs");
}

fn write_fixture(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// 写入豁免登记表（`docs/spec/architecture-constraints.md` 的登记区段）。
///
/// 登记表是**豁免指令的另一半**：代码里的 `// arch-allow: X` 只在登记表里
/// 出现过的对象上才有效。没登记的豁免会被报「豁免未登记」——
/// 否则「随手写个豁免注释就能放行」等于没有豁免机制。
fn write_registry(root: &Path, rows: &[(&str, &str)]) {
    let manifest = root.parent().unwrap();
    write_registry_at(manifest, rows)
}

/// 写入 **manifest 级**豁免登记表。
///
/// D-3 阶段 4a 起登记表按 manifest 加载一次、三个根共用（core 根的
/// parent 是 `crates/rustmark-core`，那里没有 `docs/`），故需要一个
/// 直接以 manifest 为基准的写入口；`write_registry` 保持旧口径
/// （传 src 根，parent 即 manifest），既有用例不动。
fn write_registry_at(manifest: &Path, rows: &[(&str, &str)]) {
    let path = manifest
        .join("docs")
        .join("spec")
        .join("architecture-constraints.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut text = String::from("# 架构约束\n\n<!-- arch-allow:registry -->\n");
    text.push_str("| 文件 | 对象 | 理由 |\n|---|---|---|\n");
    for (f, n) in rows {
        text.push_str(&format!("| `{f}` | `{n}` | 测试登记 |\n"));
    }
    text.push_str("<!-- /arch-allow:registry -->\n");
    std::fs::write(path, text).unwrap();
}

/// 写入 **shared 准入登记表**（`architecture-constraints.md` §4.5 三问，D-6）。
///
/// 与豁免表同一文件、不同区段标记（`<!-- shared:registry -->`）。
/// 两者互不覆盖是标记对的职责；本 helper 只写 shared 区段，
/// 既有用例需要豁免表时仍走 `write_registry_at`。
fn write_shared_registry(manifest: &Path, files: &[&str]) {
    let path = manifest
        .join("docs")
        .join("spec")
        .join("architecture-constraints.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut text = String::from("# 架构约束\n\n<!-- shared:registry -->\n");
    text.push_str("| 文件 | 三问理由 |\n|---|---|\n");
    for f in files {
        text.push_str(&format!("| `{f}` | 三问已答（测试登记） |\n"));
    }
    text.push_str("<!-- /shared:registry -->\n");
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
    // D-3 阶段 4a 起口径为整个 workspace：shell src + core src + tests。
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let violations = arch_check::scan_project(&manifest);
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
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // V11-62 批次 2 起 `state/` 住在 ui 根下（报告带 crates/rustmark-ui/src/
    // 前缀；此前在 core 根下），故按**包含**匹配层内路径，而不是前缀匹配。
    let state_violations: Vec<_> = arch_check::scan_project(&manifest)
        .into_iter()
        .filter(|v| v.file.contains("presentation/state/"))
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
/// `current_project_has_no_violations` 自 D-3 阶段 4a 起扫整个 workspace，
/// 但**全工程零违规**的断言红了还得自己找是谁的问题。这条把报告过滤到
/// `tests/` 前缀：将来有人在 `tests/` 写下库内部视角的引用，
/// 这条用例红了直接指认测试侧。
#[test]
fn tests_directory_has_no_violations() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let violations: Vec<_> = arch_check::scan_project(&manifest)
        .into_iter()
        .filter(|v| v.file.starts_with("tests/"))
        .collect();
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

/// 注入 `src/**/mod.rs` 与 `tests/**/mod.rs` 必须各报一条；`xxx.rs + xxx/` 放行。
///
/// # 为什么这条要单独钉
///
/// `mod.rs` 与「`xxx.rs` + `xxx/`」在 Rust 里**语义等价**，编译器对两者
/// 一视同仁——写错布局既不报错、也不影响测试。规则若只写在约束文档里，
/// 下一次重构就会悄悄回退（2026-10-22 复核时 `fonts/` 与 `tests/support/`
/// 两处存量正是这样活下来的）。
///
/// R-02（2026-10-27）把 `rustmark-shared` 二分到 `shared/kernel/` 与
/// `shared/support/`，正是「`xxx.rs` + `xxx/`」布局的**多层嵌套**形态：
/// 拆分时会顺手写成 `kernel/mod.rs` / `support/mod.rs`，编译器同样不报。
/// 故此处除顶层 `shared/mod.rs` 外，另钉两个新子目录的 `mod.rs` 必须被捕获。
#[test]
fn mod_rs_is_rejected_in_src_and_tests() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = dir.path().join("src");
    let tests_root = dir.path().join("tests");
    write_fixture(&src_root, "lib.rs", "//! x\n");
    // 合法形态：层根文件 `shared.rs` + 同名目录里的子模块。
    write_fixture(&src_root, "shared.rs", "//! x\npub mod gateway;\n");
    write_fixture(&src_root, "shared/gateway.rs", "//! x\n");
    // R-02 的多层嵌套合法形态：`kernel.rs` + `kernel/`、`support.rs` + `support/`。
    write_fixture(&src_root, "shared/kernel.rs", "//! x\npub mod dialog;\n");
    write_fixture(&src_root, "shared/kernel/dialog.rs", "//! x\n");
    write_fixture(&src_root, "shared/support.rs", "//! x\npub mod text;\n");
    write_fixture(&src_root, "shared/support/text.rs", "//! x\n");
    // 违规形态：src 顶层、tests 侧，以及 R-02 两个新子目录各一处。
    write_fixture(&src_root, "shared/mod.rs", "//! x\n");
    write_fixture(&src_root, "shared/kernel/mod.rs", "//! x\n");
    write_fixture(&src_root, "shared/support/mod.rs", "//! x\n");
    write_fixture(&tests_root, "support/mod.rs", "//! x\n");

    let violations = arch_check::scan_with_tests(&src_root, &tests_root);
    let mod_hits: Vec<_> = violations
        .iter()
        .filter(|v| v.file.ends_with("mod.rs"))
        .collect();
    assert_eq!(
        mod_hits.len(),
        4,
        "src 顶层 + R-02 两个新子目录 + tests 侧各报一条，实际：{:?}",
        described(&violations)
    );
    for expected in [
        "shared/mod.rs",
        "shared/kernel/mod.rs",
        "shared/support/mod.rs",
        "tests/support/mod.rs",
    ] {
        assert!(
            mod_hits.iter().any(|v| v.file == expected),
            "应报 {expected}：{mod_hits:?}"
        );
    }
    for v in &mod_hits {
        assert_eq!(v.line, 1, "文件布局违规报首行：{v:?}");
        assert!(
            v.detail.contains("CONSTR-03") && v.detail.contains("mod.rs"),
            "文案应点名规则与禁用文件名：{v:?}"
        );
    }
    // 「`xxx.rs` + `xxx/`」是合法布局（含 R-02 的两层嵌套），不得误报。
    for legal in [
        "shared.rs",
        "shared/gateway.rs",
        "shared/kernel.rs",
        "shared/kernel/dialog.rs",
        "shared/support.rs",
        "shared/support/text.rs",
    ] {
        assert!(
            !violations.iter().any(|v| v.file == legal),
            "合法布局 {legal} 不应报错，实际：{:?}",
            described(&violations)
        );
    }
}

/// R-03 B2：ui 新增子模块必须同步 root 逐项 re-export + §10.1 crate 地图。
///
/// # 为什么单独钉
///
/// 整组 `pub use rustmark_ui::presentation::{…}` 在 Rust 里与逐项**语义等价**，
/// 但「新增 ui 子模块」会被静默带入 root：编译不报、测试不红，而 root 清单与
/// §10.1 已经失配（同 `mod.rs` 禁令的失效模式——规则在文档里，不在门禁里）。
/// 本用例注入「ui 有 `delta`、root/§10 没有」的合成树，钉住两处各报一条；
/// 补齐后全绿；最后钉整组形式本身违规。
#[test]
fn ui_submodule_must_be_synced_to_root_and_crate_map() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();

    // ui 侧：rich（既有）+ delta（新增）。
    write_fixture(
        manifest,
        "crates/rustmark-ui/src/presentation.rs",
        "//! x\npub mod rich;\npub mod delta;\n",
    );
    // root 侧：只显式 re-export 了 rich，漏了 delta。
    write_fixture(
        manifest,
        "src/presentation.rs",
        "//! x\npub use rustmark_ui::presentation::rich;\n",
    );
    // §10.1 只登记了 rich。
    write_fixture(
        manifest,
        "docs/spec/architecture-constraints.md",
        "### 10.1 crate 地图\n\n| `rustmark-ui` | widgets / rich |\n\n### 10.2 判据\n",
    );

    let violations = arch_check::check_ui_submodule_sync(manifest);
    let details: Vec<&str> = violations.iter().map(|v| v.detail.as_str()).collect();
    assert_eq!(
        violations.len(),
        2,
        "应报 2 条（root 缺逐项 re-export + §10.1 缺登记），实际：{details:?}"
    );
    assert!(
        details
            .iter()
            .any(|d| d.contains("`delta`") && d.contains("未在 root 显式")),
        "应报 delta 未在 root 显式 re-export：{details:?}"
    );
    assert!(
        details
            .iter()
            .any(|d| d.contains("`delta`") && d.contains("§10.1")),
        "应报 delta 未登记 §10.1：{details:?}"
    );

    // 补齐两处 → 全绿。
    write_fixture(
        manifest,
        "src/presentation.rs",
        "//! x\npub use rustmark_ui::presentation::rich;\npub use rustmark_ui::presentation::delta;\n",
    );
    write_fixture(
        manifest,
        "docs/spec/architecture-constraints.md",
        "### 10.1 crate 地图\n\n| `rustmark-ui` | widgets / rich / delta |\n\n### 10.2 判据\n",
    );
    let after = arch_check::check_ui_submodule_sync(manifest);
    assert!(
        after.is_empty(),
        "补齐 root + §10.1 后应全绿，实际：{:?}",
        described(&after)
    );

    // 整组 `::{…}` 形式本身违规（新增子模块会被静默带入）。
    write_fixture(
        manifest,
        "src/presentation.rs",
        "//! x\npub use rustmark_ui::presentation::{rich, delta};\n",
    );
    let violations = arch_check::check_ui_submodule_sync(manifest);
    let details: Vec<&str> = violations.iter().map(|v| v.detail.as_str()).collect();
    assert_eq!(
        violations.len(),
        3,
        "整组形式 = 1 条整组禁令 + 2 条缺逐项，实际：{details:?}"
    );
    assert!(
        details.iter().any(|d| d.contains("禁止整组")),
        "应报整组转发违规：{details:?}"
    );
}

/// R-03 B1：视图不得直取组合根 `crate::app`；迁移白名单**只减不增**。
///
/// # 为什么两个方向都要钉
///
/// 只钉「新引用要报」的话，白名单会变成**只进不出**的列表：每迁完一个
/// 视图忘了划掉那一行，也没有任何东西会提醒——半年后没人知道 B1
/// 到底走到哪一步。反向那条（「名单里的文件已不再引用」必须报）
/// 才让这张表成为可读的进度。
///
/// # 为什么另钉一条注释豁免
///
/// 视图的模块文档大量讨论「为什么不再用 `EditorApp`」「旧的
/// `crate::app::frame` 长什么样」。若按字面 `contains` 扫描，
/// **解释性文字会被当成违规**，逼得人不敢写文档。守卫走词法器
/// （[`arch_check::first_crate_app_ref_line`]）而非字符串匹配，
/// 注释天然不参与——这条用例就是它的护栏。
///
/// # 为什么钉死花括号形式（P0 回归）
///
/// 本用例出现过一次**真实漏检**：旧判据匹配 `crate` `::` `app` 三个**相邻**
/// token，而 `use crate::{app::frame::EditorApp};` 的 token 流是
/// `crate` `::` `{` `app`——第三个是 `{`，必然不命中。验收方在已迁的
/// `welcome_view.rs` 里注入下面 ⑤ 那两行 → `build.rs` 层 exit 0、
/// bin 层 `check-arch: OK`，**两道门禁全绿**：视图可以真实持有组合根而无人
/// 报警。根因不是判据松，是**两套扫描器并存**（`parse_use_tree` 早就支持
/// 花括号展开，只有这条守卫没用它）。⑤ / ⑥ / ⑦ 三条就是防止回退：
/// 改回字面匹配，它们立刻转红。
#[test]
fn view_files_must_not_reach_the_composition_root() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    let view = "src/presentation/views/stats_view.rs";

    // —— ① 注入必报：空名单 + 直取 `crate::app` ——
    write_fixture(
        manifest,
        view,
        "//! x\nuse crate::app::frame::EditorApp;\npub fn show(_: &mut EditorApp) {}\n",
    );
    let violations = arch_check::check_view_decoupling_with(manifest, &[]);
    assert_eq!(
        violations.len(),
        1,
        "未登记的 `crate::app` 应报 1 条，实际：{:?}",
        described(&violations)
    );
    assert_eq!(
        violations[0].file, view,
        "违规应定位到该视图文件：{:?}",
        violations[0]
    );
    assert_eq!(
        violations[0].line, 2,
        "行号应指到 `use` 那一行，实际：{:?}",
        violations[0]
    );
    assert!(
        violations[0].detail.contains("EditorViewContext"),
        "提示应给出正确改法（收 &mut dyn EditorViewContext）：{:?}",
        violations[0].detail
    );

    // —— ② 注入不报：只有注释提到 `crate::app` ——
    write_fixture(
        manifest,
        view,
        "//! 本视图不再 use crate::app::frame::EditorApp。\n\
         //! 历史路径 crate::app::frame 已由窄接口替代。\n\
         pub fn show() {}\n",
    );
    let violations = arch_check::check_view_decoupling_with(manifest, &[]);
    assert!(
        violations.is_empty(),
        "注释里的路径不该被判违规，实际：{:?}",
        described(&violations)
    );

    // —— ③ 白名单放行 ——
    write_fixture(manifest, view, "//! x\nuse crate::app::frame::EditorApp;\n");
    let violations = arch_check::check_view_decoupling_with(manifest, &[view]);
    assert!(
        violations.is_empty(),
        "已登记的文件应放行，实际：{:?}",
        described(&violations)
    );

    // —— ④ 反向必报：名单陈旧（已解耦但没划掉） ——
    write_fixture(manifest, view, "//! x\npub fn show() {}\n");
    let violations = arch_check::check_view_decoupling_with(manifest, &[view]);
    assert_eq!(
        violations.len(),
        1,
        "名单陈旧应报 1 条，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("只减不增"),
        "提示应说明名单只减不增：{:?}",
        violations[0].detail
    );

    // —— ⑤ 花括号形式必报（P0 回归：旧判据在这里全绿） ——
    //    这一条就是验收方的一手复现：`use crate::{app::frame::EditorApp};`
    //    token 流里 `crate` 后面紧跟的是 `{` 而不是 `app`。
    write_fixture(
        manifest,
        view,
        "//! x\nuse crate::{app::frame::EditorApp};\n\
         fn _probe_holds_editor_app(a: &mut EditorApp) -> &mut EditorApp { a }\n",
    );
    let violations = arch_check::check_view_decoupling_with(manifest, &[]);
    assert_eq!(
        violations.len(),
        1,
        "花括号 `use` 展开后仍是 `crate::app`，必须报（这正是曾经绕过的形态），实际：{:?}",
        described(&violations)
    );
    assert_eq!(
        violations[0].line, 2,
        "行号应指到花括号 `use` 那一行，实际：{:?}",
        violations[0]
    );

    // —— ⑥ 嵌套花括号必报：`use crate::{app::{…, …}};` ——
    //    展开器要递归进组中组；只要有一条路径落在 `crate::app` 下就报。
    write_fixture(
        manifest,
        view,
        "//! x\nuse crate::{app::{frame::EditorApp, bootstrap::Services}};\n",
    );
    let violations = arch_check::check_view_decoupling_with(manifest, &[]);
    assert_eq!(
        violations.len(),
        1,
        "嵌套花括号组里的 `crate::app::*` 必须报（一条文件只报一条），实际：{:?}",
        described(&violations)
    );

    // —— ⑦ 跨行 use 树：行号指向真正写了 `app` 的那一行 ——
    //    Rust 允许把组拆成多行；行号若取 `use` 关键字那行，读者要多跳几行
    //    才能看到出事的段，故取**末段所在行**。
    write_fixture(
        manifest,
        view,
        "//! x\nuse crate::{\n    presentation::widgets,\n    app::frame::EditorApp,\n};\n",
    );
    let violations = arch_check::check_view_decoupling_with(manifest, &[]);
    assert_eq!(
        violations.len(),
        1,
        "跨行 use 树里的 `crate::app` 同样必须报，实际：{:?}",
        described(&violations)
    );
    assert_eq!(
        violations[0].line, 4,
        "行号应指到 `app::frame::EditorApp` 那一行（末段所在行），实际：{:?}",
        violations[0]
    );

    // —— ⑧ 花括号里没有 `crate::app` 时不误报 ——
    //    上一条的对照：同一形态、只把 `app` 换成别的模块，必须放行。
    //    少了这条，「花括号一律报」这种粗暴实现也能让 ⑤⑥⑦ 全绿。
    write_fixture(
        manifest,
        view,
        "//! x\nuse crate::{presentation::widgets, infrastructure::assets::Loader};\n",
    );
    let violations = arch_check::check_view_decoupling_with(manifest, &[]);
    assert!(
        violations.is_empty(),
        "花括号组里没有 `crate::app` 就不该报，实际：{:?}",
        described(&violations)
    );
}

/// 白名单退役后，无参 `check_view_decoupling` 仍必须抓「名单外引用」。
///
/// # 为什么这条必要
///
/// 3.1 把 `PENDING_DECOUPLED_VIEWS` 常量整个删了、`check_view_decoupling`
/// 改成传 `&[]`。若这次改动写错（比如误改成直接 `return Vec::new()`、
/// 或把 `allowlist.contains` 的判定弄反），上面那条用例仍会因为传的是显式
/// 空切片而全绿——**变异验证必须打在「无参入口」上**，那才是生产路径。
///
/// 真实工程当前干净，所以这条只能用合成夹具证明「守卫还活着」。
#[test]
fn the_retired_whitelist_does_not_weaken_the_path_guard() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    write_fixture(
        manifest,
        "src/presentation/views/stats_view.rs",
        "//! x\nuse crate::app::frame::EditorApp;\n",
    );

    // 走**无参**入口（生产路径）：白名单已退役，任何 `crate::app` 都该报。
    let violations = arch_check::check_view_decoupling(manifest);
    assert_eq!(
        violations.len(),
        1,
        "白名单退役后无参入口仍须抓住 `crate::app`，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("EditorViewContext"),
        "提示应给出正确改法：{:?}",
        violations[0].detail
    );
}

/// 视图不得出现组合根 `EditorApp` 记号（批 3 · 3.2 的守卫）。
///
/// # 这条守卫堵的是路径守卫的盲区
///
/// `check_view_decoupling` 只认路径 `crate::app`。子模块经 `use super::*`
/// 拿到父模块的私有 `use` 别名后，就能在不写一行 `crate::app` 的情况下
/// 声明 `app: &mut EditorApp`——批 2 之前 `preview_view` 的
/// `blocks` / `code` / `list` / `media` / `table` / `toc` 六个文件正是如此，
/// 它们**从未进过白名单、也从未被报过**。本用例把那个形状做成夹具钉死。
///
/// # 另一半同样重要：注释不得误伤
///
/// 收官时视图层实测有 **20 处 `EditorApp`，全在注释里**（解释「为什么不再用」）。
/// 按字面 `contains` 扫描会把文档判成违规，逼得人不敢写文档。
/// 下面第 ② 组用例就是这条护栏：四种注释形态 + 字符串字面量都必须放过。
#[test]
fn view_files_must_not_mention_the_composition_root_type() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();

    // —— ① 注入必报：`use` 引入 + 形参，两个记号都算 ——
    let facade = "src/presentation/views/stats_view.rs";
    write_fixture(
        manifest,
        facade,
        "//! x\nuse crate::app::frame::EditorApp;\npub fn show(_app: &mut EditorApp) {}\n",
    );
    let violations = arch_check::check_view_no_composition_root_marker(manifest);
    assert_eq!(
        violations.len(),
        1,
        "带 `EditorApp` 记号的视图应报 1 条，实际：{:?}",
        described(&violations)
    );
    assert_eq!(
        violations[0].file, facade,
        "违规应定位到该视图文件：{:?}",
        violations[0]
    );
    assert_eq!(
        violations[0].line, 2,
        "行号应指到首个记号所在行（`use` 那行），实际：{:?}",
        violations[0]
    );
    assert!(
        violations[0].detail.contains("EditorViewContext"),
        "提示应给出正确改法（收 &mut dyn EditorViewContext）：{:?}",
        violations[0].detail
    );

    // —— ② 注入必报：**隐式子模块**的形状（无 `crate::app`，只有类型记号）——
    //    这正是路径守卫的盲区：`preview_view/blocks.rs` 这类文件当年就是这样。
    let submodule = "src/presentation/views/preview_view/blocks.rs";
    std::fs::remove_file(manifest.join(facade)).unwrap();
    write_fixture(
        manifest,
        submodule,
        "//! 子模块（经 `use super::*` 拿到 EditorApp，本文件无 `crate::app`）\n\
         pub(super) fn draw(app: &mut EditorApp) {}\n",
    );
    let path_guard = arch_check::check_view_decoupling(manifest);
    assert!(
        path_guard.is_empty(),
        "前置：路径守卫对 `use super::*` 形态本就失明（这正是要补的缺口），实际：{:?}",
        described(&path_guard)
    );
    let marker_guard = arch_check::check_view_no_composition_root_marker(manifest);
    assert_eq!(
        marker_guard.len(),
        1,
        "记号守卫必须补上路径守卫的缺口，实际：{:?}",
        described(&marker_guard)
    );
    assert_eq!(
        marker_guard[0].line, 2,
        "行号应指到形参那一行，实际：{:?}",
        marker_guard[0]
    );

    // —— ③ 注入不报：四种注释形态 + 字符串字面量 ——
    //    文档注释里讨论「为什么不再用 EditorApp」是**健康**的（真实工程 20 处），
    //    词法器必须把它们全部剥掉；字符串字面量里出现同名字样同理。
    write_fixture(
        manifest,
        submodule,
        "//! 模块文档提到 EditorApp：历史包袱，已不再使用。\n\
         /// 函数文档也提到 EditorApp。\n\
         // 行注释：`EditorApp` / `crate::app` 都不该被判违规。\n\
         /* 块注释里也写了 EditorApp */\n\
         pub fn draw(vctx: &mut dyn EditorViewContext) {\n\
         \x20   let _ = \"EditorApp\";\n\
         \x20   let _ = vctx;\n\
         }\n",
    );
    let violations = arch_check::check_view_no_composition_root_marker(manifest);
    assert!(
        violations.is_empty(),
        "注释与字符串里的 `EditorApp` 不该被判违规，实际：{:?}",
        described(&violations)
    );
}

/// 宽上下文 `EditorViewContext` 只许**路由者**收（按视图窄化守卫）。
///
/// # 这条守卫堵的是「窄化可以悄悄回退」这个口子
///
/// 视图改收窄 trait 之后，`&mut dyn EditorViewContext` 仍是合法 Rust：
/// 它是 17 个窄 trait 的 bundle，成员面与窄化前一致。任何一个视图图省事
/// 写回宽上下文，越权访问就从「编译期不可达」退回「靠评审发现」，
/// 而 **编译、clippy、其余守卫全绿**——只有本判据能拦住。
///
/// 三组用例对应三件事：① 普通视图提到即报；② 两个路由者放行；
/// ③ 子目录里**同名**的文件不算路由者（白名单按相对 views 根的路径比对）。
#[test]
fn view_files_must_not_take_the_wide_context_unless_they_route() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();

    // —— ① 注入必报：普通视图收宽上下文（形参）——
    let view = "src/presentation/views/stats_view.rs";
    write_fixture(
        manifest,
        view,
        "//! x\npub fn show(vctx: &mut dyn EditorViewContext) {\n    let _ = vctx;\n}\n",
    );
    let violations = arch_check::check_view_narrow_context(manifest);
    assert_eq!(
        violations.len(),
        1,
        "普通视图收宽上下文应报 1 条，实际：{:?}",
        described(&violations)
    );
    assert_eq!(
        violations[0].file, view,
        "违规应定位到该视图文件：{:?}",
        violations[0]
    );
    assert_eq!(
        violations[0].line, 2,
        "行号应指到首个记号所在行（形参那行），实际：{:?}",
        violations[0]
    );
    assert!(
        violations[0].detail.contains("窄 trait"),
        "提示应给出正确改法（改收该视图自己的窄 trait）：{:?}",
        violations[0].detail
    );

    // —— ② 路由者放行：`main_view` / `overlays` 要能转交上下文 ——
    std::fs::remove_file(manifest.join(view)).unwrap();
    write_fixture(
        manifest,
        "src/presentation/views/main_view.rs",
        "//! x\npub fn show(vctx: &mut dyn EditorViewContext) {\n    let _ = vctx;\n}\n",
    );
    write_fixture(
        manifest,
        "src/presentation/views/overlays.rs",
        "//! x\npub fn draw(vctx: &mut dyn EditorViewContext) {\n    let _ = vctx;\n}\n",
    );
    let violations = arch_check::check_view_narrow_context(manifest);
    assert!(
        violations.is_empty(),
        "两个路由者收宽上下文是设计允许的，实际：{:?}",
        described(&violations)
    );

    // —— ③ 子目录里的同名文件**不是**路由者 ——
    //    白名单按相对 views 根的路径比对，`preview_view/main_view.rs` 这类
    //    同名文件若被放行，子模块就能绕过整条规则。
    write_fixture(
        manifest,
        "src/presentation/views/preview_view/main_view.rs",
        "//! x\npub fn show(vctx: &mut dyn EditorViewContext) {\n    let _ = vctx;\n}\n",
    );
    let violations = arch_check::check_view_narrow_context(manifest);
    assert_eq!(
        violations.len(),
        1,
        "子目录里的同名文件不算路由者，实际：{:?}",
        described(&violations)
    );
    assert_eq!(
        violations[0].file, "src/presentation/views/preview_view/main_view.rs",
        "违规应定位到子目录里的那个文件：{:?}",
        violations[0]
    );

    // —— ④ 注入不报：注释与字符串里的同名标识符 ——
    //    文档里讨论「为什么不再收宽上下文」是健康的（真实工程多处如此），
    //    词法器必须剥掉注释与字符串。
    std::fs::remove_file(manifest.join("src/presentation/views/preview_view/main_view.rs"))
        .unwrap();
    write_fixture(
        manifest,
        "src/presentation/views/stats_view.rs",
        "//! 本视图收自己的窄 trait，不再收 EditorViewContext。\n\
         // 行注释里的 EditorViewContext 不算违规。\n\
         pub fn show(vctx: &mut dyn StatsViewAccess) {\n\
         \x20   let _ = \"EditorViewContext\";\n\
         \x20   let _ = vctx;\n\
         }\n",
    );
    let violations = arch_check::check_view_narrow_context(manifest);
    assert!(
        violations.is_empty(),
        "注释与字符串里的同名标识符不该被判违规，实际：{:?}",
        described(&violations)
    );
}

/// `scan_project` 必须挂上窄化守卫。
///
/// 「函数写好了但没接进扫描」是本项目踩过的坑（守卫本身不产生编译错误）。
#[test]
fn the_narrow_context_guard_is_wired_into_scan_project() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    // `scan_project` 需要若干真实层根才不早退，故铺最小的 src + 登记表。
    write_fixture(manifest, "src/lib.rs", "//! x\n");
    write_registry(manifest, &[]);
    write_fixture(
        manifest,
        "src/presentation/views/stats_view.rs",
        "//! x\npub fn show(vctx: &mut dyn EditorViewContext) {\n    let _ = vctx;\n}\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert!(
        violations
            .iter()
            .any(|v| v.file == "src/presentation/views/stats_view.rs"
                && v.detail.contains("宽上下文")),
        "窄化守卫必须接进 scan_project，实际：{:?}",
        described(&violations)
    );
}

/// `scan_project` 必须**同时**挂上路径守卫与记号守卫。
///
/// 「函数写好了但没接进扫描」是本项目踩过的坑（守卫本身不产生编译错误）。
/// 这条只验接入：注入一个只有类型记号的视图，`scan_project` 要报出来。
#[test]
fn the_marker_guard_is_wired_into_scan_project() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    // `scan_project` 需要若干真实层根才不早退，故铺最小的 src + 登记表。
    write_fixture(manifest, "src/lib.rs", "//! x\n");
    write_registry(manifest, &[]);
    write_fixture(
        manifest,
        "src/presentation/views/stats_view.rs",
        "//! x\npub fn show(app: &mut EditorApp) {}\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert!(
        violations
            .iter()
            .any(|v| v.file == "src/presentation/views/stats_view.rs"
                && v.detail.contains("EditorApp")),
        "记号守卫必须接进 scan_project，实际：{:?}",
        described(&violations)
    );
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

/// 转义字符字面量（`'\\'`）之后的 token **不得被吞**。
///
/// # 为什么必须单独一条
///
/// 这是 2026-10-25 守卫②（shared 无环守卫）变异验证意外挖出的**存量
/// 词法 bug：`skip_char_or_lifetime` 的转义分支曾是「循环找闭合引号」，
/// 对 `'\\'` 会在吃掉第二个反斜杠时连带吞掉闭合引号，循环找不到闭合
/// 引号便不停，一路吃到文件里下一个 `'`——字符字面量之后的全部 token
/// 静默丢失，藏在后面的跨层引用全部漏检。真实工程约 200 处转义字符
/// 字面量（`path.rs` 的 `rsplit(['/', '\\'])` 即其一；crates 98 /
/// src 17 / tests 85，剔注释与字符串实测）处于该路径，意味着这条
/// 路径上的泄漏长期不可见。本用例把 `'\\'` 放在违规之前：旧实现下
/// 违规被吞（0 处），修复后必须报 1 处。
#[test]
fn escaped_backslash_char_literal_does_not_swallow_later_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = dir.path().join("src");
    write_fixture(
        &src_root,
        "domain/probe.rs",
        "//! x\nfn f() {\n    \
         let bs = '\\\\';\n    \
         let c = egui::Color32::WHITE;\n}\n",
    );

    let violations = arch_check::scan(&src_root);
    assert_eq!(
        violations.len(),
        1,
        "字符字面量之后的 token 不得被吞，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("egui"),
        "违规说明应点名 egui，实际：{}",
        violations[0].detail
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

/// **R-03 点名的变异验证**：把预览区的调用改回直连 `ports::system::open_external`
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
// 一之补二、V11-63：infrastructure 禁 egui（无条件生效）
// ============================================================================

/// **规则的牙口**：infrastructure 文件引 `egui` 必报。
///
/// # 为什么需要这条
///
/// §10.2 四问之④规定「egui 只应出现在表现适配器」。V11-63 两阶段端口化
/// （阶段 A `ColorImage` → `PortFrame`；阶段 B `egui::FontDefinitions` →
/// `PortFontDefinitions` 一族）之后，infrastructure 已零 egui 引用，
/// 规则**无条件生效**——不再有文件级放行清单。
/// 用例取 `highlighting/` 下的真实兄弟文件：它是最典型的「适配器层不该
/// 知道 egui」的形状。
#[test]
fn egui_is_forbidden_in_infrastructure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "infrastructure/highlighting/syntect_highlighter.rs",
        "//! x\nuse egui::Color32;\npub fn f(c: Color32) {}\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "infrastructure 文件引 egui 应报 1 处，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0]
            .detail
            .contains("infrastructure 层禁止依赖 egui"),
        "违规说明应点名 egui，实际：{}",
        violations[0].detail
    );
    assert_eq!(violations[0].line, 2, "应报告具体行号");
}

/// **棘轮已闭合**：V11-63 的 5 个历史泄漏路径现在**全部**照拦。
///
/// # 这条钉的是什么
///
/// 2026-10-25 的棘轮方案用文件级放行清单（`V11_63_KNOWN_EGUI_LEAKS`）
/// 让存量 5 个文件先过关（`fonts` 系 3 + `viewport_capture` 系 2）。
/// 两阶段端口化后清单与放行分支**都已删除**，规则升为无条件生效。
///
/// 本用例把那 5 个历史路径逐个写成 fixture，断言每个都报 1 处违规——
/// 若哪天有人把清单（或等价的位置型豁免，如目录前缀）加回来，这条当场红。
/// 末尾带上「同目录新文件」，钉住「位置不构成豁免」。
#[test]
fn former_v11_63_leak_paths_are_now_caught() {
    let former_leaks = [
        "infrastructure/fonts.rs",
        "infrastructure/fonts/discovery.rs",
        "infrastructure/fonts/assembly.rs",
        "infrastructure/export/viewport_capture.rs",
        "infrastructure/export/viewport_capture/image.rs",
        // 同目录新文件：不存在「整个目录放行」这种形状
        "infrastructure/fonts/extra.rs",
    ];

    for path in former_leaks {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("src");
        write_fixture(
            &root,
            path,
            "//! x\nuse egui::Color32;\npub fn f(c: Color32) {}\n",
        );

        let violations = arch_check::scan(&root);
        assert_eq!(
            violations.len(),
            1,
            "{path} 现在应无条件被拦（V11-63 放行清单已删），实际：{:?}",
            described(&violations)
        );
        assert!(
            violations[0]
                .detail
                .contains("infrastructure 层禁止依赖 egui"),
            "{path} 的违规说明应点名 egui，实际：{}",
            violations[0].detail
        );
    }
}

// ============================================================================
// 一之补、R-13：presentation 内部依赖方向
// ============================================================================

/// **R-13 点名判据**：`presentation/state/x.rs` 写
/// `use crate::presentation::views::y;` → 必须报违规。
///
/// # 这条方向为什么最重要
///
/// `views/` 读 `state/` 是单向的。若 `state/` 回指 `views/`，数据流成环：
/// 「改一个开关要先构造视图」，而立即模式框架下视图每帧重建，
/// 环会以「状态被视图的渲染副作用污染」的形式发作，极难定位。
#[test]
fn state_may_not_depend_on_views() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/state/scroll_sync_state.rs",
        "//! x\nuse crate::presentation::views::preview_view;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "state 引用 views 应被拦，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("crate::presentation::views"),
        "{:?}",
        violations[0]
    );
}

/// `views/` **可以**读 `state/`（那是它存在的理由）——方向必须是单向的。
///
/// # 为什么必须有一条「反向放行」用例
///
/// 只测「state→views 报」不够：那可能是因为 `views` 前缀被无差别禁了。
/// 这条钉住「禁的是**方向**，不是**名字**」——`views` 引用 `state` 合法。
#[test]
fn views_may_depend_on_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/preview_view.rs",
        "//! x\nuse crate::presentation::state::scroll_sync_state::ScrollSyncState;\n",
    );

    let violations = arch_check::scan(&root);
    assert!(
        violations.is_empty(),
        "views 读 state 是既定方向，应放行。实际：{:?}",
        described(&violations)
    );
}

/// `rich/` 不得依赖四个兄弟子层（`views` / `state` / `theme` / `widgets`）。
///
/// 注意本项**不含 egui**：禁兄弟子层与「rich 主体 egui-free」是两条独立
/// 判据，后者见 [`rich_may_not_reference_egui_outside_the_backend`]
/// （白名单仅 `render/egui.rs`）。`rich/` 是**渲染算法**：一旦能读
/// `state/`，「排版结果随 UI 状态变化」就会进来——同一段文档在侧栏开着
/// 与关着时排版不同，而这种差异不报错、也看不出原因。
#[test]
fn rich_may_not_depend_on_its_siblings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    let banned = [
        ("views", "crate::presentation::views"),
        ("state", "crate::presentation::state"),
        ("theme", "crate::presentation::theme"),
        ("widgets", "crate::presentation::widgets"),
    ];
    for (name, prefix) in banned {
        write_fixture(
            &root,
            &format!("presentation/rich/math/{name}_case.rs"),
            &format!("//! x\nuse {prefix}::x::Y;\n"),
        );
    }

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        banned.len(),
        "rich 对四个兄弟子层的依赖应全部报出，实际：{:?}",
        described(&violations)
    );
}

// ============================================================================
// R-04 步4：`presentation/rich/` 主体 egui-free（唯一白名单 render/egui.rs）
// ============================================================================

/// `rich/` 下非白名单文件写了 `use egui` → 必须报，行号指到 import 那一行。
///
/// # 为什么这条不能靠编译期保证
///
/// `rich/` 与 egui 在同一个 workspace 里，`use egui::Painter;` 编译得过、
/// 单测照绿，唯一代价是「排版代码从此离不开 egui」——导出 / headless 复用
/// 要拖进整个 egui。文档里写「不碰 egui」挡不住这一行。
#[test]
fn rich_may_not_reference_egui_outside_the_backend() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    let root = manifest.join("src");
    write_fixture(
        &root,
        "presentation/rich/math/paint.rs",
        "//! x\nuse egui::Painter;\n",
    );

    let violations = arch_check::check_rich_egui_isolation(manifest);
    assert_eq!(
        violations.len(),
        1,
        "rich/math 下的 `use egui` 应被报出，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0]
            .file
            .contains("presentation/rich/math/paint.rs"),
        "{:?}",
        violations[0]
    );
    assert_eq!(
        violations[0].line, 2,
        "行号应指到 import 所在行，实际：{:?}",
        violations[0]
    );
}

/// 不写 import、直接全限定引用 `egui::Color32` → 同样必须报。
///
/// 只查 `use` 的守卫会被这一行绕过，而它是最省事的「偷渡」写法：
/// 一个文件里出现十几次 `egui::`，却一条 import 都没有。
#[test]
fn rich_may_not_reference_egui_by_qualified_path_without_import() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    let root = manifest.join("src");
    write_fixture(
        &root,
        "presentation/rich/mermaid/paint.rs",
        "//! x\npub fn f() {\n    let _: egui::Color32 = egui::Color32::WHITE;\n}\n",
    );

    let violations = arch_check::check_rich_egui_isolation(manifest);
    assert_eq!(
        violations.len(),
        1,
        "全限定 `egui::` 也应被报出，实际：{:?}",
        described(&violations)
    );
    assert_eq!(
        violations[0].line, 3,
        "行号应指到限定符所在行，实际：{:?}",
        violations[0]
    );
}

/// 反向：**白名单内**的后端可以 `use egui`（否则整条规则无从落地）。
///
/// 少了这一条，「把守卫改成一律放行」也能过——白名单本身没被钉住时，
/// 它就只是一句注释。
#[test]
fn the_render_backend_may_use_egui() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    let root = manifest.join("src");
    write_fixture(
        &root,
        "presentation/rich/render/egui.rs",
        "use egui::Painter;\n",
    );
    // 注释里提到 egui 不算引用（`rich/` 的模块文档里这类解释性文字有 20+ 处）。
    write_fixture(
        &root,
        "presentation/rich/math.rs",
        "//! 不碰 `egui::Painter`：度量注入。\n// 旧实现直接调 egui。\n",
    );

    let violations = arch_check::check_rich_egui_isolation(manifest);
    assert!(
        violations.is_empty(),
        "白名单后端与注释里的 egui 字样都不应报，实际：{:?}",
        described(&violations)
    );
}

/// 新判据与既有的 `PRESENTATION_RICH_FORBIDDEN`（禁兄弟子层）**叠加**，
/// 不是替换：同时犯两条的文件应各报一处。
#[test]
fn the_egui_ban_stacks_with_the_sibling_layer_ban() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    let root = manifest.join("src");
    write_fixture(
        &root,
        "presentation/rich/mermaid/paint.rs",
        "//! x\nuse crate::presentation::views::preview_view;\nuse egui::Painter;\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert_eq!(
        violations.len(),
        2,
        "同一文件同时犯「依赖 views」与「引用 egui」应各报一处，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations
            .iter()
            .any(|v| v.detail.contains("crate::presentation::views")),
        "应保留禁兄弟子层的那条，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations.iter().any(|v| v.detail.contains("R-04")),
        "应叠加 rich-egui 那条，实际：{:?}",
        described(&violations)
    );
}

/// `widgets/` 与 `icons.rs` 不得依赖 `views/`。
///
/// 控件与字形图标是**被视图复用**的叶子；它们读视图就形成「叶子→根」
/// 的反向边，而复用关系一旦成环，控件再也抽不出来单独测试。
#[test]
fn leaf_layers_may_not_depend_on_views() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/widgets/elided_text.rs",
        "//! x\nuse crate::presentation::views::preview_view::helper;\n",
    );
    write_fixture(
        &root,
        "presentation/icons.rs",
        "//! x\nuse crate::presentation::views::preview_view::helper;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        2,
        "widgets 与 icons 引用 views 应各报一处，实际：{:?}",
        described(&violations)
    );
    for v in &violations {
        assert!(v.detail.contains("crate::presentation::views"), "{v:?}");
    }
}

/// 层**根文件**同样受细粒度判定（`presentation/state.rs` 不是 `state/` 目录）。
///
/// # 为什么必须单独一条
///
/// 方案风险点原文点名：「需保证判定对**层根文件**
/// （`src/presentation/state.rs`、`src/presentation/views.rs`）同样生效」。
/// `layer_of` 靠「剥掉末段 `.rs` 后取第二段」覆盖两种形态：
/// `presentation/state/foo.rs` 与 `presentation/state.rs` 的第二段都是
/// `"state"`。若这条红了，说明根文件形态漏判——而那正是 R-01 修掉的
/// 同一类缺陷（`src/domain.rs` 曾整份逃过检查）。
#[test]
fn the_layer_root_files_are_judged_too() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/state.rs",
        "//! x\nuse crate::presentation::views::preview_view;\n",
    );
    write_fixture(
        &root,
        "presentation/views.rs",
        "//! x\nuse crate::presentation::state::ScrollSyncState;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "state.rs（根文件）引用 views 应报；views.rs 引用 state 应放行。实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].file.contains("presentation/state.rs"),
        "{:?}",
        violations[0]
    );
}

/// `views/` 的 `crate::app::frame` 窄豁免**已随迁移收官撤回**。
///
/// # 这条用例的方向为什么反过来了
///
/// R-13 时它叫 `views_keep_the_narrow_app_frame_allowance`，钉的是
/// 「细化层判定别漏给 `views/` 同步豁免，否则 35 个视图同时报错」——
/// 那是对的，因为当时视图确实在收 `&EditorApp`。
///
/// R-03 B1 收官后视图层零 `crate::app` 引用，留下的豁免只剩坏处：
/// 漏写 `use` 到 `crate::app::frame` 不会有任何提示，规则从「不该这么做」
/// 退化成「随你」。故按 `docs/spec/architecture-constraints.md` §10.1 硬约束 5
/// 撤回。本用例随之反转为「必须报」——**方向反转本身就是兑现证据**。
///
/// 若哪天视图真的又需要它，正确处理是重新评估 `EditorViewContext` 是否缺
/// 能力，而不是把豁免加回来：加回豁免会让记号守卫仍然绿，问题被永久掩盖。
#[test]
fn views_no_longer_get_the_app_frame_allowance() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/sidebar_view.rs",
        "//! x\nuse crate::app::frame::EditorApp;\nuse crate::app::frame::PendingDialog;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        2,
        "视图碰 `crate::app::frame` 现在应各报一条（豁免已撤回）。实际：{:?}",
        described(&violations)
    );
    assert!(
        violations
            .iter()
            .all(|v| v.detail.contains("crate::app") || v.detail.contains("组合根")),
        "提示应说明视图不得直取组合根：{:?}",
        described(&violations)
    );
}

/// 但窄豁免**不外溢**：`views/` 碰 `crate::app::bootstrap` 照样报。
///
/// 与上一条互为对照：豁免是「更窄前缀」而非「放开整个 `crate::app`」。
/// 视图伸手去拿启动编排等于绕过组合根。
#[test]
fn the_narrow_allowance_does_not_leak_to_the_rest_of_app() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/startup_view.rs",
        "//! x\nuse crate::app::bootstrap::Services;\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "views 引用 app::bootstrap 应被拦，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("crate::app"),
        "{:?}",
        violations[0]
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
/// 现实：表现层 54 个文件都这么干。规则若一刀切禁掉 `crate::app`，
/// 就会变成「一条没人能遵守的规则」——例外随之蔓延成惯例，
/// 连真正要拦的反向依赖也一起淹掉。
///
/// # 落点为什么是 `presentation/shell.rs` 而不是 `presentation/views/`
///
/// R-03 B1 收官后 `views/` 的窄豁免已撤回（见
/// `views_no_longer_get_the_app_frame_allowance`），`Layer::PresentationViews`
/// 不再放行 `crate::app::frame`。但**机制本身仍在**——`Layer::Presentation`
/// （`presentation/` 直下、非 `views/`）保留豁免，`shell.rs` 这类外壳
/// 仍在收 `&EditorApp`。换落点而不是删用例：否则撤回豁免时很容易
/// 顺手把「窄前缀」机制整个删掉，下次真需要时又得重新发明。
#[test]
fn presentation_may_reach_the_app_frame_handle() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/shell.rs",
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
// 七之补、R-12：presentation 不得直写 app 状态字段（赋值形态）
// ============================================================================
//
// 本节四个夹具的形参一律写成**裸 `EditorApp`**（不带 `crate::app::frame::`
// 前缀）。原因：本节判的是 R-12（「不得直写 app 状态字段」），不是层级依赖；
// 而 R-03 B1 收官后 `views/` 的 `crate::app::frame` 窄豁免已撤回，带前缀的
// 形参会额外触发一条层级违规，把 `violations.len() == 1` 的断言撑成 2 条，
// 让用例红在**另一个**规则上。裸记号恰好也是真实代码的形状（子模块经
// `use super::*` 拿到别名），故这不是「为了过测而削足」，是把用例的判据
// 收敛到它真正要钉的那一条。

/// **R-12 点名判据**：`presentation/views/x.rs` 写 `app.a.b = z;`
/// → 必须报「不得直写 app 状态字段」。
///
/// 这条把 R-11 收敛掉的形状**机械化**：R-11 把 `app.<state>.<field> =`
/// 收敛到 `EditorApp` 的方法入口，本规则把「零直写」钉死——将来有人
/// 在视图里图省事直接 `app.x.y = z`，编译照过、单测未必覆盖，这条必报。
#[test]
fn presentation_direct_write_of_app_state_field_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/foo_view.rs",
        "//! x\nfn f(app: &mut EditorApp) {\n    app.a.b = 1;\n}\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "表现层直写 app 状态字段必须被拦：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("不得直写 app 状态字段"),
        "应点名 R-12 规则：{:?}",
        violations[0]
    );
}

/// 单层直写同样要拦：`app.x = z;`（R-11 的两层 grep 口径曾漏掉这一形态，
/// `editor_view.rs` 的 `app.editor_text =` 正是这种）。
#[test]
fn presentation_single_level_write_of_app_state_field_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/foo_view.rs",
        "//! x\nfn f(app: &mut EditorApp) {\n    app.editor_text = String::new();\n}\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "单层直写 app 字段也必须被拦：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("不得直写 app 状态字段"),
        "{:?}",
        violations[0]
    );
}

/// **R-12 的已知边界**：`&mut app.a.b`（egui 控件必须拿 `&mut`）是**借用**
/// 不是赋值，必须放行——否则 `TextEdit::singleline(&mut app.x.y)` 这类
/// 既合法又必要的代码会被误伤。
#[test]
fn presentation_may_borrow_app_state_field_for_egui() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/foo_view.rs",
        "//! x\nfn f(app: &mut EditorApp) {\n    \
         TextEdit::singleline(&mut app.a.b);\n}\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "egui 控件的 &mut 借用不是直写，应放行：{:?}",
        described(&arch_check::scan(&root))
    );
}

/// **R-12 的已知边界**：`app.a.b == z` 是判据里的相等比较，不是赋值，必须放行。
///
/// 这条钉住「`=` 与 `==` 必须区分」——若词法器把两者混为一谈，
/// `export_view.rs` 里十多处 `app.export_ui.format == …` 会整批误报。
#[test]
fn presentation_may_compare_app_state_field() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "presentation/views/foo_view.rs",
        "//! x\nfn f(app: &mut EditorApp) {\n    \
         let selected = app.a.b == 1;\n}\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "相等比较不是赋值，应放行：{:?}",
        described(&arch_check::scan(&root))
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
    assert!(report.contains("docs/spec/architecture-constraints.md"));
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
    //
    // R-13 起 `views/` 也独立成层，故筛 `presentation/views 层`
    // （细粒度化前这里筛的是父层名 `presentation 层`）。
    let view_hits: Vec<_> = violations
        .iter()
        .filter(|v| v.detail.starts_with("presentation/views 层"))
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
        "`views/` 按自己的层名报告（细粒度化后不再是父层名）：{:?}",
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

// ============================================================================
// 十五、workspace 拆分（D-3 阶段 4a）：三根扫描与 rustmark_core 前缀归一
// ============================================================================

/// `scan_project` 三个根都扫到，且各根报告路径正确：
/// shell 侧不带前缀、core 侧带 `crates/rustmark-core/src/`、测试侧带 `tests/`。
///
/// # 为什么这条必须钉
///
/// 拆分后最危险的两种失覆盖都是**静默**的：只扫 shell `src/` → 整个 core
/// 逃过全部禁令且构建照绿；core 违规不补前缀 → CI 日志指不到文件；
/// 层判定误吃了显示前缀 → 首段 `crates` 把整个 core 判成「未知层」全量误报。
/// 三种形态一次钉死（core 侧特意放 `application/`——若被误判成未知层，
/// 会多出一条「未知层」违规，`len == 3` 当场红）。
#[test]
fn scan_project_covers_both_src_roots_and_tests() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    write_fixture(&manifest.join("src"), "lib.rs", "//! x\n");
    write_fixture(
        &manifest.join("src"),
        "domain/x.rs",
        "//! x\nuse std::fs;\n",
    );
    write_fixture(
        &manifest.join("crates/rustmark-core/src"),
        "application/y.rs",
        "//! x\nuse crate::presentation::z;\n",
    );
    write_fixture(
        &manifest.join("tests"),
        "domain/main.rs",
        "//! x\nuse crate::domain::x::Y;\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert_eq!(
        violations.len(),
        3,
        "三个根各注入一条，应恰好报 3 条（core 被误判未知层会多报）：{:?}",
        described(&violations)
    );
    assert!(
        violations.iter().any(|v| v.file == "domain/x.rs"),
        "shell 侧报告不带前缀：{:?}",
        described(&violations)
    );
    assert!(
        violations
            .iter()
            .any(|v| v.file == "crates/rustmark-core/src/application/y.rs"),
        "core 侧报告须带 crates/rustmark-core/src/ 前缀：{:?}",
        described(&violations)
    );
    assert!(
        violations.iter().any(|v| v.file == "tests/domain/main.rs"),
        "tests 侧报告带 tests/ 前缀：{:?}",
        described(&violations)
    );
}

/// `rustmark_core::X` 与 `crate::X` 是同一条边的两种拼写——归一后匹配
/// **同一张**禁令表，改写前缀绕不过去。
///
/// # 为什么归一是必须的
///
/// root 靠 re-export 把 core 拼回 `crate::`，词法只认字面路径、认不出两种
/// 拼写等价。若禁令表只认 `crate::`，把违规写成 `rustmark_core::domain::…`
/// 即可绕过全部层禁令——且这类写法**能编译**（root 依赖 core），
/// 编译器不会兜底。报告须同时点名**表词条**（`crate::domain`，
/// 读者要知道撞的是哪条规则）与**源码原拼写**（读者要对上自己那一行）。
#[test]
fn rustmark_core_prefix_does_not_bypass_layer_bans() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    write_fixture(&manifest.join("src"), "lib.rs", "//! x\n");
    write_fixture(
        &manifest.join("crates/rustmark-core/src"),
        "presentation/state/x.rs",
        "//! x\nuse rustmark_core::domain::workspace::repository::Repo;\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert_eq!(
        violations.len(),
        1,
        "rustmark_core:: 前缀写法必须按 crate:: 同等报出：{:?}",
        described(&violations)
    );
    let v = &violations[0];
    assert_eq!(
        v.file, "crates/rustmark-core/src/presentation/state/x.rs",
        "报告路径须带 core 前缀（层判定仍按根内相对路径）：{v:?}"
    );
    assert_eq!(v.line, 2, "行号须指向违规行：{v:?}");
    assert!(
        v.detail.contains("crate::domain"),
        "文案须点名禁令表词条：{v:?}"
    );
    assert!(
        v.detail.contains("rustmark_core::domain"),
        "文案须保留源码原拼写（读者要对上自己写的那一行）：{v:?}"
    );
}

/// root 的两个 re-export 拼接点**不产生**违规——归一化后天然落在禁令之外，
/// 无需给它们加豁免锚点。
///
/// 拼接点是 4a 的结构前提：`lib.rs`（Free 层，无禁令）与
/// `presentation.rs`（Presentation 层，禁令是 app / domain / infrastructure，
/// 归一后的 `crate::presentation::…` 不在其中）。这条钉住「无需豁免」这个
/// 结论——哪天有人往拼接点**追加** `rustmark_core::{domain,…}` 之外的
/// 形状、或禁令表被改宽，这条会红，逼人重新审视而不是静默放行。
///
/// V11-58：`presentation.rs` 的拼接形状随 icons 抽出改为两行——
/// `rustmark_core::presentation::{rich, shortcuts, state, theme, widgets}`
/// 之外再加一行 `pub use rustmark_icons as icons;`（把 crate 本身 re-export
/// 成模块）。后者同样不在 Presentation 禁令表里，一并钉住。
///
/// V11-62 批次 1：widgets 抽去 `rustmark-ui` 后改为三行——core 侧少一个
/// 子模块、新增 `pub use rustmark_ui::widgets;`。注意 `rustmark_ui::` **不做**
/// `crate::` 归一（归一只覆盖 `rustmark_core::`：root 拼接的是 core 的**全部**
/// 模块，而 ui 只拼接 `widgets` 一个模块，两种拼写不等价）；该写法不在
/// Presentation 禁令表里，同样零豁免通过。
#[test]
fn shell_reexport_splice_points_are_not_violations() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    write_fixture(
        &manifest.join("src"),
        "lib.rs",
        "//! x\npub use rustmark_core::{application, domain, infrastructure, shared};\n",
    );
    write_fixture(
        &manifest.join("src"),
        "presentation.rs",
        "//! x\npub mod views;\npub use rustmark_core::presentation::{rich, shortcuts, state, theme};\npub use rustmark_ui::widgets;\npub use rustmark_icons as icons;\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert!(
        violations.is_empty(),
        "两个拼接点是约定结构，不设豁免也不该报：{:?}",
        described(&violations)
    );
}

/// core 侧的 `arch-allow` 从 **manifest 级**登记表命中（登记表按 manifest
/// 加载一次、三个根共用）。
///
/// # 为什么单独钉
///
/// 登记表原按「扫描根的 parent」推断路径。core 根的 parent 是
/// `crates/rustmark-core`——那里没有 `docs/`，推断会加载**空表**，
/// 于是 core 侧所有合规豁免都被报成「豁免未登记」。当前 core 存量里
/// 尚无 `arch-allow`，这条用例是该修复的**唯一**守卫：若哪天有人把
/// `load_registry_at(manifest)` 改回逐根推断，这里立刻红。
#[test]
fn core_exemptions_resolve_from_the_manifest_registry() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    write_fixture(&manifest.join("src"), "lib.rs", "//! x\n");
    write_fixture(
        &manifest.join("crates/rustmark-core/src"),
        "domain/x.rs",
        "//! x\nuse egui; // arch-allow: egui\n",
    );
    write_registry_at(manifest, &[("domain/x.rs", "egui")]);

    let violations = arch_check::scan_project(manifest);
    assert!(
        violations.is_empty(),
        "core 侧豁免应从 manifest 级登记表命中（键为层相对路径）：{:?}",
        described(&violations)
    );
}

// ============================================================================
// 十六、shared 准入登记制（D-6 / 约束 §4.5）
// ============================================================================

/// `shared/` 下**未登记**的文件必须被 `scan_project` 报出（文件级、报首行）。
///
/// # 为什么这条判据必须机械化
///
/// 三问（≥2 层需要 / 零 IO / 无可归属的层）如果只写在约束文档里，
/// 回答与否完全靠自觉——`std::process` 当年就是「不在禁止清单」地混进
/// `shared/system.rs` 的。登记制把它变成可执行判据：不登记，
/// `build.rs`（每次构建）与 `check-arch`（CI）当场红。
#[test]
fn unregistered_shared_file_is_reported_by_scan_project() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    write_fixture(&manifest.join("src"), "lib.rs", "//! x\n");
    write_fixture(
        &manifest.join("crates/rustmark-core/src"),
        "shared/gateway.rs",
        "//! x\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert_eq!(
        violations.len(),
        1,
        "未登记的 shared 文件应恰好报 1 条，实际：{:?}",
        described(&violations)
    );
    let v = &violations[0];
    assert_eq!(
        v.file, "crates/rustmark-core/src/shared/gateway.rs",
        "报告路径须带 core 前缀（层判定仍按根内相对路径）：{v:?}"
    );
    assert_eq!(v.line, 1, "文件级规则报首行：{v:?}");
    assert!(v.detail.contains("三问"), "文案须点名准入三问：{v:?}");
    assert!(
        v.detail.contains("shared 登记表"),
        "文案须指路登记表：{v:?}"
    );
}

/// 登记过的 `shared/` 文件放行；登记列的**三种写法**等价。
///
/// 用最啰嗦的 `crates/rustmark-core/src/shared/gateway.rs` 做登记键，
/// 钉住归一化链（先剥 core 前缀再剥 `src/`）——写法一旦漂移，
/// 真实工程 18 个登记键会全部失配、守卫用例当场红。
#[test]
fn registered_shared_file_passes_scan_project() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    write_fixture(&manifest.join("src"), "lib.rs", "//! x\n");
    write_fixture(
        &manifest.join("crates/rustmark-core/src"),
        "shared/gateway.rs",
        "//! x\n",
    );
    write_shared_registry(manifest, &["crates/rustmark-core/src/shared/gateway.rs"]);

    let violations = arch_check::scan_project(manifest);
    assert!(
        violations.is_empty(),
        "已登记的 shared 文件不应报错：{:?}",
        described(&violations)
    );
}

/// **边界**：合成目录口径（`scan` / `scan_with_tests`）不套用登记制。
///
/// # 为什么单独钉这条边界
///
/// 登记制只挂在 `scan_project`（真实工程入口）：fixture 的合成目录里
/// 有几十个用例往 tempdir 写 `shared/*.rs` 注入层禁令违规，那里没有
/// 「三问登记表」的语境——把登记查挂进 `scan_impl` 会把它们全部误伤成
/// 两倍违规（既有用例的 `len == 1` 断言当场红）。这条用例是反方向钉子：
/// 哪天有人把校验下沉进 `scan_impl`「统一口径」，这里立刻红，
/// 逼人显式面对 fixture 边界而不是静默改坏一片。
#[test]
fn synthetic_scan_does_not_require_shared_registration() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = dir.path().join("src");
    write_fixture(&src_root, "lib.rs", "//! x\n");
    write_fixture(&src_root, "shared/gateway.rs", "//! x\n");

    let violations = arch_check::scan(&src_root);
    assert!(
        violations.is_empty(),
        "合成目录口径不查 shared 登记（无登记表语境），实际：{:?}",
        described(&violations)
    );
}

// ============================================================================
// 十六、V11-57：rustmark-actions 契约 crate（第三扫描根判 Free）
// ============================================================================

/// actions crate 的合成布局零违规：第三根整根判 Free，不误报「未知层」。
///
/// # 为什么这条必须钉
///
/// `layer_of` 按**根内相对路径**判层且不感知 crate：actions 根里的
/// `action.rs` / `action/impls.rs` / `action/menu_group.rs` 首段是
/// `action`，不在任何层名里 → 判 Unknown → 「未知层」误报。
/// 扫描根加了**层覆盖**没加（或覆盖被删）时，这条当场红。合成布局逐字
/// 复刻真实 crate 的文件名与 `use super::*;` 形状——复刻文件名是因为
/// 判层的正是文件名，改一个名字就可能改判结果。
///
/// # 为什么判 Free 而不是给 actions 配一张禁令表
///
/// 零依赖词汇 crate 没有层内边界可言；「不得引用 core / 任何 crate」由
/// Cargo 循环依赖（core 已依赖 actions，反向加普通依赖即解析期报错）与
/// workspace 拓扑断言（`docs/spec/workspace-topology.expected.txt`）兜底——
/// 两者都比词法规则强，且覆盖「获得任意依赖」而不只「引用 core」一种
/// 形状。配套的依赖面规格断言在 `tests/workspace_topology_test.rs`。
#[test]
fn actions_root_is_judged_free_without_false_unknown_layer() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    let actions = manifest.join("crates/rustmark-actions/src");
    write_fixture(&actions, "lib.rs", "//! x\npub mod action;\n");
    write_fixture(
        &actions,
        "action.rs",
        "//! x\nmod impls;\nmod menu_group;\npub enum UiAction { NewDocument }\npub enum MenuGroup { File }\n",
    );
    write_fixture(
        &actions,
        "action/impls.rs",
        "use super::*;\nimpl UiAction { pub const ALL: &'static [UiAction] = &[UiAction::NewDocument]; }\n",
    );
    write_fixture(
        &actions,
        "action/menu_group.rs",
        "use super::*;\nimpl MenuGroup { pub const ALL: &'static [MenuGroup] = &[MenuGroup::File]; }\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert!(
        violations.is_empty(),
        "actions 根应整体判 Free、零违规（漏了层覆盖会误报 3 条未知层）：{:?}",
        described(&violations)
    );
}

/// 层无关规则在新根照旧：禁 `mod.rs` 与帧预约唯一出口都报。
///
/// # 为什么这条必须钉
///
/// Free 只豁免**层禁令**；与层无关的规则（`CONSTR-03` 禁 `mod.rs` /
/// `request_repaint_after` 唯一出口）对全部文件生效。这条钉住「新根不是
/// checker 盲区」：若有人给第三根加 `continue` 式跳过（「零依赖 crate
/// 不用查」），或把 `forced_layer` 误用成「连 `check_file` 都不跑」，
/// 这里立刻红——失覆盖永远是静默的，只能靠注入形状逼它现形。
#[test]
fn layer_independent_rules_still_apply_in_actions_root() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    let actions = manifest.join("crates/rustmark-actions/src");
    write_fixture(&actions, "lib.rs", "//! x\npub mod action;\n");
    write_fixture(&actions, "action/mod.rs", "//! x\n");
    write_fixture(
        &actions,
        "action/impls.rs",
        "//! x\nfn f() { request_repaint_after(); }\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert_eq!(
        violations.len(),
        2,
        "层无关规则在新根照旧：mod.rs 与帧预约各一条，实际：{:?}",
        described(&violations)
    );
    let mod_rs = violations
        .iter()
        .find(|v| v.file == "crates/rustmark-actions/src/action/mod.rs")
        .expect("mod.rs 必报（CONSTR-03 不设豁免）");
    assert!(
        mod_rs.detail.contains("CONSTR-03"),
        "文案须点名 CONSTR-03：{}",
        mod_rs.detail
    );
    let repaint = violations
        .iter()
        .find(|v| v.file == "crates/rustmark-actions/src/action/impls.rs")
        .expect("帧预约越权必报");
    assert!(
        repaint.detail.contains("request_repaint_after"),
        "文案须点名帧预约唯一出口：{}",
        repaint.detail
    );
}

// ============================================================================
// 十七、V11-62：rustmark-ui 第五扫描根（presentation/ 前缀保子层判定）
// ============================================================================

/// ui 根的合成布局零违规：`presentation/` 路径前缀让 `layer_of` 判对应
/// 子层，不误报「未知层」。
///
/// # 为什么这条必须钉
///
/// `layer_of` 按**根内相对路径**判层且不感知 crate：ui 根若不用
/// `presentation/` 前缀（把 `widgets.rs` 平铺到 src 根下），首段
/// `widgets` 不在任何层名里 → 判 Unknown → 全量误报。批次 1 的方案是
/// **保留前缀**（`crates/rustmark-ui/src/presentation/widgets.rs`），层
/// 禁令表与豁免表零改动。合成布局复刻真实 crate 的文件名与跨 crate 引用
/// 形状：`rustmark_core::presentation::theme::builtin::Theme` 归一为
/// `crate::presentation::theme…` 后不在 widgets 子层禁令表里（widgets 是
/// 主题的合法消费者）——若哪天有人把 widgets→theme 禁掉，这条当场红。
#[test]
fn ui_root_presentation_prefix_keeps_sublayer_judgement() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    let ui = manifest.join("crates/rustmark-ui/src");
    write_fixture(&ui, "lib.rs", "//! x\npub mod presentation;\n");
    write_fixture(&ui, "presentation.rs", "//! x\npub mod widgets;\n");
    write_fixture(
        &ui,
        "presentation/widgets.rs",
        "//! x\npub mod row;\npub mod text;\n",
    );
    write_fixture(
        &ui,
        "presentation/widgets/row.rs",
        "//! x\nuse rustmark_core::presentation::theme::builtin::Theme;\npub fn f(t: &Theme) {}\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert!(
        violations.is_empty(),
        "ui 根应凭 presentation/ 前缀判对应子层、零违规（前缀丢了会误报未知层）：{:?}",
        described(&violations)
    );
}

/// ui 根不是 checker 盲区：widgets 子层禁令在新根照旧报。
///
/// # 为什么必须钉
///
/// 与上一条是一对：那条防「误报」（前缀丢了会全量未知层），这条防「漏报」
/// （新根被跳过、或「搬进新 crate = 洗白层禁令」的错觉）。搬到 ui 不等于
/// 豁免：`presentation/widgets/` 引 `views` 在 core 里报，在 ui 里同样报，
/// 且报告带 `crates/rustmark-ui/src/` 前缀（层判定仍按根内相对路径，
/// 不吃显示前缀——首段 `crates` 会把整个根误判成未知层）。
#[test]
fn ui_root_does_not_launder_sublayer_bans() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    let ui = manifest.join("crates/rustmark-ui/src");
    write_fixture(&ui, "lib.rs", "//! x\npub mod presentation;\n");
    write_fixture(
        &ui,
        "presentation/widgets/row.rs",
        "//! x\nuse crate::presentation::views::file_tree::FileTree;\npub fn f() {}\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert_eq!(
        violations.len(),
        1,
        "widgets 引 views 在 ui 根应恰好报 1 条：{:?}",
        described(&violations)
    );
    let v = &violations[0];
    assert_eq!(
        v.file, "crates/rustmark-ui/src/presentation/widgets/row.rs",
        "报告路径须带 ui 前缀（层判定按根内相对路径）：{v:?}"
    );
    assert!(
        v.detail.contains("crate::presentation::views"),
        "文案须点名子层禁令表词条：{}",
        v.detail
    );
}

/// views 若被误搬进 ui 根，既有禁令自动生效（ADR-013 决策 3 的结构防护）。
///
/// # 为什么必须钉
///
/// 决策 3「views 必须留 root」此前靠**人记得**；批次 1 起它有了结构形态：
/// views 的文件落在哪就按哪判层——搬进 ui 的 `presentation/views/` 同样判
/// `PresentationViews`，`crate::domain` 照报。这条钉住「防护是真的」：
/// 若 `layer_of` 的 views 分支被改成只认 shell 根，或有人给 ui 根加
/// `forced_layer = Free` 式豁免，这里立刻红。
#[test]
fn views_misplaced_in_ui_root_still_trip_the_bans() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path();
    let ui = manifest.join("crates/rustmark-ui/src");
    write_fixture(&ui, "lib.rs", "//! x\npub mod presentation;\n");
    write_fixture(
        &ui,
        "presentation/views/sidebar.rs",
        "//! x\nuse crate::domain::document::repository::Repo;\npub fn f() {}\n",
    );

    let violations = arch_check::scan_project(manifest);
    assert_eq!(
        violations.len(),
        1,
        "views 搬进 ui 也逃不过 presentation 父层禁令：{:?}",
        described(&violations)
    );
    let v = &violations[0];
    assert_eq!(
        v.file, "crates/rustmark-ui/src/presentation/views/sidebar.rs",
        "报告路径须带 ui 前缀：{v:?}"
    );
    assert!(
        v.detail.contains("crate::domain"),
        "文案须点名父层禁令表词条：{}",
        v.detail
    );
}

// ============================================================================
// 十八、`super::` 相对路径与 glob 导入（E-04 / E-05）
// ============================================================================

/// `super::` 链跨层必须拦——相对路径不是「换个写法就绕开门禁」的后门。
///
/// # 这条为什么必须钉
///
/// 禁令表是**绝对路径**词条（`crate::infrastructure`），而 Rust 允许用
/// `super::super::…` 表达同一条边。旧实现只在意第一段的字面 expect，于是
/// 「domain 里写 `use super::super::super::infrastructure::x`」编译照过、
/// 门禁照绿——层禁令从窗口悄悄变成门帘。
#[test]
fn super_chain_reaching_another_layer_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nuse super::super::super::infrastructure::cache::Entry;\npub fn f() {}\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "domain 经 super 链引 infrastructure 应报 1 处，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0].detail.contains("infrastructure"),
        "{:?}",
        violations[0]
    );
}

/// glob 导入不得成为绕过全局禁令的通道。
#[test]
fn glob_import_of_a_forbidden_layer_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nuse crate::infrastructure::*;\npub fn f() {}\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "domain 用 glob 拉入 infrastructure 应报 1 处，实际：{:?}",
        described(&violations)
    );
}

/// 同层的 `super::` 链**不**是违规——判据区分的是「是否越过层界」，不是「是否用了相对路径」。
///
/// # 为什么必须有一条反向放行
///
/// 只测「跨层报」的守卫可以被一条粗暴实现骗过去：把所有 `super::` 一律判违规。
/// 那会让工程里几十处同层兄弟引用全部告警，规则当即不可遵守。
/// 这条钉住「禁的是**越层**，不是**相对写法**」。
#[test]
fn same_layer_super_chain_is_not_a_violation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "application/document/service/drafts.rs",
        "//! x\nuse super::super::queries::Draft;\npub fn f(d: Draft) {}\n",
    );

    let violations = arch_check::scan(&root);
    assert!(
        violations.is_empty(),
        "同层兄弟引用不该报，实际：{:?}",
        described(&violations)
    );
}

/// 内联测试模块里的 `super::` 链同样要拦。
///
/// # 为什么这条单独成例
///
/// `#[cfg(test)] mod tests { use super::super::…; }` 里的 `super` 相对的是
/// **那个内联模块**，比文件本身深一层。若解析器只按文件模块深度算，这一整类
/// 「躲进测试模块里跨层」的写法会被漏掉——而这恰好是「写测试时图省事」最常
/// 走的路。故要做多深度猜测（[:arch_check::MAX_INLINE_MODULE_DEPTH]）。
#[test]
fn super_chain_inside_an_inline_test_module_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\n#[cfg(test)]\nmod tests {\n    use super::super::super::super::infrastructure::cache::Entry;\n}\n",
    );

    let violations = arch_check::scan(&root);
    assert_eq!(
        violations.len(),
        1,
        "内联测试模块里的跨层 super 链应报 1 处，实际：{:?}",
        described(&violations)
    );
    assert!(
        violations[0]
            .detail
            .contains("解析为 crate::infrastructure"),
        "文案须给出解析后的绝对路径：{}",
        violations[0].detail
    );
}

/// glob 导入的三种形态都必须拦：裸 glob、组内 glob、重导出 glob。
///
/// # 为什么留着这条（以及为什么它不是新增规则）
///
/// 评估阶段曾怀疑 glob 会绕过禁令；实测三种形态**都已被现有段前缀匹配命中**。
/// 用例保留下来是为了把这桩已查证的事实钉住：日后若有人把 `matches_ban`
/// 改成全字面相等（看 glob 的 `::*` 尾巴不顺眼），这里立刻红。
#[test]
fn glob_import_shapes_are_all_detected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/a.rs",
        "//! x\nuse crate::infrastructure::*;\npub fn f() {}\n",
    );
    write_fixture(
        &root,
        "domain/b.rs",
        "//! x\nuse crate::{infrastructure::*, shared::lru};\npub fn f() {}\n",
    );
    write_fixture(
        &root,
        "domain/c.rs",
        "//! x\npub use crate::infrastructure::*;\n",
    );

    let violations = arch_check::scan(&root);
    let detail = described(&violations);
    assert_eq!(violations.len(), 3, "三种 glob 形态各报 1 处：{detail:?}");
    assert!(
        detail.iter().all(|d| d.contains("infrastructure")),
        "每条都应点名 infrastructure：{detail:?}"
    );
}
