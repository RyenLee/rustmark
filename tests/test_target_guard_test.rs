//! 测试目标登记守卫（D-6）：`tests/` 下的每个测试入口都必须被 `[[test]]` 登记。
//!
//! # 为什么需要这条守卫
//!
//! 根包的 `Cargo.toml` 设了 `autotests = false`——这是**故意**的（测试入口要能
//! 显式命名、按层分组），但它带来一个静默失效模式：新增 `tests/<dir>/main.rs`
//! 却忘了写 `[[test]]` 时，cargo **不会**报错，那份测试**根本不会被编译**，
//! 于是全量测试依旧全绿，覆盖率却悄悄掉了。
//!
//! 「跑了失败」是响的，「压根没跑」是哑的。本守卫把后者变成红的。
//!
//! # 判据（四条，双向）
//!
//! 1. 每条 `[[test]]` 的 `path` 必须存在——防死条目（删了文件忘了删登记）；
//! 2. `tests/**/main.rs` 必须全部被登记——防新增目录静默不跑；
//! 3. 顶层 `tests/*.rs` 必须被登记**或**在 [`EXEMPT`] 里——防新增单文件目标
//!    静默不跑；
//! 4. 豁免清单里的每一条都必须真实存在，且不得是已登记的目标——防「过时豁免」
//!    长期掩盖一个已被改名/删除的模块。

// 测试代码允许 panic / expect（规范 §七-20）：本文件读 manifest 与目录树，
// 读不到就是环境坏了，继续跑没有意义——应让守卫当场响，而不是静默跳过。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// 仓库根（集成测试的 cwd 不保证是根，用编译期常量最稳）。
const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// 不是测试目标、只作为模块被 `mod` 进来的路径。
///
/// 每一条都要写清理由：豁免清单本身就是文档，新增条目必须解释「为什么它不是
/// 一个独立目标」。清单双向断言（见模块文档第 4 条），不会腐烂。
const EXEMPT: &[(&str, &str)] = &[
    (
        "tests/support.rs",
        "集成测试共享模块：各目标以 `#[path = \"../support.rs\"] mod support;` 引入",
    ),
    (
        "tests/support",
        "`support.rs` 的子模块目录（clock / keypad / text_buffer 等）",
    ),
    (
        "tests/common",
        "共享夹具（fakes / golden_fixture），由各目标 `mod` 引入",
    ),
    ("tests/data", "测试数据文件（非 Rust 源）"),
];

/// 一条 `[[test]]` 声明。
struct Declared {
    name: String,
    path: String,
}

/// 解析根 `Cargo.toml` 里的 `[[test]]` 块。
///
/// 刻意手写而不用 toml 依赖：只读 `name` / `path` 两个键，且 manifest 是人工
/// 维护的固定格式；引入一个解析库来读两个字段不划算（体积红线，见 `metrics.md` §2）。
fn declared_targets() -> Vec<Declared> {
    let manifest =
        fs::read_to_string(Path::new(ROOT).join("Cargo.toml")).expect("应能读到根 manifest");
    let mut out = Vec::new();
    let mut current: Option<Declared> = None;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed == "[[test]]" {
            if let Some(done) = current.take() {
                out.push(done);
            }
            current = Some(Declared {
                name: String::new(),
                path: String::new(),
            });
            continue;
        }
        // 任何其他 section 头都表示本 `[[test]]` 块结束。
        if trimmed.starts_with('[') {
            if let Some(done) = current.take() {
                out.push(done);
            }
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        if let Some(value) = trimmed.strip_prefix("name = ") {
            entry.name = value.trim().trim_matches('"').to_string();
        } else if let Some(value) = trimmed.strip_prefix("path = ") {
            entry.path = value.trim().trim_matches('"').to_string();
        }
    }
    if let Some(done) = current.take() {
        out.push(done);
    }
    out
}

/// `tests/` 下所有「看起来是测试入口」的路径：目录里的 `main.rs` 与顶层 `.rs`。
fn entry_points() -> Vec<String> {
    let tests = Path::new(ROOT).join("tests");
    let mut out = Vec::new();
    let mut dirs: Vec<PathBuf> = fs::read_dir(&tests)
        .expect("tests/ 应存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    dirs.sort();
    for path in dirs {
        if path.is_dir() {
            if path.join("main.rs").is_file() {
                out.push(format!("tests/{}/main.rs", name_of(&path)));
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(format!("tests/{}", name_of(&path)));
        }
    }
    out
}

/// 取路径最后一段（`Path::file_name` 在非 UTF-8 下会返回 `None`，测试目录不该有这种名字）。
fn name_of(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| panic!("路径应有 UTF-8 文件名：{}", path.display()))
        .to_string()
}

/// `tests/` 下的顶层目录名（用于「目录型入口必须含 main.rs 或进豁免」）。
fn top_level_dirs() -> Vec<String> {
    let tests = Path::new(ROOT).join("tests");
    let mut out: Vec<String> = fs::read_dir(&tests)
        .expect("tests/ 应存在")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .map(|p| format!("tests/{}", name_of(&p)))
        .collect();
    out.sort();
    out
}

#[test]
fn every_declared_target_path_exists() {
    let missing: Vec<String> = declared_targets()
        .into_iter()
        .filter(|d| !Path::new(ROOT).join(&d.path).is_file())
        .map(|d| format!("{} → {}", d.name, d.path))
        .collect();
    assert!(
        missing.is_empty(),
        "这些 [[test]] 登记指向不存在的文件（删了测试忘了删登记？）：{missing:#?}"
    );
}

#[test]
fn every_test_entry_point_is_declared() {
    let declared: BTreeSet<String> = declared_targets().into_iter().map(|d| d.path).collect();
    let exempt: BTreeSet<&str> = EXEMPT.iter().map(|(path, _)| *path).collect();
    let undeclared: Vec<String> = entry_points()
        .into_iter()
        .filter(|p| !declared.contains(p) && !exempt.contains(p.as_str()))
        .collect();
    assert!(
        undeclared.is_empty(),
        "这些测试入口没有 [[test]] 登记——`autotests = false` 下它们**不会被编译**，\
         全量测试仍会全绿（静默缺席）：{undeclared:#?}\n\
         要么在根 Cargo.toml 补 `[[test]] name/path`，要么（若它只是被 mod 引入的共享模块）\
         加进本文件 EXEMPT 并写明理由。"
    );
}

#[test]
fn every_test_directory_is_either_a_target_or_exempt() {
    let declared: BTreeSet<String> = declared_targets()
        .into_iter()
        .filter_map(|d| d.path.rsplit_once('/').map(|(dir, _)| dir.to_string()))
        .collect();
    let exempt: BTreeSet<&str> = EXEMPT.iter().map(|(path, _)| *path).collect();
    let stray: Vec<String> = top_level_dirs()
        .into_iter()
        .filter(|dir| !declared.contains(dir) && !exempt.contains(dir.as_str()))
        .collect();
    assert!(
        stray.is_empty(),
        "这些 tests/ 顶层目录既不是测试目标（缺 main.rs 或未登记）也不在豁免清单：{stray:#?}"
    );
}

#[test]
fn exemptions_are_real_and_not_declared_targets() {
    let declared: BTreeSet<String> = declared_targets().into_iter().map(|d| d.path).collect();
    for (path, reason) in EXEMPT {
        assert!(
            Path::new(ROOT).join(path).exists(),
            "豁免清单里的 `{path}` 已不存在——过时豁免会长期掩盖改名/删除（理由：{reason}）"
        );
        assert!(
            !declared.contains(*path),
            "`{path}` 已登记为测试目标，却仍留在豁免清单里（两边打架）"
        );
    }
}

#[test]
fn target_names_and_paths_are_unique() {
    let declared = declared_targets();
    let names: BTreeSet<&str> = declared.iter().map(|d| d.name.as_str()).collect();
    let paths: BTreeSet<&str> = declared.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(names.len(), declared.len(), "[[test]] 的 name 有重复");
    assert_eq!(paths.len(), declared.len(), "[[test]] 的 path 有重复");
}
