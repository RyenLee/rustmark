//! 架构边界校验器自身的验证（S2-07）。
//!
//! 两条断言：
//! 1. 当前工程**零违规**（真实源码扫描）；
//! 2. 注入违规的临时工程**必须被检出**（证明校验器不是形同虚设）。

use std::path::{Path, PathBuf};

mod arch_check {
    include!("../tools/arch_check.rs");
}

fn write_fixture(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn current_project_has_no_violations() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let violations = arch_check::scan(&src_root);
    assert!(
        violations.is_empty(),
        "当前工程不应存在分层违规，实际：{:?}",
        violations
            .iter()
            .map(|v| format!("{}:{} {}", v.file, v.line, v.detail))
            .collect::<Vec<_>>()
    );
}

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

#[test]
fn arch_allow_comment_suppresses_single_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(
        &root,
        "domain/document/aggregate.rs",
        "//! x\nuse egui; // arch-allow: egui（已在 architecture.md 登记）\n",
    );

    assert!(
        arch_check::scan(&root).is_empty(),
        "带 arch-allow 注释的行应被豁免"
    );
}

#[test]
fn signature_changes_when_source_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("src");
    write_fixture(&root, "shared/ids.rs", "//! x\n");
    let before = arch_check::signature(&root);

    write_fixture(&root, "shared/ids.rs", "//! x\n// changed\n");
    let after = arch_check::signature(&root);

    assert_ne!(
        before, after,
        "源文件变更后指纹应变化（驱动 build.rs 重扫）"
    );
}

// ============================================================================
// S6-12：presentation/state 子层
// ============================================================================

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
    // `Violation` 不派生 `Debug`（S2-07 的既定接口），故手工格式化。
    let described = |vs: &[arch_check::Violation]| -> Vec<String> {
        vs.iter()
            .map(|v| format!("{}:{} {}", v.file, v.line, v.detail))
            .collect()
    };
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
        state_violations
            .iter()
            .map(|v| format!("{}:{} {}", v.file, v.line, v.detail))
            .collect::<Vec<_>>()
    );
}
