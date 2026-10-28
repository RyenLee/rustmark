//! 原生对话框：请求、结果回退、删除确认兑现。
use super::*;

// `PendingDialog` 已于 R-03 B1 迁入 `rustmark-ui`（见
// `crates/rustmark-ui/src/presentation/state/pending_dialog.rs`）——
// 它是表现层的请求标签，视图要弹框就不该被迫认识组合根。
// 本文件经 `use super::*` 取回该类型，`src/app/frame.rs` 保留同名 re-export，
// 故 `rustmark::app::frame::PendingDialog` 路径逐字不变。

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
            // 三类确认框（删除 / 全部关闭 / 还原）共用同一个 `Confirm` 出口，
            // 靠「哪个 pending 有值」分流——与 `PendingDialog` 当年
            // 「一个字段多种含义」的缺陷相反：这里三个字段各自只有一种含义。
            if self.pending_close_all {
                self.apply_close_all_confirm(result);
            } else if self.pending_revert.is_some() {
                self.apply_revert_confirm(result);
            } else {
                self.apply_delete_confirm(result);
            }
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
                    self.status_bar.notice(
                        format!("已保存到 {}", Self::display_name(path.as_str())),
                        now,
                    );
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
                // 同步扫描：scan_tree 是纯领域层 + 内存数据结构构造，
                // 对于常规项目目录（< 1000 项）耗时远低于 UI 刷新阈值。
                // 保持同步让 workspace_root() / workspace_entries 立即可用
                // ——对话框返回后直接能渲染文件树，也避免了测试代码
                // （没有 update() 循环）等待后台 channel 完成的时序问题。
                let root = path.as_str().to_string();
                match self.scan_workspace(&root) {
                    Ok(count) => {
                        self.status_bar.notice(
                            format!("已打开工作区：{}（{count} 项）", Self::display_name(&root)),
                            self.services.markdown.now().millis(),
                        );
                        // 切到工作区视图（问题 064）：离开欢迎页、亮出文件
                        // 树面板、按需展开侧栏。只在扫描成功时做——失败时
                        // 根目录没换上，停在欢迎页看错误提示才是对的。
                        self.reveal_workspace_sidebar();
                    }
                    Err(e) => self.set_status_error(&format!(
                        "无法打开工作区 {}：{e}",
                        Self::display_name(&root)
                    )),
                }
            }
            // 选择图片（R-04b）：导入资源目录 + 插链接的编排在 images.rs。
            // 取消已在上面静默返回，走不到这里。
            DialogOutcome::PickImage(Ok(path)) => self.apply_picked_image(path),
            // 非取消的错误：说明是「对话框系统不可用」这类问题，
            // 必须提示——静默会让用户以为点了没反应。
            DialogOutcome::OpenFile(Err(e)) => self.report_dialog_failure("打开文件", &e),
            DialogOutcome::SaveFile(Err(e)) => self.report_dialog_failure("另存为", &e),
            DialogOutcome::PickFolder(Err(e)) => self.report_dialog_failure("选择文件夹", &e),
            DialogOutcome::PickImage(Err(e)) => self.report_dialog_failure("选择图片", &e),
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
    ///
    /// 走 `shared::path::file_name_of`：它同时认 `/` 与 `\`，Windows 的
    /// 原始路径（`C:\a\b.md`）不会整条被当成文件名显示。
    pub(super) fn display_name(path: &str) -> &str {
        file_name_of(path)
    }

    /// 报告「对话框本身不可用」这类失败。
    pub(super) fn report_dialog_failure(&mut self, what: &str, error: &AppError) {
        self.set_status_error(&format!("{what}对话框不可用：{error}"));
    }

    /// 关闭全部文档；**有未保存修改时先弹确认**（问题 038）。
    ///
    /// 标签条的「全部关闭」按钮与「关掉最后一个标签」都走这里：
    /// 两条路径都会丢弃未保存修改，风险相同，保护也相同。
    /// 没有脏文档时直接关，不为一个零风险操作弹框。
    pub fn close_all_with_confirm(&mut self) {
        if self.open_document_list().iter().any(|doc| doc.is_dirty) {
            self.request_close_all();
        } else {
            self.close_all_documents();
        }
    }

    /// 请求「全部关闭」确认（登记意图 → 后台确认框 → 每帧取回）。
    ///
    /// 与 [`Self::request_delete`] 同一套路，理由同源：`confirm` 是同步
    /// 阻塞的原生模态框。文案报出脏文档数——用户要凭这条信息决定
    /// 「值是不值得为它再存一次」。
    pub fn request_close_all(&mut self) {
        if self.pending_close_all {
            return;
        }
        let dirty = self
            .open_document_list()
            .iter()
            .filter(|doc| doc.is_dirty)
            .count();
        self.pending_close_all = true;
        let message =
            format!("有 {dirty} 个文档有未保存的修改，「全部关闭」将丢失这些修改。确定继续吗？");
        self.dialogs.spawn(DialogRequest::Confirm { message });
    }

    /// 兑现一次「全部关闭」确认的答复（问题 038）。
    ///
    /// 三种答复与删除确认同义：是 → 执行；否 / 关掉对话框 → 什么都不做
    /// （用户刚明确拒绝，再弹一条「已取消」是废话）；对话框系统不可用 →
    /// 必须提示，否则用户以为关了、实际一个都没关。
    fn apply_close_all_confirm(&mut self, result: &Result<bool, AppError>) {
        self.pending_close_all = false;
        match result {
            Ok(true) => self.close_all_documents(),
            Ok(false) => {}
            Err(AppError::Cancelled) => {}
            Err(e) => self.set_status_error(&format!("未能确认全部关闭：{e}")),
        }
    }

    /// 请求「还原修改」确认（登记意图 → 后台确认框 → 每帧取回，R-04a）。
    ///
    /// 与 [`Self::request_delete`] 同一套路：还原丢弃全部未保存修改，
    /// 不可逆，而 `confirm` 是同步阻塞的原生模态框。文案报出文档名——
    /// 用户要凭它确认「丢的确实是这一份」。
    pub(super) fn request_revert(&mut self, id: DocumentId, path: &FilePath) {
        if self.pending_revert.is_some() {
            return;
        }
        self.pending_revert = Some(id);
        let message = format!(
            "「{}」有未保存的修改，「还原」将丢弃这些修改并从磁盘重新加载。确定继续吗？",
            Self::display_name(path.as_str())
        );
        self.dialogs.spawn(DialogRequest::Confirm { message });
    }

    /// 兑现一次「还原修改」确认的答复（R-04a）。
    ///
    /// 三种答复与删除确认同义：是 → 执行还原；否 / 关掉对话框 → 什么都不做
    /// （用户刚明确拒绝，再弹一条「已取消」是废话）；对话框系统不可用 →
    /// 必须提示，否则用户以为还原了、实际内容没变。
    pub(super) fn apply_revert_confirm(&mut self, result: &Result<bool, AppError>) {
        let Some(id) = self.pending_revert.take() else {
            return;
        };
        match result {
            Ok(true) => self.run_revert(id),
            Ok(false) => {}
            Err(AppError::Cancelled) => {}
            Err(e) => self.set_status_error(&format!("未能确认还原：{e}")),
        }
    }

    /// 请求删除一个工作区条目（**先弹二次确认，答复为「是」才真删**）。
    ///
    /// # 为什么不能在这里直接删
    ///
    /// 原生确认框是模态同步调用（见 [`rustmark_core::ports::dialog::DialogPort::confirm`]），
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
        let name = file_name_of(path);
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
