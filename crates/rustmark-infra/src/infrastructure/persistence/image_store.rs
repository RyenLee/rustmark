//! 图片资源导入的文件系统实现（R-04b）。
//!
//! # 三段式：建目录 → 定名（重名去重）→ 复制
//!
//! 1. **建目录**：`<文档目录>/assets/`，`create_dir_all` 幂等——
//!    第一个图片落地时顺带建出资源目录，不需要用户先手动建；
//! 2. **定名**：目标已存在时派生 `-1` / `-2` … 序号后缀（见
//!    [`deduped_target`]）。**绝不覆盖**：覆盖意味着上一次插入的图
//!    静静消失，而两份引用长得一模一样、零报错；
//! 3. **复制**：`fs::copy` 而非移动——用户在文件管理器里的原文件
//!    不该被编辑器搬走。
//!
//! # 为什么链接在这里拼而不是在调用方
//!
//! 资源目录名（`IMAGE_ASSET_DIR`）与相对路径拼法是**一条规则**，
//! 放在端口文档里由实现保证；调用方只拿 `link` 嵌进 `![](...)`。
//! 漏出去的结果是「这里拼 `assets/`、那里拼 `./assets/`」——
//! 拼错不报错，只是预览里图裂了。

use std::path::{Path, PathBuf};

use rustmark_core::shared::error::AppError;
use rustmark_core::shared::image_asset::{IMAGE_ASSET_DIR, ImageAssetStore, ImportedImage};
use rustmark_core::shared::path::FilePath;
use rustmark_core::shared::result::AppResult;

/// 重名去重的序号上限。
///
/// 正常使用永远碰不到（同目录同文件名第 1000 次插入才有意义），
/// 设上限是为了把「病态循环」变成一条可读的错误而不是无限循环。
const MAX_DEDUPE_ATTEMPTS: u32 = 1000;

/// 基于文件系统的图片资源导入。
///
/// 无状态，纯粹的能力包装（与 `SystemExternalOpener` / `RfdDialogAdapter`
/// 同一形态）。
#[derive(Debug, Clone, Copy, Default)]
pub struct FsImageAssetStore;

impl FsImageAssetStore {
    /// 构造导入器。
    pub const fn new() -> Self {
        Self
    }
}

impl ImageAssetStore for FsImageAssetStore {
    fn import_image(&self, source: &FilePath, doc_dir: &Path) -> AppResult<ImportedImage> {
        let source_path = Path::new(source.as_str());
        // 对话框返回的路径可能已经不存在（用户选完就删了），也可能选到
        // 目录——两种都按「导入失败」报，让用户重选，而不是复制出一个
        // 0 字节文件或把目录当文件读。
        if !source_path.is_file() {
            return Err(AppError::io(format!(
                "图片文件不存在或不可读：{}",
                source.as_str()
            )));
        }
        let assets_dir = doc_dir.join(IMAGE_ASSET_DIR);
        std::fs::create_dir_all(&assets_dir)?;
        let file_name = source.file_name();
        let target = deduped_target(&assets_dir, file_name)?;
        std::fs::copy(source_path, &target)?;
        let stored_name = target
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| AppError::io("资源文件名不是合法 UTF-8"))?;
        Ok(ImportedImage {
            link: format!("{IMAGE_ASSET_DIR}/{stored_name}"),
        })
    }
}

/// 在 `assets_dir` 下为 `file_name` 找一个**不冲突**的落点。
///
/// 首轮用原名；已存在则按 `stem-1.ext` / `stem-2.ext` … 递增。
/// 无扩展名的文件同样处理（`Makefile` → `Makefile-1`）。
fn deduped_target(assets_dir: &Path, file_name: &str) -> AppResult<PathBuf> {
    let candidate = assets_dir.join(file_name);
    if !candidate.exists() {
        return Ok(candidate);
    }
    let (stem, extension) = match file_name.rsplit_once('.') {
        // 点号在最前（`.gitignore` 形态）不算扩展名分隔。
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (file_name, None),
    };
    for n in 1..=MAX_DEDUPE_ATTEMPTS {
        let name = match extension {
            Some(ext) => format!("{stem}-{n}.{ext}"),
            None => format!("{stem}-{n}"),
        };
        let candidate = assets_dir.join(&name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(AppError::io(format!(
        "资源目录下同名文件过多（{file_name} 已占用 1-{MAX_DEDUPE_ATTEMPTS}），请先清理"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> FsImageAssetStore {
        FsImageAssetStore::new()
    }

    /// 首轮原名落盘，链接形如 `assets/photo.png`。
    #[test]
    fn first_import_uses_the_original_name() {
        let dir = tempfile::tempdir().expect("临时目录");
        let src = dir.path().join("photo.png");
        std::fs::write(&src, b"png-bytes").expect("写源文件");

        let imported = store()
            .import_image(
                &FilePath::new(src.to_str().expect("路径")).expect("合法路径"),
                dir.path(),
            )
            .expect("导入应成功");

        assert_eq!(imported.link, "assets/photo.png");
        assert!(dir.path().join("assets/photo.png").exists());
    }

    /// 重名派生成 `-1` 后缀，且不覆盖已有文件。
    #[test]
    fn second_import_dedupes_without_overwriting() {
        let dir = tempfile::tempdir().expect("临时目录");
        let src = dir.path().join("photo.png");
        std::fs::write(&src, b"second").expect("写源文件");
        std::fs::create_dir_all(dir.path().join("assets")).expect("建资源目录");
        std::fs::write(dir.path().join("assets/photo.png"), b"first").expect("预置同名文件");

        let imported = store()
            .import_image(
                &FilePath::new(src.to_str().expect("路径")).expect("合法路径"),
                dir.path(),
            )
            .expect("导入应成功");

        assert_eq!(imported.link, "assets/photo-1.png");
        // 旧文件必须原样还在。
        assert_eq!(
            std::fs::read(dir.path().join("assets/photo.png")).expect("读旧文件"),
            b"first"
        );
    }

    /// 序号连续递增（`-1` 也占用时进 `-2`）。
    #[test]
    fn dedupe_counter_advances() {
        let dir = tempfile::tempdir().expect("临时目录");
        let src = dir.path().join("a.png");
        std::fs::write(&src, b"x").expect("写源文件");
        let assets = dir.path().join("assets");
        std::fs::create_dir_all(&assets).expect("建资源目录");
        std::fs::write(assets.join("a.png"), b"0").expect("预置");
        std::fs::write(assets.join("a-1.png"), b"1").expect("预置");

        let imported = store()
            .import_image(
                &FilePath::new(src.to_str().expect("路径")).expect("合法路径"),
                dir.path(),
            )
            .expect("导入应成功");

        assert_eq!(imported.link, "assets/a-2.png");
    }

    /// 无扩展名文件同样去重（`Makefile` → `Makefile-1`）。
    #[test]
    fn extensionless_names_dedupe_too() {
        let dir = tempfile::tempdir().expect("临时目录");
        let src = dir.path().join("Makefile");
        std::fs::write(&src, b"x").expect("写源文件");
        std::fs::create_dir_all(dir.path().join("assets")).expect("建资源目录");
        std::fs::write(dir.path().join("assets/Makefile"), b"0").expect("预置");

        let imported = store()
            .import_image(
                &FilePath::new(src.to_str().expect("路径")).expect("合法路径"),
                dir.path(),
            )
            .expect("导入应成功");

        assert_eq!(imported.link, "assets/Makefile-1");
    }

    /// 以点开头、无扩展名的形态（`.env`）不该被拆成 stem="" + ext="env"。
    #[test]
    fn dotfile_names_are_not_split() {
        let dir = tempfile::tempdir().expect("临时目录");
        let src = dir.path().join(".env");
        std::fs::write(&src, b"x").expect("写源文件");
        std::fs::create_dir_all(dir.path().join("assets")).expect("建资源目录");
        std::fs::write(dir.path().join("assets/.env"), b"0").expect("预置");

        let imported = store()
            .import_image(
                &FilePath::new(src.to_str().expect("路径")).expect("合法路径"),
                dir.path(),
            )
            .expect("导入应成功");

        assert_eq!(imported.link, "assets/.env-1");
    }

    /// 源不存在时报错，不造 0 字节文件。
    #[test]
    fn missing_source_is_an_error() {
        let dir = tempfile::tempdir().expect("临时目录");
        let missing = dir.path().join("gone.png");
        std::fs::write(&missing, b"x").expect("先写");
        std::fs::remove_file(&missing).expect("再删");

        let err = store()
            .import_image(
                &FilePath::new(missing.to_str().expect("路径")).expect("合法路径"),
                dir.path(),
            )
            .expect_err("不存在的源必须报错");

        assert!(
            matches!(err, AppError::Io { .. }),
            "应是 I/O 类错误，实为 {err:?}"
        );
    }
}
