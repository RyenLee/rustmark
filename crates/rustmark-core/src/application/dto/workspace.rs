//! 工作区上下文的 DTO：导出 / 最近文件 / 文件树 / 标签页 / 全局搜索 /
//! 关闭决策 / 快捷键冲突 / 格式指令结果。

use serde::{Deserialize, Serialize};

use crate::shared::ids::{DocumentId, JobId};
use crate::shared::path::FilePath;

use super::document::{RangeView, SearchHitView};

/// 导出结果视图。
#[derive(Clone, Debug, Serialize)]
pub struct ExportResultView {
    /// 导出任务 ID。
    pub job_id: JobId,
    /// 输出文件路径。
    pub output_path: FilePath,
    /// 输出字节数（**产出文件**的大小，不是源文长度）。
    ///
    /// `0` 表示「大小未知」：PDF 由系统打印对话框产出，本进程拿不到最终大小
    /// （用户可在对话框里改纸张与缩放）。见 `Exporter::export`。
    pub bytes: usize,
    /// 导出格式（中文名，如 `HTML`）。
    pub format: String,
}

/// 最近文件视图。
#[derive(Clone, Debug, Serialize)]
pub struct RecentFileView {
    /// 路径。
    pub path: FilePath,
    /// 最近打开时间（UNIX 毫秒）。
    pub last_opened: u64,
}

/// 文件树条目视图。
#[derive(Clone, Debug, Serialize)]
pub struct FileEntryView {
    /// 路径。
    pub path: FilePath,
    /// 是否为目录。
    pub is_directory: bool,
}

/// 标签页视图。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub struct TabView {
    /// 文档 ID。
    pub document_id: DocumentId,
}

/// 跨文件搜索结果视图。
#[derive(Clone, Debug, Serialize)]
pub struct SearchResultView {
    /// 命中列表（已按上限裁剪）。
    pub hits: Vec<SearchHitView>,
    /// 是否因超过总命中上限而被截断。
    pub truncated: bool,
}

/// 关闭文档的决策视图（供表现层决定是否弹保存对话框）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub enum CloseDecisionView {
    /// 文档已干净，可直接关闭。
    Closed,
    /// 文档有未保存修改，需先询问保存。
    NeedsSave,
}

/// 导出请求视图（表现层提交的导出参数）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportRequestView {
    /// 导出格式（`html` / `markdown` / `pdf` / `image`）。
    pub format: String,
    /// 是否生成目录。
    pub include_toc: bool,
    /// 是否内联图片为 data URI。
    pub embed_images: bool,
    /// 主题名。
    pub theme: String,
    /// 页面尺寸（`a4` / `letter`）。
    pub page_size: String,
    /// 是否分页（仅 PDF 有意义）。
    pub paginate: bool,
}

/// 快捷键冲突视图。
#[derive(Clone, Debug, Serialize)]
pub struct ConflictView {
    /// 冲突类型说明（含组合与动作名）。
    pub description: String,
    /// 冲突的组合键规范显示文本。
    pub combo: String,
    /// 涉及的动作显示名。
    pub actions: Vec<String>,
}

/// 格式指令执行结果（S6-07）。
///
/// 表现层拿不到 `EditPlan`（`check-arch` 禁止 presentation 依赖 domain），
/// 故由应用层把「编辑 + 选区」一并折算成 DTO。
///
/// # 为什么返回**整篇文本**而不是「区间 + 替换串」
///
/// 二者等价，但 `TextEdit` 每帧都要整篇文本。若 DTO 只给区间，
/// 表现层还得再做一次「拼接 + 重算字符偏移」，那处换算既冗余又是最容易
/// 出错的地方（多字节边界）。让应用层一次算干净，表现层只做赋值。
#[derive(Clone, Debug)]
pub struct FormatResultView {
    /// 编辑后的整篇文本。
    pub text: String,
    /// 编辑完成后应选中的区间（**字节偏移**，与 `RangeView` 同语义）。
    ///
    /// 字节而非字符：应用层不知道 UI 用 egui，故不替它选坐标系；
    /// 表现层按需用 `char_to_byte` 的逆变换换算。
    pub selection: RangeView,
    /// 本次指令是否真的改变了内容（`false` 表示无需重排高亮与预览）。
    pub changed: bool,
}
