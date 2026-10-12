//! 原生对话框：请求、结果回退、删除确认兑现。
use super::*;

use crate::application::workspace::commands::ScanTreeCommand;
use crate::application::workspace::service::TREE_MAX_DEPTH;
use crate::shared::path::FilePath;

/// 一次待执行的阻塞对话框请求。
///
/// **S6-11f 起本类型只作为请求的标签**，`PendingDialog::SaveFile`
/// 缺少「建议文件名」这个必需参数，故实际请求改用
/// [`crate::app::dialog_runner::DialogRequest`]。
/// 保留本枚举是因为 `PendingDialog` 已被侧栏 / 欢迎页 / 设置引用，
/// 而那三处传的都是 `OpenFile` / `PickFolder`（无需额外参数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingDialog {
    /// 打开文件对话框。
    OpenFile,
    /// 另存为对话框。
    ///
    /// **已弃用**：走这条会丢掉建议文件名，对话框只能给一个空名。
    /// 正确做法是 [`crate::app::dialog_runner::DialogRequest::SaveFile`]。
    SaveFile,
    /// 选择工作区目录。
    PickFolder,
}

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
                // 后台扫描：避免大目录阻塞 UI 帧循环。
                //
                // 领域层 `scan_tree`（RwLock 写入 + EventBus publish）线程安全
                // （WorkspaceAppService 有 Send + Sync 编译期断言），放后台无副作用。
                // 主线程每帧通过 `poll_scan_result` 取回，收到后再构造 WorkspaceEntries
                // + 展开顶层目录 + 更新状态栏。其他扫描路径（reload / refresh）
                // 保持同步——那些路径的目录规模已知且通常较小。
                let root = path.as_str().to_string();
                let workspace_svc = self.services.workspace.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                let root_clone = root.clone();
                std::thread::spawn(move || {
                    let root_path = match FilePath::new(&root_clone) {
                        Ok(p) => p,
                        Err(e) => {
                            let _ = tx.send((root_clone, Err(e.to_string())));
                            return;
                        }
                    };
                    let result = workspace_svc
                        .scan_tree(ScanTreeCommand {
                            root: root_path,
                            max_depth: TREE_MAX_DEPTH,
                        })
                        .map_err(|e| e.to_string());
                    let _ = tx.send((root_clone, result));
                });
                self.scan_loading = true;
                self.scan_pending = Some(rx);
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

    /// 请求删除一个工作区条目（**先弹二次确认，答复为「是」才真删**）。
    ///
    /// # 为什么不能在这里直接删
    ///
    /// 原生确认框是模态同步调用（见 [`crate::shared::dialog::DialogPort::confirm`]），
    /// 在 `update()` 里调用会让窗口在用户思考的那几秒里完全失去响应。
    /// 故与「打开 / 另存为」同一套路：登记意图 → 后台线程问 → 每帧取回。
    ///
    /// 已有确认在飞时**忽略**本次请求（`DialogRunner::spawn` 的语义）：
    /// 连点两下「删除」不该弹出两个系统窗口，更不该在第一个答复之后
    /// 把第二个目标也顺手删掉。
    pub fn request_delete(&mut self, path: &str) {
        if self.pending_delete.is_some() {
            return;
        }
        let message = self.delete_confirm_message(path);
        self.pending_delete = Some(path.to_string());
        self.dialogs.spawn(DialogRequest::Confirm { message });
    }

    /// 当前是否有删除在等用户确认（状态栏 / 测试观察用）。
    pub fn delete_pending(&self) -> Option<&str> {
        self.pending_delete.as_deref()
    }

    /// 构造删除确认的提示文案。
    ///
    /// 目录要**明说会连内容一起删**：`remove_dir_all` 一旦执行不可恢复，
    /// 而用户右键点到的是目录本身，很可能没意识到里面还有东西。
    /// 只写「确定删除 X？」属于把风险藏起来。
    fn delete_confirm_message(&self, path: &str) -> String {
        let name = path.rsplit('/').next().unwrap_or(path);
        let is_directory = self
            .workspace_entries
            .items
            .iter()
            .any(|(p, is_dir)| *is_dir && p == path);
        if !is_directory {
            return format!("确定删除「{name}」？删除后不可恢复。");
        }
        // 目录：明说会连内容一起删。`remove_dir_all` 不可恢复，
        // 而用户右键点到的是目录自身，很可能没意识到里面还有东西。
        let prefix = format!("{path}/");
        let inside = self
            .workspace_entries
            .items
            .iter()
            .filter(|(p, _)| p.starts_with(&prefix))
            .count();
        format!("确定删除目录「{name}」？\n其中包含约 {inside} 项内容，删除后不可恢复。")
    }
}
