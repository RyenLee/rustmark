//! 事件订阅与跨上下文编排（阶段 04 · S4-10）。
//!
//! 在应用层订阅领域事件完成跨上下文协作，事件作为**通知而非命令**——订阅者只更新
//! 派生状态，不反向修改触发事件的聚合（避免循环）：
//!
//! - `TextModified` → [`AutoSaveTracker`] 记录最后编辑时间（自动保存计时，按 `auto_save_interval`）；
//! - `DocumentSaved` → 更新「最近文件」（通过 [`DocumentContentProvider`] 取路径）；
//! - `PreferencesChanged` → 失效 Markdown 渲染缓存（主题影响渲染样式）；
//! - `DocumentClosed` → 停止追踪该文档的自动保存计时。
//!
//! **无重入死循环**：自动保存触发 `DocumentSaved`，而 `DocumentSaved` 的处理只更新最近文件、
//! 不发布 `TextModified`，故不会再次触发自动保存。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::shared::event::{DomainEvent, EventBus};
use crate::shared::ids::DocumentId;
use crate::shared::result::AppResult;
use crate::shared::time::{Clock, Timestamp};

use crate::application::document::DocumentContentProvider;
use crate::application::markdown::commands::ClearCacheCommand;
use crate::application::markdown::service::MarkdownAppService;
use crate::application::workspace::commands::TouchRecentCommand;
use crate::application::workspace::service::WorkspaceAppService;

/// 自动保存计时器（纯逻辑，可单测）。
///
/// 记录每个文档的最后一次修改时间（含版本号），由调用方在适合的时刻（如空闲 tick）
/// 询问「哪些文档应当自动保存」。
pub struct AutoSaveTracker {
    last_edit: Mutex<HashMap<DocumentId, (u64, Timestamp)>>,
}

impl AutoSaveTracker {
    /// 新建空计时器。
    #[allow(clippy::new_without_default)] // 必须显式新建以初始化内部 Mutex<HashMap>；Default 无法委托
    pub fn new() -> Self {
        Self {
            last_edit: Mutex::new(HashMap::new()),
        }
    }

    /// 记录一次修改（覆盖同文档的旧记录）。
    pub fn record(&self, doc_id: DocumentId, revision: u64, at: Timestamp) {
        if let Ok(mut map) = self.last_edit.lock() {
            map.insert(doc_id, (revision, at));
        }
    }

    /// 移除对某文档的追踪（已保存或已关闭）。
    pub fn forget(&self, doc_id: DocumentId) {
        if let Ok(mut map) = self.last_edit.lock() {
            map.remove(&doc_id);
        }
    }

    /// 是否仍在追踪某文档。
    pub fn contains(&self, doc_id: DocumentId) -> bool {
        self.last_edit
            .lock()
            .map(|map| map.contains_key(&doc_id))
            .unwrap_or(false)
    }

    /// 返回「最后编辑距现在 >= `interval_ms`」的文档，并清除其追踪（避免重复保存）。
    ///
    /// 返回后即视为已处理；下一次修改会重新记录。
    pub fn due(&self, now: Timestamp, interval_ms: u64) -> Vec<DocumentId> {
        let mut map = match self.last_edit.lock() {
            Ok(map) => map,
            Err(_) => return Vec::new(),
        };
        let mut due = Vec::new();
        map.retain(|doc_id, (_, at)| {
            if now.elapsed_ms_since(*at) >= interval_ms {
                due.push(*doc_id);
                false
            } else {
                true
            }
        });
        due
    }
}

/// 跨上下文事件订阅装配点。
///
/// 持有所需服务的共享引用，按依赖倒置接入 `app.rs`（阶段 06）。订阅者只做派生状态更新，
/// 不反向修改聚合，杜绝循环。
pub struct EventSubscribers {
    provider: Arc<dyn DocumentContentProvider>,
    workspace: Arc<WorkspaceAppService>,
    markdown: Arc<MarkdownAppService>,
    clock: Arc<dyn Clock>,
    tracker: Arc<AutoSaveTracker>,
}

impl EventSubscribers {
    /// 构造订阅装配点。
    pub fn new(
        provider: Arc<dyn DocumentContentProvider>,
        workspace: Arc<WorkspaceAppService>,
        markdown: Arc<MarkdownAppService>,
        clock: Arc<dyn Clock>,
        tracker: Arc<AutoSaveTracker>,
    ) -> Self {
        Self {
            provider,
            workspace,
            markdown,
            clock,
            tracker,
        }
    }

    /// 注册全部跨上下文订阅者到事件总线。
    pub fn register(&self, bus: &dyn EventBus) {
        let tracker = self.tracker.clone();
        let clock = self.clock.clone();
        let handler_text = Arc::new(move |event: &DomainEvent| -> AppResult<()> {
            if let DomainEvent::TextModified {
                document_id,
                revision,
                ..
            } = *event
            {
                tracker.record(document_id, revision, clock.now());
            }
            Ok(())
        });
        bus.subscribe(handler_text);

        let tracker = self.tracker.clone();
        let provider = self.provider.clone();
        let workspace = self.workspace.clone();
        let clock = self.clock.clone();
        let handler_saved = Arc::new(move |event: &DomainEvent| -> AppResult<()> {
            if let DomainEvent::DocumentSaved(id) = *event {
                let _now = clock.now();
                if let Some(path) = provider.path_of(id) {
                    workspace.touch_recent(TouchRecentCommand { path })?;
                }
                tracker.forget(id);
            }
            Ok(())
        });
        bus.subscribe(handler_saved);

        let markdown = self.markdown.clone();
        let handler_prefs = Arc::new(move |event: &DomainEvent| -> AppResult<()> {
            if matches!(event, DomainEvent::PreferencesChanged) {
                markdown.clear_cache(ClearCacheCommand { doc_id: None });
            }
            Ok(())
        });
        bus.subscribe(handler_prefs);

        let tracker = self.tracker.clone();
        let markdown = self.markdown.clone();
        let handler_closed = Arc::new(move |event: &DomainEvent| -> AppResult<()> {
            if let DomainEvent::DocumentClosed(id) = *event {
                tracker.forget(id);
                // 失效该文档在拼写 / 高亮 / 渲染三类缓存中的条目，
                // 释放其内存（缺陷 G2：关闭文档后条目原本滞留）。
                markdown.clear_cache(ClearCacheCommand { doc_id: Some(id) });
            }
            Ok(())
        });
        bus.subscribe(handler_closed);
    }
}

/// 编译期断言：`EventSubscribers` 可跨线程共享。
const fn _assert_send_sync<T: Send + Sync>() {}
const _ASSERT_SUBS: () = _assert_send_sync::<EventSubscribers>();
