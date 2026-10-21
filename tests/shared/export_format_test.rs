//! 导出格式的唯一真源：`shared::export_format`（R-07）。
//!
//! # 这组用例在钉什么
//!
//! R-07 之前，导出格式有**两份**枚举：领域层的 `ExportFormat` 与
//! 表现层的 `ExportFormatChoice`。两者方法逐一对齐，但**只靠字符串耦合**——
//! 表现层加一个变体，领域层不会报错，两边就此漂移。
//!
//! 收敛后真源只有一处（[`rustmark::shared::export_format`]），本组用例守住它：
//!
//! 1. **穷举**：`parse` 能反解 `ALL` 里的每一个变体，且 `ALL` 的长度
//!    恰为 4（加了变体必须同步改这里，否则红）；
//! 2. **不漏分支**：解析走的是「遍历 `ALL` + 比对标识」，
//!    因此「新增变体时忘记加解析分支」这件事**不可能发生**——
//!    用例反过来钉住这一点：每个变体的标识都能被反解回去；
//! 3. **持久化兼容**：标识字符串（`html` / `markdown` / `pdf` / `image`）
//!    是写进用户配置的契约，**不许改**——否则既有配置全部失效；
//! 4. **历史别名**：`md` / `png` 仍被接受（曾被接受过的写法）。
//!
//! # 为什么用例在 `tests/shared` 而不是 `tests/domain`
//!
//! 真源现在住在 `shared`（三方共同可达的唯一层），用例就跟着住它。

use rustmark::shared::export_format::{ExportFormat, PageSize};

/// `parse` 能反解 `ALL` 里的每一个变体。
///
/// 这就是 R-07 的核心验收：**标识 ↔ 变体是双射**。
/// 少一个分支（变异：把某变体从 `ALL` 里删掉）→ 红。
#[test]
fn export_format_from_str_is_exhaustive() {
    for format in ExportFormat::ALL {
        assert_eq!(
            ExportFormat::parse(format.as_str()),
            Some(*format),
            "{format:?} 的标识 {:?} 必须能反解回自己",
            format.as_str()
        );
    }
}

/// 变体清单的长度钉死为 4。
///
/// **为什么要有这个断言**：解析实现是「遍历 `ALL`」，
/// 所以「加了变体但忘了登记进 `ALL`」在**运行期完全不可见**
/// （那个变体就是解析不出来，却没人报错）。
/// 这个数字把清单的变化暴露成一条必须被改的红线——
/// 加变体的人会看到它，然后去改 `ALL`，于是变体真的被支持了。
///
/// 变了这个数字时，请同时确认：新变体的标识（`as_str`）已定、
/// 基础设施层的路由 `match` 已加分支（否则编译不过——那是另一道防线）。
#[test]
fn export_format_all_has_exactly_four_entries() {
    assert_eq!(
        ExportFormat::ALL.len(),
        4,
        "导出格式清单的长度变了：新增格式时请同步更新本断言，\
         并确认标识（as_str）与基础设施的路由 match 都已就位"
    );
}

/// 标识字符串是**持久化契约**，不许改。
///
/// 这些值写进过用户的配置文件与快捷键绑定。改动会让既有配置
/// 静默失效——不报错，只是「设置回到默认」，用户无从察觉。
#[test]
fn export_format_persisted_ids_are_stable() {
    let expected = [
        (ExportFormat::Html, "html"),
        (ExportFormat::Markdown, "markdown"),
        (ExportFormat::Pdf, "pdf"),
        (ExportFormat::Image, "image"),
    ];
    for (format, id) in expected {
        assert_eq!(
            format.as_str(),
            id,
            "{format:?} 的持久化标识不能改（改了就让既有用户配置失效）"
        );
    }
}

/// 反解必须**不区分大小写**、忽略首尾空白。
///
/// 旧的手写 `parse_format` 就是这么做的（`to_ascii_lowercase`），
/// 收敛时不能把这个宽容度丢掉——否则用户写 `"HTML"` 就报「未知导出格式」。
#[test]
fn export_format_from_str_ignores_case_and_surrounding_space() {
    assert_eq!(ExportFormat::parse("HTML"), Some(ExportFormat::Html));
    assert_eq!(
        ExportFormat::parse("  Markdown  "),
        Some(ExportFormat::Markdown)
    );
    assert_eq!(ExportFormat::parse("PdF"), Some(ExportFormat::Pdf));
}

/// 历史别名仍被接受。
///
/// `md`（Markdown）与 `png`（图片）曾被手写解析器接受过。
/// 突然不认会让外部调用方（旧脚本、复制的命令行）报「未知导出格式」，
/// 而它们本意明确。
#[test]
fn export_format_still_accepts_legacy_aliases() {
    assert_eq!(
        ExportFormat::parse("md"),
        Some(ExportFormat::Markdown),
        "`md` 是 Markdown 的历史别名，不该被拒"
    );
    assert_eq!(
        ExportFormat::parse("png"),
        Some(ExportFormat::Image),
        "`png` 是图片格式的历史别名，不该被拒"
    );
}

/// 无法识别的字符串必须返回 `None`（而不是猜一个）。
///
/// 尤其**不能**把未知值退回默认格式（HTML）：那会让用户以为自己导出了
/// Markdown，实际得到的是 HTML，且没有任何报错。
#[test]
fn export_format_from_str_rejects_unknown_values() {
    for raw in ["docx", "", "   ", "htm", "html5", "mark down"] {
        assert_eq!(
            ExportFormat::parse(raw),
            None,
            "{raw:?} 不是合法格式，不该被解析成某个格式"
        );
    }
}

/// 页面尺寸同样走唯一真源，且标识是持久化契约。
#[test]
fn page_size_round_trips_through_its_persisted_id() {
    for size in PageSize::ALL {
        assert_eq!(
            PageSize::parse(size.as_str()),
            Some(*size),
            "{size:?} 的标识 {:?} 必须能反解回自己",
            size.as_str()
        );
    }
    assert_eq!(PageSize::A4.as_str(), "a4");
    assert_eq!(PageSize::Letter.as_str(), "letter");
    assert_eq!(PageSize::ALL.len(), 2, "纸张只有 A4 与 Letter 两种");
}

/// 页面尺寸的解析同样宽容、同样拒绝未知值。
#[test]
fn page_size_from_str_is_forgiving_but_strict() {
    assert_eq!(PageSize::parse("A4"), Some(PageSize::A4));
    assert_eq!(PageSize::parse(" letter "), Some(PageSize::Letter));
    for raw in ["a3", "", "legal"] {
        assert_eq!(PageSize::parse(raw), None, "{raw:?} 不是合法纸张");
    }
}

/// 能力判据（目录 / 分页 / 纸张）必须与格式一一对应。
///
/// 这些判据是**领域规则**（`ExportJob::validate` 依赖它们），
/// 不是界面装饰：判错会让「Markdown 也能带目录」这种非法组合通过校验。
#[test]
fn format_capabilities_match_the_known_formats() {
    assert!(ExportFormat::Html.supports_toc(), "HTML 支持目录");
    assert!(ExportFormat::Pdf.supports_toc(), "PDF 支持目录");
    assert!(
        !ExportFormat::Markdown.supports_toc(),
        "Markdown 是纯文本，不支持目录"
    );
    assert!(
        !ExportFormat::Image.supports_toc(),
        "长图是单张图片，没有页码，不支持目录"
    );

    assert!(ExportFormat::Pdf.supports_pagination(), "PDF 支持分页");
    assert!(!ExportFormat::Html.supports_pagination(), "HTML 不分页");
    assert!(!ExportFormat::Markdown.supports_pagination());
    assert!(!ExportFormat::Image.supports_pagination());

    assert!(ExportFormat::Pdf.needs_page_size(), "PDF 要指定纸张");
    assert!(!ExportFormat::Html.needs_page_size());
    assert!(!ExportFormat::Markdown.needs_page_size());
    assert!(!ExportFormat::Image.needs_page_size());
}

/// 每个格式都要有扩展名与显示名。
///
/// 扩展名决定产出文件名（`目标.md` / `目标.png`），
/// 显示名决定对话框里那一行字——缺一个都是用户可见的空洞。
#[test]
fn every_format_has_an_extension_and_a_label() {
    for format in ExportFormat::ALL {
        assert!(!format.extension().is_empty(), "{format:?} 缺扩展名");
        assert!(!format.label().is_empty(), "{format:?} 缺显示名");
    }
    assert_eq!(ExportFormat::Html.extension(), "html");
    assert_eq!(ExportFormat::Markdown.extension(), "md");
    assert_eq!(ExportFormat::Pdf.extension(), "pdf");
    assert_eq!(ExportFormat::Image.extension(), "png");
}

/// 扩展名必须与标识**不同**的那些格式，要说清为什么。
///
/// Markdown 的标识是 `markdown` 而扩展名是 `md`：这不是笔误，
/// 是「提交给应用层的取值」与「落到磁盘的文件名」两件事。
/// 这个用例把差异钉住，防止有人「顺手统一」成同一个值
/// ——那会让 `.md` 文件的导出产物名变成 `目标.markdown`。
#[test]
fn format_id_and_extension_may_differ_and_said_so() {
    assert_ne!(
        ExportFormat::Markdown.as_str(),
        ExportFormat::Markdown.extension(),
        "Markdown 的标识与扩展名本就不同，别「顺手统一」"
    );
    assert_eq!(
        ExportFormat::Markdown.extension(),
        "md",
        "Markdown 的产物仍是 .md"
    );
    assert_eq!(ExportFormat::Image.extension(), "png", "图片产物仍是 .png");
}
