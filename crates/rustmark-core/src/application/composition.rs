//! 依赖装配的 DI 边界（R-10 项 B，M3）。
//!
//! # 为什么装配放在应用层
//!
//! 组合根（root 的 `src/app/bootstrap.rs`）的职责被收窄为两件事：**点名 impl**
//! （[`ImplBundle`] 的字段填空，全仓唯一一处）与**注入**（把 bundle 交给
//! [`compose`]）。而「谁依赖谁、必须按什么顺序构造」这类**装配顺序知识**属于
//! 应用层的用例编排，落在这里。
//!
//! 收益是双向的：新增一个可注入依赖只需要给 [`ImplBundle`] 加一个字段 + 组合根
//! 填一行，测试调用点零改动；而顺序知识（`provider()` 后置提取、事件订阅必须在
//! `bus` 被消费前取克隆）不再散落在 root 的一串 `with_X` 级联里。
//!
//! # 依赖面红线
//!
//! 本模块只依赖 `crate::ports` / `crate::domain` / `crate::application` /
//! `crate::shared`，**绝不** `use rustmark_infra`：具体实现由组合根以
//! `Arc<dyn …>` 注入，core 的 Cargo 依赖面被
//! `tests/workspace_topology_test.rs::core_crate_dependency_face` 机械钉死为
//! `rustmark-actions + rustmark-shared + rustmark-ports`。
//!
//! # 装配顺序不是自由的
//!
//! 服务之间有真实的依赖环，必须靠 `provider()` 这类**后置提取**打破：
//!
//! ```text
//!   DocumentAppService ──provider()──► Arc<dyn DocumentContentProvider>
//!          │                                      │
//!          │            (同一份注册表状态)          ▼
//!          └── Arc<RwLock<DocumentRegistry>> ──► Markdown / Export / Workspace
//! ```
//!
//! 故顺序固定为：**注册表 → 文档服务 → 其余三个服务 → 事件订阅**。

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use crate::application::document::registry::DocumentRegistry;
use crate::application::document::service::DocumentAppService;
use crate::application::event_subscriptions::{AutoSaveTracker, EventSubscribers};
use crate::application::export::service::ExportAppService;
use crate::application::markdown::service::{
    DebouncePolicy, MarkdownAppService, RENDER_CACHE_CAPACITY, RenderCache,
};
use crate::application::preferences::service::PreferencesAppService;
use crate::application::workspace::service::WorkspaceAppService;
use crate::domain::document::repository::DocumentRepository;
use crate::domain::document::services::BufferFactory;
use crate::domain::export::services::Exporter;
use crate::domain::markdown::services::{MarkdownParser, SpellChecker, SyntaxHighlighter};
use crate::domain::preferences::repository::PreferencesRepository;
use crate::domain::workspace::aggregate::Workspace;
use crate::domain::workspace::repository::WorkspaceRepository;
use crate::ports::dialog::DialogPort;
use crate::ports::event::EventBus;
use crate::ports::image_asset::ImageAssetStore;
use crate::ports::system::ExternalOpener;
use crate::ports::time::Clock;
use crate::shared::constants::DEBOUNCE_MS;
use crate::shared::ids::WorkspaceId;
use crate::shared::result::AppResult;

/// 全部基础设施实现的集合，**由组合根点名后注入**。
///
/// 每个字段都是 core 侧定义的端口 / 领域服务 trait；core 不认识任何具体实现。
/// 想让某个依赖可注入，就给它在这里加一个字段——装配链不再需要改动。
#[derive(Clone)]
pub struct ImplBundle {
    /// 原生对话框端口（同步阻塞）。
    pub dialogs: Arc<dyn DialogPort>,
    /// 时钟端口（时间相关行为的可测性入口）。
    pub clock: Arc<dyn Clock>,
    /// 外部打开器端口（预览区外链交给系统默认程序）。
    pub opener: Arc<dyn ExternalOpener>,
    /// 图片资源导入端口。
    pub images: Arc<dyn ImageAssetStore>,
    /// 进程内事件总线。
    pub bus: Arc<dyn EventBus>,
    /// 偏好设置持久化。
    pub preferences_repository: Arc<dyn PreferencesRepository>,
    /// 文档持久化。
    pub document_repository: Arc<dyn DocumentRepository>,
    /// 工作区（最近文件）持久化。
    pub workspace_repository: Arc<dyn WorkspaceRepository>,
    /// 文本缓冲区工厂。
    pub buffer_factory: Arc<dyn BufferFactory>,
    /// Markdown 解析器。
    pub parser: Arc<dyn MarkdownParser>,
    /// 语法高亮器。
    pub highlighter: Arc<dyn SyntaxHighlighter>,
    /// 拼写检查器。
    pub spell_checker: Arc<dyn SpellChecker>,
    /// 导出器。
    pub exporter: Arc<dyn Exporter>,
}

/// 装配参数（非端口类的标量配置）。
///
/// 字段私有：组合根只经 [`ComposeConfig::new`] 构造，避免调用点直接摆弄内部
/// 字段——未来加参数时同样是「本文件一处改动」。
pub struct ComposeConfig {
    drafts_dir: PathBuf,
}

impl ComposeConfig {
    /// 以草稿目录构造装配参数。
    pub fn new(drafts_dir: impl Into<PathBuf>) -> Self {
        Self {
            drafts_dir: drafts_dir.into(),
        }
    }
}

/// 装配产物：全部用例服务 + 仍需在表现层持有的端口与共享句柄。
///
/// 组合根把它映射成自己的 `Services`（多一个 UI 侧的 `AssetLoader`）。
pub struct AppServices {
    /// 文档用例服务。
    pub documents: DocumentAppService,
    /// Markdown 渲染 / 高亮 / 滚动映射服务。
    pub markdown: Arc<MarkdownAppService>,
    /// 导出用例服务。
    pub export: ExportAppService,
    /// 偏好设置用例服务。
    pub preferences: PreferencesAppService,
    /// 工作区（文件树 / 标签 / 最近文件 / 搜索）用例服务。
    pub workspace: Arc<WorkspaceAppService>,
    /// 原生对话框端口（阻塞，只能在后台线程调用）。
    pub dialogs: Arc<dyn DialogPort>,
    /// 外部打开器端口。
    pub opener: Arc<dyn ExternalOpener>,
    /// 图片资源导入端口。
    pub images: Arc<dyn ImageAssetStore>,
    /// 文档注册表句柄。
    pub registry: Arc<RwLock<DocumentRegistry>>,
    /// 渲染缓存句柄（与 `markdown` 服务共享同一份）。
    pub render_cache: Arc<Mutex<RenderCache>>,
    /// 工作区 ID。
    pub workspace_id: WorkspaceId,
    /// 自动保存计时器。
    pub auto_save_tracker: Arc<AutoSaveTracker>,
    /// 草稿目录。
    pub drafts_dir: PathBuf,
}

/// 按固定顺序装配全部服务。
///
/// 顺序与理由见模块文档：**注册表 → 文档服务 → 其余三个服务 → 事件订阅**。
/// 装配阶段唯一的可失败点是偏好读盘（`PreferencesAppService::new`）。
pub fn compose(bundle: ImplBundle, config: ComposeConfig) -> AppResult<AppServices> {
    let drafts_dir = config.drafts_dir;
    let ImplBundle {
        dialogs,
        clock,
        opener,
        images,
        bus,
        preferences_repository,
        document_repository,
        workspace_repository,
        buffer_factory,
        parser,
        highlighter,
        spell_checker,
        exporter,
    } = bundle;

    // 渲染缓存与 `markdown` 服务共享同一份句柄；容量真源在应用层常量。
    let render_cache = Arc::new(Mutex::new(RenderCache::new(RENDER_CACHE_CAPACITY)));

    // —— 偏好（会读盘）——
    // 装配阶段唯一的可失败点。注意：**配置损坏不会走到这里失败**——
    // 偏好仓储按 ADR-003「备份原文件 + 回退默认」处理，理由是：让用户因为
    // 一个语法错误的配置文件**打不开编辑器**，比「设置悄悄回到默认」更糟
    // （后者只丢配置，不丢文档）。
    let preferences = PreferencesAppService::new(preferences_repository, bus.clone())?;

    // —— 文档服务（provider 的来源，必须先于其余三个服务）——
    let registry = Arc::new(RwLock::new(DocumentRegistry::new()));
    let documents = DocumentAppService::new(
        registry.clone(),
        document_repository,
        bus.clone(),
        buffer_factory,
        clock.clone(),
    )
    // 撤销合并窗口来自偏好设置（F-1：此前 `new_untitled` 走默认策略，
    // 领域里持久化的 `undo_merge_window_ms` 从未到达任何文档）。
    .with_undo_policy(preferences.undo_policy());
    let provider = documents.provider();

    // —— 其余服务（共享同一份 provider，即同一份注册表状态）——
    let markdown = Arc::new(
        MarkdownAppService::new(
            parser,
            highlighter,
            provider.clone(),
            clock.clone(),
            DebouncePolicy::new(DEBOUNCE_MS),
            render_cache.clone(),
        )
        // 拼写检查用 builder 注入（保持 `new` 签名稳定，见方法注释）。
        .with_spell_checker(spell_checker),
    );

    let export = ExportAppService::new(provider.clone(), exporter, bus.clone());

    let workspace_id = WorkspaceId::new();
    // 保留一份总线克隆用于注册事件订阅：下方 `workspace` 构造会消费 `bus`，
    // 故订阅接线必须在那之前取好克隆。
    let bus_for_subs = bus.clone();
    let workspace = Arc::new(WorkspaceAppService::new(
        workspace_repository,
        provider.clone(),
        bus,
        clock.clone(),
        Arc::new(RwLock::new(Workspace::new(workspace_id))),
        workspace_id,
    ));

    // —— DEF-02 接线：把自动保存计时器 / 草稿恢复接入事件总线 ——
    // 订阅者是注册进总线的 Arc 闭包，与结构体存活无关；运行期询问「该保存
    // 哪些文档」走 `AppServices::auto_save_tracker`（同一份 Arc）。
    let auto_save_tracker = Arc::new(AutoSaveTracker::new());
    let event_subscribers = EventSubscribers::new(
        provider,
        workspace.clone(),
        markdown.clone(),
        clock,
        auto_save_tracker.clone(),
    );
    event_subscribers.register(bus_for_subs.as_ref());
    // 最近文件恢复失败**不阻断启动**：它只是侧栏里的一行行列表，
    // 失败时留空即可；为此弹窗打断启动的代价远大于收益。
    let _ = workspace.restore_recent();

    Ok(AppServices {
        documents,
        markdown,
        export,
        preferences,
        workspace,
        dialogs,
        opener,
        images,
        registry,
        render_cache,
        workspace_id,
        auto_save_tracker,
        drafts_dir,
    })
}
