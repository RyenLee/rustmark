//! `shared` 内部依赖环守卫（V11-55 附案②，2026-10-25）。
//!
//! # 守的是什么
//!
//! Rust 允许 crate 内模块互相引用——环**编译得过**，编译器全程沉默。
//! 但共享内核内部有环意味着两件事：① 模块无法单独理解与单独测试
//! （读 `path.rs` 被迫先懂 `text_format`，反之亦然）；② 日后拆 crate
//! （V11-64 抽 `rustmark-infra` 一类）时环就是硬墙。评审（2026-10-21
//! 快照）时 shared 无环，其后新增的生产期代码造出二环：
//! `path.rs` 的 `FilePath::extension` 调
//! `crate::shared::text_format::extension_index`，而
//! `text_format::extension_of` 调 `crate::shared::path::file_name_of`。
//! 该环已破（`extension_index` 迁入 `path.rs`，理由见迁移点的文档），
//! 本守卫保证它**不会悄悄长回来**。
//!
//! # 判据（双向钉死）
//!
//! - **无环**：对真实工程跑 DFS，有环即报红并打出环路径；
//! - **边集相等**：把 11 条期望边钉成显式清单——新增边（环的雏形，
//!   哪怕暂时不成环）必须先落清单才能过，即「shared 内部依赖面的任何
//!   变化都要落到清单上」；删边同样报红（清单出现过期项）。
//!
//! 双向断言的理由同字段级可见性守卫：单向断言（只禁环）会让
//! 「加一条暂时不成环的边」无声通过，环的前置条件逐日累积。
//!
//! # 判定口径（为什么边只认一种写法）
//!
//! 跨模块引用有四种写法：`crate::shared::<mod>`（绝对）、`super::<mod>`
//! （super 链）、glob 导入后的裸 `<mod>::`、以及混用。词法层看不出
//! `super` 链的嵌套深度（`mod tests` 里的 `super` 指向本文件而非
//! shared 根），裸名更依赖 glob 导入的作用域——两种形态都会让边
//! **悄悄漏走**，而漏走的边正是环的候选。故口径收敛为：
//!
//! - **边只从 `crate::shared::<mod>` 绝对路径提取**（含 `use …::{A, B}`
//!   组形态，`event → ids` 正是组形态）；
//! - **其它形态只要指向已知模块名就报红**，要求改用绝对路径
//!   （`find_noncanonical_module_refs`）——实测当前 shared 零命中
//!   （`use super::*` 6 处均为根项通配导入，`super::item` 2 处均在
//!   `mod tests` 内指向本文件项），规则不增加现存代码的负担；
//! - **词法真源 = `arch_check::lex_dump`**（与字段级可见性守卫同口径）：
//!   注释与字符串不产生 token。这不是洁癖——`path.rs` 的文档注释里
//!   就写着 `crate::shared::text_format::extension_of`（intra-doc 链接），
//!   用正则扫原文会把文档引用当成真实依赖，phantom 边让守卫常红；
//! - **shared/ 必须保持扁平**（无子目录）：出现子目录即报红，要求先
//!   扩展本守卫——缺口要响，不要静。

// V11-65：workspace lints 为包级声明，覆盖本测试 crate；规范 §七-20
// 允许测试代码 unwrap/expect/panic（main.rs 的 crate 级豁免已罩住
// 本模块，这里不重复写）。
#[allow(dead_code)]
mod arch_check {
    include!("../../tools/arch_check.rs");
}

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

/// 已批准的 `shared` 内部依赖边（破环后的期望集合，2026-10-25）。
///
/// 每条边的实证（引用点）：
///
/// - `dialog → error` / `path` / `text_format`：`AppError` / `FilePath` /
///   `TextFormats`、`BUILTIN_TEXT_EXTENSIONS`（对话框过滤器与白名单同源）；
/// - `event → error` / `geometry` / `ids`：事件载荷只带 ID 与标量
///   （模块文档的设计约束）；
/// - `image_asset → path` / `result`：`FilePath` 定位资源、`AppResult` 归口错误；
/// - `result → error`：`AppResult` 是 `AppError` 的别名出口；
/// - `text → geometry`：`floor_char_boundary` 防 UTF-8 切片 panic；
/// - `text_format → path`：原始路径串的扩展名判定经 `file_name_of` 与
///   `extension_index`（后者即二环的破环迁移点）。
///
/// 增删规则：**新增 = shared 内部依赖面增长**，先证明必要性（谁依赖谁、
/// 为什么不能在调用侧解决）再登记；**删除 = 破环成果**，同样落清单
/// （如 `path → text_format` 边的消失记录在 git 历史）。
const EXPECTED_EDGES: &[(&str, &str)] = &[
    ("dialog", "error"),
    ("dialog", "path"),
    ("dialog", "text_format"),
    ("event", "error"),
    ("event", "geometry"),
    ("event", "ids"),
    ("image_asset", "path"),
    ("image_asset", "result"),
    ("result", "error"),
    ("text", "geometry"),
    ("text_format", "path"),
];

/// shared 源码目录（相对本测试 crate 的 manifest，即仓库根）。
///
/// TD-04（2026-10-26）抽出 `rustmark-shared` crate 后目录从
/// `crates/rustmark-core/src/shared` 迁到 `crates/rustmark-shared/src/shared`
/// ——路径前缀 `shared/` 保留（模块树不变），故边提取的 `crate::shared::<mod>`
/// 口径与全部合成用例零改动，只换扫描根。
fn shared_dir() -> PathBuf {
    // `shared` 测试目标属根包，`CARGO_MANIFEST_DIR` 就是仓库根。
    Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/rustmark-shared/src/shared")
}

use std::path::Path;

/// 解析 `lex_dump` 的一行：`{line}:{Kind}({text})` → `(Kind, text)`。
///
/// 非 Ident token 的 text 为空（如 `12:PathSep()`）；行注释/块注释/
/// 字符串在词法层已被剔除，根本不会出现在流里。
fn parse_dump_line(line: &str) -> Option<(String, String)> {
    let rest = line.split_once(':')?.1;
    let open = rest.find('(')?;
    let kind = rest[..open].to_string();
    let text = rest[open + 1..].strip_suffix(')')?.to_string();
    Some((kind, text))
}

/// `lex_dump` 输出 → token 流 `(Kind, text)`。
fn parse_tokens(dump: &str) -> Vec<(String, String)> {
    dump.lines().filter_map(parse_dump_line).collect()
}

/// 一条路径链：`seg0 :: seg1 :: …`，以及可选的尾部 `::{A, B}` 组。
///
/// 与 `arch_check` 的 `collect_candidates` 同思路：链 = Ident
/// (PathSep Ident)*；链尾若接 `PathSep LBrace`，则组内 Ident 全收
/// （`use crate::shared::ids::{A, B}` 的组）。
struct Chain {
    segs: Vec<String>,
    group: Vec<String>,
}

/// 把 token 流切成路径链。
///
/// 每个 Ident 都尝试起链，故关键字（`use` / `pub` / `fn`…）也会产出
/// 单段链——无害：分类时只认 `crate` / `super` / 已知模块名开头。
fn chains(tokens: &[(String, String)]) -> Vec<Chain> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].0 != "Ident" {
            i += 1;
            continue;
        }
        let mut segs = vec![tokens[i].1.clone()];
        let mut j = i + 1;
        while tokens.get(j).map(|t| t.0.as_str()) == Some("PathSep")
            && tokens.get(j + 1).map(|t| t.0.as_str()) == Some("Ident")
        {
            segs.push(tokens[j + 1].1.clone());
            j += 2;
        }
        // 尾部 `::{ … }` 组（`use crate::shared::{path, result};`）。
        let mut group = Vec::new();
        if tokens.get(j).map(|t| t.0.as_str()) == Some("PathSep")
            && tokens.get(j + 1).map(|t| t.0.as_str()) == Some("LBrace")
        {
            let mut k = j + 2;
            while let Some(t) = tokens.get(k) {
                if t.0 == "RBrace" {
                    break;
                }
                if t.0 == "Ident" {
                    group.push(t.1.clone());
                }
                k += 1;
            }
            j = k;
        }
        out.push(Chain { segs, group });
        i = j.max(i + 1);
    }
    out
}

/// 是否 `crate::shared` 开头的绝对路径链。
fn is_absolute_shared(chain: &Chain) -> bool {
    chain.segs.len() >= 2 && chain.segs[0] == "crate" && chain.segs[1] == "shared"
}

/// 从一段 `lex_dump` 输出里抽取 shared 内部边（目标模块名集合）。
///
/// 只认 `crate::shared::<mod>`（第三段）与 `crate::shared::<mod>::{…}`
/// 之后的 `crate::shared::{<mod>, …}` 组；组内非模块名（类型/函数导入，
/// 如 `ids::{DocumentId, JobId}`）按 `modules` 过滤掉。未知的第三段
/// （笔误）不在这里拦，由 `scan_shared_graph` 的模块集断言收口。
fn extract_edges(dump: &str, modules: &BTreeSet<String>) -> BTreeSet<String> {
    let tokens = parse_tokens(dump);
    let mut edges = BTreeSet::new();
    for chain in chains(&tokens) {
        if !is_absolute_shared(&chain) {
            continue;
        }
        if chain.segs.len() >= 3 {
            edges.insert(chain.segs[2].clone());
        }
        // `use crate::shared::{path, result};` —— 链恰止于 shared，
        // 模块名在组里。
        if chain.segs.len() == 2 {
            for name in &chain.group {
                if modules.contains(name) {
                    edges.insert(name.clone());
                }
            }
        }
    }
    edges
}

/// 收集「非绝对路径形态的 shared 模块引用」。
///
/// 边口径只解析 `crate::shared::<mod>`；`super::<mod>`（含
/// `super::super::<mod>`）、glob 导入后的裸 `<mod>::`、`self::<mod>`
/// 都会让边悄悄漏走，故报红要求改写。判定只抓两种形态，避免误伤：
///
/// - **裸名**：链首就是已知模块名，且被当作限定路径用（≥2 段或带
///   `{…}` 组）——`use path::FilePath`（glob 导入后的裸名）；
/// - **相对链**：`super` / `self` 链的首段之后的任一段或组内出现
///   已知模块名——`use super::path::x`、`use super::{path, result}`。
///
/// 不误伤的形态（shared 现存，实测零命中）：`#[error("…")]` 的属性名
/// （单段链，不是限定符）、`std::error::Error` 的外来链中段、
/// `use super::*` 与 `super::item`（不指向模块名）。
fn find_noncanonical_module_refs(dump: &str, modules: &BTreeSet<String>) -> Vec<String> {
    let tokens = parse_tokens(dump);
    let mut bad = Vec::new();
    for chain in chains(&tokens) {
        if is_absolute_shared(&chain) {
            continue;
        }
        let bare_hit =
            modules.contains(&chain.segs[0]) && (chain.segs.len() >= 2 || !chain.group.is_empty());
        let relative_hit = (chain.segs[0] == "super" || chain.segs[0] == "self")
            && (chain.segs.iter().skip(1).any(|s| modules.contains(s))
                || chain.group.iter().any(|g| modules.contains(g)));
        if bare_hit || relative_hit {
            bad.push(chain.segs.join("::"));
        }
    }
    bad
}

/// 扫描真实工程，产出 shared 内部依赖图（键 = 模块名，值 = 目标模块集）。
fn scan_shared_graph() -> BTreeMap<String, BTreeSet<String>> {
    let dir = shared_dir();

    // 扁平性断言：出现子目录即要求先扩展本守卫（super 链解析按扁平假设）。
    for entry in fs::read_dir(&dir).expect("shared 目录应可读") {
        let path = entry.expect("目录项应可读").path();
        assert!(
            !path.is_dir(),
            "shared/ 出现子目录 {}：请先扩展 internal_cycle_guard 的 \
             节点收集与形态口径",
            path.display()
        );
    }

    let mut modules: BTreeSet<String> = BTreeSet::new();
    for entry in fs::read_dir(&dir).expect("shared 目录应可读") {
        let path = entry.expect("目录项应可读").path();
        if path.extension().map(|e| e == "rs").unwrap_or(false) {
            modules.insert(path.file_stem().unwrap().to_string_lossy().into_owned());
        }
    }
    assert!(!modules.is_empty(), "shared/ 未收集到任何 .rs 模块");

    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for module in &modules {
        let src = fs::read_to_string(dir.join(format!("{module}.rs")))
            .unwrap_or_else(|e| panic!("读取 {module}.rs 失败：{e}"));
        let dump = arch_check::lex_dump(&src);

        let bad = find_noncanonical_module_refs(&dump, &modules);
        assert!(
            bad.is_empty(),
            "{module}.rs 用非绝对路径形态引用 shared 模块 {bad:?}——请改写为 \
             crate::shared::<mod>（本守卫的边口径只解析绝对路径，其它形态会让边漏走）"
        );

        let mut targets = extract_edges(&dump, &modules);
        targets.remove(module); // 自引用不成边
        for target in &targets {
            assert!(
                modules.contains(target),
                "{module}.rs 引用 crate::shared::{target}，但 shared/ 没有对应模块 \
                 文件——笔误，或新模块漏了进节点集"
            );
        }
        graph.insert(module.clone(), targets);
    }
    graph
}

/// DFS 找环；返回环路径（首节点在末尾重复一次，便于阅读）。
///
/// 已完全探索（子树无环）的节点记入 `settled`，避免重复访问；
/// 当前路径上的节点再次出现即环。
fn find_cycle(graph: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    let mut settled: BTreeSet<&str> = BTreeSet::new();
    let mut path: Vec<&str> = Vec::new();

    fn visit<'a>(
        node: &'a str,
        graph: &'a BTreeMap<String, BTreeSet<String>>,
        settled: &mut BTreeSet<&'a str>,
        path: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        if settled.contains(node) {
            return None;
        }
        if path.contains(&node) {
            let start = path.iter().position(|n| *n == node)?;
            let mut ring: Vec<String> = path[start..].iter().map(|s| s.to_string()).collect();
            ring.push(node.to_string());
            return Some(ring);
        }
        path.push(node);
        if let Some(edges) = graph.get(node) {
            for next in edges {
                if let Some(ring) = visit(next.as_str(), graph, settled, path) {
                    return Some(ring);
                }
            }
        }
        path.pop();
        settled.insert(node);
        None
    }

    for node in graph.keys() {
        if let Some(ring) = visit(node.as_str(), graph, &mut settled, &mut path) {
            return Some(ring);
        }
    }
    None
}

/// 把图摊平成 (from, to) 边集。
fn flatten(graph: &BTreeMap<String, BTreeSet<String>>) -> BTreeSet<(String, String)> {
    graph
        .iter()
        .flat_map(|(from, tos)| tos.iter().map(move |to| (from.clone(), to.clone())))
        .collect()
}

// ============================================================================
// 真实工程断言
// ============================================================================

#[test]
fn shared_internal_graph_is_acyclic() {
    let graph = scan_shared_graph();
    if let Some(ring) = find_cycle(&graph) {
        panic!(
            "shared 内部依赖环：{}\n破环方向见 internal_cycle_guard 文件头",
            ring.join(" → ")
        );
    }
}

#[test]
fn shared_internal_edge_set_is_pinned() {
    let graph = scan_shared_graph();
    let actual = flatten(&graph);
    let expected: BTreeSet<(String, String)> = EXPECTED_EDGES
        .iter()
        .map(|(from, to)| ((*from).to_string(), (*to).to_string()))
        .collect();
    if actual != expected {
        let added: Vec<_> = actual.difference(&expected).cloned().collect();
        let removed: Vec<_> = expected.difference(&actual).cloned().collect();
        panic!(
            "shared 内部依赖边与清单不符。\n新增（需评审后补 EXPECTED_EDGES）：{added:?}\n\
             消失（清单过期或边被误删）：{removed:?}"
        );
    }
}

// ============================================================================
// 合成用例：守卫自己的牙口（变异验证的落点）
// ============================================================================

/// 合成模块集（够用例用即可，不必与真实 shared 一致）。
fn synth_modules() -> BTreeSet<String> {
    ["path", "text_format", "geometry", "ids"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[test]
fn cycle_detector_reports_the_ring() {
    // 被破掉的那个二环的形状：path → text_format → path。
    let mut graph = BTreeMap::new();
    graph.insert(
        "path".to_string(),
        BTreeSet::from(["text_format".to_string()]),
    );
    graph.insert(
        "text_format".to_string(),
        BTreeSet::from(["path".to_string()]),
    );
    let ring = find_cycle(&graph).expect("二环必须被报告");
    assert_eq!(ring.len(), 3, "环路径应首尾相接过：{ring:?}");
    assert_eq!(ring.first(), ring.last(), "环路径应闭合：{ring:?}");
}

#[test]
fn cycle_detector_passes_a_dag() {
    // 破环后的真实形状（子集）：text_format → path，path 无出边。
    let mut graph = BTreeMap::new();
    graph.insert("path".to_string(), BTreeSet::new());
    graph.insert(
        "text_format".to_string(),
        BTreeSet::from(["path".to_string()]),
    );
    assert!(find_cycle(&graph).is_none(), "DAG 不应被误报");
}

#[test]
fn cycle_detector_catches_self_loop() {
    let mut graph = BTreeMap::new();
    graph.insert("me".to_string(), BTreeSet::from(["me".to_string()]));
    let ring = find_cycle(&graph).expect("自环必须被报告");
    assert_eq!(ring, vec!["me".to_string(), "me".to_string()]);
}

#[test]
fn extract_edges_ignores_comments_and_strings() {
    // path.rs 的文档注释里就有 `crate::shared::text_format::…` 的
    // intra-doc 链接——注释与字符串不得产生边，否则 phantom 边常红。
    let dump = arch_check::lex_dump(
        "//! 见 [`crate::shared::text_format::extension_of`]\n\
         // use crate::shared::path::file_name_of;\n\
         let s = \"crate::shared::result::AppResult\";\n",
    );
    assert!(
        extract_edges(&dump, &synth_modules()).is_empty(),
        "注释与字符串不应产生边"
    );
}

#[test]
fn extract_edges_reads_absolute_and_group_forms() {
    // 真实形状：`use crate::shared::ids::{DocumentId, JobId};`——
    // 组内是类型导入（非模块名），边仍应只有 ids。
    let dump = arch_check::lex_dump(
        "use crate::shared::path::FilePath;\n\
         use crate::shared::ids::{DocumentId, JobId};\n",
    );
    let modules: BTreeSet<String> = ["path", "ids"].iter().map(|s| s.to_string()).collect();
    assert_eq!(
        extract_edges(&dump, &modules),
        BTreeSet::from(["path".to_string(), "ids".to_string()]),
        "单路径与 {{}} 组形态都应产生边，且组内类型导入不算边"
    );
}

#[test]
fn extract_edges_reads_group_of_modules() {
    // `use crate::shared::{path, result};` —— 链止于 shared，模块名在组里。
    let dump = arch_check::lex_dump("use crate::shared::{path, result};\n");
    let modules: BTreeSet<String> = ["path", "result"].iter().map(|s| s.to_string()).collect();
    assert_eq!(
        extract_edges(&dump, &modules),
        BTreeSet::from(["path".to_string(), "result".to_string()])
    );
}

#[test]
fn noncanonical_forms_are_rejected() {
    let modules = synth_modules();
    // super 链（含嵌套 mod 里的 super::super）与裸名都会让边漏走。
    for src in [
        "use super::path::file_name_of;\n",
        "use super::super::path::file_name_of;\n",
        "use path::FilePath;\n",
        "use self::path::FilePath;\n",
    ] {
        let dump = arch_check::lex_dump(src);
        let bad = find_noncanonical_module_refs(&dump, &modules);
        assert!(!bad.is_empty(), "非绝对路径形态应报红：{src}");
    }
}

#[test]
fn noncanonical_check_spares_items_and_globs() {
    let modules = synth_modules();
    // `use super::*`（根项通配导入）与 `super::item`（mod tests 内自引用）
    // 不指向模块名，不应误伤——shared 现存 8 处即此形态。
    // `#[error("…")]` 属性名单段链（error 恰是模块名）与
    // `std::error::Error` 外来链中段同理——二者是初版守卫的实测误报。
    for src in [
        "use super::*;\n",
        "use super::is_safe_to_open;\n",
        "assert_send_sync::<dyn super::ExternalOpener>();\n",
        "#[error(\"路径不能为空\")]\n",
        "impl std::error::Error for PathError {}\n",
    ] {
        let dump = arch_check::lex_dump(src);
        assert!(
            find_noncanonical_module_refs(&dump, &modules).is_empty(),
            "根项导入与 item 自引用不应报红：{src}"
        );
    }
}
