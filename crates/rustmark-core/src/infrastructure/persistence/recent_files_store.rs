//! 最近文件持久化存储（S5-05 配套）。
//!
//! # 与领域层的关系
//!
//! 本store 只负责「把 `Vec<RecentFile>` 存到磁盘 / 从磁盘读回来」，
//! **排序、去重、上限裁剪**一律交给领域层的 [`RecentFiles`]
//! ——持久化层不复制业务规则，否则两处规则会各自漂移。
//!
//! # 损坏文件的处理（ADR-003）
//!
//! 配置文件可能因「写盘时断电 / 被其它程序改写 / 手工编辑出错」而损坏。
//! 本 store 的策略是**回退到空列表并备份损坏文件**，而不是让编辑器启动失败：
//!
//! - 直接报错 → 用户打不开编辑器，只因为一个无关紧要的「最近文件」列表坏了，
//!   这是不可接受的（可用性 > 数据完整性，且最近文件随时会被重新写入）；
//! - 直接丢弃 → 用户丢失排查线索，且原本损坏的文件被覆盖后无法复原。
//!
//! 故：备份为 `<文件名>.corrupt-<时间戳>`，再回退到空列表。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::workspace::value_objects::{RECENT_FILES_LIMIT, RecentFile};
use crate::shared::error::AppError;

/// 最近文件存储（TOML 持久化）。
#[derive(Clone, Debug)]
pub struct RecentFilesStore {
    file: PathBuf,
    /// 条数上限，写入时由领域层裁剪，这里仅作兜底。
    limit: usize,
}

impl RecentFilesStore {
    /// 以指定文件路径构造。
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            limit: RECENT_FILES_LIMIT,
        }
    }

    /// 自定义条数上限（测试用）。
    pub fn with_limit(file: impl Into<PathBuf>, limit: usize) -> Self {
        Self {
            file: file.into(),
            limit: limit.max(1),
        }
    }

    /// 存储文件路径。
    pub fn path(&self) -> &Path {
        &self.file
    }

    /// 读取最近文件列表。
    ///
    /// - 文件不存在 → 返回空列表（首次启动的正常情形，不应视为错误）；
    /// - 文件损坏 → 备份后返回空列表（见模块文档的 ADR-003 策略）。
    pub fn load(&self) -> Result<Vec<RecentFile>, AppError> {
        if !self.file.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&self.file).map_err(|e| {
            AppError::io(format!("无法读取最近文件列表 {}：{e}", self.file.display()))
        })?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        match toml::from_str::<RecentFilesDoc>(&raw) {
            Ok(doc) => Ok(doc.items.into_iter().take(self.limit).collect()),
            Err(e) => {
                // 损坏：备份后回退，绝不让编辑器因配置损坏而无法启动。
                let backup = self.backup_corrupt(&raw);
                log::warn!(
                    "最近文件列表解析失败（{e}），已备份到 {} 并回退为空列表",
                    backup.display()
                );
                Ok(Vec::new())
            }
        }
    }

    /// 保存最近文件列表（原子写）。
    ///
    /// 排序 / 去重 / 裁剪由领域层 `RecentFiles` 完成，本方法只做序列化与落盘，
    /// 并额外按 `self.limit` 兜底裁剪一次，防止调用方绕过领域层写入超量数据。
    pub fn save(&self, items: &[RecentFile]) -> Result<(), AppError> {
        let doc = RecentFilesDoc {
            version: SCHEMA_VERSION,
            items: items.iter().take(self.limit).cloned().collect(),
        };
        let body = toml::to_string_pretty(&doc)
            .map_err(|e| AppError::io(format!("序列化最近文件列表失败：{e}")))?;
        atomic_write(&self.file, body.as_bytes())
    }

    /// 把损坏内容备份到带时间戳的旁文件，返回备份路径。
    fn backup_corrupt(&self, raw: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let name = self
            .file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("recent.json");
        let backup = self.file.with_file_name(format!("{name}.corrupt-{stamp}"));
        // 备份失败不应连带影响主流程（此时用户的列表本来就已经丢了）。
        let _ = atomic_write(&backup, raw.as_bytes());
        backup
    }
}

/// 持久化文档的 schema 版本（ADR-003 要求带版本号以便将来迁移）。
const SCHEMA_VERSION: u32 = 1;

/// TOML 持久化模型。
///
/// **刻意与领域层的 `Preferences` 分离**：这是磁盘格式，可以带版本号、
/// 可以有 serde 属性；领域聚合不应为持久化而扭曲自己的形状。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RecentFilesDoc {
    /// schema 版本；缺失时 serde 会报错，故用默认值兜底旧文件。
    #[serde(default = "default_version")]
    version: u32,
    /// 最近文件条目。
    #[serde(default)]
    items: Vec<RecentFile>,
}

fn default_version() -> u32 {
    SCHEMA_VERSION
}

/// 原子写入：同目录临时文件 → rename 覆盖（与文档仓储同一策略，`FR-04`）。
pub(crate) fn atomic_write(target: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = target
        .parent()
        .ok_or_else(|| AppError::io(format!("路径没有父目录：{}", target.display())))?;
    if !parent.exists() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::io(format!("无法创建目录 {}：{e}", parent.display())))?;
    }
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config");
    let tmp = parent.join(format!(".rustmark-tmp-{name}"));

    let write = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()
    })();

    if let Err(e) = write {
        let _ = fs::remove_file(&tmp);
        return Err(AppError::io(format!("写入 {} 失败：{e}", tmp.display())));
    }
    if let Err(e) = fs::rename(&tmp, target) {
        let _ = fs::remove_file(&tmp);
        return Err(AppError::io(format!(
            "覆盖 {} 失败：{e}。文件可能正被其它程序占用",
            target.display()
        )));
    }
    Ok(())
}
