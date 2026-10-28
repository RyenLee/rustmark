//! 图片资源导入端口（R-04b：「插入图片」的第二跳）。
//!
//! # 为什么放在 `rustmark-ports`
//!
//! 与 [`crate::dialog::DialogPort`] / [`crate::system::ExternalOpener`]
//! 同一范式：导入图片要写盘（`std::fs`），而 `app` 层只应看到端口——
//! 实现在 `infrastructure`（`FsImageAssetStore`），组合根装配时选定。
//! 表现层不直接接触本端口（它经 `EditorApp` 编排），放 `rustmark-ports`
//! （R-10 项 A 从 shared 抽出）是因为 `app` 与 `infrastructure` 都需要
//! 引用同一份契约。
//!
//! # 为什么返回「链接」而不是「落点路径」
//!
//! 调用方（`EditorApp`）要的是**插进 Markdown 的那一串字符**。资源目录名
//! （[`IMAGE_ASSET_DIR`]）与相对路径的拼法是本模块的职责，漏到调用方
//! 就会出现「这里拼 `assets/`、那里拼 `./assets/`」的漂移——而拼错
//! 不会有任何报错，只是预览里图裂了。

use rustmark_shared::shared::path::FilePath;
use rustmark_shared::shared::result::AppResult;

/// 文档资源目录名（相对文档所在目录）。
///
/// 插入的图片复制到这里，Markdown 里以 `assets/xxx.png` 引用。
/// **没有**做成可配置项：这是「一种约定」而不是「一种偏好」——
/// 每个文档一套目录规则会让移动/分享文档时链接全断。
pub const IMAGE_ASSET_DIR: &str = "assets";

/// 图片导入的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedImage {
    /// 相对文档目录的链接路径（正斜杠分隔，如 `assets/photo.png`）。
    ///
    /// 可直接嵌进 `![](...)`；分隔符恒为 `/`（Markdown 惯例，
    /// Windows 的反斜杠在链接里是转义符）。
    pub link: String,
}

/// 图片资源导入端口。
pub trait ImageAssetStore: Send + Sync {
    /// 把 `source` 复制进 `doc_dir` 下的资源目录，返回可嵌入的链接。
    ///
    /// # 重名不覆盖
    ///
    /// 目标已存在时派生 `name-1.ext` / `name-2.ext` …：覆盖意味着
    /// 用户上一次插入的图**静静消失**，而两份引用长得一模一样，
    /// 没有任何报错。
    ///
    /// # 失败语义
    ///
    /// 源不存在 / 不可读、目标目录建不出来、复制失败均返回 `Err`
    /// （[`AppError::Io`](rustmark_shared::shared::error::AppError::Io)）；
    /// 调用方负责把原因告诉用户。
    fn import_image(
        &self,
        source: &FilePath,
        doc_dir: &std::path::Path,
    ) -> AppResult<ImportedImage>;
}
