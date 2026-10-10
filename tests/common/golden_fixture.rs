//! 生成 1MB 大文件样本（golden 大文件语料）。
//!
//! 1MB 文本不适合提交进仓库，故按需生成到 `target/golden/`。
//! 生成结果确定（固定种子 + 固定模板），因此每次生成内容一致，
//! 可用于「同一输入 → 同一解析耗时」的口径对比。
//!
//! 用法：
//!
//! ```text
//! cargo test --test integration -- --ignored --nocapture generate_large_fixture
//! ```
//!
//! 生成路径：`target/golden/large-1mb.md`
//!
//! # 为什么 `load_or_generate_large` 每进程只生成一次
//!
//! 集成测试是**多线程并行**的：若干个 1MB 用例同时调本模块时，曾出现
//! 多个线程同时向同一个 `large-1mb.md` 写盘的竞态——Windows 下表现为
//! `PermissionDenied (os error 5)`，用例随机爆、与代码对错无关。
//! 收敛到 `OnceLock` 后每进程只有首个线程生成并写盘，其余线程直接
//! 复用内存中的内容；写盘只剩「尽力而为」（失败不影响本进程的用例）。

use std::path::PathBuf;
use std::sync::OnceLock;

/// 目标大小：1 MB。
const TARGET_BYTES: usize = 1024 * 1024;

/// 重复片段模板（覆盖各类块，保证解析器走遍所有分支）。
const BLOCKS: &[&str] = &[
    "## 小节标题\n\n普通段落，包含 `行内代码` 与 **粗体** 文字。\n\n",
    "- 列表项一\n- 列表项二\n  - 嵌套项\n\n",
    "> 引用块，用于覆盖引用解析分支。\n\n",
    "| 列 A | 列 B |\n|:-----|------:|\n| 1 | 2 |\n\n",
    "```rust\nfn f{i}(x: i32) -> i32 {{\n    // 注释\n    x * {i}\n}}\n```\n\n",
    "- [x] 已完成 {i}\n- [ ] 未完成 {i}\n\n",
    "![图](img/{i}.png)\n\n",
    "正文段落 {i}：中文与 English mixed to exercise UTF-8 offsets.\n\n",
];

/// 拼出 1MB 语料（纯内存，确定性的：固定模板 + 固定顺序）。
fn build_content() -> String {
    let mut content = String::with_capacity(TARGET_BYTES + 4096);
    let mut index = 0usize;
    while content.len() < TARGET_BYTES {
        for template in BLOCKS {
            if content.len() >= TARGET_BYTES {
                break;
            }
            content.push_str(&template.replace("{i}", &index.to_string()));
            index += 1;
        }
    }
    content
}

/// 生成 1MB 语料并写入 `target/golden/large-1mb.md`。
///
/// 返回生成路径与字节数。仅供 `--ignored` 的手动生成用例使用——
/// 常规测试走 [`load_or_generate_large`]（不要求写盘成功）。
fn generate() -> (PathBuf, usize) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("golden");
    std::fs::create_dir_all(&dir).expect("应能创建 target/golden");

    let content = build_content();
    let path = dir.join("large-1mb.md");
    std::fs::write(&path, content.as_bytes()).expect("应能写入大文件样本");
    (path, content.len())
}

/// 生成大文件样本（`--ignored` 运行）。
#[test]
#[ignore = "生成型用例：需要时用 --ignored 手动跑"]
fn generate_large_fixture() {
    let (path, size) = generate();
    println!("已生成 {} 字节 -> {}", size, path.display());
    assert!(size >= TARGET_BYTES, "样本应达到 1MB，实际 {size}");
}

/// 读取大文件样本；若不存在则**自动生成**后读取。
///
/// 自动生成让 1MB 用例可以直接参与常规 `cargo test`，
/// 而不必先跑 `--ignored` 那条生成命令。
///
/// 每进程只生成 / 读盘一次（见文件头「为什么」）：后续调用方拿到的
/// 是同一份 `String` 的克隆，既避免了并发写盘竞态，也省掉每个用例
/// 重读 1MB 的开销。
pub fn load_or_generate_large() -> String {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("golden");
            let path = dir.join("large-1mb.md");
            // 已有合格样本就直接用（手动 `--ignored` 生成过的场景）。
            if let Ok(content) = std::fs::read_to_string(&path)
                && content.len() >= TARGET_BYTES
            {
                return content;
            }
            let content = build_content();
            // 写盘尽力而为：磁盘被占 / 只读时，本进程的用例照常跑
            // （内容在内存里），只是下次还得重新生成。
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(&path, content.as_bytes());
            content
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 样本必须达到 1MB 且每进程多次取用的是同一份内容。
    #[test]
    fn large_sample_meets_target_and_is_cached() {
        let a = load_or_generate_large();
        assert!(a.len() >= TARGET_BYTES, "样本应达到 1MB，实际 {}", a.len());
        let b = load_or_generate_large();
        assert_eq!(a, b, "同进程两次取用应是同一份内容");
    }
}
