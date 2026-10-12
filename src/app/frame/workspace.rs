//! 工作区：文件树扫描 / 条目操作 / 打开文件 / 全局搜索分组。
use super::*;

/// 工作区文件树的**已装载条目**。
///
/// # 为什么不每帧调 `scan_tree`
///
/// 扫描是**磁盘 IO**（`read_dir` 递归 + 最多 5000 条）。每帧做一次，
/// 哪怕窗口最小化也在后台狂读磁盘，文件树还会因为「顺序随文件系统返回序变化」
/// 而不停抖动。故只在打开文件夹 / 文件操作之后显式扫一次，视图每帧只读这份快照。
///
/// 它是**投影**不是缓存的业务真值：磁盘一变就重扫，
/// 快照里没有任何领域对象（只有路径字符串与一个布尔）。
#[derive(Clone, Debug, Default)]
pub struct WorkspaceEntries {
    /// 根目录路径（空串 = 未打开工作区）。
    pub root: Option<String>,
    /// 扁平条目 `(路径, 是否目录)`，已按「目录在前、同层按名称」排序。
    pub items: Vec<(String, bool)>,
}

impl WorkspaceEntries {
    /// 以根目录与条目构造。
    pub fn new(root: String, items: Vec<(String, bool)>) -> Self {
        Self {
            root: Some(root),
            items,
        }
    }

    /// 是否已装载（未打开工作区时为假）。
    pub fn is_loaded(&self) -> bool {
        self.root.is_some()
    }
}

/// 一次工作区搜索的结果（分组 + 是否被上限截断）。
///
/// 刻意**不**把「是否截断」藏进 `EditorApp` 的某个字段由搜索顺手改掉：
/// 那样「读结果」就有了隐藏副作用，而截断提示恰恰是**必须被看见**的信息
/// （用户看到 300 条会以为搜全了）。它和结果一起返回，界面不可能漏掉。
#[derive(Clone, Debug, Default)]
pub struct WorkspaceSearch {
    /// 按文件分组的命中。
    pub groups: Vec<SearchGroup>,
    /// 是否因超过总命中上限（300）而被截断。
    pub truncated: bool,
}

impl WorkspaceSearch {
    /// 命中总数（各组之和）。
    pub fn total(&self) -> usize {
        self.groups.iter().map(|g| g.hits.len()).sum()
    }

    /// 是否没有任何命中。
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

impl EditorApp {
    /// 把后台扫描的结果应用到 UI 状态（异步 PickFolder 路径使用）。
    ///
    /// 后台线程已完成领域层 `scan_tree`（RwLock 写入 + EventBus publish），
    /// 这里只构造 `WorkspaceEntries` 快照 + 展开顶层目录——纯 UI 状态变更。
    pub(crate) fn apply_scan_result(
        &mut self,
        root: &str,
        entries: Vec<crate::application::dto::FileEntryView>,
    ) {
        // 后台线程已通过 FilePath::new 验证过 root 合法性，
        // 主线程直接用字符串构造 WorkspaceEntries——scan_tree 成功返回的前提
        // 是 FilePath::new 成功（否则会走 Err 分支，本方法不会被调用）。
        self.workspace_entries = WorkspaceEntries::new(
            root.to_string(),
            entries
                .iter()
                .map(|e| (e.path.as_str().to_string(), e.is_directory))
                .collect(),
        );
        // 首次装入时把根目录下的目录全部展开。
        if self.sidebar.needs_initial_expand {
            self.sidebar.needs_initial_expand = false;
            self.expand_top_level();
        }
    }

    /// 轮询后台扫描 channel（每帧调一次）。
    ///
    /// - `Empty` → 后台线程还在扫，等下一帧；
    /// - `Disconnected` → 后台线程异常终止，清 pending + loading 并报错；
    /// - `Ok((root, Ok(entries)))` → 正常完成，应用结果 + 状态栏提示；
    /// - `Ok((root, Err(e)))` → 领域层扫描失败，清 pending + loading + 报错。
    pub(super) fn poll_scan_result(&mut self) {
        if !self.scan_loading {
            return;
        }
        let rx = match self.scan_pending.as_ref() {
            Some(r) => r,
            None => {
                self.scan_loading = false;
                return;
            }
        };
        match rx.try_recv() {
            Ok((root, Ok(entries))) => {
                let count = entries.len();
                self.apply_scan_result(&root, entries);
                self.scan_loading = false;
                self.scan_pending = None;
                let msg = format!("已打开工作区：{root}（{count} 项）");
                self.set_status_notice(msg);
            }
            Ok((root, Err(e))) => {
                self.scan_loading = false;
                self.scan_pending = None;
                let msg = format!("无法打开工作区 {root}：{e}");
                self.set_status_error(&msg);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // 后台线程还在扫描，等下一帧
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // 后台线程崩溃了
                self.scan_loading = false;
                self.scan_pending = None;
                self.set_status_error("工作区扫描异常终止");
            }
        }
    }

    /// 当前文档所在目录（导出默认输出目录）。
    ///
    /// 未保存过的新文档没有路径，此时返回 `None`——**不**回退到进程工作目录：
    /// GUI 程序的 CWD 取决于启动方式，用户完全无法预测，
    /// 把文件写到一个他看不见的地方比「要求先保存」糟糕得多。
    pub fn active_document_dir(&self) -> Option<String> {
        let doc = self.active_doc?;
        let view = self
            .services
            .documents
            .get_document(GetDocumentQuery { id: doc })
            .ok()?;
        let path = view.path?;
        path.parent().map(|parent| parent.as_str().to_string())
    }

    /// 当前文档标题去掉 `.md` 后缀（导出 / 另存为的建议文件名用）。
    ///
    /// 刻意**不**用路径的文件名：`/a/b/notes.md` 与 `/a/b/notes.txt`
    /// 去扩展名后同名，两个都导出到同一目录时会互相覆盖。
    /// 故取路径末段再判断扩展名。
    ///
    /// # 另存为的建议名要**带扩展名**
    ///
    /// 此函数服务于两处：导出（补 `.html` 等后缀）与另存为
    /// （交给系统对话框）。后者的建议名若不带 `.md`，
    /// 用户点确认就得到一个无扩展名的文件——双击打不开，
    /// 别的工具也不认。系统对话框**不会**替我们补后缀。
    /// 故另存为走 [`Self::suggested_save_name`]，而不是本函数。
    pub(super) fn active_document_title_stem(&self) -> Option<String> {
        let doc = self.active_doc?;
        let title = self
            .services
            .documents
            .get_document(GetDocumentQuery { id: doc })
            .ok()?;
        let name = title
            .path
            .as_ref()
            .map_or_else(|| title.title.as_str(), |path| path.file_name())
            .to_string();
        let stem = match name.rfind('.') {
            Some(index) if index > 0 => name[..index].to_string(),
            _ => name,
        };
        let stem = if stem.trim().is_empty() {
            "document".to_string()
        } else {
            stem
        };
        Some(stem)
    }

    /// 另存为对话框的**建议文件名**（带扩展名）。
    ///
    /// 未命名文档给 `未命名.md`：系统对话框不会替我们补后缀，
    /// 建议名不带 `.md` 时用户点确认就得到一个双击打不开的文件。
    /// 「未命名」是领域给的标题（`Document` 的默认 title），
    /// 它本身不是合法文件名之一，但加上扩展名即可用。
    pub fn suggested_save_name(&self) -> String {
        match self.active_document_title_stem() {
            Some(stem) if stem.contains('.') => stem,
            Some(stem) => format!("{stem}.md"),
            // 一个文档都没有时：给一个像样的默认值而不是空串，
            // 空串会让对话框停在「未命名」并要求用户自己敲名字。
            None => "未命名.md".to_string(),
        }
    }

    // ————————————————————————————————————————————————————————————
    // 侧栏（S6-11：文件树 / 大纲 / 全局搜索）
    // ————————————————————————————————————————————————————————————
    //
    // 编排放这里而不留在视图，理由与 `export_current_document` 同源：
    // 这些是**验收标准里的行为**（「大纲点击跳转」「搜索结果跳转」
    // 「右键菜单四项可用」），必须能被集成测试断言，而视图的私有函数够不到。

    /// 工作区根目录（未打开文件夹时为空串）。
    pub fn workspace_root(&self) -> String {
        self.workspace_entries.root.clone().unwrap_or_default()
    }

    /// 扫描工作区并把条目装进 UI 状态（返回条目数）。
    ///
    /// 扫描是**磁盘 IO**，绝不能每帧做——故只在「打开文件夹 / 刷新 /
    /// 文件操作之后」显式调用，视图每帧只读已装好的 [`Self::workspace_entries`]。
    pub fn scan_workspace(&mut self, root: &str) -> AppResult<usize> {
        let root_path = FilePath::new(root).map_err(|e| AppError::domain(e.to_string()))?;
        let entries = self.services.workspace.scan_tree(ScanTreeCommand {
            root: root_path.clone(),
            max_depth: TREE_MAX_DEPTH,
        })?;
        self.workspace_entries = WorkspaceEntries::new(
            root_path.as_str().to_string(),
            entries
                .iter()
                .map(|e| (e.path.as_str().to_string(), e.is_directory))
                .collect(),
        );
        // 首次装入时把根目录下的目录全部展开：折叠的全展开会让用户
        // 以为「文件树是空的」，而实际只是没展开。
        if self.sidebar.needs_initial_expand {
            self.sidebar.needs_initial_expand = false;
            self.expand_top_level();
        }
        Ok(self.workspace_entries.items.len())
    }

    /// 展开根目录下的全部目录（仅第一层，深层留给用户自己点）。
    pub(super) fn expand_top_level(&mut self) {
        let root = self.workspace_root();
        let top: Vec<String> = self
            .workspace_entries
            .items
            .iter()
            .filter(|(path, is_dir)| *is_dir && parent_of(path) == root)
            .map(|(path, _)| path.clone())
            .collect();
        for path in top {
            self.sidebar.expand(&path);
        }
    }

    /// 打开工作区里的某个文件（文件树点击 / 搜索跳转都走这里）。
    pub fn open_workspace_file(&mut self, path: &str) -> AppResult<()> {
        let file_path = FilePath::new(path).map_err(|e| AppError::domain(e.to_string()))?;
        let id = self.services.documents.open(OpenFileCommand {
            path: file_path.clone(),
        })?;
        self.active_doc = Some(id);
        self.find.on_document_switched(id);
        self.editor.cursor = 0;
        self.editor.anchor = None;
        self.ui.sync.reset();
        self.refresh_text();
        self.sidebar.selected_path = Some(path.to_string());
        // 展开文件所在的全部祖先目录，否则文件树里看不到刚打开的文件。
        self.expand_ancestors(path);
        // 记录到最近文件：这条数据同时喂给欢迎页与导出默认目录。
        let _ = self
            .services
            .workspace
            .touch_recent(TouchRecentCommand { path: file_path });
        Ok(())
    }

    /// 展开某路径的所有祖先目录。
    pub(super) fn expand_ancestors(&mut self, path: &str) {
        let mut current = path.to_string();
        while let Some(cut) = current.rfind('/') {
            current.truncate(cut);
            if current.is_empty() || current == self.workspace_root() {
                break;
            }
            self.sidebar.expand(&current);
        }
    }

    /// 新建文件或目录（右键菜单「新建」提交时调用）。
    pub fn create_entry(&mut self, parent: &str, name: &str, is_directory: bool) -> AppResult<()> {
        let base = if parent.is_empty() {
            FilePath::new(self.workspace_root())
                .map_err(|e: crate::shared::path::PathError| AppError::domain(e.to_string()))?
        } else {
            FilePath::new(parent).map_err(|e| AppError::domain(e.to_string()))?
        };
        let path = base
            .join(name)
            .map_err(|e| AppError::domain(e.to_string()))?;
        self.services
            .workspace
            .create_entry(CreateEntryCommand { path, is_directory })?;
        // 新建文件默认建在**根**或指定目录下；若建在某个未展开的目录里，
        // 用户会看不到刚建出来的东西。
        if !parent.is_empty() {
            self.sidebar.expand(parent);
        }
        // 条目已变，重扫一次让树与磁盘一致（顺序也按名称重排）。
        self.services.workspace.rescan()?;
        self.reload_workspace_entries()
    }

    /// 重命名条目。
    pub fn rename_entry(&mut self, path: &str, new_name: &str) -> AppResult<()> {
        let from = FilePath::new(path).map_err(|e| AppError::domain(e.to_string()))?;
        let parent = from
            .parent()
            .ok_or_else(|| AppError::domain("该条目没有父目录，无法重命名"))?;
        let to = parent
            .join(new_name)
            .map_err(|e| AppError::domain(e.to_string()))?;
        self.services
            .workspace
            .move_entry(MoveEntryCommand { from, to })?;
        self.reload_workspace_entries()
    }

    /// 删除条目（**不含二次确认**；带确认的入口是 [`Self::request_delete`]）。
    ///
    /// # V11-06 之前为什么刻意不做确认
    ///
    /// 删除不可逆，而那时还没有能用的模态确认框（`DialogPort::confirm` 是同步
    /// 阻塞的，不能在 `update()` 里调）。当时的替代保护是把删除限制成
    /// 「只能删空目录 / 单文件」——**步子小，出错也有限**。
    /// 现在确认框已异步化（见 [`Self::request_delete`]），限制随之放宽为
    /// 递归删除：确认文案里会明说目录里有多少项内容（见
    /// [`Self::delete_confirm_message`]），用户是在知情的前提下点的「是」。
    ///
    /// 本方法保留为公开：它是「答复为是」之后真正执行的那一步，
    /// 也是测试直接驱动删除的入口。
    pub fn delete_entry(&mut self, path: &str) -> AppResult<()> {
        let target = FilePath::new(path).map_err(|e| AppError::domain(e.to_string()))?;
        self.services
            .workspace
            .delete_entry(DeleteEntryCommand { path: target })?;
        if self.sidebar.selected_path.as_deref() == Some(path) {
            self.sidebar.selected_path = None;
        }
        self.reload_workspace_entries()
    }

    /// 从磁盘重新装载文件树条目（操作变更了树之后调用）。
    pub(super) fn reload_workspace_entries(&mut self) -> AppResult<()> {
        let root = self.workspace_root();
        if root.is_empty() {
            return Ok(());
        }
        self.scan_workspace(&root)?;
        Ok(())
    }

    /// 「刷新」按钮：重新扫描工作区根目录。
    ///
    /// 树是**快照**（见 `WorkspaceEntries`），只在本应用改动文件后更新。
    /// 但文件也会在本应用之外变——用户用别的编辑器改了文件、git 拉了新
    /// 版本、构建产物被清理——此时树就与磁盘不一致，用户点文件名打开却
    /// 报「文件不存在」，看起来像应用坏了。
    ///
    /// 故需要一个公开入口。放在 `frame` 而非让调用方自己串
    /// `workspace.rescan()` + `scan_workspace()`：那两步必须成对出现，
    /// 漏掉第二步就会「重扫了但界面没变」，而漏掉第一步会导致聚合
    /// 里留着已删除的条目。
    ///
    /// 未打开工作区时是**无效果**（返回 0）而不是报错：用户还没选目录
    /// 就点刷新，不该弹一个无法理解的错误。
    pub fn refresh_workspace(&mut self) -> usize {
        let root = self.workspace_root();
        if root.is_empty() {
            return 0;
        }
        // 聚合与界面快照必须都更新：先重扫领域聚合（它可能还持有
        // 磁盘上已不存在的条目），再重扫 UI 快照。
        if self.services.workspace.rescan().is_err() {
            return 0;
        }
        self.scan_workspace(&root).unwrap_or(0)
    }
}

/// 父目录路径（末段之前）；无父目录时返回空串。
pub(super) fn parent_of(path: &str) -> String {
    path.rfind('/')
        .map(|cut| path[..cut].to_string())
        .unwrap_or_default()
}

/// 把领域命中转成表现层的分组结构，并算出每条命中的高亮区间。
///
/// 高亮区间在**字符**坐标下算（理由同`scroll_sync` 那套换算）：
/// 片段要交给 egui 排版，按字节切中文会错位——「一」在UTF-8 里 3 字节，
/// 字节 7 落在字符 9 的中间，切出来的两半都不是完整字符。
///
/// 查询词与选项由调用方传入而不是从 `result` 里找：`SearchResultView`
/// 本身不携带查询（它是领域 DTO，不该知道 UI 传了什么）。
pub(crate) fn group_search_hits(
    result: &SearchResultView,
    query: &str,
    case_sensitive: bool,
) -> Vec<SearchGroup> {
    let rows: Vec<(String, usize, String)> = result
        .hits
        .iter()
        .map(|hit| (hit.path.as_str().to_string(), hit.line, hit.snippet.clone()))
        .collect();
    let mut groups = group_hits(&rows);
    for group in &mut groups {
        for hit in &mut group.hits {
            hit.highlight =
                locate_in_snippet(&hit.snippet, query, case_sensitive).unwrap_or((0, 0));
        }
    }
    groups
}
