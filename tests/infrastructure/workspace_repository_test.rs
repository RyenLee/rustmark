//! `FsWorkspaceRepository` 与 `RecentFilesStore` 单测（S5-05）。

use std::fs;
use std::path::{Path, PathBuf};

use rustmark::domain::workspace::repository::WorkspaceRepository;
use rustmark::domain::workspace::value_objects::{EntryKind, RecentFile};
use rustmark::infrastructure::persistence::fs_workspace_repository::{
    FsWorkspaceRepository, TraversalRules,
};
use rustmark::infrastructure::persistence::recent_files_store::RecentFilesStore;
use rustmark::ports::time::Timestamp;
use rustmark::shared::error::AppError;
use rustmark::shared::path::FilePath;

use crate::support::tempdir::TempDir;

fn fp(disk: &Path) -> FilePath {
    FilePath::new(disk.to_string_lossy().replace('\\', "/")).expect("临时路径应合法")
}

fn repo_in(dir: &TempDir) -> FsWorkspaceRepository {
    FsWorkspaceRepository::new(dir.path().join("recent.toml"))
}

// ---------------------------------------------------------------------------
// 目录遍历
// ---------------------------------------------------------------------------

#[test]
fn scan_lists_nested_files_within_depth_limit() {
    let dir = TempDir::new("scan-nested");
    dir.write("a.md", "a");
    dir.write("sub/b.md", "b");
    dir.write("sub/deep/c.md", "c");

    let repo = repo_in(&dir);
    let entries = repo.scan(&fp(dir.path()), 5).expect("扫描应成功");

    let names: Vec<String> = entries
        .iter()
        .map(|e| e.path().file_name().to_string())
        .collect();
    for expected in ["a.md", "b.md", "c.md"] {
        assert!(
            names.contains(&expected.to_string()),
            "应包含 {expected}，实际：{names:?}"
        );
    }
}

#[test]
fn scan_depth_zero_returns_only_top_level() {
    let dir = TempDir::new("scan-depth0");
    dir.write("top.md", "t");
    dir.write("sub/inner.md", "i");

    let repo = repo_in(&dir);
    let entries = repo.scan(&fp(dir.path()), 0).expect("扫描应成功");
    let names: Vec<String> = entries
        .iter()
        .map(|e| e.path().file_name().to_string())
        .collect();

    assert!(names.contains(&"top.md".to_string()), "应含顶层文件");
    assert!(
        !names.contains(&"inner.md".to_string()),
        "深度 0 不应递归到子目录，实际：{names:?}"
    );
}

#[test]
fn scan_skips_hidden_and_ignored_directories() {
    let dir = TempDir::new("scan-ignore");
    dir.write("visible.md", "v");
    dir.write(".hidden.md", "h");
    dir.write(".git/config", "git stuff");
    dir.write("node_modules/pkg/index.js", "js");
    dir.write("target/debug/bin", "rust");

    let repo = repo_in(&dir);
    let entries = repo.scan(&fp(dir.path()), 5).expect("扫描应成功");
    let names: Vec<String> = entries
        .iter()
        .map(|e| e.path().file_name().to_string())
        .collect();

    assert!(names.contains(&"visible.md".to_string()), "普通文件应保留");
    for unwanted in [".hidden.md", ".git", "node_modules", "target"] {
        assert!(
            !names.contains(&unwanted.to_string()),
            "{unwanted} 应被过滤，实际：{names:?}"
        );
    }
}

#[test]
fn scan_entries_are_sorted_directories_first() {
    let dir = TempDir::new("scan-order");
    dir.write("zebra.md", "z");
    dir.write("alpha.md", "a");
    dir.mkdir("m_dir");

    let repo = repo_in(&dir);
    let entries = repo.scan(&fp(dir.path()), 1).expect("扫描应成功");

    let names: Vec<String> = entries
        .iter()
        .map(|e| e.path().file_name().to_string())
        .collect();
    assert_eq!(
        names,
        vec!["m_dir", "alpha.md", "zebra.md"],
        "应目录在前、同层按名称升序（确定性顺序，不能依赖文件系统返回序）"
    );
    // 目录必须被标成 `Directory`——文件树据此建层级。
    // 曾经这里断言的是 `EntryKind::File`（与上一行的顺序断言自相矛盾），
    // 把「目录被标成文件」的缺陷锁进了测试：文件树因此完全无法建层级。
    assert_eq!(entries[0].kind(), EntryKind::Directory);
    assert_eq!(entries[0].path().file_name().to_string(), "m_dir");
    assert!(
        entries[1..].iter().all(|e| e.kind() == EntryKind::File),
        "只有首个（目录）应是 Directory，其余都是文件"
    );
}

/// 前序 DFS：目录条目之后**立即**是它的子项，然后才是下一个兄弟。
///
/// 文件树折行（`sidebar_ui_state::tree::tree_rows`）按列表顺序逐项渲染，
/// 输入不是「父目录紧跟子项」时，展开目录后子项会被拼到整棵树的尾部
/// （问题 040 截图实证：`logs` 的三个文件出现在所有根级文件之后）。
/// 旧实现先推完本层全部条目、末尾再统一递归——正是这个症状的根源。
#[test]
fn scan_lists_children_immediately_after_their_parent() {
    let dir = TempDir::new("scan-dfs");
    dir.write("b_dir/inner.md", "i");
    dir.write("a_dir/nested.md", "n");
    dir.write("z.md", "z");

    let repo = repo_in(&dir);
    let entries = repo.scan(&fp(dir.path()), 3).expect("扫描应成功");

    let names: Vec<String> = entries
        .iter()
        .map(|e| e.path().file_name().to_string())
        .collect();
    assert_eq!(
        names,
        vec!["a_dir", "nested.md", "b_dir", "inner.md", "z.md"],
        "子项必须紧跟父目录（前序 DFS），实际：{names:?}"
    );
    // 多层同样成立：孙项紧跟子目录，不跨过任何中间条目。
    dir.write("a_dir/deep/grand.md", "g");
    let entries = repo.scan(&fp(dir.path()), 5).expect("扫描应成功");
    let names: Vec<String> = entries
        .iter()
        .map(|e| e.path().file_name().to_string())
        .collect();
    assert_eq!(
        names,
        vec![
            "a_dir",
            "deep",
            "grand.md",
            "nested.md",
            "b_dir",
            "inner.md",
            "z.md"
        ],
        "深层子项也必须紧跟其父目录，实际：{names:?}"
    );
}

#[test]
fn scan_respects_max_entries_limit() {
    let dir = TempDir::new("scan-limit");
    for i in 0..50 {
        dir.write(&format!("f{i:03}.md"), "x");
    }
    let rules = TraversalRules {
        max_entries: 10,
        ..TraversalRules::default()
    };
    let repo = repo_in(&dir).with_rules(rules);

    let entries = repo.scan(&fp(dir.path()), 1).expect("扫描应成功");
    assert_eq!(entries.len(), 10, "超大目录必须截断，否则文件树会卡死");
}

#[test]
fn scan_missing_root_returns_io_error() {
    let dir = TempDir::new("scan-missing");
    let repo = repo_in(&dir);
    let missing = fp(&dir.path().join("does-not-exist"));

    let error = repo
        .scan(&missing, 3)
        .expect_err("扫描不存在的根目录必须失败");
    assert!(
        error.to_string().contains("does-not-exist"),
        "错误应含路径，实际：{error}"
    );
}

// ---------------------------------------------------------------------------
// 路径形态：产出路径的前缀必须等于调用方给定的根
// ---------------------------------------------------------------------------

/// 扫描产出的每条路径都必须以**调用方给定的根**为前缀。
///
/// 这是文件树与删除链路的共同前提：
///
/// - 树投影 `is_under_root` 用字符串前缀判断「条目是否属于当前根」；
/// - 删除链路 `Workspace::remove_entry` 要求聚合里存在**路径相等**的条目，
///   否则返回 `NotFound`，删除在碰磁盘之前就被拒。
///
/// 二者都建立在「条目路径与根同形态」之上。一旦前缀对不上，症状是
/// 「打开文件夹后文件树是空的」+「点了确认也删不掉」——正是 CI 上
/// macOS job 那 5 条红用例的表现。
#[test]
fn scan_paths_are_prefixed_with_the_given_root() {
    let dir = TempDir::new("scan-prefix");
    dir.write("readme.md", "r");
    dir.write("docs/guide.md", "g");

    let root = fp(dir.path());
    let repo = repo_in(&dir);
    let entries = repo.scan(&root, 3).expect("扫描应成功");

    assert!(!entries.is_empty(), "前提：应先扫出条目");
    let prefix = format!("{}/", root.as_str());
    for entry in &entries {
        assert!(
            entry.path().as_str().starts_with(&prefix),
            "条目路径必须以调用方给定的根为前缀，否则树投影与删除校验会失配：\
             根={}，实际={}",
            root.as_str(),
            entry.path().as_str()
        );
    }
}

/// 根的真实路径（canonicalize）与给定形态不同时，产出**仍然**用给定形态。
///
/// 这是上面那条不变量的**有效形态**：普通临时目录在多数平台上
/// canonicalize 前后同形，上面那条断言会是同义反复（不红也说明不了什么）。
/// 本用例刻意造出「两者不同」的根，让失配在**任何**能构造出差异的平台上
/// 都当场变红（Linux / macOS 用符号链接；Windows 未开开发者模式时退回
/// 大小写变体；都造不出来才跳过——此时该平台本就不会触发这个缺陷）。
///
/// 变异验证（本地）：把产出前缀改回 canonicalize 之后的形态，本用例在
/// 能构造差异的平台上立即变红。
#[test]
fn scan_keeps_the_given_root_spelling_when_canonical_form_differs() {
    let Some((dir, given)) = divergent_root("scan-divergent") else {
        // 本环境造不出「canonical 形态不同的路径」，该缺陷在此平台不会触发。
        eprintln!("跳过：本环境无法构造 canonical 形态不同的工作区根");
        return;
    };

    let root = fp(&given);
    let entries = repo_in(&dir).scan(&root, 3).expect("扫描应成功");

    assert!(
        !entries.is_empty(),
        "符号链接 / 大小写变体根下应照样扫出条目，实际为空——\
         说明产出路径用的是 canonicalize 之后的形态，与调用方持有的根对不上"
    );
    let prefix = format!("{}/", root.as_str());
    for entry in &entries {
        assert!(
            entry.path().as_str().starts_with(&prefix),
            "条目路径必须沿用调用方给定的根（{}），实际：{}",
            root.as_str(),
            entry.path().as_str()
        );
    }
    remove_link(&given);
}

/// 造一个「磁盘真实路径 ≠ 给定路径字符串」的工作区根。
///
/// ① 符号链接（Linux / macOS / 开了开发者模式的 Windows）；
/// ② 大小写变体（Windows、macOS 默认大小写不敏感）；
/// 都不行则返回 `None`（调用方跳过）。
fn divergent_root(tag: &str) -> Option<(TempDir, PathBuf)> {
    let dir = TempDir::new(tag);
    dir.write("readme.md", "r");
    dir.write("docs/guide.md", "g");

    let link = std::env::temp_dir().join(format!(
        "rustmark-ws-link-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    remove_link(&link);

    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(dir.path(), &link).is_ok();
    #[cfg(not(windows))]
    let linked = std::os::unix::fs::symlink(dir.path(), &link).is_ok();

    if linked {
        if divergent(&link) {
            return Some((dir, link));
        }
        remove_link(&link);
    }

    // 退路：大小写变体（仅对大小写不敏感的文件系统有效）。
    let text = dir.path().to_string_lossy().to_string();
    let flipped: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect();
    let variant = PathBuf::from(flipped);
    if variant.is_dir() && divergent(&variant) {
        return Some((dir, variant));
    }
    None
}

/// 该路径的 canonicalize 形态是否与它自身不同（不同才说明本用例有牙齿）。
fn divergent(path: &Path) -> bool {
    match path.canonicalize() {
        Ok(canonical) => canonical != path,
        Err(_) => false,
    }
}

/// 删除符号链接本身（**不**跟进去删真实目录）。
fn remove_link(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_dir(path);
}

// ---------------------------------------------------------------------------
// 内容读取（搜索用）
// ---------------------------------------------------------------------------

#[test]
fn read_content_normalizes_crlf_and_strips_bom() {
    let dir = TempDir::new("read-content");
    let target = dir.path().join("win.md");
    let mut raw = vec![0xEF, 0xBB, 0xBF];
    raw.extend_from_slice(b"line1\r\nline2");
    fs::write(&target, raw).expect("应能写原始字节");

    let repo = repo_in(&dir);
    let content = repo.read_content(&fp(&target)).expect("读取应成功");
    assert_eq!(
        content, "line1\nline2",
        "搜索读到的内容必须与编辑器一致（去BOM、CRLF→LF），否则行号会错位"
    );
}

#[test]
fn read_content_missing_file_returns_io_error() {
    let dir = TempDir::new("read-missing");
    let repo = repo_in(&dir);
    assert!(repo.read_content(&fp(&dir.path().join("nope.md"))).is_err());
}

// ---------------------------------------------------------------------------
// 最近文件持久化
// ---------------------------------------------------------------------------

fn recent(path: &str, at: u64) -> RecentFile {
    RecentFile::new(FilePath::new(path).unwrap(), Timestamp::from_millis(at))
}

#[test]
fn recent_files_roundtrip_through_disk() {
    let dir = TempDir::new("recent-roundtrip");
    let store = RecentFilesStore::new(dir.path().join("recent.toml"));

    assert!(
        store.load().expect("首次加载应成功").is_empty(),
        "文件不存在时应返回空列表而非报错"
    );

    let items = vec![recent("/docs/a.md", 1000), recent("/docs/b.md", 2000)];
    store.save(&items).expect("保存应成功");

    let loaded = store.load().expect("加载应成功");
    assert_eq!(loaded, items, "往返必须一致");
}

#[test]
fn recent_files_are_capped_at_limit() {
    let dir = TempDir::new("recent-cap");
    let store = RecentFilesStore::with_limit(dir.path().join("recent.toml"), 3);

    let items: Vec<RecentFile> = (0..10).map(|i| recent(&format!("/f{i}.md"), i)).collect();
    store.save(&items).expect("保存应成功");

    let loaded = store.load().expect("加载应成功");
    assert_eq!(loaded.len(), 3, "超出上限的条目必须在写盘时就裁掉");
}

#[test]
fn corrupt_recent_file_falls_back_to_empty_and_keeps_backup() {
    let dir = TempDir::new("recent-corrupt");
    let store = RecentFilesStore::new(dir.path().join("recent.toml"));
    store.save(&[recent("/a.md", 1)]).expect("保存应成功");

    // 模拟损坏：写入非法 TOML。
    fs::write(store.path(), "这不是合法的 TOML [[[").expect("应能写损坏内容");

    let loaded = store.load().expect("损坏文件不应导致加载失败");
    assert!(
        loaded.is_empty(),
        "损坏时按 ADR-003 回退为空列表（可用性优先于最近文件本身）"
    );

    let backups: Vec<String> = fs::read_dir(dir.path())
        .expect("应能列出目录")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".corrupt-"))
        .collect();
    assert_eq!(
        backups.len(),
        1,
        "损坏内容必须被备份以留待排查，实际：{backups:?}"
    );
}

#[test]
fn empty_recent_file_is_treated_as_empty_list() {
    let dir = TempDir::new("recent-empty");
    let store = RecentFilesStore::new(dir.path().join("recent.toml"));
    fs::write(store.path(), "   \n  ").expect("应能写空白内容");
    assert!(store.load().expect("空白文件应按空列表处理").is_empty());
}

#[test]
fn recent_file_without_version_field_defaults() {
    let dir = TempDir::new("recent-noversion");
    let store = RecentFilesStore::new(dir.path().join("recent.toml"));
    // 手写一个缺 `version` 的旧格式文件（v0），验证迁移路径不报错。
    // 注意 `FilePath` 是 newtype，serde 把它序列化成嵌套表 `{ raw = "..." }`，
    // 这里必须与真实格式一致，否则测的是「另一种形状」而非迁移逻辑。
    fs::write(
        store.path(),
        "[[items]]\nlast_opened = 42\n\n[items.path]\nraw = \"/legacy.md\"\n",
    )
    .expect("应能写旧格式");

    let loaded = store.load().expect("缺version 应按 v0 迁移而非报错");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].path().as_str(), "/legacy.md");
}

#[test]
fn unknown_fields_in_recent_file_are_ignored() {
    let dir = TempDir::new("recent-unknown");
    let store = RecentFilesStore::new(dir.path().join("recent.toml"));
    // 未来版本新增的字段不应导致旧版本读取失败（ADR-003 的前向兼容要求）。
    fs::write(
        store.path(),
        "version = 99\nfuture_field = \"x\"\n\n[[items]]\nlast_opened = 7\n\n[items.path]\nraw = \"/f.md\"\nextra = 1\n",
    )
    .expect("应能写含未知字段的内容");

    let loaded = store.load().expect("未知字段应被忽略而非报错");
    assert_eq!(loaded.len(), 1, "未知字段不应导致数据丢失");
}

#[test]
fn workspace_repository_delegates_recent_to_store() {
    let dir = TempDir::new("ws-recent");
    let repo = repo_in(&dir);
    let items = vec![recent("/x.md", 5)];

    repo.save_recent(&items).expect("保存最近文件应成功");
    assert_eq!(repo.load_recent().expect("读取应成功"), items);
}

#[test]
fn recent_save_creates_missing_parent_directory() {
    let dir = TempDir::new("recent-mkdir");
    let nested = dir.path().join("config/sub/recent.toml");
    let store = RecentFilesStore::new(&nested);
    store.save(&[recent("/a.md", 1)]).expect("应自动创建父目录");
    assert!(nested.exists());
}

#[test]
fn read_content_error_is_io_variant() {
    let dir = TempDir::new("read-err-kind");
    let repo = repo_in(&dir);
    let error: AppError = repo
        .read_content(&fp(&dir.path().join("ghost.md")))
        .expect_err("应失败");
    assert!(
        matches!(error, AppError::Io { .. }),
        "磁盘读取失败必须是 Io 变体，实际：{error:?}"
    );
}
