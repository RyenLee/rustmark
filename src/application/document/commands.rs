//! 文档用例命令（阶段 04 · S4-02）。
//!
//! 命令是**纯数据**结构，不含聚合引用；字段对齐原型与领域聚合的访问器。
//! 命名以动词起头，与 `Query` 的 `Get` 前缀区分。

use crate::shared::geometry::TextRange;
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;

/// 打开已有文件（按路径从仓储加载）。
#[derive(Clone, Debug)]
pub struct OpenFileCommand {
    /// 目标路径。
    pub path: FilePath,
}

/// 新建空白未命名文档。
#[derive(Clone, Debug, Default)]
pub struct NewDocumentCommand;

/// 保存文档。
///
/// `save_as = true` 时若文档已有路径，则在同目录派生 `-copy` 副本路径；
/// 未命名文档（`path` 为 `None`）必须先经「另存为」获得路径，否则保存会报错。
#[derive(Clone, Debug)]
pub struct SaveCommand {
    /// 文档 ID。
    pub id: DocumentId,
    /// 是否另存为副本。
    pub save_as: bool,
}

/// 保存到**指定路径**（S6-11f：另存为对话框回填路径后使用）。
///
/// 与 [`SaveCommand`] 分开而不是给它加一个 `Option<FilePath>`：
/// `save_as = true` 的语义是「同目录生成 `-copy` 文件名」，
/// 与「保存到用户刚在对话框里选的那个路径」是两件不同的事，
/// 合成一个可选字段会让「选了路径却还是存成了 `-copy`」成为可能。
#[derive(Clone, Debug)]
pub struct SaveToPathCommand {
    /// 文档 ID。
    pub id: DocumentId,
    /// 目标路径（来自原生对话框）。
    pub path: FilePath,
}

/// 在指定偏移插入文本（替换选区请用 [`ReplaceRangeCommand`]）。
#[derive(Clone, Debug)]
pub struct InsertTextCommand {
    /// 文档 ID。
    pub id: DocumentId,
    /// 插入字节偏移。
    pub offset: usize,
    /// 待插入文本。
    pub text: String,
}

/// 把区间替换为文本。
#[derive(Clone, Debug)]
pub struct ReplaceRangeCommand {
    /// 文档 ID。
    pub id: DocumentId,
    /// 待替换区间。
    pub range: TextRange,
    /// 替换文本。
    pub text: String,
}

/// 撤销一步。
#[derive(Clone, Debug)]
pub struct UndoCommand {
    /// 文档 ID。
    pub id: DocumentId,
}

/// 从磁盘重新加载（「还原修改」，R-04a）。
///
/// 与 [`UndoCommand`] 是两件事：撤销回退**一步编辑**，还原回到**磁盘版本**。
/// 未命名文档（无路径）没有磁盘版本可还原，用例直接报错。
#[derive(Clone, Debug)]
pub struct RevertCommand {
    /// 文档 ID。
    pub id: DocumentId,
}

/// 重做一步。
#[derive(Clone, Debug)]
pub struct RedoCommand {
    /// 文档 ID。
    pub id: DocumentId,
}

/// 关闭文档。
///
/// `force = true` 丢弃未保存修改（用户选择「不保存」）；`force = false` 时若文档脏，
/// 返回 [`crate::application::dto::CloseDecisionView::NeedsSave`] 由表现层决定是否弹保存对话框。
#[derive(Clone, Debug)]
pub struct CloseCommand {
    /// 文档 ID。
    pub id: DocumentId,
    /// 是否强制关闭（丢弃未保存修改）。
    pub force: bool,
}

/// 设置光标 / 选区。
#[derive(Clone, Debug)]
pub struct SetSelectionCommand {
    /// 文档 ID。
    pub id: DocumentId,
    /// 选区（空区间表示仅移动光标、清除选区）。
    pub selection: TextRange,
}

// ===========================================================================
// 格式指令（S6-07）
// ===========================================================================
//
// 为什么不直接用 `ReplaceRangeCommand` + 表现层自己拼字符串：
// 那样格式规则会散落到 UI 里，而快捷键与命令面板也会各自拼一套，
// 三入口必然漂移（原型缺陷 B-02 的根因）。
//
// 故格式指令是**一等 Command**：领域规则由 `EditingService` 独占，
// 应用层负责取内容 → 调规则 → 写回 → 发布事件，表现层只发命令。

/// 格式指令种类。
///
/// 与领域 `EditingService` 的函数**一一对应**，不新增规则；
/// 新增格式动作必须先在领域加纯函数，再在此加枚举变体，
/// 从而保证「领域有规则 = 应用能调用 = UI 有按钮」三者恒等。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatKind {
    /// 加粗（`**`）。
    Bold,
    /// 斜体（`*`）。
    Italic,
    /// 删除线（`~~`）。
    Strikethrough,
    /// 行内代码（`` ` ``）。
    InlineCode,
    /// 引用（`> `）。
    Quote,
    /// 无序列表（`- `）。
    BulletList,
    /// 有序列表（`1. `，序号自动递增）。
    OrderedList,
    /// 提升标题级别（h6 → h1 → h2 …）。
    PromoteHeading,
    /// 降低标题级别（剥掉一层 `#`；已是正文则不变）。
    DemoteHeading,
    /// 切换任务勾选态（`- [ ] ` ⇄ `- [x] `）。
    ToggleTask,
    /// 插入链接（`[文本](url)`，URL 预填并选中）。
    InsertLink,
    /// 插入围栏代码块。
    InsertCodeBlock,
    /// 插入 GFM 表格骨架（3×3）。
    InsertTable,
    /// 增加一级缩进。
    Indent,
    /// 减少一级缩进。
    Outdent,
}

/// 应用一种格式指令。
#[derive(Clone, Debug)]
pub struct FormatCommand {
    /// 文档 ID。
    pub id: DocumentId,
    /// 指令种类。
    pub kind: FormatKind,
    /// 作用区间（**字节偏移**）。
    ///
    /// 行级指令（引用 / 列表 / 标题 / 缩进）只取其**覆盖的行**；
    /// 行内指令（加粗等）作用于区间本身；单点指令（任务勾选）
    /// 只取其所在行。
    pub range: TextRange,
    /// 缩进单位（仅 `Indent` / `Outdent` 使用，由偏好 `tabSize` 推导）。
    ///
    /// 领域规则**不在此处硬编码缩进宽度**（原型缺陷 2.2 的根因），
    /// 故必须由调用方传入。其它指令忽略此字段。
    pub indent_unit: String,
}
