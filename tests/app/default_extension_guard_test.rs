//! 「默认扩展名」的唯一出处守卫（V11-54a / R-16）。
//!
//! # 为什么需要源码级守卫
//!
//! R-16 要消除的是「另存为建议名 / 新建文件预填名」里**硬编码的 `.md`**。
//! 但这类硬编码有个讨厌的性质：
//!
//! - 在**当前**白名单下（首选 = `md`），硬编码与走常量**行为完全一致**；
//! - 于是任何**行为断言**（"建议名等于 `未命名.md`"）在硬编码退回时
//!   **照样是绿的**——它测不出「漏改」这种只在白名单变化后才显形的问题。
//!
//! 实测记录：把 `suggested_save_name` 改回硬编码 `"md"`，行为用例
//! `default_names_follow_the_whitelist_preferred_extension` **仍然绿**。
//!
//! 唯一能抓住它的办法是**扫源码**：看这些位置到底写的是常量引用
//! 还是字面量。这与 `tools/arch_check.rs` 的思路同源（都是"文本扫描 +
//! 机械判定"），但**不是**架构规则（不涉及依赖方向），故独立成文件，
//! 不塞进 `check-arch`。
//!
//! # 判据
//!
//! 扫 `src/`（排除 `shared/support/text_format.rs` 自身——它是**定义处**，
//! 字面量 `"md"` 在那里是合法的），凡出现「默认扩展名位置硬编码
//! `.md` 字面量」即违规。
//!
//! 匹配的是**具体形态**而非所有 `.md`：
//! - `format!("{stem}.md")` / `format!("未命名.md")` —— 建议名拼接；
//! - `"未命名.md".to_string()` —— 建议名字面量；
//! - `let ext = "md";` —— 抽出变量再拼（变异时用过这个形态）。
//!
//! 合法出现（不违规）：
//! - 测试代码里的 `.md`（测试数据不是产品行为）；
//! - 注释与文档字符串里的 `.md`（说明性文字）；
//! - `shared/support/text_format.rs` 自身（白名单定义处）。

use std::path::{Path, PathBuf};

/// 扫出「默认扩展名位置硬编码 `.md`」的违规。
///
/// 返回 `(相对路径, 行号, 该行内容)` 三元组。
fn scan_hardcoded_default_extension(src_root: &Path) -> Vec<(String, usize, String)> {
    let mut hits = Vec::new();
    for file in collect_rs_files(src_root) {
        let rel = file
            .strip_prefix(src_root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        // 定义处：白名单本身就以 `md` 开头，那里出现字面量是**应该的**。
        if rel == "shared/support/text_format.rs" {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        // 跳过 `#[cfg(test)]` 模块内的代码：那里的 `.md` 是**测试数据**，
        // 不是产品行为（误报会让守卫被加豁免口，规则随即失效）。
        let test_module_start = find_test_module_start(&text);
        for (idx, line) in text.lines().enumerate() {
            if let Some(start) = test_module_start
                && idx + 1 >= start
            {
                break;
            }
            if is_hardcoded_default_extension(line) {
                hits.push((rel.clone(), idx + 1, line.trim().to_string()));
            }
        }
    }
    hits
}

/// 找到 `#[cfg(test)]` 之后的 `mod tests {` 所在行号（1-based），没有则 `None`。
///
/// 只认**顶层的** `#[cfg(test)] mod tests`（缩进为 0）——内联的
/// `#[cfg(test)] mod xxx` 也算，因为它们同样是测试代码。
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

/// 该行是否是「默认扩展名位置硬编码 `.md`」。
///
/// 只认三种具体形态（其余 `.md` 出现不算违规）：
/// 1. `format!(... .md ...)` —— 建议名拼接；
/// 2. `"未命名.md"` —— 建议名字面量；
/// 3. `let ... = "md";` —— 抽出变量（拼接前的中间量）。
fn is_hardcoded_default_extension(line: &str) -> bool {
    let code = strip_comment(line);
    if code.is_empty() {
        return false;
    }
    // 形态 2：字面量 "未命名.md"
    if code.contains("\"未命名.md\"") || code.contains("\"未命名.md") {
        return true;
    }
    // 形态 1：format! 串里带 .md
    if code.contains("format!") && code.contains(".md") {
        // 排除 `format!("{stem}.{ext}")` 这类**已经走常量**的形态：
        // 它含 `.md` 只可能来自别的片段，极少见；这里按「含 `.md{` 或
        // `.md"` 或 `.md\""` 判定，即 `.md` 紧跟字符串结束或占位符。
        if code.contains(".md\"") || code.contains(".md{") {
            return true;
        }
    }
    // 形态 3：`let ext = "md";` 一类
    let trimmed = code.trim_start();
    if trimmed.starts_with("let ")
        && (trimmed.contains("= \"md\";") || trimmed.contains("=\"md\";"))
    {
        return true;
    }
    false
}

/// 去掉行注释与字符串字面量里的内容，避免注释里的 `.md` 被误判。
fn strip_comment(line: &str) -> String {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return String::new();
    }
    // 只做**粗粒度**处理：把 `//` 之后的内容截掉（字符串里出现 `//`
    // 的情况在 Rust 源码里罕见，且本守卫只关心几种明确形态）。
    match line.find("//") {
        Some(pos) => line[..pos].to_string(),
        None => line.to_string(),
    }
}

/// 递归收集 `.rs` 文件（跳过 `target/` 等目录）。
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

/// 四根扫描（D-3 阶段 4a workspace 化 + TD-04 + V11-64 + R-02）：白名单真源
/// `shared/support/text_format.rs` 在 `crates/rustmark-shared/src`（R-02 起落在
/// support 子目录），四层在
/// `crates/rustmark-core/src`，基础设施（文件对话框过滤器等最易硬编码
/// 扩展名的地方）在 `crates/rustmark-infra/src`，组合根与视图在 `src`；
/// 四根都扫才等价于拆分前对单一 `src` 的全量覆盖（豁免按层相对路径匹配，
/// 各根通用；`shared/` `infrastructure/` 前缀随目录保留，豁免键同步搬子目录）。
fn src_roots() -> Vec<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    vec![
        base.join("src"),
        base.join("crates/rustmark-core/src"),
        base.join("crates/rustmark-infra/src"),
        base.join("crates/rustmark-shared/src"),
    ]
}

/// 现状守卫：真实工程里**零**处硬编码默认扩展名。
///
/// 这条用例的价值随时间**递增**：白名单首选一旦调整（新增格式时
/// 把它排到最前），任何退回硬编码的改动都会在这里立刻变红。
#[test]
fn no_source_file_hardcodes_the_default_extension() {
    let mut hits = Vec::new();
    for root in src_roots() {
        hits.extend(scan_hardcoded_default_extension(&root));
    }
    assert!(
        hits.is_empty(),
        "默认扩展名必须走 `shared::text_format::default_extension()`，\
         不许在调用点硬编码。实际违规：\n{}",
        hits.iter()
            .map(|(f, l, c)| format!("  {f}:{l}  {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// 变异验证：注入硬编码必须被检出（证明守卫不是形同虚设）。
///
/// 没有这条，守卫可能因匹配逻辑写错而**永远绿**——那比没有守卫更危险。
#[test]
fn injected_hardcoded_extension_is_detected() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("app/frame")).expect("应可建目录");
    std::fs::write(
        root.join("app/frame/workspace.rs"),
        "fn suggested() -> String {\n    let stem = \"a\";\n    format!(\"{stem}.md\")\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_hardcoded_default_extension(&root);
    assert_eq!(hits.len(), 1, "注入的硬编码应被检出 1 处，实际：{hits:?}");
    assert!(
        hits[0].2.contains(".md"),
        "应报告含硬编码的那一行：{hits:?}"
    );
}

/// 变异验证：字面量形态（`"未命名.md".to_string()`）也要被检出。
#[test]
fn injected_literal_default_name_is_detected() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("presentation/state")).expect("应可建目录");
    std::fs::write(
        root.join("presentation/state/sidebar_ui_state.rs"),
        "fn name() -> String {\n    \"未命名.md\".to_string()\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_hardcoded_default_extension(&root);
    assert_eq!(hits.len(), 1, "注入的字面量应被检出 1 处，实际：{hits:?}");
}

/// 反向验证：**已经走常量**的形态不得被误报。
///
/// 这条比检出用例更重要：守卫若把正确写法也判违规，开发者就会
/// 加豁免逃生口，规则随即失效。
#[test]
fn the_constant_based_form_is_not_flagged() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("app/frame")).expect("应可建目录");
    std::fs::write(
        root.join("app/frame/workspace.rs"),
        concat!(
            "fn suggested() -> String {\n",
            "    let ext = text_format::default_extension();\n",
            "    format!(\"{stem}.{ext}\")\n",
            "}\n",
        ),
    )
    .expect("写 fixture 应成功");

    let hits = scan_hardcoded_default_extension(&root);
    assert!(hits.is_empty(), "走常量的写法不应被误报：{hits:?}");
}

/// 反向验证：注释与文档里的 `.md` 不算违规。
#[test]
fn comments_and_docs_mentioning_md_are_not_flagged() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("app/frame")).expect("应可建目录");
    std::fs::write(
        root.join("app/frame/workspace.rs"),
        concat!(
            "//! 未命名文档给 `未命名.md`：系统对话框不会替我们补后缀。\n",
            "// 另存为建议名固定带 .md 后缀（历史说明）\n",
            "fn suggested() -> String {\n",
            "    let ext = text_format::default_extension();\n",
            "    format!(\"{stem}.{ext}\")\n",
            "}\n",
        ),
    )
    .expect("写 fixture 应成功");

    let hits = scan_hardcoded_default_extension(&root);
    assert!(hits.is_empty(), "注释里的 .md 不应被判违规：{hits:?}");
}
