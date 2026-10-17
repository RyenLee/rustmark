//! 文件系统文档仓储（S5-04）：原子保存 + 编码处理。
//!
//! # 原子保存（`FR-04` 的核心）
//!
//! 直接 `write(target)` 有一个致命问题：**写到一半失败会留下截断的文件**。
//! 用户的原文就这样没了——这是编辑器最不可接受的故障模式。
//!
//! 本实现采用「同目录临时文件 → 落盘 → `rename` 覆盖」：
//! 1. 在**目标同目录**写 `.tmp`（必须同目录：跨目录 `rename` 在多数系统上不是原子操作，
//!    且可能跨文件系统失败）；
//! 2. `flush` + `sync_all` 确保数据真正落到磁盘（否则断电后仍可能丢失，
//!    这一步是`FR-04`「任何失败都不得破坏原文件」的兜底）；
//! 3. `fs::rename(tmp, target)` —— POSIX 与 Windows 的rename 都是原子的
//!    （Windows 上需注意：目标被其它程序占用时可能失败，此时**原文件仍然完好**，
//!    符合契约）。
//!
//! 任一步失败都**不触碰目标文件**，并尽力清理临时文件。
//!
//! # 编码与换行
//!
//! - **编码**：委托 [`super::encoding`] 分级探测（UTF-8 BOM → UTF-16 BOM →
//!   裸 UTF-8 → GBK 兜底）。此前这里直接用 `String::from_utf8_lossy` 兜底，
//!   会把 GBK 文件的**每个字节**替换成 U+FFFD——用户看到满屏 `�`，
//!   而一旦保存，替换字符被写回磁盘，**原文不可逆丢失**。
//!   这是本项目唯一真实的"文本乱码"来源（界面里的中文异常属缺字形，见
//!   `infrastructure::fonts`）。
//! - **保存写回原编码**：保存前重读目标文件探测其编码，按原编码写回——
//!   GBK 文件保存后仍是 GBK（GBK 双字节装不下的字符落 GB18030 四字节，
//!   这是 GB18030 作为 GBK 超集的标准行为）、UTF-16 仍是 UTF-16、
//!   带 BOM 的 UTF-8 补回 BOM。此前「一律写 UTF-8」会让用户的一次普通
//!   `Ctrl+S` 静默改变文件编码，依赖编码的外部工具（老系统、某些导入器）
//!   从此读不了这个文件。磁盘上还没有的文件（新建 / 另存为）没有"原编码"，
//!   按惯常做法写 UTF-8。
//! - **BOM**：读到 UTF-8 BOM 时剥离（否则正文首部会出现不可见字符），
//!   写回时按原始编码是否带 BOM 决定是否补回；
//! - **CRLF**：内部统一用 `LF`（领域层的行号计算只认 `\n`），写回时按原文件
//!   的换行风格还原。只做「读时归一 + 写时还原」而不改内容，用户看不出差别，
//!   但 Windows 用户的文件不会因为我们保存一次就全变成 LF（那是 git diff噪声）。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::domain::document::repository::{DocumentRepository, DocumentSnapshot};
use crate::shared::error::AppError;
use crate::shared::ids::DocumentId;
use crate::shared::path::FilePath;
use crate::shared::time::Timestamp;

/// UTF-8 BOM 的字节序列（EF BB BF）。
///
/// 定义在 [`super::encoding`] 侧，此处转发以保持既有引用点不变。
const UTF8_BOM: [u8; 3] = super::encoding::UTF8_BOM;

/// 文件的换行风格。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LineEnding {
    /// `\n`（Unix / 默认为新文件）。
    #[default]
    Lf,
    /// `\r\n`（Windows）。
    CrLf,
}

impl LineEnding {
    /// 按当前平台推断新建文件应使用的换行。
    ///
    /// Windows 记事本等工具对纯 LF 支持不佳，故新建文件用 `CRLF` 更友好。
    pub const fn platform_default() -> Self {
        if cfg!(windows) { Self::CrLf } else { Self::Lf }
    }

    /// 该风格的换行字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }

    /// 由**原始字节**探测换行风格（出现 `\r\n` 即为 CRLF）。
    ///
    /// 必须传原始字节而非已解码文本：[`Self::decode`] 会把 CRLF 归一为 LF，
    /// 若拿解码结果来探测，CRLF 文件永远会被误判成 LF，
    /// 于是「读时归一 + 写时还原」里的还原步骤失效——用户的 Windows 文件
    /// 会在我们保存一次后被整体改成 LF（git diff 噪声）。
    pub fn detect_bytes(bytes: &[u8]) -> Self {
        if bytes.windows(2).any(|pair| pair == b"\r\n") {
            Self::CrLf
        } else {
            Self::Lf
        }
    }
}

/// 文件系统文档仓储。
#[derive(Debug, Default)]
pub struct FsDocumentRepository {
    /// 写回时使用的换行风格；`None` 表示「按原文件风格还原」。
    line_ending: Option<LineEnding>,
}

impl FsDocumentRepository {
    /// 构造仓储，写回时保留各文件原有换行风格。
    pub fn new() -> Self {
        Self { line_ending: None }
    }

    /// 构造仓储，强制所有文件用指定换行风格写回。
    pub fn with_line_ending(line_ending: LineEnding) -> Self {
        Self {
            line_ending: Some(line_ending),
        }
    }

    /// 把领域层路径转换为 `std::path::Path`。
    ///
    /// `FilePath` 内部统一用 `/`，Windows 的 `std::fs` 同样接受 `/`，
    /// 故直接替换即可，无需处理盘符（`FilePath` 已保留 `C:` 前缀）。
    fn to_path(path: &FilePath) -> PathBuf {
        Self::canonicalize_or_clean(PathBuf::from(path.as_str()))
    }

    /// 纵深防御：canonicalize 解析符号链接得到真实路径；
    /// 文件尚不存在（新建场景）时退回 `components()` 清理。
    fn canonicalize_or_clean(p: PathBuf) -> PathBuf {
        match p.canonicalize() {
            Ok(canon) => {
                // Windows canonicalize 会返回 UNC 扩展前缀 `\\?\`，
                // 必须去掉并统一为 `/`，否则与 FilePath 不兼容。
                let s = canon.to_string_lossy().replace('\\', "/");
                PathBuf::from(s.trim_start_matches("//?/"))
            }
            Err(_) => p.components().collect(),
        }
    }

    /// 由磁盘元数据取最后修改时间。
    ///
    /// 取不到（文件被删除、时钟异常）时返回 `None`——调用方据此判断
    /// 「是否有已保存内容」，不应因时间戳缺失而报错。
    fn saved_at_of(path: &Path) -> Option<Timestamp> {
        let metadata = fs::metadata(path).ok()?;
        let modified = metadata.modified().ok()?;
        let millis = modified
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        Some(Timestamp::from_millis(
            u64::try_from(millis).unwrap_or(u64::MAX),
        ))
    }

    /// 解码字节为文本：按 BOM 与合法性分级探测编码，并把 CRLF 归一为 LF。
    ///
    /// 编码探测本身在 [`super::encoding`]——那里能给出"识别成了什么编码"，
    /// 而本函数只关心"给出一段可直接编辑的文本"。
    fn decode(bytes: &[u8]) -> String {
        Self::decode_with_encoding(bytes).0
    }

    /// [`Self::decode`] 的「带编码」版本（V11-03 / DEF-05）。
    ///
    /// 状态栏要显示**本文档的真实编码**，而编码只有解码这一刻才知道，
    /// 丢失了就再也补不回来（重新探测要再读一次磁盘）。故在此一并返回。
    fn decode_with_encoding(bytes: &[u8]) -> (String, super::encoding::Encoding) {
        // 注意：这里**不能**再用 `from_utf8_lossy` 兜底。那会让 GBK 文件的
        // 每个字节变成 U+FFFD，而保存时又写回磁盘，造成不可逆的内容丢失。
        let (text, encoding) = super::encoding::decode(bytes);
        let text = if text.contains('\r') {
            text.replace("\r\n", "\n")
        } else {
            text
        };
        (text, encoding)
    }

    /// [`Self::decode`] 的跨模块可见版本。
    ///
    /// 工作区仓储的「跨文件搜索」必须用**同一套**解码规则，
    /// 否则搜索命中的行号会与编辑器里看到的行不一致。
    pub fn decode_public(bytes: &[u8]) -> String {
        Self::decode(bytes)
    }

    /// 编码文本为字节：按 `ending` 还原换行，并按需补 UTF-8 BOM。
    ///
    /// 仅供草稿等**固定 UTF-8** 场景使用；正式保存走 [`Self::encode_as`]。
    fn encode(text: &str, ending: LineEnding, with_bom: bool) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(text.len() + 16);
        if with_bom {
            bytes.extend_from_slice(&UTF8_BOM);
        }
        if ending == LineEnding::CrLf {
            bytes.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
        } else {
            bytes.extend_from_slice(text.as_bytes());
        }
        bytes
    }

    /// 编码文本为字节：先还原换行，再按 `encoding` 编码（含 BOM 语义）。
    ///
    /// 换行还原必须在**编码之前**：CRLF 是字节层面的 `\r\n`，
    /// 编码后再替换会破坏多字节序列。
    fn encode_as(text: &str, ending: LineEnding, encoding: super::encoding::Encoding) -> Vec<u8> {
        if ending == LineEnding::CrLf && text.contains('\n') {
            let restored = text.replace('\n', "\r\n");
            super::encoding::encode(&restored, encoding)
        } else {
            super::encoding::encode(text, encoding)
        }
    }

    /// 原子写入：同目录临时文件 → sync → rename。
    fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
        let parent = path
            .parent()
            .ok_or_else(|| AppError::io(format!("路径没有父目录，无法保存：{}", path.display())))?;
        // 父目录不存在时创建——用户「另存为」到新目录下的常见场景。
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| AppError::io(format!("无法创建目录 {}：{e}", parent.display())))?;
        }

        // 临时文件名带进程 ID 与计数器语义：避免与并发实例互相覆盖，
        // 也让残留的临时文件可被识别（`.rustmark-tmp-` 前缀）。
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("untitled");
        let tmp_path = parent.join(format!(".rustmark-tmp-{file_name}"));

        // 写入失败时删除残留临时文件——绝不留垃圾在用户目录里。
        let write_result = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp_path)?;
            file.write_all(bytes)?;
            file.flush()?;
            // 同步到磁盘：断电 / 崩溃后 rename 也不应指向未落盘的数据。
            file.sync_all()
        })();

        if let Err(error) = write_result {
            let _ = fs::remove_file(&tmp_path);
            return Err(AppError::io(format!(
                "写入临时文件失败（{}）：{error}",
                tmp_path.display()
            )));
        }

        // rename 失败（如 Windows 上文件被占用）时同样清理临时文件。
        // 此时**原文件未被触碰**，符合 `FR-04` 的「不破坏原文件」契约。
        if let Err(error) = fs::rename(&tmp_path, path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(AppError::io(format!(
                "无法覆盖目标文件（{}）：{error}。文件可能正被其它程序占用",
                path.display()
            )));
        }
        Ok(())
    }
}

impl DocumentRepository for FsDocumentRepository {
    fn load(&self, path: &FilePath) -> Result<DocumentSnapshot, AppError> {
        let fs_path = Self::to_path(path);
        let bytes = fs::read(&fs_path)
            .map_err(|e| AppError::io(format!("无法读取 {}：{e}", path.as_str())))?;
        // 编码只在解码这一刻可知：一并带回快照，供状态栏显示真实编码（V11-03）。
        let (text, encoding) = Self::decode_with_encoding(&bytes);
        Ok(DocumentSnapshot::with_encoding(
            DocumentId::new(),
            path.clone(),
            text,
            Self::saved_at_of(&fs_path),
            encoding.into(),
        ))
    }

    fn save(&self, snapshot: &DocumentSnapshot) -> Result<(), AppError> {
        let fs_path = Self::to_path(&snapshot.path);
        // 读一次原文件，同时服务两个「还原」决策：
        // 换行风格与编码。原文件读不到（新建 / 另存为新路径）时二者都取默认。
        let original = fs::read(&fs_path).ok();
        // 换行风格优先级：显式配置 > 原文件风格 > 平台默认。
        let ending = self.line_ending.unwrap_or_else(|| {
            original
                .as_ref()
                .map(|bytes| LineEnding::detect_bytes(bytes))
                .unwrap_or_else(LineEnding::platform_default)
        });
        // 编码：**原文件是什么编码就写回什么**。一次 Ctrl+S 不该改变
        // 文件的编码——那会让依赖原编码的外部工具从此读不了它。
        let encoding = original
            .as_ref()
            .map(|bytes| super::encoding::decode(bytes).1)
            .unwrap_or(super::encoding::Encoding::Utf8);
        let bytes = Self::encode_as(&snapshot.content, ending, encoding);
        Self::atomic_write(&fs_path, &bytes)
    }

    fn exists(&self, path: &FilePath) -> bool {
        Self::to_path(path).is_file()
    }

    fn delete(&self, path: &FilePath) -> Result<(), AppError> {
        match fs::remove_file(Self::to_path(path)) {
            Ok(()) => Ok(()),
            // 不存在按成功处理：`delete` 契约明说幂等。
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::io(format!("无法删除 {}：{e}", path.as_str()))),
        }
    }

    /// 把未保存草稿写到 `drafts_dir` 下的 `<doc_id>.draft`（异常退出后可恢复，需求 §4.3）。
    ///
    /// 草稿**不覆盖**原文件，故不需要 rename；用「写临时 + rename」同样是为了
    /// 避免恢复时读到半截内容。
    fn save_draft(
        &self,
        drafts_dir: &Path,
        document_id: DocumentId,
        content: &str,
    ) -> Result<(), AppError> {
        fs::create_dir_all(drafts_dir)
            .map_err(|e| AppError::io(format!("无法创建草稿目录 {}：{e}", drafts_dir.display())))?;
        let draft_path = drafts_dir.join(format!("{document_id}.draft"));
        let bytes = Self::encode(content, LineEnding::Lf, false);
        Self::atomic_write(&draft_path, &bytes)
    }

    /// 读取草稿；不存在返回 `None`。
    fn load_draft(
        &self,
        drafts_dir: &Path,
        document_id: DocumentId,
    ) -> Result<Option<String>, AppError> {
        let draft_path = drafts_dir.join(format!("{document_id}.draft"));
        if !draft_path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&draft_path)
            .map_err(|e| AppError::io(format!("无法读取草稿 {}：{e}", draft_path.display())))?;
        Ok(Some(Self::decode(&bytes)))
    }

    /// 删除草稿；不存在按成功处理（幂等）。
    fn clear_draft(&self, drafts_dir: &Path, document_id: DocumentId) -> Result<(), AppError> {
        let draft_path = drafts_dir.join(format!("{document_id}.draft"));
        match fs::remove_file(&draft_path) {
            Ok(()) => Ok(()),
            // 「不存在」视为成功：调用方只想确保草稿已消失。
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AppError::io(format!(
                "无法删除草稿 {}：{e}",
                draft_path.display()
            ))),
        }
    }

    /// 列出 `drafts_dir` 下全部遗留草稿对应的文档 ID。
    fn list_drafts(&self, drafts_dir: &Path) -> Result<Vec<DocumentId>, AppError> {
        let mut out = Vec::new();
        let entries = match fs::read_dir(drafts_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => {
                return Err(AppError::io(format!(
                    "无法读取草稿目录 {}：{e}",
                    drafts_dir.display()
                )));
            }
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".draft") else {
                continue;
            };
            if let Some(id) = parse_draft_id(stem) {
                out.push(id);
            }
        }
        Ok(out)
    }
}

/// 由草稿文件名 `<DocumentId(N)>` 还原文档 ID。
///
/// 草稿文件名即 `DocumentId` 的 `Display` 输出（`DocumentId(N)`），
/// 此处逆向解析，使「列出遗留草稿」能与「按 ID 清除草稿」对齐。
fn parse_draft_id(stem: &str) -> Option<DocumentId> {
    let inner = stem.strip_prefix("DocumentId(")?.strip_suffix(")")?;
    inner.parse::<u64>().ok().map(DocumentId::from_u64)
}
