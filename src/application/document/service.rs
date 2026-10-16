//! 文档用例编排服务（阶段 04 · S4-04）。
//!
//! 编排打开 / 新建 / 保存 / 编辑 / 撤销重做 / 关闭，并负责发布领域事件。
//! 不持有 IO：磁盘读写交给注入的 [`DocumentRepository`]，缓冲区构造交给 [`BufferFactory`]。
//!
//! 失败语义优先（原型 2.1 静默失败的反面教材）：保存失败**不**调用 `mark_saved`，
//! 因此绝不清除脏标记。
//!
//! # 模块结构
//!
//! 用例方法按场景分散在私有子模块（inherent impl 跨模块挂载，
//! `DocumentAppService::*` 对外路径不变），本文件作为 facade 保留服务结构与构造：
//! - `lifecycle`：打开 / 新建 / 保存 / 撤销重做 / 关闭与编码旁路表；
//! - `editing`：插入 / 替换 / 格式指令 / 选区设置；
//! - `queries`：文档 / 内容 / 统计 / 选区查询；
//! - `drafts`：崩溃恢复草稿；
//! - `support`：注册表锁与事件发布原语；
//! - `mapping`：聚合 / EditPlan → DTO 投影与另存为路径派生；
//! - `stats`：字数等统计纯函数。

mod drafts;
mod editing;
mod lifecycle;
mod mapping;
mod queries;
mod stats;
mod support;

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};

use crate::application::dto::StatsView;
use crate::domain::document::repository::DocumentRepository;
use crate::domain::document::services::BufferFactory;
use crate::shared::encoding::TextEncoding;
use crate::shared::event::EventBus;
use crate::shared::ids::DocumentId;
use crate::shared::time::Clock;

use super::DocumentContentProvider;
use super::registry::{DocumentRegistry, RegistryProvider};

pub use mapping::to_document_view;
pub use stats::compute_stats;

/// 文档用例服务：编排文档生命周期与编辑命令。
pub struct DocumentAppService {
    registry: Arc<RwLock<DocumentRegistry>>,
    repository: Arc<dyn DocumentRepository>,
    bus: Arc<dyn EventBus>,
    buffer_factory: Arc<dyn BufferFactory>,
    clock: Arc<dyn Clock>,
    /// 各文档**从磁盘读入时**识别到的编码（V11-03 / DEF-05）。
    ///
    /// 为何不进 `Document` 聚合：编码是 IO 的关注点（要读字节才知道），
    /// 聚合一旦持有它，领域层就被 IO 细节污染了。放在应用层做一张旁路表，
    /// 生命周期与文档一致（打开时记、关闭时删），表现层按需查询。
    ///
    /// 新建未保存的文档**不进表**——它没有编码可言，查询返回 `None`，
    /// 状态栏据此显示「—」而不是撒谎说 UTF-8。
    encodings: HashMap<DocumentId, TextEncoding>,
    /// 字数统计缓存：`(文档 ID, 版本, 统计结果)`——单槽，只缓存最近查询的那份。
    ///
    /// 为何需要（问题 043）：`get_stats` 每帧被状态栏调 **两次**
    /// （写作目标进度 + 字数项），每次都是 rope 全文快照 + O(全文) 统计。
    /// 而统计结果只随「文档版本」变化——键带上 `revision`，文本没变的帧
    /// 直接命中，语义与重算完全一致，不存在「数字滞后」。
    ///
    /// 为何单槽够用：同一帧内两个调用方查的都是**同一份**活动文档；
    /// 切文档后第一次查询必然未命中并回填，也只是付一次本就要付的钱。
    stats_cache: Mutex<Option<(DocumentId, u64, StatsView)>>,
    /// 统计缓存命中次数（调试 / 测试断言用）。
    pub stats_cache_hits: AtomicU64,
}

impl DocumentAppService {
    /// 构造服务（依赖全部注入）。
    pub fn new(
        registry: Arc<RwLock<DocumentRegistry>>,
        repository: Arc<dyn DocumentRepository>,
        bus: Arc<dyn EventBus>,
        buffer_factory: Arc<dyn BufferFactory>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            registry,
            repository,
            bus,
            buffer_factory,
            clock,
            encodings: HashMap::new(),
            stats_cache: Mutex::new(None),
            stats_cache_hits: AtomicU64::new(0),
        }
    }

    /// 取出只读内容提供者（供 Markdown 预览 / 导出服务注入，复用同一份注册表状态）。
    pub fn provider(&self) -> Arc<dyn DocumentContentProvider> {
        Arc::new(RegistryProvider::new(self.registry.clone()))
    }
}

/// 编译期断言：`DocumentAppService` 可跨线程共享。
const fn _assert_send_sync<T: Send + Sync>() {}
const _ASSERT_DOC_SERVICE: () = _assert_send_sync::<DocumentAppService>();
