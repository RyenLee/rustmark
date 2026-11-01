//! `FsDocumentRepository` 的内联单测（评审 P2 #1 / #2）。
//!
//! 这里只覆盖**原子写入的两处加固**：父目录 fsync（#1）与临时文件命名（#2）。
//! 编码 / 换行 / 草稿恢复等行为由 `tests/infrastructure/fs_document_repository_test.rs`
//! 从外部（`rustmark::…` 路径）覆盖，两边刻意不重复。

use std::fs;
use std::path::Path;

use rustmark_core::domain::document::repository::{DocumentRepository, DocumentSnapshot};
use rustmark_core::shared::ids::DocumentId;
use rustmark_core::shared::path::FilePath;

use super::super::atomic_file::tmp_path_for;
use super::FsDocumentRepository;
// 目录 fsync 只在 Unix 上有实现，非 Unix 平台这两条用例整个不进编译，
// 导入也必须同 cfg——否则 Windows 上是 `unused_imports`（`-D warnings` 会红）。
#[cfg(unix)]
use super::super::atomic_file::sync_parent_dir;

/// 把磁盘路径包装成领域层的 `FilePath`。
fn fp(disk: &Path) -> FilePath {
    FilePath::new(disk.to_string_lossy().replace('\\', "/")).expect("临时路径应合法")
}

fn snapshot(disk: &Path, content: &str) -> DocumentSnapshot {
    DocumentSnapshot::new(DocumentId::new(), fp(disk), content, None)
}

/// 列出目录下残留的临时文件。
fn tmp_leftovers(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .expect("应能列出目录")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with(".rustmark-tmp-"))
        .collect()
}

// ---------------------------------------------------------------------------
// #2 临时文件命名：`.rustmark-tmp-<pid>-<seq>-<name>`
// ---------------------------------------------------------------------------

/// 连续两次取 tmp 名必须不同——这正是旧实现做不到的（只用文件名）。
///
/// 若相同，并发保存会在同一个 tmp 上互相 `truncate`，rename 后目标内容即错。
#[test]
fn tmp_names_of_same_target_are_unique() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let target = dir.path().join("note.md");

    let first = tmp_path_for(&target);
    let second = tmp_path_for(&target);
    let third = tmp_path_for(&target);

    assert_ne!(first, second, "同目标的两次 tmp 名不得相同");
    assert_ne!(second, third, "计数器必须持续推进");
}

/// 名字里必须带当前进程 ID，且保持 `.rustmark-tmp-` 前缀与原名后缀。
#[test]
fn tmp_name_carries_pid_prefix_and_original_name() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let target = dir.path().join("note.md");

    let tmp = tmp_path_for(&target);
    let name = tmp
        .file_name()
        .expect("tmp 应有文件名")
        .to_string_lossy()
        .into_owned();

    assert!(
        name.starts_with(".rustmark-tmp-"),
        "前缀是残留识别 / 清理的约定，实际：{name}"
    );
    assert!(
        name.contains(&std::process::id().to_string()),
        "名字应含当前 PID，实际：{name}"
    );
    assert!(
        name.ends_with("-note.md"),
        "名字应保留原文件名后缀，实际：{name}"
    );
    // 去掉 `.rustmark-tmp-<pid>-<seq>-note.md` 的三段，中间的 seq 必须非空。
    let middle = name
        .strip_prefix(".rustmark-tmp-")
        .and_then(|rest| rest.strip_suffix("-note.md"))
        .expect("应是 <pid>-<seq> 两段");
    let (pid, seq) = middle.split_once('-').expect("应是 <pid>-<seq>");
    assert_eq!(pid, std::process::id().to_string());
    assert!(seq.parse::<u64>().is_ok(), "seq 应是数字，实际：{seq}");
}

/// 临时文件必须落在**目标同目录**——跨目录 rename 不保证原子。
#[test]
fn tmp_file_lives_in_target_directory() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let sub = dir.path().join("nested");
    fs::create_dir_all(&sub).expect("应能建子目录");

    let target = sub.join("a.md");
    let tmp = tmp_path_for(&target);
    assert_eq!(tmp.parent(), Some(sub.as_path()), "tmp 必须在同一父目录");
}

// ---------------------------------------------------------------------------
// 正常保存路径仍成功（#1 的 fsync 不得让保存失败）+ 无残留
// ---------------------------------------------------------------------------

/// 保存成功、内容正确、无残留 tmp —— 这是 #1 改动最需要钉住的不变量：
/// 父目录 fsync 是 best-effort，**绝不能**让本来成功的保存变成失败。
#[test]
fn save_succeeds_and_leaves_no_tmp_file() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let repo = FsDocumentRepository::new();
    let target = dir.path().join("clean.md");

    repo.save(&snapshot(&target, "第一段\n第二段"))
        .expect("保存应成功（目录 fsync 失败也不得影响返回值）");

    // 用仓储读回（而不是裸 `fs::read_to_string`）：新建文件的换行按平台默认
    // 落盘（Windows 上是 CRLF），读时归一是仓储契约的一部分。
    assert_eq!(
        repo.load(&fp(&target)).expect("应能读回目标文件").content,
        "第一段\n第二段"
    );
    assert!(tmp_leftovers(dir.path()).is_empty(), "不应残留临时文件");
}

/// 同一目标连续覆盖保存：每次结果都正确，且不留残留。
#[test]
fn repeated_saves_overwrite_in_place() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let repo = FsDocumentRepository::new();
    let target = dir.path().join("again.md");

    for round in 0..5 {
        let content = format!("第 {round} 版");
        repo.save(&snapshot(&target, &content)).expect("保存应成功");
        assert_eq!(
            fs::read_to_string(&target).expect("应能读回"),
            content,
            "第 {round} 次覆盖后的内容不对"
        );
    }
    assert!(tmp_leftovers(dir.path()).is_empty(), "不应残留临时文件");
}

/// 并发保存同一目标：结尾内容必须**恰好等于某一个**写入者的内容，
/// 不能出现交错 / 截断（旧实现的固定 tmp 名正会导致这类损坏）。
///
/// 单次 `save` 在极端竞争下仍可能因 rename 被抢占而失败（Windows 上尤其如此，
/// 这是 `save` 既有语义：失败时原文件完好），故这里**容忍个别失败**，
/// 只钉住真正的加固目标——最终字节不会互相污染，也不留残留。
#[test]
fn concurrent_saves_do_not_corrupt_target() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let target = dir.path().join("race.md");

    let payloads: Vec<String> = (0..8)
        .map(|i| format!("writer-{i}-").repeat(2048))
        .collect();

    let mut succeeded = 0usize;
    std::thread::scope(|scope| {
        let handles: Vec<_> = payloads
            .iter()
            .map(|payload| {
                let target = target.clone();
                scope.spawn(move || {
                    let repo = FsDocumentRepository::new();
                    let mut ok = 0usize;
                    for _ in 0..4 {
                        if repo.save(&snapshot(&target, payload)).is_ok() {
                            ok += 1;
                        }
                    }
                    ok
                })
            })
            .collect();
        for handle in handles {
            succeeded += handle.join().expect("工作线程不得 panic");
        }
    });

    assert!(succeeded > 0, "至少要有一次保存成功");
    let final_content = fs::read_to_string(&target).expect("应能读回目标文件");
    assert!(
        payloads.contains(&final_content),
        "最终内容必须是某一次写入的完整内容（长度 {}，疑似交错 / 截断）",
        final_content.len()
    );
    assert!(tmp_leftovers(dir.path()).is_empty(), "不应残留临时文件");
}

// ---------------------------------------------------------------------------
// #1 父目录 fsync 的失败分支：不可注入 IO 错误，故直接调用辅助函数
// ---------------------------------------------------------------------------

/// 目录打不开时走 `log::warn!` 分支，且**不得 panic**（`panic = "abort"` 下
/// 任何意外都会终止编辑器）。用「不存在的目录」构造这条分支。
#[cfg(unix)]
#[test]
fn sync_parent_dir_survives_missing_directory() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let missing = dir.path().join("not-created");
    assert!(!missing.exists());
    sync_parent_dir(&missing);
}

/// 正常目录上执行目录 fsync：不 panic、无副作用。
///
/// 注：`sync_all` 自身的失败无法在不引入 mock 层的条件下注入——
/// 生产路径上它只会在异常文件系统（如某些网络盘 / 只读挂载）里失败，
/// 故失败分支由上面那条「打不开目录」的用例覆盖（两者共用同一个 warn 惯例）。
#[cfg(unix)]
#[test]
fn sync_parent_dir_on_real_directory_is_quiet() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    sync_parent_dir(dir.path());
}

// ---------------------------------------------------------------------------
// #2 的配套：旧命名残留的迁移清理
// ---------------------------------------------------------------------------

/// 旧命名（无 PID / 计数器）的崩溃残留必须被本次保存顺手清掉。
///
/// 旧实现靠「同名 tmp 复用」侥幸消化它；新命名不再复用，不显式清理就会
/// 变成用户目录里的永久垃圾（`tests/integration/crash_recovery_test.rs`
/// 里那条崩溃恢复用例依赖的正是这条行为）。
#[test]
fn legacy_temp_residue_is_cleaned_on_next_save() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let repo = FsDocumentRepository::new();
    let target = dir.path().join("note.md");

    // 模拟旧版本崩溃：留下 `.rustmark-tmp-note.md`（不含 PID / 计数器）。
    let legacy = dir.path().join(".rustmark-tmp-note.md");
    fs::write(&legacy, "GARBAGE-HALF-WRITTEN").expect("应能写残留");

    repo.save(&snapshot(&target, "新内容")).expect("保存应成功");

    assert!(!legacy.exists(), "旧命名残留应被清理");
    assert_eq!(
        repo.load(&fp(&target)).expect("应能读回").content,
        "新内容",
        "残留不得污染目标内容"
    );
    assert!(tmp_leftovers(dir.path()).is_empty(), "不应残留任何临时文件");
}
