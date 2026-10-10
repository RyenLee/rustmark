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

use std::path::PathBuf;

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

/// 生成 1MB 语料并写入 `target/golden/large-1mb.md`。
///
/// 返回生成路径与字节数。
fn generate() -> (PathBuf, usize) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("golden");
    std::fs::create_dir_all(&dir).expect("应能创建 target/golden");

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
pub fn load_or_generate_large() -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("golden");
    let path = dir.join("large-1mb.md");
    match std::fs::read_to_string(&path) {
        Ok(content) if content.len() >= TARGET_BYTES => content,
        _ => {
            let (path, size) = generate();
            std::fs::read_to_string(&path)
                .expect("刚生成的文件应可读")
                .tap(size)
        }
    }
}

/// 供上面的 `map` 使用的小扩展：读完后顺便断言字节数。
trait Tap {
    fn tap(self, size: usize) -> Self;
}

impl Tap for String {
    fn tap(self, size: usize) -> Self {
        assert!(size >= TARGET_BYTES, "样本应达到 1MB，实际 {size}");
        self
    }
}
