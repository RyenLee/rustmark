//! 「插入图片」（R-04b）：选图 → 导入资源目录 → 光标处插相对链接。
//!
//! # 三步为什么这样分
//!
//! 1. **入口只登记意图**（`insert_image`）：`pick_image` 是同步阻塞的
//!    原生模态框，必须走 [`DialogRunner`] 的后台线程，与「打开文件」
//!    同一套路（见 `dialog_runner` 模块文档）；
//! 2. **回填做编排**（`apply_picked_image`）：有路径的文档 → 复制进
//!    资源目录、插**相对**链接；未命名文档 → 插**绝对**路径
//!    （没有文档目录可相对，也无法建资源目录）；
//! 3. **插入复用 `insert_at_cursor`**：与符号面板 / 工具栏插入结构
//!    同一条路径——撤销点、光标后移、选区替换的语义只此一份。
//!
//! # 为什么复制而不是只插链接
//!
//! 用户选中的图可能在临时目录 / 桌面上，文档被移动或分享后链接即断。
//! 复制进 `<文档目录>/assets/` 后，文档自带资源、可整体搬迁——
//! 这是 2026-10-20 与用户确认的契约（方案原文的两个候选项之一）。

use super::*;

impl EditorApp {
    /// 「插入图片」入口（工具栏「插入」组 / 菜单 / 命令面板共用）。
    ///
    /// 只起对话框，不做别的：真正的复制与插入在结果回填时发生
    /// （[`Self::apply_picked_image`]）。
    pub fn insert_image(&mut self) {
        self.dialogs.spawn(DialogRequest::PickImage);
    }

    /// 选择图片对话框的回填：导入资源 → 拼 Markdown → 插入光标处。
    ///
    /// 取消由调用方（`apply_dialog_outcome`）静默处理，走不到这里。
    pub(super) fn apply_picked_image(&mut self, path: FilePath) {
        // 与插入符号同一取舍：没有活动文档就先建一个，别让用户
        // 「点了插入却什么都没发生」。
        let Some(_id) = self.ensure_document() else {
            return;
        };
        let link = match self.document_dir() {
            Some(dir) => match self.services.images.import_image(&path, &dir) {
                Ok(imported) => imported.link,
                Err(e) => {
                    self.set_status_error(&format!("导入图片失败：{e}"));
                    return;
                }
            },
            // 未命名文档：没有文档目录，资源无处安放。插绝对路径
            // （`FilePath` 已归一化为 `/` 分隔，可直接嵌进 Markdown），
            // 预览侧能解析，保存后用户可自行整理成相对链接。
            None => path.as_str().to_string(),
        };
        let markdown = format!("![]({link})");
        if let Err(e) = self.insert_at_cursor(&markdown) {
            self.set_status_error(&format!("插入图片失败：{e}"));
        }
    }
}
