//! 应用层测试替身（阶段 04 · S4-11）。
//!
//! 为阶段 05 尚未就绪的外部依赖提供 fake，全部与真实实现**共享同一 trait**
//! （编译期保证接口一致）。本文件在 `tests/` 下，不进入 `release` 产物。

// 测试支撑代码：为后续阶段与更多用例预留的完整 fake API，未被当前用例全部引用属正常。
//
// `duplicate_mod`：本文件自带一个 `mod support`（供自身使用），若引用它的测试
// target 也声明了自己的 `mod support`，同一文件路径会被加载两次。两份实例互不干涉
// （各自在独立模块树下），但 clippy 会报重复模块——对测试支撑代码属可接受代价。
#![allow(dead_code, clippy::type_complexity, clippy::duplicate_mod)]

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use rustmark::domain::document::repository::{DocumentRepository, DocumentSnapshot};
use rustmark::domain::document::services::BufferFactory;
use rustmark::domain::export::aggregate::ExportJob;
use rustmark::domain::export::errors::ExportError;
use rustmark::domain::export::services::Exporter;
use rustmark::domain::markdown::services::{MarkdownParser, SpellChecker, SyntaxHighlighter};
use rustmark::domain::markdown::value_objects::{
    BlockId, BlockKind, Misspelling, RenderedBlock, SpanStyle, SyntaxSpan,
};
use rustmark::domain::preferences::aggregate::Preferences;
use rustmark::domain::preferences::repository::PreferencesRepository;
use rustmark::domain::workspace::repository::WorkspaceRepository;
use rustmark::domain::workspace::value_objects::{RecentFile, WorkspaceEntry};
use rustmark::ports::event::{DomainEvent, EventBus, EventHandler};
use rustmark::ports::time::{Clock, Timestamp};
use rustmark::shared::error::AppError;
use rustmark::shared::geometry::{ByteOffset, TextRange};
use rustmark::shared::ids::DocumentId;
use rustmark::shared::path::FilePath;

#[path = "../support.rs"]
mod support;

use support::text_buffer::StringTextBuffer;

/// 可注入的假时钟（时间可手动设置，用于防抖等时间相关测试）。
pub struct FakeClock {
    millis: Mutex<u64>,
}

impl FakeClock {
    /// 构造假时钟，初始时间为给定毫秒。
    pub fn new(millis: u64) -> Self {
        Self {
            millis: Mutex::new(millis),
        }
    }

    /// 设置当前时间（毫秒）。
    pub fn set(&self, millis: u64) {
        *self.millis.lock().unwrap() = millis;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(*self.millis.lock().unwrap())
    }
}

/// 假缓冲区工厂：用朴素 `String` 缓冲区承载文档内容。
pub struct FakeBufferFactory;

impl BufferFactory for FakeBufferFactory {
    fn create(&self, content: &str) -> rustmark::domain::document::aggregate::BufferHandle {
        Box::new(StringTextBuffer::from_str(content))
    }
}

/// 内存文档仓储：按路径保存快照，ID 在同一路径上保持稳定。
pub struct InMemoryDocumentRepository {
    files: Mutex<HashMap<FilePath, DocumentSnapshot>>,
}

impl InMemoryDocumentRepository {
    /// 构造空仓储。
    pub fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
        }
    }

    /// 预置一个文件（供 `load` 命中）。
    pub fn seed(&self, path: FilePath, content: &str, saved_at: Option<Timestamp>) {
        let id = DocumentId::new();
        self.files.lock().unwrap().insert(
            path.clone(),
            DocumentSnapshot::new(id, path, content, saved_at),
        );
    }

    /// 已保存文件数量。
    pub fn len(&self) -> usize {
        self.files.lock().unwrap().len()
    }
}

impl Default for InMemoryDocumentRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentRepository for InMemoryDocumentRepository {
    fn load(&self, path: &FilePath) -> Result<DocumentSnapshot, AppError> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| AppError::io(format!("文件不存在：{path}")))
    }

    fn save(&self, snapshot: &DocumentSnapshot) -> Result<(), AppError> {
        self.files
            .lock()
            .unwrap()
            .insert(snapshot.path.clone(), snapshot.clone());
        Ok(())
    }

    fn exists(&self, path: &FilePath) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    fn delete(&self, path: &FilePath) -> Result<(), AppError> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }
}

/// 总是保存失败的文档仓储：用于验证「保存失败**不**清除脏标记」的失败语义。
///
/// `load` 仍成功（以便正常打开文件），但 `save` 始终返回 `I/O` 错误。
pub struct FailingDocumentRepository {
    files: Mutex<HashMap<FilePath, DocumentSnapshot>>,
}

impl FailingDocumentRepository {
    /// 构造空仓储。
    pub fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
        }
    }

    /// 预置一个文件（供 `load` 命中）。
    pub fn seed(&self, path: FilePath, content: &str, saved_at: Option<Timestamp>) {
        let id = DocumentId::new();
        self.files.lock().unwrap().insert(
            path.clone(),
            DocumentSnapshot::new(id, path, content, saved_at),
        );
    }
}

impl Default for FailingDocumentRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentRepository for FailingDocumentRepository {
    fn load(&self, path: &FilePath) -> Result<DocumentSnapshot, AppError> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| AppError::io(format!("文件不存在：{path}")))
    }

    fn save(&self, _snapshot: &DocumentSnapshot) -> Result<(), AppError> {
        Err(AppError::io("模拟保存失败"))
    }

    fn exists(&self, path: &FilePath) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    fn delete(&self, path: &FilePath) -> Result<(), AppError> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }
}

/// 内存工作区仓储：文件树、最近文件与磁盘内容三者在内存中模拟。
pub struct InMemoryWorkspaceRepository {
    entries: Mutex<Vec<WorkspaceEntry>>,
    recent: Mutex<Vec<RecentFile>>,
    files: Mutex<HashMap<FilePath, String>>,
    /// 目录 → 其直接子条目路径（`create_directory` / `rename` 维护）。
    dirs: Mutex<HashMap<FilePath, Vec<FilePath>>>,
    /// 强制下一次写操作失败（验证「落盘失败时聚合不应改变」）。
    fail_writes: Mutex<bool>,
}

impl InMemoryWorkspaceRepository {
    /// 构造空仓储。
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            recent: Mutex::new(Vec::new()),
            files: Mutex::new(HashMap::new()),
            dirs: Mutex::new(HashMap::new()),
            fail_writes: Mutex::new(false),
        }
    }

    /// 预置一个文件树条目。
    pub fn seed_entry(&self, entry: WorkspaceEntry) {
        self.entries.lock().unwrap().push(entry);
    }

    /// 预置一个磁盘文件内容（供 `read_content` 命中）。
    pub fn seed_file(&self, path: FilePath, content: &str) {
        self.files.lock().unwrap().insert(path, content.to_string());
    }

    /// 已扫描条目数量。
    pub fn entry_count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    /// 让此后所有写操作失败（不改变已写入的内容）。
    pub fn set_fail_writes(&self, fail: bool) {
        *self.fail_writes.lock().unwrap() = fail;
    }

    /// 某路径是否已作为文件存在。
    pub fn has_file(&self, path: &FilePath) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    /// 某路径是否已作为目录存在。
    pub fn has_dir(&self, path: &FilePath) -> bool {
        self.dirs.lock().unwrap().contains_key(path)
    }

    /// 写入被拒绝时构造的错误。
    fn write_guard(&self) -> Result<(), AppError> {
        if *self.fail_writes.lock().unwrap() {
            Err(AppError::io("模拟写入失败"))
        } else {
            Ok(())
        }
    }
}

impl Default for InMemoryWorkspaceRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceRepository for InMemoryWorkspaceRepository {
    fn scan(&self, _root: &FilePath, _max_depth: usize) -> Result<Vec<WorkspaceEntry>, AppError> {
        Ok(self.entries.lock().unwrap().clone())
    }

    fn load_recent(&self) -> Result<Vec<RecentFile>, AppError> {
        Ok(self.recent.lock().unwrap().clone())
    }

    fn save_recent(&self, items: &[RecentFile]) -> Result<(), AppError> {
        *self.recent.lock().unwrap() = items.to_vec();
        Ok(())
    }

    fn read_content(&self, path: &FilePath) -> Result<String, AppError> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| AppError::io(format!("无法读取：{path}")))
    }

    /// 与真实实现同规则：同名即失败，**绝不覆盖**已有文件。
    ///
    /// 内存替身若在这里宽松（「直接 insert」），用例就测不出
    /// 「新建撞上同名文件会清空用户内容」这类缺陷——替身比真实实现
    /// 宽松，测出来的绿是假的。
    fn create_file(&self, path: &FilePath) -> Result<(), AppError> {
        self.write_guard()?;
        if self.files.lock().unwrap().contains_key(path) {
            return Err(AppError::io(format!("已存在同名文件：{path}")));
        }
        self.files
            .lock()
            .unwrap()
            .insert(path.clone(), String::new());
        Ok(())
    }

    fn create_directory(&self, path: &FilePath) -> Result<(), AppError> {
        self.write_guard()?;
        self.dirs.lock().unwrap().entry(path.clone()).or_default();
        Ok(())
    }

    /// 同名即失败（与真实实现的显式存在性检查一致）。
    fn rename(&self, from: &FilePath, to: &FilePath) -> Result<(), AppError> {
        self.write_guard()?;
        if self.files.lock().unwrap().contains_key(to) || self.dirs.lock().unwrap().contains_key(to)
        {
            return Err(AppError::io(format!("已存在同名条目：{to}")));
        }
        if let Some(content) = self.files.lock().unwrap().remove(from) {
            self.files.lock().unwrap().insert(to.clone(), content);
            return Ok(());
        }
        if self.dirs.lock().unwrap().remove(from).is_some() {
            self.dirs.lock().unwrap().insert(to.clone(), Vec::new());
            return Ok(());
        }
        Err(AppError::io(format!("源不存在：{from}")))
    }

    /// 目录非空即失败（与真实实现的 `remove_dir` 语义一致）。
    fn remove(&self, path: &FilePath) -> Result<(), AppError> {
        self.write_guard()?;
        let children = self
            .dirs
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .unwrap_or_default();
        if self.dirs.lock().unwrap().contains_key(path) {
            if !children.is_empty() {
                return Err(AppError::io(format!("目录非空：{path}")));
            }
            self.dirs.lock().unwrap().remove(path);
            return Ok(());
        }
        match self.files.lock().unwrap().remove(path) {
            Some(_) => Ok(()),
            None => Err(AppError::io(format!("不存在：{path}"))),
        }
    }
}

/// 内存偏好设置仓储：持久化为内存中的单一 `Preferences`。
pub struct InMemoryPreferencesRepository {
    stored: Mutex<Option<Preferences>>,
}

impl InMemoryPreferencesRepository {
    /// 构造空仓储（未写入任何配置）。
    pub fn new() -> Self {
        Self {
            stored: Mutex::new(None),
        }
    }

    /// 是否已有持久化配置。
    pub fn is_persisted(&self) -> bool {
        self.stored.lock().unwrap().is_some()
    }
}

impl Default for InMemoryPreferencesRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl PreferencesRepository for InMemoryPreferencesRepository {
    fn load(&self) -> Result<Preferences, AppError> {
        match self.stored.lock().unwrap().clone() {
            Some(prefs) => Ok(prefs),
            None => Ok(Preferences::default_preferences()),
        }
    }

    fn save(&self, prefs: &Preferences) -> Result<(), AppError> {
        *self.stored.lock().unwrap() = Some(prefs.clone());
        Ok(())
    }

    fn reset(&self) -> Result<Preferences, AppError> {
        let prefs = Preferences::default_preferences();
        *self.stored.lock().unwrap() = Some(prefs.clone());
        Ok(prefs)
    }
}

/// 假 Markdown 解析器：返回预设块，并记录调用次数（用于缓存命中断言）。
pub struct FakeMarkdownParser {
    blocks: Mutex<Vec<RenderedBlock>>,
    call_count: Mutex<usize>,
}

impl FakeMarkdownParser {
    /// 构造解析器，返回固定的块序列。
    pub fn new(blocks: Vec<RenderedBlock>) -> Self {
        Self {
            blocks: Mutex::new(blocks),
            call_count: Mutex::new(0),
        }
    }

    /// 重新设定返回的块序列。
    pub fn set_blocks(&self, blocks: Vec<RenderedBlock>) {
        *self.blocks.lock().unwrap() = blocks;
    }

    /// 解析调用次数（用于断言「缓存命中不重新解析」）。
    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

impl MarkdownParser for FakeMarkdownParser {
    fn parse(
        &self,
        _text: &str,
    ) -> Result<Vec<RenderedBlock>, rustmark::domain::markdown::errors::ParseError> {
        *self.call_count.lock().unwrap() += 1;
        Ok(self.blocks.lock().unwrap().clone())
    }
}

/// 假语法高亮器：返回预设片段，并记录调用。
pub struct FakeHighlighter {
    spans: Mutex<Vec<SyntaxSpan>>,
    call_count: Mutex<usize>,
}

impl FakeHighlighter {
    /// 构造高亮器，返回固定的片段序列。
    pub fn new(spans: Vec<SyntaxSpan>) -> Self {
        Self {
            spans: Mutex::new(spans),
            call_count: Mutex::new(0),
        }
    }

    /// 高亮调用次数。
    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }
}

impl SyntaxHighlighter for FakeHighlighter {
    fn highlight(
        &self,
        _text: &str,
        _lang: Option<&str>,
    ) -> Result<Vec<SyntaxSpan>, rustmark::domain::markdown::errors::HighlightError> {
        *self.call_count.lock().unwrap() += 1;
        Ok(self.spans.lock().unwrap().clone())
    }

    fn highlight_range(
        &self,
        _text: &str,
        _range: TextRange,
        _lang: Option<&str>,
    ) -> Result<Vec<SyntaxSpan>, rustmark::domain::markdown::errors::HighlightError> {
        *self.call_count.lock().unwrap() += 1;
        Ok(self.spans.lock().unwrap().clone())
    }

    fn supports(&self, _lang: &str) -> bool {
        true
    }
}

/// 假导出器：可配置是否失败，并记录调用参数（含 `include_toc`）。
pub struct FakeExporter {
    should_fail: Mutex<bool>,
    calls: Mutex<Vec<ExportCall>>,
}

/// 一次导出调用的记录。
#[derive(Clone, Debug)]
pub struct ExportCall {
    /// 被导出的内容字节长度。
    pub content_len: usize,
    /// 目标格式。
    pub format: rustmark::domain::export::value_objects::ExportFormat,
    /// 是否要求目录（验证该选项被下传给 Exporter）。
    pub include_toc: bool,
}

impl FakeExporter {
    /// 构造导出器；`should_fail` 控制是否模拟导出失败。
    pub fn new(should_fail: bool) -> Self {
        Self {
            should_fail: Mutex::new(should_fail),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// 调用记录。
    pub fn calls(&self) -> Vec<ExportCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl Exporter for FakeExporter {
    fn export(&self, content: &str, job: &ExportJob) -> Result<usize, ExportError> {
        self.calls.lock().unwrap().push(ExportCall {
            content_len: content.len(),
            format: job.format(),
            include_toc: job.options().include_toc(),
        });
        if *self.should_fail.lock().unwrap() {
            Err(ExportError::io_unavailable("模拟导出失败"))
        } else {
            // 假导出器不写盘，但「返回产出大小」这条契约要遵守：
            // 用 `content.len()` 当大小，让断言 `bytes` 的用例仍能跑通。
            Ok(content.len())
        }
    }
}

/// 内存事件总线：记录全部已发布事件、收集订阅者错误，供测试断言。
pub struct InMemoryEventBus {
    handlers: Mutex<Vec<EventHandler>>,
    published: Mutex<Vec<DomainEvent>>,
    error_count: Mutex<usize>,
}

impl InMemoryEventBus {
    /// 构造空总线。
    pub fn new() -> Self {
        Self {
            handlers: Mutex::new(Vec::new()),
            published: Mutex::new(Vec::new()),
            error_count: Mutex::new(0),
        }
    }

    /// 全部已发布事件（按时间顺序）。
    pub fn published(&self) -> Vec<DomainEvent> {
        self.published.lock().unwrap().clone()
    }

    /// 订阅者返回错误的累计次数。
    pub fn error_count(&self) -> usize {
        *self.error_count.lock().unwrap()
    }
}

impl Default for InMemoryEventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus for InMemoryEventBus {
    fn publish(&self, event: DomainEvent) -> Vec<AppError> {
        self.published.lock().unwrap().push(event);
        let handlers = self.handlers.lock().unwrap().clone();
        let mut errors = Vec::new();
        for handler in handlers {
            if let Err(e) = handler(&event) {
                errors.push(e);
                *self.error_count.lock().unwrap() += 1;
            }
        }
        errors
    }

    fn subscribe(&self, handler: EventHandler) {
        self.handlers.lock().unwrap().push(handler);
    }
}

/// 假只读内容提供者：为 Markdown / 导出等服务的单元测试提供确定性的文档内容。
pub struct FakeDocumentContentProvider {
    docs: Mutex<HashMap<DocumentId, (String, u64, Option<FilePath>)>>,
    /// `content_and_revision`（= 全文快照）被调用的次数。
    ///
    /// 用于钉死「编辑区的每帧路径不得快照全文」（问题 046）：缓存命中、
    /// 防抖期、大文件降级三条路径都只需要版本号与字节长度。
    snapshots: AtomicUsize,
    /// `revision_and_len`（= O(1) 元数据）被调用的次数。
    revision_queries: AtomicUsize,
}

impl FakeDocumentContentProvider {
    /// 构造空提供者。
    pub fn new() -> Self {
        Self {
            docs: Mutex::new(HashMap::new()),
            snapshots: AtomicUsize::new(0),
            revision_queries: AtomicUsize::new(0),
        }
    }

    /// `content_and_revision`（全文快照）累计调用次数。
    pub fn snapshot_calls(&self) -> usize {
        self.snapshots.load(Ordering::Relaxed)
    }

    /// `revision_and_len`（O(1) 元数据）累计调用次数。
    pub fn revision_len_calls(&self) -> usize {
        self.revision_queries.load(Ordering::Relaxed)
    }

    /// 插入（或替换）一个文档的内容、版本与路径。
    pub fn insert(&self, id: DocumentId, content: &str, revision: u64, path: Option<FilePath>) {
        self.docs
            .lock()
            .unwrap()
            .insert(id, (content.to_string(), revision, path));
    }

    /// 文档数量。
    pub fn len(&self) -> usize {
        self.docs.lock().unwrap().len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.docs.lock().unwrap().is_empty()
    }
}

impl Default for FakeDocumentContentProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl rustmark::application::document::DocumentContentProvider for FakeDocumentContentProvider {
    fn content_and_revision(&self, id: DocumentId) -> Option<(String, u64)> {
        self.snapshots.fetch_add(1, Ordering::Relaxed);
        self.docs
            .lock()
            .unwrap()
            .get(&id)
            .map(|(content, revision, _)| (content.clone(), *revision))
    }

    /// 覆写为 O(1) 路径：只读元数据，**不克隆内容**，也不计入 [`Self::snapshot_calls`]。
    ///
    /// 语义与生产实现（`DocumentRegistry`）一致：长度取自存储的文本字节数。
    fn revision_and_len(&self, id: DocumentId) -> Option<(u64, usize)> {
        self.revision_queries.fetch_add(1, Ordering::Relaxed);
        self.docs
            .lock()
            .unwrap()
            .get(&id)
            .map(|(content, revision, _)| (*revision, content.len()))
    }

    fn path_of(&self, id: DocumentId) -> Option<FilePath> {
        self.docs
            .lock()
            .unwrap()
            .get(&id)
            .and_then(|(_, _, p)| p.clone())
    }

    fn open_documents(&self) -> Vec<(DocumentId, Option<FilePath>, String)> {
        self.docs
            .lock()
            .unwrap()
            .iter()
            .map(|(id, (content, _, path))| (*id, path.clone(), content.clone()))
            .collect()
    }
}

/// 便捷构造：生成一个渲染块（供 fake 解析器使用）。
pub fn block(id: u64, kind: BlockKind, start: usize, end: usize, content: &str) -> RenderedBlock {
    RenderedBlock::new(
        BlockId::from_u64(id),
        kind,
        TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
        content,
    )
}

/// 便捷构造：生成一个语法片段（供 fake 高亮器使用）。
pub fn span(start: usize, end: usize, style: SpanStyle) -> SyntaxSpan {
    SyntaxSpan::new(
        TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
        style,
    )
}

/// 拼写检查器替身：返回固定区间，并记录调用次数与可用性开关。
///
/// 之所以不用真词表做应用层测试：应用层要验的是「**编排**」——
/// 开关是否透传、缓存是否复用、失败是否降级——这些与词典内容无关。
/// 词典本身的行为由 `tests/infrastructure/spell_checker_test.rs` 负责。
pub struct FakeSpellChecker {
    hits: Mutex<Vec<Misspelling>>,
    call_count: Mutex<usize>,
    available: bool,
    /// 令 `check` 返回 `Err`，用于验证「检查失败应降级为空集而非报错」。
    fail: bool,
    /// `suggest` 的固定返回值（V11-07）。
    suggestions: Vec<String>,
    /// 最近一次 `suggest` 收到的词与上限（断言「应用层传下去的是什么」）。
    last_suggest: Mutex<Option<(String, usize)>>,
}

impl FakeSpellChecker {
    /// 构造返回固定区间的检查器。
    pub fn new(hits: Vec<Misspelling>) -> Self {
        Self {
            hits: Mutex::new(hits),
            call_count: Mutex::new(0),
            available: true,
            fail: false,
            suggestions: Vec::new(),
            last_suggest: Mutex::new(None),
        }
    }

    /// 构造一个「会给建议」的检查器（V11-07）。
    ///
    /// 建议与 `hits` 分开注入：应用层要验的是「把哪个词、以什么上限传下去」，
    /// 与「哪些区间被判错」是两件事。
    pub fn suggesting(hits: Vec<Misspelling>, suggestions: &[&str]) -> Self {
        Self {
            suggestions: suggestions.iter().map(|s| (*s).to_string()).collect(),
            ..Self::new(hits)
        }
    }

    /// 构造「词典不可用」的检查器。
    pub fn unavailable() -> Self {
        Self {
            hits: Mutex::new(Vec::new()),
            call_count: Mutex::new(0),
            available: false,
            fail: false,
            suggestions: Vec::new(),
            last_suggest: Mutex::new(None),
        }
    }

    /// 构造「检查会失败」的检查器。
    pub fn failing() -> Self {
        Self {
            hits: Mutex::new(Vec::new()),
            call_count: Mutex::new(0),
            available: true,
            fail: true,
            suggestions: Vec::new(),
            last_suggest: Mutex::new(None),
        }
    }

    /// `check` 调用次数（缓存断言用）。
    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }

    /// 最近一次 `suggest` 收到的 `(词, 上限)`；未调用过为 `None`。
    pub fn last_suggest(&self) -> Option<(String, usize)> {
        self.last_suggest.lock().unwrap().clone()
    }
}

impl SpellChecker for FakeSpellChecker {
    fn check(
        &self,
        _text: &str,
    ) -> Result<Vec<Misspelling>, rustmark::domain::markdown::errors::SpellCheckError> {
        *self.call_count.lock().unwrap() += 1;
        if self.fail {
            return Err(rustmark::domain::markdown::errors::SpellCheckError::new(
                "注入的失败",
            ));
        }
        Ok(self.hits.lock().unwrap().clone())
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        *self.last_suggest.lock().unwrap() = Some((word.to_string(), limit));
        self.suggestions.clone()
    }
}
