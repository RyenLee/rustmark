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

/// 从根 manifest 里读某个「字符串数组」键（`members` / `default-members`）。
///
/// 只解析本项目用到的形态——`key = [` 换行、逐行一个带引号的路径、`]` 收尾，
/// 允许行内 `#` 注释（本条只取引号内的内容，注释文字不影响）。刻意不引入
/// `toml` 依赖：这是门禁测试，判据越少依赖越好，解析失败也要能一眼看懂。
fn manifest_array_values(text: &str, key: &str) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    let mut buffer = String::new();
    let mut in_array = false;
    for raw in text.lines() {
        let line = raw.trim();
        if !in_array {
            if line.starts_with('#') {
                continue;
            }
            let Some(rest) = line.strip_prefix(key) else {
                continue;
            };
            let Some(rest) = rest.trim_start().strip_prefix('=') else {
                continue;
            };
            let Some(rest) = rest.trim_start().strip_prefix('[') else {
                continue;
            };
            in_array = true;
            buffer.push_str(rest);
        } else {
            buffer.push_str(line);
        }
        if buffer.contains(']') {
            let body = buffer.split(']').next().unwrap_or_default();
            for piece in body.split('"').skip(1).step_by(2) {
                let value = piece.trim();
                if !value.is_empty() {
                    values.insert(value.to_string());
                }
            }
            break; // 本键解析完即止，后面的 `[package]` / `[dependencies]` 不再看
        }
        buffer.push(' ');
    }
    values
}

/// `default-members` 必须覆盖**全部** workspace 成员（多 crate 评审 §4.1 的机械化）。
///
/// 背景：`--workspace` 一直靠人记。漏带时 cargo 只跑默认成员（root 包），
/// core / ui / infra 的单测**静默缺席**——本地全绿、CI 才红，正是本项目最忌讳的
/// 「检查缺席却显示绿」。`default-members` 把这个纪律变成 cargo 自己的默认行为，
/// 但它自身有个同型失效模式：**新增 crate 只改 `members`、忘了改 `default-members`**，
/// 新 crate 重新对裸命令隐身，而且没有任何报错。本条把两份清单一对一钉死。
#[test]
fn default_members_cover_every_workspace_member() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("根 Cargo.toml 应可读");

    let members = manifest_array_values(&text, "members");
    let defaults = manifest_array_values(&text, "default-members");
    assert!(
        !defaults.is_empty(),
        "根 Cargo.toml 的 [workspace] 缺少 default-members——裸 `cargo test` 会只跑 root，\
         其余成员的单测静默缺席"
    );

    let mut expected = members;
    // root 包（`.`）是隐式成员，不写进 `members`；有 [package] 就必须写进默认成员，
    // 否则裸命令连组合根自己都不编。
    if text.lines().any(|line| line.trim() == "[package]") {
        expected.insert(".".to_string());
    }
    assert_eq!(
        defaults, expected,
        "default-members 必须与「全部 workspace 成员」逐一对应（`members` + 隐式 root 的 \".\"）——\
         新增 crate 只改 members 会让它对裸 cargo 命令隐身，这类漏改不会报错，只会在某天\
         「本地全绿但 CI 红」时暴露"
    );
}

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

/// `rustmark-icons` 的依赖面必须恰为 **egui**（R-05，2026-10-27）。
///
/// 图标是「16×16 规格数据 + Painter 渲染器」：绘制原语只要 egui。R-05 前
/// `icons` 曾 `use rustmark_actions::UiAction`（`Icon::Action(UiAction)`），
/// 资源 crate 反向依赖动作词汇表；R-05 把映射上移到表现层调用方
/// （`rustmark::presentation::icon_key_for`），`icons` 改认自有的 `IconKey`
/// 词汇表，依赖面收敛为纯 egui。任何第二个依赖都说明资源 crate 在长领域
/// 逻辑——「抽出」只换了住处、没换职责。判据同样走 manifest 文本：收集
/// `[dependencies]` 段内的键与期望集合做**相等**断言，再全文禁掉其余依赖段
/// （dev / build / target-specific 一个都不许有）。
#[test]
fn icons_crate_dependency_face_is_egui_only() {
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
    let expected: BTreeSet<String> = ["egui"].iter().map(|s| s.to_string()).collect();
    assert_eq!(
        deps, expected,
        "rustmark-icons 的依赖面必须恰为 egui——资源 crate 不长领域逻辑，\
         要加依赖先来改这条断言"
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

/// `rustmark-ui` 的依赖面必须恰为 **egui + rustmark-core + rustmark-render**
/// （V11-62 批次 1 验收第 1 条的机械化；R-04 升 crate，2026-10-28 增补 render）。
///
/// widgets 是「egui 布局配方 + 主题配色消费者」：绘制原语只要 egui，
/// `row::paint_row_background` 读 `Theme` 的 accent_soft / bg_tertiary 只要
/// core；R-04 后富内容渲染引擎（`rustmark-render`）也由 ui 经 re-export 拼回
/// `rustmark_ui::presentation::rich`，故 ui 多要这一条本地依赖。任何第四个
/// 依赖都说明表现层 crate 在长领域逻辑——「抽出」只换了住处、没换职责。
/// 判据同样走 manifest 文本：`[dependencies]` 段键集合与期望集合**相等**
/// 断言，再全文禁掉其余依赖段（dev / build / target）。
///
/// 与 CI 拓扑 diff 的分工：那条守「root→ui、ui→core、ui→render 各边的增减」，
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
    let expected: BTreeSet<String> = ["egui", "rustmark-core", "rustmark-render"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        deps, expected,
        "rustmark-ui 的依赖面必须恰为 egui + rustmark-core + rustmark-render——\
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

/// `rustmark-render` 默认依赖面必须 **egui-free**（R-04 升 crate，2026-10-28）。
///
/// 富内容渲染引擎拆成独立 crate 的核心收益是「无头 / 导出环境可复用同一套
/// 排版而不背负 egui」：唯一允许 `use egui` 的 `render/egui.rs` 走
/// `feature = "egui"`，默认（无 feature）构建不引入 egui。判据走 manifest
/// 文本：
/// - `[dependencies]` 仅 `egui`，且 `optional = true`；
/// - `egui` 不出现在 `default` features（默认编译面 egui-free）；
/// - 不得有任何 dev / build / target-specific 依赖段。
///
/// 与 `check_rich_egui_isolation` 的分工：那条守「`render/` 下除 `egui.rs`
/// 外不得引用 egui」，本条守「**默认**特征下 egui 根本不在依赖图里」——
/// 前者防运行时误用，后者防依赖面静默膨胀，两道防线互补。
#[test]
fn render_crate_egui_free_by_default() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates")
        .join("rustmark-render")
        .join("Cargo.toml");
    let text = std::fs::read_to_string(&path).expect("render crate 的 Cargo.toml 应可读");

    let mut deps = BTreeSet::new();
    let mut section = String::new();
    let mut egui_spec = String::new();
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
        if let Some((name, rest)) = line.split_once('=') {
            let name = name.trim();
            deps.insert(name.to_string());
            if name == "egui" {
                egui_spec = rest.trim().to_string();
            }
        }
    }
    let expected: BTreeSet<String> = ["egui"].iter().map(|s| s.to_string()).collect();
    assert_eq!(
        deps, expected,
        "rustmark-render 的 [dependencies] 必须仅含 egui（其余依赖都说明渲染层在长领域逻辑）：{deps:?}"
    );
    assert!(
        egui_spec.contains("optional = true"),
        "rustmark-render 的 egui 必须是 optional——默认（无 feature）构建不应引入 egui：{egui_spec}"
    );

    // default features 不得含 egui（默认编译面 egui-free）。
    let mut features_section = String::new();
    let mut default_features: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            features_section = line.to_string();
            continue;
        }
        if features_section == "[features]"
            && line.starts_with("default")
            && let Some((_, rest)) = line.split_once('=')
        {
            default_features = rest
                .trim()
                .trim_matches(['[', ']'])
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    assert!(
        !default_features.iter().any(|f| f == "egui"),
        "rustmark-render 的 default features 不得含 egui——默认编译面必须 egui-free：{default_features:?}"
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
            "rustmark-render 只许有 normal [dependencies] 一段；\
             dev / build / target-specific 依赖段同样稀释「纯渲染」定位：{line}"
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
