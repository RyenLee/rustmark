//! 文档用例编排服务（阶段 04 · S4-04）。
//!
//! 编排打开 / 新建 / 保存 / 编辑 / 撤销重做 / 关闭，并负责发布领域事件。
//! 不持有 IO：磁盘读写交给注入的 [`DocumentRepository`]，缓冲区构造交给 [`BufferFactory`]。
//!
//! 失败语义优先（原型 2.1 静默失败的反面教材）：保存失败**不**调用 `mark_saved`，
//! 因此绝不清除脏标记。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::domain::document::aggregate::Document;
use crate::domain::document::errors::DocumentError;
use crate::domain::document::repository::{DocumentRepository, DocumentSnapshot};
use crate::domain::document::services::{BufferFactory, EditingService};
use crate::domain::document::value_objects::EditPlan;
use crate::shared::encoding::TextEncoding;
use crate::shared::error::AppError;
use crate::shared::event::{DomainEvent, EventBus};
use crate::shared::geometry::{ByteOffset, TextRange};
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;
use crate::shared::result::AppResult;
use crate::shared::time::Clock;

use super::DocumentContentProvider;
use super::commands::*;
use super::queries::*;
use super::registry::{DocumentRegistry, RegistryProvider};
use crate::application::dto::{
    CloseDecisionView, DocumentView, FormatResultView, RangeView, StatsView,
};

/// 文档用例服务：编排文档生命周期与编辑命令。
pub struct DocumentAppService {
    registry: Arc<RwLock<DocumentRegistry>>,
    repository: Arc<dyn DocumentRepository>,
    bus: Arc<dyn EventBus>,
    buffer_factory: Arc<dyn BufferFactory>,
    clock: Arc<dyn Clock>,
    /// 各文档**从磁盘读入时**识别到的编码（V11-03 / DEF-05）。
    ///
    /// 为何不进 `Document` 聚合：编码是 IO 的关注点（要读字节才知道），
    /// 聚合一旦持有它，领域层就被 IO 细节污染了。放在应用层做一张旁路表，
    /// 生命周期与文档一致（打开时记、关闭时删），表现层按需查询。
    ///
    /// 新建未保存的文档**不进表**——它没有编码可言，查询返回 `None`，
    /// 状态栏据此显示「—」而不是撒谎说 UTF-8。
    encodings: HashMap<DocumentId, TextEncoding>,
}

impl DocumentAppService {
    /// 构造服务（依赖全部注入）。
    pub fn new(
        registry: Arc<RwLock<DocumentRegistry>>,
        repository: Arc<dyn DocumentRepository>,
        bus: Arc<dyn EventBus>,
        buffer_factory: Arc<dyn BufferFactory>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            registry,
            repository,
            bus,
            buffer_factory,
            clock,
            encodings: HashMap::new(),
        }
    }

    /// 取出只读内容提供者（供 Markdown 预览 / 导出服务注入，复用同一份注册表状态）。
    pub fn provider(&self) -> Arc<dyn DocumentContentProvider> {
        Arc::new(RegistryProvider::new(self.registry.clone()))
    }

    /// 打开已有文件：加载 → 建聚合 → 发 `DocumentOpened`。
    pub fn open(&mut self, cmd: OpenFileCommand) -> AppResult<DocumentId> {
        let now = self.clock.now();
        let snapshot = self.repository.load(&cmd.path)?;
        let buffer = self.buffer_factory.create(&snapshot.content);
        let document = Document::open(
            snapshot.id,
            snapshot.path.clone(),
            buffer,
            snapshot.saved_at,
            now,
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
        let document = Document::new_untitled(buffer, now);
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

    /// 在指定偏移插入文本（替换选区请用 [`ReplaceRangeCommand`]）。
    pub fn insert_text(&mut self, cmd: InsertTextCommand) -> AppResult<()> {
        let now = self.clock.now();
        let events = {
            let mut reg = self.lock_write()?;
            let doc = reg
                .get_mut(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            doc.insert_at(ByteOffset::new(cmd.offset), &cmd.text, now)?;
            Self::drain_doc_events(doc)
        };
        self.publish_all(events);
        Ok(())
    }

    /// 把区间替换为文本。
    pub fn replace_range(&mut self, cmd: ReplaceRangeCommand) -> AppResult<()> {
        let now = self.clock.now();
        let events = {
            let mut reg = self.lock_write()?;
            let doc = reg
                .get_mut(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            doc.replace(cmd.range, &cmd.text, now)?;
            Self::drain_doc_events(doc)
        };
        self.publish_all(events);
        Ok(())
    }

    /// 应用一种格式指令（S6-07）。
    ///
    /// # 流程：取内容 → 领域算 `EditPlan` → 写回 → 发布事件 → 折算 DTO
    ///
    /// 规则全部来自 [`EditingService`]，本方法**不含任何 Markdown 拼接逻辑**——
    /// 那是「三入口一致」的前提：UI、快捷键、命令面板都只是本方法的不同调用方。
    ///
    /// 写回复用 [`Self::replace_range`]，因此格式操作与普通编辑**共享同一条撤销链**，
    /// Ctrl+Z 能把加粗撤掉（格式动作必须可撤销）。
    ///
    /// 返回 [`FormatResultView`] 而非 `()`：表现层需要新文本与新选区，
    /// 而它拿不到领域类型（`check-arch` 禁止 presentation 依赖 domain）。
    pub fn format(&mut self, cmd: FormatCommand) -> AppResult<FormatResultView> {
        // 1) 取当前内容。格式规则是纯函数，需要整篇文本作为上下文
        //    （例如判断是否「已包裹」要看包裹符是否紧邻在选区外）。
        let text = self.get_content(GetContentQuery {
            id: cmd.id,
            range: None,
        })?;

        // 2) 夹紧区间：调用方给的光标可能落在文本之外
        //    （例如文档刚被外部改短），不夹紧会在切片时 panic，
        //    而本项目 `panic = "abort"` 下等于终止进程。
        let range = TextRange::new(
            ByteOffset::new(cmd.range.start().value().min(text.len())),
            ByteOffset::new(cmd.range.end().value().min(text.len())),
        );

        let plan = match cmd.kind {
            FormatKind::Bold => EditingService::wrap_selection(&text, range, "**", "**"),
            FormatKind::Italic => EditingService::wrap_selection(&text, range, "*", "*"),
            FormatKind::Strikethrough => EditingService::wrap_selection(&text, range, "~~", "~~"),
            FormatKind::InlineCode => EditingService::wrap_selection(&text, range, "`", "`"),
            FormatKind::Quote => EditingService::toggle_line_prefix(&text, range, "> "),
            FormatKind::BulletList => EditingService::toggle_line_prefix(&text, range, "- "),
            FormatKind::PromoteHeading => EditingService::promote_heading(&text, range),
            FormatKind::DemoteHeading => EditingService::demote_heading(&text, range),
            FormatKind::OrderedList => Self::toggle_ordered_list(&text, range),
            FormatKind::ToggleTask => EditingService::toggle_task(&text, range.start()),
            FormatKind::InsertLink => EditingService::insert_link(&text, range),
            FormatKind::InsertCodeBlock => EditingService::insert_code_block(&text, range),
            FormatKind::InsertTable => {
                EditingService::insert_table(range.end(), TABLE_ROWS, TABLE_COLS)
            }
            FormatKind::Indent => {
                EditingService::indent_lines(&text, range, cmd.indent_unit.as_str())
            }
            FormatKind::Outdent => {
                EditingService::outdent_lines(&text, range, cmd.indent_unit.as_str())
            }
        };

        let edit = plan.edit();
        if edit.is_noop() {
            // 无效果编辑：不写、不发事件、不入撤销栈
            // （否则按一次 Ctrl+Z 只会「撤掉一个什么也没发生的操作」，
            //  用户会以为撤销坏了）。
            return Ok(FormatResultView {
                text,
                selection: RangeView::new(range.start().value(), range.end().value()),
                changed: false,
            });
        }

        // 3) 构造新文本并折算选区——必须在写回**之前**完成：
        //    领域给的选区是「编辑完成后在新内容中的位置」，
        //    写回后旧文本就没了。
        let new_text = apply_edit(&text, edit.range(), edit.replacement());
        let selection = shift_selection(&text, &new_text, plan.selection(), edit.range());

        // 4) 写回。走 `replace_range` 以复用事件发布与撤销链。
        self.replace_range(ReplaceRangeCommand {
            id: cmd.id,
            range: edit.range(),
            text: edit.replacement().to_string(),
        })?;

        Ok(FormatResultView {
            text: new_text,
            selection,
            changed: true,
        })
    }

    /// 切换有序列表：先探测范围内已用的序号，没有则从 1 起。
    ///
    /// 有序列表不能用固定前缀（`toggle_line_prefix` 只会插同一个字符串），
    /// 否则从第 5 行开始插入会得到「1. 1. 1.」。
    fn toggle_ordered_list(text: &str, range: TextRange) -> EditPlan {
        // 必须先把区间**扩展到整行**再探测序号：`toggle_line_prefix`
        // 内部会这么做，但探测发生在它之前。若直接用原始区间
        // （光标是空区间时为空串），`first` 永远回退到 1，
        // 于是 `3. third` 会被拼成 `1. 3. third`（序号被叠加）。
        let lines = EditingService::expand_to_full_lines(text, range);
        let first = text
            .get(lines.as_usize_range())
            .unwrap_or("")
            .lines()
            .filter_map(leading_list_number)
            .next()
            .unwrap_or(1);
        let prefix = format!("{first}. ");
        EditingService::toggle_line_prefix(text, range, prefix.as_str())
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

    /// 设置光标 / 选区（空区间表示仅移动光标、清除选区）。
    pub fn set_selection(&mut self, cmd: SetSelectionCommand) -> AppResult<()> {
        let events = {
            let mut reg = self.lock_write()?;
            let doc = reg
                .get_mut(cmd.id)
                .ok_or_else(|| AppError::domain("文档未打开"))?;
            doc.select(cmd.selection);
            Self::drain_doc_events(doc)
        };
        self.publish_all(events);
        Ok(())
    }

    /// 查询单个文档视图。
    pub fn get_document(&self, cmd: GetDocumentQuery) -> AppResult<DocumentView> {
        let reg = self.lock_read()?;
        let doc = reg
            .get(cmd.id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        Ok(to_document_view(doc))
    }

    /// 查询文档内容；`range = None` 取全文。
    pub fn get_content(&self, cmd: GetContentQuery) -> AppResult<String> {
        let reg = self.lock_read()?;
        let doc = reg
            .get(cmd.id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        match cmd.range {
            None => Ok(doc.text()),
            Some(range) => Ok(doc.slice(range)?),
        }
    }

    /// 查询字数统计。
    pub fn get_stats(&self, cmd: GetStatsQuery) -> AppResult<StatsView> {
        let reg = self.lock_read()?;
        let doc = reg
            .get(cmd.id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        Ok(compute_stats(doc.text().as_str(), doc.meta().title()))
    }

    /// 查询当前选区（无选区返回 `None`）。
    pub fn get_selection(&self, cmd: GetSelectionQuery) -> AppResult<Option<RangeView>> {
        let reg = self.lock_read()?;
        let doc = reg
            .get(cmd.id)
            .ok_or_else(|| AppError::domain("文档未打开"))?;
        Ok(doc
            .cursor()
            .selection()
            .map(|r| RangeView::new(r.start().value(), r.end().value())))
    }

    /// 列出全部已打开文档。
    pub fn list_open(&self, _cmd: ListOpenDocumentsQuery) -> AppResult<Vec<DocumentView>> {
        let reg = self.lock_read()?;
        Ok(reg
            .ids()
            .iter()
            .filter_map(|id| reg.get(*id).map(to_document_view))
            .collect())
    }

    // ---------- 草稿 / 恢复（DEF-02）----------

    /// 把未保存草稿落到磁盘（异常退出后可恢复，需求 §4.3 / NFR-REL-03）。
    ///
    /// 内容取自注册表当前态，**不**走领域事件：写草稿只是「崩溃保险」，
    /// 不应污染文档的脏标记或触发保存流程。
    pub fn save_draft(&self, drafts_dir: &Path, id: DocumentId) -> AppResult<()> {
        let content = self.get_content(GetContentQuery { id, range: None })?;
        self.repository.save_draft(drafts_dir, id, &content)
    }

    /// 读取某文档的遗留草稿（不存在返回 `None`）。
    pub fn load_draft(&self, drafts_dir: &Path, id: DocumentId) -> AppResult<Option<String>> {
        self.repository.load_draft(drafts_dir, id)
    }

    /// 删除某文档的草稿（已正常保存 / 已恢复 / 已丢弃后清理）。
    pub fn clear_draft(&self, drafts_dir: &Path, id: DocumentId) -> AppResult<()> {
        self.repository.clear_draft(drafts_dir, id)
    }

    /// 扫描草稿目录，返回全部遗留草稿 `(文档 ID, 内容)`（启动恢复用）。
    pub fn pending_recoveries(&self, drafts_dir: &Path) -> AppResult<Vec<(DocumentId, String)>> {
        let ids = self.repository.list_drafts(drafts_dir)?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(content) = self.load_draft(drafts_dir, id)? {
                out.push((id, content));
            }
        }
        Ok(out)
    }

    /// 从一段内容直接新建未命名文档（草稿恢复 / 模板插入用）。
    ///
    /// 与 [`Self::new_document`] 的区别仅在于初始内容：恢复出来的草稿
    /// 不应该是空文档。新建即发 `DocumentOpened`，与正常打开走同一条事件链。
    pub fn new_document_with_content(&mut self, content: &str) -> AppResult<DocumentId> {
        let now = self.clock.now();
        let buffer = self.buffer_factory.create(content);
        let document = Document::new_untitled(buffer, now);
        let id = document.id();
        {
            let mut reg = self.lock_write()?;
            reg.insert(document);
        }
        self.bus.publish(DomainEvent::DocumentOpened(id));
        Ok(id)
    }

    // ---------- 内部 ----------

    fn lock_read(&self) -> AppResult<std::sync::RwLockReadGuard<'_, DocumentRegistry>> {
        self.registry
            .read()
            .map_err(|_| AppError::io("文档注册表读锁已损坏"))
    }

    fn lock_write(&self) -> AppResult<std::sync::RwLockWriteGuard<'_, DocumentRegistry>> {
        self.registry
            .write()
            .map_err(|_| AppError::io("文档注册表写锁已损坏"))
    }

    /// 把聚合事件队列转换为总线事件（`take_events` 必须在锁内调用）。
    fn drain_doc_events(doc: &mut Document) -> Vec<DomainEvent> {
        doc.take_events().into_iter().map(Into::into).collect()
    }

    /// 在**不持有注册表锁**的前提下发布事件。
    ///
    /// 事件发布是同步调用订阅者回调，而订阅者（如 `EventSubscribers` 的
    /// `DocumentSaved` 处理）会通过 [`DocumentContentProvider`] 反向读取同一份注册表。
    /// 若持锁发布，同一线程重入 `RwLock` 将直接死锁——故所有事件必须在锁释放后才发出。
    fn publish_all(&self, events: Vec<DomainEvent>) {
        for event in events {
            self.bus.publish(event);
        }
    }
}

/// 把 `Document` 聚合投影为 [`DocumentView`](crate::application::dto::DocumentView)。
/// 「插入表格」按钮生成的表格尺寸。
///
/// 固定 3×3：它只是骨架，用户按需增删行列。做成命令参数反而要在
/// 工具栏上再加两个输入框，与 S6-07「一个按钮一个动作」的语义不符。
const TABLE_ROWS: usize = 3;
const TABLE_COLS: usize = 3;

/// 把一次编辑施加到文本上，返回新文本。
///
/// 切片全部走 `get().unwrap_or("")`：偏移若落在非字符边界（例如调用方
/// 用字符数当字节数传进来），`&text[a..b]` 会 panic，
/// 而本项目 `panic = "abort"` 下等于终止进程——宁可少替换一点也不能崩。
fn apply_edit(text: &str, range: TextRange, replacement: &str) -> String {
    let start = range.start().value().min(text.len());
    let end = range.end().value().min(text.len()).max(start);
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(text.get(..start).unwrap_or(""));
    out.push_str(replacement);
    out.push_str(text.get(end..).unwrap_or(""));
    out
}

/// 把领域给出的选区（**新文本**坐标系）折算成可返回的区间。
///
/// `EditPlan::selection` 已经是「编辑完成后在新内容里」的位置，
/// 所以这里**不需要**平移——只需夹紧到新文本长度内。
/// `None` 表示领域未指定选区，此时把光标放在**替换文本末尾**，
/// 与 `TextEdit` 插入后的默认行为一致，用户可立刻继续输入。
fn shift_selection(
    _old: &str,
    new_text: &str,
    selection: Option<TextRange>,
    edit_range: TextRange,
) -> RangeView {
    let len = new_text.len();
    let sel = selection.unwrap_or_else(|| {
        let at = edit_range.start().value().saturating_add(
            edit_range
                .end()
                .value()
                .saturating_sub(edit_range.start().value()),
        );
        TextRange::new(ByteOffset::new(at), ByteOffset::new(at))
    });
    let start = sel.start().value().min(len);
    let end = sel.end().value().min(len).max(start);
    RangeView::new(start, end)
}

/// 取一行开头的有序列表序号（`3. ` → `Some(3)`）。
///
/// 只认「数字 + `. `」，避免把正文里的 `2024. 年计划` 误判为列表项。
fn leading_list_number(line: &str) -> Option<u32> {
    let t = line.trim_start();
    let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 9 {
        return None;
    }
    t[digits.len()..]
        .starts_with(". ")
        .then(|| digits.parse().ok())?
}

pub fn to_document_view(doc: &Document) -> DocumentView {
    let text = doc.text();
    let preview: String = text.chars().take(120).collect();
    DocumentView {
        id: doc.id(),
        title: doc.meta().title().to_string(),
        path: doc.path().cloned(),
        content_preview: preview,
        is_dirty: doc.is_dirty(),
        cursor: doc.cursor().position().value(),
        selection: doc
            .cursor()
            .selection()
            .map(|r| RangeView::new(r.start().value(), r.end().value())),
        line_count: text.split('\n').count().max(1),
        byte_len: doc.len_bytes(),
    }
}

/// 由文档全文与标题计算统计信息（口径逐项对齐原型 `computeStats`）。
///
/// # 为什么不按空白切分
///
/// 原型的口径是**中日韩按字、西文按词**（`countWords`）。
/// `split_whitespace().count()` 把一整段没有空格的中文算成 1 个「词」，
/// 中文文档的字数会因此少到离谱（一篇 800 字的中文稿显示「8 字」）。
/// 两者不是精度差异，是**数量级**差异。
pub fn compute_stats(text: &str, title: &str) -> StatsView {
    let mut headings = 0;
    let mut code_blocks = 0;
    let mut tasks_done = 0;
    let mut tasks_total = 0;
    let mut paragraphs = 0;
    // 上一个字符是不是空白（或在文首）——用来判「新的非空段落开始了」。
    let mut in_blank = true;
    // 当前所在的围栏（` ``` ` 或 `~~~ `）；非空表示在代码块**内**。
    //
    // 围栏内的内容必须整体跳过：代码示例里写 `## 不是标题`
    // 不该被计成标题，写 `- [ ] 不是任务` 不该被计成任务。
    let mut fence: Option<String> = None;

    for raw in text.split('\n') {
        let line = raw.trim();
        let fence_token = fence_token_of(raw);
        if let Some(open) = fence.as_deref() {
            if fence_token.as_deref() == Some(open) {
                fence = None;
            }
            continue;
        }
        if let Some(token) = fence_token {
            fence = Some(token);
            code_blocks += 1;
            continue;
        }
        if is_atx_heading(line) {
            headings += 1;
        }
        if let Some(done) = task_marker_of(line) {
            tasks_total += 1;
            if done {
                tasks_done += 1;
            }
        }
        if line.is_empty() {
            in_blank = true;
        } else if in_blank {
            paragraphs += 1;
            in_blank = false;
        }
    }

    let (links, images) = count_links_and_images(text);
    let words = count_words(text);
    StatsView {
        words,
        characters: text.chars().count(),
        characters_no_space: text.chars().filter(|c| !c.is_whitespace()).count(),
        lines: text.split('\n').count().max(1),
        paragraphs,
        headings,
        code_blocks,
        links,
        images,
        title: title.to_string(),
        tasks_done,
        tasks_total,
        // 原型：`max(1, round(words / 300))`。
        reading_minutes: words.div_ceil(300).max(1) as u32,
    }
}

/// 是否是 ATX 标题（`#` ~ `######` 后必须跟空格）。
fn is_atx_heading(line: &str) -> bool {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(' ')
}

/// 取出行首的围栏标记（` ``` ` 或 `~~~ `，含其后的语言名）。
///
/// 返回 `None` 表示这行不是围栏。**只看行首**：代码块内部的
/// 缩进代码里出现 ```` ``` ```` 才会提前闭合，与 CommonMark 一致。
fn fence_token_of(line: &str) -> Option<String> {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    // 超过 3 个空格的缩进是缩进代码块而非围栏。
    if indent > 3 {
        return None;
    }
    let body = line.trim_start_matches([' ', '\t']);
    for marker in ["```", "~~~"] {
        if let Some(rest) = body.strip_prefix(marker) {
            // 闭合围栏后面不能跟语言名（`` ```rust `` 是开启而非闭合），
            // 但这里不做这个区分：开启与闭合都返回同一个 token，
            // 由调用方用「当前是否已在围栏内」决定语义。
            let _ = rest;
            return Some(marker.to_string());
        }
    }
    None
}

/// 判定任务列表项；返回 `Some(已完成)`。
///
/// `-` 与 `[` 之间**允许有空白**（`- [ ]` 是最常见写法，也是原型
/// 正则 `^\s*[-*+]\s\[([ xX])\]` 明确允许的）；`[` 与状态位之间
/// 不允许（`[ x]` 不是合法任务项）。
fn task_marker_of(line: &str) -> Option<bool> {
    let rest = line.strip_prefix(['-', '*', '+'])?.trim_start();
    let rest = rest.strip_prefix('[')?;
    match rest.chars().next()? {
        ' ' => Some(false),
        'x' | 'X' => Some(true),
        _ => None,
    }
}

/// 统计链接与图片数。
///
/// 图片 `![说明](地址)` **不计入**链接数（原型口径），
/// 否则一张图会在「链接」和「图片」两栏各数一次。
fn count_links_and_images(text: &str) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut links = 0;
    let mut images = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'[' && bytes[i] != b'!' {
            i += 1;
            continue;
        }
        let is_image = bytes[i] == b'!';
        // `!` 后面必须紧跟 `[`，否则 `!` 是普通字符。
        let open = if is_image {
            if bytes.get(i + 1) != Some(&b'[') {
                i += 1;
                continue;
            }
            i + 1
        } else {
            i
        };
        match link_tail_len(&bytes[open..]) {
            Some(len) => {
                if is_image {
                    images += 1;
                } else {
                    links += 1;
                }
                i = open + len;
            }
            None => i += 1,
        }
    }
    (links, images)
}

/// 从 `[` 开始匹配到配对的 `)`，返回**含** `[` 与 `)` 的总长度。
///
/// 括号可嵌套：`[文字](a(b)c)` 是合法链接，扫到第一个 `)` 就停会把它
/// 误拆成「链接 + 残余」两段。链接地址里的 `)` 不转义时按嵌套配对处理。
fn link_tail_len(bytes: &[u8]) -> Option<usize> {
    debug_assert_eq!(bytes.first(), Some(&b'['));
    // 找到配对的 `]`（`]` 可出现在链接文字里，故记深度）。
    let mut i = 1;
    let mut bracket_depth = 1usize;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => bracket_depth += 1,
            b']' => {
                bracket_depth -= 1;
                if bracket_depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if i >= bytes.len() {
        return None; // 没有 `]`
    }
    if bytes.get(i + 1) != Some(&b'(') {
        return None; // `[]` 后面不是 `(` → 不是链接（可能是引用式 `[^1]`）
    }
    // 从 `(` 起算括号深度。
    i += 2;
    let mut paren_depth = 1usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => paren_depth += 1,
            b')' => {
                paren_depth -= 1;
                if paren_depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None // 括号没闭合
}

/// 字数：中日韩按字 + 西文按词（对齐原型 `countWords`）。
///
/// 「西文单词」= 连续的 `[A-Za-z0-9_'-]`。连字符与下划线算单词内字符，
/// 因为 `well-known` 是一个词，拆成两个会虚高。
///
/// # 与原型正则的一处有意偏离
///
/// 原型的正则是 `/[A-Za-z0-9_'-]+/g`，它把**孤立的连字符**也算成一个词：
/// `- [ ] todo` 里的 `-` 命中一次、`[x]` 里的 `x` 命中一次，
/// 于是一份 5 词的任务清单会显示「8 字」。
///
/// 故这里加一条判据：单词必须**至少含一个字母或数字**才算。
/// 破折号、星号、方括号都是 Markdown 的结构符号而非词，
/// 把它们计进字数只会让「列表越多字数越虚高」。
/// 这也是原型该修的地方（它自己的 `charsNoSpace` 也没排除它们，口径自相矛盾）。
fn count_words(text: &str) -> usize {
    let mut cjk = 0;
    let mut western = 0;
    let mut current = String::new();
    let flush = |token: &mut String, western: &mut usize| {
        if token.chars().any(|c| c.is_ascii_alphanumeric()) {
            *western += 1;
        }
        token.clear();
    };
    for ch in text.chars() {
        if is_cjk(ch) {
            flush(&mut current, &mut western);
            cjk += 1;
        } else if ch.is_ascii_alphanumeric() || ch == '_' || ch == '\'' || ch == '-' {
            current.push(ch);
        } else {
            flush(&mut current, &mut western);
        }
    }
    flush(&mut current, &mut western);
    cjk + western
}

/// 是否是「按字计」的东亚文字。
///
/// 覆盖范围与原型一致：CJK 统一表意文字 + 日文假名。
/// 刻意**不含**韩文（`\u{AC00}` 起）：原型的正则没算它，
/// 口径对齐优先于「看起来更完整」。
fn is_cjk(ch: char) -> bool {
    matches!(ch, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}' | '\u{3040}'..='\u{30ff}')
}

/// 由已有路径派生「另存为」路径（同目录、文件名插入 `-copy`）。
fn derive_save_as_path(original: &FilePath) -> FilePath {
    let name = original.file_name();
    let new_name = match name.rfind('.') {
        Some(idx) => format!("{}-copy{}", &name[..idx], &name[idx..]),
        None => format!("{}-copy", name),
    };
    match original.parent() {
        Some(parent) => parent.join(&new_name).unwrap_or_else(|_| original.clone()),
        None => original.clone(),
    }
}

/// 编译期断言：`DocumentAppService` 可跨线程共享。
const fn _assert_send_sync<T: Send + Sync>() {}
const _ASSERT_DOC_SERVICE: () = _assert_send_sync::<DocumentAppService>();
