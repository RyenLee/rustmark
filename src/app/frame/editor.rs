//! 编辑器内部操作：行列换算、编辑同步、格式变换。
use super::*;

mod ime;

impl EditorApp {
    // ================================================================
    // 行列换算（状态栏「行 N, 列 M」用）
    // ================================================================

    /// 当前光标的行列（**1-based**，与原型 `posToLineCol` 一致）。
    ///
    /// # 列为什么是**字符**列而不是字节列
    ///
    /// 一个汉字在 UTF-8 里占 3 字节。按字节算，「你好」的第二个字
    /// 会显示成「列 4」——用户数着自己的光标位置会数出「2」。
    /// 原型用 JS 字符串的 `slice`，天然是 UTF-16 码元口径，
    /// 对 BMP 内的汉字等价于字符列。
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let starts = self.status_line_starts();
        // 光标可能落在非字符边界：`clamp_to` 只按**长度**夹紧，
        // 而程序化设值（跳转、测试）能给出任何一个字节偏移。
        // 直接切片会 panic（`panic = "abort"` 下等于终止进程）。
        let cursor = floor_char_boundary(&self.editor_text, self.editor.cursor);
        let line = crate::presentation::views::scroll_sync::line_of_offset(&starts, cursor);
        let line_start = starts.get(line).copied().unwrap_or(0);
        // 行首也可能是非法边界（理论上不会：行首由 `split('\n')` 推出，
        // 必然落在字符边界上）。夹一次是为了不给「理论上」留 panic。
        let line_start = floor_char_boundary(&self.editor_text, line_start);
        let col = self.editor_text[line_start..cursor].chars().count() + 1;
        (line + 1, col)
    }

    /// 状态栏行列换算用的行起点表：[`line_start_offsets`] 摘掉**文末哨兵**。
    ///
    /// `line_start_offsets` 在文本不以 `\n` 结尾时会补一个「长度哨兵」
    /// （供 y 反查等场景把「文末」解析成下一行）。状态栏不能要它：
    /// 光标停在单行文档的行尾时会被算成「行 2 列 1」——凭空多出的一行。
    /// 文本以 `\n` 结尾时末行起点本就等于长度（真实的空行），保留。
    pub(super) fn status_line_starts(&self) -> Vec<usize> {
        let mut starts =
            crate::presentation::views::scroll_sync::line_start_offsets(&self.editor_text);
        if !self.editor_text.is_empty()
            && !self.editor_text.ends_with('\n')
            && starts.last() == Some(&self.editor_text.len())
        {
            starts.pop();
        }
        starts
    }

    /// 选区的行列范围（有选区时状态栏显示 `行 a, 列 b - 行 c, 列 d`）。
    pub fn selection_line_col(&self) -> Option<((usize, usize), (usize, usize))> {
        let (start, end) = self.editor.selection()?;
        if start == end {
            return None;
        }
        let starts = self.status_line_starts();
        let at = |offset: usize| {
            let offset = floor_char_boundary(&self.editor_text, offset);
            let line = crate::presentation::views::scroll_sync::line_of_offset(&starts, offset);
            let line_start =
                floor_char_boundary(&self.editor_text, starts.get(line).copied().unwrap_or(0));
            let col = self.editor_text[line_start..offset].chars().count() + 1;
            (line + 1, col)
        };
        Some((at(start), at(end)))
    }

    /// 文档行数（状态栏右端「N 行」用）。
    pub fn current_line_count(&self) -> usize {
        self.editor_text.split('\n').count().max(1)
    }

    /// 当前文档统计（状态栏字数、统计浮层共用）。
    ///
    /// 没有活动文档时返回 `None` 而不是全零：全零会被显示成
    /// 「字数 0」，读起来像「文档是空的」而不是「没有文档」。
    pub fn current_stats(&self) -> Option<StatsView> {
        let id = self.active_doc?;
        self.services.documents.get_stats(GetStatsQuery { id }).ok()
    }

    /// 把一次文本编辑（来自 egui 的 diff 或纯变换）写入领域。
    ///
    /// `domain_len` 是**领域当前内容**的字节长度（即编辑发生前的旧文本），
    /// 由调用方传入——不是 `self.editor_text` 的长度。
    ///
    /// # 为什么不能拿 `editor_text` 当钳制基准
    ///
    /// 编辑区（`editor_view::show` 第 5 步）在调用本方法**之前**就把
    /// `editor_text` 拼成了变换后的最终串（省掉一帧回读），此时它的长度
    /// 已经是**新文本**的。而 `edit.byte_start/end` 是相对**旧文本**的字节
    /// 区间。「回车退出列表」这类变换会**把文本变短**（拉起点删掉整个
    /// `4. ` 标记，只插入一个 `\n`）：`byte_end`（旧插入点）会大于新文本
    /// 长度，若按 `editor_text.len()` 钳制，区间终点被砍短，领域层只删掉
    /// 标记的前半个字符——实测症状：在空列表项上回车，`4. ` 变成 `. `
    /// 垃圾残留在行尾，之后的回车再也续不出正常序号。
    ///
    /// 注意：**本方法不再整篇回读领域内容到 `editor_text`**——那是旧实现里
    /// 每敲一个键就触发一次「从领域重读全文并整体覆盖绑定字符串」的操作，
    /// 会迫使 egui 的 `TextEdit` 每帧重新推导光标 / 选区 / 撤销栈，表现为编辑区
    /// 闪烁（见 `editor_view::show` 第 5 步的配套说明）。调用方（编辑区）已把
    /// `editor_text` 同步成与领域一致的最终串，这里只负责：写领域、标记预览需要
    /// 重解析（防抖起点）、记下光标、清除 egui 撤销栈。
    ///
    /// # `transformed`：只在领域真的改写了编辑时才回设光标（V11-14）
    ///
    /// 普通输入（打字、回车换行）里领域应用的就是 egui 给的那次编辑，
    /// egui 自己的光标已经落在正确位置。此时再写一遍 `pending_cursor` 是
    /// **纯粹的多余状态变更**：它会让下一帧的 `apply_pending_editor_state`
    /// 强行覆写 egui 的 `TextEditState`，既制造一次额外的光标重算（闪烁），
    /// 又正好撞上 IME 组合期的守卫——一旦守卫卡住，光标就此失联，
    /// 之后每次回车都插到同一处（用户症状：连按回车只多出一个新行）。
    ///
    /// 列表续行 / 括号配对 / 空项退出列表会改写插入内容或区间，
    /// 此时 egui 的光标确实错了，必须回设。故把该判据显式传进来。
    pub(crate) fn flush_edit(
        &mut self,
        edit: editor_ops::TextEdit,
        domain_len: usize,
        transformed: bool,
    ) {
        let Some(id) = self.active_doc else { return };
        // 用字节区间切片必须合法——`diff_replace` 已保证 `byte_start ≤ byte_end`
        // 且落在旧文本边界内；但若调用方给的基准有误则可能越界，
        // 这里按 `domain_len` 用 `saturating` 防御（越界替换会 panic="abort"
        // 终止进程）。
        let start = edit.byte_start.min(domain_len);
        let end = edit.byte_end.min(domain_len);
        let range = TextRange::new(ByteOffset::new(start), ByteOffset::new(end));
        let _ = self.services.documents.replace_range(ReplaceRangeCommand {
            id,
            range,
            text: edit.inserted,
        });
        // 文本被改写 = 内容可能已变 → 重置预览防抖的计时起点。
        // 刻意放在这里（而非 `refresh_text` 内部）：编辑入口有十几处，漏任何一处
        // 都会让防抖退化；全部调用点都是命令 / 事件驱动的（不是每帧）。
        let now_ms = self.services.markdown.now().millis();
        self.preview.mark_edited(now_ms);
        // 只在领域真的改写了这次编辑时才回设光标（见上文 `transformed`）。
        if transformed {
            self.pending_cursor = Some((
                edit.cursor_char,
                edit.anchor_char.unwrap_or(edit.cursor_char),
            ));
        }
        self.clear_undoer_next = true;
    }

    /// 替换源码的一段字节区间（S6-05：预览区点击任务复选框回写）。
    ///
    /// 与 [`Self::flush_edit`] 的区别：本方法**不移动光标也不设 pending**——
    /// 用户此刻的焦点在预览区，编辑区光标应保持原位。
    pub(crate) fn replace_source(&mut self, byte_start: usize, byte_end: usize, text: &str) {
        // 这里直接把 `id` 取出来用，避免后面再 `expect` 一次
        // （规范 §四-15 禁用 `expect()`，V11-12 裁决）。
        let Some(id) = self.active_doc else {
            return;
        };
        let len = self.editor_text.len();
        let start = byte_start.min(len);
        let end = byte_end.min(len).max(start);
        let _ = self.services.documents.replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
            text: text.to_string(),
        });
        self.refresh_text();
        self.clear_undoer_next = true;
    }

    /// 当前文档所在目录（用于解析预览区图片的相对路径）。
    ///
    /// 未保存的草稿没有目录 → 返回 `None`，此时相对路径图片无法解析，
    /// 预览显示占位而非报错。
    pub(crate) fn document_dir(&self) -> Option<std::path::PathBuf> {
        let raw = self.current_document_path();
        if raw.is_empty() || raw == "未命名文档" {
            return None;
        }
        let path = std::path::Path::new(&raw);
        path.parent().map(|p| p.to_path_buf())
    }

    /// 整篇替换（缩进 / 反缩进 / 移动行这类**全文本变换**走这里）。
    pub(super) fn replace_all(&mut self, new_text: String) {
        let Some(id) = self.active_doc else { return };
        let range = TextRange::new(ByteOffset::new(0), ByteOffset::new(self.editor_text.len()));
        let _ = self.services.documents.replace_range(ReplaceRangeCommand {
            id,
            range,
            text: new_text,
        });
        self.refresh_text();
        self.clear_undoer_next = true;
    }

    /// 撤销一步（领域权威）。
    pub fn undo_edit(&mut self) {
        let Some(id) = self.active_doc else { return };
        let _ = self.services.documents.undo(UndoCommand { id });
        self.sync_cursor_from_domain();
        self.clear_undoer_next = true;
    }

    /// 重做一步（领域权威）。
    pub fn redo_edit(&mut self) {
        let Some(id) = self.active_doc else { return };
        let _ = self.services.documents.redo(RedoCommand { id });
        self.sync_cursor_from_domain();
        self.clear_undoer_next = true;
    }

    /// 撤销/重做后把 egui 光标对齐到领域光标（含选区）。
    pub(super) fn sync_cursor_from_domain(&mut self) {
        let Some(id) = self.active_doc else { return };
        // 必须先回读文本：撤销 / 重做改的是**领域内容**，
        // 而 `editor_text` 是它的 UI 副本。不同步的话，
        // 撤销后编辑区仍显示撤销前的文本，下一次输入会把它覆盖回去
        // —— 表现为「Ctrl+Z 之后内容又回来了」。
        // （S6-07 之前不容易发现：那时只有输入类编辑，
        //  撤销后紧接着的操作通常就是输入，两者恰好抵消。）
        self.refresh_text();
        if let Ok(view) = self
            .services
            .documents
            .get_document(GetDocumentQuery { id })
        {
            let cursor = editor_ops::byte_to_char(&self.editor_text, view.cursor);
            let anchor = view
                .selection
                .map(|r| editor_ops::byte_to_char(&self.editor_text, r.start))
                .unwrap_or(cursor);
            self.pending_cursor = Some((cursor, anchor));
            self.editor.move_cursor(cursor); // 先把 UI 状态也对齐，避免后续读到的偏移错位
        }
    }

    /// 选区覆盖到的字符区间（无选区为光标处单点）。
    pub(super) fn selection_chars(&self) -> (usize, usize) {
        match self.editor.selection() {
            Some((s, e)) => (
                editor_ops::byte_to_char(&self.editor_text, s),
                editor_ops::byte_to_char(&self.editor_text, e),
            ),
            None => {
                let c = editor_ops::byte_to_char(&self.editor_text, self.editor.cursor);
                (c, c)
            }
        }
    }

    /// 缩进选区（或光标所在行）。
    ///
    /// 走应用层 `format`（S6-07）而非本地拼接：缩进规则归领域
    /// `EditingService` 所有，这里只负责把「字符选区」翻译成字节区间。
    pub fn indent_selection(&mut self) {
        self.run_format(FormatKind::Indent);
    }

    /// 反缩进选区（或光标所在行）。
    pub fn outdent_selection(&mut self) {
        self.run_format(FormatKind::Outdent);
    }

    /// 执行一个格式动作（S6-07）。
    ///
    /// 这是**三入口的唯一汇聚点**：工具栏按钮、快捷键分发、命令面板
    /// 都以 `UiAction` 为参数进入 [`EditorApp::execute`]，再由它转到此处。
    /// 因此「三入口一致」不是靠三处都写对，而是**按构造**成立。
    pub fn apply_format(&mut self, action: UiAction) {
        let kind = match action {
            UiAction::Bold => FormatKind::Bold,
            UiAction::Italic => FormatKind::Italic,
            UiAction::Strikethrough => FormatKind::Strikethrough,
            UiAction::InlineCode => FormatKind::InlineCode,
            UiAction::InsertLink => FormatKind::InsertLink,
            UiAction::CodeBlock => FormatKind::InsertCodeBlock,
            UiAction::InsertTable => FormatKind::InsertTable,
            UiAction::ToggleQuote => FormatKind::Quote,
            UiAction::ToggleList => FormatKind::BulletList,
            UiAction::ToggleOrderedList => FormatKind::OrderedList,
            UiAction::PromoteHeading => FormatKind::PromoteHeading,
            UiAction::DemoteHeading => FormatKind::DemoteHeading,
            UiAction::ToggleTask => FormatKind::ToggleTask,
            // 缩进已由 `Indent` / `Outdent` 动作直接处理，不走这里。
            _ => return,
        };
        self.run_format(kind);
    }

    /// 调用应用层格式用例并落地结果（新文本 + 选区）。
    pub(super) fn run_format(&mut self, kind: FormatKind) {
        let Some(id) = self.active_doc else {
            return;
        };
        // UI 用字符索引、应用层用字节偏移，交接处必须换算
        // （`İ`、emoji、组合字符都会让两者不等）。
        let (s, e) = self.selection_chars();
        let range = TextRange::new(
            ByteOffset::new(editor_ops::char_to_byte(&self.editor_text, s)),
            ByteOffset::new(editor_ops::char_to_byte(&self.editor_text, e)),
        );

        let Ok(result) = self.services.documents.format(FormatCommand {
            id,
            kind,
            range,
            indent_unit: self.tab_unit(),
        }) else {
            // 文档已被关闭等异常：静默放弃，不打断当前帧。
            // 格式是增强功能，不该让整个编辑器崩掉。
            return;
        };
        if !result.changed {
            return; // 无效果编辑：不写文本、不重排高亮与预览
        }

        // `format` 已经把改动写进领域，这里只需把内容与选区同步到 UI。
        self.editor_text = result.text;
        let sel = &result.selection;
        let ns = editor_ops::byte_to_char(&self.editor_text, sel.start.min(self.editor_text.len()));
        let ne = editor_ops::byte_to_char(&self.editor_text, sel.end.min(self.editor_text.len()));
        self.editor.cursor = editor_ops::char_to_byte(&self.editor_text, ns);
        self.editor.anchor = Some(editor_ops::char_to_byte(&self.editor_text, ne));
        self.pending_cursor = Some((ns, ne));
        // 格式改动同样进领域撤销栈，故 egui 内置栈必须让位（与普通编辑一致）。
        self.clear_undoer_next = true;
    }

    /// 上移/下移光标所在行（`dir = -1` 上移，`+1` 下移）。
    pub fn move_line(&mut self, dir: i8) {
        let cursor = editor_ops::byte_to_char(&self.editor_text, self.editor.cursor);
        if let Some((new_text, new_cursor)) = editor_ops::move_line(&self.editor_text, cursor, dir)
        {
            self.replace_all(new_text);
            self.pending_cursor = Some((new_cursor, new_cursor));
        }
    }
}
