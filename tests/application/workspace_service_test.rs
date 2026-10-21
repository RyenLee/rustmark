//! Workspace 用例服务测试（S4-07）。
//!
//! 覆盖：扫描文件树、最近文件（去重 / 上限 20）、标签页（脏文档不关闭）、
//! 跨文件搜索（覆盖草稿、单文件上限 50、总上限 300）。

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
    let content: String = "apple ".repeat(60);
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
        "单文件命中不得超过 50（上限保护，避免阻塞 UI 帧）"
    );
    assert!(!result.truncated);
}

#[test]
fn search_caps_total_hits() {
    let f = build();
    let content: String = "apple ".repeat(60);
    seed_files(
        &f,
        &[
            "/d/a.md", "/d/b.md", "/d/c.md", "/d/d.md", "/d/e.md", "/d/f.md", "/d/g.md",
        ],
        &content,
    );

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
    assert_eq!(result.hits.len(), MAX_TOTAL_HITS, "总命中不得超过 300");
    assert!(result.truncated, "超过总上限必须标记截断");
}

#[test]
fn search_snippet_never_splits_utf8_chars() {
    let f = build();
    // 崩溃回归（docs/logs/crash-20261014135941-29300.txt）：摘要窗口按前后
    // 各 24 **字节**截取，混合字节宽度的上下文使窗口落点必然切进多字节
    // 字符中间，直接切片曾令搜索功能 panic。注意单一宽度序列测不出来——
    // 24 能被 3 和 4 整除，落点恰好都是边界，必须混合宽度才覆盖得到。
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
/// 用例内容让摘要窗口的**两端都落在多字节字符中间**：
/// - 窗口起点 = `128 - 24 = 104` 字节 → 落在「文」字中间（该字占 103..106）；
/// - 窗口终点 = `134 + 24 = 158` 字节 → 落在「中」字中间（该字占 154..157）。
///
/// 向下吸附 → 起点 103（首字符「文」）、终点 157（末字符「中」）；
/// 向上吸附 → 起点 106（首字符「测」）、终点 160（末字符「文」）。
///
/// 两者的**首字符与末字符都不同**，因此断言首尾字符即可区分方向；
/// 字节长度（54）再钉一道，防「整体平移但首尾恰好同字」的漏网。
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

    // 窗口两端向下吸附到最近的字符边界：起点 103、终点 157。
    assert_eq!(
        hit.snippet.chars().next(),
        Some('文'),
        "摘要起点应向下吸附到「文」字开头（字节 103），而非向上取整的「测」：{:?}",
        hit.snippet
    );
    assert_eq!(
        hit.snippet.chars().last(),
        Some('中'),
        "摘要终点应向下吸附到「中」字结尾（字节 157），而非向上取整的「文」：{:?}",
        hit.snippet
    );
    // 字节长度 = 157 - 103 = 54。
    assert_eq!(
        hit.snippet.len(),
        54,
        "摘要字节长度应为 54（103..157），方向或偏移不对：{:?}",
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
