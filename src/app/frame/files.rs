//! 文件与文档操作：保存 / 新建 / 状态栏提示 / 路径读取 / 自动保存 / 拖放打开。
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

    /// 在状态栏显示一条错误。
    ///
    /// 时钟取自 `services.markdown`（所有应用服务共用同一个 `Clock`），
    /// 不直接调 `SystemClock`：消息的「4 秒后消失」是可测规则，
    /// 用系统时钟就没法写测试验证它。
    pub fn set_status_error(&mut self, text: &str) {
        let now = self.services.markdown.now().millis();
        self.status_bar.error(text, now);
    }

    /// 在状态栏显示一条普通提示。
    pub fn set_status_notice(&mut self, text: impl Into<String>) {
        let now = self.services.markdown.now().millis();
        self.status_bar.notice(text, now);
    }

    /// 当前文档的面包屑路径；无活动文档或无路径时返回「未命名文档」。
    ///
    /// 走轻量摘要而非 `get_document`：面包屑每帧都取，后者要付一次
    /// 全文快照只为拿一个路径（问题 043）。
    pub fn current_document_path(&self) -> String {
        let Some(id) = self.active_doc else {
            return "未命名文档".to_string();
        };
        match self
            .services
            .documents
            .get_tab_summary(GetDocumentQuery { id })
        {
            Ok(view) => view
                .path
                .map(|p| p.as_str().to_string())
                .unwrap_or_else(|| "未命名文档".to_string()),
            Err(_) => "未命名文档".to_string(),
        }
    }

    // ================================================================
    // S6-11f：浮层编排
    // ================================================================

    /// 当前生效主题的显示名（「关于」面板用）。
    pub fn theme_kind_label(&self) -> &'static str {
        match self.ui.theme.kind {
            BuiltinTheme::Dark => "深色",
            BuiltinTheme::Light => "浅色",
            BuiltinTheme::System => "跟随系统",
        }
    }

    /// 在光标处插入一段文本（符号面板 / 工具栏插入结构共用）。
    ///
    /// 走领域 `insert_text` 而非直接改 `editor_text`，理由与
    /// S6-07 的「三入口一致」同源：直接改 UI 文本会绕过一次撤销点，
    /// 让用户「插入一个 emoji 之后按 Ctrl+Z 什么也没撤销」。
    ///
    /// # 插入点必须取 **UI 光标**而非领域光标
    ///
    /// 两者是不同的坐标系：领域记的是**上一次领域命令后**的光标，
    /// UI 记的是 egui `TextEdit` 当前画出来的那个。而大纲跳转
    /// （[`Self::goto_byte_offset`]）、查找命中（`reveal_match`）
    /// 都只改 UI 光标——用户看到光标跳到了第 40 行，
    /// 领域那边还停在第 3 行。此时按「插入符号」若读领域光标，
    /// 内容会插到用户**看不见**的位置。
    /// （`move_line` / `run_format` 同样取 UI 光标，规则一致。）
    ///
    /// 光标停在插入内容**之后**：停在内容里意味着用户接着打字
    /// 会把文字插进代码块标记中间——这是插入类操作最常见的投诉。
    ///
    /// 有选区时**替换选区**：用户选中一段文字再插链接，
    /// 得到「选中文字链接选中文字」是几乎所有编辑器的行为，
    /// 也符合「插入」这个词的含义。
    pub fn insert_at_cursor(&mut self, text: &str) -> AppResult<()> {
        let Some(id) = self.ensure_document() else {
            return Ok(());
        };
        // 选区优先：无选区时 `selection()` 返回 `None`，退回光标。
        // 选区优先：无选区时 `selection()` 返回 `None`，退回光标。
        let (start, end) = self
            .editor
            .selection()
            .unwrap_or((self.editor.cursor, self.editor.cursor));
        let end = end.max(start);
        if start != end {
            self.services.documents.replace_range(ReplaceRangeCommand {
                id,
                range: TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
                text: text.to_string(),
            })?;
        } else {
            self.services.documents.insert_text(InsertTextCommand {
                id,
                offset: start,
                text: text.to_string(),
            })?;
        }
        self.refresh_text();
        let after = start + text.len();
        // 领域光标也要跟上：否则下一次插入仍会用旧位置（见上）。
        self.editor.set_selection(after, after);
        self.pending_cursor = Some((
            editor_ops::byte_to_char(&self.editor_text, after),
            editor_ops::byte_to_char(&self.editor_text, after),
        ));
        self.clear_undoer_next = true;
        Ok(())
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
    /// # 为什么必须 `request_repaint_after`（问题 068 根因）
    ///
    /// egui 是**事件驱动重绘**：没有输入事件就不产生新帧。本方法挂在帧循环里，
    /// 用户停止输入后它就不再被调用——间隔到期也无人执行，自动保存表现为
    /// 「完全不生效」（只有碰巧动一下鼠标才顺带补写）。因此只要还有文档在
    /// 计时，就向 egui 预约「剩余时长后的一帧」，到期时刻必定有帧跑到这里。
    pub(super) fn run_autosave(&mut self, ctx: &egui::Context) {
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
        // 为「下一次到期」安排一帧（问题 068）：`due` 已取走到期文档，
        // 剩余文档按各自的记录时刻算出最近的到期点。没有在计时的文档时
        // 返回 `None`，不安排（下一次击键的帧自然会重新调度）。
        if let Some(remaining_ms) = self
            .services
            .auto_save_tracker
            .next_due_in(now, interval_ms)
        {
            // 0ms 也至少等一帧的节奏，避免同一帧内反复请求立刻重绘。
            ctx.request_repaint_after(std::time::Duration::from_millis(remaining_ms.max(1)));
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

    /// 该路径能否**作为文本**打开（问题 045）。
    ///
    /// 文件树置灰、双击拦截、拖放分类、打开守卫共用这一条判据；
    /// 实现体只查 [`TextFormats`]，不碰磁盘（扩展名判定零 IO，
    /// 可以安全地放进每帧的渲染循环）。
    pub fn can_open_as_text(&self, path: &str) -> bool {
        self.text_formats.accepts(path)
    }

    /// 非白名单格式的拒绝提示文案（唯一出处）。
    ///
    /// 关联函数而非 `&self` 方法：表现层要在**置灰的行**上直接用同一句话
    /// 做悬停说明，那时手边不一定有可变借用。
    pub fn unsupported_format_notice(path: &str) -> String {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        format!("暂不支持打开该格式：{name}")
    }

    /// 把「该格式暂不支持」报到状态栏。
    ///
    /// 文件树双击非白名单文件时调用——那条路径**不**走打开管线
    /// （入口已禁用），但仍要给用户一个原因，否则表现得像「点了没反应」。
    pub fn report_unsupported_format(&mut self, path: &str) {
        self.set_status_notice(Self::unsupported_format_notice(path));
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

    /// 打开一个文件（用于最近文件、文件对话框）。
    ///
    /// 失败时**自己**把原因报到状态栏，不只把 `Err` 往上抛：
    /// 欢迎页点一个已被删除的最近文件时，调用方若忘了展示错误，
    /// 用户看到的就是「点了没反应」——比任何错误提示都更让人困惑。
    /// 报错的职责放在这里而不是各调用方，是为了**按构造**保证可见。
    pub fn open_path(&mut self, path: &str) -> AppResult<()> {
        let result = self.open_workspace_file(path);
        if let Err(e) = &result {
            let name = path.rsplit('/').next().unwrap_or(path);
            self.set_status_error(&format!("无法打开 {name}：{e}"));
        }
        result
    }

    /// 文件拖放打开（P2）。
    ///
    /// egui/winit 后端在「拖放发生的那一帧」把文件填进 `ctx.input().raw.dropped_files`，
    /// 下一帧即清空，因此每帧读一次即可，无需手动消费或去重。
    ///
    /// 行为（推荐方案）：
    /// - 按扩展名分类（`classify_dropped`，判据是**全应用唯一**的格式白名单）——
    ///   白名单内（内置：`md` / `markdown` / `mdown` / `mkd` / `txt` / `text` / `csv`）
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
