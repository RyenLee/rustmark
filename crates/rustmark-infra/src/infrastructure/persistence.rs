//! infrastructure::persistence 模块声明。

pub mod asset_loader;

/// 原子写入的单一实现（文档 / 草稿 / 配置 / 导出共用，评审 P3）。
///
/// 不上公开 API：调用方一律是 crate 内的仓储与导出器，外部无需知道
/// 「临时文件叫什么」。唯一对外的能力是残留清理，见 [`tmp_residue`]。
pub(crate) mod atomic_file;

pub mod encoding;
pub mod fs_document_repository;
pub mod fs_workspace_repository;
pub mod image_store;
pub mod recent_files_store;

/// 崩溃残留临时文件的清理（评审 P3）。
///
/// 对组合根公开的唯一入口是 [`tmp_residue::sweep_stale`]：启动时清一次
/// 数据目录；文档目录由保存路径上的内部钩子顺带清理。
pub mod tmp_residue;

pub mod toml_preferences_repository;
