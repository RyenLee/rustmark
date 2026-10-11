//! Markdown 用例编排服务（阶段 04 · S4-05 / S4-06）。
//!
//! 编排渲染、高亮、滚动同步与大纲：
//! - **防抖策略（FR-01，150ms）下沉到应用层**：[`DebouncePolicy::should_render`]
//!   是纯函数，由注入的 [`crate::shared::time::Clock`] 与领域事件时间驱动，可单测；
//! - **渲染缓存**：以 `文档 ID → (版本号, 块序列)` 为键，命中时不重新解析
//!   （断言 fake parser 调用次数即可验证）；
//! - **大文件降级（FR-06）**：源超过 [`TRUNCATE_THRESHOLD_BYTES`] 时返回
//!   `RenderedView { truncated: true }`，且不调用解析器；
//! - **滚动同步**：复用领域层 [`BlockMapper`]，支持块首 / 块中 / 块尾与预览点击跳转。
//!
//! 服务内部用 `Arc<Mutex<RenderCache>>` 持有缓存，方法均为 `&self`，便于事件订阅者
//! （`PreferencesChanged`）在主题切换时主动失效缓存而不引入额外耦合。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::domain::markdown::lint::{LintRule, LintRuleSet, MarkdownLinter};
use crate::domain::markdown::services::{
    BlockMapper, MarkdownParser, SpellChecker, SyntaxHighlighter,
};
use crate::domain::markdown::value_objects::{
    BlockId, BlockKind, CellAlignment, InlineSpan, InlineStyle, RenderedBlock, SpanStyle,
};
use crate::shared::error::AppError;
use crate::shared::geometry::ByteOffset;
use crate::shared::ids::DocumentId;
use crate::shared::result::AppResult;
use crate::shared::text::word_bounds_at;
use crate::shared::time::{Clock, Timestamp};

use crate::application::document::DocumentContentProvider;
use crate::application::dto::{
    BlockView, CellAlignView, DefinitionBodyView, DefinitionEntryView, LintIssueView,
    LintRulesView, ListItemView, OutlineItemView, OutlineView, RangeView, RenderedView, SpanView,
    TableView,
};

use super::commands::*;
use super::queries::*;

/// 超过该字节数（500 KiB）的源文档标记为截断预览（FR-06 应用层判定）。
pub const TRUNCATE_THRESHOLD_BYTES: usize = 500 * 1024;

/// 偏好里的 lint 开关 → 领域规则集（V11-08）。
///
/// 映射放在应用层而不是让领域去读偏好：领域不该知道「设置」这个上下文，
/// 它只该知道「要跑哪几条规则」。
fn lint_rule_set(view: &LintRulesView) -> LintRuleSet {
    let mut rules = Vec::new();
    if view.heading_jump {
        rules.push(LintRule::HeadingJump);
    }
    if view.list_marker_mix {
        rules.push(LintRule::ListMarkerMixed);
    }
    if view.trailing_whitespace {
        rules.push(LintRule::TrailingWhitespace);
    }
    if view.unclosed_fence {
        rules.push(LintRule::UnclosedFence);
    }
    LintRuleSet::new(rules)
}

/// 默认渲染缓存容量（LRU 上限）。
pub const DEFAULT_RENDER_CACHE_CAPACITY: usize = 16;

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

/// 防抖策略（FR-01，150ms）：把时间相关决策做成可注入、可单测的纯函数。
///
/// 原型把 150ms 防抖写在组件里（`setTimeout`），无法单测，这是可测性上的教训；
/// 本类型把「是否应当渲染」收束为一个纯判断。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DebouncePolicy {
    interval_ms: u64,
}

impl DebouncePolicy {
    /// 构造策略；`interval_ms` 通常为 150（FR-01）。
    pub const fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }

    /// 防抖间隔（毫秒）。
    pub const fn interval_ms(&self) -> u64 {
        self.interval_ms
    }

    /// 是否应当渲染：**距上次编辑 >= interval_ms（含端点）** 时返回 `true`。
    ///
    /// - 越短（仍在防抖窗口内）→ `false`：连续输入只保留最后一次；
    /// - 时钟回拨（now < last_edit，理论上不应发生）→ 饱和到 `0`，返回 `false`。
    pub fn should_render(&self, last_edit: Timestamp, now: Timestamp) -> bool {
        now.elapsed_ms_since(last_edit) >= self.interval_ms
    }
}

/// 一份缓存的渲染结果。
struct CachedRender {
    revision: u64,
    blocks: Vec<RenderedBlock>,
}

/// 渲染缓存：文档 ID → (版本号, 块序列)，近似 LRU（达到容量后驱逐一个旧键）。
#[derive(Default)]
pub struct RenderCache {
    entries: HashMap<DocumentId, CachedRender>,
    capacity: usize,
}

impl RenderCache {
    /// 构造缓存；容量至少为 1。
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity: capacity.max(1),
        }
    }

    /// 取缓存命中（要求版本号一致，否则视为失效）。
    pub fn get(&self, id: DocumentId, revision: u64) -> Option<&Vec<RenderedBlock>> {
        self.entries
            .get(&id)
            .filter(|entry| entry.revision == revision)
            .map(|entry| &entry.blocks)
    }

    /// 写入缓存（达到容量且为新键时驱逐一个旧键）。
    pub fn insert(&mut self, id: DocumentId, revision: u64, blocks: Vec<RenderedBlock>) {
        if self.entries.len() >= self.capacity
            && !self.entries.contains_key(&id)
            && let Some(&old) = self.entries.keys().next()
        {
            self.entries.remove(&old);
        }
        self.entries.insert(id, CachedRender { revision, blocks });
    }

    /// 失效单个文档的缓存。
    pub fn invalidate(&mut self, id: DocumentId) {
        self.entries.remove(&id);
    }

    /// 失效全部缓存（主题切换后调用）。
    pub fn invalidate_all(&mut self) {
        self.entries.clear();
    }

    /// 缓存条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 一份缓存的高亮结果。
struct CachedHighlight {
    revision: u64,
    lang: Option<String>,
    range: (usize, usize),
    spans: Vec<SpanView>,
}

/// 一份缓存的拼写检查结果。
struct CachedSpell {
    revision: u64,
    hits: Vec<(usize, usize)>,
}

/// 拼写检查缓存：文档 ID → 上一次的错词区间（近似 LRU）。
///
/// # 为什么拼写检查也需要缓存
///
/// 词表近邻查找虽已做到 O(词长)，但整篇文档仍是 O(全文)——实测 900KB 文档
/// 单次 94ms（debug）。编辑区每帧都可能请求，滚动/空闲帧反复付这份钱不可接受。
/// 键带版本号 → 内容一改即失效，**与重算完全一致**，不存在「波浪线滞后」。
///
/// 与 [`HighlightCache`] 同构，故刻意复用其容量常量而非另立配置。
#[derive(Default)]
pub struct SpellCache {
    entries: HashMap<DocumentId, CachedSpell>,
    capacity: usize,
    /// 命中次数（调试 / 测试断言用）。
    pub hits: u64,
    /// 未命中次数。
    pub misses: u64,
}

impl SpellCache {
    /// 构造容量为 `capacity` 的缓存。
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity,
            hits: 0,
            misses: 0,
        }
    }

    /// 读取；版本不匹配视为未命中。
    pub fn get(&mut self, doc_id: DocumentId, revision: u64) -> Option<Vec<(usize, usize)>> {
        match self.entries.get(&doc_id) {
            Some(e) if e.revision == revision => {
                self.hits += 1;
                Some(e.hits.clone())
            }
            _ => {
                self.misses += 1;
                None
            }
        }
    }

    /// 写入（超容量时驱逐一个旧键）。
    pub fn insert(&mut self, doc_id: DocumentId, revision: u64, hits: Vec<(usize, usize)>) {
        if self.entries.len() >= self.capacity
            && !self.entries.contains_key(&doc_id)
            && let Some(k) = self.entries.keys().next().copied()
        {
            self.entries.remove(&k);
        }
        self.entries.insert(doc_id, CachedSpell { revision, hits });
    }

    /// 清空。
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 高亮缓存：文档 ID → 上一次的高亮结果（近似 LRU，达到容量后驱逐一个旧键）。
///
/// # 为什么需要（S8-05 实测）
///
/// 编辑区每帧请求全文 span，而 syntect 解析是 O(全文)：10KB 文档一次 56.5ms。
/// 文档未变的帧（滚动 / 空闲 / 切换面板）占绝大多数，却要反复付这份钱。
/// 键里带上版本号 → 内容一改即失效，**语义与重算完全一致**，不存在「颜色滞后」。
///
/// 与 [`RenderCache`] 的区别：那份缓存的是解析产物（预览用），这份缓存的是
/// 高亮产物（编辑区用）；两者的失效条件都是「文档版本变化」。
#[derive(Default)]
pub struct HighlightCache {
    entries: HashMap<DocumentId, CachedHighlight>,
    capacity: usize,
    /// 命中次数（调试 / 测试断言用）。
    pub hits: u64,
    /// 未命中次数。
    pub misses: u64,
}

impl HighlightCache {
    /// 构造缓存；容量至少为 1。
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity: capacity.max(1),
            hits: 0,
            misses: 0,
        }
    }

    /// 取缓存命中（版本 / 语言 / 区间任一不同即视为失效）。
    pub fn get(
        &mut self,
        id: DocumentId,
        revision: u64,
        lang: Option<&str>,
        range: (usize, usize),
    ) -> Option<&Vec<SpanView>> {
        let matched = self.entries.get(&id).is_some_and(|e| {
            e.revision == revision && e.lang.as_deref() == lang && e.range == range
        });
        if matched {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        if !matched {
            return None;
        }
        self.entries.get(&id).map(|e| &e.spans)
    }

    /// 写入缓存（达到容量且为新键时驱逐一个旧键）。
    pub fn insert(
        &mut self,
        id: DocumentId,
        revision: u64,
        lang: Option<String>,
        range: (usize, usize),
        spans: Vec<SpanView>,
    ) {
        if self.entries.len() >= self.capacity
            && !self.entries.contains_key(&id)
            && let Some(&old) = self.entries.keys().next()
        {
            self.entries.remove(&old);
        }
        self.entries.insert(
            id,
            CachedHighlight {
                revision,
                lang,
                range,
                spans,
            },
        );
    }

    /// 失效单个文档的缓存。
    pub fn invalidate(&mut self, id: DocumentId) {
        self.entries.remove(&id);
    }

    /// 失效全部缓存。
    pub fn invalidate_all(&mut self) {
        self.entries.clear();
    }

    /// 缓存条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 高亮防抖：记住「最近一次发现文档版本变化的时刻」（OPT-011 ①）。
///
/// # 为什么必须打在「版本变化被发现的那一帧」
///
/// 与预览防抖（OPT-012）同一个教训：时间戳若打在「算完了」的时刻，计时起点会被
/// 每次计算刷新，语义就从**防抖**退化成**节流**——连续打字照样每帧重算，
/// 10KB 文档白付 56ms/帧。这里打的戳来自「本帧见到的 revision 与上次不同」
/// 这一事实，**与是否真的算了高亮无关**，因此防抖期内戳不会被刷新。
///
/// # 为什么首次见某篇文档不防抖
///
/// 打开 / 切换文档应当**立刻**有颜色：那是用户停下来看的时刻，
/// 再等 150ms 只会看到一段「先无色、再上色」的闪烁。
#[derive(Default)]
pub struct HighlightPace {
    /// 文档 ID → (上次见到的版本, 上次见到它的时刻)。
    seen: HashMap<DocumentId, (u64, Timestamp)>,
    capacity: usize,
}

impl HighlightPace {
    /// 构造；容量至少为 1。
    pub fn new(capacity: usize) -> Self {
        Self {
            seen: HashMap::new(),
            capacity: capacity.max(1),
        }
    }

    /// 记录本帧见到的版本，返回 `true` 表示「文档已静止，可以重算」。
    ///
    /// - 首次见该文档 → `true`（立刻上色，见类型文档）；
    /// - 版本变了 → 记下新时刻并返回 `false`（本帧不上色）；
    /// - 版本未变 → 距上次变化是否已满 `interval_ms`。
    pub fn settled(
        &mut self,
        id: DocumentId,
        revision: u64,
        now: Timestamp,
        interval_ms: u64,
    ) -> bool {
        match self.seen.get(&id) {
            None => {
                self.remember(id, revision, now);
                true
            }
            Some((rev, _)) if *rev != revision => {
                self.remember(id, revision, now);
                false
            }
            Some((_, at)) => now.elapsed_ms_since(*at) >= interval_ms,
        }
    }

    fn remember(&mut self, id: DocumentId, revision: u64, now: Timestamp) {
        if self.seen.len() >= self.capacity
            && !self.seen.contains_key(&id)
            && let Some(&old) = self.seen.keys().next()
        {
            self.seen.remove(&old);
        }
        self.seen.insert(id, (revision, now));
    }

    /// 已登记的文档数（测试用）。
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    /// 是否为空（测试用）。
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
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
    /// 拼写检查器；`None` 表示未注入 → 拼写检查整体降级为「不检查」。
    ///
    /// 用 `Option` 而非必填，理由与`SpellCheckError` 不进 `AppError` 一致：
    /// 拼写是**可选增值功能**，缺少实现时应用应照常工作，只是没有波浪线。
    spell: Option<Arc<dyn SpellChecker>>,
    /// 拼写检查结果缓存（文档 + 版本 → 错词区间）。
    spell_cache: Arc<Mutex<SpellCache>>,
}

/// 高亮缓存的容量（同时缓存多少个文档的高亮结果）。
///
/// 编辑区只可能同时呈现少数几个文档，8 已远大于实际并发；
/// 超过后驱逐旧键，不会无界增长。
const HIGHLIGHT_CACHE_CAPACITY: usize = 8;

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

    /// 主动失效渲染缓存（`PreferencesChanged` 时调用）。
    ///
    /// 高亮缓存同步失效：虽然它的键带版本号（本不必手动失效），
    /// 但显式清一次可以让「切主题 / 关文档」后立刻释放 span 占用的内存。
    pub fn clear_cache(&self, cmd: ClearCacheCommand) {
        let mut cache = lock_or_recover(&self.cache);
        let mut hl = lock_or_recover(&self.highlight_cache);
        match cmd.doc_id {
            Some(id) => {
                cache.invalidate(id);
                hl.invalidate(id);
            }
            None => {
                cache.invalidate_all();
                hl.invalidate_all();
            }
        }
    }

    /// 渲染预览：返回块的 DTO 视图。
    ///
    /// 流程：取内容 + 版本 → 超大源直接返回 `truncated` → 缓存命中返回 → 解析并缓存。
    /// `version` 与注册表版本不一致时（内容已变）强制重解析。
    pub fn render_preview(&self, cmd: RenderPreviewQuery) -> AppResult<RenderedView> {
        let (content, revision) = self.fetch(cmd.doc_id)?;
        if content.len() > TRUNCATE_THRESHOLD_BYTES {
            // 大文件降级：不解析、不缓存，直接返回截断标记。
            return Ok(RenderedView {
                blocks: Vec::new(),
                version: revision,
                truncated: true,
            });
        }
        let blocks = self.parse_or_cache(cmd.doc_id, &content, revision)?;
        let blocks = blocks.iter().map(to_block_view).collect();
        Ok(RenderedView {
            blocks,
            version: revision,
            truncated: false,
        })
    }

    /// 语法高亮：返回片段 DTO 视图。
    ///
    /// 编辑区**每帧**都会请求全文 span（FR-03），而 syntect 解析是 O(全文)——
    /// 实测 10KB 文档一次 **56.5ms**（`docs/metrics.md`）。文档未变的帧
    /// （滚动、空闲、切换面板）本不该付这份钱，故按 `(文档, 版本, 语言, 区间)`
    /// 缓存最近一次结果：版本一变即失效，语义与「重算」完全一致。
    ///
    /// 超过 [`TRUNCATE_THRESHOLD_BYTES`] 的源直接返回空片段：表现层的 mapper
    /// 在同等阈值上已降级为纯文本排版（FR-06），此处若仍解析全文，
    /// 等于每帧付一份**无人使用**的成本（1MB 文档会直接卡死）。
    ///
    /// **防抖（OPT-011 ①）**：文档版本刚变的那 `policy.interval_ms` 毫秒内
    /// **直接返回空片段**，不进高亮器。理由与代价见 [`HighlightPace`]：
    /// 10KB 文档一次 syntect 全文解析 56–58ms（归因见 `docs/metrics.md` §3.2.1，
    /// 97.4% 在解析本身），每击一次键就付一次会把编辑帧推到 67.9ms；
    /// 改为「打字期不上色、停手后一次性上色」后，打字期这一项降到 ≈0，
    /// 代价是连续打字时语法色暂时消失（停手 150ms 后立刻恢复）。
    pub fn highlight(&self, cmd: HighlightQuery) -> AppResult<Vec<SpanView>> {
        let (content, revision) = self.fetch(cmd.doc_id)?;
        if content.len() > TRUNCATE_THRESHOLD_BYTES {
            return Ok(Vec::new());
        }
        let range = (cmd.range.start().value(), cmd.range.end().value());
        // 防抖：未静止则不查缓存也不重算（注意缓存键带版本号，跳过它不会读到旧 span）。
        let settled = lock_or_recover(&self.highlight_pace).settled(
            cmd.doc_id,
            revision,
            self.clock.now(),
            self.policy.interval_ms(),
        );
        if !settled {
            return Ok(Vec::new());
        }
        if let Some(spans) = lock_or_recover(&self.highlight_cache).get(
            cmd.doc_id,
            revision,
            cmd.lang.as_deref(),
            range,
        ) {
            return Ok(spans.clone());
        }
        let spans = self
            .highlighter
            .highlight_range(&content, cmd.range, cmd.lang.as_deref())
            .map_err(AppError::from)?;
        let spans: Vec<SpanView> = spans
            .iter()
            .map(|span| SpanView {
                range: RangeView::new(span.range().start().value(), span.range().end().value()),
                style: span_style_str(span.style()),
            })
            .collect();
        lock_or_recover(&self.highlight_cache).insert(
            cmd.doc_id,
            revision,
            cmd.lang.clone(),
            range,
            spans.clone(),
        );
        Ok(spans)
    }

    /// 高亮缓存的命中 / 未命中计数（测试与调试面板用）。
    pub fn highlight_cache_stats(&self) -> (u64, u64) {
        let c = lock_or_recover(&self.highlight_cache);
        (c.hits, c.misses)
    }

    /// 高亮一段独立文本（预览区的代码块）。
    ///
    /// 走的是与编辑区同一个 `SyntaxHighlighter`，故「预览里的代码配色」
    /// 与「编辑器里的代码配色」天然一致——两套解析器会立刻产生色差。
    pub fn highlight_snippet(&self, cmd: HighlightSnippetQuery) -> AppResult<Vec<SpanView>> {
        let spans = self
            .highlighter
            .highlight(&cmd.text, cmd.lang.as_deref())
            .map_err(AppError::from)?;
        Ok(spans
            .iter()
            .map(|span| SpanView {
                range: RangeView::new(span.range().start().value(), span.range().end().value()),
                style: span_style_str(span.style()),
            })
            .collect())
    }

    /// 滚动同步：编辑区偏移 ↔ 预览块，双向映射。
    pub fn map_scroll(&self, cmd: MapScrollQuery) -> AppResult<ScrollTarget> {
        let (content, revision) = self.fetch(cmd.doc_id)?;
        // 超大文档不解析，直接给出默认目标（UI 此时已降级为纯文本预览）。
        if content.len() > TRUNCATE_THRESHOLD_BYTES {
            return Ok(match cmd.source {
                ScrollSource::Editor { .. } => ScrollTarget::Preview {
                    block_id: 0,
                    ratio: 0.0,
                },
                ScrollSource::Preview { .. } => ScrollTarget::Editor { offset: 0 },
            });
        }
        let blocks = self.parse_or_cache(cmd.doc_id, &content, revision)?;
        match cmd.source {
            ScrollSource::Editor { offset } => {
                match BlockMapper::offset_to_block(&blocks, ByteOffset::new(offset)) {
                    Some((id, ratio)) => Ok(ScrollTarget::Preview {
                        block_id: id.as_u64(),
                        ratio,
                    }),
                    None => Ok(ScrollTarget::Preview {
                        block_id: 0,
                        ratio: 0.0,
                    }),
                }
            }
            ScrollSource::Preview { block_id } => {
                let id = BlockId::from_u64(block_id);
                match blocks.iter().find(|block| block.id() == id) {
                    Some(block) => Ok(ScrollTarget::Editor {
                        offset: block.source_range().start().value(),
                    }),
                    None => Ok(ScrollTarget::Editor { offset: 0 }),
                }
            }
        }
    }

    /// 重排大纲：由当前内容重建标题大纲。
    pub fn rebuild_outline(&self, cmd: RebuildOutlineQuery) -> AppResult<OutlineView> {
        let (content, revision) = self.fetch(cmd.doc_id)?;
        let blocks = self.parse_or_cache(cmd.doc_id, &content, revision)?;
        let items = BlockMapper::build_outline(&blocks, &content);
        Ok(OutlineView(
            items
                .iter()
                .map(|item| OutlineItemView {
                    level: item.level(),
                    text: item.text().to_string(),
                    line: item.line(),
                })
                .collect(),
        ))
    }

    /// 拼写检查：返回错词**字节区间**列表（`(start, end)`，按起点升序）。
    ///
    /// 与 [`Self::highlight`] 的三处刻意差异：
    ///
    /// 1. **不报错**：词典缺失 / 检查失败一律降级为空集。拼写是辅助提示，
    ///    让它把编辑器卡住或弹错是本末倒置。
    /// 2. **不防抖**：防抖是为 syntect 那种「停手后才算」的高亮设计的；
    ///    拼写检查本身已是 O(词长) 的哈希查找（实测 900KB / 94ms 在 debug，
    ///    release 下远低于此），且**开着就应立刻反映最新输入**，延迟显示反而像 bug。
    ///    版本号缓存已保证未变的帧不重复付费。
    /// 3. **大文件降级**：超过 [`TRUNCATE_THRESHOLD_BYTES`] 直接返回空集——
    ///    与高亮 / 预览同策略，保证大文档下编辑帧不因可选功能退化。
    pub fn spell_check(&self, q: SpellCheckQuery) -> Vec<(usize, usize)> {
        let Some(checker) = self.spell.as_ref() else {
            return Vec::new();
        };
        if !q.enabled || !checker.is_available() {
            return Vec::new();
        }
        let Ok((content, revision)) = self.fetch(q.doc_id) else {
            return Vec::new();
        };
        if content.len() > TRUNCATE_THRESHOLD_BYTES {
            return Vec::new();
        }
        if let Some(hits) = lock_or_recover(&self.spell_cache).get(q.doc_id, revision) {
            return hits;
        }
        let Ok(marks) = checker.check(&content) else {
            // 检查失败按「无错词」处理：宁可漏标，不可弹错。
            return Vec::new();
        };
        let hits: Vec<(usize, usize)> = marks
            .iter()
            .map(|m| (m.range.start().value(), m.range.end().value()))
            .collect();
        lock_or_recover(&self.spell_cache).insert(q.doc_id, revision, hits.clone());
        hits
    }

    /// 格式检查（V11-08）：返回当前文档里的全部格式问题。
    ///
    /// # 为什么不做缓存
    ///
    /// lint 是**线性扫描**（O(行数)），1MB 文档在 debug 下也就几十毫秒量级；
    /// 而「检查」标签不是每帧可见的——只有用户切过去才会调。
    /// 为它引入一份带失效规则的缓存，收益不抵复杂度（缓存一旦失效不彻底，
    /// 用户就会看到「改完了问题还在」）。
    ///
    /// # 边界
    ///
    /// - 规则全关 → 空集（领域层直接短路，连扫描都不做）；
    /// - 文档不存在 → 空集；
    /// - 大文档**不降级**：与高亮 / 预览不同，lint 不走渲染路径，
    ///   且「检查不出来」比「慢一点」更让人困惑。
    pub fn lint_document(&self, q: LintQuery) -> Vec<LintIssueView> {
        let rules = lint_rule_set(&q.rules);
        if rules.enabled().is_empty() {
            return Vec::new();
        }
        let Ok((content, _revision)) = self.fetch(q.doc_id) else {
            return Vec::new();
        };
        MarkdownLinter::lint(&content, &rules)
            .into_iter()
            .map(|issue| LintIssueView {
                rule: issue.rule().id().to_string(),
                rule_label: issue.rule().label().to_string(),
                line: issue.line(),
                column: issue.column(),
                message: issue.message().to_string(),
            })
            .collect()
    }

    /// 拼写建议（V11-07）：给 `offset` 处的那个词返回候选替换。
    ///
    /// # 为什么只按 `offset` 定位，不接受「词」本身
    ///
    /// 表现层手里只有**字节偏移**（egui 的排版结果给不出词），
    /// 若把「扩到整词」的责任推给调用方，就会有两份分词规则
    /// （一份在视图、一份在检查器），改了一处必然漏另一处。
    /// 这里复用与检查器同源的「ASCII 字母即词」约定，保持单一真相。
    ///
    /// # 边界
    ///
    /// - `offset` 落在非字母（空白 / 中文 / 标点）上 → 空集（没有词可建议）；
    /// - 该词**未被判错**（在词表或找不到近邻）→ 空集；
    /// - 检查器不可用 / 未装配 → 空集。
    ///
    /// 不开拼写检查时也照常返回建议：用户既然右键了，说明他看到了波浪线
    /// 或觉得这个词可疑，此时拒绝回答没有意义。
    pub fn spell_suggestions(&self, q: SpellSuggestQuery) -> Vec<String> {
        let Some(checker) = self.spell.as_ref() else {
            return Vec::new();
        };
        if !checker.is_available() || q.limit == 0 {
            return Vec::new();
        }
        let Ok((content, _revision)) = self.fetch(q.doc_id) else {
            return Vec::new();
        };
        let Some((start, end)) = word_bounds_at(&content, q.offset) else {
            return Vec::new();
        };
        let word = content[start..end].to_ascii_lowercase();
        checker.suggest(&word, q.limit)
    }

    // ---------- 内部 ----------

    fn fetch(&self, doc_id: DocumentId) -> AppResult<(String, u64)> {
        self.provider
            .content_and_revision(doc_id)
            .ok_or_else(|| AppError::domain("文档未打开"))
    }

    /// 取缓存命中结果；未命中时解析并写入缓存（返回克隆，便于并发读取）。
    fn parse_or_cache(
        &self,
        doc_id: DocumentId,
        content: &str,
        revision: u64,
    ) -> AppResult<Vec<RenderedBlock>> {
        {
            let cache = self
                .cache
                .lock()
                .map_err(|_| AppError::io("渲染缓存锁已损坏"))?;
            if let Some(blocks) = cache.get(doc_id, revision) {
                return Ok(blocks.clone());
            }
        }
        let blocks = self.parser.parse(content).map_err(AppError::from)?;
        self.cache
            .lock()
            .map_err(|_| AppError::io("渲染缓存锁已损坏"))?
            .insert(doc_id, revision, blocks.clone());
        Ok(blocks)
    }
}

/// 渲染块 → DTO 视图。
fn to_block_view(block: &RenderedBlock) -> BlockView {
    let kind = block.kind();
    BlockView {
        id: block.id().as_u64(),
        kind: block_kind_str(kind),
        content: block.content().to_string(),
        level: kind.heading_level(),
        lang: kind.language().map(|l| l.to_string()),
        ordered: matches!(kind, BlockKind::List { ordered: true, .. }),
        items: kind.list_items().map_or_else(Vec::new, |items| {
            items
                .iter()
                .map(|item| ListItemView {
                    text: item.text().to_string(),
                    checked: item.checked(),
                    start: item.source_range().start().value(),
                    end: item.source_range().end().value(),
                    depth: item.depth(),
                    ordinal: item.ordinal(),
                    spans: spans_to_view(item.spans()),
                })
                .collect()
        }),
        table: kind.table().map(|data| TableView {
            header: data.header().to_vec(),
            rows: data.rows().to_vec(),
            aligns: data
                .aligns()
                .iter()
                .map(|a| match a {
                    CellAlignment::None => CellAlignView::None,
                    CellAlignment::Left => CellAlignView::Left,
                    CellAlignment::Center => CellAlignView::Center,
                    CellAlignment::Right => CellAlignView::Right,
                })
                .collect(),
            // 片段表要**按列补齐到与表体等长**：源文里某些行可能少写单元格
            // （`| a | b |` 后面一行只写 `| c |`），此时解析器只收集到 1 组
            // 片段而 `rows` 仍是 2 行。渲染层按 `(r, c)` 索引时若越界就得
            // 处处判空，补齐在这里一次性做掉。
            header_spans: (0..data.header().len())
                .map(|c| spans_to_view(data.header_spans(c)))
                .collect(),
            rows_spans: (0..data.rows().len())
                .map(|r| {
                    (0..data.columns())
                        .map(|c| spans_to_view(data.cell_spans(r, c)))
                        .collect()
                })
                .collect(),
        }),
        alt: match kind {
            BlockKind::Image { alt, .. } => Some(alt.clone()),
            _ => None,
        },
        url: match kind {
            BlockKind::Image { url, .. } => Some(url.clone()),
            _ => None,
        },
        marker: match kind {
            BlockKind::Raw { marker } => Some(marker.clone()),
            _ => None,
        },
        display: match kind {
            BlockKind::Math { display } => Some(*display),
            _ => None,
        },
        diagram: match kind {
            BlockKind::Diagram { kind } => Some(kind.clone()),
            _ => None,
        },
        label: kind.footnote_label().map(str::to_string),
        definitions: kind.definitions().map_or_else(Vec::new, |entries| {
            entries
                .iter()
                .map(|entry| DefinitionEntryView {
                    term: entry.term().to_string(),
                    term_spans: spans_to_view(entry.term_spans()),
                    definitions: entry
                        .definitions()
                        .iter()
                        .map(|body| DefinitionBodyView {
                            text: body.text().to_string(),
                            spans: spans_to_view(body.spans()),
                        })
                        .collect(),
                })
                .collect()
        }),
        spans: spans_to_view(block.spans()),
    }
}

/// 领域行内片段 → DTO 片段（区间 + 样式字符串）。
///
/// 单独提出来是因为**五处**都要做同一件事（块 / 列表项 / 定义术语 / 定义正文 /
/// 表格单元格）。各自内联一遍 `map(...)` 的话，将来往 `SpanView` 加字段
/// （比如「是否可点击」）就会漏改其中几处，且漏改后仍能编译。
fn spans_to_view(spans: &[InlineSpan]) -> Vec<SpanView> {
    spans
        .iter()
        .map(|span| SpanView {
            range: RangeView::new(span.start(), span.end()),
            style: inline_style_str(span.style()).to_string(),
        })
        .collect()
}

/// 行内样式 → 稳定字符串（供表现层分发渲染器）。
fn inline_style_str(style: InlineStyle) -> &'static str {
    match style {
        InlineStyle::Strong => "strong",
        InlineStyle::Emphasis => "emphasis",
        InlineStyle::Strikethrough => "strikethrough",
        InlineStyle::Code => "code",
        InlineStyle::Link => "link",
        InlineStyle::Math => "math",
        InlineStyle::Keycap => "keycap",
        InlineStyle::Mark => "mark",
        InlineStyle::Subscript => "sub",
        InlineStyle::Superscript => "sup",
        InlineStyle::FootnoteRef => "footnote_ref",
    }
}

/// 块类型 → 稳定字符串（供表现层分发渲染器）。
fn block_kind_str(kind: &BlockKind) -> String {
    match kind {
        BlockKind::Heading { .. } => "heading".to_string(),
        BlockKind::Paragraph => "paragraph".to_string(),
        BlockKind::CodeBlock { .. } => "code_block".to_string(),
        BlockKind::Table { .. } => "table".to_string(),
        // 任务列表与普通列表共用渲染器（按 `items[].checked` 有无区分），
        // 故只输出 `list` 会让任务列表丢掉「可勾选」语义。
        BlockKind::List { .. } => "list".to_string(),
        BlockKind::TaskList { .. } => "task_list".to_string(),
        BlockKind::Quote => "quote".to_string(),
        BlockKind::Image { .. } => "image".to_string(),
        BlockKind::Rule => "rule".to_string(),
        // 标签（`label`）与正文分开展示，但**共用同一个 kind 字符串**：
        // 表现层只需要一个渲染器，标签从 `BlockView::label` 读。
        BlockKind::Footnote { .. } => "footnote".to_string(),
        // 定义列表（`术语` / `: 定义`）——与普通列表渲染形态不同（无项目符号、
        // 定义要缩进在术语下方），故单列一种 kind。
        BlockKind::DefinitionList { .. } => "definition_list".to_string(),
        // 行内 / 块级公式共用 `math` 类型，由 `BlockView::display` 区分呈现。
        BlockKind::Math { .. } => "math".to_string(),
        BlockKind::Diagram { .. } => "diagram".to_string(),
        // 目录：`@[TOC](标题)` / `[TOC]`，预览里渲染成可点击的标题列表。
        BlockKind::Toc { .. } => "toc".to_string(),
        BlockKind::Raw { marker } => format!("raw:{marker}"),
    }
}

/// 语法样式 → 稳定字符串（供表现层取色）。
fn span_style_str(style: SpanStyle) -> String {
    match style {
        SpanStyle::Plain => "plain".to_string(),
        SpanStyle::Heading { .. } => "heading".to_string(),
        SpanStyle::Emphasis => "emphasis".to_string(),
        SpanStyle::Strong => "strong".to_string(),
        SpanStyle::Strikethrough => "strikethrough".to_string(),
        SpanStyle::Code => "code".to_string(),
        SpanStyle::Link => "link".to_string(),
        SpanStyle::Image => "image".to_string(),
        SpanStyle::Quote => "quote".to_string(),
        SpanStyle::ListMarker => "list_marker".to_string(),
        SpanStyle::TableBorder => "table_border".to_string(),
        SpanStyle::TaskMarker { done } => format!("task_marker:{}", done),
        SpanStyle::Keyword => "keyword".to_string(),
        SpanStyle::String => "string".to_string(),
        SpanStyle::Comment => "comment".to_string(),
        SpanStyle::Number => "number".to_string(),
        SpanStyle::Function => "function".to_string(),
        SpanStyle::Type => "type".to_string(),
        SpanStyle::Variable => "variable".to_string(),
        SpanStyle::Operator => "operator".to_string(),
    }
}

/// 编译期断言：`MarkdownAppService` 可跨线程共享（缓存与 trait 对象均为 `Send + Sync`）。
const fn _assert_send_sync<T: Send + Sync>() {}
const _ASSERT_MD_SERVICE: () = _assert_send_sync::<MarkdownAppService>();
