//! 「新增一种主题种类只改 N 处」的源码级守卫（R-09）。
//!
//! # 为什么需要源码级守卫
//!
//! R-09 收敛掉的 `BuiltinTheme` 是**两份同构枚举靠字符串对齐**。
//! 那种结构没有任何机制阻止它复活：任何人新增一个枚举、写一份自己的
//! `as_str()`，编译器不会吭声，而两份枚举会从「刚建时一致」慢慢漂移到
//! 「不一致」——**没有任何报错**，用户看到的是「选了深色却显示浅色」。
//!
//! 行为断言抓不住这类漂移：两份枚举**刚建时行为完全一致**，
//! 于是任何行为用例在漂移前都是绿的。要抓住「又多了一份定义」，
//! 只能**扫源码**（与 `tools/arch_check.rs` 的思路同源，但这是
//! 「同一概念只有一处定义」的规则，不涉及依赖方向，故独立成文件）。
//!
//! # 判据
//!
//! 扫 `src/`：
//!
//! - **主题种类枚举只有一处定义**：`pub enum <名字>` 且带
//!   `Light` / `Dark` / `System` / `Custom` 四个变体之一为特征。
//!   真源 `shared/theme.rs` 豁免。
//! - **主题字符串不得硬编码在别处**：`"light"` / `"dark"` /
//!   `"system"` / `"custom"` 这四个**持久化契约值**只许出现在真源、
//!   领域侧的 serde 属性、以及**明确标注为「比较/判断」**的位置。
//!
//! # 判据二为什么不做成「零命中」
//!
//! 「比较某个已知字符串」是**合法**用法（如设置页判断
//! `view.kind == "system"`）。一刀切禁掉会把正常代码也判违规，
//! 守卫随即被加豁免逃生口，规则失效。故判据二只在**与主题种类
//! 无关**的位置**豁免**真源、serde 属性与注释，
//! 其余位置要求走 `ThemeKind::as_str()`。

use std::path::{Path, PathBuf};

/// 真源文件（相对 `src/` 的路径）。
const SOURCE_OF_TRUTH: &str = "shared/theme.rs";

/// 领域侧的转出文件（`pub use` 真源，不算第二份定义）。
const DOMAIN_REEXPORT_FILE: &str = "domain/preferences/value_objects/theme.rs";

/// 主题种类的**特征变体**。
///
/// 取四个互不相关的代表变体，要求**至少命中两个**才判定为
/// 「主题种类枚举」——单个 `Light` 可能来自别处（如 `Light` 方向键、
/// 亮色遮罩），但四个里命中两个，基本只可能是主题种类。
const SIGNATURE_VARIANTS: &[&str] = &["Light", "Dark", "System", "Custom"];

/// 主题种类的**持久化契约值**。
const CONTRACT_STRINGS: &[&str] = &["light", "dark", "system", "custom"];

/// 扫出「主题种类枚举的多余定义」。
///
/// 返回 `(相对路径, 行号, 该行内容)` 三元组。
fn scan_duplicate_theme_enums(src_root: &Path) -> Vec<(String, usize, String)> {
    let mut hits = Vec::new();
    for file in collect_rs_files(src_root) {
        let rel = file
            .strip_prefix(src_root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        // 真源处合法；领域侧是 `pub use` 转出，不是第二份定义。
        if rel == SOURCE_OF_TRUTH || rel == DOMAIN_REEXPORT_FILE {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            let Some(name) = trimmed
                .strip_prefix("pub enum ")
                .or_else(|| trimmed.strip_prefix("pub(crate) enum "))
                .or_else(|| trimmed.strip_prefix("enum "))
            else {
                continue;
            };
            let name = name
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("");
            if !looks_like_a_theme_enum(name) && !has_theme_variants(&text) {
                continue;
            }
            if !has_theme_variants(&text) {
                continue;
            }
            hits.push((rel.clone(), idx + 1, trimmed.to_string()));
        }
    }
    hits
}

/// 该文件里是否出现主题种子的**特征变体**。
fn has_theme_variants(text: &str) -> bool {
    SIGNATURE_VARIANTS
        .iter()
        .filter(|variant| text.contains(*variant))
        .count()
        >= 2
}

/// 枚举名是否像「主题种类」。
fn looks_like_a_theme_enum(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    name.contains("Theme")
}

/// 扫出「主题持久化契约值被硬编码」的位置。
///
/// 返回 `(相对路径, 行号, 该行内容)` 三元组。
///
/// 豁免：
/// - 真源（定义处）；
/// - 领域转出文件（`pub use`）；
/// - 注释与文档字符串；
/// - `#[serde(rename_all = "snake_case")]` 这类 serde 属性行
///   （那里的字面量是**属性值**而非比较值）；
/// - 表现为 `view.kind == "system"` 这类**比较**（含 `==` / `!=` /
///   `eq_ignore_ascii_case`）——它们在**读**契约值，不是在**定义**它；
/// - `#[cfg(test)]` 模块内的代码——那里的字面量是**测试数据**，
///   不是产品行为（误报会让守卫被加豁免口，规则随即失效）。
fn scan_hardcoded_theme_strings(src_root: &Path) -> Vec<(String, usize, String)> {
    let mut hits = Vec::new();
    for file in collect_rs_files(src_root) {
        let rel = file
            .strip_prefix(src_root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel == SOURCE_OF_TRUTH || rel == DOMAIN_REEXPORT_FILE {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let test_module_start = find_test_module_start(&text);
        for (idx, line) in text.lines().enumerate() {
            if let Some(start) = test_module_start
                && idx + 1 >= start
            {
                break;
            }
            let code = strip_comment(line);
            if code.trim().is_empty() {
                continue;
            }
            // serde 属性行：字面量是属性值。
            if code.trim_start().starts_with("#[serde") {
                continue;
            }
            // 比较 / 判断：这些位置在**读**契约值，不是在**定义**它。
            if is_comparison_context(&code) {
                continue;
            }
            for contract in CONTRACT_STRINGS {
                if code.contains(&format!("\"{contract}\"")) {
                    hits.push((rel.clone(), idx + 1, code.trim().to_string()));
                    break;
                }
            }
        }
    }
    hits
}

/// 找到 `#[cfg(test)]` 之后的 `mod tests {` 所在行号（1-based），没有则 `None`。
///
/// 与 R-07 的 `export_format_guard_test` 同款判据：只认**顶层的**
/// `#[cfg(test)] mod xxx`（缩进为 0）。
fn find_test_module_start(text: &str) -> Option<usize> {
    let mut in_cfg_test = false;
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[cfg(test)]") {
            in_cfg_test = true;
            continue;
        }
        if in_cfg_test {
            if trimmed.starts_with("mod ") {
                return Some(idx + 1);
            }
            // `#[cfg(test)]` 后若不是 `mod`（例如是 `use`），继续等。
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("use ") {
                continue;
            }
            in_cfg_test = false;
        }
    }
    None
}

/// 该行是否是「比较 / 判断」形态。
///
/// 判据：含 `==` / `!=` / `eq_ignore_ascii_case` / `starts_with` /
/// `contains` 之一（后两者用于 `view.kind` 的前缀判断）。
fn is_comparison_context(code: &str) -> bool {
    [
        "==",
        "!=",
        "eq_ignore_ascii_case",
        "starts_with",
        "contains",
    ]
    .iter()
    .any(|marker| code.contains(marker))
}

/// 去掉行注释内容。
fn strip_comment(line: &str) -> String {
    match line.find("//") {
        Some(pos) => line[..pos].to_string(),
        None => line.to_string(),
    }
}

/// 递归收集 `.rs` 文件。
fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(collect_rs_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

/// 三根扫描（D-3 阶段 4a workspace 化 + V11-62）：四层与非视图子模块在
/// `crates/rustmark-core/src`，组合根与视图在 `src`，从 core 抽出的
/// `rustmark-ui` 非视图子模块（widgets / state / shortcuts / theme）在
/// `crates/rustmark-ui/src`。三根都扫，才等价于拆分前对单一 `src` 的
/// 全量覆盖；各根内均按层相对路径记键。
fn src_roots() -> Vec<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    vec![
        base.join("src"),
        base.join("crates/rustmark-core/src"),
        base.join("crates/rustmark-ui/src"),
    ]
}

/// 现状守卫：主题种类**只有一处**定义。
///
/// 这条用例的价值随时间**递增**：只要有人再写一份同构枚举
/// （不管是改名叫 `BuiltinTheme` 还是 `ThemeOption`），
/// 它立刻变红。
#[test]
fn theme_kind_has_exactly_one_definition() {
    let mut hits = Vec::new();
    for root in src_roots() {
        hits.extend(scan_duplicate_theme_enums(&root));
    }
    assert!(
        hits.is_empty(),
        "主题种类只能有一处定义（`{SOURCE_OF_TRUTH}`）。\
         实际多出的定义：\n{}",
        hits.iter()
            .map(|(f, l, c)| format!("  {f}:{l}  {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// 真源确实存在（否则上一条会因为「扫不到任何定义」而空过）。
#[test]
fn the_single_source_of_truth_exists() {
    let path = src_roots()
        .iter()
        .map(|root| root.join(SOURCE_OF_TRUTH))
        .find(|p| p.exists());
    let Some(path) = path else {
        panic!("真源文件 {SOURCE_OF_TRUTH} 必须存在于 src 或 core src，否则主题种类无处安放");
    };
    let text = std::fs::read_to_string(&path).expect("真源应可读");
    assert!(
        text.contains("pub enum ThemeKind"),
        "真源里必须有 `pub enum ThemeKind`"
    );
}

/// 变异验证：注入一份同构枚举必须被检出。
#[test]
fn injected_duplicate_theme_enum_is_detected() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("presentation/theme")).expect("应可建目录");
    std::fs::write(
        root.join("presentation/theme/kind.rs"),
        "pub enum BuiltinTheme {\n    Light,\n    Dark,\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_duplicate_theme_enums(&root);
    assert_eq!(hits.len(), 1, "注入的同构枚举应被检出 1 处，实际：{hits:?}");
}

/// 变异验证：换个名字重建同构枚举，同样必须被检出。
///
/// 上一条只认得「变体像」；这一条证明守卫认的是
/// 「**主题种类枚举**」这个概念，而不是某个具体名字。
#[test]
fn injected_renamed_theme_enum_is_detected() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("presentation/theme")).expect("应可建目录");
    std::fs::write(
        root.join("presentation/theme/option.rs"),
        "pub enum ThemeOption {\n    Light,\n    Dark,\n    System,\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_duplicate_theme_enums(&root);
    assert_eq!(hits.len(), 1, "换名的同构枚举应被检出 1 处，实际：{hits:?}");
}

/// 反向验证：真源处**不得**被误报。
#[test]
fn the_source_of_truth_is_not_flagged() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("shared")).expect("应可建目录");
    std::fs::write(
        root.join("shared/theme.rs"),
        "pub enum ThemeKind {\n    Light,\n    Dark,\n    System,\n    Custom,\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_duplicate_theme_enums(&root);
    assert!(hits.is_empty(), "真源不应被误报：{hits:?}");
}

/// 现状守卫：主题的持久化契约值**不得**在别处硬编码。
///
/// 契约值写错的后果是「用户的主题选不中」——**编译不报错、运行不报错**，
/// 只是界面上那个选项点了没反应。
#[test]
fn theme_contract_strings_are_not_hardcoded_elsewhere() {
    let mut hits = Vec::new();
    for root in src_roots() {
        hits.extend(scan_hardcoded_theme_strings(&root));
    }
    assert!(
        hits.is_empty(),
        "主题的持久化契约值（light/dark/system/custom）必须走 \
         `ThemeKind::as_str()`，不许在调用点硬编码。实际硬编码：\n{}",
        hits.iter()
            .map(|(f, l, c)| format!("  {f}:{l}  {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// 变异验证：注入硬编码必须被检出。
#[test]
fn injected_hardcoded_theme_string_is_detected() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("app/frame")).expect("应可建目录");
    std::fs::write(
        root.join("app/frame/actions.rs"),
        "fn theme_kind() -> &'static str {\n    \"dark\"\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_hardcoded_theme_strings(&root);
    assert_eq!(hits.len(), 1, "注入的硬编码应被检出 1 处，实际：{hits:?}");
}

/// 反向验证：**比较**契约值不算违规。
///
/// `view.kind == "system"` 是**读**契约值（判断用户选了什么），
/// 不是**定义**它。若把这种用法也判违规，守卫会被加豁免逃生口，
/// 规则随即失效。
#[test]
fn comparing_a_contract_string_is_not_flagged() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("presentation/views")).expect("应可建目录");
    std::fs::write(
        root.join("presentation/views/settings.rs"),
        concat!(
            "fn is_system(kind: &str) -> bool {\n",
            "    kind == \"system\"\n",
            "}\n",
        ),
    )
    .expect("写 fixture 应成功");

    let hits = scan_hardcoded_theme_strings(&root);
    assert!(hits.is_empty(), "比较契约值不应被判违规：{hits:?}");
}

/// 反向验证：注释与 serde 属性行里的字面量不算违规。
#[test]
fn comments_and_serde_attributes_are_not_flagged() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("domain/preferences")).expect("应可建目录");
    std::fs::write(
        root.join("domain/preferences/theme.rs"),
        concat!(
            "//! 主题种类：light / dark / system / custom。\n",
            "#[serde(rename_all = \"snake_case\")]\n",
            "pub struct Theme;\n",
        ),
    )
    .expect("写 fixture 应成功");

    let hits = scan_hardcoded_theme_strings(&root);
    assert!(hits.is_empty(), "注释与 serde 属性不应被判违规：{hits:?}");
}
