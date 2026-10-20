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
    /// 这里只构造 `WorkspaceEntries` 快照——纯 UI 状态变更（**不**自动展开
    /// 任何目录，默认全收起见 [`Self::scan_workspace`]）。
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
                let msg = format!("已打开工作区：{}（{count} 项）", Self::display_name(&root));
                self.set_status_notice(msg);
            }
            Ok((root, Err(e))) => {
                self.scan_loading = false;
                self.scan_pending = None;
                let msg = format!("无法打开工作区 {}：{e}", Self::display_name(&root));
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
    /// 建议名不带扩展名时用户点确认就得到一个双击打不开的文件。
    /// 「未命名」是领域给的标题（`Document` 的默认 title），
    /// 它本身不是合法文件名之一，但加上扩展名即可用。
    ///
    /// 扩展名取 [`text_format::default_extension`]（白名单首选的唯一出处），
    /// 不在此处硬编码 `.md`——白名单的首选变了这里必须跟着变。
    pub fn suggested_save_name(&self) -> String {
        let ext = text_format::default_extension();
        match self.active_document_title_stem() {
            Some(stem) if stem.contains('.') => stem,
            Some(stem) => format!("{stem}.{ext}"),
            // 一个文档都没有时：给一个像样的默认值而不是空串，
            // 空串会让对话框停在「未命名」并要求用户自己敲名字。
            None => format!("未命名.{ext}"),
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
        // 树**默认全收起**（问题 062）：装载后只画根目录的直接子项，
        // 目录一律不自动展开。
        //
        // 此前会展开顶层目录，理由是「怕用户以为树是空的」——那个担心不成立：
        // `tree_rows` 里根的直接子项（depth 0）恒可见，收起的目录自己也仍然
        // 画出来，只是不画它的子项。反过来，自动展开等于用户打开一个文件夹
        // 就先被塞进一层他没要求看的内容，目录一大就是一屏噪音；而用户真正
        // 要找的文件所在的目录，在打开文件时由 `expand_ancestors` 逐层展开
        // 到可见——那才是「按需展开」。
        Ok(self.workspace_entries.items.len())
    }

    /// 打开文件夹成功后，把界面切到**工作区视图**（问题 064）。
    ///
    /// # 修的是什么
    ///
    /// 从欢迎页「打开文件夹」后，旧行为是停在欢迎页原地：`should_show_welcome`
    /// 只看 `active_doc`，而打开文件夹不建文档，于是页面不退、侧栏默认
    /// 收起（062）、文件树画了也没人看见——用户以为点了没反应。
    ///
    /// # 三件事各自的理由
    ///
    /// 1. **切到「文件」面板**：打开文件夹的意图就是看文件树；若上次停在
    ///    搜索 / 大纲面板，用户得再点一次才看到刚打开的文件夹。
    /// 2. **清空展开项**：`expanded_dirs` 是**上一个**工作区的路径，换根后
    ///    全部失效。不清会让新根下「莫名其妙」处于展开态的目录出现
    ///    （路径字符串恰好命中的话），且每次换工作区都累积一批死路径。
    /// 3. **展开侧栏——仅当用户没决定过**：`sidebar_visible_decision()`
    ///    为 `Some` 说明用户手动开/关过（或系统已替他落过盘），尊重该
    ///    状态不强制展开；为 `None`（首次 / 旧配置）才自动展开。这与
    ///    062 的「默认收起」不矛盾：前者管「没打开文件夹时」，后者管
    ///    「刚打开文件夹时」——后者正是用户明确要看文件树的时刻。
    ///
    /// 「根节点 + 一层」由树投影天然保证：`tree_rows` 里根的直接子项
    /// （depth 0）恒可见，不需要额外展开动作（062 删掉自动展开时确认过）。
    pub(crate) fn reveal_workspace_sidebar(&mut self) {
        self.ui.sidebar_tab = SidebarTab::Files;
        self.sidebar.collapse_all();
        if self.settings().editor.sidebar_visible.is_none() {
            self.ui.sidebar_visible = true;
        }
        // 落盘不在本方法里做：帧末 `persist_sidebar_visibility` 统一比对
        // （能改开合的入口四条以上，逐个写必漏——062 的结论）。这里改了
        // `ui.sidebar_visible`，下一帧就会被它捡走。
    }

    /// 打开工作区里的某个文件（文件树点击 / 搜索跳转都走这里）。
    ///
    /// # 可打开的格式白名单（问题 045）
    ///
    /// 只有 [`crate::shared::text_format::TextFormats`] 放行的扩展名能被打开；
    /// 内置清单见 [`crate::shared::text_format::BUILTIN_TEXT_EXTENSIONS`]
    /// （大小写不敏感）。图片与二进制一律拒绝并提示。守卫放在
    /// 本方法开头，故**五条入口**（文件树 / 搜索跳转 / 最近文件 /
    /// 快速打开 / 拖放）一次性全覆盖，各调用方不必各自判一遍。
    ///
    /// # 同一路径只保留一个标签（问题 037）
    ///
    /// 旧实现每次调用都 `documents.open` 建一份新聚合——同一个文件
    /// 点十次就是十个标签，标题栏一字排开完全认不出谁是谁。现在先查
    /// 已打开列表：**路径已在某个标签里就直接切过去**（激活既有页），
    /// 不再新建。这条校验放在这里而不是各调用方：文件树、搜索跳转、
    /// 最近文件、快速打开、拖放五条入口都汇聚到本方法，只此一处
    /// 即可覆盖全部打开路径（与「失败自己报状态栏」同款的按构造保证）。
    ///
    /// # 打开文档数量上限（问题 038）
    ///
    /// 达上限后拒绝**新的**打开并提示，已打开的文档不受影响。限额检查
    /// 刻意放在去重**之后**：已打开的文档走的是「切换」，不占新名额——
    /// 放在前面会挡住「切回那个已经开着的文档」这种最常见的操作。
    pub fn open_workspace_file(&mut self, path: &str) -> AppResult<()> {
        // # 格式白名单守卫（问题 045）
        //
        // 放在**最前**——先于去重与限额。理由有二：
        // 1. 这是**内容安全**判定，不是资源策略：图片 / 二进制一旦进了
        //    编辑缓冲，屏幕上就是一屏乱码，用户还可能顺手 `Ctrl+S` 把它
        //    写回去。任何「先让它进来、之后再拦」的顺序都留了这个窗口。
        // 2. 白名单收窄后，上一版放行的文件可能**已经开着标签**；
        //    「切回那个标签」若不拦，就成了绕过守卫的后门。
        //
        // 提示语用 `notice`（中性）而非 `error`（红色）：用户点了一个
        // 图片文件不是异常，是「这个格式我们不支持」——红色错误框会让人
        // 以为程序坏了。文案与文件树置灰行的悬停说明**同源**
        // （[`Self::unsupported_format_notice`]）。
        if !self.can_open_as_text(path) {
            let notice = Self::unsupported_format_notice(path);
            self.set_status_notice(notice);
            return Ok(());
        }
        // 去重前先把请求路径归一化：库里存的是 `FilePath` 归一化形式
        // （统一 `/` 分隔），而调用方给的可能是 OS 原生形态——Windows 的
        // 拖放 / 原生对话框给的是 `\`。直接比字符串的话，同一文件从
        // 「文件树」与「拖放」两条入口打开会被判成两个文档，重复标签
        // （037 的去重被分隔符差异绕过；038 的限额测试以反斜杠路径
        // 复现：去重不命中 → 落到限额拒绝 → 「切回已打开文档」被挡）。
        let wanted = FilePath::new(path).ok();
        let already_open = self.open_document_list().into_iter().find(|doc| {
            doc.path.as_ref().is_some_and(|p| {
                let stored = p.as_str();
                stored.eq_ignore_ascii_case(path)
                    || wanted.as_ref().is_some_and(|w| w.as_str() == stored)
            })
        });
        if let Some(doc) = already_open {
            // 切到既有标签：光标 / 滚动 / 查找状态一并复位（与手动切标签一致）。
            self.switch_document(doc.id);
            self.sidebar.selected_path = Some(path.to_string());
            // 展开文件所在的全部祖先目录，否则文件树里看不到刚打开的文件。
            // 用归一化路径：`expand_ancestors` 按 `/` 切段，OS 原生
            // 反斜杠路径下它一段都切不出来。
            if let Some(fp) = &wanted {
                self.expand_ancestors(fp.as_str());
            }
            // 记录到最近文件：这条数据同时喂给欢迎页与导出默认目录。
            // 即使文件早已打开，「再次打开」也该刷新它的最近次序。
            if let Ok(file_path) = FilePath::new(path) {
                let _ = self
                    .services
                    .workspace
                    .touch_recent(TouchRecentCommand { path: file_path });
            }
            return Ok(());
        }
        // 数量限额：满了就地拒绝（提示而非错误——这是策略拒绝，不是异常）。
        // 提示语带上限值：用户看到「12」才知道要去设置里调，否则只会困惑。
        let limit = self.settings().editor.max_open_documents;
        if self.open_document_list().len() >= limit as usize {
            self.set_status_notice(format!(
                "已达打开文档上限（{limit} 个），可先关闭部分标签或在设置里调整上限"
            ));
            return Ok(());
        }
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
        self.expand_ancestors(file_path.as_str());
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
