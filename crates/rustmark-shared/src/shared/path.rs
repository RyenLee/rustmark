//! 领域层路径值对象 `FilePath`。
//!
//! 存在意义（`.agent/conventions.md` 第 12 条）：领域层**不使用** `std::path::PathBuf`。
//! 原因有二：① `PathBuf` 的很多方法会诱导 IO；② 路径规则（重名、越界、相对性）
//! 应当是**可单测的纯规则**，而不是散落在调用点的字符串拼接。
//!
//! 本值对象是 `document` / `workspace` / `export` / `preferences` 四个上下文的
//! 共享内核，故置于 `shared`（需求文档 §6.5 的 Shared Kernel）。
//!
//! 已知限制：不处理 Windows UNC（`\\?\`）与网络共享路径（`\\host\share`），
//! 这类路径在桌面编辑器的「打开单个文件」场景中不会出现。

use std::fmt;

use thiserror::Error;

/// 路径构造失败的原因。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PathError {
    /// 路径为空字符串，或规范化后没有任何有效片段。
    #[error("路径不能为空")]
    Empty,

    /// 路径含有 NUL 字节（操作系统不接受）。
    #[error("路径含有非法字符（NUL）")]
    NullByte,

    /// 路径试图向上穿越（`..`）。本值对象只承载**已确定的**路径，不接受相对穿越。
    #[error("路径不允许向上穿越（`..`）")]
    Traversal,

    /// 待拼接的文件名为空。
    #[error("文件名不能为空")]
    EmptyFileName,

    /// 待拼接的文件名含有分隔符或为 `.` / `..`。
    #[error("文件名不是合法的单段名称：{name}")]
    InvalidFileName {
        /// 被拒绝的原始文件名。
        name: String,
    },
}

/// 已规范化的文件路径（内部统一使用 `/` 分隔）。
///
/// 规范化规则：
/// - `\` → `/`；
/// - 丢弃空片段（即折叠连续的 `/`）与 `.` 片段；
/// - 遇到 `..` 片段直接**拒绝**（返回 [`PathError::Traversal`]）；
/// - 保留 Windows 盘符前缀（如 `C:`）与绝对性。
#[derive(
    Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct FilePath {
    raw: String,
}

impl FilePath {
    /// 构造并规范化路径；不合法时返回 [`PathError`]。
    pub fn new(raw: impl AsRef<str>) -> Result<Self, PathError> {
        let raw = raw.as_ref();
        if raw.is_empty() {
            return Err(PathError::Empty);
        }
        if raw.contains('\0') {
            return Err(PathError::NullByte);
        }

        let normalized = raw.replace('\\', "/");
        let (prefix, body) = split_drive(&normalized);
        let absolute = body.starts_with('/');

        let mut segments: Vec<&str> = Vec::new();
        for segment in body.split('/') {
            match segment {
                "" | "." => continue,
                ".." => return Err(PathError::Traversal),
                other => segments.push(other),
            }
        }
        if segments.is_empty() {
            return Err(PathError::Empty);
        }

        let slash = if absolute { "/" } else { "" };
        Ok(Self {
            raw: format!("{prefix}{slash}{}", segments.join("/")),
        })
    }

    /// 取出规范化的路径字符串（分隔符统一为 `/`）。
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// 最后一段（文件名）；`FilePath` 至少有一段，故不返回 `Option`。
    pub fn file_name(&self) -> &str {
        let body = split_drive(&self.raw).1;
        // 已归一化的路径里只有 `/`，但走同一实现免得两处规则分家。
        file_name_of(body)
    }

    /// 扩展名（不含点号）；无扩展名或文件名以点开头且无后续内容时为 `None`。
    ///
    /// 例：`a.md` → `Some("md")`；`Makefile` → `None`；`.gitignore` → `None`。
    ///
    /// 判定规则与 [`crate::shared::text_format::extension_of`] **同源**
    /// （共用本文件末尾的 [`extension_index`]）——那是「可打开格式白名单」的
    /// 依据，两者若各写一份，白名单与实际扩展名识别迟早会给出不同答案。
    pub fn extension(&self) -> Option<&str> {
        let name = self.file_name();
        let start = extension_index(name)?;
        Some(&name[start..])
    }

    /// 替换扩展名，返回新路径。
    ///
    /// `new_extension` **不含点号**（如 `"html"`）。原路径无扩展名时直接追加。
    ///
    /// 用途：PDF 导出需要把 `report.pdf` 改写成 `report.print.html`
    /// 交给系统打印管线——扩展名替换必须发生在**值对象**层，
    /// 否则每个适配器都要各自拼字符串，Windows 的 `\` 分隔符极易出错。
    pub fn with_extension(&self, new_extension: &str) -> Self {
        let name = self.file_name();
        let (prefix, body) = split_drive(&self.raw);
        let dir = match body.rfind('/') {
            Some(slash) => format!("{}/", &body[..slash]),
            None => String::new(),
        };
        // 用 `match` 而非 `is_some_and`：后者在无扩展名时返回 `&str`，
        // 类型与 `&name[..]` 分支不一致，需再包一层，徒增噪音。
        let stem = match name.rfind('.') {
            Some(dot) if dot > 0 && dot + 1 < name.len() => &name[..dot],
            _ => name,
        };
        Self {
            raw: format!("{prefix}{dir}{stem}.{new_extension}"),
        }
    }

    /// 父目录；已是顶层（仅剩盘符或单段）时为 `None`。
    pub fn parent(&self) -> Option<Self> {
        let (prefix, body) = split_drive(&self.raw);
        let slash = body.rfind('/')?;
        let parent_body = &body[..slash];
        if parent_body.is_empty() {
            // 形如 `/a`：父目录是根 `/`，但根没有片段，视为无父目录。
            return None;
        }
        Some(Self {
            raw: format!("{prefix}{parent_body}"),
        })
    }

    /// 在当前路径下拼接一个**单段**名称。
    ///
    /// `name` 含分隔符、为 `.` / `..` 或为空时返回错误——避免出现「拼接出越界路径」。
    pub fn join(&self, name: &str) -> Result<Self, PathError> {
        if name.is_empty() {
            return Err(PathError::EmptyFileName);
        }
        if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
            return Err(PathError::InvalidFileName {
                name: name.to_string(),
            });
        }
        Ok(Self {
            raw: format!("{}/{}", self.raw, name),
        })
    }

    /// 本路径是否位于 `ancestor` 之下（包含 `ancestor` 自身之外的所有后代）。
    ///
    /// 自身与祖先相同返回 `false`（不是「之下」）。
    pub fn is_under(&self, ancestor: &Self) -> bool {
        let me = self.raw.as_str();
        let base = ancestor.raw.as_str();
        if !me.starts_with(base) {
            return false;
        }
        me.len() > base.len() && me.as_bytes().get(base.len()) == Some(&b'/')
    }

    /// 是否以 `prefix` 开头（用于判断「移动到自身子目录」时被移动目录自身的匹配）。
    pub fn starts_with_path(&self, prefix: &Self) -> bool {
        self.raw.starts_with(prefix.raw.as_str())
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl AsRef<str> for FilePath {
    fn as_ref(&self) -> &str {
        &self.raw
    }
}

impl TryFrom<String> for FilePath {
    type Error = PathError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<&str> for FilePath {
    type Error = PathError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// 拆分 Windows 盘符前缀（如 `C:`）与剩余部分；非盘符路径前缀为空串。
fn split_drive(path: &str) -> (&str, &str) {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        (&path[..2], &path[2..])
    } else {
        ("", path)
    }
}

/// 路径末段（文件名）——**接受未归一化的原始路径**。
///
/// # 为什么不能直接用 [`FilePath::file_name`]
///
/// [`FilePath`] 是**值对象**：构造即校验（拒绝空串、拒绝 `..`）并归一化
/// （`\` → `/`）。而这里要处理的是**用户机器上的原始路径字符串**——
/// Windows 的原生对话框、拖放事件、错误消息里的路径都可能是
/// `C:\Users\me\a.md`。那种路径：
/// - 未归一化（`\` 还在）；
/// - 可能含 `..`、可能为空（理论上）。
///
/// 用 `FilePath::new(path).ok().map(|p| p.file_name())` 会把**合法路径**
/// 也判成失败（`..` 被拒）→ 退化成"显示整条路径"，比现在更糟。
/// 故本函数**只做取末段这一件事**，不做任何校验：
///
/// ```text
/// "C:\\a\\b.md"  → "b.md"     （反斜杠也算分隔符）
/// "/a/b/c.md"    → "c.md"
/// "b.md"         → "b.md"     （无分隔符，原样返回）
/// ```
///
/// # 唯一的实现
///
/// 全库「取路径末段」只应走这里。此前散落 9 处 `rsplit('/')`，
/// 其中 3 处（app 层的对话框标题、删除确认、打开失败提示）**只认 `/`**，
/// 在 Windows 上会把整条路径当文件名显示给用户。
pub fn file_name_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// 扩展名在**文件名**（末段）中的起始字节下标（点在 `index - 1`）。
///
/// 这是「什么算扩展名」这条规则的**唯一实现**，两处入口共用：
/// - [`FilePath::extension`]：已归一化的值对象路径（本文件）；
/// - [`crate::shared::text_format::extension_of`]：原始路径字符串
///   （可能带 `\`、可能大小写混杂）。
///
/// 分开写两份的后果在 `FilePath` 上出现过：规则一致时看不出问题，
/// 一旦有人改了一处（比如决定 `.tar.gz` 只算 `gz`、或 `.gitignore`
/// 不算扩展名），另一处就会静默地给出不同答案。
///
/// 住在 `path.rs` 而不是 `text_format.rs`：本文件已有 `file_name_of`
/// 取末段，`text_format` 取原始路径的扩展名时本就依赖本文件；规则放在
/// 被依赖侧，`shared` 内部依赖方向保持 `text_format → path` 单向
/// （内部环的破法见守卫测试 `internal_cycle_guard_test.rs`）。
pub(crate) fn extension_index(name: &str) -> Option<usize> {
    let dot = name.rfind('.')?;
    // 前导点开头（`.gitignore`）与结尾点（`a.`）都不是扩展名。
    if dot == 0 || dot + 1 >= name.len() {
        return None;
    }
    Some(dot + 1)
}
