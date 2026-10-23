//! Export 上下文的领域服务：目录（TOC）构建与转义校验。
//!
//! 原型缺陷 2.4 的根治点：**TOC 拼接在清洗之前、且必须转义**。
//! 这里把「哪些标题需要转义」下沉为领域层规则，导出器（基础设施层）只负责消费标记。

use super::errors::ExportError;
use super::value_objects::{NEEDS_ESCAPE_CHARS, TocEntry};

/// 目录构建服务（无状态纯函数）。
pub struct TocBuilder;

impl TocBuilder {
    /// 由（级别, 标题）序列构建目录条目。
    ///
    /// 输入通常来自 `Markdown` 上下文的大纲；此处刻意只接受 `(u8, &str)` 二元组，
    /// 以免 `export` 反向依赖 `markdown` 的值对象。
    pub fn build(items: &[(u8, &str)]) -> Vec<TocEntry> {
        items
            .iter()
            .map(|(level, title)| TocEntry::new(*level, *title))
            .collect()
    }

    /// 校验全部条目的转义标记是否与实际内容一致。
    ///
    /// 这是**兜底防线**：正常路径下 [`TocEntry::new`] 会自动标记，
    /// 若调用方手工构造条目导致标记缺失，这里会拦下（原型缺陷 2.4）。
    pub fn verify(entries: &[TocEntry]) -> Result<(), ExportError> {
        for entry in entries {
            let actually_needs = entry
                .title()
                .chars()
                .any(|c| NEEDS_ESCAPE_CHARS.contains(&c));
            if actually_needs && !entry.needs_escape() {
                return Err(ExportError::UnescapedTocEntry {
                    title: entry.title().to_string(),
                });
            }
        }
        Ok(())
    }

    /// 生成 HTML 转义后的标题（供导出器直接写入 `<a>` 文本）。
    ///
    /// 与 [`TocBuilder::verify`] 配套：只要 `verify` 通过，导出器用本函数即可保证一致。
    pub fn escape_title(entry: &TocEntry) -> String {
        let mut out = String::with_capacity(entry.title().len());
        for ch in entry.title().chars() {
            match ch {
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '&' => out.push_str("&amp;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&#39;"),
                other => out.push(other),
            }
        }
        out
    }
}

use super::aggregate::ExportJob;

/// 导出器抽象：由基础设施层实现（S5-09），应用层据此触发具体格式的导出。
///
/// 实现负责把 `content` 渲染为目标格式并写入 `job.destination()`；
/// 应用层只负责选项校验、状态机推进与 [`crate::shared::event::DomainEvent::ExportFinished`]
/// 事件发布，**不在本层触碰文件系统**（违反分层会被 `check-arch` 拦截）。
pub trait Exporter: Send + Sync {
    /// 执行导出，返回**实际写入**的字节数。
    ///
    /// 成功时由调用方把 `job` 推进为 `Succeeded`，失败时推进为 `Failed` 并返回 `Err`。
    /// 实现内部如需写磁盘，应使用基础设施的 IO 能力，而非在应用层或领域层直接 `std::fs`。
    ///
    /// # 为什么必须返回**产出**大小而不是让调用方去 `stat`
    ///
    /// 应用层拿不到文件大小（`std::fs` 被 `check-arch` 禁止），所以「导出了
    /// 多大」这个信息只能由真正写盘的实现给出。
    ///
    /// 曾经让应用层用 `content.len()` 顶替，那是**源文**长度：同一份 Markdown
    /// 导出成 HTML 时实际文件是它的十几倍（样式表、标签），而界面显示的
    /// 「已导出 X（90 字节）」与磁盘上的 1281 字节对不上。
    /// 这类「数字看着合理但就是错的」比没有数字更难查。
    fn export(&self, content: &str, job: &ExportJob) -> Result<usize, ExportError>;
}
