//! 依赖装配：构造基础设施实现并注入应用层服务（S6-01）。
//!
//! # 这是整个代码库**唯一**允许同时看见四个层的地方
//!
//! 其余位置都被 `check-arch` 强制单向依赖（见 `tools/arch_check.rs`）：
//! 表现层拿不到 `domain` 与 `infrastructure`，应用层拿不到 `infrastructure`。
//! 于是「用哪个实现替换哪个 trait」这个决定**无处可表达**，只能集中在这里。
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
//! 若先构造 Markdown 服务再构造文档服务，就拿不到 provider；
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
use crate::domain::preferences::repository::PreferencesRepository;
use crate::domain::workspace::aggregate::Workspace;
use crate::domain::workspace::repository::WorkspaceRepository;
use crate::infrastructure::dialogs::rfd_adapter::RfdDialogAdapter;
use crate::infrastructure::events::in_process_event_bus::InProcessEventBus;
use crate::infrastructure::export::routing_exporter::RoutingExporter;
use crate::infrastructure::highlighting::syntect_highlighter::SyntectHighlighter;
use crate::infrastructure::parsing::pulldown_parser::PulldownMarkdownParser;
use crate::infrastructure::persistence::asset_loader::AssetLoader;
use crate::infrastructure::persistence::fs_document_repository::FsDocumentRepository;
use crate::infrastructure::persistence::fs_workspace_repository::FsWorkspaceRepository;
use crate::infrastructure::persistence::image_store::FsImageAssetStore;
use crate::infrastructure::persistence::toml_preferences_repository::TomlPreferencesRepository;
use crate::infrastructure::spell::VendoredSpellChecker;
use crate::infrastructure::system::SystemExternalOpener;
use crate::infrastructure::text::rope_text_buffer::RopeBufferFactory;
use crate::shared::dialog::DialogPort;
use crate::shared::event::EventBus;
use crate::shared::ids::WorkspaceId;
use crate::shared::image_asset::ImageAssetStore;
use crate::shared::result::AppResult;
use crate::shared::system::ExternalOpener;
use crate::shared::time::{Clock, SystemClock};

/// 预览防抖间隔（毫秒，FR-01）。
pub const PREVIEW_DEBOUNCE_MS: u64 = 150;

// 渲染缓存容量（`RENDER_CACHE_CAPACITY`）的**单一事实来源**在
// `application::markdown::service` 层定义，本文件通过上方 `use` 引入并在装配处使用。
// 取 32 的权衡：每个条目存一份块序列，1MB 级文档的单份块序列可达数十 KB；
// 32 份约几 MB，换来「来回切标签页不重解析」。超过这个量级用户几乎不会用到，
// 而内存是实打实常驻的。

/// 配置与最近文件的存放位置。
#[derive(Clone, Debug)]
pub struct ConfigPaths {
    /// `preferences.toml` 的完整路径。
    pub preferences: PathBuf,
    /// `recent.json` 的完整路径。
    pub recent: PathBuf,
}

impl ConfigPaths {
    /// 按平台惯例推导配置目录。
    ///
    /// - Windows：`%APPDATA%\rustmark\`
    /// - macOS：`~/Library/Application Support/rustmark/`
    /// - Linux：`$XDG_CONFIG_HOME/rustmark/`（未设则 `~/.config/rustmark/`）
    ///
    /// 目录**不**在此创建：构造失败（如只读文件系统）应在真正写配置时才暴露，
    /// 否则应用会在「只想读一次配置」时被磁盘状态挡住启动。
    /// 全部环境变量缺失时退化到当前目录 `.`，让「首次运行」仍能进行，
    /// 而不是因取不到路径就 panic。
    pub fn detect() -> Self {
        let base = if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        } else if cfg!(target_os = "macos") {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join("Library/Application Support"))
                .unwrap_or_else(|| PathBuf::from("."))
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
                .unwrap_or_else(|| PathBuf::from("."))
        };
        Self::in_dir(base.join("rustmark"))
    }

    /// 指定目录（测试与便携模式）。
    pub fn in_dir(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        Self {
            preferences: dir.join("preferences.toml"),
            recent: dir.join("recent.json"),
        }
    }

    /// 草稿（异常退出后可恢复内容）存放目录：配置目录下的 `.rustmark-draft/`。
    ///
    /// 草稿**不覆盖**原文件，单独成目录便于「启动扫描 + 一键清理」，
    /// 也避免与 `preferences.toml` / `recent.json` 混在一起。
    pub fn drafts_dir(&self) -> PathBuf {
        self.preferences
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".rustmark-draft")
    }

    /// 崩溃报告（S9-08）存放目录：配置目录下的 `crash/`。
    ///
    /// 与草稿同理单独成目录：崩溃报告是**用户要手动提交**的诊断文件，
    /// 混进配置目录会被误删；集中在 `crash/` 里，文档（troubleshooting.md）
    /// 才能用一句话指路。正常使用**不会**创建该目录——只有真的崩溃过
    /// （或启动清理扫描到旧报告）才会出现。
    pub fn crash_dir(&self) -> PathBuf {
        self.preferences
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("crash")
    }
}

/// 装配产物：全部用例服务 + 仍需在 UI 侧持有的基础设施能力。
///
/// # 为什么这些 `Arc<dyn …>` 是 `pub`
///
/// 对话框端口必须由 `frame` 在**非 UI 线程**调用（`RfdDialogAdapter` 同步阻塞），
/// 若它藏在服务内部就无从取用。这里显式暴露，是为了让「谁在什么线程上用它」
/// 保持可见，而不是让阻塞调用悄悄混进 `update()`。
pub struct Services {
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
    /// 原生对话框端口（**阻塞**，只能在后台线程调用）。
    pub dialogs: Arc<dyn DialogPort>,
    /// 外部打开器端口：预览区点击外链时用系统默认程序打开。
    ///
    /// R-03 把 `std::process` 移出 `shared` 后，预览区不能再直调
    /// `shared::system::open_external`，改经此端口（端口在 `shared`、
    /// 实现在 `infrastructure`，与 `dialogs` / 时钟同一范式）。
    /// 测试可注入假实现断言「点了链接且参数正确」。
    pub opener: Arc<dyn ExternalOpener>,
    /// 图片资源导入端口：插入图片时把选中的文件复制进文档资源目录（R-04b）。
    ///
    /// 与 `opener` / `dialogs` 同一范式：端口在 `shared`、实现在
    /// `infrastructure`，测试可注入假实现。默认 [`FsImageAssetStore`]。
    pub images: Arc<dyn ImageAssetStore>,
    /// 文档注册表句柄，供事件订阅者失效缓存时读取版本号。
    pub registry: Arc<RwLock<DocumentRegistry>>,
    /// 渲染缓存句柄（与 `markdown` 服务共享同一份）。
    pub render_cache: Arc<Mutex<RenderCache>>,
    /// 图片资源加载器（本地路径 / 远程 URL / data URI 三种来源）。
    ///
    /// S6-05 的预览区要显示图片就必须能解码，而 `AssetLoader` 已经把
    /// 「MIME 白名单 + 体积硬约束 + 坏图不 panic」三件事做掉了
    /// （见 `infrastructure/persistence/asset_loader.rs`），此处复用而非重写。
    pub assets: AssetLoader,
    /// 工作区 ID。
    pub workspace_id: WorkspaceId,
    /// 自动保存计时器：由 `EditorApp::update` 每帧询问「哪些文档应自动保存」。
    pub auto_save_tracker: Arc<AutoSaveTracker>,
    /// 草稿目录（`.rustmark-draft/`），自动保存落盘与启动恢复都读/写这里。
    pub drafts_dir: PathBuf,
}

impl Services {
    /// 从零装配（使用平台默认配置路径）。
    pub fn bootstrap() -> AppResult<Self> {
        Self::assemble(ConfigPaths::detect())
    }

    /// 以指定配置路径装配。
    ///
    /// 唯一会返回 `Err` 的是**读不到**配置文件（权限、路径是目录等）；
    /// 「解析失败」不算错误——按 ADR-003 走备份 + 回退默认。
    /// `SyntectHighlighter::new` 也不返回 `Result`：syntect 对
    /// `include_bytes!` 直接 `unwrap`，加载失败等价于产物损坏，
    /// 不存在运行期可恢复路径。
    pub fn assemble(paths: ConfigPaths) -> AppResult<Self> {
        Self::assemble_with_dialogs(paths, Arc::new(RfdDialogAdapter::new()))
    }

    /// 以指定配置路径与**指定对话框端口**装配。
    ///
    /// 存在的唯一理由是测试：S6-11f 起「打开文件 / 另存为 / 选择文件夹」
    /// 真的会起后台线程调端口，若不换掉端口，测试里会弹出**真实的系统
    /// 对话框**——那不是「测试慢一点」，而是测试进程卡在等人点确定，
    /// 无人值守的 CI 上直接超时。
    ///
    /// 生产路径走 [`Self::assemble`]，那里固定用 `rfd`。
    pub fn assemble_with_dialogs(
        paths: ConfigPaths,
        dialogs: Arc<dyn DialogPort>,
    ) -> AppResult<Self> {
        Self::assemble_with_clock_and_dialogs(paths, Arc::new(SystemClock), dialogs)
    }

    /// 以指定配置路径、**指定时钟**与指定对话框端口装配。
    ///
    /// 与 [`Self::assemble_with_dialogs`] 同理只为测试存在，但解决的是另一个
    /// 问题：**时间相关的行为必须可测**（`.agent/conventions.md` 第 21 条）。
    /// 典型如编辑区的 IME 组合期保护窗口（[`IME_GUARD_MS`]）——无头用例要在
    /// 「窗口内」与「窗口已超时」两种状态下断言回车行为，而真实 `SystemClock`
    /// 下整个用例跑完也未必跨过 800ms，窗口超时分支**永远测不到**。
    ///
    /// 传 [`crate::shared::time::SystemClock`] 之外的可控时钟即可推进时间。
    ///
    /// [`IME_GUARD_MS`]: crate::app::frame::IME_GUARD_MS
    pub fn assemble_with_clock_and_dialogs(
        paths: ConfigPaths,
        clock: Arc<dyn Clock>,
        dialogs: Arc<dyn DialogPort>,
    ) -> AppResult<Self> {
        Self::assemble_with_opener(paths, clock, dialogs, Arc::new(SystemExternalOpener::new()))
    }

    /// 以指定配置路径、时钟、对话框端口与**外部打开器端口**装配。
    ///
    /// 转调 [`Self::assemble_with_image_store`]（固定用文件系统实现）：
    /// 与 R-03 加 opener 时同款取舍——给中间层加参数会一次性改动
    /// 所有测试调用点，新增依赖一律落在**最深层**。
    pub fn assemble_with_opener(
        paths: ConfigPaths,
        clock: Arc<dyn Clock>,
        dialogs: Arc<dyn DialogPort>,
        opener: Arc<dyn ExternalOpener>,
    ) -> AppResult<Self> {
        Self::assemble_with_image_store(
            paths,
            clock,
            dialogs,
            opener,
            Arc::new(FsImageAssetStore::new()),
        )
    }

    /// 装配链的最深点（R-04b 起）：显式指定图片资源导入端口。
    ///
    /// 生产路径经 [`Self::assemble_with_opener`] 固定用
    /// [`FsImageAssetStore`]；本变体为需要假导入器的测试存在
    /// （与 `assemble_with_dialogs` 同理）。
    pub fn assemble_with_image_store(
        paths: ConfigPaths,
        clock: Arc<dyn Clock>,
        dialogs: Arc<dyn DialogPort>,
        opener: Arc<dyn ExternalOpener>,
        images: Arc<dyn ImageAssetStore>,
    ) -> AppResult<Self> {
        // —— 共享内核 ——
        let bus: Arc<dyn EventBus> = Arc::new(InProcessEventBus::new());
        // 草稿目录在 `paths.preferences` 被 `TomlPreferencesRepository` 部分移动之前取出，
        // 否则 `paths` 进入部分移动状态后无法再整体使用。
        let drafts_dir = paths.drafts_dir();

        // —— 持久化 ——
        let preferences_repo: Arc<dyn PreferencesRepository> =
            Arc::new(TomlPreferencesRepository::new(paths.preferences));
        let document_repo: Arc<dyn DocumentRepository> = Arc::new(FsDocumentRepository::new());
        let workspace_repo: Arc<dyn WorkspaceRepository> =
            Arc::new(FsWorkspaceRepository::new(paths.recent));

        // —— 文本 / 解析 / 高亮 ——
        let buffers: Arc<dyn BufferFactory> = Arc::new(RopeBufferFactory);
        let parser = Arc::new(PulldownMarkdownParser::new());
        let render_cache = Arc::new(Mutex::new(RenderCache::new(RENDER_CACHE_CAPACITY)));

        // —— 偏好（会读盘）——
        // 装配阶段唯一的可失败点。注意：**配置损坏不会走到这里失败**——
        // `TomlPreferencesRepository` 按 ADR-003「备份原文件 + 回退默认」处理，
        // 理由是：让用户因为一个语法错误的配置文件**打不开编辑器**，
        // 比「设置悄悄回到默认」更糟（后者只丢配置，不丢文档）。
        // 备份文件会留在配置目录里，用户报障时可查。
        let preferences = PreferencesAppService::new(preferences_repo, bus.clone())?;

        // —— 文档服务（provider 的来源，必须先于其余三个服务）——
        let registry = Arc::new(RwLock::new(DocumentRegistry::new()));
        let documents = DocumentAppService::new(
            registry.clone(),
            document_repo,
            bus.clone(),
            buffers,
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
                Arc::new(SyntectHighlighter::new()),
                provider.clone(),
                clock.clone(),
                DebouncePolicy::new(PREVIEW_DEBOUNCE_MS),
                render_cache.clone(),
            )
            // 拼写检查用 builder 注入（保持 `new` 签名稳定，见方法注释）。
            // 词典在 `VendoredSpellChecker::new` 内构建一次近邻索引，之后复用。
            .with_spell_checker(Arc::new(VendoredSpellChecker::new())),
        );

        let exporter: Arc<dyn Exporter> = Arc::new(RoutingExporter::new());
        let export = ExportAppService::new(provider.clone(), exporter, bus.clone());

        let workspace_id = WorkspaceId::new();
        // 保留一份总线克隆用于注册事件订阅：下方 `workspace` 构造会消费 `bus`，
        // 故订阅接线必须在那之前取好克隆。
        let bus_for_subs = bus.clone();
        let workspace = Arc::new(WorkspaceAppService::new(
            workspace_repo,
            provider.clone(),
            bus,
            clock.clone(),
            Arc::new(RwLock::new(Workspace::new(workspace_id))),
            workspace_id,
        ));

        // —— DEF-02 接线：把自动保存计时器 / 草稿恢复接入事件总线 ——
        // 此前 `EventSubscribers` 只定义未装配，导致 `AutoSaveTracker` 从未收到
        // `TextModified`、`DocumentSaved`/`DocumentClosed` 也从未触发计时清理，
        // 自动保存与最近文件、缓存失效全部不生效。这里统一注册。
        //
        // 注册后**不保留** `EventSubscribers` 本体：订阅者是注册进总线的 Arc
        // 闭包，与结构体存活无关；运行期询问「该保存哪些文档」走
        // `Services::auto_save_tracker`（同一份 Arc），结构体留着只会多一条
        // 永远没人调的访问器（`auto_save_now` / `tracker`，2026-10-17 清理）。
        let tracker = Arc::new(AutoSaveTracker::new());
        let event_subscribers = EventSubscribers::new(
            provider,
            workspace.clone(),
            markdown.clone(),
            clock,
            tracker.clone(),
        );
        event_subscribers.register(bus_for_subs.as_ref());
        // 最近文件恢复失败**不阻断启动**：它只是侧栏里的一行行列表，
        // 失败时留空即可；为此弹窗打断启动的代价远大于收益。
        let _ = workspace.restore_recent();

        Ok(Self {
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
            assets: AssetLoader::default(),
            workspace_id,
            auto_save_tracker: tracker,
            drafts_dir,
        })
    }
}
