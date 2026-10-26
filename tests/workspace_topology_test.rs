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

use std::collections::BTreeSet;

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

/// `rustmark-icons` 的依赖面必须恰为 **egui + rustmark-actions**
/// （V11-58 验收第 1 条的机械化）。
///
/// 图标是「16×16 规格数据 + Painter 渲染器」：绘制原语只要 egui，动作映射
/// （`Icon::Action(UiAction)`）只要词汇表。任何第三个依赖都说明资源 crate
/// 在长领域逻辑——「抽出」只换了住处、没换职责。判据同样走 manifest 文本：
/// 收集 `[dependencies]` 段内的键与期望集合做**相等**断言，再全文禁掉其余
/// 依赖段（dev / build / target-specific 一个都不许有）。
#[test]
fn icons_crate_dependency_face_is_egui_plus_actions() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates")
        .join("rustmark-icons")
        .join("Cargo.toml");
    let text = std::fs::read_to_string(&path).expect("icons crate 的 Cargo.toml 应可读");

    let mut deps = BTreeSet::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line.to_string();
            continue;
        }
        if section != "[dependencies]" {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            deps.insert(name.trim().to_string());
        }
    }
    let expected: BTreeSet<String> = ["egui", "rustmark-actions"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        deps, expected,
        "rustmark-icons 的依赖面必须恰为 egui + rustmark-actions——\
         资源 crate 不长领域逻辑，要加依赖先来改这条断言"
    );

    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('#') {
            continue;
        }
        assert!(
            !(line.starts_with("[dev-dependencies")
                || line.starts_with("[build-dependencies")
                || line.starts_with("[target")),
            "rustmark-icons 只许有 normal [dependencies] 一段；\
             dev / build / target-specific 依赖段同样稀释「纯资源」定位：{line}"
        );
    }
}

/// `rustmark-ui` 的依赖面必须恰为 **egui + rustmark-core**
/// （V11-62 批次 1 验收第 1 条的机械化）。
///
/// widgets 是「egui 布局配方 + 主题配色消费者」：绘制原语只要 egui，
/// `row::paint_row_background` 读 `Theme` 的 accent_soft / bg_tertiary 只要
/// core。任何第三个依赖都说明表现层 crate 在长领域逻辑——「抽出」只换了
/// 住处、没换职责。判据同样走 manifest 文本：`[dependencies]` 段键集合与
/// 期望集合**相等**断言，再全文禁掉其余依赖段（dev / build / target）。
///
/// 与 CI 拓扑 diff 的分工：那条守「root→ui、ui→core 两条边的增减」，
/// 本条守「ui 自身不再多要依赖」——后者是前者的细化，本地先红。
#[test]
fn ui_crate_dependency_face_is_egui_plus_core() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates")
        .join("rustmark-ui")
        .join("Cargo.toml");
    let text = std::fs::read_to_string(&path).expect("ui crate 的 Cargo.toml 应可读");

    let mut deps = BTreeSet::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line.to_string();
            continue;
        }
        if section != "[dependencies]" {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            deps.insert(name.trim().to_string());
        }
    }
    let expected: BTreeSet<String> = ["egui", "rustmark-core"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        deps, expected,
        "rustmark-ui 的依赖面必须恰为 egui + rustmark-core——\
         表现层 crate 不长领域逻辑，要加依赖先来改这条断言"
    );

    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('#') {
            continue;
        }
        assert!(
            !(line.starts_with("[dev-dependencies")
                || line.starts_with("[build-dependencies")
                || line.starts_with("[target")),
            "rustmark-ui 只许有 normal [dependencies] 一段；\
             dev / build / target-specific 依赖段同样稀释「纯表现层」定位：{line}"
        );
    }
}

/// `rustmark-shared` 不得依赖任何 workspace 成员（TD-04 验收第 1 条的机械化）。
///
/// 共享内核被 core / ui / root 依赖；它一旦反向依赖任一成员，Cargo 会以
/// 循环依赖报错——但那条错误只在具体边成环时出现，**「shared 获得本地依赖」
/// 这件事本身**应当是显式评审事件。本条把判据前移到 manifest：任何
/// `rustmark-*` 出现在依赖段（normal / dev / build 均算）即红。
///
/// 外部纯数据依赖（serde / thiserror / ahash）不受此限——它们正是
/// 「纯值 / 端口 / 唯一真源」定位的实现手段；要新增，仍应同步本测试的
/// 注释与 §10.1 的依赖面记录。
#[test]
fn shared_crate_declares_no_workspace_dependency() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates")
        .join("rustmark-shared")
        .join("Cargo.toml");
    let text = std::fs::read_to_string(&path).expect("shared crate 的 Cargo.toml 应可读");

    let mut offenders = Vec::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line.to_string();
            continue;
        }
        if !section.contains("dependencies") {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            let name = name.trim();
            if name.starts_with("rustmark") {
                offenders.push(format!("{section} → {name}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "rustmark-shared 是共享内核，不得依赖任何 workspace 成员\
         （否则与 core→shared 成环，或让「共享」变成「某层的下游」）：{offenders:?}"
    );
}

/// `rustmark-infra` 的 workspace 依赖面必须**恰为** `rustmark-core`
/// （V11-64 验收第 1 条的机械化）。
///
/// 基础设施是「端口在 core、实现在此」——它**必须**看得到端口与领域类型
/// （故 core 一条是刚需，不是可选项），又**只能**看得到这些：
///
/// - 反向边（infra → ui / actions）会与「core ← ui」一起绕出环，Cargo 只
///   在成环时才报，本断言把「infra 拿到了新的一方」前移成显式评审事件；
/// - infra → root（`rustmark`）会让基础设施依赖组合根，层级彻底倒置；
/// - 第三个本地依赖出现即说明 infra 在替某一层干活，「只是换住处、没换职责」
///   的迁移纪律失守。
///
/// 外部依赖（pulldown-cmark / syntect / rfd / ureq 等）不受此限——它们正是
/// 「适配外部库」的实现手段；要新增，仍应同步本测试注释与 §10.1 记录。
#[test]
fn infra_crate_depends_on_core_only() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates")
        .join("rustmark-infra")
        .join("Cargo.toml");
    let text = std::fs::read_to_string(&path).expect("infra crate 的 Cargo.toml 应可读");

    let mut workspace_deps = BTreeSet::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line.to_string();
            continue;
        }
        // normal / dev / build / target-specific 一律算——dev 边同样是
        // 「infra 能看见某人」的事实，只是不进产物。
        if !section.contains("dependencies") {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            let name = name.trim();
            if name.starts_with("rustmark") {
                workspace_deps.insert(name.to_string());
            }
        }
    }

    let expected: BTreeSet<String> = ["rustmark-core"].iter().map(|s| s.to_string()).collect();
    assert_eq!(
        workspace_deps, expected,
        "rustmark-infra 的 workspace 依赖面必须恰为 rustmark-core（端口与领域类型在 core）；\
         多一条即层级倒置或成环，少一条则端口不可见"
    );
}
