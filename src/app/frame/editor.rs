//! 编辑器内部操作：行列换算、编辑同步、格式变换、IME 守卫。
use super::*;

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

    /// 把上一帧领域编辑留下的「重设光标 / 清撤销栈」意图落到 egui 内部状态。
    ///
    /// 必须在 `TextEdit::show` **之前**调用（写入的状态会在本帧 show 时被读走）。
    /// 放在编辑区视图顶部，由视图持 `ctx` 调用。
    ///
    /// # IME 组合期守卫（为什么这里要扫一遍输入事件）
    ///
    /// egui 0.33 的 `Preedit` 处理会把预编辑串**真实写进文本缓冲区**
    /// （`text.insert_text_at`——「组合期文本不进缓冲区」的旧假设不成立），
    /// 并靠 `state.cursor` 保持着「(组合锚点, 预编辑串尾)」这一选区：
    /// 下一次 `Preedit` 靠它删掉旧串，`Commit` 还会校验
    /// `cursor_range.secondary`（锚点）是否仍在组合起点，不在则**整个
    /// 提交被静默忽略**。而本方法回设的 `pending_cursor` 是折叠光标
    /// （secondary = 串尾），恰好打掉这个选区——实测症状：
    /// 预编辑串逐帧累积（"s"→"ssa"→"ssasadsadaf"），候选词确认后中文
    /// 根本进不来，之后每次回车只是把残渣往下推。
    ///
    /// 故组合期间（以及恰好携带 IME 事件的帧）**冻结**两个动作：
    /// 不回设光标、不清撤销栈；组合结束（`Commit` / `Disabled`）后的
    /// 下一帧恢复正常回设——彼时 `pending_cursor` 已被提交帧的
    /// `flush_edit` 覆写为「中文之后」的正确位置。
    ///
    /// # 为什么冻结必须**有界**（V11-14：连按回车只出一个新行）
    ///
    /// 旧实现用粘性标志 `ime_composing` 表示组合期，只有 `Commit`/`Disabled`
    /// 能清它。真实输入法在编辑区获得焦点时就发 `Ime::Enabled`，而 winit 的
    /// `Disabled` 只在失焦时发——切换输入法、直接关候选窗、焦点落到浮层等
    /// 路径都可能收不到。标志一旦卡在 `true`：
    ///
    /// ```text
    /// 回车 → flush_edit 写 pending_cursor → 守卫冻结，光标不更新
    ///      → 领域多了一个 \n，光标却仍在旧位置
    /// 再回车 → egui 把 \n 插到旧位置 → 再 flush_edit → 又是旧位置……
    /// ```
    ///
    /// 于是**连按回车只有第一个生效**，界面只多出一个新行；直到用户敲了别的
    /// 字符，`prev != editor_text` 的判定路径把它「带出来」，观感就是
    /// 「必须再输入字符才会真正换行」。实测逐字复现（见
    /// `enter_diag_test::diag_enter_after_ime_enabled_never_disabled`：
    /// 第 2、3 次回车后 `domain` 纹丝不动停在 `"hello\n"`）。
    ///
    /// 改为**有界窗口**后，组合期最长只冻结 [`IME_GUARD_MS`] 毫秒：
    /// 正常中文输入（逐字母 `Preedit`、几十到几百毫秒内 `Commit`）完全
    /// 落在窗口内、行为不变；而「标志卡死」在结构上不再可能——超时自动恢复。
    ///
    /// # 超时时还必须**顺带复位 egui 自己的 IME 标志**（这才是回车被吞的真凶）
    ///
    /// 光有「自己的守卫解冻」是不够的：`Ime::Enabled` 会让 egui 的
    /// `TextEdit` 把 `state.ime_enabled` 置真，此后它按「候选窗开着」的
    /// 假设处理键盘——`remove_ime_incompatible_events` 会剔除它认为
    /// 「IME 期间会打架」的按键（`repeat: true` 的全部按键 +
    /// Backspace/方向键）。该标志**只能**由 `Preedit` / `Commit` /
    /// `Disabled` 或焦点变化复位，而 `Disabled` 在真实环境里常常收不到。
    ///
    /// 于是窗口到期时，除了解冻自己的守卫，还要给 egui **补一个**
    /// `Ime::Disabled`——语义上完全正确（应用确实判定「组合已结束」），
    /// 且这是 egui 唯一公开的复位入口（`ime_enabled` 是 `pub(crate)`，
    /// `request_focus` 对已聚焦控件幂等、不产生 `gained_focus`，都试过了）。
    /// 补发后实测连按回车立刻恢复正常（见
    /// `enter_diag_test::diag_synthetic_disabled_resets`）。
    ///
    /// # Commit 不得续期冻结窗（2026-10-11「行号递增但内容不换行」复报的根因）
    ///
    /// 旧实现把 `Commit` 与 `Preedit` 同列为「组合进行中」，收到 `Commit` 会把
    /// [`IME_GUARD_MS`] 保护窗**再续 800ms**。但 `Commit` 的语义是「组合结束」，
    /// egui 在它的处理分支里已经把 `ime_enabled` 置回 false 并删掉了预编辑
    /// 选区。续期的后果：
    ///
    /// ```text
    /// 中文提交 → 守卫被 Commit 续到 +800ms
    /// 用户立刻在列表项上回车（transformed=true）→ flush_edit 写下
    ///     pending_cursor（"2. " 之后），但守卫冻结、光标回不去
    /// 下一帧 egui 光标仍停在 "\n" 之后、"2. " 之前
    /// 再回车 → "\n" 插到 "2. " 前面 → transform_insert 又续一个标记
    ///        → "1. 中文\n2. \n2. "…… 标记/空行逐次堆积
    /// ```
    ///
    /// 症状正是「行号正常递增，光标附近却不见新行」。修复：`Commit` 与
    /// `Disabled` 同样**立即**解冻（仅保留事件帧当帧的冻结，供 egui 的
    /// Commit 处理器依赖未碰的选区），组合结束后光标回设最迟下一帧执行。
    pub fn apply_pending_editor_state(&mut self, ctx: &egui::Context) {
        let now_ms = self.services.markdown.now().millis();
        // 本帧 IME 事件的四分类：
        //   · `Preedit` → 组合**确实在进行**：缓冲区里有实打实的预编辑串，
        //     egui 正靠那个选区删旧换新，我们的守卫必须冻结。
        //   · `Commit`/`Disabled` → 组合**收尾**，立即解冻。`Commit` 不得续期
        //     冻结窗——它不是「组合进行中」，续期会让提交后 800ms 内所有
        //     transformed 编辑的光标回设被冻结，见函数文档的堆积推演。
        //   · `Enabled` → 只表示「egui 已把 `ime_enabled` 置真」，本身不代表有
        //     组合在进行，但它**会**改变 egui 处理键盘的方式，故要单独跟踪
        //     （见 `egui_ime_armed`）。
        let (ime_preedit_now, ime_commit_now, ime_ended, ime_enabled_now) = ctx.input(|i| {
            let mut preedit = false;
            let mut commit = false;
            let mut ended = false;
            let mut enabled = false;
            for event in &i.events {
                if let egui::Event::Ime(ime) = event {
                    match ime {
                        egui::ImeEvent::Preedit(_) => preedit = true,
                        egui::ImeEvent::Commit(_) => commit = true,
                        egui::ImeEvent::Disabled => ended = true,
                        egui::ImeEvent::Enabled => enabled = true,
                    }
                }
            }
            (preedit, commit, ended, enabled)
        });
        // ---- 守卫侧：组合期是否有界 ----
        //   · 收到 Commit/Disabled → 立即解除冻结（组合确实结束了）；
        //   · 收到 Preedit → 续上/开启一个 IME_GUARD_MS 的保护窗；
        //   · 只收到 Enabled 或什么都没收到 → 不进入（或退出）组合期。
        // 于是「守卫标志永久为 true」这条路径在结构上不再存在；且 Commit 不再
        // 续期——组合结束后 `pending_cursor` 的回设最迟下一帧就能执行。
        if ime_ended || ime_commit_now {
            self.ime_composing = false;
            self.ime_composing_until_ms = 0;
        } else if ime_preedit_now {
            self.ime_composing = true;
            self.ime_composing_until_ms = now_ms.saturating_add(IME_GUARD_MS);
        } else if self.ime_composing && now_ms >= self.ime_composing_until_ms {
            self.ime_composing = false;
            self.ime_composing_until_ms = 0;
        }
        // ---- egui 侧：复位它的 `ime_enabled` ----
        // 这个标志由 `Ime::Enabled` 置真，**同样只由** `Preedit`/`Commit`/
        // `Disabled`/焦点变化复位。`Disabled` 收不到时它就一直为真，
        // 于是 egui 按「候选窗开着」剔除按键（`remove_ime_incompatible_events`），
        // 回车被吞——这才是「连按回车只多出一个新行」的直接原因。
        //
        // 复位时机刻意取得**很激进**（[`IME_ARM_MS`]，比组合期窗口短一个量级）：
        // 真实组合中，`Enabled` 之后**紧接着**就是第一个 `Preedit`
        // （winit 在 `Preedit` 分支里也会调 `ime_event_enable`），二者间隔
        // 不到一帧（~16ms）。所以只要「`Enabled` 之后短时间没有任何
        // `Preedit`」，就说明并没有组合在进行——这是判据，不是猜测。
        // 用长窗口反而会重蹈「标志卡住」的覆辙：800ms 内用户的每一次回车
        // 都会被吞，而用户连按回车的间隔恰好在这个量级。
        //
        // 误伤成本也可控：万一真在组合中而我们补了 `Disabled`，egui 只是把
        // `ime_enabled` 提前清掉；下一个 `Preedit` 会重新置位并继续组合，
        // 最多丢一次预编辑串的锚点——而这正是旧实现**常态性**发生的损害。
        //
        // `Commit` 同样解除 armed：egui 在 Commit 分支里自己就把 `ime_enabled`
        // 置回了 false（text_edit/builder.rs 的 `ImeEvent::Commit` 臂），
        // 之后无需、也不应再补发 Disabled。
        let mut tell_egui_ime_over = false;
        if ime_ended || ime_commit_now {
            self.egui_ime_armed = false;
        } else if ime_preedit_now || ime_enabled_now {
            // 刚 `Enabled`：给一个短窗口等紧随其后的 `Preedit`。
            self.egui_ime_armed = true;
            self.egui_ime_armed_until_ms = now_ms.saturating_add(IME_ARM_MS);
        } else if self.egui_ime_armed && now_ms >= self.egui_ime_armed_until_ms {
            self.egui_ime_armed = false;
            tell_egui_ime_over = true;
        }
        // 给 egui 补一个 `Disabled`。放在事件流**尾部**：本帧若同时有
        // `Preedit`，必须让它先被处理（`Preedit` 会重新置 `ime_enabled = true`），
        // 否则这个 `Disabled` 会被随后的 `Preedit` 覆盖回去。
        if tell_egui_ime_over {
            ctx.input_mut(|i| i.events.push(egui::Event::Ime(egui::ImeEvent::Disabled)));
        }
        // Enabled 之后尚未见到任何组合事件（Preedit/Commit）的窗口期内，
        // egui 的 `ime_enabled` 为真，`remove_ime_incompatible_events` 会剔除
        // 全部 `repeat: true` 的按键——它的假设是「候选窗开着」。但我们知道
        // **没有组合在进行**（armed 判据的存在意义），此时 repeat 的回车只会
        // 来自「上一帧 release 丢失后 `keys_down` 残留」（Windows 上候选窗或
        // 其他窗口抢焦点时会这样），或用户真的按住回车——两种情况用户都
        // 期待换行。把 repeat 抹平，让 egui 当作一次普通按下处理。
        // 只抹 Enter、且仅在「无组合」窗口内：组合期（候选窗真开着）的
        // repeat 剔除是 egui 的正当防护，不能碰。
        if !ime_preedit_now && !ime_commit_now && !ime_ended && self.egui_ime_armed {
            ctx.input_mut(|i| {
                for event in &mut i.events {
                    if let egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        repeat,
                        ..
                    } = event
                    {
                        *repeat = false;
                    }
                }
            });
        }
        // 组合帧自身也要冻结：`Preedit`/`Commit` 处理器在这一帧稍后执行，
        // 依赖的正是那个未被我们碰过的选区。
        //
        // 注意：**刚补发的 `Disabled` 不参与冻结判定**——它是我们自己
        // 判定「组合已结束」后才补的，若把它算进去会让本帧凭空多冻结一次，
        // 回车又要晚一帧才生效。
        let ime_guard = ime_preedit_now || ime_commit_now || self.ime_composing;

        if self.clear_undoer_next
            && !ime_guard
            && let Some(mut state) = egui::TextEdit::load_state(ctx, Self::editor_id())
        {
            self.clear_undoer_next = false;
            state.clear_undoer();
            state.store(ctx, Self::editor_id());
        }
        if !ime_guard
            // 注意 `&&` 的顺序：必须先拿到 `state` 再 `take()`。
            // 写成 `take()` 在前时，`load_state` 返回 `None`（TextEdit 尚未
            // 建立状态的那一帧）会把 `pending_cursor` 白白吞掉，光标从此失联。
            && let Some(mut state) = egui::TextEdit::load_state(ctx, Self::editor_id())
            && let Some((cursor, anchor)) = self.pending_cursor.take()
        {
            let primary = egui::text::CCursor {
                index: cursor,
                prefer_next_row: false,
            };
            let secondary = egui::text::CCursor {
                index: anchor,
                prefer_next_row: false,
            };
            state.cursor.set_char_range(Some(egui::text::CCursorRange {
                primary,
                secondary,
                h_pos: None,
            }));
            state.store(ctx, Self::editor_id());
        }
    }
}
