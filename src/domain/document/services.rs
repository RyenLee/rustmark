//! Document 上下文的领域服务（S3-02 `TextBuffer` trait、S3-05 `EditingService`）。
//!
//! - [`TextBuffer`]：文本缓冲的**领域抽象**，由基础设施层用 `ropey` 实现（S5-01）。
//!   所有方法返回 `Result` 或饱和值，**不 panic**（`panic = "abort"` 下 panic 即进程终止）。
//! - [`EditingService`]：**无状态纯函数**集合，输入 `&str` 输出 [`EditPlan`]，
//!   不触碰缓冲区，因而全部可单测。
//!
//! # 模块结构
//!
//! `EditingService` 的方法按业务能力分散在私有子模块中，对外仍统一经
//! [`EditingService`] 类型调用（inherent impl 跨模块挂载，路径不变）：
//!
//! - `coordinates`：UTF-8 字符边界对齐与行区间换算；
//! - `selection`：选区标记符包裹 / 解除；
//! - `headings`：ATX 标题层级读取与升降级；
//! - `lines`：行级编辑（前缀 / 缩进 / 整行移动）与逐行变换原语；
//! - `compose`：列表回车延续、任务勾选、链接 / 代码块 / 表格插入。

mod compose;
mod coordinates;
mod headings;
mod lines;
mod selection;

use std::borrow::Cow;

use crate::shared::geometry::{ByteOffset, LineCol, TextRange};

use super::errors::TextBufferError;

/// 文本缓冲区的领域接口。
///
/// 一切偏移以**字节**为单位；实现必须 `Send + Sync`（跨线程异步加载大文件）。
///
/// 越界约定：`offset == len_bytes()` 合法（表示末尾）；`offset > len_bytes()` 返回
/// [`TextBufferError::OutOfRange`]。
pub trait TextBuffer: Send + Sync {
    /// 缓冲区字节长度。
    fn len_bytes(&self) -> usize;

    /// 缓冲区字符（Unicode 标量值）数量。
    fn len_chars(&self) -> usize;

    /// 行数。约定：空文本为 1 行；`"a\n"` 为 2 行（末行是空行）。
    fn len_lines(&self) -> usize;

    /// 在 `offset` 处插入文本（不换行语义由调用方决定）。
    fn insert(&mut self, offset: ByteOffset, text: &str) -> Result<(), TextBufferError>;

    /// 删除 `range` 区间，返回被删除的内容。
    fn remove(&mut self, range: TextRange) -> Result<String, TextBufferError>;

    /// 取出 `range` 区间的文本。
    ///
    /// 连续存储时返回借用的 [`Cow::Borrowed`]，需要拼接时返回 [`Cow::Owned`]。
    fn slice(&self, range: TextRange) -> Result<Cow<'_, str>, TextBufferError>;

    /// 全量文本快照。
    ///
    /// 用于撤销快照与保存；大文件场景下由阶段 08 优化为增量快照。
    fn snapshot(&self) -> String;

    /// 字节偏移 → 行列坐标（均为 0-based）。
    fn byte_to_line_col(&self, offset: ByteOffset) -> Result<LineCol, TextBufferError>;

    /// 行列坐标 → 字节偏移。行号越界时返回该缓冲区末尾偏移量。
    fn line_col_to_byte(&self, line_col: LineCol) -> Result<ByteOffset, TextBufferError>;

    /// 第 `line` 行（0-based）起始字节偏移。
    fn line_start_byte(&self, line: usize) -> Result<ByteOffset, TextBufferError>;

    /// 第 `line` 行（0-based）结束字节偏移（**不含**换行符）。
    fn line_end_byte(&self, line: usize) -> Result<ByteOffset, TextBufferError>;

    /// 向下取到最近的 UTF-8 字符边界（`offset` 已在边界上时原样返回）。
    fn char_boundary_floor(&self, offset: ByteOffset) -> ByteOffset;

    /// 向上取到最近的 UTF-8 字符边界（`offset` 已在边界上时原样返回）。
    fn char_boundary_ceil(&self, offset: ByteOffset) -> ByteOffset;
}

/// 编辑规则领域服务（无状态纯函数）。
///
/// 所有函数只依赖传入的文本与坐标，不持有缓冲区、不读系统时间、不做 IO，
/// 因此可被表现层（生成编辑命令）与测试直接复用。
///
/// # 换行约定
///
/// 全部函数同时支持 `LF` 与 `CRLF`：按 `\n` 切分，行尾可能残留的 `\r`
/// 会在处理时剥离并在输出时恢复，因此**不会**把 `\r` 当作行内容的一部分。
pub struct EditingService;

/// 缓冲区句柄别名（与聚合根内定义一致，重导出以集中引用）。
///
/// 应用层据此持有「可替换的缓冲区实现」，不直接依赖 `ropey`。
pub use super::aggregate::BufferHandle;

/// 缓冲区工厂：由基础设施层注入具体的 [`TextBuffer`] 实现（如 `RopeTextBuffer`）。
///
/// 应用层需把文本装进缓冲区后再构造 `Document`，但**不得**直接依赖 `ropey`，
/// 故通过本 trait 在组合根处注入实现，满足「应用层 → 领域，基础设施反向注入」的依赖矩阵。
pub trait BufferFactory: Send + Sync {
    /// 用给定内容创建一个缓冲区句柄。
    fn create(&self, content: &str) -> BufferHandle;
}
