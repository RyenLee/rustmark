//! 崩溃残留临时文件的清理（评审 P3）。
//!
//! # 为什么需要它
//!
//! 原子写入的临时文件名是 `.rustmark-tmp-<pid>-<seq>-<原文件名>`。进程若正好
//! 崩在「写完 tmp、还没 rename」之间，那个变量就再也没人引用：进程没了，
//! 文件留在用户目录里——看不见的垃圾，且每崩一次多一个。P2 之前「复用同名
//! 临时文件」的旧实现能侥幸自己消化掉；带上 PID / 计数器之后，复用不再发生，
//! 残留就成永久垃圾。故必须有人来扫。
//!
//! # 两种时机
//!
//! - **启动**（[`sweep_stale`]）：只扫配置目录。用户文档目录无法在启动时穷举
//!   （最近文件列表也只是子集），扫整个磁盘更不可能；
//! - **保存**（[`sweep_parent_once`]）：扫目标所在目录，**每目录每进程一次**。
//!   保存时恰好路过用户真正在用的目录，覆盖成本几乎为零。
//!
//! # 判定规则：宁漏勿错
//!
//! 删错文件比留一点垃圾严重得多（用户的真实文件不可恢复），故逐条收紧：
//!
//! 1. 不是 `.rustmark-tmp-` 前缀 → 不碰；
//! 2. 不是**普通文件**（目录、符号链接）→ 不碰；
//! 3. PID 等于当前进程 → 不碰：可能是本进程别的线程正在写的临时文件，
//!    删了会让那次保存失败（虽然目标文件不会坏）；
//! 4. PID 是别人的：Linux 上查 `/proc/<pid>`，**进程已不存在才删**；
//!    其它平台（macOS / Windows）没有零依赖的存活探测手段，一律按
//!    「判不出来」处理，退回年龄兜底；
//! 5. 年龄兜底：修改时间早于一小时才删。任何合法的临时文件都只活到
//!    `write_all + sync_all` 的那几毫秒，一小时的宽限只为「绝不误删」。
//!
//! 判定为残留后删除失败（权限、文件被占用）只记 `debug` 日志：清理是
//! 锦上添花，任何情况下都不能影响一次正常保存。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use super::atomic_file::TMP_PREFIX;

/// 年龄兜底阈值：超过这个年龄的临时文件，即使判不出进程是否存活也算残留。
///
/// 一小时远超任何一次真实写入（含慢速网络盘上的 `sync_all`），故不会误删
/// 正在进行中的写入；同时又能在一周内消化掉用户不再访问的目录里的残留。
const MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// 临时文件所属进程的存活判定结果。
///
/// 非 Linux 平台上 [`liveness`] 只会返回 `Unknown`，另外两个变体照旧保留：
/// 它们是这条规则的完整语义（测试也直接构造它们），删掉会让「判不出来」
/// 与「判出来是死的」在类型层面混为一谈。
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Liveness {
    /// 进程还在（它的临时文件可能正在写）。
    Alive,
    /// 进程已经不存在（留下的只可能是崩溃残留）。
    Dead,
    /// 本平台 / 本环境判不出来（如 Windows、macOS，或 `/proc` 未挂载）。
    Unknown,
}

/// 已扫过的目录（每目录每进程只扫一次）。`None` 表示还没扫过任何目录。
///
/// 加这个缓存是为了不让「每次 Ctrl+S 都 `read_dir` 一遍所在目录」成为成本：
/// 大目录（下载目录动辄上万项）在 Windows 上并不便宜。
static SWEPT_DIRS: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// 清理 `dir` 下判定为残留的临时文件，返回删除个数。
///
/// 只在启动时对配置目录调用（[`sweep_parent_once`] 覆盖不到「用户再也不会
/// 打开的目录」）。目录不存在、不可读、删除失败都只返回 0 或跳过，
/// **不会**返回错误——启动流程不该被一次清理挡住。
pub fn sweep_stale(dir: &Path) -> usize {
    let removed = sweep_with(dir, SystemTime::now(), &liveness);
    if removed > 0 {
        log::info!("清理了 {removed} 个崩溃残留的临时文件：{}", dir.display());
    }
    removed
}

/// 清理 `dir`，但每个目录在本进程内只做一次（供 [`super::atomic_file`] 调用）。
pub(crate) fn sweep_parent_once(dir: &Path) {
    if !mark_swept(dir) {
        return;
    }
    let removed = sweep_with(dir, SystemTime::now(), &liveness);
    if removed > 0 {
        log::debug!("清理了 {removed} 个崩溃残留的临时文件：{}", dir.display());
    }
}

/// 登记「本进程已扫过该目录」，返回是否**首次**登记。
///
/// 锁中毒（别的线程 panic 在扫描期间）时返回 `false`：跳过清理，
/// 而把 panic 继续留给上层——清理模块没有资格吞掉别人的故障。
fn mark_swept(dir: &Path) -> bool {
    let Ok(mut guard) = SWEPT_DIRS.lock() else {
        return false;
    };
    guard
        .get_or_insert_with(HashSet::new)
        .insert(dir.to_path_buf())
}

/// 清扫主体（`now` 与存活判定均可注入，故这条规则在 Windows 开发机上也能测）。
fn sweep_with(dir: &Path, now: SystemTime, liveness: &dyn Fn(u32) -> Liveness) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        // `file_type()` 不跟随符号链接：链接本身不会被当成普通文件删掉。
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(TMP_PREFIX) {
            continue;
        }
        let modified = entry.metadata().ok().and_then(|m| m.modified().ok());
        if !is_stale(name, modified, now, liveness) {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(error) => log::debug!("清理残留临时文件失败 {}：{error}", entry.path().display()),
        }
    }
    removed
}

/// 判定一个临时文件是否是「可以删的残留」（规则见模块文档）。
fn is_stale(
    name: &str,
    modified: Option<SystemTime>,
    now: SystemTime,
    liveness: &dyn Fn(u32) -> Liveness,
) -> bool {
    match pid_of(name) {
        // 自己的临时文件一律不碰：别的线程可能正写着它。
        Some(pid) if pid == std::process::id() => false,
        Some(pid) => match liveness(pid) {
            Liveness::Alive => false,
            Liveness::Dead => true,
            Liveness::Unknown => older_than_max_age(modified, now),
        },
        // 旧命名（无 PID）无从判定归属，只按年龄兜底。
        None => older_than_max_age(modified, now),
    }
}

/// 从 `.rustmark-tmp-<pid>-<seq>-<原文件名>` 里取出 `<pid>`。
///
/// 形状不符（旧命名的 `.rustmark-tmp-<原文件名>`、手工改过的名字）返回 `None`
/// ——**绝不能**把「文件名开头恰好是数字」误当成 PID 去判定，那会导致按错误的
/// 进程状态删一个真实文件。
fn pid_of(name: &str) -> Option<u32> {
    let rest = name.strip_prefix(TMP_PREFIX)?;
    let mut segments = rest.splitn(3, '-');
    let pid = segments.next()?.parse::<u32>().ok()?;
    let seq = segments.next()?;
    if seq.is_empty() || !seq.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if segments.next()?.is_empty() {
        return None;
    }
    Some(pid)
}

/// 修改时间是否早于 [`MAX_AGE`]。
fn older_than_max_age(modified: Option<SystemTime>, now: SystemTime) -> bool {
    // 取不到修改时间就不删（判不了的东西按「可能是活的」处理）。
    let Some(modified) = modified else {
        return false;
    };
    match now.duration_since(modified) {
        Ok(age) => age > MAX_AGE,
        // 修改时间在「现在」之后（时钟回拨 / 跨机器同步的目录）→ 当作新文件。
        Err(_) => false,
    }
}

/// 判定 `pid` 的存活状态。
///
/// Linux 用 `/proc/<pid>`：进程表里没有它 = 已死，这是精确判定。`/proc` 未挂载
/// （极简容器、非 Linux 内核）时退回 `Unknown`，让年龄兜底生效。
/// 其它平台没有零依赖的探测手段（macOS 要 `kill(pid, 0)`、Windows 要
/// `OpenProcess`），都不引入 libc / winapi 只为这件事，一律 `Unknown`。
fn liveness(pid: u32) -> Liveness {
    #[cfg(target_os = "linux")]
    {
        if !Path::new("/proc/self").exists() {
            return Liveness::Unknown;
        }
        return if Path::new(&format!("/proc/{pid}")).exists() {
            Liveness::Alive
        } else {
            Liveness::Dead
        };
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        Liveness::Unknown
    }
}

#[cfg(test)]
mod tests;
