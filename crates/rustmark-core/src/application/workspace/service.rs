//! Workspace 用例编排服务（阶段 04 · S4-07）。
//!
//! 编排文件树、最近文件、标签页与跨文件搜索。最近文件的去重与上限（20）由领域层
//! [`crate::domain::workspace::value_objects::RecentFiles`] 保证；跨文件搜索沿用原型的
//! 命中上限（单文件 50、总 300），并**优先使用未保存草稿的内存内容**，确保检索覆盖
//! 尚未落盘的修改。
//!
//! # 模块结构
//!
//! 用例方法按场景分散在私有子模块（inherent impl 跨模块挂载，
//! `WorkspaceAppService::*` 对外路径不变），本文件作为 facade 保留服务结构、
//! 构造与跨场景常量：
//! - `recent`：最近文件的恢复 / 记录 / 失效清理与列举；
//! - `tabs`：标签页开关与列举（脏文档关闭需先保存）；
//! - `tree`：文件树扫描与新建 / 移动 / 删除 / 按根重扫；
//! - `search`：跨文件搜索（候选收集、正则 / 字面量扫描）与单文件读取；
//! - `support`：聚合锁、副本验规则与事件发布原语。

mod recent;
mod search;
mod support;
mod tabs;
mod tree;

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};

use crate::domain::workspace::aggregate::Workspace;
use crate::domain::workspace::repository::WorkspaceRepository;
use crate::shared::event::EventBus;
use crate::shared::ids::WorkspaceId;
use crate::shared::time::Clock;

use crate::application::document::DocumentContentProvider;

use search::SearchCacheEntry;

/// 单文件命中上限（与原型一致）。
pub const MAX_HITS_PER_FILE: usize = 50;
/// 总命中上限（与原型一致）。
pub const MAX_TOTAL_HITS: usize = 300;

/// 文件树扫描深度上限（S6-11）。
///
/// 取 8 层：普通工程目录深到 8 层已经极少，而无限深度会被
/// 误指向 `node_modules` 之类的大目录拖死 UI。文件系统实现另有
/// 5000 条的硬上限兜底，两者都不该由表现层决定。
pub const TREE_MAX_DEPTH: usize = 8;

/// Workspace 用例服务。
pub struct WorkspaceAppService {
    // 字段均为私有：对 `service::*` 子模块可见（定义模块的后代），
    // 服务之外只能通过用例方法访问。
    repository: Arc<dyn WorkspaceRepository>,
    provider: Arc<dyn DocumentContentProvider>,
    bus: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    workspace: Arc<RwLock<Workspace>>,
    id: WorkspaceId,
    /// 搜索结果缓存：**单槽**，只缓存最近一次「不读盘」范围查询
    /// （当前文档 / 打开的文档，即侧栏搜索标签的两条 UI 路径）的那份结果。
    ///
    /// 键 = 查询指纹 + revision 快照（见 `service::search`）：任一变化即重算，
    /// 语义与不缓存完全一致。参与读盘的全盘检索**不进缓存**——磁盘内容没有
    /// 廉价的变更信号，缓存它会让外部编辑在界面上迟迟不生效。
    ///
    /// `Mutex` 而非 `RefCell`：服务有 Send+Sync 编译期断言（见文件末尾），
    /// 且与文档服务的 `stats_cache` 同范式。
    search_cache: Mutex<Option<SearchCacheEntry>>,
    /// 搜索缓存命中次数（测试与命中率统计用；与 `stats_cache_hits` 同范式）。
    pub search_cache_hits: AtomicU64,
    /// 搜索缓存未命中次数（补未命中以算命中率，教训 G6）。
    pub search_cache_misses: AtomicU64,
}

impl WorkspaceAppService {
    /// 构造服务（依赖全部注入）。
    pub fn new(
        repository: Arc<dyn WorkspaceRepository>,
        provider: Arc<dyn DocumentContentProvider>,
        bus: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        workspace: Arc<RwLock<Workspace>>,
        id: WorkspaceId,
    ) -> Self {
        Self {
            repository,
            provider,
            bus,
            clock,
            workspace,
            id,
            search_cache: Mutex::new(None),
            search_cache_hits: AtomicU64::new(0),
            search_cache_misses: AtomicU64::new(0),
        }
    }
}

/// 编译期断言：`WorkspaceAppService` 可跨线程共享。
const fn _assert_send_sync<T: Send + Sync>() {}
const _ASSERT_WS_SERVICE: () = _assert_send_sync::<WorkspaceAppService>();
