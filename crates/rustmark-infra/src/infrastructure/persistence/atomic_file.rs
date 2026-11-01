//! 原子文件写入的**唯一实现**（文档 / 草稿 / 配置 / 导出产物共用）。
//!
//! # 为什么要把两份实现合成一份
//!
//! 此前同一件事有两个版本：`fs_document_repository` 自己一份（文档、草稿），
//! `recent_files_store` 又一份（偏好、最近文件、导出产物）。两份的临时文件命名
//! 还不一样：前者 `.rustmark-tmp-<pid>-<seq>-<name>`，后者仍是老式的
//! `.rustmark-tmp-<name>`。这带来两个真实后果（评审 P3）：
//!
//! 1. **导出路径上原样留着已修掉的 bug**：`pulldown_html_exporter` /
//!    `system_pdf_exporter` 走老命名，两个实例同时导出到同一目标时会互相
//!    `truncate`——正是文档保存早在 P2 就修掉的竞争，只是换了条路径存在。
//! 2. **残留清理各清一半**：两份代码互相看不见对方命名的残留。
//!
//! 故命名、同目录、`sync`、`rename`、父目录 `fsync`、失败清理、残留清扫
//! 在这里各定义一次，调用方只提供「写哪个路径」和「写什么字节」。
//!
//! # 原子性契约（`FR-04`）
//!
//! 直接 `write(target)` 有一个致命问题：**写到一半失败会留下截断的文件**。
//! 用户的原文就这样没了——这是编辑器最不可接受的故障模式。流程：
//!
//! 1. 在**目标同目录**写临时文件（必须同目录：跨目录 `rename` 在多数系统上
//!    不是原子操作，且可能跨文件系统失败）；
//! 2. `flush` + `sync_all` 确保数据真正落到磁盘（否则断电后仍可能丢失）；
//! 3. `fs::rename` 覆盖目标——POSIX 与 Windows 的 rename 都是原子的
//!    （Windows 上目标被占用时可能失败，此时**原文件仍然完好**，符合契约）；
//! 4. **Unix** 上再对父目录 `fsync` 一次：POSIX 不保证 rename 后的目录项
//!    已持久化，断电可能让「刚保存的文件」消失。这一步是 **best-effort**
//!    ——失败只记日志，绝不因此报错或回滚已成功的保存。
//!
//! 任一步失败都**不触碰目标文件**，并尽力清理临时文件。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rustmark_core::shared::error::AppError;

use super::tmp_residue;

/// 临时文件名前缀：残留识别与清理都依赖这个约定。
pub(crate) const TMP_PREFIX: &str = ".rustmark-tmp-";

/// 临时文件名的进程内单调计数器（配合进程 ID 保证唯一）。
///
/// 只用 `Relaxed`：这里需要的仅是「同进程内两次取值不同」，
/// 不与任何其它内存操作建立 happens-before 关系。
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 生成临时文件路径：`<父目录>/.rustmark-tmp-<pid>-<seq>-<原文件名>`。
///
/// 三条约束都是硬要求：
/// - **同父目录**：跨目录 `rename` 不保证原子，还可能跨文件系统失败；
/// - **`.rustmark-tmp-` 前缀**：残留识别与清理都依赖这个约定；
/// - **进程 ID + 进程内计数器**：同一目标被并发保存时（多实例、多线程、
///   或上一次崩溃遗留的同名临时文件），两次写绝不能落到同一个 tmp 上——
///   否则后一个 `truncate(true)` 会把前一个正在写的临时文件截断，
///   `rename` 之后目标文件的内容就错了。
///
/// P3 起配置 / 导出也走这里：导出到同一目标的两个实例不再互相截断。
pub(crate) fn tmp_path_for(target: &Path) -> PathBuf {
    let parent = parent_of(target);
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        "{TMP_PREFIX}{}-{seq}-{}",
        std::process::id(),
        file_name_of(target)
    ))
}

/// 上一代命名（无 PID / 计数器）的残留路径 `.rustmark-tmp-<原文件名>`。
///
/// 命名的变更必须处理**前一代命名的遗留物**：老版本崩溃时会在目标目录留下
/// 这个文件，新版本的 `rename` 永远用不到这个名字，于是它成了永久垃圾。
/// 故每次写之前顺手清掉它。
///
/// 为什么删它不会伤到别人：新代码从不使用这个格式的文件名（一定带 PID 与
/// 计数器）；唯一可能是「同一台机器上还跑着旧版本」——那种情况下旧实例的
/// `rename` 会失败并提示用户重试，**目标文件不会被损坏**（写失败不进目标）。
pub(crate) fn legacy_tmp_path_for(target: &Path) -> PathBuf {
    parent_of(target).join(format!("{TMP_PREFIX}{}", file_name_of(target)))
}

/// 目标的父目录（取不到时用当前目录，便于错误信息里给出可读路径）。
fn parent_of(target: &Path) -> &Path {
    target.parent().unwrap_or_else(|| Path::new("."))
}

/// 目标文件名（取不到时用 `untitled`）。
fn file_name_of(path: &Path) -> &str {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("untitled")
}

/// 让父目录的目录项落盘（Unix 专有，**best-effort**）。
///
/// POSIX 只保证 `rename` 的原子性，**不保证**新目录项已经持久化：断电后可能
/// 出现「文件数据在盘上，但目录里仍指向旧 inode / 新名字不存在」的窗口。
/// 对编辑器来说这等于「刚保存的文件消失了」，故 rename 成功后再对父目录
/// `fsync` 一次。
///
/// 失败处理的原则：**恢复性**优先于**持久性**。此时目标文件已是新内容，
/// 若因目录 fsync 失败而向上返回 Err，调用方会以为「没保存成功」并可能
/// 提示用户重试或丢弃编辑——那才是真正的事故。故这里只记日志，
/// 不回滚、不报错（best-effort durability）。
///
/// Windows 不在此列：`MoveFileEx` 的持久性语义与 POSIX 不同，且无法用
/// `File::open` 打开目录再 `sync_all`，整段跳过。
#[cfg(unix)]
pub(crate) fn sync_parent_dir(parent: &Path) {
    match fs::File::open(parent) {
        Ok(dir) => {
            if let Err(error) = dir.sync_all() {
                log::warn!(
                    "父目录 fsync 失败（保存已完成，仅影响断电后的目录项持久性）{}：{error}",
                    parent.display()
                );
            }
        }
        Err(error) => {
            log::warn!(
                "无法打开父目录做 fsync（保存已完成）{}：{error}",
                parent.display()
            );
        }
    }
}

/// 非 Unix 平台的空实现：见 Unix 版本的说明。
#[cfg(not(unix))]
pub(crate) fn sync_parent_dir(_parent: &Path) {}

/// 原子写入：同目录临时文件 → sync → rename。
///
/// 调用方拿到的 `Err` 一律意味着**目标文件未被改动**（此前的旧内容仍在）。
pub(crate) fn atomic_write(target: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = target
        .parent()
        .ok_or_else(|| AppError::io(format!("路径没有父目录，无法写入：{}", target.display())))?;
    // 父目录不存在时创建——用户「另存为」到新目录、或首次写配置的常见场景。
    if !parent.exists() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::io(format!("无法创建目录 {}：{e}", parent.display())))?;
    }

    // 清掉同目录下**确定已死**的旧残留：每目录每进程只扫一次，见 `tmp_residue`。
    tmp_residue::sweep_parent_once(parent);

    let tmp_path = tmp_path_for(target);

    // 顺手清掉旧命名（无 PID / 计数器）的崩溃残留：新命名不会覆盖它，
    // 不清就是永久垃圾（见 `legacy_tmp_path_for` 的说明）。
    let legacy_tmp = legacy_tmp_path_for(target);
    if legacy_tmp.exists() {
        let _ = fs::remove_file(&legacy_tmp);
    }

    // 写入失败时删除残留临时文件——绝不留垃圾在用户目录里。
    let write_result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        // 同步到磁盘：断电 / 崩溃后 rename 也不应指向未落盘的数据。
        file.sync_all()
    })();

    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(AppError::io(format!(
            "写入临时文件失败（{}）：{error}",
            tmp_path.display()
        )));
    }

    // rename 失败（如 Windows 上文件被占用）时同样清理临时文件。
    // 此时**原文件未被触碰**，符合 `FR-04` 的「不破坏原文件」契约。
    if let Err(error) = fs::rename(&tmp_path, target) {
        let _ = fs::remove_file(&tmp_path);
        return Err(AppError::io(format!(
            "无法覆盖目标文件（{}）：{error}。文件可能正被其它程序占用",
            target.display()
        )));
    }

    // rename 成功即「写入已成功」：此后的任何失败都不允许回滚或报错
    // （best-effort durability，见 `sync_parent_dir` 的说明）。
    sync_parent_dir(parent);
    Ok(())
}
