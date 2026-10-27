//! 跨文件搜索用例：候选收集、正则 / 字面量扫描、单文件读取与结果缓存。
//!
//! 命中受上限保护（单文件 [`MAX_HITS_PER_FILE`]、总 [`MAX_TOTAL_HITS`]，
//! 常量在 facade），并**优先使用未保存草稿的内存内容**，确保检索覆盖
//! 尚未落盘的修改。
//!
//! # 模块结构
//!
//! 文本扫描的三条路径（字面量 / 正则 / 片段窗口）都是 `&str → Vec` 的纯计算，
//! 不依赖服务状态，已分到 [`scan`] 子模块——独立成模块才能在**不构造**
//! 本服务的前提下直接测「行号口径」「窗口覆盖命中」这类不变量。本文件只留
//! 用例编排：候选收集、上限保护与结果缓存。
//!
//! # 结果缓存（revision 键快照 + 查询指纹短路）
//!
//! 侧栏搜索标签**每帧**调一次 `search`（开关行计数与结果列表已合并为
//! 一次，见 `sidebar_view::search`），而用户打字之间的一串帧里查询与
//! 内容都没变——重算是白做的。缓存判据两层缺一不可：
//!
//! - **查询指纹**：`SearchQuery` 全字段。改模式 / 改开关 / 换范围 → 指纹变；
//! - **revision 快照**：候选集合里每份文档的 `(id, path, revision, len)`。
//!   打字（revision 变）、存盘拿到路径（path 变）、开关标签（集合变）→ 键变。
//!
//! 任一层不等即重算并覆写单槽；语义与不缓存逐字一致，只是省掉重算。
//! 只有「不读盘」的两条范围进缓存——磁盘内容没有廉价的变更信号，
//! 缓存它会让外部编辑在界面上迟迟不生效（全盘检索保持每次现读）。

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use crate::domain::workspace::value_objects::EntryKind;
use crate::shared::error::AppError;
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;
use crate::shared::pattern::Regex;
use crate::shared::result::AppResult;

use crate::application::dto::{RangeView, SearchHitView, SearchResultView};

use super::super::queries::*;
use super::{MAX_HITS_PER_FILE, MAX_TOTAL_HITS, WorkspaceAppService};

use scan::search_text;

/// 文本扫描纯函数（字符比较 / 行号游标 / 两条分支 / 片段窗口）。
///
/// 分出去的理由见该模块文档：这些都是 `&str → Vec` 的纯计算，独立成模块
/// 才能在**不构造** `WorkspaceAppService` 的前提下直接测不变量。
mod scan;

impl WorkspaceAppService {
    /// 跨文件搜索：覆盖已打开文档与磁盘文件，命中受上限保护。
    ///
    /// `cmd.only_document` 为 `Some` 时只搜那一个文档（范围切换器的
    /// 「当前文档」）。该范围**不进磁盘循环**：目标文档必然已打开，
    /// 内容就在内存里；而未打开的条目按路径读盘纯属白读——一个千文件
    /// 工作区每次搜索要开 1000 次文件、逐份解码，实测单次 150ms
    /// （debug，见 `tests/presentation/search_perf_test.rs`）。
    /// 过滤与读盘必须由**同一个**判断决定，分成两步就会重演这个缺陷。
    ///
    /// `cmd.only_open_documents` 为 `true` 时只搜**已打开的文档**
    /// （范围切换器的「打开的文档」）：连磁盘条目的循环都不进，
    /// 读盘量随标签数而不是工作区大小增长。
    ///
    /// `cmd.regex` 为 `true` 时按迷你正则引擎匹配
    /// （与查找替换面板同一引擎）：**模式编译一次、逐文件复用**；模式无效
    /// 时整次搜索失败（`AppError::Domain`，`Display` 即「正则表达式无效：…」，
    /// 视图可原样呈现）。正则模式下 `whole_word` 被忽略——引擎不支持
    /// `\b` 词边界，而在模式两侧硬拼 `\b` 对 `^`/`$`/字符类等开头的模式
    /// 会悄悄改变语义；忽略一项有言在先的开关，好过静默改写用户写的正则。
    pub fn search(&self, cmd: SearchQuery) -> AppResult<SearchResultView> {
        // 空查询直接给空结果：省掉一轮候选收集（含可能的读盘），
        // 也保证正则分支不会去编译空模式（空正则会逐位置零宽命中）。
        if cmd.pattern.is_empty() {
            return Ok(SearchResultView {
                hits: Vec::new(),
                truncated: false,
            });
        }
        // 「当前文档」范围下 `only_document` 必然是已打开文档，磁盘候选
        // 一个都不需要；只有「打开的文档」范围之外、又没限定单一文档时，
        // 才需要把未打开的文件读进来。
        let include_disk = cmd.only_document.is_none() && !cmd.only_open_documents;
        if include_disk {
            // 读盘路径不进缓存（模块文档「结果缓存」一节）：每次现读，
            // 外部编辑的可见性与加缓存之前逐字一致。
            return self.search_uncached(&cmd);
        }

        // 缓存短路：查询指纹 + revision 快照两层都相等才命中。
        let fingerprint = SearchFingerprint::of(&cmd);
        let revision = self.revision_key(cmd.only_document);
        let mut slot = self.lock_search_cache();
        if let Some(entry) = slot.as_ref()
            && entry.fingerprint == fingerprint
            && entry.revision == revision
        {
            self.search_cache_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(entry.result.clone());
        }
        let result = self.search_uncached(&cmd)?;
        self.search_cache_misses.fetch_add(1, Ordering::Relaxed);
        *slot = Some(SearchCacheEntry {
            fingerprint,
            revision,
            result: result.clone(),
        });
        Ok(result)
    }

    /// 显式清空搜索结果缓存。
    ///
    /// 常规失效不需要它：revision 快照键在内容一变时就不同（打字、存盘、
    /// 开关标签自动 miss）。本方法是给「状态已变但没有 revision 信号」的
    /// 场景（测试、未来接入的外部文件监听）留的手动出口——
    /// 仓库纪律「缓存放应用层、可显式失效」。
    pub fn invalidate_search_cache(&self) {
        *self.lock_search_cache() = None;
    }

    /// 实际执行一次检索：编译正则 → 收集候选 → 逐文件扫描。
    ///
    /// 缓存判定在 [`Self::search`] 外层，这里永远真算——性能门禁
    /// （`tests/presentation/search_perf_test.rs`）会先失效缓存再计时，
    /// 量的就是这条路径本身，不会被命中短路偷换成克隆耗时。
    fn search_uncached(&self, cmd: &SearchQuery) -> AppResult<SearchResultView> {
        // 大小写敏感在编译期固化进正则（引擎不支持内联 flag，
        // `case_sensitive` 就是唯一来源），故必须在候选收集**之前**编译：
        // 模式无效时连读盘都不必发生。
        let regex = match cmd.regex {
            true => Some(
                Regex::compile(&cmd.pattern, cmd.case_sensitive)
                    .map_err(|e| AppError::domain(e.to_string()))?,
            ),
            false => None,
        };
        let include_disk = cmd.only_document.is_none() && !cmd.only_open_documents;
        let candidates = self.gather_candidates(include_disk, cmd.only_document)?;
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
                regex.as_ref(),
                MAX_HITS_PER_FILE,
            );
            // 单文件命中被上限截断时同样标记：否则用户只看到「前 100 条」，
            // 会误以为该文件真的只有这些命中（与总量截断同一语义）。
            if raw.len() >= MAX_HITS_PER_FILE {
                truncated = true;
            }
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
    /// [`crate::domain::workspace::repository::WorkspaceRepository::read_content`]
    /// 而不是 `documents.open`。
    ///
    /// 路径合法性由仓储判定（`FilePath` 无法构造非法路径，
    /// 但文件不存在 / 不可读仍会失败），这里原样把错误往上抛，
    /// 由调用方决定如何展示——静默返回空串会让「文件被删了」
    /// 与「文件是空的」变得无法区分。
    pub fn read_file(&self, cmd: ReadFileQuery) -> AppResult<String> {
        self.repository.read_content(&cmd.path)
    }

    /// 收集搜索候选：**已打开文档**（内存内容，含未落盘草稿）+ 磁盘文件。
    ///
    /// `only_document` 为 `Some` 时只快照那一份，且**不进磁盘循环**
    /// （理由见 [`Self::search`] 的文档）。
    ///
    /// `include_disk = false` 时只收已打开文档——「打开的文档」范围
    /// 专用：读盘是这里最贵的一步（按工作区大小线性），不读未打开的
    /// 文件既是性能也是语义（用户要的就是手头开着的那几份）。
    fn gather_candidates(
        &self,
        include_disk: bool,
        only_document: Option<DocumentId>,
    ) -> AppResult<Vec<(DocumentId, FilePath, String)>> {
        // 「当前文档」范围：只取这一份的全文。
        //
        // 不能沿用下面「全部已打开文档」的路径再 `retain`：`open_documents`
        // 会对**每一份**打开文档做一次全文快照（rope → String），
        // 有 5 份 1MB 文档时每帧白复制 4MB，而其中 4 份立刻被丢掉。
        if let Some(id) = only_document {
            let path = self.provider.path_of(id);
            let Some((content, _)) = self.provider.content_and_revision(id) else {
                return Ok(Vec::new());
            };
            let fp = match path {
                Some(p) => p,
                // 与下面同一条规则：未落盘草稿用 `/draft/<id>` 占位。
                None => match FilePath::new(format!("/draft/{}", id.as_u64())) {
                    Ok(p) => p,
                    Err(_) => return Ok(Vec::new()),
                },
            };
            return Ok(vec![(id, fp, content)]);
        }

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
        if !include_disk {
            return Ok(out);
        }
        let entries = {
            let ws = self.lock_read()?;
            ws.entries().to_vec()
        };
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

    /// 构建当前查询的 revision 快照（候选来源与 [`Self::gather_candidates`] 一致）。
    ///
    /// 限定单文档时快照那一个（即使它没打开——`None` 键在它打开后自然变化）；
    /// 「打开的文档」范围快照 `open_ids` 全集——**覆盖面必须等于候选集合**，
    /// 漏一份文档它的编辑就落在键外，缓存会返回过期结果。
    fn revision_key(&self, only_document: Option<DocumentId>) -> RevisionKey {
        let ids: Vec<DocumentId> = match only_document {
            Some(id) => vec![id],
            None => self.provider.open_ids(),
        };
        ids.into_iter()
            .map(|id| {
                (
                    id,
                    self.provider.path_of(id),
                    self.provider.revision_and_len(id),
                )
            })
            .collect()
    }

    /// 取缓存锁；中毒则恢复——与 `RegistryProvider` 同策略，
    /// 单槽缓存不值得让进程崩掉或让一次搜索失败。
    fn lock_search_cache(&self) -> std::sync::MutexGuard<'_, Option<SearchCacheEntry>> {
        self.search_cache.lock().unwrap_or_else(|poisoned| {
            log::warn!("WorkspaceAppService::search_cache: Mutex poisoned; recovering guard");
            poisoned.into_inner()
        })
    }
}

/// 查询指纹：[`SearchQuery`] 参与缓存判等的全字段。
///
/// 语义上就是「这次搜索问的是什么」——模式、三个开关、范围两字段，
/// 任何一项不同都是不同的查询，不可互相复用结果。
#[derive(Clone, PartialEq, Eq)]
struct SearchFingerprint {
    pattern: String,
    regex: bool,
    case_sensitive: bool,
    whole_word: bool,
    only_document: Option<DocumentId>,
    only_open_documents: bool,
}

impl SearchFingerprint {
    /// 由查询构造（字段的直接搬运，无派生逻辑）。
    fn of(cmd: &SearchQuery) -> Self {
        Self {
            pattern: cmd.pattern.clone(),
            regex: cmd.regex,
            case_sensitive: cmd.case_sensitive,
            whole_word: cmd.whole_word,
            only_document: cmd.only_document,
            only_open_documents: cmd.only_open_documents,
        }
    }
}

/// revision 快照：候选集合里每份文档的 `(id, 路径, (revision, len))`。
///
/// 顺序即 `open_ids` 顺序（= 标签顺序，稳定）；集合、顺序、任一版本号
/// 变化都会让整键不等。`path` 进键是因为命中里的 `path` 字段直接取自它
/// （存盘前后同一文档会从 `/draft/<id>` 变成真实路径，结果必须跟着变）。
type RevisionKey = Vec<(DocumentId, Option<FilePath>, Option<(u64, usize)>)>;

/// 单槽缓存条目（facade 的 `Mutex<Option<…>>` 存的就是它）。
pub(super) struct SearchCacheEntry {
    /// 这次搜索问的是什么。
    fingerprint: SearchFingerprint,
    /// 搜的时候候选内容长什么样。
    revision: RevisionKey,
    /// 算好的结果；命中时整份克隆返回。
    result: SearchResultView,
}
