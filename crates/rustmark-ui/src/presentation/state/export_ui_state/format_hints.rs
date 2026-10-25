//! 导出对话框的**界面专属**辅助数据（与取值枚举分离）。
//!
//! # 为什么这里没有枚举
//!
//! 格式与纸张的**取值**只有一个真源：
//! [`rustmark_core::shared::export_format::ExportFormat`] /
//! [`rustmark_core::shared::export_format::PageSize`]。它们是跨层值对象——
//! 领域层用它校验任务、基础设施层用它路由导出器、表现层用它渲染对话框。
//!
//! 此前本文件另有一份 `ExportFormatChoice`（与领域枚举逐方法同构），
//! 两份枚举**只靠字符串对齐**，没有编译期保证：表现层加一个变体，
//! 领域层不会报错，两边就此漂移（R-07 收敛掉）。
//!
//! # 那界面的私货放哪
//!
//! 下面这几个是**纯界面数据**，领域层没有、也不该有：
//!
//! - [`is_available`] / [`unavailable_reason`]：该格式在本实现中是否真的可用；
//! - [`pdf_caveat`]：选 PDF 时必须提前告知的产出方式差异；
//! - [`page_size_label`]：纸张的**界面**显示名（带毫米尺寸，领域层只需 `A4`）。
//!
//! 它们写成**独立函数**而非枚举上的方法，正是为了不与取值枚举同构：
//! 一旦同构（同样的方法名、同样的签名），下一次就会有人把它们
//! 「顺手搬回」枚举里，双份实现的坑就回来了。

use rustmark_core::shared::export_format::{ExportFormat, PageSize};

/// 该格式在本实现中是否**真的可用**。
///
/// **PDF 已开放（V11-04）**：选择「开放 + 语义诚实」而非继续禁用——
/// 基础设施（S5-10 打印管线）本就就绪，禁用只是把已能用的能力藏起来。
/// 产出方式取决于本机有什么：装有 Edge / Chrome 时**直接落盘** `.pdf`，
/// 否则退回系统打印对话框。这一点由 [`pdf_caveat`] 在选择 PDF 时
/// 就讲清楚，而不是让用户点了导出才发现。
///
/// 全部格式当前均可用，故此函数恒为 `true`；它保留下来是因为
/// 「可用性」是**界面层**的判断（某个格式的产物依赖本机环境），
/// 与「格式是什么」是两件事——把两者混在一个枚举里，就会出现
/// 「领域层被迫知道界面是否开放了它」的错误依赖方向。
pub const fn is_available(_format: ExportFormat) -> bool {
    true
}

/// 不可用时的原因（`None` = 可用）。
pub const fn unavailable_reason(_format: ExportFormat) -> Option<&'static str> {
    None
}

/// PDF 的**前置声明**：点了会发生什么（V11-04，V11-04a 修订）。
///
/// 之所以必须写在界面上：PDF 与另外三种格式的行为**可能**不同——
/// 其余格式一定是「选好路径 → 点导出 → 文件就在那」，
/// PDF 在缺少浏览器内核的机器上会变成
/// 「生成打印用 HTML → 调起系统对话框 → 用户在对话框里另存」。
/// 不提前说明，用户会以为导出失败（目标 `.pdf` 没出现）。
///
/// 这里**不**去探测本机装了什么再改文案：那要求表现层查文件系统，
/// 既跨层又每帧一次。把两种情形各说一句更省事，也不会说错。
pub const fn pdf_caveat(format: ExportFormat) -> Option<&'static str> {
    match format {
        ExportFormat::Pdf => Some(
            "PDF 由系统内置排版内核直接生成到目标路径（需本机装有 Edge 或 Chrome）；缺少时改为调起系统打印，请在对话框中选择「另存为 PDF」。",
        ),
        ExportFormat::Html | ExportFormat::Markdown | ExportFormat::Image => None,
    }
}

/// 纸张的**界面**显示名（带毫米尺寸）。
///
/// 领域层的 [`PageSize::label`] 只给 `A4` / `Letter`——那是**取值**；
/// 「210×297mm」是给人看的补充说明，属于界面文案，故分开放。
pub const fn page_size_label(size: PageSize) -> &'static str {
    match size {
        PageSize::A4 => "A4 210×297mm",
        PageSize::Letter => "Letter 216×279mm",
    }
}
