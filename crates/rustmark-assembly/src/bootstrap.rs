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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

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
use rustmark_infra::infrastructure::dialogs::rfd_adapter::RfdDialogAdapter;
use rustmark_infra::infrastructure::events::in_process_event_bus::InProcessEventBus;
use rustmark_infra::infrastructure::export::routing_exporter::RoutingExporter;
use rustmark_infra::infrastructure::highlighting::syntect_highlighter::SyntectHighlighter;
use rustmark_infra::infrastructure::parsing::pulldown_parser::PulldownMarkdownParser;
use rustmark_infra::infrastructure::persistence::asset_loader::AssetLoader;
use rustmark_infra::infrastructure::persistence::fs_document_repository::FsDocumentRepository;
use rustmark_infra::infrastructure::persistence::fs_workspace_repository::FsWorkspaceRepository;
use rustmark_infra::infrastructure::persistence::image_store::FsImageAssetStore;
use rustmark_infra::infrastructure::persistence::toml_preferences_repository::TomlPreferencesRepository;
use rustmark_infra::infrastructure::spell::VendoredSpellChecker;
use rustmark_infra::infrastructure::system::SystemExternalOpener;
use rustmark_infra::infrastructure::text::rope_text_buffer::RopeBufferFactory;

/// 便携数据目录名（可执行文件同级）。
const PORTABLE_DIR: &str = "data";
/// 迁移完成标记：存在即不再重复扫描旧目录。
const MIGRATION_MARKER: &str = ".rustmark-migrated";
/// 写权限探针文件名前缀（探测后立即删除）。
const WRITE_PROBE: &str = ".rustmark-write-probe";
/// 探针序号：多个测试二进制并行启动时避免抢同一个文件名。
static PROBE_SEQ: AtomicU64 = AtomicU64::new(0);

/// 配置与最近文件的存放位置。
#[derive(Clone, Debug)]
pub struct ConfigPaths {
    /// `preferences.toml` 的完整路径。
    pub preferences: PathBuf,
    /// `recent.json` 的完整路径。
    pub recent: PathBuf,
}

impl ConfigPaths {
    /// 推导数据目录：**应用目录优先**（ADR-014）。
    ///
    /// - 默认：可执行文件同级的 `data/`。Windows 便携 zip、Linux tar.gz
    ///   解压即用形态都落在这里，卸载只需删掉整个应用目录。
    /// - 回退：应用目录不可写，或属于 macOS `.app` / Linux AppImage 这类
    ///   只读、签名的分发形态时，沿用各平台配置目录
    ///   （Windows `%APPDATA%\rustmark\`、macOS
    ///   `~/Library/Application Support/rustmark/`、Linux
    ///   `$XDG_CONFIG_HOME/rustmark/`）。
    ///
    /// 与旧版不同，这里**会创建 `data/` 目录**：写权限必须在启动期判定，
    /// 否则「改设置时才失败」会把文件系统问题伪装成配置读取错误。
    /// 探测只写一个随即删除的探针文件，不产生常驻缓存。
    /// 全部环境变量缺失时退化到当前目录 `.`，首次运行不会因此 panic。
    pub fn detect() -> Self {
        Self::detect_with_note().0
    }

    /// 同 [`Self::detect`]，额外返回回退原因（`None` = 正在使用便携目录）。
    ///
    /// 组合根用这条提示做启动日志：用户若发现数据没落在应用目录，
    /// 原因必须可查，而不是「什么都没说就换了地方」。
    pub fn detect_with_note() -> (Self, Option<&'static str>) {
        let legacy = platform_config_root().join("rustmark");
        let exe = std::env::current_exe().ok();
        let (root, note) = choose_root(exe.as_deref(), legacy, probe_writable);
        (Self::in_dir(root), note)
    }

    /// 本次实际使用的数据根目录。
    ///
    /// 诊断文案打印**实际值**而不是列一张三平台路径表：默认目录变了之后，
    /// 硬编码的路径表必然与回退行为脱节。
    pub fn root_dir(&self) -> PathBuf {
        self.preferences
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// 首次启动把旧平台配置目录的内容复制进当前数据目录。
    ///
    /// **只复制、不移动、不删除**：旧目录是用户既有数据，草稿尤其不能丢；
    /// 目标已存在的文件一律跳过（用户改过的新配置优先）。成功后写
    /// `.rustmark-migrated` 标记，既避免每次启动扫描旧目录，也避免用户
    /// 删掉新配置想「重置」时旧配置又被复制回来。
    /// 任何失败都静默降级为「不迁移」，绝不阻断启动。
    /// 返回值是本次复制的文件数（供启动日志与测试观察）。
    pub fn migrate_legacy(&self) -> usize {
        let from = platform_config_root().join("rustmark");
        migrate_between(&from, &self.root_dir())
    }

    /// 指定目录（测试与调用方显式注入）。
    pub fn in_dir(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        Self {
            preferences: dir.join("preferences.toml"),
            recent: dir.join("recent.json"),
        }
    }

    /// 草稿（异常退出后可恢复内容）存放目录：数据目录下的 `.rustmark-draft/`。
    ///
    /// 草稿**不覆盖**原文件，单独成目录便于「启动扫描 + 一键清理」，
    /// 也避免与 `preferences.toml` / `recent.json` 混在一起。
    pub fn drafts_dir(&self) -> PathBuf {
        self.root_dir().join(".rustmark-draft")
    }

    /// 崩溃报告（S9-08）存放目录：数据目录下的 `crash/`。
    ///
    /// 与草稿同理单独成目录：崩溃报告是**用户要手动提交**的诊断文件，
    /// 混进配置目录会被误删；集中在 `crash/` 里，文档（troubleshooting）
    /// 才能用一句话指路。正常使用**不会**创建该目录——只有真的崩溃过
    /// （或启动清理扫描到旧报告）才会出现。
    pub fn crash_dir(&self) -> PathBuf {
        self.root_dir().join("crash")
    }
}

/// 平台配置根目录（**不含** `rustmark` 尾段）。
///
/// 它在 ADR-014 之后只承担两个角色：回退目标、旧数据迁移来源。
fn platform_config_root() -> PathBuf {
    if cfg!(target_os = "windows") {
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
    }
}

/// 只读 / 签名分发形态：不能往可执行文件旁边写数据。
///
/// 这两类目录**可能**恰好可写，但写进去是错的：macOS `.app` 一旦被改动
/// 就破坏代码签名（Gatekeeper 拒启）；AppImage 是只读 squashfs 挂载点，
/// `current_exe` 落在临时挂载目录里，那个 `data/` 重启即消失。
/// 因此主动识别并回退，不依赖「碰巧写失败」。
fn packaged_in_readonly_bundle(exe: &Path) -> bool {
    let text = exe.to_string_lossy();
    let macos_app = cfg!(target_os = "macos") && text.contains(".app/Contents/");
    let appimage = cfg!(target_os = "linux") && text.contains("/.mount_");
    macos_app || appimage
}

/// 目录可写探测：建目录 + 写一个探针文件再删掉。
fn probe_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    // 带 pid + 序号：CI 上多个测试二进制并行、或用户同时开两个实例时，
    // 固定文件名会互相截断，把「可写」误判成「不可写」而白白回退。
    let seq = PROBE_SEQ.fetch_add(1, Ordering::Relaxed);
    let probe = dir.join(format!("{WRITE_PROBE}-{}-{seq}", std::process::id()));
    let ok = std::fs::write(&probe, b"ok").is_ok();
    if ok {
        let _ = std::fs::remove_file(&probe);
    }
    ok
}

/// 目录决策（拆成可注入探测器的纯函数，便于无副作用地钉住各分支）。
fn choose_root(
    exe: Option<&Path>,
    legacy: PathBuf,
    probe: impl FnOnce(&Path) -> bool,
) -> (PathBuf, Option<&'static str>) {
    let Some(exe) = exe else {
        return (legacy, Some("无法定位可执行文件，已回退到平台配置目录"));
    };
    if packaged_in_readonly_bundle(exe) {
        return (legacy, Some("只读/签名分发形态，已回退到平台配置目录"));
    }
    let Some(parent) = exe.parent() else {
        return (legacy, Some("可执行文件没有父目录，已回退到平台配置目录"));
    };
    let portable = parent.join(PORTABLE_DIR);
    if probe(&portable) {
        (portable, None)
    } else {
        (legacy, Some("应用目录 data/ 不可写，已回退到平台配置目录"))
    }
}

/// 把旧目录内容复制到新目录，返回复制的文件数。
///
/// 公开是为了让测试能注入两个临时目录，不必改动进程级环境变量
/// （`APPDATA` / `HOME` 是全局状态，并发测试里改它必然互相干扰）。
/// 符号链接一律不跟随（宁漏勿错）；所有 I/O 失败都降级为跳过该条目。
pub fn migrate_between(from: &Path, to: &Path) -> usize {
    if from == to || !from.is_dir() || to.join(MIGRATION_MARKER).exists() {
        return 0;
    }
    if std::fs::create_dir_all(to).is_err() {
        return 0;
    }
    let mut copied = 0;
    if let Ok(entries) = std::fs::read_dir(from) {
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            if name == MIGRATION_MARKER {
                continue;
            }
            let dst = to.join(&name);
            if kind.is_dir() {
                copied += copy_tree_new_only(&entry.path(), &dst);
            } else if kind.is_file() && !dst.exists() && std::fs::copy(entry.path(), &dst).is_ok() {
                copied += 1;
            }
        }
    }
    // 即使一个文件都没复制到也要写标记：旧目录里没有需要迁移的东西时，
    // 不该每次启动都重复 read_dir。写失败则下次重试（无副作用）。
    let _ = std::fs::write(to.join(MIGRATION_MARKER), b"rustmark data migrated\n");
    copied
}

/// 递归复制目录，只补目标缺失的文件，不覆盖、不删源。
fn copy_tree_new_only(src: &Path, dst: &Path) -> usize {
    if std::fs::create_dir_all(dst).is_err() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(src) else {
        return 0;
    };
    let mut copied = 0;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let target = dst.join(entry.file_name());
        if kind.is_dir() {
            copied += copy_tree_new_only(&entry.path(), &target);
        } else if kind.is_file() && !target.exists() && std::fs::copy(entry.path(), &target).is_ok()
        {
            copied += 1;
        }
    }
    copied
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
    /// 从零装配（自动推导数据目录）。
    ///
    /// 生产入口 `app::run` 不调用本函数：它要用同一组路径同时装崩溃 hook、
    /// 做残留清理与迁移，故显式走 [`ConfigPaths::detect_with_note`] +
    /// [`Self::assemble`]。本函数保留给只需要「装配一套服务」的调用方。
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
    /// 典型如编辑区的 IME 组合期保护窗口——无头用例要在
    /// 「窗口内」与「窗口已超时」两种状态下断言回车行为，而真实 `SystemClock`
    /// 下整个用例跑完也未必跨过 800ms，窗口超时分支**永远测不到**。
    ///
    /// 传 [`rustmark_core::ports::time::SystemClock`] 之外的可控时钟即可推进时间。
    ///
    /// 该保护窗口常量定义在 root 的 `app::frame` 模块；此处仅作语义说明，
    /// 不再以 `crate::` 路径引用（本 crate 不依赖 root）。
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

/// 目录决策的纯函数分支：注入探测器，不碰真实文件系统。
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod config_paths_tests {
    use super::{Path, PathBuf, choose_root, migrate_between, packaged_in_readonly_bundle};

    #[test]
    fn writable_app_directory_uses_exe_sibling_data() {
        let legacy = PathBuf::from("/legacy/rustmark");
        let (root, note) = choose_root(
            Some(Path::new("/apps/rustmark/rustmark.exe")),
            legacy,
            |candidate| {
                assert_eq!(candidate, Path::new("/apps/rustmark/data"));
                true
            },
        );
        assert_eq!(root, PathBuf::from("/apps/rustmark/data"));
        assert!(note.is_none(), "可写时不应产生回退提示");
    }

    #[test]
    fn unwritable_app_directory_falls_back_with_a_reason() {
        let legacy = PathBuf::from("/legacy/rustmark");
        let (root, note) = choose_root(
            Some(Path::new("/apps/rustmark/rustmark")),
            legacy.clone(),
            |_| false,
        );
        assert_eq!(root, legacy);
        assert!(
            note.is_some_and(|n| n.contains("不可写")),
            "回退必须给出可查的原因：{note:?}"
        );
    }

    #[test]
    fn missing_exe_path_falls_back_to_platform_directory() {
        let legacy = PathBuf::from("/legacy/rustmark");
        let (root, note) = choose_root(None, legacy.clone(), |_| true);
        assert_eq!(root, legacy);
        assert!(note.is_some(), "拿不到 exe 路径时也要有回退提示");
    }

    #[test]
    fn ordinary_paths_are_not_mistaken_for_readonly_bundles() {
        assert!(!packaged_in_readonly_bundle(Path::new(
            "/home/me/apps/rustmark/rustmark"
        )));
        assert!(!packaged_in_readonly_bundle(Path::new(
            "C:/tools/rustmark/rustmark.exe"
        )));
    }

    #[test]
    fn same_source_and_target_is_a_no_op() {
        let dir =
            std::env::temp_dir().join(format!("rustmark-migrate-same-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        assert_eq!(migrate_between(&dir, &dir), 0);
        let _ = std::fs::remove_dir(&dir);
    }
}
