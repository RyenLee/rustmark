//! 原生对话框：请求、结果回退、删除确认兑现。
use super::*;

impl EditorApp {
    /// 请求一次阻塞对话框（**在后台线程执行**，见 [`DialogRunner`]）。
    ///
    /// 沿用 [`PendingDialog`] 作为对外签名是为了让侧栏 / 欢迎页
    /// 不必知道「建议文件名」这类只对另存为有意义的细节。
    pub fn request_dialog(&mut self, kind: PendingDialog) {
        let request = match kind {
            PendingDialog::OpenFile => DialogRequest::OpenFile,
            PendingDialog::PickFolder => DialogRequest::PickFolder,
            PendingDialog::SaveFile => DialogRequest::SaveFile {
                suggested: self.suggested_save_name(),
            },
        };
        self.dialogs.spawn(request);
    }

    /// 当前是否有原生对话框在等待用户（状态栏提示用）。
    pub fn dialog_busy(&self) -> bool {
        self.dialogs.is_busy()
    }

    /// 应用一个后台对话框的结果（每帧调一次，无结果立即返回）。
    ///
    /// 公开而非私有：它是「取回结果 → 产生副作用」这条通路的**唯一**实现，
    /// 集成测试必须能驱动它——若测试自己另写一遍「拿到路径就打开」，
    /// 变异测试会立刻证明那是两份真相（S6-08 的教训）。
    ///
    /// # 取消必须**静默**
    ///
    /// 用户按「取消」是正常操作，不是错误。弹一条「打开失败：用户取消」
    /// 会让人以为程序出了故障。判据是错误**种类**
    /// （[`AppError::Cancelled`]）而非文案——按文案判断会在一次
    /// 措辞调整后静默失效。
    pub fn apply_dialog_outcome(&mut self, outcome: DialogOutcome) {
        // 确认框**先于**上面的取消短路处理：它的「取消」有具体含义——
        // 撤销待删目标。若跟着路径类结果一起短路返回，`pending_delete`
        // 会一直悬着，用户下次点「删除」反而被「已有确认在飞」吞掉。
        if let DialogOutcome::Confirm(result) = &outcome {
            self.apply_delete_confirm(result);
            return;
        }
        if outcome.is_cancelled() {
            return;
        }
        match outcome {
            DialogOutcome::OpenFile(Ok(path)) => {
                let _ = self.open_path(path.as_str());
            }
            DialogOutcome::SaveFile(Ok(path)) => match self.save_document_to(path.as_str()) {
                Ok(()) => {
                    let now = self.services.markdown.now().millis();
                    self.status_bar
                        .notice(format!("已保存到 {}", path.as_str()), now);
                }
                Err(e) => self.set_status_error(&format!("另存为失败：{e}")),
            },
            DialogOutcome::PickFolder(Ok(path)) => {
                // 「选择文件夹」有两个用途、共用同一个原生目录对话框：
                // ① 打开工作区（「文件」菜单 / 欢迎页）；
                // ② 选导出输出目录（导出对话框里的「选择文件夹…」）。
                // 二者靠**上下文**区分：导出对话框是模态浮层，用户在它上面点
                // 「选择文件夹」时它必然开着；此时结果应回填输出目录，而不是
                // 把工作区根目录也切过去（那会顺带把文件树换掉）。
                if self.dialog.is_open(Overlay::Export) {
                    self.export_ui.output_dir = path.as_str().to_string();
                    self.export_ui.refresh_resolved_path();
                    return;
                }
                let root = path.as_str().to_string();
                match self.scan_workspace(&root) {
                    Ok(count) => {
                        let msg = format!("已打开工作区：{root}（{count} 项）");
                        self.set_status_notice(msg);
                    }
                    Err(e) => self.set_status_error(&format!("无法打开工作区 {root}：{e}")),
                }
            }
            // 非取消的错误：说明是「对话框系统不可用」这类问题，
            // 必须提示——静默会让用户以为点了没反应。
            DialogOutcome::OpenFile(Err(e)) => self.report_dialog_failure("打开文件", &e),
            DialogOutcome::SaveFile(Err(e)) => self.report_dialog_failure("另存为", &e),
            DialogOutcome::PickFolder(Err(e)) => self.report_dialog_failure("选择文件夹", &e),
            // 确认框已在上方先行处理（它有自己的取消语义）。此臂只为
            // 让编译器确认枚举穷尽——真走到这里说明前面的分支被改坏了。
            DialogOutcome::Confirm(_) => {}
        }
    }

    /// 兑现一次删除确认的答复（V11-06）。
    ///
    /// # 三种答复的语义
    ///
    /// - `Ok(true)`：执行删除；
    /// - `Ok(false)`：用户点了「否」，撤销待删目标（**不提示**——他刚明确拒绝，
    ///   再弹一条「已取消删除」是废话）；
    /// - `Err(Cancelled)`：关掉了对话框，与「否」同处理（取消不是错误）；
    /// - `Err(其他)`：对话框系统不可用，**必须提示**——否则用户以为删了，
    ///   实际什么都没发生，或反之。此时同样撤销待删目标，避免下次点击被吞。
    pub(super) fn apply_delete_confirm(&mut self, result: &Result<bool, AppError>) {
        let Some(path) = self.pending_delete.take() else {
            return;
        };
        match result {
            Ok(true) => match self.delete_entry(&path) {
                Ok(()) => self
                    .sidebar
                    .succeed(format!("已删除 {}", Self::display_name(&path))),
                Err(e) => self.sidebar.fail(e.to_string()),
            },
            Ok(false) => {}
            Err(AppError::Cancelled) => {}
            Err(e) => self
                .sidebar
                .fail(format!("未能确认删除 {}：{e}", Self::display_name(&path))),
        }
    }

    /// 路径末段（提示文案里显示的名字）。
    pub(super) fn display_name(path: &str) -> &str {
        path.rsplit('/').next().unwrap_or(path)
    }

    /// 报告「对话框本身不可用」这类失败。
    pub(super) fn report_dialog_failure(&mut self, what: &str, error: &AppError) {
        self.set_status_error(&format!("{what}对话框不可用：{error}"));
    }
}
