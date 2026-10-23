//! 跨文件搜索用例：候选收集、正则 / 字面量扫描、单文件读取与结果缓存。
//!
//! 命中受上限保护（单文件 [`MAX_HITS_PER_FILE`]、总 [`MAX_TOTAL_HITS`]，
//! 常量在 facade），并**优先使用未保存草稿的内存内容**，确保检索覆盖
//! 尚未落盘的修改。文本扫描的三条路径（字面量 / 正则 / 片段窗口）都是
//! 本模块的自由函数，不依赖服务状态，故与用例方法同文件放置。
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
use crate::shared::geometry::floor_char_boundary;
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;
use crate::shared::pattern::Regex;
use crate::shared::result::AppResult;

use crate::application::dto::{RangeView, SearchHitView, SearchResultView};

use super::super::queries::*;
use super::{MAX_HITS_PER_FILE, MAX_TOTAL_HITS, WorkspaceAppService};

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

/// 单次命中的中间结果（字节偏移 + 行列 + 上下文片段）。
struct RawHit {
    start: usize,
    end: usize,
    line: usize,
    col: usize,
    snippet: String,
}

/// 大小写不敏感的单字符比较，不分配。
///
/// `char::to_lowercase()` 是迭代器，`Iterator::eq` 直接逐项比较。原实现
/// `tc.to_lowercase().collect::<Vec<char>>() == pc.to_lowercase().collect::<Vec<char>>()`
/// 会在**每一次**字符试配时申请两个小 Vec——1MB 文档按字符逐一试配即
/// 数十万次堆分配，独占单次搜索的大头（见 `tests/presentation/search_perf_test.rs`）。
fn char_eq_ignore_case(a: char, b: char) -> bool {
    if a == b {
        return true;
    }
    // 只要有一侧是 ASCII，等值判定就退化为 ASCII 忽略大小写比较：
    // 非 ASCII 字符的小写永远不可能等于 ASCII 字符。
    if a.is_ascii() || b.is_ascii() {
        return a.eq_ignore_ascii_case(&b);
    }
    a.to_lowercase().eq(b.to_lowercase())
}

/// 在文本中搜索：`regex` 为 `Some` 走正则分支，否则按字面量
/// （支持大小写不敏感与整词匹配）。
///
/// 字面量路径字符级扫描：以字符为单位比较，大小写不敏感走 Unicode 小写归一化，
/// 整词匹配检查命中两侧的字母数字边界。**仅返回前 `max_per_file` 个命中**
/// （上限保护留给调用方，此处只限制单文件上限以避免超大文件阻塞 UI 帧）。
fn search_text(
    text: &str,
    pattern: &str,
    case_sensitive: bool,
    whole_word: bool,
    regex: Option<&Regex>,
    max_per_file: usize,
) -> Vec<RawHit> {
    if let Some(regex) = regex {
        return search_text_regex(text, regex, max_per_file);
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let pat: Vec<char> = pattern.chars().collect();
    if pat.is_empty() || chars.len() < pat.len() {
        return Vec::new();
    }

    let mut hits = Vec::new();
    // 行号与行首**增量**维护：命中位置按 `i` 单调递增，跨命中累计扫过的
    // 字节只需 O(n) 一遍。原实现每条命中都重新 `text[..start_off]` 数换行
    // 再 `rfind`，单文件 50 条命中就是 50 遍全文扫描。
    let mut line = 0usize;
    let mut line_start = 0usize;
    let mut counted_to = 0usize;
    for i in 0..=(chars.len() - pat.len()) {
        let mut matched = true;
        for (j, &pc) in pat.iter().enumerate() {
            let tc = chars[i + j].1;
            let eq = if case_sensitive {
                tc == pc
            } else {
                char_eq_ignore_case(tc, pc)
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

        // 把 [counted_to, start_off) 这段新字节数进去（两端都是字符边界）。
        if start_off > counted_to {
            let chunk = &text[counted_to..start_off];
            line += chunk.bytes().filter(|&b| b == b'\n').count();
            if let Some(pos) = chunk.rfind('\n') {
                line_start = counted_to + pos + 1;
            }
            counted_to = start_off;
        }
        let col = start_off - line_start;
        let snippet = snippet_window(text, line_start, start_off, end_off);

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

/// 正则模式的单文件搜索（[`search_text`] 的正则分支）。
///
/// 与字面量路径同一套约定：命中按字节偏移递增、行号/行首增量维护
/// （正则命中顺序由 `find_in` 从 `from` 向后扫保证单调）、单文件上限
/// `max_per_file` 兜底。大小写敏感已固化在编译期的 [`Regex`] 里，
/// 整词开关不参与（理由见 [`WorkspaceAppService::search`] 的文档）。
fn search_text_regex(text: &str, regex: &Regex, max_per_file: usize) -> Vec<RawHit> {
    let mut hits = Vec::new();
    let mut line = 0usize;
    let mut line_start = 0usize;
    let mut counted_to = 0usize;
    let mut from = 0usize;
    while hits.len() < max_per_file {
        let Some((start_off, end_off)) = regex.find_in(text, from, text.len()) else {
            break;
        };
        // 与字面量路径同一段增量行号逻辑（两端都是字符边界）。
        if start_off > counted_to {
            let chunk = &text[counted_to..start_off];
            line += chunk.bytes().filter(|&b| b == b'\n').count();
            if let Some(pos) = chunk.rfind('\n') {
                line_start = counted_to + pos + 1;
            }
            counted_to = start_off;
        }
        let col = start_off - line_start;
        let snippet = snippet_window(text, line_start, start_off, end_off);

        hits.push(RawHit {
            start: start_off,
            end: end_off,
            line,
            col,
            snippet,
        });
        // 前进：普通命中跳到终点；零宽命中（`a*` 可匹配空串）向前推进
        // 一个字符——原地不动会在同一位置无限重配（`find_in` 只扫
        // `from` 之后，零宽命中的起点原样返回）。
        from = if end_off > start_off {
            end_off
        } else {
            let Some(step) = text[start_off..].chars().next().map(char::len_utf8) else {
                break;
            };
            start_off + step
        };
    }
    hits
}

/// 取命中上下文片段（前后各 24 字节，行内截断）。
///
/// 摘要窗口按**字节**取，而 `±24` 后的落点完全可能停在多字节 UTF-8
/// 字符中间：中文 3 字节、emoji 4 字节，混合排版时 24 的偏移几乎必然
/// 切进字符内部，直接切片会 panic（崩溃记录
/// docs/logs/crash-20261014135941-29300.txt：`end byte index 305 is not
/// a char boundary; it is inside '行'`）。切片前把两端都向下对齐到字符
/// 边界：最多少取半个字符，摘要语义不受影响；`find('\n')` 那条路径
/// 本就是边界，对齐是恒等的。
fn snippet_window(text: &str, line_start: usize, start_off: usize, end_off: usize) -> String {
    let snippet_start = floor_char_boundary(text, line_start.max(start_off.saturating_sub(24)));
    let snippet_end = floor_char_boundary(
        text,
        (text[end_off..]
            .find('\n')
            .map(|x| end_off + x)
            .unwrap_or(text.len()))
        .min(end_off + 24),
    );
    text[snippet_start..snippet_end].to_string()
}
