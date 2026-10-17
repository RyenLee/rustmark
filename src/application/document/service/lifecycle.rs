//! 文档生命周期用例：打开 / 新建 / 保存 / 另存 / 撤销重做 / 关闭，
//! 以及读入编码旁路表的维护。

use crate::domain::document::aggregate::Document;
use crate::domain::document::errors::DocumentError;
use crate::domain::document::repository::DocumentSnapshot;
use crate::shared::encoding::TextEncoding;
use crate::shared::error::AppError;
use crate::shared::event::DomainEvent;
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;
use crate::shared::result::AppResult;

use crate::application::dto::CloseDecisionView;

use super::super::commands::*;
use super::DocumentAppService;
use super::mapping::derive_save_as_path;

impl DocumentAppService {
    /// 打开已有文件：加载 → 建聚合 → 发 `DocumentOpened`。
    pub fn open(&mut self, cmd: OpenFileCommand) -> AppResult<DocumentId> {
        let now = self.clock.now();
        let snapshot = self.repository.load(&cmd.path)?;
        let buffer = self.buffer_factory.create(&snapshot.content);
        let document = Document::open_with_policy(
            snapshot.id,
            snapshot.path.clone(),
            buffer,
            snapshot.saved_at,
            now,
            self.undo_policy,
        );
        let id = document.id();
        // 记下读入时识别到的编码，供状态栏显示真实编码（V11-03 / DEF-05）。
        if let Some(encoding) = snapshot.encoding {
            self.encodings.insert(id, encoding);
        }
        {
            let mut reg = self.lock_write()?;
            reg.insert(document);
        }
        self.bus.publish(DomainEvent::DocumentOpened(id));
        Ok(id)
    }

    /// 某文档读入时识别到的编码；新建未保存的文档返回 `None`（V11-03）。
    ///
    /// 保存**不改变**这里的值：保存写回的是原编码（另存为也沿用原编码），
    /// 故编码在整个文档生命周期内保持稳定，直到关闭。
    pub fn encoding_of(&self, id: DocumentId) -> Option<TextEncoding> {
        self.encodings.get(&id).copied()
    }

    /// 首次落盘后补记编码（V11-03）。
    ///
    /// 目标文件**不存在**时，基础设施层按 UTF-8 无 BOM 写入
    /// （`encode_as` 的「无原文件」分支），故可确定地记为 `Utf8`；
    /// 目标**已存在**时，基础设施层会读原文件并写回它的编码，
    /// 此处**不猜**——猜错就等于在状态栏显示一个假编码。
    fn note_encoding_after_first_write(&mut self, id: DocumentId, path: &FilePath) {
        if !self.encodings.contains_key(&id) && !self.repository.exists(path) {
            self.encodings.insert(id, TextEncoding::Utf8);
        }
    }

    /// 新建空白未命名文档：建聚合 → 发 `DocumentOpened`。
    pub fn new_document(&mut self, _cmd: NewDocumentCommand) -> AppResult<DocumentId> {
        let now = self.clock.now();
        let buffer = self.buffer_factory.create("");
        let document = Document::new_untitled_with_policy(buffer, self.undo_policy, now);
        let id = document.id();
        {
            let mut reg = self.lock_write()?;
            reg.insert(document);
        }
        self.bus.publish(DomainEvent::DocumentOpened(id));
        Ok(id)
    }

    /// 保存文档：取内容 → 仓储原子写 → 聚合 `mark_saved` → 发 `DocumentSaved`。
    ///
    /// **关键失败路径**：若 `repository.save` 失败，提前返回，**不**调用 `mark_saved`，
    /// 从而保留脏标记（脏文档在失败后仍是脏的，不会被误判为已保存）。
    pub fn save(&mut self, cmd: SaveCommand) -> AppResult<()> {
        let now = self.clock.now();
        let (content, path) = {
            let mut reg = self.lock_write()?;
            let doc = reg
                .get_mut(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            let path = match (doc.path().cloned(), cmd.save_as) {
                (Some(path), false) => path,
                (Some(path), true) => derive_save_as_path(&path),
                (None, _) => return Err(AppError::domain("未命名文档需先指定保存路径")),
            };
            (doc.text(), path)
        };

        // 锁已释放，再执行可能阻塞的磁盘 IO，避免持锁期间阻塞预览线程。
        self.repository
            .save(&DocumentSnapshot::new(cmd.id, path.clone(), content, None))?;
        self.note_encoding_after_first_write(cmd.id, &path);

        // 写盘成功才标记已保存；`NotDirty`（已干净）视为幂等成功。
        let events = {
            let mut reg = self.lock_write()?;
            let doc = reg
                .get_mut(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            match doc.mark_saved(path, now) {
                Ok(()) => {}
                Err(DocumentError::NotDirty) => {}
                Err(e) => return Err(e.into()),
            }
            Self::drain_doc_events(doc)
        };
        // 锁外发布：订阅者会反向读注册表，持锁发布会重入 RwLock 死锁。
        self.publish_all(events);
        Ok(())
    }

    /// 保存到指定路径（S6-11f：另存为对话框回填）。
    ///
    /// 与 [`Self::save`] 的差别只有**路径来源**：这里是调用方
    /// （原生对话框）给的确定路径，而 `save` 是从聚合里取的
    /// （另存为则派生 `-copy`）。
    ///
    /// 为什么不把路径塞进 [`SaveCommand`] 的可选字段：那会让
    /// 「选了路径却仍存成 `-copy`」成为可能——两个语义混在一个
    /// 结构体里，靠 `if let` 分支区分，迟早有一处写错。
    pub fn save_to(&mut self, cmd: SaveToPathCommand) -> AppResult<()> {
        let now = self.clock.now();
        let content = {
            let reg = self.lock_read()?;
            let doc = reg
                .get(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            doc.text()
        };
        // 锁外写盘：同 `save` 的理由（持锁期间阻塞磁盘 IO）。
        self.repository.save(&DocumentSnapshot::new(
            cmd.id,
            cmd.path.clone(),
            content,
            None,
        ))?;
        self.note_encoding_after_first_write(cmd.id, &cmd.path);
        let events = {
            let mut reg = self.lock_write()?;
            let doc = reg
                .get_mut(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            match doc.mark_saved(cmd.path.clone(), now) {
                Ok(()) => {}
                // 文档干净**且**路径没变时领域会拒绝（幂等成功）。
                // 但本方法的存在意义正是「存到新路径」，
                // 若路径确实不同，`mark_saved` 不会走到这个分支。
                Err(DocumentError::NotDirty) => {}
                Err(e) => return Err(e.into()),
            }
            Self::drain_doc_events(doc)
        };
        self.publish_all(events);
        Ok(())
    }

    pub fn undo(&mut self, cmd: UndoCommand) -> AppResult<()> {
        let now = self.clock.now();
        let events = {
            let mut reg = self.lock_write()?;
            let doc = reg
                .get_mut(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            doc.undo(now)?;
            Self::drain_doc_events(doc)
        };
        self.publish_all(events);
        Ok(())
    }

    /// 重做一步；空栈返回 [`DocumentError::RedoStackEmpty`] 映射的错误。
    pub fn redo(&mut self, cmd: RedoCommand) -> AppResult<()> {
        let now = self.clock.now();
        let events = {
            let mut reg = self.lock_write()?;
            let doc = reg
                .get_mut(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            doc.redo(now)?;
            Self::drain_doc_events(doc)
        };
        self.publish_all(events);
        Ok(())
    }

    /// 关闭文档。
    ///
    /// - `force = true` 或文档已干净：标记关闭、发 `DocumentClosed`、移除注册表项；
    /// - 否则（脏且非强制）：返回 [`CloseDecisionView::NeedsSave`]，**保持打开**。
    pub fn close(&mut self, cmd: CloseCommand) -> AppResult<CloseDecisionView> {
        let is_dirty = {
            let reg = self.lock_read()?;
            reg.get(cmd.id).map(|doc| doc.is_dirty()).unwrap_or(false)
        };

        if cmd.force || !is_dirty {
            // 文档生命周期结束：编码记录一并删除，避免旁路表无限增长。
            self.encodings.remove(&cmd.id);
            let events = {
                let mut reg = self.lock_write()?;
                let doc = reg
                    .get_mut(cmd.id)
                    .ok_or_else(|| AppError::domain("文档未打开"))?;
                doc.mark_closed();
                doc.take_events()
            };
            for event in events {
                self.bus.publish(event.into());
            }
            {
                let mut reg = self.lock_write()?;
                reg.remove(cmd.id);
            }
            Ok(CloseDecisionView::Closed)
        } else {
            Ok(CloseDecisionView::NeedsSave)
        }
    }

    /// 从一段内容直接新建未命名文档（草稿恢复 / 模板插入用）。
    ///
    /// 与 [`Self::new_document`] 的区别仅在于初始内容：恢复出来的草稿
    /// 不应该是空文档。新建即发 `DocumentOpened`，与正常打开走同一条事件链。
    pub fn new_document_with_content(&mut self, content: &str) -> AppResult<DocumentId> {
        let now = self.clock.now();
        let buffer = self.buffer_factory.create(content);
        let document = Document::new_untitled_with_policy(buffer, self.undo_policy, now);
        let id = document.id();
        {
            let mut reg = self.lock_write()?;
            reg.insert(document);
        }
        self.bus.publish(DomainEvent::DocumentOpened(id));
        Ok(id)
    }
}
