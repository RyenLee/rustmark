//! 测试用临时目录守卫：Drop 时递归删除。
//!
//! 命名用「标签 + 进程 ID + 线程 ID」，避免同机并行测试互相覆盖；
//! 由各测试入口经 `support::tempdir::TempDir` 使用，不必每个适配器测试
//! 各自复制一份建目录 / 删目录逻辑。

use std::fs;
use std::path::{Path, PathBuf};

pub struct TempDir(PathBuf);

impl TempDir {
    /// 创建以 `tag` 区分的临时目录（同名残留先清理）。
    pub fn new(tag: &str) -> Self {
        let unique = format!(
            "rustmark-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let path = std::env::temp_dir().join(unique.replace(['(', ')', ' '], ""));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("应能创建临时目录");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// 在目录下写文件（按需建父目录），返回写入路径。
    pub fn write(&self, relative: &str, content: &str) -> PathBuf {
        let target = self.0.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).expect("应能创建父目录");
        }
        fs::write(&target, content).expect("应能写文件");
        target
    }

    /// 在目录下建子目录（按需建父目录），返回目录路径。
    pub fn mkdir(&self, relative: &str) -> PathBuf {
        let target = self.0.join(relative);
        fs::create_dir_all(&target).expect("应能创建目录");
        target
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
