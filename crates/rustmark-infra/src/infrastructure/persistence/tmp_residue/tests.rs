//! `tmp_residue` 的内联单测（评审 P3）。
//!
//! 这条规则的价值全在**边界**上：删少了只是多几个垃圾文件，删错了是用户的
//! 真实数据。故每个用例钉住的都是「什么情况下**不**能删」，或「什么情况下
//! 判不出来就必须退回年龄兜底」。
//!
//! 时间与存活判定都可注入（[`sweep_with`] 的 `now` / `liveness`），
//! 因此「一小时后才算残留」这类规则在 Windows 开发机上也能测——不必真的等，
//! 也不必依赖 Linux 的 `/proc`。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{Liveness, MAX_AGE, pid_of, sweep_parent_once, sweep_stale, sweep_with};

/// 一个必然不存在的 PID：Linux 的 `pid_max` 上限远小于 `u32::MAX`。
const DEAD_PID: u32 = u32::MAX - 1;

/// 在 `dir` 下造一个临时文件，返回其路径。
fn touch(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, b"half-written").expect("应能写测试文件");
    path
}

/// 「现在」之后的时刻：用来让文件「看起来」已经放了一小时以上，
/// 而不必真的去改文件时间（那要额外依赖）。
fn later(seconds: u64) -> SystemTime {
    SystemTime::now() + Duration::from_secs(seconds)
}

/// 判定为「死进程」：模拟 Linux 上 `/proc/<pid>` 不存在。
fn dead(_: u32) -> Liveness {
    Liveness::Dead
}

/// 判定为「活进程」。
fn alive(_: u32) -> Liveness {
    Liveness::Alive
}

/// 判定不出来（macOS / Windows 的真实情形）。
fn unknown(_: u32) -> Liveness {
    Liveness::Unknown
}

// ---------------------------------------------------------------------------
// 基础：死进程的残留要删
// ---------------------------------------------------------------------------

/// 进程已死 → 立刻删，不必等年龄。
#[test]
fn dead_process_residue_is_removed() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let residue = touch(dir.path(), &format!(".rustmark-tmp-{DEAD_PID}-7-note"));

    let removed = sweep_with(dir.path(), SystemTime::now(), &dead);

    assert_eq!(removed, 1, "死进程的残留应被删掉");
    assert!(!residue.exists(), "残留文件应已消失");
}

/// 活进程 → 一律不碰，哪怕文件已经很老（可能是慢速网络盘上的慢写）。
#[test]
fn live_process_residue_is_kept_even_when_old() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let residue = touch(dir.path(), ".rustmark-tmp-12345-1-note");

    let removed = sweep_with(dir.path(), later(MAX_AGE.as_secs() * 3), &alive);

    assert_eq!(removed, 0, "活进程的临时文件不得清理");
    assert!(residue.exists());
}

/// 当前进程自己的临时文件：**即使**存活判定说「已死」也不能删
/// （本进程别的线程可能正在写）。这条是避免「自己删自己」的最后一道闸。
#[test]
fn own_process_residue_is_never_removed() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let name = format!(".rustmark-tmp-{}-3-note", std::process::id());
    let own = touch(dir.path(), &name);

    let removed = sweep_with(dir.path(), later(MAX_AGE.as_secs() * 3), &dead);

    assert_eq!(removed, 0, "当前进程的临时文件绝不能被清理");
    assert!(own.exists());
}

// ---------------------------------------------------------------------------
// 判不出来时：只按年龄兜底
// ---------------------------------------------------------------------------

/// 判不出存活（Windows / macOS 的真实情形）：新文件保留，老文件清理。
#[test]
fn unknown_liveness_falls_back_to_age_gate() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let fresh = touch(dir.path(), &format!(".rustmark-tmp-{DEAD_PID}-1-fresh"));

    assert_eq!(
        sweep_with(dir.path(), SystemTime::now(), &unknown),
        0,
        "刚写下的临时文件不得清理"
    );
    assert!(fresh.exists(), "年龄不足的临时文件必须留着");

    let removed = sweep_with(dir.path(), later(MAX_AGE.as_secs() + 1), &unknown);
    assert_eq!(removed, 1, "超过年龄上限的临时文件应被清理");
    assert!(!fresh.exists());
}

/// 旧命名（无 PID / 计数器）的残留无从判定归属，同样走年龄兜底。
#[test]
fn legacy_named_residue_is_age_gated() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let legacy = touch(dir.path(), ".rustmark-tmp-note");

    // 即便存活判定说「死」，旧命名没有 PID 可查，也只能看年龄。
    assert_eq!(
        sweep_with(dir.path(), SystemTime::now(), &dead),
        0,
        "旧命名残留只看年龄，不看存活判定"
    );
    assert!(legacy.exists());

    assert_eq!(
        sweep_with(dir.path(), later(MAX_AGE.as_secs() + 1), &dead),
        1,
        "超过年龄上限的旧命名残留应被清理"
    );
}

// ---------------------------------------------------------------------------
// 不误伤：不是我们的文件一律不碰
// ---------------------------------------------------------------------------

/// 普通文件、以及**名字像临时文件的目录**都不能删。
#[test]
fn unrelated_entries_are_ignored() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let note = touch(dir.path(), "note.txt");
    let own_file = touch(dir.path(), ".rustmark-draft-01.draft");
    let lookalike_dir = dir
        .path()
        .join(format!(".rustmark-tmp-{DEAD_PID}-2-directory"));
    fs::create_dir(&lookalike_dir).expect("应能建目录");

    let removed = sweep_with(dir.path(), later(MAX_AGE.as_secs() * 3), &dead);

    assert_eq!(removed, 0, "非临时文件 / 目录不应被清理");
    assert!(note.exists(), "用户的普通文件绝不能被删");
    assert!(own_file.exists());
    assert!(lookalike_dir.is_dir(), "同名目录不是临时文件");
}

/// 名字形状不合法（开头像数字但没有 `<seq>-<名字>` 两段）：只能按旧命名处理，
/// 绝不能把开头的数字误当 PID 去查存活。
#[test]
fn malformed_names_are_not_treated_as_pid_owner() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let malformed = touch(dir.path(), ".rustmark-tmp-12345");

    // 若把 `12345` 当 PID，`dead` 会让它立刻被删——这正是要防的误判。
    assert_eq!(
        sweep_with(dir.path(), SystemTime::now(), &dead),
        0,
        "形状不符的名字不得按 PID 判定"
    );
    assert!(malformed.exists());
}

/// PID 解析的边界：数字段必须齐全且为纯数字。
#[test]
fn pid_parsing_rejects_non_pid_shapes() {
    assert_eq!(pid_of(".rustmark-tmp-4242-1-note"), Some(4242));
    assert_eq!(pid_of(".rustmark-tmp-4242-1-a-b-c"), Some(4242));
    // 旧命名。
    assert_eq!(pid_of(".rustmark-tmp-note"), None);
    // 缺 `<seq>` 段 / `<seq>` 非数字 / 缺原文件名（末段为空）。
    assert_eq!(pid_of(".rustmark-tmp-4242"), None);
    assert_eq!(pid_of(".rustmark-tmp-4242-x-note"), None);
    assert_eq!(pid_of(".rustmark-tmp-4242-1-"), None);
    // 前缀不对。
    assert_eq!(pid_of("note"), None);
}

// ---------------------------------------------------------------------------
// 健壮性：清理失败绝不升级为错误
// ---------------------------------------------------------------------------

/// 目录不存在（首次运行、配置目录还没创建）→ 返回 0，不 panic、不报错。
#[test]
fn missing_directory_is_not_an_error() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let missing = dir.path().join("not-created");

    assert_eq!(sweep_stale(&missing), 0);
    // 幂等：再调一次也是 0。
    assert_eq!(sweep_stale(&missing), 0);
}

/// 每目录每进程只扫一次：第一次扫过之后，新出现的残留不会被第二次调用清掉。
///
/// 在 Linux 上这条是有牙齿的（真判定为死进程，若不节流就会被删）；
/// 在 Windows / macOS 上因存活判定为 `Unknown` + 文件新鲜而恰好也成立，
/// 故它同时是一条跨平台的「不 panic / 不误删」回归。
#[test]
fn sweep_parent_once_runs_once_per_directory() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    sweep_parent_once(dir.path());

    let created_after = touch(dir.path(), &format!(".rustmark-tmp-{DEAD_PID}-9-late"));
    sweep_parent_once(dir.path());

    assert!(created_after.exists(), "同一目录不应被重复扫描");
}
