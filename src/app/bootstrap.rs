//! 依赖装配：构造基础设施实现并注入应用层服务（S6-01）。
//!
//! # 这是整个代码库**唯一**允许同时看见四个层的地方
//!
//! 其余位置都被 `check-arch` 强制单向依赖（见 `tools/arch_check.rs`）：
//! 表现层拿不到 `domain` 与 `infrastructure`，应用层拿不到 `infrastructure`。
//! 于是「用哪个实现替换哪个 trait」这个决定**无处可表达**，只能集中在这里。
//!
//! **装配顺序知识已下沉到 `rustmark_core::application::composition::compose`**
//! （R-10 项 B），本文件只负责两件事：点名具体 impl（[`production_bundle`]）
//! 与把它们注入 [`compose`]（[`Services::assemble_with_bundle`]）。
//!
//! # 装配顺序不是自由的（顺序由 core 的 `compose` 保证）
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
use rustmark_core::application::composition::{AppServices, ComposeConfig, ImplBundle, compose};
use rustmark_core::application::document::registry::DocumentRegistry;
use rustmark_core::application::document::service::DocumentAppService;
use rustmark_core::application::event_subscriptions::AutoSaveTracker;
use rustmark_core::application::export::service::ExportAppService;
use rustmark_core::application::markdown::service::{MarkdownAppService, RenderCache};
use rustmark_core::application::preferences::service::PreferencesAppService;
use rustmark_core::application::workspace::service::WorkspaceAppService;
use rustmark_core::ports::dialog::DialogPort;
use rustmark_core::ports::image_asset::ImageAssetStore;
use rustmark_core::ports::system::ExternalOpener;
use rustmark_core::ports::time::{Clock, SystemClock};
use rustmark_core::shared::ids::WorkspaceId;
use rustmark_core::shared::result::AppResult;

/// 预览防抖间隔（毫秒，FR-01）。
///
/// 与统计防抖、高亮静止窗口共用 core 的单一真源。
pub const PREVIEW_DEBOUNCE_MS: u64 = rustmark_core::shared::constants::DEBOUNCE_MS;

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
    /// `ports::system::open_external`，改经此端口（端口在 `shared`、
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

/// 生产实现集合：**全仓唯一点名 infra impl 的地方**。
///
/// 13 个具体类型在这里各出现一次；除本函数外，全仓不再有第二处
/// 「impl 与 trait 的配对」知识。要换实现只改这里，要新增可注入依赖只加
/// [`ImplBundle`] 字段 + 本函数一行。
fn production_bundle(paths: &ConfigPaths) -> ImplBundle {
    ImplBundle {
        dialogs: Arc::new(RfdDialogAdapter::new()),
        clock: Arc::new(SystemClock),
        opener: Arc::new(SystemExternalOpener::new()),
        images: Arc::new(FsImageAssetStore::new()),
        bus: Arc::new(InProcessEventBus::new()),
        preferences_repository: Arc::new(TomlPreferencesRepository::new(paths.preferences.clone())),
        document_repository: Arc::new(FsDocumentRepository::new()),
        workspace_repository: Arc::new(FsWorkspaceRepository::new(paths.recent.clone())),
        buffer_factory: Arc::new(RopeBufferFactory),
        parser: Arc::new(PulldownMarkdownParser::new()),
        highlighter: Arc::new(SyntectHighlighter::new()),
        spell_checker: Arc::new(VendoredSpellChecker::new()),
        exporter: Arc::new(RoutingExporter::new()),
    }
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
        Self::assemble_with_bundle(&paths, production_bundle(&paths))
    }

    /// 以指定配置路径与**指定对话框端口**装配。
    ///
    /// 存在的唯一理由是测试：S6-11f 起「打开文件 / 另存为 / 选择文件夹」
    /// 真的会起后台线程调端口，若不换掉端口，测试里会弹出**真实的系统
    /// 对话框**——那不是「测试慢一点」，而是测试进程卡在等人点确定，
    /// 无人值守的 CI 上直接超时。
    ///
    /// 生产路径走 [`Self::assemble`]，那里固定用 `rfd`。
    /// 覆盖方式是 **bundle 单点覆盖一个字段**，不再有中间层转调。
    pub fn assemble_with_dialogs(
        paths: ConfigPaths,
        dialogs: Arc<dyn DialogPort>,
    ) -> AppResult<Self> {
        let mut bundle = production_bundle(&paths);
        bundle.dialogs = dialogs;
        Self::assemble_with_bundle(&paths, bundle)
    }

    /// 以指定配置路径、**指定时钟**与指定对话框端口装配。
    ///
    /// 与 [`Self::assemble_with_dialogs`] 同理只为测试存在，但解决的是另一个
    /// 问题：**时间相关的行为必须可测**（`.agent/conventions.md` 第 21 条）。
    /// 典型如编辑区的 IME 组合期保护窗口（[`IME_GUARD_MS`]）——无头用例要在
    /// 「窗口内」与「窗口已超时」两种状态下断言回车行为，而真实 `SystemClock`
    /// 下整个用例跑完也未必跨过 800ms，窗口超时分支**永远测不到**。
    ///
    /// 传 [`rustmark_core::ports::time::SystemClock`] 之外的可控时钟即可推进时间。
    ///
    /// [`IME_GUARD_MS`]: crate::app::frame::IME_GUARD_MS
    pub fn assemble_with_clock_and_dialogs(
        paths: ConfigPaths,
        clock: Arc<dyn Clock>,
        dialogs: Arc<dyn DialogPort>,
    ) -> AppResult<Self> {
        let mut bundle = production_bundle(&paths);
        bundle.clock = clock;
        bundle.dialogs = dialogs;
        Self::assemble_with_bundle(&paths, bundle)
    }

    /// 以指定配置路径、时钟、对话框端口与**外部打开器端口**装配。
    ///
    /// 与 R-03 加 opener 时同款取舍已随 R-10 项 B 消失：新增依赖不再需要
    /// 「落在最深层」的级联——bundle 单点覆盖一个字段即可，测试调用点零改动。
    pub fn assemble_with_opener(
        paths: ConfigPaths,
        clock: Arc<dyn Clock>,
        dialogs: Arc<dyn DialogPort>,
        opener: Arc<dyn ExternalOpener>,
    ) -> AppResult<Self> {
        let mut bundle = production_bundle(&paths);
        bundle.clock = clock;
        bundle.dialogs = dialogs;
        bundle.opener = opener;
        Self::assemble_with_bundle(&paths, bundle)
    }

    /// 显式指定图片资源导入端口（R-04b）。
    ///
    /// 生产路径固定用 [`FsImageAssetStore`]；本变体为需要假导入器的测试存在
    /// （与 `assemble_with_dialogs` 同理）。4 个 `assemble_with_*` 互不级联，
    /// 各自只覆盖自己关心的字段。
    pub fn assemble_with_image_store(
        paths: ConfigPaths,
        clock: Arc<dyn Clock>,
        dialogs: Arc<dyn DialogPort>,
        opener: Arc<dyn ExternalOpener>,
        images: Arc<dyn ImageAssetStore>,
    ) -> AppResult<Self> {
        let mut bundle = production_bundle(&paths);
        bundle.clock = clock;
        bundle.dialogs = dialogs;
        bundle.opener = opener;
        bundle.images = images;
        Self::assemble_with_bundle(&paths, bundle)
    }

    /// 装配终点：把 bundle 交给应用层 [`compose`]，再把结果映射成 [`Services`]。
    ///
    /// 这里是 root 与 core 的唯一交接面。`Services` 比 [`AppServices`] 多一个
    /// UI 侧专用的 `AssetLoader`（图片解码），故需显式逐字段映射，而不是类型别名。
    fn assemble_with_bundle(paths: &ConfigPaths, bundle: ImplBundle) -> AppResult<Self> {
        let app = compose(bundle, ComposeConfig::new(paths.drafts_dir()))?;
        // 用**穷尽解构**而不是逐字段 `app.documents`：`AppServices` 一旦新增
        // 字段，这里立即编译失败，强迫作者决定是否在 `Services` 上暴露它。
        // 若写成逐字段取用，「core 加了服务、root 静默漏接」不会报错——
        // 那正是映射最容易腐化的方向。
        let AppServices {
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
        } = app;
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
            auto_save_tracker,
            drafts_dir,
        })
    }
}
