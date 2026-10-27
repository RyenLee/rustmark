//! 待执行的阻塞对话框请求（R-03 B1 从组合根迁入）。
//!
//! # 为什么它住在 `rustmark-ui`
//!
//! 它是**表现层的请求标签**：视图（欢迎页 / 导出 / 快速打开）只说「我要一个
//! 打开文件对话框」，不关心对话框怎么弹、结果怎么回。它原先住在 root 的
//! `app/frame/dialog.rs`，于是**每一个**要弹框的视图都得 `use crate::app::frame::…`
//! ——这正是 R-03 B1 要剪断的那条边。
//!
//! 搬到这里后，trait 的 `request_dialog` 端口可以名正言顺地收它，
//! 而视图不必认识组合根。
//!
//! # 公开路径不变
//!
//! root 侧 `src/app/frame.rs` 保留同名 re-export
//! （`pub use crate::presentation::state::pending_dialog::PendingDialog;`），
//! 故 `rustmark::app::frame::PendingDialog` 这条路径**逐字不变**，
//! `tests/app/dialog_runner_test.rs` 等 6 处调用点零改动。

/// 一次待执行的阻塞对话框请求。
///
/// **S6-11f 起本类型只作为请求的标签**，`PendingDialog::SaveFile`
/// 缺少「建议文件名」这个必需参数，故实际请求改用
/// [`crate::app::dialog_runner::DialogRequest`]。
/// 保留本枚举是因为 `PendingDialog` 已被侧栏 / 欢迎页 / 设置引用，
/// 而那三处传的都是 `OpenFile` / `PickFolder`（无需额外参数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingDialog {
    /// 打开文件对话框。
    OpenFile,
    /// 另存为对话框。
    ///
    /// **已弃用**：走这条会丢掉建议文件名，对话框只能给一个空名。
    /// 正确做法是 `crate::app::dialog_runner::DialogRequest::SaveFile`。
    SaveFile,
    /// 选择工作区目录。
    PickFolder,
}
