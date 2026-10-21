//! 文件与文档操作：保存 / 新建 / 路径读取 / 自动保存 / 拖放打开。
//!
//! 状态栏写入口在 `status_bar.rs`（R-21 归位），光标插入在 `editor.rs`，
//! 打开管线（守卫 / 拒绝文案 / 入口 / 实现）在 `workspace.rs`。
use super::*;

impl EditorApp {
    // ================================================================
    // 文件动作（`execute` 的分支实现）
    // ================================================================

    /// 保存 / 另存为。
    ///
    /// 「未命名文档没有路径」时**不弹错误**而是转成「另存为」请求：
    /// 用户按 `Ctrl+S` 的意图是「把这份东西存下来」，
    /// 弹一个「需先指定保存路径」等于把「另存为」这一步的工作推给用户，
    /// 而用户按 `Ctrl+Shift+S` 就能做到同一件事。
    pub(super) fn run_save(&mut self, save_as: bool) {
        let needs_path = self
            .active_doc
            .and_then(|id| {
                self.services
                    .documents
                    .get_document(GetDocumentQuery { id })
                    .ok()
            })
            .is_none_or(|v| v.path.is_none());
        if needs_path {
            self.request_dialog(PendingDialog::SaveFile);
            return;
        }
        if let Err(e) = self.save_document(save_as) {
            self.set_status_error(&e.to_string());
        }
    }

    /// 新建文档（`Ctrl+N`）。
    pub(super) fn run_new_document(&mut self) {
        if let Err(e) = self.new_document() {
            self.set_status_error(&e.to_string());
        }
    }

    /// 当前文档的**保存路径**（`None` = 未保存过的新文档）。
    ///
    /// 这是「当前文档路径」的**唯一出处**（V11-54a / R-15）：
    /// 此前有三处各自取路径（`current_document_path` 的哨兵字符串、
    /// `document_dir` 的 `PathBuf`、`active_document_dir` 的 `String`），
    /// 语义相近却**成本不同**（`active_document_dir` 走 `get_document`
    /// 全文快照，每帧调用就是白付一次 O(全文) 克隆——问题 043 的教训）。
    ///
    /// 走轻量摘要（`get_tab_summary`）而非 `get_document`：面包屑每帧都取，
    /// 后者要付一次全文快照只为拿一个路径。
    pub fn current_document_file(&self) -> Option<FilePath> {
        let id = self.active_doc?;
        self.services
            .documents
            .get_tab_summary(GetDocumentQuery { id })
            .ok()?
            .path
    }

    /// 当前文档的面包屑路径；无活动文档或无路径时返回「未命名文档」。
    ///
    /// 走轻量摘要而非 `get_document`：面包屑每帧都取，后者要付一次
    /// 全文快照只为拿一个路径（问题 043）。
    pub fn current_document_path(&self) -> String {
        self.current_document_file()
            .map(|p| p.as_str().to_string())
            .unwrap_or_else(|| "未命名文档".to_string())
    }

    // ================================================================
    // S6-11f：浮层编排
    // ================================================================

    /// 当前生效主题的显示名（「关于」面板用）。
    pub fn theme_kind_label(&self) -> &'static str {
        crate::presentation::theme::builtin::theme_kind_label(self.ui.theme.kind)
    }

    /// 读取一个工作区文件的文本内容（差异对比的右侧）。
    pub fn read_workspace_file(&self, path: &str) -> AppResult<String> {
        let target = FilePath::new(path).map_err(|e| AppError::Config {
            message: format!("路径非法：{e}"),
        })?;
        self.services
            .workspace
            .read_file(ReadFileQuery { path: target })
    }

    // ================================================================
    // 关闭浮层
    // ================================================================

    /// 新建空白文档并切到它。
    ///
    /// 「切换」而不只是「创建」：用户在欢迎页点「新建文件」期待的是
    /// **立刻能打字**，若只是建了文档却仍停在旧标签，他会以为按钮坏了。
    ///
    /// # 打开文档数量上限（问题 038）
    ///
    /// 新建也是「多开一个标签」，与 [`Self::open_workspace_file`] 同一条
    /// 限额规则。注意 [`Self::ensure_document`] 的兜底**不经这里**
    /// （那时列表为空，永远够名额），不会把编辑区卡在「一页都没有」。
    pub fn new_document(&mut self) -> AppResult<()> {
        let limit = self.settings().editor.max_open_documents;
        if self.open_document_list().len() >= limit as usize {
            self.set_status_notice(format!(
                "已达打开文档上限（{limit} 个），可先关闭部分标签或在设置里调整上限"
            ));
            return Ok(());
        }
        let id = self.services.documents.new_document(NewDocumentCommand)?;
        self.active_doc = Some(id);
        self.find.on_document_switched(id);
        self.refresh_text();
        Ok(())
    }

    /// 保存当前文档。
    ///
    /// `save_as = true` 时另存副本。**未命名文档没有路径**，
    /// 领域会拒绝并返回「未命名文档需先指定保存路径」——
    /// 那是正确行为：静默存到某个猜测的位置比报错更糟。
    pub fn save_document(&mut self, save_as: bool) -> AppResult<()> {
        let Some(id) = self.ensure_document() else {
            return Ok(());
        };
        self.services.documents.save(SaveCommand { id, save_as })?;
        // 已正常保存：草稿失去意义，清掉以免启动时误报「待恢复」。
        let _ = self
            .services
            .documents
            .clear_draft(&self.services.drafts_dir, id);
        // 内容没变但「已保存」状态变了，仍要回读让脏标记与标签标题一致。
        self.refresh_text();
        Ok(())
    }

    /// 「还原修改」（R-04a）：从磁盘重新加载当前文档，丢弃未保存修改。
    ///
    /// 与 [`Self::undo_edit`] 是两件事：撤销回退一步编辑，还原回到磁盘版本。
    /// 路由表里 `Revert` 是独立动作（UI 标签「还原修改」），不是 `Undo` 的别名。
    ///
    /// # 三条分支
    ///
    /// - 无活动文档 / 未命名文档：没有磁盘版本可还原，提示后返回
    ///   （未命名文档猜一个保存位置比不动作更糟）；
    /// - 文档干净：直接还原，不为一个零风险操作弹框；
    /// - 文档脏：先走确认框（丢弃修改不可逆），答复「是」才兑现。
    pub fn revert_document(&mut self) {
        let Some(id) = self.ensure_document() else {
            return;
        };
        let Some(path) = self.current_document_file() else {
            self.set_status_notice("未命名文档尚未保存过，无从还原");
            return;
        };
        if self.current_document_dirty() {
            self.request_revert(id, &path);
            return;
        }
        self.run_revert(id);
    }

    /// 兑现还原：调应用层用例 → 清草稿 → 同步 UI。
    ///
    /// **草稿必须清**：自动保存的 `.draft` 里躺着刚被丢弃的修改，
    /// 不清则下次启动会问「是否恢复未保存修改」——把用户明确扔掉的内容
    /// 又捡回来，还与「还原」的语义直接矛盾（保存后清草稿同此理）。
    pub(super) fn run_revert(&mut self, id: DocumentId) {
        match self.services.documents.revert(RevertCommand { id }) {
            Ok(()) => {
                let _ = self
                    .services
                    .documents
                    .clear_draft(&self.services.drafts_dir, id);
                if self.active_doc == Some(id) {
                    // 领域内容已换：回读文本 + 把 egui 光标对齐到领域光标
                    // （还原后光标在文档头，与「重新打开」一致）。
                    self.sync_cursor_from_domain();
                    let name = self
                        .current_document_file()
                        .map(|p| Self::display_name(p.as_str()).to_string())
                        .unwrap_or_else(|| "文档".to_string());
                    self.set_status_notice(format!("已还原「{name}」"));
                } else {
                    // 确认框是模态的，正常情况下走不到这里；保留 generic 文案
                    // 是为了「按 id 兑现」不依赖「当时谁是活动文档」。
                    self.set_status_notice("已还原");
                }
            }
            Err(e) => self.set_status_error(&format!("还原失败：{e}")),
        }
    }

    /// 保存**全部**已修改文档（`Ctrl+Alt+S` / 「文件 → 保存全部」）。
    ///
    /// # 为什么不在 `execute` 里循环调 `save_document`
    ///
    /// `save_document` 存的是**活动**文档；这里要的是「所有脏文档」。
    /// 且未命名文档没有路径，领域必然拒绝——若逐个调 `save_document`，
    /// 得自己在调用处处理「哪个有路径、哪个没有」，那段逻辑放在视图或
    /// 动作分支里都会与单文档保存的真实语义漂移。
    ///
    /// # 未命名文档：计数提示，不弹对话框
    ///
    /// 一次批量保存不可能连着弹 N 个系统对话框。未命名文档缺的是**路径**，
    /// 用户自己知道要先「另存为」，故单独计数并在提示里说清，
    /// 不让它们以「保存失败」的面貌出现。
    ///
    /// # 失败不中断
    ///
    /// 某个文档写盘失败（磁盘满 / 权限）不应让后面的脏文档一个都存不下来——
    /// 逐个 `save` 各自捕获错误，最后汇总成一条状态栏提示。
    pub(super) fn save_all_documents(&mut self) {
        let dirty: Vec<TabSummary> = self
            .open_document_list()
            .into_iter()
            .filter(|doc| doc.is_dirty)
            .collect();
        if dirty.is_empty() {
            self.set_status_notice("没有未保存的文档");
            return;
        }
        let mut saved = 0usize;
        let mut unnamed = 0usize;
        let mut failed: Vec<String> = Vec::new();
        for doc in &dirty {
            if doc.path.is_none() {
                unnamed += 1;
                continue;
            }
            match self.services.documents.save(SaveCommand {
                id: doc.id,
                save_as: false,
            }) {
                Ok(()) => {
                    saved += 1;
                    // 与单文档保存同一条后守：草稿失去意义，清掉以免
                    // 下次启动误报「待恢复」。
                    let _ = self
                        .services
                        .documents
                        .clear_draft(&self.services.drafts_dir, doc.id);
                }
                Err(e) => {
                    let name = doc
                        .path
                        .as_ref()
                        .map(|p| Self::display_name(p.as_str()).to_string())
                        .unwrap_or_else(|| doc.title.clone());
                    failed.push(format!("{name}：{e}"));
                }
            }
        }
        // 脏标记与标签标题要回读：保存后 `is_dirty` 变了，
        // 不回读会出现「内容已存盘但标签还显示脏点」。
        self.refresh_text();
        let mut message = format!("已保存 {saved} 个文档");
        if unnamed > 0 {
            message.push_str(&format!("；{unnamed} 个未命名文档需先另存为"));
        }
        if failed.is_empty() {
            self.set_status_notice(message);
        } else {
            self.set_status_error(&format!(
                "{message}；{} 个保存失败：{}",
                failed.len(),
                failed.join("；")
            ));
        }
    }

    /// 自动保存节拍（DEF-02 修复；问题 068 补「到期帧」调度）。
    ///
    /// 读出当前「自动保存间隔」，询问计时器哪些文档已空闲达间隔，
    /// 为到期文档写入草稿。空转极廉价——未到期时 `due` 立即返回空列表，不落盘。
    ///
    /// 计时器的输入来自 `EventSubscribers` 订阅的 `TextModified` 事件
    /// （在 `bootstrap` 装配阶段接线），故**只有真正编辑过的脏文档**才会进入到期队列。
    ///
    /// # 为什么必须向帧循环「登记到期点」（问题 068 根因）
    ///
    /// egui 是**事件驱动重绘**：没有输入事件就不产生新帧。本方法挂在帧循环里，
    /// 用户停止输入后它就不再被调用——间隔到期也无人执行，自动保存表现为
    /// 「完全不生效」（只有碰巧动一下鼠标才顺带补写）。因此只要还有文档在
    /// 计时，就把「剩余时长后的那一刻」登记给 [`FrameScheduler`]，
    /// 由它保证到期时刻必定有一帧跑到这里。
    ///
    /// 注意签名里**没有 `ctx`**：本方法不做任何预约，只登记。预约的唯一出口是
    /// 帧末的 `flush`——把 `&egui::Context` 递给这里，就等于又开了一个
    /// 「可以自己 request_repaint_after」的后门（T11 要收掉的正是它）。
    ///
    /// # 为什么是 `pub`
    ///
    /// 与 `apply_dialog_outcome` 同一理由：集成测试要驱动**真实登记路径**。
    /// 若测试自己 `arm(Deadline::AutoSave, ..)`，被测的就成了 `FrameScheduler`
    /// 而非「自动保存有没有接线」，后者才是历史上出错的地方。
    pub fn run_autosave(&mut self) {
        let interval = self
            .services
            .preferences
            .get_settings(GetSettingsQuery)
            .ok()
            .and_then(|s| s.editor.auto_save_interval);
        let Some(secs) = interval else {
            return;
        };
        if secs == 0 {
            return; // 0 表示「关闭自动保存」
        }
        let interval_ms = secs as u64 * 1000;
        let now = self.services.markdown.now();
        let due = self.services.auto_save_tracker.due(now, interval_ms);
        if !due.is_empty() {
            let drafts_dir = self.services.drafts_dir.clone();
            for id in due {
                // 失败不再静默吞掉（问题 068）：草稿是崩溃恢复的唯一凭据，
                // 写失败还不告知，用户既没有草稿也没有任何线索。
                if let Err(e) = self.services.documents.save_draft(&drafts_dir, id) {
                    self.set_status_error(&format!("自动保存失败：{e}"));
                }
            }
        }
        // 为「下一次到期」登记到期点（问题 068 的根因；T11 收口到统一调度）。
        // `due` 已取走到期文档，剩余文档按各自的记录时刻算出最近的到期点；
        // 没有在计时的文档时返回 `None`，不登记（下一次击键的帧自然会重新调度）。
        //
        // 这里**只登记、不预约**：`request_repaint_after` 集中由帧末的
        // `FrameScheduler::flush` 发出。调用点自己预约是问题 068 之后
        // 「新增定时功能就漏一次」的来源——统计/高亮/消息 TTL 三处都漏过。
        if let Some(remaining_ms) = self
            .services
            .auto_save_tracker
            .next_due_in(now, interval_ms)
        {
            self.scheduler.arm(
                crate::app::frame::Deadline::AutoSave,
                now.millis().saturating_add(remaining_ms),
            );
        }
    }

    /// 当前文档是否有未保存修改。
    pub fn current_document_dirty(&self) -> bool {
        self.active_doc
            .and_then(|id| {
                self.services
                    .documents
                    .get_tab_summary(GetDocumentQuery { id })
                    .ok()
            })
            .is_some_and(|v| v.is_dirty)
    }

    /// 当前文档的类型标签（状态栏左端「Markdown」用）。
    ///
    /// 按**扩展名**判定而非写死「Markdown」：编辑器能打开 `.txt`，
    /// 状态栏却显示「Markdown」会让用户怀疑自己打开了错的文件。
    /// 扩展名提取与展示名都取自 [`crate::shared::text_format`]（唯一出处）——
    /// 在这里另写一张匹配表，就会出现「白名单加了一种格式、状态栏却仍然
    /// 显示『纯文本』」的漂移。
    pub fn current_document_kind(&self) -> &'static str {
        let Some(id) = self.active_doc else {
            return text_format::display_name("");
        };
        let ext = self
            .services
            .documents
            .get_tab_summary(GetDocumentQuery { id })
            .ok()
            .and_then(|v| v.path)
            // 走到这里路径已是 `FilePath`（归一化过），故用值对象自己的
            // `extension()`；它与 `text_format::extension_of` 同源。
            .and_then(|p| p.extension().map(str::to_string));
        match ext {
            Some(ext) => text_format::display_name(&ext),
            None => text_format::display_name(""),
        }
    }

    /// 打开最近文件列表（按最近使用降序）。
    ///
    /// 欢迎页与「文件 → 最近」共用。**每次现取**而不是缓存：
    /// 最近文件会被本应用自己的打开操作改写，缓存必然出现
    /// 「刚打开的文件不在最近列表里」这种自相矛盾的状态。
    pub fn recent_files(&self) -> Vec<RecentFileView> {
        self.services
            .workspace
            .list_recent(ListRecentQuery)
            .unwrap_or_default()
    }

    /// 文件拖放打开（P2）。
    ///
    /// egui/winit 后端在「拖放发生的那一帧」把文件填进 `ctx.input().raw.dropped_files`，
    /// 下一帧即清空，因此每帧读一次即可，无需手动消费或去重。
    ///
    /// 行为（推荐方案）：
    /// - 按扩展名分类（`classify_dropped`，判据是**全应用唯一**的格式白名单）——
    ///   白名单内的扩展名（内置清单见
    ///   [`crate::shared::text_format::BUILTIN_TEXT_EXTENSIONS`]）
    ///   逐个打开为标签；其余扩展名拒绝并在状态栏提示；
    /// - 拖入无本地路径的「虚拟文件」（如浏览器拖拽）忽略；
    /// - 一次拖入多个文件：全部可识别的都打开（批量），不截断到第一个；
    /// - **目录**在分类前单独拦下：目录也可能带 `.md` 后缀，若放进扩展名分类
    ///   会被当作文本交给 `open_path`，最终以「无法读取文件」报错——
    ///   一次误拖换来一条误导性的红色错误。目录不是异常，提示语用「忽略」不用「拒绝」。
    pub(super) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        // egui 0.37 起 `DroppedFile` 是 trait，`path()` 返回 `&Path`（必存在，不再是 `Option`）。
        let dropped: Vec<String> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_string_lossy().to_string())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        // 目录判定用真实文件系统：扩展名对目录没有任何意义。
        let dir_count = dropped
            .iter()
            .filter(|p| std::path::Path::new(p.as_str()).is_dir())
            .count();
        let files: Vec<String> = dropped
            .into_iter()
            .filter(|p| !std::path::Path::new(p.as_str()).is_dir())
            .collect();
        // 分类用**应用持有的那份**白名单（而非内置常量）：将来接设置页允许
        // 用户追加扩展名时，拖放会跟着一起放行，不必再改这里。
        let (open, reject) = classify_dropped(&self.text_formats, &files);
        for p in &open {
            let _ = self.open_path(p);
        }
        // 组装「打开了什么、忽略了什么」的提示：两类忽略原因分开说清，
        // 否则用户无法区分「格式不支持」与「拖了个文件夹」。
        //
        // 只拖入**一个**不支持的文件时走单句提示（与文件树双击、快速打开
        // 同一句，见 [`Self::unsupported_format_notice`]）：套上
        // 「已打开 0 个文件；1 个…已忽略」的汇总句式反而像模板消息，
        // 用户看到的是「发生了点事」而不是「这个文件打不开」。
        if open.is_empty() && dir_count == 0 && reject.len() == 1 {
            self.set_status_notice(Self::unsupported_format_notice(&reject[0]));
            return;
        }
        let mut skipped: Vec<String> = Vec::new();
        if !reject.is_empty() {
            skipped.push(format!("{} 个暂不支持打开的格式", reject.len()));
        }
        if dir_count > 0 {
            skipped.push(format!("{dir_count} 个目录"));
        }
        if !skipped.is_empty() {
            self.set_status_notice(format!(
                "已打开 {} 个文件；{} 已忽略",
                open.len(),
                skipped.join("、")
            ));
        }
    }
}
