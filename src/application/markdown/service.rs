//! Markdown 用例编排服务（阶段 04 · S4-05 / S4-06）。
//!
//! 编排渲染、高亮、滚动同步与大纲：
//! - **防抖策略（FR-01，150ms）下沉到应用层**：[`DebouncePolicy::should_render`]
//!   是纯函数，由注入的 [`crate::shared::time::Clock`] 与领域事件时间驱动，可单测；
//! - **渲染缓存**：以 `文档 ID → (版本号, 块序列)` 为键，命中时不重新解析
//!   （断言 fake parser 调用次数即可验证）；
//! - **大文件降级（FR-06）**：源超过 [`TRUNCATE_THRESHOLD_BYTES`] 时返回
//!   `RenderedView { truncated: true }`，且不调用解析器；
//! - **滚动同步**：复用领域层 `BlockMapper`，支持块首 / 块中 / 块尾与预览点击跳转。
//!
//! 服务内部用 `Arc<Mutex<RenderCache>>` 持有缓存，方法均为 `&self`，便于事件订阅者
//! （`PreferencesChanged`）在主题切换时主动失效缓存而不引入额外耦合。
//!
//! # 模块结构
//!
//! 用例方法按场景分散在私有子模块（inherent impl 跨模块挂载，
//! `MarkdownAppService::*` 对外路径不变），本文件作为 facade 保留服务结构、
//! 构造 / 缓存访问与三个缓存类型的 re-export：
//! - `render_cache`：防抖策略与渲染缓存；
//! - `spell_cache`：拼写检查缓存；
//! - `highlight_cache`：高亮缓存与高亮防抖计时表；
//! - `preview`：预览渲染用例与内容获取 / 解析缓存原语；
//! - `highlight`：语法高亮用例；
//! - `navigation`：滚动映射与大纲重建；
//! - `spell`：拼写检查与建议；
//! - `lint`：格式检查；
//! - `view_mapper`：领域渲染产物 → DTO 的映射函数。

mod highlight;
mod lint;
mod navigation;
mod preview;
mod render_cache;
mod span_cache;
mod spell;
mod spell_cache;
mod view_mapper;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::domain::markdown::services::{
    HighlightSession, MarkdownParser, SpellChecker, SyntaxHighlighter,
};
use crate::shared::ids::DocumentId;
use crate::shared::time::{Clock, Timestamp};

use crate::application::document::DocumentContentProvider;

use super::commands::ClearCacheCommand;

pub use render_cache::{DebouncePolicy, RenderCache};
pub use span_cache::{HighlightCache, HighlightPace};
pub use spell_cache::SpellCache;

/// 超过该字节数（500 KiB）的源文档标记为截断预览（FR-06 应用层判定）。
pub const TRUNCATE_THRESHOLD_BYTES: usize = 500 * 1024;

/// 渲染缓存容量（LRU 上限）。
///
/// **单一事实来源**：生产装配（`app/bootstrap.rs`）与测试都引用本常量，避免
/// 历史上「`DEFAULT_RENDER_CACHE_CAPACITY = 16` 与 `RENDER_CACHE_CAPACITY = 32`
/// 并存」的数值漂移（缺陷 G4）。常量定义在本层（缓存类型的归属层），
/// 由上层 `app::bootstrap` 向下 `use` 引用，符合分层依赖方向。
pub const RENDER_CACHE_CAPACITY: usize = 32;

/// 高亮缓存的容量（同时缓存多少个文档的高亮结果）。
///
/// 曾是 8——注释声称「编辑区只可能同时呈现少数几个文档」，但打开文档
/// 上限默认就是 12（`max_open_documents`）：开满时缓存必然溢出，来回切
/// 标签就反复触发 syntect 全文重算（10KB 文档一次 56.5ms，问题 043 的
/// 卡顿尖峰来源）。容量对齐打开上限 32，正常使用不再互相驱逐；
/// 超出仍会驱逐旧键，不会无界增长。
const HIGHLIGHT_CACHE_CAPACITY: usize = 32;

/// 取互斥锁：**中毒（持锁线程 panic）时恢复内部数据**，而不是让进程终止。
///
/// 规范 §四-15 禁用 `unwrap()` / `expect()`——本服务的缓存锁原先一律写
/// `.lock().expect("…锁已损坏")`。本项目 `panic = "abort"` 下 panic 不展开，
/// 实际不会出现中毒，但「锁坏了就终止进程」既违反规范也无从恢复；
/// `PoisonError::into_inner` 拿回的仍是被保护的数据（缓存失效重算即可），
/// 语义等价、且不会因锁状态终止进程（V11-12 裁决）。
fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Markdown 用例服务。
pub struct MarkdownAppService {
    parser: Arc<dyn MarkdownParser>,
    highlighter: Arc<dyn SyntaxHighlighter>,
    provider: Arc<dyn DocumentContentProvider>,
    clock: Arc<dyn Clock>,
    policy: DebouncePolicy,
    cache: Arc<Mutex<RenderCache>>,
    /// 高亮结果缓存（本服务内部持有，无需外部失效：版本一变即自动失效）。
    highlight_cache: Arc<Mutex<HighlightCache>>,
    /// 高亮防抖的计时表（OPT-011 ①）。
    highlight_pace: Arc<Mutex<HighlightPace>>,
    /// 文档 → 增量高亮会话（`FR-03` / OPT-011 复议）。
    ///
    /// 命中 [`HighlightCache`] 时不碰这把锁；只有「真要重算」才取。
    /// 与缓存不同，会话**不按 range 分键**：它维护的是全文解析状态，
    /// 与调用方当前想看哪一段无关。
    highlight_sessions: Arc<Mutex<HashMap<DocumentId, Box<dyn HighlightSession>>>>,
    /// 拼写检查器；`None` 表示未注入 → 拼写检查整体降级为「不检查」。
    ///
    /// 用 `Option` 而非必填，理由与`SpellCheckError` 不进 `AppError` 一致：
    /// 拼写是**可选增值功能**，缺少实现时应用应照常工作，只是没有波浪线。
    spell: Option<Arc<dyn SpellChecker>>,
    /// 拼写检查结果缓存（文档 + 版本 → 错词区间）。
    spell_cache: Arc<Mutex<SpellCache>>,
}

impl MarkdownAppService {
    /// 构造服务（依赖全部注入）。
    pub fn new(
        parser: Arc<dyn MarkdownParser>,
        highlighter: Arc<dyn SyntaxHighlighter>,
        provider: Arc<dyn DocumentContentProvider>,
        clock: Arc<dyn Clock>,
        policy: DebouncePolicy,
        cache: Arc<Mutex<RenderCache>>,
    ) -> Self {
        Self {
            parser,
            highlighter,
            provider,
            clock,
            policy,
            cache,
            highlight_cache: Arc::new(Mutex::new(HighlightCache::new(HIGHLIGHT_CACHE_CAPACITY))),
            highlight_pace: Arc::new(Mutex::new(HighlightPace::new(HIGHLIGHT_CACHE_CAPACITY))),
            highlight_sessions: Arc::new(Mutex::new(HashMap::new())),
            spell: None,
            spell_cache: Arc::new(Mutex::new(SpellCache::new(HIGHLIGHT_CACHE_CAPACITY))),
        }
    }

    /// 注入拼写检查器（**builder 风格**，保持 `new` 签名不变）。
    ///
    /// 单独提供而不是加进 `new` 的参数表：`new` 已被大量既有测试与
    /// 组合根调用，多一个必填参数会让所有调用点都要改，且对不启用
    /// 拼写检查的路径是无谓的负担。
    #[must_use]
    pub fn with_spell_checker(mut self, spell: Arc<dyn SpellChecker>) -> Self {
        self.spell = Some(spell);
        self
    }

    /// 拼写检查结果缓存（供测试失效用）。
    pub fn spell_cache(&self) -> Arc<Mutex<SpellCache>> {
        self.spell_cache.clone()
    }

    /// 取出渲染缓存（供事件订阅者失效）。
    pub fn cache(&self) -> Arc<Mutex<RenderCache>> {
        self.cache.clone()
    }

    /// 当前时间（由注入时钟读取）。
    pub fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// 防抖策略。
    pub const fn policy(&self) -> DebouncePolicy {
        self.policy
    }

    /// 是否应当渲染（`last_edit` 距现在 >= 防抖间隔）。
    pub fn should_render(&self, last_edit: Timestamp) -> bool {
        self.policy.should_render(last_edit, self.clock.now())
    }

    /// 主动失效渲染缓存（`PreferencesChanged` / `DocumentClosed` 时调用）。
    ///
    /// 高亮、拼写、渲染三类按文档键的缓存一并失效：
    /// - **全量失效**（`doc_id = None`，主题切换）：三者全部 `invalidate_all`，
    ///   因为主题会改变着色 / 渲染样式，所有文档的缓存结果都不再有效。
    /// - **单文档失效**（`doc_id = Some`，文档关闭）：精确失效该文档在三类缓存中的条目，
    ///   立刻释放其占用的内存（缺陷 G2：此前关闭文档后条目滞留，长会话内存单调增长）。
    ///   绝不会波及活动文档——`invalidate` 只移除指定 ID。
    ///
    /// 高亮缓存虽键带版本号（本不必手动失效），但显式清一次可让「切主题 / 关文档」
    /// 后立刻释放 span 占用的内存。
    pub fn clear_cache(&self, cmd: ClearCacheCommand) {
        let mut cache = lock_or_recover(&self.cache);
        let mut hl = lock_or_recover(&self.highlight_cache);
        let mut spell = lock_or_recover(&self.spell_cache);
        let mut sessions = lock_or_recover(&self.highlight_sessions);
        match cmd.doc_id {
            Some(id) => {
                cache.invalidate(id);
                hl.invalidate(id);
                spell.invalidate(id);
                // 会话持有整篇文本与检查点，文档关闭时必须一并释放（G2 同类问题）。
                sessions.remove(&id);
            }
            None => {
                cache.invalidate_all();
                hl.invalidate_all();
                spell.invalidate_all();
                sessions.clear();
            }
        }
    }
}

/// 编译期断言：`MarkdownAppService` 可跨线程共享（缓存与 trait 对象均为 `Send + Sync`）。
const fn _assert_send_sync<T: Send + Sync>() {}
const _ASSERT_MD_SERVICE: () = _assert_send_sync::<MarkdownAppService>();
