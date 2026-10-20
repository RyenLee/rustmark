//! 系统原生对话框抽象（S5-08 的端口侧）。
//!
//! # 为什么放在 shared而不是 domain / application
//!
//! 依赖矩阵（`docs/architecture-constraints.md`）规定：
//! `presentation` **不得**依赖 `domain`，`infrastructure` **不得**依赖
//! `application` / `presentation`。而「打开文件」这件事同时被三方需要：
//! - `presentation` 在菜单里触发「打开」；
//! - `application` 的 `OpenDocument` 用例需要一个可替换的取路径途径；
//! - `infrastructure` 提供 rfd 实现。
//!
//! 三方共同的依赖点只有 `shared`（`EventBus` / `Clock` 也在此），
//! 故端口定义在此，与那两个抽象保持同一模式。
//!
//! # 为什么方法返回 `Result<_, AppError>` 而非 `Option`
//!
//! 「用户取消」与「对话框系统不可用」必须区分：
//! - 取消 → [`AppError::Cancelled`]，调用方**静默返回**，不弹错误提示；
//! - 不可用 → [`AppError::Dialog`]，需提示用户。
//!
//! 用 `Option` 表达会把两者混成"没选到文件"，导致不可用时静默失败——
//! 这是验收标准「取消操作返回 `Err(Cancelled)` 而非空路径」的本意。

use crate::shared::error::AppError;
use crate::shared::path::FilePath;
use crate::shared::text_format::TextFormats;

/// 文件类型过滤器。
///
/// 映射到原生对话框里的「文件类型」下拉项，如
/// `Markdown (*.md, *.markdown)` 对应 `name = "Markdown"`、
/// `extensions = ["md", "markdown"]`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFilter {
    /// 显示名（如 `Markdown`）。
    pub name: String,
    /// 扩展名列表，**不含点号**（如 `["md", "markdown"]`）。
    pub extensions: Vec<String>,
}

impl FileFilter {
    /// 构造过滤器。
    pub fn new(name: impl Into<String>, extensions: &[&str]) -> Self {
        Self {
            name: name.into(),
            extensions: extensions.iter().map(|e| (*e).to_string()).collect(),
        }
    }
}

/// 原生对话框端口。
///
/// 阶段 04 的测试用 fake 实现；S5-08 提供 rfd 实现。
/// 两者可互换，故**禁止**在此 trait 上出现 rfd 的任何类型。
pub trait DialogPort: Send + Sync {
    /// 打开单个文件。
    ///
    /// 用户取消时返回 [`AppError::Cancelled`]（**不是**空路径）。
    fn open_file(&self, filters: &[FileFilter]) -> Result<FilePath, AppError>;

    /// 选择保存位置。
    ///
    /// `suggested_name` 为建议文件名（可为空串，由对话框决定默认名）。
    fn save_file(&self, suggested_name: &str, filters: &[FileFilter])
    -> Result<FilePath, AppError>;

    /// 选择目录。
    fn pick_folder(&self) -> Result<FilePath, AppError>;

    /// 选择**图片**文件（R-04b：插入图片的第一跳）。
    ///
    /// 与 [`Self::open_file`] 分成两个方法而不是「同一个方法传不同过滤器」：
    /// 「打开文档」与「选一张图来插引用」是两件不同的事，过滤器只是
    /// 表象——前者选中即打开（换掉当前标签），后者选中后还要复制资源、
    /// 拼链接、插入光标处。合成一个入口会让调用方靠过滤器反推语义。
    ///
    /// 过滤器固定用 [`image_filters`]（调用方无需也**不应**自备）：
    /// 支持格式的清单只有这一份。
    fn pick_image(&self) -> Result<FilePath, AppError>;

    /// 确认框（是 / 否）。
    ///
    /// 用于「未保存的修改是否丢弃」这类不可逆操作前的确认。
    fn confirm(&self, message: &str) -> Result<bool, AppError>;

    /// 错误提示框（仅展示，无返回值）。
    fn show_error(&self, message: &str) -> Result<(), AppError>;
}

/// 取默认的**文本文件**过滤器集合。
///
/// 清单**派生自** [`TextFormats`]（问题 045 的「唯一出处」）：在这里另写
/// 一份就会与其他三条入口（打开守卫、拖放分类、文件树置灰）漂移。
/// 曾经的实情正是如此——原生对话框只认 `.md` / `.markdown` / `.txt`，
/// 而拖放已经放行 `.csv`：同一个文件，拖进来能开、用对话框选却选不中。
pub fn default_filters() -> Vec<FileFilter> {
    vec![TextFormats::builtin().file_filter()]
}

/// 插入图片时支持的扩展名（R-04b）。
///
/// **唯一出处**：原生对话框的图片过滤器（[`image_filters`]）从这里推。
/// 与文本格式的 [`TextFormats`] 分开：那套白名单答的是「能否作为
/// **文本**打开」，图片答不了这个问题。
///
/// 清单 = 预览侧解码器（`image::load_from_memory`）实际支持的格式：
/// 位图五件套（PNG / JPEG / GIF / WebP / BMP）。**不含 SVG**——`image`
/// crate 不解码它，插进去只会得到占位符；数据 URI 白名单同样排除
/// `svg+xml`（`asset_loader` 的 XSS 防线），两处口径一致。
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// 取默认的**图片文件**过滤器集合（插入图片用，R-04b）。
pub fn image_filters() -> Vec<FileFilter> {
    vec![FileFilter::new("图片", IMAGE_EXTENSIONS)]
}
