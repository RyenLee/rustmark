//! 可打开的**文本格式白名单**（共享内核，问题 045）。
//!
//! # 为什么单独一个模块
//!
//! 「哪些文件能被当作文本打开」这条规则原先散落在三处，各写一份：
//!
//! - `app::frame::drag_drop::is_openable_text`（拖放分类）；
//! - `shared::dialog::default_filters`（原生「打开」对话框的扩展名过滤器）；
//! - `app::frame::files::current_document_kind`（状态栏的类型标签）。
//!
//! 三份清单必然漂移：拖放放行了 `.csv`，原生对话框却选不中它；状态栏的
//! 标签表又比另两份多几种扩展名。本模块是这条规则的**唯一**出处——
//! 新增一种可打开的文本格式，只改这里。
//!
//! # 为什么放在 shared 而非 domain / application
//!
//! 这条规则同时被三方需要：表现层（文件树置灰、双击拦截）、应用层
//! （打开守卫、拖放分类）、基础设施层（原生对话框过滤器）。而
//! `.agent/architecture.md` §4 规定：表现层不得依赖 `domain`，
//! 基础设施层不得依赖 `application`。三方共同的依赖点只有 `shared`
//! （与 [`crate::shared::encoding`] / [`crate::shared::dialog`] 同一模式）。
//!
//! # 为什么按扩展名而不是按内容嗅探
//!
//! 内容嗅探（读魔数、试解码）要在**打开之前**读文件，而打开入口有多条
//! （文件树 / 拖放 / 最近文件 / 快速打开），每条都嗅探一次等于把 IO
//! 摊到 UI 路径上。扩展名判定是纯字符串运算、零 IO，且用户对「
//! `.png` 不该被当文本打开」的预期本来就是按扩展名建立的。真正的
//! 「避免乱码」由白名单**拒绝**来保证（而不是「读进来再猜编码」）。
//!
//! 编码仍由基础设施层探测（[`crate::shared::encoding`]，DEF-05）：
//! 白名单决定**能不能开**，编码探测决定**按什么读**。默认路径是
//! UTF-8（无 BOM），带 BOM 的 UTF-8 / UTF-16 与 GBK 在有明确字节
//! 特征时才启用——见 `infrastructure::persistence::encoding::decode`。

use crate::shared::dialog::FileFilter;

/// 内置白名单的扩展名（**小写、不含前导点**）。
///
/// 这是「默认支持哪些格式」的唯一出处。新增格式时同时更新
/// [`display_name`] 的展示名表，否则状态栏会退回中性标签。
pub const BUILTIN_TEXT_EXTENSIONS: &[&str] = &[
    // —— Markdown 家族 ——
    // `markdown` / `mdown` / `mkd` 都是真实存在的写法（早期 Markdown
    // 实现的分支），拒绝它们只会被用户当成「程序坏了」。
    "md", "markdown", "mdown", "mkd", // —— 纯文本 ——
    "txt", "text",
    // —— 逗号分隔值 ——
    // 单/多文件表格在编辑器里以纯文本打开（不做表格视图，只按文本编辑）。
    "csv",
];

/// 扩展名 → 状态栏展示名。
///
/// 只列白名单内的格式：白名单外的文件根本进不了编辑缓冲，
/// 没有「展示名」可言。取不到时由 [`display_name`] 退回中性标签。
const DISPLAY_NAMES: &[(&str, &str)] = &[
    ("md", "Markdown"),
    ("markdown", "Markdown"),
    ("mdown", "Markdown"),
    ("mkd", "Markdown"),
    ("txt", "纯文本"),
    ("text", "纯文本"),
    ("csv", "CSV"),
];

/// 扩展名在**文件名**（末段）中的起始字节下标（点在 `index - 1`）。
///
/// 这是「什么算扩展名」这条规则的**唯一实现**，两处入口共用：
/// - [`extension_of`]：原始路径字符串（可能带 `\`、可能大小写混杂）；
/// - [`crate::shared::path::FilePath::extension`]：已归一化的值对象路径。
///
/// 分开写两份的后果在 `FilePath` 上出现过：规则一致时看不出问题，
/// 一旦有人改了一处（比如决定 `.tar.gz` 只算 `gz`、或 `.gitignore`
/// 不算扩展名），另一处就会静默地给出不同答案。
pub(crate) fn extension_index(name: &str) -> Option<usize> {
    let dot = name.rfind('.')?;
    // 前导点开头（`.gitignore`）与结尾点（`a.`）都不是扩展名。
    if dot == 0 || dot + 1 >= name.len() {
        return None;
    }
    Some(dot + 1)
}

/// 取路径的**扩展名**（小写、不含点）。
///
/// 语义与用户直觉一致：
/// - `笔记.MD` → `Some("md")`（大小写不敏感）；
/// - `.gitignore` → `None`（前导点开头的是**隐藏文件名**，不是扩展名）；
/// - `README` → `None`（无扩展名）；
/// - `a.tar.gz` → `Some("gz")`（只看最后一段）；
/// - `dir/` → `None`（目录，末段为空）。
///
/// 反斜杠一并按分隔符处理：Windows 的原生对话框与拖放事件给的是 `\`，
/// 若只认 `/`，`D:\docs\a.md` 的分隔符判断就会失手（`FilePath` 只在
/// 归一化**之后**才保证 `/`，而本函数面对的正是归一化之前的原始输入）。
pub fn extension_of(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let start = extension_index(name)?;
    Some(name[start..].to_ascii_lowercase())
}

/// 展示名（白名单外的扩展名退回中性的「纯文本」）。
///
/// **不**退回「Markdown」：那会让一个未知格式在状态栏冒充 Markdown，
/// 用户据此判断「我打开对了文件」就错了。
pub fn display_name(ext: &str) -> &'static str {
    let lower = ext.to_ascii_lowercase();
    DISPLAY_NAMES
        .iter()
        .find(|(candidate, _)| *candidate == lower)
        .map_or("纯文本", |(_, name)| *name)
}

/// 一字不差的默认过滤器名（原生对话框的「文件类型」下拉项）。
const FILTER_NAME: &str = "Markdown 与文本文件";

/// **默认**扩展名（另存为对话框的建议文件名用）。
///
/// 这是「新建 / 另存为时默认给哪个后缀」这条规则的**唯一出处**：
/// 此前散落在调用点的 `format!("{stem}.md")` / `"未命名.md"` 是硬编码，
/// 白名单一旦变化（新增或调整首选格式）就会漏改——而漏改的后果是
/// **用户拿到的建议文件名与实际支持的格式不一致**（建议 `.md` 却被
/// 按 `.txt` 打开，或反之）。
///
/// 取白名单的**首项**而不是另立一个常量：白名单的顺序本身即表达了
/// 「首选在前」的意图（[`BUILTIN_TEXT_EXTENSIONS`] 的 Markdown 家族在最前），
/// 另立常量等于把「首选」这件事记两处。
pub const fn default_extension() -> &'static str {
    // `BUILTIN_TEXT_EXTENSIONS` 是 `&[&str]` 切片，取首项需在编译期求值。
    // 用 `match` 而非 `[0]`：切片的索引在 const 上下文里不是 const fn。
    match BUILTIN_TEXT_EXTENSIONS {
        [first, ..] => first,
        [] => "md",
    }
}

/// 可打开的文本格式白名单。
///
/// 内置清单来自 [`BUILTIN_TEXT_EXTENSIONS`]；[`Self::extended_with`]
/// 允许在运行时追加（未来接设置页时，用户自定义扩展名经它合入，
/// 不必改动本模块）。
///
/// 内部一律**小写归一化**存储：判定大小写不敏感是这条规则的一部分
/// （`.PNG` 与 `.png` 必须是同一个判定），把归一化收在构造期，
/// 每个 `accepts` 调用就只剩一次二分/线性比较。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFormats {
    extensions: Vec<String>,
}

impl TextFormats {
    /// 内置白名单（默认值）。
    pub fn builtin() -> Self {
        Self::from_extensions(BUILTIN_TEXT_EXTENSIONS)
    }

    /// 按给定扩展名构造（自动小写归一化、去重、忽略空白项）。
    ///
    /// 前导点会被剥掉：`".md"` 与 `"md"` 是同一个扩展名——配置里
    /// 写成哪种写法都不该导致判定失效。
    pub fn from_extensions<I, S>(extensions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut list: Vec<String> = Vec::new();
        for raw in extensions {
            let normalized = raw
                .as_ref()
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            if normalized.is_empty() || list.contains(&normalized) {
                continue;
            }
            list.push(normalized);
        }
        Self { extensions: list }
    }

    /// 追加若干扩展名，返回新白名单（原值不变）。
    pub fn extended_with<I, S>(&self, extra: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut list = self.extensions.clone();
        for raw in extra {
            let normalized = raw
                .as_ref()
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            if normalized.is_empty() || list.contains(&normalized) {
                continue;
            }
            list.push(normalized);
        }
        Self { extensions: list }
    }

    /// 白名单内的扩展名（小写、不含点）。
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// 该扩展名是否被放行（大小写不敏感）。
    pub fn accepts_extension(&self, ext: &str) -> bool {
        let lower = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        self.extensions.contains(&lower)
    }

    /// 该路径能否被当作文本打开。
    ///
    /// 无扩展名的路径一律拒绝：没有扩展名就没有任何依据判定它是文本，
    /// 「默认放行」等于把二进制文件放进编辑缓冲（本模块存在的理由）。
    pub fn accepts(&self, path: &str) -> bool {
        extension_of(path).is_some_and(|ext| self.accepts_extension(&ext))
    }

    /// 原生「打开」对话框的扩展名过滤器。
    ///
    /// 由白名单派生而非另写一份清单：对话框选不中、但拖放进得来的
    /// 格式（曾经的 `.csv`）是用户最容易碰到的自相矛盾。
    pub fn file_filter(&self) -> FileFilter {
        FileFilter::new(
            FILTER_NAME,
            &self
                .extensions
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )
    }
}

impl Default for TextFormats {
    fn default() -> Self {
        Self::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认扩展名必须是白名单的**首项**（V11-54a / R-16）。
    ///
    /// 断言的是「等于首项」而不是字面量 `"md"`：白名单的首选一旦调整，
    /// 这条用例必须跟着红——否则「默认扩展名」与「白名单」就变成两个
    /// 各自独立的事实，调用点又会退回硬编码。
    #[test]
    fn default_extension_is_the_first_builtin_extension() {
        assert_eq!(default_extension(), BUILTIN_TEXT_EXTENSIONS[0]);
        // 当前首选是 Markdown（默认值取自白名单顺序，不是另立的常量）。
        assert_eq!(default_extension(), "md");
    }

    /// 默认扩展名必须是白名单**内**的扩展名——否则默认名指向一个打不开的格式。
    #[test]
    fn default_extension_is_inside_the_whitelist() {
        assert!(
            BUILTIN_TEXT_EXTENSIONS.contains(&default_extension()),
            "默认扩展名 {:?} 必须在白名单 {:?} 内",
            default_extension(),
            BUILTIN_TEXT_EXTENSIONS
        );
    }

    #[test]
    fn builtin_covers_the_three_required_formats() {
        let w = TextFormats::builtin();
        assert!(w.accepts("/d/notes.md"), "默认可打开 .md");
        assert!(w.accepts("/d/notes.txt"), "默认可打开 .txt");
        assert!(w.accepts("/d/table.csv"), "默认可打开 .csv");
    }

    #[test]
    fn judgement_is_case_insensitive() {
        let w = TextFormats::builtin();
        assert!(w.accepts("/d/NOTES.MD"));
        assert!(w.accepts("/d/Notes.Markdown"));
        assert!(w.accepts("/d/TABLE.CSV"));
    }

    #[test]
    fn images_and_binaries_are_rejected() {
        let w = TextFormats::builtin();
        for path in [
            "/d/a.png",
            "/d/a.jpg",
            "/d/a.jpeg",
            "/d/a.gif",
            "/d/a.bmp",
            "/d/a.webp",
            "/d/a.pdf",
            "/d/a.zip",
            "/d/a.exe",
            "/d/a",
            "/d/.gitignore",
        ] {
            assert!(!w.accepts(path), "{path} 不该被当作文本打开");
        }
    }

    #[test]
    fn extension_extraction_matches_user_expectation() {
        assert_eq!(extension_of("/d/笔记.MD").as_deref(), Some("md"));
        assert_eq!(extension_of("a.tar.gz").as_deref(), Some("gz"));
        // Windows 原生路径：反斜杠也是分隔符，否则 `D:\docs\.md` 这类
        // 路径的末段会被整体当成文件名，前导点判定跟着错。
        assert_eq!(extension_of("D:\\docs\\a.md").as_deref(), Some("md"));
        assert_eq!(extension_of(".gitignore"), None);
        assert_eq!(extension_of("README"), None);
        assert_eq!(extension_of("a."), None);
        assert_eq!(extension_of("dir/"), None);
        assert_eq!(extension_of(""), None);
    }

    #[test]
    fn whitelist_is_extensible_at_runtime() {
        let w = TextFormats::builtin().extended_with([".log", "JSON"]);
        assert!(w.accepts("/d/app.log"), "追加的 .log 应放行");
        assert!(w.accepts("/d/data.JSON"), "追加项同样大小写不敏感");
        // 追加不改变内置清单本身（`&self` 语义）。
        assert!(!TextFormats::builtin().accepts("/d/app.log"));
    }

    #[test]
    fn constructor_normalizes_and_dedupes() {
        let w = TextFormats::from_extensions([" .MD ", "md", "TXT", ""]);
        assert_eq!(w.extensions(), ["md", "txt"].map(String::from).as_slice());
        assert!(w.accepts_extension(".Md"), "带点的写法也应命中");
    }

    #[test]
    fn display_names_are_honest() {
        assert_eq!(display_name("md"), "Markdown");
        assert_eq!(display_name("MARKDOWN"), "Markdown");
        assert_eq!(display_name("txt"), "纯文本");
        assert_eq!(display_name("csv"), "CSV");
        // 未知格式不得冒充 Markdown。
        assert_eq!(display_name("rst"), "纯文本");
    }

    #[test]
    fn file_filter_is_derived_from_the_whitelist() {
        let filter = TextFormats::builtin().file_filter();
        let names: Vec<&str> = filter.extensions.iter().map(String::as_str).collect();
        assert_eq!(names, BUILTIN_TEXT_EXTENSIONS);
        assert!(names.contains(&"csv"), "对话框必须能选中 csv");
    }
}
