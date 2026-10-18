//! 多标签管理：打开列表、切换、关闭、循环切换、按编号跳转、分割比例落盘。
use super::*;

impl EditorApp {
    /// 当前打开的文档列表（按领域内部顺序）。
    ///
    /// 领域按 `DocumentId` 各存各的内容；UI 的「标签」只是这些文档的视图。
    ///
    /// 返回**轻量摘要**而非完整 `DocumentView`：后者要为每个文档做一次
    /// rope 全文快照（填预览与行数），而调用方只用标题 / 脏标记 / 路径 / ID。
    /// 标签条每帧都取这份列表，多文档时那就是每帧 N 次 O(全文) 克隆
    /// ——问题 043「打开多个文档后明显卡顿」的主因，故在此收口。
    pub fn open_document_list(&self) -> Vec<TabSummary> {
        self.services
            .documents
            .list_open_tabs(ListOpenDocumentsQuery)
            .unwrap_or_default()
    }

    /// 切换到指定文档（多标签）。
    ///
    /// 领域按 `DocumentId` 各自保存内容，`active_doc` 只是 UI 视角的「当前页」；
    /// 切换即换视图，不涉及任何内容搬运——双写不一致在此被结构性排除。
    pub(crate) fn switch_document(&mut self, id: DocumentId) {
        if self.active_doc == Some(id) {
            return;
        }
        self.active_doc = Some(id);
        // 查找状态跨文档策略在此生效（S6-08）：保留查询与选项，
        // 但清空属于旧文档的命中。不调的话「下一个」会按旧文档的
        // 字节偏移跳到新文档的无关位置，甚至越界。
        self.find.on_document_switched(id);
        // 每文档独立的光标 / 选区尚未在 UI State 中建模（S6-12 范畴），
        // 切换时回到文档开头，避免停在另一个文档的偏移上。
        self.editor.cursor = 0;
        self.editor.anchor = None;
        // 两侧滚动偏移都归零，若不重置同步状态，这一帧会被判成「某一侧
        // 发生了巨大位移」并把对侧也拖走——表现为「切标签后预览自己滚了一下」。
        self.ui.sync.reset();
        self.refresh_text();
        // 面板若开着，重扫一次让计数与新文档一致。
        if self.dialog.is_open(Overlay::Find) {
            self.sync_find();
        }
    }

    /// 关闭指定标签页（多标签 × 按钮用）。
    ///
    /// 仅当存在多个打开文档时才真正关闭；最后一份文档关闭后会由
    /// `ensure_document` 在下一帧补建一份空白文档（保证「永远至少有一页可编辑」）。
    pub(crate) fn close_document(&mut self, id: DocumentId) {
        if self.open_document_list().len() <= 1 {
            return;
        }
        let _ = self
            .services
            .documents
            .close(CloseCommand { id, force: false });
        // 仅当文档确实被关闭（不再是「需要保存」而保持打开）才清草稿，
        // 否则脏文档仍在编辑中，草稿仍是它的崩溃保险，不该清掉。
        if self
            .services
            .documents
            .get_document(GetDocumentQuery { id })
            .is_err()
        {
            let _ = self
                .services
                .documents
                .clear_draft(&self.services.drafts_dir, id);
        }
        // 关闭后重新取列表，挑一个相邻文档作为新的活动页（若关掉的是当前页）。
        let remaining = self.open_document_list();
        if remaining.is_empty() {
            self.active_doc = None;
        } else if self.active_doc == Some(id) {
            let idx = remaining.iter().position(|d| d.id == id).unwrap_or(0);
            let next = remaining
                .get(idx.saturating_sub(1))
                .or_else(|| remaining.first());
            if let Some(doc) = next {
                self.active_doc = Some(doc.id);
            }
        }
        self.editor.cursor = 0;
        self.editor.anchor = None;
        self.ui.sync.reset();
        self.refresh_text();
    }

    /// 关闭当前标签页（快捷键 `CloseTab`）。
    pub(crate) fn close_active_tab(&mut self) {
        if let Some(id) = self.active_doc {
            self.close_document(id);
        }
    }

    /// 关掉**全部**标签，回到欢迎页。
    ///
    /// [`Self::close_document`] 刻意**不**关最后一个标签
    /// （「永远至少有一页可编辑」是硬约束），但欢迎页需要
    /// 「一个标签都不剩」这个状态。二者不冲突：这里走
    /// `force = true` 逐个关掉，不经过那个约束。
    ///
    /// 未命名文档被丢弃是**无损**的（内容为空），
    /// 已保存过的文档则已在磁盘上；未保存的修改会丢失，
    /// 故只在欢迎页的「关闭全部」这类明确动作上调用。
    pub fn close_all_documents(&mut self) {
        for view in self.open_document_list() {
            let _ = self.services.documents.close(CloseCommand {
                id: view.id,
                force: true,
            });
            // 文档已关闭：清掉其草稿，避免下次启动误报「待恢复」。
            let _ = self
                .services
                .documents
                .clear_draft(&self.services.drafts_dir, view.id);
        }
        self.active_doc = None;
        self.editor_text.clear();
        self.editor.reset_for_new_document();
        self.ui.sync.reset();
        self.refresh_text();
    }

    /// 在打开的文档间循环切换（`NextTab` / `PrevTab`）。
    pub(crate) fn cycle_document(&mut self, delta: isize) {
        let open = self.open_document_list();
        if open.len() <= 1 {
            return;
        }
        let current = self
            .active_doc
            .and_then(|id| open.iter().position(|d| d.id == id));
        let len = open.len() as isize;
        let cur = current.unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len);
        self.switch_document(open[next as usize].id);
    }

    /// 跳转到第 `n` 个标签页（`Alt+1` ~ `Alt+9`，1-based）。
    pub(crate) fn goto_document(&mut self, n: usize) {
        let open = self.open_document_list();
        if n == 0 || n > open.len() {
            return;
        }
        self.switch_document(open[n - 1].id);
    }

    /// 把当前分割比例落盘到设置（拖拽结束时调用，避免每帧写文件）。
    ///
    /// 走 `preferences.set_editor`：它会校验并 `repository.save`。
    /// 读回当前编辑器设置、只改 `split_ratio`、其余原样回写，
    /// 保证不丢用户已设的缩进 / 自动换行 / 自动保存等。
    pub(crate) fn persist_split_ratio(&mut self) {
        let Ok(settings) = self.services.preferences.get_settings(GetSettingsQuery) else {
            return;
        };
        let mut editor = settings.editor;
        editor.split_ratio = self.ui.split_ratio;
        let _ = self
            .services
            .preferences
            .set_editor(SetEditorCommand { editor });
    }

    /// 侧栏开合**变化时**落盘（每帧调用一次，内部比对，没变就直接返回）。
    ///
    /// # 为什么是「每帧比对」而不是「在切换处写」
    ///
    /// 能改 `sidebar_visible` 的入口有四条以上：`UiAction::ToggleSidebar`
    /// （右上角按钮 / Ctrl+\ / 视图菜单）、`ToggleOutline` 与 `FindInFiles`
    /// （`toggle_sidebar_tab` 会顺带展开或收起）、以及侧栏活动栏的点击
    /// （`sidebar_view` 里直接调 `AppUiState::toggle_sidebar_tab`，不经
    /// `execute`）。逐个入口写一遍就是「漏一个静默失效」的经典形状——
    /// 项目里 `UiAction` 漏接线的代价已经在 038 付过一次。故改为在帧末
    /// 统一比对：任何入口改了它，这一帧都会被落盘。
    ///
    /// 与 `persist_split_ratio` 同一手法（读回设置 → 只改一个字段 → 回写），
    /// 保证不丢用户已设的缩进 / 自动换行 / 自动保存等。写盘失败刻意忽略：
    /// 开合状态是显示偏好，为它弹错会打断编辑。
    pub fn persist_sidebar_visibility(&mut self) {
        if self.ui.sidebar_visible == self.sidebar_visible_persisted {
            return;
        }
        let Ok(settings) = self.services.preferences.get_settings(GetSettingsQuery) else {
            return;
        };
        let mut editor = settings.editor;
        editor.sidebar_visible = self.ui.sidebar_visible;
        if self
            .services
            .preferences
            .set_editor(SetEditorCommand { editor })
            .is_err()
        {
            return;
        }
        // 只有真写成了才更新基准：写失败时下一帧会再试一次，
        // 否则这次失败会被永久记住（状态从此再也不落盘）。
        self.sidebar_visible_persisted = self.ui.sidebar_visible;
    }

    /// 取得活动文档；没有则新建一个空白未命名文档。
    ///
    /// # 谁在调用
    ///
    /// * `editor_view::show` **每帧**兜底调用 —— 一旦离开欢迎页，
    ///   编辑区必须永远有一页可编辑（否则用户看到的是一块点不动的灰板）；
    /// * `save_document` 在保存前调用，让「没打开任何文档时按 Ctrl+S」
    ///   也能建一份出来而不是静默失败。
    ///
    /// 启动时**不**调用：那一次是欢迎页，见 [`Self::should_show_welcome`]。
    pub fn ensure_document(&mut self) -> Option<DocumentId> {
        if self.active_doc.is_some() {
            return self.active_doc;
        }
        if let Ok(id) = self.services.documents.new_document(NewDocumentCommand) {
            self.active_doc = Some(id);
            // 登记查找状态的「当前文档」：不登记的话，首次跨文档时
            // `on_document_switched` 会把这一次当成「首次」而非「切换」，
            // 于是本该清空的旧命中被保留下来。
            self.find.on_document_switched(id);
            self.refresh_text();
        }
        self.active_doc
    }

    /// 当前是否应显示**欢迎页**（一个标签都没开）。
    ///
    /// 判据是「有没有活动文档」而不是「文档数是不是 1」：
    /// 启动时那份空白未命名文档**不算**「用户开了个文档」，
    /// 它只是为了让编辑区第一帧可编辑。所以启动时若也显示欢迎页，
    /// 底下那份空白文档就成了「看不见但存在」的多余标签。
    pub fn should_show_welcome(&self) -> bool {
        self.active_doc.is_none()
    }
}
