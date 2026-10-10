//! Markdown 用例查询（阶段 04 · S4-05）。
//!
//! 查询与命令命名不冲突（动名词区分）。
//!
//! # 滚动同步（`MapScrollQuery`）
//!
//! 编辑区与预览区双向滚动同步的核心用例：
//!
//! - 输入编辑区偏移 `offset` → 求预览区目标（命中块 ID + 块内比例 `ratio`）。
//!   比例映射算法见 [`crate::domain::markdown::services::BlockMapper::offset_to_block`]：
//!   `ratio = (offset - 块起点) / 块长`；块内插值使预览能线性跟随。
//! - 输入预览区点击的块 `block_id` → 求编辑区目标偏移（该块的源码起点）。
//!
//! 稳定吸附规则（命中空隙时）：
//! - `offset` 早于首块起点 → 归入首块、比例 `0.0`；
//! - `offset` 落在两块的空隙 / 超出末块 → 归入前一块、比例 `1.0`；
//! - 空块（长度为 0）→ 比例 `0.0`（滚动到块顶）。
//!
//! 以上均由领域层 [`BlockMapper`] 保证，应用层只是透传结果。

use crate::shared::geometry::TextRange;
use crate::shared::ids::DocumentId;

/// 渲染预览查询。
///
/// `version` 是表现层上次看到的源文档版本号：若与注册表当前版本一致且缓存命中，
/// 直接返回缓存（不重新解析）；若不一致（内容已变），强制重新解析。
#[derive(Clone, Debug)]
pub struct RenderPreviewQuery {
    /// 目标文档 ID。
    pub doc_id: DocumentId,
    /// 表现层上次看到的版本号（用于快速判断是否需要重解析）。
    pub version: u64,
}

/// 语法高亮查询。
#[derive(Clone, Debug)]
pub struct HighlightQuery {
    /// 目标文档 ID。
    pub doc_id: DocumentId,
    /// 待高亮的区间（相对整篇文档）。
    pub range: TextRange,
    /// 语言提示；`None` 表示纯文本（不报错）。
    pub lang: Option<String>,
}

/// 独立片段的高亮查询（S6-05：预览区代码块）。
///
/// 与 [`HighlightQuery`] 的区别：后者按**文档**的字节区间取内容再高亮，
/// 而预览区的代码块只有块自身的文本（`BlockView::content`），
/// 既不知道自己在文档里的偏移，也不该为了高亮再去查一次文档。
/// 故此处直接把文本交给高亮器。
#[derive(Clone, Debug)]
pub struct HighlightSnippetQuery {
    /// 待高亮的文本（不是文档，只是一段代码）。
    pub text: String,
    /// 语言提示（来自 ``` 围栏后的标记）。
    pub lang: Option<String>,
}

/// 滚动同步查询。
#[derive(Clone, Debug)]
pub struct MapScrollQuery {
    /// 目标文档 ID。
    pub doc_id: DocumentId,
    /// 滚动来源（编辑区偏移 / 预览区点击块）。
    pub source: ScrollSource,
}

/// 重排大纲查询。
#[derive(Clone, Debug)]
pub struct RebuildOutlineQuery {
    /// 目标文档 ID。
    pub doc_id: DocumentId,
}

/// 滚动来源：从哪一侧发起的滚动/点击。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrollSource {
    /// 编辑区滚动到某字节偏移 → 求预览区目标（块 + 比例）。
    Editor {
        /// 编辑区光标/视口顶部的字节偏移。
        offset: usize,
    },
    /// 预览区点击某块 → 求编辑区目标偏移（块起点）。
    Preview {
        /// 被点击的渲染块 ID。
        block_id: u64,
    },
}

/// 滚动目标：对侧应当跳转到的位置。
///
/// 不含 `Eq`：`ratio` 为 `f32`，浮点不实现 `Eq`。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ScrollTarget {
    /// 预览区目标：命中块 ID + 块内比例（0.0–1.0）。
    Preview {
        /// 命中块 ID。
        block_id: u64,
        /// 块内比例（0.0 块顶，1.0 块底）。
        ratio: f32,
    },
    /// 编辑区目标：应跳转到的字节偏移。
    Editor {
        /// 目标字节偏移（块起点）。
        offset: usize,
    },
}

/// 拼写检查查询（编辑器内英文错词波浪线）。
#[derive(Clone, Debug)]
pub struct SpellCheckQuery {
    /// 目标文档 ID。
    pub doc_id: DocumentId,
    /// 是否启用检查。关闭时服务直接返回空集，**不查缓存也不重算**——
    /// 让「关掉开关」这一动作零成本。
    pub enabled: bool,
}
