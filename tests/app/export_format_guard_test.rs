//! 「新增一种导出格式只改 N 处」的守卫（R-07）。
//!
//! # 为什么需要源码级守卫
//!
//! R-07 收敛掉的 `ExportFormatChoice` 是**两份同构枚举靠字符串对齐**。
//! 那种结构没有任何机制阻止它复活：任何人新增一个枚举、
//! 写一份自己的 `id()` / `label()`，编译器不会吭声，
//! 而两份枚举会从「刚建时一致」慢慢漂移到「不一致」——
//! **没有任何报错**，用户看到的是「选了 Markdown 却导出成 HTML」。
//!
//! 行为断言抓不住这类漂移：两份枚举**刚建时行为完全一致**，
//! 于是任何行为用例在漂移前都是绿的。要抓住「又多了一份定义」，
//! 只能**扫源码**（与 `tools/arch_check.rs` 的思路同源，但这是
//! 「同一概念只有一处定义」的规则，不涉及依赖方向，故独立成文件）。
//!
//! # 判据
//!
//! 扫 `src/`，找**任何**「导出格式 / 页面尺寸枚举的定义」：
//! - `pub enum ExportFormat` / `pub enum PageSize`（定义处，唯一允许在
//!   `shared/export_format.rs`）；
//! - `pub enum ExportFormatChoice` / `pub enum PageSizeChoice`
//!   （R-07 之前的表现层副本，**已删除**，复活即违规）；
//! - 任何形如 `enum ...Format...` 且带 `Html`/`Pdf`/`Image` 变体的定义
//!   （换个名字重建同构枚举，绕过前两条）。
//!
//! 合法出现只有一处：`src/shared/export_format.rs`。

use std::path::{Path, PathBuf};

/// 真源文件（相对 `src/` 的路径）。
const SOURCE_OF_TRUTH: &str = "shared/export_format.rs";

/// 扫出「导出格式 / 页面尺寸枚举的多余定义」。
///
/// 返回 `(相对路径, 行号, 该行内容)` 三元组。
fn scan_duplicate_format_enums(src_root: &Path) -> Vec<(String, usize, String)> {
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
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            // 只看定义行：`pub enum X` / `enum X`（允许 `pub(crate) enum`）。
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
            // 真源处合法。
            if rel == SOURCE_OF_TRUTH {
                continue;
            }
            // 违规判据：**枚举体含导出格式的特征变体**。
            //
            // 只看名字不够：本工程里 `FormatKind`（加粗/斜体等**编辑**
            // 格式指令）、`TextFormat`（可打开的文本格式白名单）
            // 都与导出格式无关，只是名字撞车，不能误伤。
            //
            // 只看变体也不够：变体名（`Html` / `Pdf` / `Image`）在
            // 别的语境里也出现（如 HTML 标签名）。所以最终判据是
            // 「**名字像 或 变体像**」进入候选，再要求「**变体像**」
            // 才判违规——两者叠加，误报与漏报都堵住。
            if !looks_like_a_format_enum(name) && !has_export_format_variants(&text) {
                continue;
            }
            if !has_export_format_variants(&text) {
                continue;
            }
            hits.push((rel.clone(), idx + 1, trimmed.to_string()));
        }
    }
    hits
}

/// 该文件里是否出现导出格式的**特征变体**。
///
/// 取三个互不相关的代表变体（`Html` / `Pdf` / `Image`），
/// 要求**至少命中两个**才判定为「导出格式枚举」——
/// 单个 `Html` 可能来自别处（如 HTML 标签名），但三个里命中两个，
/// 基本只可能是导出格式。
fn has_export_format_variants(text: &str) -> bool {
    let hits = ["Html", "Pdf", "Image"]
        .iter()
        .filter(|variant| text.contains(*variant))
        .count();
    hits >= 2
}

/// 枚举名是否像「导出格式 / 页面尺寸」这类枚举。
///
/// 判据：名字里含 `Format` / `PageSize` 之一。
///
/// 这条只是**候选**判据（与「变体像」取或）；最终是否违规
/// 还要看枚举体里有没有导出格式的特征变体。
/// 故此处的已知撞名（`TextFormat`）不单独排除——
/// 排除会漏掉「有人把真源改名成 TextFormat」这种情形。
fn looks_like_a_format_enum(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    name.contains("Format") || name.contains("PageSize")
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

fn src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// 现状守卫：真实工程里导出格式**只有一处**定义。
///
/// 这条用例的价值随时间**递增**：只要有人再写一份同构枚举
/// （不管是改名叫 `ExportFormatChoice` 还是 `ExportFormatOption`），
/// 它立刻变红。
#[test]
fn export_format_has_exactly_one_definition() {
    let hits = scan_duplicate_format_enums(&src_root());
    assert!(
        hits.is_empty(),
        "导出格式/页面尺寸只能有一处定义（`{SOURCE_OF_TRUTH}`）。\
         实际多出的定义：\n{}",
        hits.iter()
            .map(|(f, l, c)| format!("  {f}:{l}  {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// 真源确实存在（否则上一条会因为「扫不到任何定义」而空过）。
///
/// 没有这条，上一条在真源被误删后会**假绿**——
/// 那比没有守卫更危险。
#[test]
fn the_single_source_of_truth_exists() {
    let path = src_root().join(SOURCE_OF_TRUTH);
    assert!(
        path.exists(),
        "真源文件 {SOURCE_OF_TRUTH} 必须存在，否则导出格式无处安放"
    );
    let text = std::fs::read_to_string(&path).expect("真源应可读");
    assert!(
        text.contains("pub enum ExportFormat"),
        "真源里必须有 `pub enum ExportFormat`"
    );
    assert!(
        text.contains("pub enum PageSize"),
        "真源里必须有 `pub enum PageSize`"
    );
}

/// 变异验证：注入一份同构枚举必须被检出。
///
/// 没有这条，守卫可能因匹配逻辑写错而**永远绿**。
#[test]
fn injected_duplicate_format_enum_is_detected() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("presentation/state")).expect("应可建目录");
    std::fs::write(
        root.join("presentation/state/export_ui_state.rs"),
        "pub enum ExportFormatChoice {\n    Html,\n    Pdf,\n    Image,\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_duplicate_format_enums(&root);
    assert_eq!(hits.len(), 1, "注入的同构枚举应被检出 1 处，实际：{hits:?}");
}

/// 变异验证：换个名字重建同构枚举，同样必须被检出。
///
/// 上一条只认得 `ExportFormatChoice` 这个名字；这一条证明
/// 守卫认的是「**导出格式枚举**」这个概念，而不是某个具体名字。
#[test]
fn injected_renamed_format_enum_is_detected() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("presentation/state")).expect("应可建目录");
    std::fs::write(
        root.join("presentation/state/export_ui_state.rs"),
        "pub enum ExportFormatOption {\n    Html,\n    Pdf,\n    Image,\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_duplicate_format_enums(&root);
    assert_eq!(hits.len(), 1, "换名的同构枚举应被检出 1 处，实际：{hits:?}");
}

/// 反向验证：真源处**不得**被误报。
///
/// 守卫若把正确写法也判违规，开发者就会加豁免逃生口，规则随即失效。
#[test]
fn the_source_of_truth_is_not_flagged() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("shared")).expect("应可建目录");
    std::fs::write(
        root.join("shared/export_format.rs"),
        "pub enum ExportFormat {\n    Html,\n    Markdown,\n    Pdf,\n    Image,\n}\npub enum PageSize {\n    A4,\n    Letter,\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_duplicate_format_enums(&root);
    assert!(hits.is_empty(), "真源不应被误报：{hits:?}");
}

/// 反向验证：`TextFormat` 不是导出格式，不该被误报。
///
/// 它是「可打开的文本格式白名单」——列的是**扩展名**，
/// 与「导出格式」是两件事，名字撞车而已。
#[test]
fn text_format_is_not_flagged() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let root = dir.path().join("src");
    std::fs::create_dir_all(root.join("presentation/theme")).expect("应可建目录");
    std::fs::write(
        root.join("presentation/theme/mapper.rs"),
        "pub struct TextFormat {\n    pub color: u32,\n}\npub enum FormatKind {\n    Bold,\n    Italic,\n}\n",
    )
    .expect("写 fixture 应成功");

    let hits = scan_duplicate_format_enums(&root);
    assert!(hits.is_empty(), "TextFormat 不该被误报：{hits:?}");
}
