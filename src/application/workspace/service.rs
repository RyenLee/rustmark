//! Workspace 用例编排服务（阶段 04 · S4-07）。
//!
//! 编排文件树、最近文件、标签页与跨文件搜索。最近文件的去重与上限（20）由领域层
//! [`crate::domain::workspace::value_objects::RecentFiles`] 保证；跨文件搜索沿用原型的
//! 命中上限（单文件 50、总 300），并**优先使用未保存草稿的内存内容**，确保检索覆盖
//! 尚未落盘的修改。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::domain::workspace::aggregate::Workspace;
use crate::domain::workspace::errors::WorkspaceError;
use crate::domain::workspace::repository::WorkspaceRepository;
use crate::domain::workspace::value_objects::{CloseDecision, EntryKind, WorkspaceEntry};

use crate::shared::error::AppError;
use crate::shared::event::EventBus;
use crate::shared::ids::{DocumentId, WorkspaceId};
use crate::shared::path::FilePath;
use crate::shared::result::AppResult;
use crate::shared::time::Clock;

use crate::application::document::DocumentContentProvider;
use crate::application::dto::{
    CloseDecisionView, FileEntryView, RangeView, RecentFileView, SearchHitView, SearchResultView,
    TabView,
};

use super::commands::*;
use super::queries::*;

/// 单文件命中上限（与原型一致）。
pub const MAX_HITS_PER_FILE: usize = 50;
/// 总命中上限（与原型一致）。
pub const MAX_TOTAL_HITS: usize = 300;

/// 文件树扫描深度上限（S6-11）。
///
/// 取 8 层：普通工程目录深到 8 层已经极少，而无限深度会被
/// 误指向 `node_modules` 之类的大目录拖死 UI。文件系统实现另有
/// 5000 条的硬上限兜底，两者都不该由表现层决定。
pub const TREE_MAX_DEPTH: usize = 8;

/// 由布尔构造 [`EntryKind`]（命令用布尔表达，避免 DTO 依赖领域枚举）。
const fn entry_kind(is_directory: bool) -> EntryKind {
    if is_directory {
        EntryKind::Directory
    } else {
        EntryKind::File
    }
}

/// Workspace 用例服务。
pub struct WorkspaceAppService {
    repository: Arc<dyn WorkspaceRepository>,
    provider: Arc<dyn DocumentContentProvider>,
    bus: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    workspace: Arc<RwLock<Workspace>>,
    id: WorkspaceId,
}

impl WorkspaceAppService {
    /// 构造服务（依赖全部注入）。
    pub fn new(
        repository: Arc<dyn WorkspaceRepository>,
        provider: Arc<dyn DocumentContentProvider>,
        bus: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        workspace: Arc<RwLock<Workspace>>,
        id: WorkspaceId,
    ) -> Self {
        Self {
            repository,
            provider,
            bus,
            clock,
            workspace,
            id,
        }
    }

    /// 启动时从持久化恢复最近文件列表。
    pub fn restore_recent(&self) -> AppResult<()> {
        let items = self.repository.load_recent()?;
        self.lock_write()?.restore_recent(items);
        Ok(())
    }

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

    /// 记录一次文件打开（更新最近文件并持久化）。
    pub fn touch_recent(&self, cmd: TouchRecentCommand) -> AppResult<()> {
        let now = self.clock.now();
        let items = {
            let mut ws = self.lock_write()?;
            ws.touch_recent(cmd.path.clone(), now);
            ws.recent().list().to_vec()
        };
        self.repository.save_recent(&items)?;
        self.bus
            .publish(crate::shared::event::DomainEvent::WorkspaceChanged(self.id));
        Ok(())
    }

    /// 从最近文件中移除并持久化。
    pub fn forget_recent(&self, cmd: ForgetRecentCommand) -> AppResult<()> {
        let items = {
            let mut ws = self.lock_write()?;
            ws.forget_recent(&cmd.path);
            ws.recent().list().to_vec()
        };
        self.repository.save_recent(&items)?;
        self.bus
            .publish(crate::shared::event::DomainEvent::WorkspaceChanged(self.id));
        Ok(())
    }

    /// 列出最近文件（按最近使用降序）。
    pub fn list_recent(&self, _cmd: ListRecentQuery) -> AppResult<Vec<RecentFileView>> {
        let ws = self.lock_read()?;
        Ok(ws
            .recent()
            .list()
            .iter()
            .map(|item| RecentFileView {
                path: item.path().clone(),
                last_opened: item.last_opened().millis(),
            })
            .collect())
    }

    /// 打开（或激活）一个标签页。
    pub fn open_tab(&self, cmd: OpenTabCommand) -> AppResult<()> {
        self.lock_write()?.open_tab(cmd.doc_id);
        self.bus
            .publish(crate::shared::event::DomainEvent::WorkspaceChanged(self.id));
        Ok(())
    }

    /// 关闭标签页；脏文档返回 [`CloseDecisionView::NeedsSave`]，保持打开。
    pub fn close_tab(&self, cmd: CloseTabCommand) -> AppResult<CloseDecisionView> {
        let decision = self
            .lock_write()?
            .close_tab(cmd.doc_id, cmd.dirty)
            .map_err(|e: WorkspaceError| AppError::domain(e.to_string()))?;
        self.bus
            .publish(crate::shared::event::DomainEvent::WorkspaceChanged(self.id));
        Ok(match decision {
            CloseDecision::Closed => CloseDecisionView::Closed,
            CloseDecision::NeedsSave => CloseDecisionView::NeedsSave,
        })
    }

    /// 强制关闭标签页（不保存）。
    pub fn force_close_tab(&self, cmd: ForceCloseTabCommand) -> AppResult<()> {
        self.lock_write()?.force_close_tab(cmd.doc_id);
        self.bus
            .publish(crate::shared::event::DomainEvent::WorkspaceChanged(self.id));
        Ok(())
    }

    /// 列出全部标签页。
    pub fn list_tabs(&self, _cmd: ListTabsQuery) -> AppResult<Vec<TabView>> {
        let ws = self.lock_read()?;
        Ok(ws
            .tabs()
            .iter()
            .map(|tab| TabView {
                document_id: tab.document_id(),
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
            let _ = ws.add_entry(entry);
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
            let _ = ws.move_entry(&cmd.from, &cmd.to);
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

    /// 跨文件搜索：覆盖磁盘文件与未保存草稿，命中受上限保护。
    pub fn search(&self, cmd: SearchQuery) -> AppResult<SearchResultView> {
        let candidates = self.gather_candidates()?;
        let mut hits: Vec<SearchHitView> = Vec::with_capacity(MAX_TOTAL_HITS);
        let mut truncated = false;

        for (file_id, path, content) in candidates {
            if hits.len() >= MAX_TOTAL_HITS {
                truncated = true;
                break;
            }
            let raw = search_text(
                &content,
                &cmd.pattern,
                cmd.case_sensitive,
                cmd.whole_word,
                MAX_HITS_PER_FILE,
            );
            for hit in raw {
                if hits.len() >= MAX_TOTAL_HITS {
                    truncated = true;
                    break;
                }
                hits.push(SearchHitView {
                    file_id,
                    path: path.clone(),
                    line: hit.line,
                    col: hit.col,
                    snippet: hit.snippet,
                    match_range: RangeView::new(hit.start, hit.end),
                });
            }
        }

        Ok(SearchResultView { hits, truncated })
    }

    /// 读取单个工作区文件的内容（S6-11f：差异对比的右侧）。
    ///
    /// 刻意**不开文档**：开文档会多一个标签、切走 `active_doc`、
    /// 让预览区重排——而用户只想看一眼差异。为此走
    /// [`WorkspaceRepository::read_content`] 而不是 `documents.open`。
    ///
    /// 路径合法性由仓储判定（`FilePath` 无法构造非法路径，
    /// 但文件不存在 / 不可读仍会失败），这里原样把错误往上抛，
    /// 由调用方决定如何展示——静默返回空串会让「文件被删了」
    /// 与「文件是空的」变得无法区分。
    pub fn read_file(&self, cmd: ReadFileQuery) -> AppResult<String> {
        self.repository.read_content(&cmd.path)
    }

    // ---------- 内部 ----------

    /// 在聚合的**副本**上试跑一条规则；不通过则直接返回错误，真聚合不动。
    fn check_on_clone(
        &self,
        rule: impl FnOnce(&mut Workspace) -> Result<(), WorkspaceError>,
    ) -> AppResult<()> {
        let mut probe = self.lock_read()?.clone();
        rule(&mut probe).map_err(|e: WorkspaceError| AppError::domain(e.to_string()))
    }

    /// 移除一条失效的最近文件记录（失败静默）。
    ///
    /// 这只是清理动作：条目已经不在磁盘上，把它留在最近文件里只会让
    /// 用户点到一个打不开的路径。清理失败（配置目录只读）不值得打断操作。
    fn forget_recent_quietly(&self, path: &FilePath) {
        let _ = self.forget_recent(ForgetRecentCommand { path: path.clone() });
    }

    /// 发布工作区变更事件。
    fn publish_changed(&self) {
        self.bus
            .publish(crate::shared::event::DomainEvent::WorkspaceChanged(self.id));
    }

    fn gather_candidates(&self) -> AppResult<Vec<(DocumentId, FilePath, String)>> {
        let entries = {
            let ws = self.lock_read()?;
            ws.entries().to_vec()
        };
        let drafts = self.provider.open_documents();
        let open_paths: HashMap<FilePath, DocumentId> = drafts
            .iter()
            .filter_map(|(id, path, _)| path.clone().map(|p| (p, *id)))
            .collect();

        let mut out: Vec<(DocumentId, FilePath, String)> = Vec::new();
        for (id, path, content) in drafts {
            // 未落盘的草稿没有路径：用 u64 ID 合成 `/draft/<id>` 占位。
            // 该路径由数字拼出、必然合法；万一被拒则跳过该草稿，
            // 而不是 `unwrap()` 终止进程（规范 §四-15 / V11-12 裁决）。
            let fp = match path {
                Some(p) => p,
                None => match FilePath::new(format!("/draft/{}", id.as_u64())) {
                    Ok(p) => p,
                    Err(_) => continue,
                },
            };
            out.push((id, fp, content));
        }
        for entry in entries {
            if entry.kind() == EntryKind::Directory {
                continue;
            }
            if open_paths.contains_key(entry.path()) {
                continue; // 已打开的文档用内存草稿内容，避免读到磁盘旧内容
            }
            match self.repository.read_content(entry.path()) {
                Ok(content) => {
                    out.push((DocumentId::new(), entry.path().clone(), content));
                }
                Err(_) => continue, // 不可读的文件跳过
            }
        }
        Ok(out)
    }

    fn lock_read(&self) -> AppResult<std::sync::RwLockReadGuard<'_, Workspace>> {
        self.workspace
            .read()
            .map_err(|_| AppError::io("工作区读锁已损坏"))
    }

    fn lock_write(&self) -> AppResult<std::sync::RwLockWriteGuard<'_, Workspace>> {
        self.workspace
            .write()
            .map_err(|_| AppError::io("工作区写锁已损坏"))
    }
}

/// 单次命中的中间结果（字节偏移 + 行列 + 上下文片段）。
struct RawHit {
    start: usize,
    end: usize,
    line: usize,
    col: usize,
    snippet: String,
}

/// 在文本中按字面量搜索（支持大小写不敏感与整词匹配）。
///
/// 字符级扫描：以字符为单位比较，大小写不敏感走 Unicode 小写归一化，整词匹配
/// 检查命中两侧的字母数字边界。**仅返回前 `max_per_file` 个命中**（上限保护留给调用方，
/// 此处只限制单文件上限以避免超大文件阻塞 UI 帧）。
fn search_text(
    text: &str,
    pattern: &str,
    case_sensitive: bool,
    whole_word: bool,
    max_per_file: usize,
) -> Vec<RawHit> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let pat: Vec<char> = pattern.chars().collect();
    if pat.is_empty() || chars.len() < pat.len() {
        return Vec::new();
    }

    let mut hits = Vec::new();
    for i in 0..=(chars.len() - pat.len()) {
        let mut matched = true;
        for (j, &pc) in pat.iter().enumerate() {
            let tc = chars[i + j].1;
            let eq = if case_sensitive {
                tc == pc
            } else {
                tc.to_lowercase().collect::<Vec<char>>() == pc.to_lowercase().collect::<Vec<char>>()
            };
            if !eq {
                matched = false;
                break;
            }
        }
        if !matched {
            continue;
        }
        if whole_word {
            let before = i.checked_sub(1).map(|k| chars[k].1);
            let after = chars.get(i + pat.len()).map(|&(_, c)| c);
            if before.is_some_and(|c| c.is_alphanumeric())
                || after.is_some_and(|c| c.is_alphanumeric())
            {
                continue;
            }
        }
        let (start_off, _) = chars[i];
        let (end_off, end_char) = chars[i + pat.len() - 1];
        let end_off = end_off + end_char.len_utf8();

        let line = text[..start_off].bytes().filter(|&b| b == b'\n').count();
        let line_start = text[..start_off].rfind('\n').map(|x| x + 1).unwrap_or(0);
        let col = start_off - line_start;

        let snippet_start = line_start.max(start_off.saturating_sub(24));
        let snippet_end = (text[end_off..]
            .find('\n')
            .map(|x| end_off + x)
            .unwrap_or(text.len()))
        .min(end_off + 24);
        let snippet = text[snippet_start..snippet_end].to_string();

        hits.push(RawHit {
            start: start_off,
            end: end_off,
            line,
            col,
            snippet,
        });
        if hits.len() >= max_per_file {
            break;
        }
    }
    hits
}

/// 编译期断言：`WorkspaceAppService` 可跨线程共享。
const fn _assert_send_sync<T: Send + Sync>() {}
const _ASSERT_WS_SERVICE: () = _assert_send_sync::<WorkspaceAppService>();
