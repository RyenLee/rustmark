//! 跨上下文事件链路测试（S4-10 / S4-11）。
//!
//! 验证 `EventSubscribers` 把领域事件转译为派生状态更新：
//! - `TextModified` → 自动保存计时（记录最后编辑）；
//! - `DocumentSaved` → 更新最近文件 + 停止追踪；
//! - `PreferencesChanged` → 失效 Markdown 渲染缓存；
//! - `DocumentClosed` → 停止追踪；
//! - 全链路：打开 → 修改 → 保存 → 最近文件正确，且无重入死循环。

use std::sync::{Arc, Mutex, RwLock};

use rustmark::application::document::commands::*;
use rustmark::application::document::registry::DocumentRegistry;
use rustmark::application::document::service::DocumentAppService;
use rustmark::application::event_subscriptions::{AutoSaveTracker, EventSubscribers};
use rustmark::application::markdown::queries::RenderPreviewQuery;
use rustmark::application::markdown::service::{
    DebouncePolicy, MarkdownAppService, RENDER_CACHE_CAPACITY, RenderCache,
};
use rustmark::application::preferences::commands::*;
use rustmark::application::preferences::service::PreferencesAppService;
use rustmark::application::workspace::queries::*;
use rustmark::application::workspace::service::WorkspaceAppService;
use rustmark::domain::markdown::value_objects::{BlockKind, SpanStyle};
use rustmark::domain::workspace::aggregate::Workspace;
use rustmark::shared::event::{DomainEvent, EventBus};
use rustmark::shared::ids::{DocumentId, WorkspaceId};
use rustmark::shared::path::FilePath;
use rustmark::shared::time::Timestamp;

use super::fakes::*;

fn fp(s: &str) -> FilePath {
    FilePath::new(s).unwrap()
}

/// 完整装配：共享事件总线 + 文档 / 工作区 / Markdown / 偏好 服务，并注册跨上下文订阅者。
struct Wiring {
    bus: Arc<InMemoryEventBus>,
    doc_service: DocumentAppService,
    doc_repo: Arc<InMemoryDocumentRepository>,
    clock: Arc<dyn rustmark::shared::time::Clock>,
    ws_service: Arc<WorkspaceAppService>,
    md_service: Arc<MarkdownAppService>,
    md_parser: Arc<FakeMarkdownParser>,
    tracker: Arc<AutoSaveTracker>,
    prefs_service: Arc<PreferencesAppService>,
}

fn wiring() -> Wiring {
    let bus: Arc<InMemoryEventBus> = Arc::new(InMemoryEventBus::new());
    let bus_dyn: Arc<dyn EventBus> = bus.clone();

    let registry = Arc::new(RwLock::new(DocumentRegistry::new()));
    let doc_repo = Arc::new(InMemoryDocumentRepository::new());
    let buffer_factory: Arc<dyn rustmark::domain::document::services::BufferFactory> =
        Arc::new(FakeBufferFactory);
    let clock: Arc<dyn rustmark::shared::time::Clock> = Arc::new(FakeClock::new(0));
    let doc_service = DocumentAppService::new(
        registry.clone(),
        doc_repo.clone(),
        bus_dyn.clone(),
        buffer_factory,
        clock.clone(),
    );

    let provider_dyn = doc_service.provider();

    let md_parser = Arc::new(FakeMarkdownParser::new(vec![
        block(1, BlockKind::Heading { level: 1 }, 0, 10, "Title"),
        block(2, BlockKind::Paragraph, 10, 30, "body text here"),
    ]));
    let md_highlighter = Arc::new(FakeHighlighter::new(vec![span(0, 5, SpanStyle::Plain)]));
    let md_cache = Arc::new(Mutex::new(RenderCache::new(RENDER_CACHE_CAPACITY)));
    let md_service = Arc::new(MarkdownAppService::new(
        md_parser.clone(),
        md_highlighter.clone(),
        provider_dyn.clone(),
        clock.clone(),
        DebouncePolicy::new(150),
        md_cache,
    ));

    let ws_repo = Arc::new(InMemoryWorkspaceRepository::new());
    let workspace = Arc::new(RwLock::new(Workspace::new(WorkspaceId::new())));
    let ws_service = Arc::new(WorkspaceAppService::new(
        ws_repo.clone(),
        provider_dyn.clone(),
        bus_dyn.clone(),
        clock.clone(),
        workspace,
        WorkspaceId::new(),
    ));

    let prefs_repo = Arc::new(InMemoryPreferencesRepository::new());
    let prefs_service = Arc::new(PreferencesAppService::new(prefs_repo, bus_dyn.clone()).unwrap());

    let tracker = Arc::new(AutoSaveTracker::new());
    let subscribers = EventSubscribers::new(
        provider_dyn.clone(),
        ws_service.clone(),
        md_service.clone(),
        clock.clone(),
        tracker.clone(),
    );
    // 事件总线已共享为 Arc；subscribe 采用 &self，可在共享后安全注册。
    subscribers.register(&*bus);

    Wiring {
        bus,
        doc_service,
        doc_repo,
        clock,
        ws_service,
        md_service,
        md_parser,
        tracker,
        prefs_service,
    }
}

#[test]
fn text_modified_records_autosave_tracker() {
    let mut w = wiring();
    let path = fp("/d/note.md");
    w.doc_repo.seed(path.clone(), "hello", None);
    let id = w.doc_service.open(OpenFileCommand { path }).unwrap();
    w.doc_service
        .insert_text(InsertTextCommand {
            id,
            offset: 0,
            text: "abc".to_string(),
        })
        .unwrap();

    assert!(
        w.tracker.contains(id),
        "TextModified 必须被订阅者记录，用于自动保存计时"
    );
    // 距最后编辑 0ms，未到自动保存间隔（30s），尚不触发。
    assert!(w.tracker.due(w.clock.now(), 30_000).is_empty());
}

#[test]
fn document_saved_updates_recent_and_forgets_tracker() {
    let mut w = wiring();
    let path = fp("/d/note.md");
    w.doc_repo.seed(path.clone(), "hello", None);
    let id = w
        .doc_service
        .open(OpenFileCommand { path: path.clone() })
        .unwrap();
    w.doc_service
        .insert_text(InsertTextCommand {
            id,
            offset: 0,
            text: "abc".to_string(),
        })
        .unwrap();
    w.doc_service
        .save(SaveCommand { id, save_as: false })
        .unwrap();

    assert!(
        !w.tracker.contains(id),
        "保存后必须停止追踪该文档的自动保存"
    );
    let recents = w.ws_service.list_recent(ListRecentQuery).unwrap();
    assert!(
        recents.iter().any(|r| r.path == path),
        "保存应经 DocumentSaved 事件更新最近文件"
    );
}

#[test]
fn preferences_changed_invalidates_markdown_cache() {
    let mut w = wiring();
    let path = fp("/d/note.md");
    w.doc_repo.seed(path, "# Hello\nworld", None);
    let id = w
        .doc_service
        .open(OpenFileCommand {
            path: fp("/d/note.md"),
        })
        .unwrap();

    let _ = w
        .md_service
        .render_preview(RenderPreviewQuery {
            doc_id: id,
            version: 0,
        })
        .unwrap();
    assert_eq!(w.md_parser.call_count(), 1);

    // 切换主题 → 广播 PreferencesChanged → 订阅者失效渲染缓存。
    w.prefs_service
        .set_theme(SetThemeCommand {
            theme: rustmark::application::dto::ThemeView {
                kind: "dark".to_string(),
                font_family: "serif".to_string(),
                font_size: 16.0,
                line_height: 1.5,
                custom: None,
            },
        })
        .unwrap();

    let _ = w
        .md_service
        .render_preview(RenderPreviewQuery {
            doc_id: id,
            version: 0,
        })
        .unwrap();
    assert_eq!(
        w.md_parser.call_count(),
        2,
        "主题切换经 PreferencesChanged 失效缓存后必须重新解析"
    );
}

#[test]
fn document_closed_forgets_tracker() {
    let mut w = wiring();
    let path = fp("/d/note.md");
    w.doc_repo.seed(path, "hello", None);
    let id = w
        .doc_service
        .open(OpenFileCommand {
            path: fp("/d/note.md"),
        })
        .unwrap();
    w.doc_service
        .insert_text(InsertTextCommand {
            id,
            offset: 0,
            text: "abc".to_string(),
        })
        .unwrap();
    assert!(w.tracker.contains(id));

    w.doc_service
        .close(CloseCommand { id, force: true })
        .unwrap();
    assert!(
        !w.tracker.contains(id),
        "关闭文档必须停止追踪自动保存（无重入）"
    );
}

#[test]
fn full_chain_input_to_recent_without_reentrancy() {
    let mut w = wiring();
    let path = fp("/d/note.md");
    w.doc_repo.seed(path.clone(), "hello", None);
    let id = w
        .doc_service
        .open(OpenFileCommand { path: path.clone() })
        .unwrap();

    // 输入修改：产生 TextModified（应被追踪）。
    w.doc_service
        .insert_text(InsertTextCommand {
            id,
            offset: 0,
            text: " world".to_string(),
        })
        .unwrap();
    assert!(w.tracker.contains(id));

    // 保存：产生 DocumentSaved（更新最近文件 + 停止追踪），但**不应**再发出 TextModified。
    w.doc_service
        .save(SaveCommand { id, save_as: false })
        .unwrap();

    assert!(!w.tracker.contains(id));
    let recents = w.ws_service.list_recent(ListRecentQuery).unwrap();
    assert!(recents.iter().any(|r| r.path == path));

    // 无重入：TextModified 只来自插入那一次，保存触发的 DocumentSaved 处理不回发 TextModified。
    let text_modified_count = w
        .bus
        .published()
        .iter()
        .filter(|e| matches!(e, DomainEvent::TextModified { .. }))
        .count();
    assert_eq!(text_modified_count, 1, "保存链路不得重新触发 TextModified");
}

#[test]
fn autosave_tracker_due_is_pure_and_consuming() {
    let tracker = AutoSaveTracker::new();
    let id = DocumentId::new();
    tracker.record(id, 0, Timestamp::from_millis(0));

    assert!(
        tracker.due(Timestamp::from_millis(0), 1_000).is_empty(),
        "距最后编辑 0ms 不应触发"
    );
    let due = tracker.due(Timestamp::from_millis(1_500), 1_000);
    assert_eq!(due, vec![id], "超过间隔应返回待保存文档");
    assert!(
        !tracker.contains(id),
        "due 后必须清除追踪，避免重复自动保存"
    );
}
