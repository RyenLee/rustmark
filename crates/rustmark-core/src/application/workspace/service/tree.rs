//! 文件树用例：扫描、新建 / 移动 / 删除与按根重扫。

use crate::domain::workspace::value_objects::{EntryKind, WorkspaceEntry};
use crate::shared::result::AppResult;

use crate::application::dto::FileEntryView;

use super::super::commands::*;
use super::{TREE_MAX_DEPTH, WorkspaceAppService};

/// 由布尔构造 [`EntryKind`]（命令用布尔表达，避免 DTO 依赖领域枚举）。
const fn entry_kind(is_directory: bool) -> EntryKind {
    if is_directory {
        EntryKind::Directory
    } else {
        EntryKind::File
    }
}

impl WorkspaceAppService {
    /// 扫描文件树：写入聚合并发布 `WorkspaceChanged`。
    pub fn scan_tree(&self, cmd: ScanTreeCommand) -> AppResult<Vec<FileEntryView>> {
        let entries = self.repository.scan(&cmd.root, cmd.max_depth)?;
        {
            let mut ws = self.lock_write()?;
            ws.set_root(cmd.root.clone());
            ws.replace_entries(entries.clone());
        }
        self.bus
            .publish(crate::shared::event::DomainEvent::WorkspaceChanged(self.id));
        Ok(entries
            .iter()
            .map(|entry| FileEntryView {
                path: entry.path().clone(),
                is_directory: entry.kind().is_directory(),
            })
            .collect())
    }

    /// 新建文件或目录（S6-11：文件树右键菜单「新建」）。
    ///
    /// # 顺序：先在聚合的**副本**上验规则，再落盘，最后改真聚合
    ///
    /// 三种顺序各有代价，故逐一排除：
    ///
    /// - 「先落盘再改聚合」：规则拒绝时磁盘已经被改了。用户点了「移动」，
    ///   界面报错说「不能移进自己的子目录」，而文件其实已经不在原位了。
    /// - 「先改聚合再落盘」：落盘失败（权限、只读盘）时聚合里多了一个
    ///   不存在的条目，文件树会显示一个点开就报错的文件。
    /// - **副本验规则 → 落盘 → 改真聚合**：任一步失败，聚合都是原状。
    ///
    /// 副本的成本是一次 `Vec` 克隆，而这是**用户点击触发**而非每帧执行，
    /// 量级完全可接受。
    pub fn create_entry(&self, cmd: CreateEntryCommand) -> AppResult<()> {
        let entry = WorkspaceEntry::new(cmd.path.clone(), entry_kind(cmd.is_directory));
        self.check_on_clone(|ws| ws.add_entry(entry.clone()))?;
        // 目录创建用 `create_dir_all`（允许缺父目录），文件则必须父目录已存在——
        // 后者是刻意的：用户在一个不存在的目录里建文件时应当失败，
        // 而不是悄悄在别处造出一个目录来。
        if cmd.is_directory {
            self.repository.create_directory(&cmd.path)?;
        } else {
            self.repository.create_file(&cmd.path)?;
        }
        {
            let mut ws = self.lock_write()?;
            // 规则已在副本上验过、磁盘也已落盘，这一步按设计不会失败。
            // 若仍然失败，说明「副本 → 落盘 → 真聚合」之间状态漂移了
            // （并发改动，或副本与真身不一致）：此时磁盘已变更而聚合没有。
            // 沿用「磁盘优先、不回滚」的取舍，但**不能静默丢弃**——
            // 丢掉的后果是文件树长期与磁盘不一致且无人知晓。
            if let Err(err) = ws.add_entry(entry) {
                log::warn!(
                    "工作区已落盘但聚合未同步（新建 {}）：{}",
                    cmd.path.as_str(),
                    err
                );
            }
        }
        self.publish_changed();
        Ok(())
    }

    /// 重命名 / 移动条目（S6-11：右键菜单「重命名」与拖拽移动）。
    pub fn move_entry(&self, cmd: MoveEntryCommand) -> AppResult<()> {
        self.check_on_clone(|ws| ws.move_entry(&cmd.from, &cmd.to))?;
        self.repository.rename(&cmd.from, &cmd.to)?;
        {
            let mut ws = self.lock_write()?;
            // 同 `create_entry`：磁盘已改名而未落到聚合里，偏差必须可见。
            if let Err(err) = ws.move_entry(&cmd.from, &cmd.to) {
                log::warn!(
                    "工作区已落盘但聚合未同步（移动 {} → {}）：{}",
                    cmd.from.as_str(),
                    cmd.to.as_str(),
                    err
                );
            }
        }
        // 移动过的文件若在最近文件里，路径已失效——不清理的话
        // 欢迎页点它会报「文件不存在」，而用户会以为文件丢了。
        self.forget_recent_quietly(&cmd.from);
        self.publish_changed();
        Ok(())
    }

    /// 删除条目（S6-11：右键菜单「删除」）。
    ///
    /// 删除后必须重扫：磁盘上可能还有**聚合不知道**的文件（外部创建的、
    /// 上次扫描后才出现的）。只从聚合里删会让它们留在树上，
    /// 而点开时报「文件不存在」。
    pub fn delete_entry(&self, cmd: DeleteEntryCommand) -> AppResult<()> {
        self.check_on_clone(|ws| ws.remove_entry(&cmd.path))?;
        self.repository.remove(&cmd.path)?;
        self.rescan()?;
        self.forget_recent_quietly(&cmd.path);
        self.publish_changed();
        Ok(())
    }

    /// 按当前根目录重扫文件树。
    ///
    /// 未设置根目录时是**成功的空操作**（返回 `Ok`）而非错误：
    /// 用户还没打开工作区目录时，「刷新」按钮不该弹一条错误。
    pub fn rescan(&self) -> AppResult<usize> {
        let Some(root) = self.lock_read()?.root().cloned() else {
            return Ok(0);
        };
        let entries = self.scan_tree(ScanTreeCommand {
            root,
            max_depth: TREE_MAX_DEPTH,
        })?;
        Ok(entries.len())
    }
}
