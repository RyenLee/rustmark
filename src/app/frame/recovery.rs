//! 启动恢复提示：遗留草稿的居中浮层与「恢复 / 丢弃」两条通路。
use super::*;

impl EditorApp {
    /// 启动恢复提示（DEF-02）：若 `pending_recoveries` 非空，弹出一个居中的
    /// 非阻塞窗口，提供「恢复全部」（按内容新建未命名文档）与「丢弃全部」
    /// （清掉草稿）两个动作。
    ///
    /// 直到用户点击之前，`pending_recoveries` 持续非空、每帧重绘；点击后才清空，
    /// 因此不会因某一帧没点到就丢失待恢复内容。
    pub(super) fn render_recovery_prompt(&mut self, ctx: &egui::Context) {
        if self.pending_recoveries.is_empty() {
            return;
        }
        let previews: Vec<String> = self
            .pending_recoveries
            .iter()
            .map(|(_, content)| {
                let preview: String = content.chars().take(80).collect();
                if content.chars().count() > 80 {
                    format!("{preview}…")
                } else {
                    preview
                }
            })
            .collect();
        let mut action: Option<bool> = None;
        egui::Window::new("未保存草稿恢复")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("检测到上次异常退出时遗留的未保存草稿，是否恢复？");
                ui.separator();
                for preview in &previews {
                    ui.label(format!("· {preview}"));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("恢复全部").clicked() {
                        action = Some(true);
                    }
                    if ui.button("丢弃全部").clicked() {
                        action = Some(false);
                    }
                });
            });
        match action {
            Some(true) => self.recover_all_drafts(),
            Some(false) => self.discard_all_drafts(),
            None => {}
        }
    }

    /// 恢复全部遗留草稿：按草稿内容新建未命名文档，并删除对应草稿文件。
    ///
    /// 从 [`Self::render_recovery_prompt`] 的「恢复全部」分支抽出，使按钮点击与
    /// 集成测试走**同一条**代码路径——否则测试覆盖的是副本，按钮本身反而是未测代码。
    pub fn recover_all_drafts(&mut self) {
        let drafts_dir = self.services.drafts_dir.clone();
        for (id, content) in std::mem::take(&mut self.pending_recoveries) {
            if let Ok(new_id) = self.services.documents.new_document_with_content(&content) {
                // 仅在恢复成功后清草稿：新建失败时保留草稿，下次启动仍会再提示一次，
                // 而不是让用户的内容随一次失败一起消失。
                let _ = self.services.documents.clear_draft(&drafts_dir, id);
                self.active_doc = Some(new_id);
                self.refresh_text();
            }
        }
    }

    /// 丢弃全部遗留草稿：删除草稿文件并清空待恢复列表。
    pub fn discard_all_drafts(&mut self) {
        let drafts_dir = self.services.drafts_dir.clone();
        for (id, _) in &self.pending_recoveries {
            let _ = self.services.documents.clear_draft(&drafts_dir, *id);
        }
        self.pending_recoveries.clear();
    }

    /// 启动扫描发现的待恢复草稿（只读快照，供状态断言与集成测试）。
    pub fn pending_recoveries(&self) -> &[(DocumentId, String)] {
        &self.pending_recoveries
    }
}
