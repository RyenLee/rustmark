//! 「新增一个快捷键动作要改几处」的源码级守卫（R-08①）。
//!
//! # 为什么需要源码级守卫
//!
//! R-08① 把 `UiAction::persisted_key` 从「`_` 兜底」改成**穷举 `match`**，
//! 收益是：新增 `UiAction` 变体时**编译失败**，作者必须当场表态
//! 「它能不能持久化」。这一半是**编译器**给的，不需要用例。
//!
//! 但「新增一个动作要改 N 处」这件事上，仍有两处**编译器管不到**：
//!
//! 1. **兜底分支**：`match self { … _ => return None }` 一旦写回，
//!    编译就重新变绿——新动作又落回「默认不可持久化」且**零报错**。
//!    编译器不会说「你少表态了一个」。
//! 2. **手写清单**：`UiAction::ALL` 与领域 `ActionId::all()` 都是
//!    **手写数组**（`const fn` / `const` 返回 `&[_]`），加变体忘了加进
//!    清单 → 该动作**不出现在命令面板**、**不可改键**，而**零报错**。
//!    `assert_eq!(UiAction::ALL.len(), 57)` 只能钉住「长度」，
//!    钉不住「清单与枚举是否同步」——因为它两边都数不出「枚举变体数」
//!    （`std::mem::variant_count` 在 Rust 1.99 仍是 unstable）。
//!
//! 要抓住这两类，只能**扫源码**：看 `persisted_key` 到底穷举了没有、
//! `ALL` 清单到底列全了没有。这与 `tools/arch_check.rs` 的思路同源
//! （都是「文本扫描 + 机械判定」），但不是架构规则，故独立成文件。
//!
//! # 判据
//!
//! 扫全部源码根（`src` / `crates/rustmark-core/src` /
//! `crates/rustmark-actions/src`，见 `src_roots`）：
//!
//! - **`persisted_key` 必须穷举**：其函数体内不得出现 `_ =>` 兜底分支
//!   （任何形态：`_ => return None` / `_ => None` / `_ => …`）。
//! - **`ALL` 清单必须与枚举变体同步**：`UiAction` 枚举体里出现的每个
//!   变体名，都必须出现在 `UiAction::ALL` 清单里（反之亦然）。
//! - **领域侧同理**：`ActionId` 枚举体与 `ActionId::all()` 清单同步。

use std::path::{Path, PathBuf};

/// 表现层动作枚举的定义处。
///
/// V11-57：`UiAction` 词汇表随零依赖契约 crate `rustmark-actions` 抽出，
/// 从 `presentation/shortcuts/registry/action.rs` 迁到
/// `crates/rustmark-actions/src/action.rs`。路径锚点必须跟着搬——
/// 否则本守卫的「非空断言」会以 `.expect` panic 的形式报出来，
/// 而 `persisted_key` 兜底扫描也会**静默丢失**这一文件的覆盖。
const UI_ACTION_ENUM_FILE: &str = "action.rs";

/// 表现层 `ALL` 清单与 `persisted_key` 的定义处。
///
/// V11-57：随词汇表一起迁入 `crates/rustmark-actions/src/action/impls.rs`
/// （原 `presentation/shortcuts/registry/action/impls.rs`）。
const UI_ACTION_IMPLS_FILE: &str = "action/impls.rs";

/// 领域动作枚举的定义处。
const DOMAIN_ACTION_FILE: &str = "domain/preferences/value_objects/action.rs";

/// 扫出「`persisted_key` 里的兜底分支」。
///
/// 返回 `(相对路径, 行号, 该行内容)` 三元组。
///
/// # 为什么扫全仓而不是只扫定义处
///
/// `UiAction::persisted_key` 只能有一处定义（同名固有方法重复定义是
/// 编译错误 E0592），故只扫定义处**够用**。但若将来出现**另一个类型**
/// 也叫 `persisted_key`（例如领域侧补一个同名字段），那处若也写成
/// 兜底，本守卫就管不到了。故扫全仓：凡叫 `persisted_key` 的函数，
/// 一律不许有 `_ =>` 兜底。
fn scan_persisted_key_fallback(src_root: &Path) -> Vec<(String, usize, String)> {
    let mut hits = Vec::new();
    for file in collect_rs_files(src_root) {
        let rel = file
            .strip_prefix(src_root)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        // 文件里没有 `fn persisted_key` 就跳过（绝大多数文件）。
        if !text.contains("fn persisted_key") {
            continue;
        }
        // 一个文件里可能有多个 `fn persisted_key`（不同类型各一），
        // 逐个取出函数体检查。
        let mut search_from = 0usize;
        while let Some(offset) = text[search_from..].find("fn persisted_key") {
            let start = search_from + offset;
            let Some((body_start, body_end)) = extract_fn_body_span(&text, start) else {
                break;
            };
            let body = &text[body_start..=body_end];
            // 报告的行号必须是**文件内**的真实行号——
            // 函数体内的相对行号会把人引到错误的位置。
            let first_line = text[..body_start].lines().count();
            for (idx, line) in body.lines().enumerate() {
                let code = strip_comment(line);
                let trimmed = code.trim();
                // 兜底分支的形态：`_ =>`。注释里的不算（`strip_comment` 已去）。
                if trimmed.starts_with("_ =>") || trimmed.starts_with("_=>") {
                    hits.push((rel.clone(), first_line + idx + 1, trimmed.to_string()));
                }
            }
            search_from = body_end + 1;
        }
    }
    hits
}

/// 取出从 `decl_start` 处开始的函数的函数体**字节区间**（`{` … `}`）。
///
/// 返回 `(花括号起点, 花括号终点)`，两者都是**原文字符串**里的字节下标。
///
/// 不用「先找 `fn <name>` 再找 `{`」的两段式：那会在调用方已定位到
/// 声明处时二次匹配到别处。调用方直接给出声明位置，本函数从那里往后
/// 找第一个 `{` 即可。
fn extract_fn_body_span(text: &str, decl_start: usize) -> Option<(usize, usize)> {
    let open = text[decl_start..].find('{')? + decl_start;
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, open + offset));
                }
            }
            _ => {}
        }
    }
    None
}

/// 提取枚举的变体名列表。
///
/// 从 `pub enum <name>` 处开始，取到花括号配平的 `}`，再逐行收集形如
/// `Variant,` / `Variant {` / `Variant(` 的行首标识符。
fn extract_enum_variants(text: &str, enum_name: &str) -> Vec<String> {
    let Some(decl) = text.find(&format!("enum {enum_name}")) else {
        return Vec::new();
    };
    let rest = &text[decl..];
    let Some(open) = rest.find('{') else {
        return Vec::new();
    };
    let bytes = rest.as_bytes();
    let mut depth = 0usize;
    let mut end = rest.len();
    for (idx, byte) in bytes.iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = idx;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &rest[open + 1..end];
    let mut variants = Vec::new();
    for line in body.lines() {
        let code = strip_comment(line);
        let trimmed = code.trim();
        // 跳过空行、属性（`#[…]`）、文档注释残留。
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // 取行首标识符：`Variant` / `Variant,` / `Variant {` / `Variant(`。
        let ident: String = trimmed
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if ident.is_empty() {
            continue;
        }
        variants.push(ident);
    }
    variants
}

/// 提取 `ALL` 清单里列出的动作名。
///
/// 从 `pub const ALL` 处开始，取到 `&[` 与 `]` 之间的内容，逐项收集
/// `UiAction::Variant` 形态的变体名。
fn extract_all_list_entries(text: &str) -> Vec<String> {
    let Some(decl) = text.find("pub const ALL") else {
        return Vec::new();
    };
    let rest = &text[decl..];
    let Some(open) = rest.find("&[") else {
        return Vec::new();
    };
    let Some(close) = rest[open..].find(']') else {
        return Vec::new();
    };
    let body = &rest[open + 2..open + close];
    let mut entries = Vec::new();
    for line in body.lines() {
        let code = strip_comment(line);
        let trimmed = code.trim();
        if trimmed.is_empty() {
            continue;
        }
        // 形如 `UiAction::NewDocument,`
        let Some((_, variant)) = trimmed.split_once("::") else {
            continue;
        };
        let name: String = variant
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            entries.push(name);
        }
    }
    entries
}

/// 提取 `fn all()` 清单里列出的变体名（领域侧形态）。
///
/// 领域 `ActionId::all()` 的形态与表现层**不同**：
/// - 是 `pub const fn all() -> &'static [ActionId] { &[ … ] }`（**函数**
///   而非 `const`），清单项写作 `Self::NewDocument`（`Self` 而非 `ActionId`）。
///
/// 这两处差异都足以让「按表现层形态写的提取器」**扫不到任何东西**——
/// 而「扫不到」在守卫里表现为**假绿**（断言两侧都非空的检查会挡住它，
/// 但那要求提取器与判据配套改，脆弱）。故领域侧单列一个提取器。
fn extract_fn_all_list_entries(text: &str) -> Vec<String> {
    let Some(decl) = text.find("pub const fn all()") else {
        return Vec::new();
    };
    let rest = &text[decl..];
    let Some(open) = rest.find("&[") else {
        return Vec::new();
    };
    let Some(close) = rest[open..].find(']') else {
        return Vec::new();
    };
    let body = &rest[open + 2..open + close];
    let mut entries = Vec::new();
    for line in body.lines() {
        let code = strip_comment(line);
        let trimmed = code.trim();
        if trimmed.is_empty() {
            continue;
        }
        // 形如 `Self::NewDocument,`
        let Some((_, variant)) = trimmed.split_once("::") else {
            continue;
        };
        let name: String = variant
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            entries.push(name);
        }
    }
    entries
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

/// 四根扫描（D-3 阶段 4a workspace 化 + V11-57 抽 actions crate + V11-64 抽
/// infra crate）：四层与非视图子模块在 `crates/rustmark-core/src`，组合根与
/// 视图在 `src`，UiAction 词汇表在 `crates/rustmark-actions/src`，
/// 基础设施在 `crates/rustmark-infra/src`（当前无 `persisted_key` 定义，
/// 但扫描面与拆分前的单一 `src` 保持一致——搬迁不该静默收窄守卫范围）；
/// 四根都扫才等价于拆分前对单一 `src` 的全量覆盖（各根内按层相对路径记键）。
fn src_roots() -> Vec<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    vec![
        base.join("src"),
        base.join("crates/rustmark-core/src"),
        base.join("crates/rustmark-actions/src"),
        base.join("crates/rustmark-infra/src"),
    ]
}

/// 按层相对路径在两根中读文件（以第一个存在的为准）。
fn read_from_roots(rel: &str) -> Option<String> {
    src_roots()
        .iter()
        .find_map(|root| std::fs::read_to_string(root.join(rel)).ok())
}
/// 现状守卫：`persisted_key` **没有兜底分支**。
///
/// 这条是 R-08① 的核心判据。兜底分支一旦写回，编译重新变绿，
/// 而「新动作默认不可持久化」这一语义漂移**没有任何报错**。
#[test]
fn persisted_key_has_no_fallback_branch() {
    let mut hits = Vec::new();
    for root in src_roots() {
        hits.extend(scan_persisted_key_fallback(&root));
    }
    assert!(
        hits.is_empty(),
        "`UiAction::persisted_key` 必须穷举全部变体，不许有 `_ =>` 兜底分支——\
         兜底会让「新增一个动作」编译通过但语义未定。实际兜底：\n{}",
        hits.iter()
            .map(|(f, l, c)| format!("  {f}:{l}  {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// 变异验证：注入兜底分支必须被检出。
///
/// 没有这条，守卫可能因匹配逻辑写错而**永远绿**——那比没有守卫更危险。
#[test]
fn injected_fallback_branch_is_detected() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    let impls = root.join(UI_ACTION_IMPLS_FILE);
    std::fs::create_dir_all(impls.parent().expect("应可取父目录")).expect("应可建目录");
    std::fs::write(
        &impls,
        "impl UiAction {\n    pub const fn persisted_key(self) -> Option<&'static str> {\n        match self {\n            UiAction::Save => Some(\"save\"),\n            _ => return None,\n        }\n    }\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_persisted_key_fallback(&root);
    assert_eq!(hits.len(), 1, "注入的兜底分支应被检出 1 处，实际：{hits:?}");
}

/// 反向验证：穷举写法不得被误报。
///
/// 守卫若把正确写法也判违规，开发者就会加豁免逃生口，规则随即失效。
#[test]
fn the_exhaustive_form_is_not_flagged() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    let impls = root.join(UI_ACTION_IMPLS_FILE);
    std::fs::create_dir_all(impls.parent().expect("应可取父目录")).expect("应可建目录");
    std::fs::write(
        &impls,
        "impl UiAction {\n    pub const fn persisted_key(self) -> Option<&'static str> {\n        match self {\n            UiAction::Save => Some(\"save\"),\n            UiAction::Undo => None,\n        }\n    }\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_persisted_key_fallback(&root);
    assert!(hits.is_empty(), "穷举写法不应被误报：{hits:?}");
}

/// 反向验证：注释里提到兜底不算违规。
#[test]
fn comments_mentioning_fallback_are_not_flagged() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    let impls = root.join(UI_ACTION_IMPLS_FILE);
    std::fs::create_dir_all(impls.parent().expect("应可取父目录")).expect("应可建目录");
    std::fs::write(
        &impls,
        "impl UiAction {\n    // 此前是 `_ => return None` 兜底，现改为穷举。\n    pub const fn persisted_key(self) -> Option<&'static str> {\n        match self {\n            UiAction::Save => Some(\"save\"),\n        }\n    }\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_persisted_key_fallback(&root);
    assert!(hits.is_empty(), "注释里的兜底字样不应被判违规：{hits:?}");
}

/// 现状守卫：表现层 `UiAction::ALL` 与枚举变体**同步**。
///
/// `ALL` 是手写清单，编译器管不到。漏登记的症状是「命令面板少一项」
/// ——**零报错**。这条把两边的变体名**逐项比对**。
#[test]
fn the_presentation_action_list_matches_the_enum() {
    let enum_text = read_from_roots(UI_ACTION_ENUM_FILE).expect("枚举应可读");
    let impls_text = read_from_roots(UI_ACTION_IMPLS_FILE).expect("impls 应可读");

    let variants = extract_enum_variants(&enum_text, "UiAction");
    let entries = extract_all_list_entries(&impls_text);

    assert!(
        !variants.is_empty(),
        "应能从 {UI_ACTION_ENUM_FILE} 提取到 `UiAction` 的变体，\
         否则本守卫会空过（扫不到任何东西 = 假绿）"
    );
    assert!(
        !entries.is_empty(),
        "应能从 {UI_ACTION_IMPLS_FILE} 提取到 `UiAction::ALL` 的清单项，\
         否则本守卫会空过"
    );

    let missing: Vec<&String> = variants.iter().filter(|v| !entries.contains(v)).collect();
    let extra: Vec<&String> = entries.iter().filter(|e| !variants.contains(e)).collect();

    assert!(
        missing.is_empty() && extra.is_empty(),
        "`UiAction::ALL` 与枚举变体不同步：\
         漏登记 {missing:?}（该动作不会出现在命令面板），\
         多余登记 {extra:?}（该动作在枚举里不存在）。\
         两侧的变体数：枚举 {} / 清单 {}",
        variants.len(),
        entries.len()
    );
}

/// 变异验证：清单漏登记一个变体必须被检出。
#[test]
fn an_action_missing_from_the_list_is_detected() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    let enum_file = root.join(UI_ACTION_ENUM_FILE);
    let impls_file = root.join(UI_ACTION_IMPLS_FILE);
    std::fs::create_dir_all(enum_file.parent().expect("应可取父目录")).expect("应可建目录");
    std::fs::create_dir_all(impls_file.parent().expect("应可取父目录")).expect("应可建目录");
    std::fs::write(
        &enum_file,
        "pub enum UiAction {\n    Save,\n    Undo,\n    BrandNewAction,\n}\n",
    )
    .expect("写 fixture 应成功");
    std::fs::write(
        &impls_file,
        "impl UiAction {\n    pub const ALL: &'static [UiAction] = &[\n        UiAction::Save,\n        UiAction::Undo,\n    ];\n}\n",
    )
    .expect("写 fixture 应成功");

    let enum_text = std::fs::read_to_string(&enum_file).expect("应可读");
    let impls_text = std::fs::read_to_string(&impls_file).expect("应可读");
    let variants = extract_enum_variants(&enum_text, "UiAction");
    let entries = extract_all_list_entries(&impls_text);

    let missing: Vec<&String> = variants.iter().filter(|v| !entries.contains(v)).collect();
    assert_eq!(
        missing.len(),
        1,
        "清单漏登记的变体应被检出 1 个，实际：{missing:?}"
    );
    assert_eq!(missing[0], "BrandNewAction");
}

/// 变异验证：清单多登记一个不存在的变体必须被检出。
#[test]
fn an_action_absent_from_the_enum_is_detected() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    let enum_file = root.join(UI_ACTION_ENUM_FILE);
    let impls_file = root.join(UI_ACTION_IMPLS_FILE);
    std::fs::create_dir_all(enum_file.parent().expect("应可取父目录")).expect("应可建目录");
    std::fs::create_dir_all(impls_file.parent().expect("应可取父目录")).expect("应可建目录");
    std::fs::write(&enum_file, "pub enum UiAction {\n    Save,\n    Undo,\n}\n")
        .expect("写 fixture 应成功");
    std::fs::write(
        &impls_file,
        "impl UiAction {\n    pub const ALL: &'static [UiAction] = &[\n        UiAction::Save,\n        UiAction::Undo,\n        UiAction::GhostAction,\n    ];\n}\n",
    )
    .expect("写 fixture 应成功");

    let enum_text = std::fs::read_to_string(&enum_file).expect("应可读");
    let impls_text = std::fs::read_to_string(&impls_file).expect("应可读");
    let variants = extract_enum_variants(&enum_text, "UiAction");
    let entries = extract_all_list_entries(&impls_text);

    let extra: Vec<&String> = entries.iter().filter(|e| !variants.contains(e)).collect();
    assert_eq!(
        extra.len(),
        1,
        "清单多余的变体应被检出 1 个，实际：{extra:?}"
    );
    assert_eq!(extra[0], "GhostAction");
}

/// 现状守卫：领域 `ActionId::all()` 与枚举变体**同步**。
///
/// 与表现层同构的问题：领域侧 `all()` 也是手写清单，漏登记的症状是
/// 「该动作不出现在命令面板、且不可改键」——同样**零报错**。
#[test]
fn the_domain_action_list_matches_the_enum() {
    let text = read_from_roots(DOMAIN_ACTION_FILE).expect("领域动作应可读");

    let variants = extract_enum_variants(&text, "ActionId");
    let entries = extract_fn_all_list_entries(&text);

    assert!(
        !variants.is_empty(),
        "应能从 {DOMAIN_ACTION_FILE} 提取到 `ActionId` 的变体，\
         否则本守卫会空过（扫不到任何东西 = 假绿）"
    );
    assert!(
        !entries.is_empty(),
        "应能从 {DOMAIN_ACTION_FILE} 提取到 `ActionId::all()` 的清单项，\
         否则本守卫会空过"
    );

    let missing: Vec<&String> = variants.iter().filter(|v| !entries.contains(v)).collect();
    let extra: Vec<&String> = entries.iter().filter(|e| !variants.contains(e)).collect();

    assert!(
        missing.is_empty() && extra.is_empty(),
        "`ActionId::all()` 与枚举变体不同步：\
         漏登记 {missing:?}（该动作不会出现在命令面板），\
         多余登记 {extra:?}（该动作在枚举里不存在）。\
         两侧的变体数：枚举 {} / 清单 {}",
        variants.len(),
        entries.len()
    );
}
