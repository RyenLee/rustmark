//! Workspace 用例服务测试（S4-07）。
//!
//! 覆盖：扫描文件树、最近文件（去重 / 上限 20）、标签页（脏文档不关闭）、
//! 跨文件搜索（覆盖草稿、单文件上限 50、总上限 300）。

use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};

use rustmark::application::dto::{CloseDecisionView, FileEntryView};
use rustmark::application::workspace::commands::*;
use rustmark::application::workspace::queries::*;
use rustmark::application::workspace::service::{
    MAX_HITS_PER_FILE, MAX_TOTAL_HITS, WorkspaceAppService,
};
use rustmark::domain::workspace::aggregate::Workspace;
use rustmark::domain::workspace::value_objects::{EntryKind, WorkspaceEntry};
use rustmark::shared::ids::{DocumentId, WorkspaceId};
use rustmark::shared::path::FilePath;

use super::fakes::*;

fn fp(s: &str) -> FilePath {
    FilePath::new(s).unwrap()
}

type DynBus = Arc<dyn rustmark::shared::event::EventBus>;

struct WsFixture {
    service: WorkspaceAppService,
    repo: Arc<InMemoryWorkspaceRepository>,
    provider: Arc<FakeDocumentContentProvider>,
    clock: Arc<FakeClock>,
}

fn build() -> WsFixture {
    let repo = Arc::new(InMemoryWorkspaceRepository::new());
    let provider = Arc::new(FakeDocumentContentProvider::new());
    let bus: DynBus = Arc::new(InMemoryEventBus::new());
    let fake_clock = Arc::new(FakeClock::new(1000));
    let clock: Arc<dyn rustmark::shared::time::Clock> = fake_clock.clone();
    let workspace = Arc::new(RwLock::new(Workspace::new(WorkspaceId::new())));
    let service = WorkspaceAppService::new(
        repo.clone(),
        provider.clone(),
        bus.clone(),
        clock,
        workspace,
        WorkspaceId::new(),
    );
    WsFixture {
        service,
        repo,
        provider,
        clock: fake_clock,
    }
}

#[test]
fn scan_tree_returns_entries() {
    let f = build();
    f.repo.seed_entry(WorkspaceEntry::file(fp("/d/a.md")));
    f.repo.seed_entry(WorkspaceEntry::file(fp("/d/b.md")));

    let entries: Vec<FileEntryView> = f
        .service
        .scan_tree(ScanTreeCommand {
            root: fp("/d"),
            max_depth: 2,
        })
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|e| !e.is_directory));
}

#[test]
fn touch_recent_then_list() {
    let f = build();
    f.service
        .touch_recent(TouchRecentCommand {
            path: fp("/d/a.md"),
        })
        .unwrap();
    let recents = f.service.list_recent(ListRecentQuery).unwrap();
    assert_eq!(recents.len(), 1);
    assert_eq!(recents[0].path, fp("/d/a.md"));
}

#[test]
fn recent_files_dedupe_and_limit_to_20() {
    let f = build();
    for i in 0..25 {
        // 时间戳必须递增，否则「最近使用降序」在同刻记录下无法区分先后。
        f.clock.set(1000 + i as u64);
        f.service
            .touch_recent(TouchRecentCommand {
                path: fp(&format!("/d/file_{i}.md")),
            })
            .unwrap();
    }
    let recents = f.service.list_recent(ListRecentQuery).unwrap();
    assert_eq!(
        recents.len(),
        20,
        "最近文件不得超过上限 20（领域层 RecentFiles 保证）"
    );
    // 最近一次 touch 的排在最前。
    assert_eq!(recents[0].path, fp("/d/file_24.md"));
}

#[test]
fn forget_recent_removes_entry() {
    let f = build();
    f.service
        .touch_recent(TouchRecentCommand {
            path: fp("/d/a.md"),
        })
        .unwrap();
    f.service
        .forget_recent(ForgetRecentCommand {
            path: fp("/d/a.md"),
        })
        .unwrap();
    assert!(f.service.list_recent(ListRecentQuery).unwrap().is_empty());
}

#[test]
fn tab_close_dirty_asks_for_save() {
    let f = build();
    let doc_id = DocumentId::new();
    f.service.open_tab(OpenTabCommand { doc_id }).unwrap();
    assert_eq!(f.service.list_tabs(ListTabsQuery).unwrap().len(), 1);

    let decision = f
        .service
        .close_tab(CloseTabCommand {
            doc_id,
            dirty: true,
        })
        .unwrap();
    assert_eq!(decision, CloseDecisionView::NeedsSave);
    assert_eq!(f.service.list_tabs(ListTabsQuery).unwrap().len(), 1);
}

#[test]
fn tab_close_clean_removes() {
    let f = build();
    let doc_id = DocumentId::new();
    f.service.open_tab(OpenTabCommand { doc_id }).unwrap();
    let decision = f
        .service
        .close_tab(CloseTabCommand {
            doc_id,
            dirty: false,
        })
        .unwrap();
    assert_eq!(decision, CloseDecisionView::Closed);
    assert!(f.service.list_tabs(ListTabsQuery).unwrap().is_empty());
}

#[test]
fn force_close_tab_removes_regardless() {
    let f = build();
    let doc_id = DocumentId::new();
    f.service.open_tab(OpenTabCommand { doc_id }).unwrap();
    f.service
        .force_close_tab(ForceCloseTabCommand { doc_id })
        .unwrap();
    assert!(f.service.list_tabs(ListTabsQuery).unwrap().is_empty());
}

/// 装配若干磁盘文件并扫描，供搜索用例使用。
fn seed_files(f: &WsFixture, paths: &[&str], content: &str) {
    for p in paths {
        f.repo
            .seed_entry(WorkspaceEntry::new(fp(p), EntryKind::File));
        f.repo.seed_file(fp(p), content);
    }
    f.service
        .scan_tree(ScanTreeCommand {
            root: fp("/d"),
            max_depth: 4,
        })
        .unwrap();
}

#[test]
fn search_finds_disk_content() {
    let f = build();
    seed_files(&f, &["/d/a.md"], "hello world from disk");

    let result = f
        .service
        .search(SearchQuery {
            pattern: "world".to_string(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            only_document: None,
            only_open_documents: false,
        })
        .unwrap();
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].line, 0);
    assert!(!result.truncated);
}

#[test]
fn search_covers_unsaved_draft_over_disk() {
    let f = build();
    // 同一路径既有磁盘旧内容，又有未保存草稿；检索必须命中草稿。
    let path = fp("/d/conflict.md");
    f.provider.insert(
        DocumentId::new(),
        "needle in the draft copy",
        1,
        Some(path.clone()),
    );
    f.repo
        .seed_entry(WorkspaceEntry::new(path.clone(), EntryKind::File));
    f.repo
        .seed_file(path.clone(), "no needle on the stale disk copy");
    f.repo
        .seed_entry(WorkspaceEntry::new(fp("/d/other.md"), EntryKind::File));
    f.repo
        .seed_file(fp("/d/other.md"), "needle also appears on disk");
    f.service
        .scan_tree(ScanTreeCommand {
            root: fp("/d"),
            max_depth: 4,
        })
        .unwrap();

    let result = f
        .service
        .search(SearchQuery {
            pattern: "needle".to_string(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            only_document: None,
            only_open_documents: false,
        })
        .unwrap();
    assert_eq!(result.hits.len(), 2, "草稿与另一磁盘文件都应命中");
    let draft_hit = result
        .hits
        .iter()
        .find(|h| h.snippet.contains("draft"))
        .expect("必须命中未保存草稿而非磁盘旧内容");
    assert_eq!(draft_hit.path, path);
}

#[test]
fn search_caps_hits_per_file() {
    let f = build();
    // 160 次命中 > 单文件上限 100，确保真的触发截断。
    let content: String = "apple ".repeat(160);
    seed_files(&f, &["/d/a.md"], &content);

    let result = f
        .service
        .search(SearchQuery {
            pattern: "apple".to_string(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            only_document: None,
            only_open_documents: false,
        })
        .unwrap();
    assert_eq!(
        result.hits.len(),
        MAX_HITS_PER_FILE,
        "单文件命中不得超过 100（上限保护，避免阻塞 UI 帧）"
    );
    assert!(result.truncated, "单文件命中被截断时也必须标记");
}

#[test]
fn search_caps_total_hits() {
    let f = build();
    // 20 个文件 × 60 次命中 = 1200，超过总量上限 1000。
    let content: String = "apple ".repeat(60);
    let files: Vec<String> = (0..20).map(|i| format!("/d/f{i}.md")).collect();
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    seed_files(&f, &refs, &content);

    let result = f
        .service
        .search(SearchQuery {
            pattern: "apple".to_string(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            only_document: None,
            only_open_documents: false,
        })
        .unwrap();
    assert_eq!(result.hits.len(), MAX_TOTAL_HITS, "总命中不得超过上限");
    assert!(result.truncated, "超过总上限必须标记截断");
}

#[test]
fn search_snippet_never_splits_utf8_chars() {
    let f = build();
    // 崩溃回归（docs/logs/crash-20261014135941-29300.txt）：摘要窗口按前后
    // 各 40 **字节**截取，混合字节宽度的上下文使窗口落点必然切进多字节
    // 字符中间，直接切片曾令搜索功能 panic。注意单一宽度序列测不出来——
    // 40 能被 4 整除，落点恰好都是边界，必须混合宽度才覆盖得到。
    let content = format!("{}needle{}", "🎉中文测试".repeat(8), "🎉中文测试".repeat(8),);
    seed_files(&f, &["/d/mixed.md"], &content);

    let result = f
        .service
        .search(SearchQuery {
            pattern: "needle".to_string(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            only_document: None,
            only_open_documents: false,
        })
        .unwrap();
    assert_eq!(result.hits.len(), 1);
    let hit = &result.hits[0];
    assert!(
        hit.snippet.contains("needle"),
        "摘要窗口应完整覆盖命中：{:?}",
        hit.snippet
    );
    // 边界对齐只影响窗口两端的半个字符，行列坐标不受影响。
    assert_eq!(hit.line, 0);
    assert_eq!(hit.col, 128);
}

/// 摘要窗口的字符边界必须**向下**吸附（V11-54a / R-14）。
///
/// # 为什么上面那条用例不够
///
/// `search_snippet_never_splits_utf8_chars` 只断言「不 panic + 覆盖命中」——
/// **向上取整同样能过**，所以它钉不住「向下」这个语义。删掉对齐逻辑它会 panic，
/// 但把「向下」换成「向上」它一声不响。
///
/// # 判据设计
///
/// 用例内容让摘要窗口的**两端都落在多字节字符中间**（每 16 字节一个
/// 「🎉中文测试」单元）：
/// - 窗口起点 = `128 - 40 = 88` 字节 → 落在单元内偏移 8，「文」字（7..10）；
/// - 窗口终点 = `134 + 40 = 174` 字节 → 落在单元内偏移 14，「试」字（13..16）。
///
/// 向下吸附 → 起点 87（首字符「文」）、终点 173（末字符「中」）；
/// 向上吸附 → 起点 90（首字符「测」）、终点 176（末字符「试」）。
///
/// 两者的**首字符与末字符都不同**，因此断言首尾字符即可区分方向；
/// 字节长度（86）再钉一道，防「整体平移但首尾恰好同字」的漏网。
#[test]
fn search_snippet_snaps_to_char_boundaries_downward() {
    let f = build();
    let content = format!("{}needle{}", "🎉中文测试".repeat(8), "🎉中文测试".repeat(8));
    seed_files(&f, &["/d/mixed.md"], &content);

    let result = f
        .service
        .search(SearchQuery {
            pattern: "needle".to_string(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            only_document: None,
            only_open_documents: false,
        })
        .unwrap();
    assert_eq!(result.hits.len(), 1);
    let hit = &result.hits[0];

    // 窗口两端向下吸附到最近的字符边界：起点 87、终点 173。
    assert_eq!(
        hit.snippet.chars().next(),
        Some('文'),
        "摘要起点应向下吸附到「文」字开头（字节 87），而非向上取整的「测」：{:?}",
        hit.snippet
    );
    assert_eq!(
        hit.snippet.chars().last(),
        Some('中'),
        "摘要终点应向下吸附到「中」字结尾（字节 173），而非向上取整的「试」：{:?}",
        hit.snippet
    );
    // 字节长度 = 173 - 87 = 86。
    assert_eq!(
        hit.snippet.len(),
        86,
        "摘要字节长度应为 86（87..173），方向或偏移不对：{:?}",
        hit.snippet
    );
}

/// 范围切换器：「当前文档」只搜那一个文档（V11-28）。
///
/// 限定目标用**未落盘草稿**：它的 ID 由调用方给出，而磁盘文件的 ID 是
/// `gather_candidates` 现场合成的、外部拿不到。这也正是线上最常见的形态——
/// 用户正在改、还没保存的那一篇。
#[test]
fn search_can_be_limited_to_one_document() {
    let f = build();
    let draft_id = DocumentId::new();
    f.provider.insert(
        draft_id,
        "needle in the draft copy",
        1,
        Some(fp("/d/draft.md")),
    );
    f.repo
        .seed_entry(WorkspaceEntry::new(fp("/d/draft.md"), EntryKind::File));
    f.repo
        .seed_file(fp("/d/draft.md"), "no needle on the stale disk copy");
    f.repo
        .seed_entry(WorkspaceEntry::new(fp("/d/other.md"), EntryKind::File));
    f.repo
        .seed_file(fp("/d/other.md"), "needle also appears on disk");
    f.service
        .scan_tree(ScanTreeCommand {
            root: fp("/d"),
            max_depth: 4,
        })
        .unwrap();

    let query = |only: Option<DocumentId>| SearchQuery {
        pattern: "needle".to_string(),
        regex: false,
        case_sensitive: false,
        whole_word: false,
        only_document: only,
        only_open_documents: false,
    };

    // 不限范围：草稿（内存内容）+ 另一磁盘文件，两条都命中。
    let all = f.service.search(query(None)).unwrap();
    assert_eq!(all.hits.len(), 2);

    // 限定到草稿：只剩它一条，且取的是**内存草稿**而不是磁盘旧内容。
    let only = f.service.search(query(Some(draft_id))).unwrap();
    assert_eq!(only.hits.len(), 1, "限定后只应剩目标文档的命中");
    assert!(
        only.hits[0].snippet.contains("draft"),
        "应命中草稿内容而非磁盘旧副本：{:?}",
        only.hits[0].snippet
    );
    assert_eq!(only.hits[0].path, fp("/d/draft.md"));

    // 限定到不存在的 ID（文档已关掉）：空结果而非报错。
    // 上层 `search_workspace` 用「返回空结果」表达无范围可搜，
    // 这里必须同样是空结果，那条约定才立得住。
    let gone = f.service.search(query(Some(DocumentId::new()))).unwrap();
    assert!(gone.hits.is_empty(), "限定到不存在的文档应无命中");
    assert!(!gone.truncated);
}

/// 正则开关必须真的走正则匹配（修复「正则死开关」）。
///
/// 旧实现里 `SearchQuery::regex` 被忽略，`true` 时退化为字面量子串——
/// UI 上的 `.*` 开关点与不点结果完全相同。这里用**只有正则才可能命中**
/// 的模式（`n.d` 匹配 "nod"，字面量 `n.d` 在语料里不存在）钉住语义。
#[test]
fn search_regex_pattern_actually_matches_as_regex() {
    let f = build();
    seed_files(&f, &["/d/a.md"], "the node module and the nod answer");

    let result = f
        .service
        .search(SearchQuery {
            pattern: "n.d".to_string(),
            regex: true,
            case_sensitive: false,
            whole_word: false,
            only_document: None,
            only_open_documents: false,
        })
        .unwrap();
    assert_eq!(
        result.hits.len(),
        2,
        "`n.d` 应以正则命中 node 与 nod：{:?}",
        result.hits
    );

    // 同一模式按字面量搜：语料里没有 `n.d` 字面文本，必须零命中——
    // 两条对照都过，才说明 `regex` 开关真的在分岔。
    let literal = f
        .service
        .search(SearchQuery {
            pattern: "n.d".to_string(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            only_document: None,
            only_open_documents: false,
        })
        .unwrap();
    assert!(literal.hits.is_empty(), "字面量模式不应命中 node/nod");
}

/// 无效正则必须整次失败并透出**用户可读**的文案（而非静默零命中）。
///
/// 吞掉错误的话，用户输入了括号没闭合的模式，界面只显示「没有找到匹配」，
/// 会以为这个词真的不存在，永远不知道是自己写错了。
#[test]
fn search_invalid_regex_returns_user_facing_error() {
    let f = build();
    seed_files(&f, &["/d/a.md"], "anything on disk");

    let err = f
        .service
        .search(SearchQuery {
            pattern: "(未闭合".to_string(),
            regex: true,
            case_sensitive: false,
            whole_word: false,
            only_document: None,
            only_open_documents: false,
        })
        .expect_err("未闭合括号的正则必须报错而非返回空结果");
    let message = err.to_string();
    assert!(
        message.contains("正则表达式无效"),
        "错误文案应可直接呈现给用户，实际：{message:?}"
    );
}

/// 限定单文档的查询模板（缓存用例共用；该范围**不读盘**，进缓存）。
fn current_doc_query(id: DocumentId) -> SearchQuery {
    SearchQuery {
        pattern: "needle".to_string(),
        regex: false,
        case_sensitive: false,
        whole_word: false,
        only_document: Some(id),
        only_open_documents: false,
    }
}

/// 缓存命中：同一查询 + revision 快照不变 → 第二次调用走短路，
/// 计数器递增且结果与首算逐字一致。
///
/// 结果视图无 `PartialEq`，用 `Debug` 全文比对（命中少，足够分辨）。
#[test]
fn search_cache_second_identical_call_hits_and_returns_same_result() {
    let f = build();
    let id = DocumentId::new();
    f.provider
        .insert(id, "needle in the draft", 1, Some(fp("/d/a.md")));

    let first = f.service.search(current_doc_query(id)).unwrap();
    assert_eq!(
        f.service.search_cache_misses.load(Ordering::Relaxed),
        1,
        "首算必须记为 miss"
    );
    assert_eq!(f.service.search_cache_hits.load(Ordering::Relaxed), 0);

    let second = f.service.search(current_doc_query(id)).unwrap();
    assert_eq!(
        f.service.search_cache_hits.load(Ordering::Relaxed),
        1,
        "指纹与 revision 都没变，第二次必须命中"
    );
    assert_eq!(
        f.service.search_cache_misses.load(Ordering::Relaxed),
        1,
        "命中路径不得再记 miss"
    );
    assert_eq!(
        format!("{first:?}"),
        format!("{second:?}"),
        "缓存返回的结果必须与首算一致"
    );
}

/// 缓存失效（revision 层）：内容版本一变，键不等 → 重新计算并刷新结果。
///
/// 钉的是「打字后不能看到旧命中」——缓存若比对不上版本号，
/// 用户改完文档搜索结果还是改前的，且不报任何错。
#[test]
fn search_cache_reflects_content_change_after_revision_bump() {
    let f = build();
    let id = DocumentId::new();
    f.provider
        .insert(id, "nothing relevant here", 1, Some(fp("/d/a.md")));

    let before = f.service.search(current_doc_query(id)).unwrap();
    assert!(before.hits.is_empty(), "前置条件：修订 1 的内容不含 needle");
    assert_eq!(f.service.search_cache_misses.load(Ordering::Relaxed), 1);

    // 同一文档，revision 1 → 2，内容换成含 needle 的新文本。
    f.provider
        .insert(id, "a needle appears now", 2, Some(fp("/d/a.md")));

    let after = f.service.search(current_doc_query(id)).unwrap();
    assert_eq!(
        f.service.search_cache_hits.load(Ordering::Relaxed),
        0,
        "revision 变了不允许命中旧缓存"
    );
    assert_eq!(f.service.search_cache_misses.load(Ordering::Relaxed), 2);
    assert_eq!(
        after.hits.len(),
        1,
        "结果必须反映修订 2 的新内容：{:?}",
        after.hits
    );
}

/// 缓存失效（手动层）：`invalidate_search_cache()` 后即使指纹与 revision
/// 都没变也强制重算——「状态已变但没有 revision 信号」场景的手动出口。
#[test]
fn invalidate_search_cache_forces_recompute() {
    let f = build();
    let id = DocumentId::new();
    f.provider
        .insert(id, "needle in the draft", 1, Some(fp("/d/a.md")));

    f.service.search(current_doc_query(id)).unwrap();
    f.service.search(current_doc_query(id)).unwrap();
    assert_eq!(f.service.search_cache_hits.load(Ordering::Relaxed), 1);

    f.service.invalidate_search_cache();
    f.service.search(current_doc_query(id)).unwrap();
    assert_eq!(
        f.service.search_cache_hits.load(Ordering::Relaxed),
        1,
        "失效后不得命中"
    );
    assert_eq!(
        f.service.search_cache_misses.load(Ordering::Relaxed),
        2,
        "失效后必须重算"
    );
}

/// 读盘范围（全工作区）不进缓存：计数器纹丝不动，每次现读。
///
/// 磁盘内容没有廉价变更信号，缓存它会让外部编辑迟迟不生效；
/// 这条用例把「include_disk 路径绕过缓存」钉死。
#[test]
fn search_disk_scope_bypasses_cache_entirely() {
    let f = build();
    seed_files(&f, &["/d/a.md"], "needle on disk");

    let disk_query = || SearchQuery {
        pattern: "needle".to_string(),
        regex: false,
        case_sensitive: false,
        whole_word: false,
        only_document: None,
        only_open_documents: false,
    };

    let first = f.service.search(disk_query()).unwrap();
    let second = f.service.search(disk_query()).unwrap();
    assert_eq!(first.hits.len(), 1);
    assert_eq!(second.hits.len(), 1, "每次都应真算出相同结果");
    assert_eq!(
        f.service.search_cache_hits.load(Ordering::Relaxed),
        0,
        "读盘路径不得记命中"
    );
    assert_eq!(
        f.service.search_cache_misses.load(Ordering::Relaxed),
        0,
        "读盘路径不写缓存，不得记 miss"
    );
}
