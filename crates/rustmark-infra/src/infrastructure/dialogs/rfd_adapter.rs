//! `rfd` 对话框适配器（S5-08）。
//!
//! # 阻塞性：为什么这里用同步 API 而不是 `AsyncFileDialog`
//!
//! rfd 同时提供同步与异步两套 API。本适配器用**同步**版，原因是：
//! 同步版返回 `Option<PathBuf>`，实现简单且不依赖 `async-std` 的运行时；
//! 真正的阻塞问题由**调用方**在阶段 06/08 解决——UI 线程不得直接调本适配器，
//! 必须先在后台线程取回路径，再投递回UI 事件循环。
//! 这一点在此显式记录，避免阶段 06接线时误在 `update()` 里直接调用而卡死界面。
//!
//! # 取消语义
//!
//! rfd 用 `None` 表示用户取消，本适配器转为 [`AppError::Cancelled`]。
//! 绝不返回空`FilePath`——那会让上层把「取消」误判为「打开了一个无名文件」
//! 并触发保存，进一步造成数据丢失。
//!
//! # 扩展名过滤器
//!
//! 只把 `FileFilter` 的扩展名交给 rfd，不加前导点（rfd 期望 `md` 而非 `.md`）。
//! 过滤器列表为空时不添加任何过滤器，此时对话框显示全部文件——
//! 这比强行加一个空的过滤器更符合用户预期。

use rfd::{FileDialog, MessageButtons, MessageDialog, MessageLevel};

use rustmark_core::shared::dialog::{DialogPort, FileFilter, image_filters};
use rustmark_core::shared::error::AppError;
use rustmark_core::shared::path::FilePath;

/// 基于 `rfd` 的原生对话框适配器。
#[derive(Debug, Clone, Copy, Default)]
pub struct RfdDialogAdapter;

impl RfdDialogAdapter {
    /// 构造适配器（无状态，纯粹的能力包装）。
    pub const fn new() -> Self {
        Self
    }

    /// 把 [`FileFilter`] 列表灌进 `FileDialog`。
    ///
    /// 返回 `FileDialog` 便于链式继续设置标题等参数。
    fn apply_filters(dialog: FileDialog, filters: &[FileFilter]) -> FileDialog {
        filters.iter().fold(dialog, |acc, filter| {
            acc.add_filter(
                filter.name().to_string(),
                &filter
                    .extensions
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
        })
    }

    /// `Option<PathBuf>` → `Result<FilePath, AppError>`，把 `None` 映射为取消。
    fn to_result(path: Option<std::path::PathBuf>) -> Result<FilePath, AppError> {
        match path {
            Some(path) => {
                // rfd 返回的是系统原生路径；统一转成`/` 分隔的 `FilePath`。
                let normalized = path.to_string_lossy().replace('\\', "/");
                FilePath::new(normalized).map_err(|e| AppError::Dialog {
                    message: format!("路径非法：{e}"),
                })
            }
            None => Err(AppError::Cancelled),
        }
    }
}

impl DialogPort for RfdDialogAdapter {
    fn open_file(&self, filters: &[FileFilter]) -> Result<FilePath, AppError> {
        let dialog = Self::apply_filters(FileDialog::new().set_title("打开文本文件"), filters);
        Self::to_result(dialog.pick_file())
    }

    fn save_file(
        &self,
        suggested_name: &str,
        filters: &[FileFilter],
    ) -> Result<FilePath, AppError> {
        let mut dialog = Self::apply_filters(FileDialog::new().set_title("保存文本文件"), filters);
        if !suggested_name.is_empty() {
            dialog = dialog.set_file_name(suggested_name);
        }
        Self::to_result(dialog.save_file())
    }

    fn pick_folder(&self) -> Result<FilePath, AppError> {
        let dialog = FileDialog::new().set_title("选择工作区目录");
        Self::to_result(dialog.pick_folder())
    }

    fn pick_image(&self) -> Result<FilePath, AppError> {
        // 过滤器固定用 `image_filters()`（端口契约）：调用方不传，
        // 支持格式的清单只有 shared 那一份。
        let dialog = Self::apply_filters(FileDialog::new().set_title("选择图片"), &image_filters());
        Self::to_result(dialog.pick_file())
    }

    fn confirm(&self, message: &str) -> Result<bool, AppError> {
        let result = MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("请确认")
            .set_description(message)
            .set_buttons(MessageButtons::YesNo)
            .show();
        Ok(result == rfd::MessageDialogResult::Yes)
    }

    fn show_error(&self, message: &str) -> Result<(), AppError> {
        MessageDialog::new()
            .set_level(MessageLevel::Error)
            .set_title("出错了")
            .set_description(message)
            .set_buttons(MessageButtons::Ok)
            .show();
        Ok(())
    }
}
