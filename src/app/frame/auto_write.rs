//! 「自动保存写回原文件」的磁盘保护（`frame::auto_write`）。
//!
//! # 为什么需要这一层
//!
//! 自动保存原本只写草稿（ADR-003）：草稿落在数据目录，永远不碰用户文件。
//! 「写回原文件」把这个前提换掉了——每 30 秒一次 `atomic_write` 覆盖磁盘，
//! 用户可能根本没意识到文件已经变了。若此时 Git 切分支、另一个编辑器或
//! 同步盘改过同一个文件，一次无人值守的覆盖就是**不可逆的数据丢失**。
//! 手动 Ctrl+S 时用户有意识地决定覆盖；自动写回没有这个决定，所以必须在
//! 写之前自己判定「磁盘还是不是我们上次同步时的那份」。
//!
//! # 判据：磁盘内容指纹，不用 mtime
//!
//! `DocumentMeta::last_saved_at` 在 `mark_saved` 后写的是逻辑时钟
//! （生产里是系统时钟、测试里是 `TestClock`），与文件 mtime 不是同一个源；
//! 拿它跟 `fs::metadata().modified()` 比会在测试里必然误判。改用**内容指纹**：
//! 打开文档时记下当时磁盘字节的哈希，此后每拍写回前重算一次，不等就判定
//! 外部改动。哈希只在进程内比较，用 FNV-1a 足够，不引入新依赖。
//!
//! # 冲突后的处置
//!
//! 检测到外部改动 → 标记 `Blocked`、状态栏提示**一次**、本拍与后续所有拍
//! 只写草稿（不覆盖磁盘）。`Blocked` 不会因为下一次节拍自动解除：磁盘上
//! 的改动仍在，自动流程没有资格替用户决定谁对。只有用户手动 Ctrl+S
//! （显式覆盖）或「还原修改」（显式采纳磁盘版本）才刷新基线并解锁。
//!
//! # 范围
//!
//! 只对**当前活动文档**写回；其余到期文档仍走草稿路径，未命名文档没有
//! 原文件可写，也退回草稿。这个范围由 `files.rs::run_autosave` 判定。
use super::*;

/// 某文档在自动写回上的状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AutoWriteState {
    /// 上次与磁盘同步时的内容指纹；写回前要比对。
    Baseline(u64),
    /// 检测到外部改动，写回被拒；直到用户手动处置才解除。
    Blocked,
}

/// 一次写回尝试的结果：决定本节拍还要不要补写草稿。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AutoWriteOutcome {
    /// 已写回原文件（`save_document` 已清草稿并同步脏标记）。
    Written,
    /// 未写回，调用方继续写草稿。
    DraftOnly,
}

/// 磁盘内容指纹（FNV-1a 64 位）；读不到文件返回 `None`。
fn disk_fingerprint(path: &str) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    Some(bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    }))
}

impl EditorApp {
    /// 取指定文档的路径（轻量摘要，不取全文）；未命名文档返回 `None`。
    fn auto_write_path(&self, id: DocumentId) -> Option<FilePath> {
        self.services
            .documents
            .get_tab_summary(GetDocumentQuery { id })
            .ok()?
            .path
    }

    /// 打开文档时记下磁盘基线：此刻之后磁盘若被别的程序改动，写回必须停手。
    ///
    /// 传的是**原始路径字符串**（未经 `FilePath` 归一化）：`std::fs::read`
    /// 需要 OS 能直接打开的形式，归一化后的 `/` 形式在 Windows 上也能用，
    /// 但没必要多一层转换。
    pub(crate) fn seed_auto_write_baseline(&mut self, id: DocumentId, raw_path: &str) {
        // 默认关闭时**一次磁盘 IO 都不加**：绝大多数用户不会开启写回，
        // 不该让「打开一份 1MB 文档」平白多读一遍全文。开关在设置页
        // 从关→开时由 `seed_open_auto_write_baselines` 为已打开的文档补齐。
        if !self.settings().editor.auto_save_to_source {
            return;
        }
        match disk_fingerprint(raw_path) {
            Some(hash) => {
                self.auto_write_guard
                    .insert(id, AutoWriteState::Baseline(hash));
            }
            None => {
                self.auto_write_guard.remove(&id);
            }
        }
    }

    /// 开关刚从关→开时，为**已经打开**的文档补播种基线。
    ///
    /// 这些文档是在开关关闭时打开的，当时刻意跳过了读盘；不补齐的话，
    /// 它们要等下一次重新打开才享有外部改动保护。
    pub(crate) fn seed_open_auto_write_baselines(&mut self) {
        for doc in self.open_document_list() {
            if let Some(path) = doc.path.as_ref() {
                self.seed_auto_write_baseline(doc.id, path.as_str());
            }
        }
    }

    /// 手动保存 / 还原成功后刷新基线并解除封锁。
    ///
    /// 用户已经显式处置过磁盘（覆盖它或采纳它），此前那次「冲突」不再是
    /// 自动流程能替他判断的事；不解锁会让功能永久停摆且毫无提示。
    pub(crate) fn note_auto_write_synced(&mut self, id: DocumentId) {
        if !self.settings().editor.auto_save_to_source {
            return;
        }
        let hash = self
            .auto_write_path(id)
            .and_then(|path| disk_fingerprint(path.as_str()));
        match hash {
            Some(hash) => {
                self.auto_write_guard
                    .insert(id, AutoWriteState::Baseline(hash));
            }
            None => {
                // 读不到磁盘（内存仓储 / 文件被删）：宁可没有基线，
                // 也不留一个陈旧值让下一拍误判成「外部改动」。
                self.auto_write_guard.remove(&id);
            }
        }
    }

    /// 尝试把当前活动文档写回原文件；调用方负责在 `DraftOnly` 时补写草稿。
    ///
    /// 失败一律**不抛**、不中断其余文档的自动保存：写回是增强能力，
    /// 草稿才是崩溃恢复的保底（ADR-003）。
    pub(crate) fn try_auto_write_to_source(&mut self, id: DocumentId) -> AutoWriteOutcome {
        let Some(path) = self.auto_write_path(id) else {
            // 未命名文档：没有原文件可写，退回草稿，不必打扰用户。
            return AutoWriteOutcome::DraftOnly;
        };
        if self.auto_write_guard.get(&id) == Some(&AutoWriteState::Blocked) {
            // 已提示过一次，后续节拍静默跳过，别每 30 秒刷一条状态栏。
            return AutoWriteOutcome::DraftOnly;
        }
        let Some(current) = disk_fingerprint(path.as_str()) else {
            // 读不到磁盘（被删除 / 权限 / 内存仓储）：不猜，保守退回草稿。
            return AutoWriteOutcome::DraftOnly;
        };
        let external_change = matches!(
            self.auto_write_guard.get(&id),
            Some(AutoWriteState::Baseline(expected)) if *expected != current
        );
        if external_change {
            self.auto_write_guard.insert(id, AutoWriteState::Blocked);
            self.set_status_error(&format!(
                "「{}」已被其他程序改动，已跳过自动写回；本次修改仅存草稿，请核对后再手动保存。",
                file_name_of(path.as_str())
            ));
            return AutoWriteOutcome::DraftOnly;
        }
        // 无基线（如草稿恢复出来的文档）：以本次读到的磁盘内容为基准，
        // 写回成功后由 `save_document` 里的 `note_auto_write_synced` 刷新。
        // 复用手动保存入口，白拿三件事：原子写、清草稿、脏标记回读同步。
        match self.save_document(false) {
            Ok(()) => AutoWriteOutcome::Written,
            Err(e) => {
                self.set_status_error(&format!("自动写回原文件失败：{e}"));
                AutoWriteOutcome::DraftOnly
            }
        }
    }
}
