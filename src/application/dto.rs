//! 应用层 DTO（阶段 04 · S4-01）。
//!
//! 表现层**唯一可见**的数据结构。硬性边界：
//! - 不含任何聚合根类型（`Document` / `Workspace` / `Preferences` / `ExportJob`）；
//! - 字段均为可序列化标量或其他 DTO，不持有 `Arc<Mutex<>>` 之类的内部可变共享；
//! - 全部 `Clone + Debug + Serialize`；仅 [`SettingsView`] 需要 `Deserialize`
//!   （设置页面回写）。
//!
//! # 模块结构
//!
//! 按所属限界上下文拆为四个私有子模块，本文件作为 facade 全量 re-export，
//! 外部路径 `application::dto::*` 保持不变：
//! - `document`：区间 / 文档视图 / 统计 / 大纲 / 搜索命中；
//! - `preferences`：主题 / 编辑器设置 / lint / 写作目标 / 快捷键 / 设置总视图；
//! - `markdown`：预览块 / 表格 / 列表 / 定义列表 / 渲染结果 / 高亮片段；
//! - `workspace`：导出 / 最近文件 / 文件树 / 标签页 / 全局搜索 / 关闭决策 /
//!   快捷键冲突 / 格式指令结果。

mod document;
mod markdown;
mod preferences;
mod workspace;

pub use document::{
    DocumentView, OutlineItemView, OutlineView, RangeView, SearchHitView, StatsView, TabSummary,
};
pub use markdown::{
    BlockView, CellAlignView, DefinitionBodyView, DefinitionEntryView, ImageSpecView,
    InlineImageView, ListItemView, RenderedView, SpanView, TableView,
};
pub use preferences::{
    CustomThemePaletteView, EditorSettingsView, GoalProgressView, LintIssueView, LintRulesView,
    SettingsView, ShortcutView, ThemeView,
};
pub use workspace::{
    CloseDecisionView, ConflictView, ExportRequestView, ExportResultView, FileEntryView,
    FormatResultView, RecentFileView, SearchResultView, TabView,
};
