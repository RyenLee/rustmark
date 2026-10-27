//! 编辑器内部操作：行列换算、编辑同步、格式变换。
use super::*;
use crate::presentation::state::editor_ui_state::SpellSuggestMenu;

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
    ///
    /// 自取行索引的便捷入口（单点调用与测试用）；一帧内多处换算请用
    /// [`Self::cursor_line_col_in`] 共用一次扫描（问题 053）。
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let index = self.line_index();
        self.cursor_line_col_in(&index)
    }

    /// 同上，但由调用方提供行索引。
    pub fn cursor_line_col_in(&self, index: &LineIndex) -> (usize, usize) {
        let starts = index.status_starts();
        // 光标可能落在非字符边界：`clamp_to` 只按**长度**夹紧，
        // 而程序化设值（跳转、测试）能给出任何一个字节偏移。
        // 直接切片会 panic（`panic = "abort"` 下等于终止进程）。
        let cursor = floor_char_boundary(&self.editor_text, self.editor.cursor);
        let line = index.line_of_offset(cursor);
        let line_start = starts.get(line).copied().unwrap_or(0);
        // 行首也可能是非法边界（理论上不会：行首由 `split('\n')` 推出，
        // 必然落在字符边界上）。夹一次是为了不给「理论上」留 panic。
        let line_start = floor_char_boundary(&self.editor_text, line_start);
        let col = self.editor_text[line_start..cursor].chars().count() + 1;
        (line + 1, col)
    }

    /// 选区的行列范围（有选区时状态栏显示 `行 a, 列 b - 行 c, 列 d`）。
    pub fn selection_line_col(&self) -> Option<((usize, usize), (usize, usize))> {
        let index = self.line_index();
        self.selection_line_col_in(&index)
    }

    /// 同上，但由调用方提供行索引。
    pub fn selection_line_col_in(
        &self,
        index: &LineIndex,
    ) -> Option<((usize, usize), (usize, usize))> {
        let (start, end) = self.editor.selection()?;
        if start == end {
            return None;
        }
        let starts = index.status_starts();
        let at = |offset: usize| {
            let offset = floor_char_boundary(&self.editor_text, offset);
            let line = index.line_of_offset(offset);
            let line_start =
                floor_char_boundary(&self.editor_text, starts.get(line).copied().unwrap_or(0));
            let col = self.editor_text[line_start..offset].chars().count() + 1;
            (line + 1, col)
        };
        Some((at(start), at(end)))
    }

    /// 文档行数（状态栏右端「N 行」用）。
    pub fn current_line_count(&self) -> usize {
        self.line_index().count()
    }

    /// 当前文本的行索引（**一遍全文扫描**的产物，见 [`LineIndex`]）。
    ///
    /// # 缓存键与失效：`(活动文档, 领域修订版本, 文本字节长度)`
    ///
    /// 缓存能成立的前提是「`editor_text` 是领域内容的忠实视图」——这正是本项目的
    /// 核心纪律（见 [`Self::refresh_text`]）。于是领域修订版本就是「文本已变」的
    /// **权威信号**，不必再在五处 `editor_text` 写入点上接线一个「脏」标志，
    /// 也就少了一类漏接线即静默失效的风险（问题 038 的教训）。
    ///
    /// 第三个分量是长度，作为两道保险：区分「都还没写过领域的空白态」，
    /// 并挡住「`editor_text` 已改而领域命令尚未发出」的那一帧窗口
    /// （帧内窗口见 `editor_view::show` 第 5 步；状态栏读在它之前，故不受影响）。
    ///
    /// # 为什么是 `RefCell` 而不是 `&mut`
    ///
    /// 取用它的入口（状态栏的 `build_model`、`cursor_line_col`）都是 `&self`，
    /// 而命中/填充缓存要写回。`EditorApp` 只活在 UI 线程（`eframe::App`），
    /// 不存在并发共享。返回 `Rc` 而非 `&` 借用是为了让调用方**不持有借用**——
    /// 否则一帧内两处取用会互相锁死可变借用。
    pub fn line_index(&self) -> std::rc::Rc<LineIndex> {
        let key = self.line_index_key();
        let mut slot = self.line_index_cache.borrow_mut();
        if let Some((cached_key, index)) = slot.as_ref()
            && *cached_key == key
        {
            return index.clone();
        }
        let index = std::rc::Rc::new(LineIndex::new(&self.editor_text));
        *slot = Some((key, index.clone()));
        index
    }

    /// 行索引缓存的键（见 [`Self::line_index`]）。
    fn line_index_key(&self) -> (Option<DocumentId>, u64, usize) {
        let revision = self.active_doc.and_then(|id| {
            self.services
                .registry
                .read()
                .ok()
                .and_then(|registry| registry.revision_of(id))
        });
        (
            self.active_doc,
            revision.unwrap_or(0),
            self.editor_text.len(),
        )
    }

    /// 当前文档统计（状态栏字数、统计浮层共用）。
    ///
    /// 没有活动文档时返回 `None` 而不是全零：全零会被显示成
    /// 「字数 0」，读起来像「文档是空的」而不是「没有文档」。
    pub fn current_stats(&self) -> Option<StatsView> {
        let id = self.active_doc?;
        self.services.documents.get_stats(GetStatsQuery { id }).ok()
    }

    /// 状态栏统计（防抖版；建议一 / 问题 039）。
    ///
    /// 与 [`rustmark_core::application::markdown::service::span_cache::HighlightPace`]
    /// 同源：打字期不每击键整篇重算（`compute_stats` 在 1MB 上 ≈ 9.5ms，
    /// 见 `docs/metrics.md` §3.2.5），而是沿用「上一次静止时算出的结果」，
    /// 直到文档**静止满 [`super::STATS_DEBOUNCE_MS`]** 才刷新。
    ///
    /// 与高亮的区别：高亮首次见某篇要立刻上色；这里**首次见某篇也要立刻算**
    /// （否则打开文档后字数先显 0、150ms 后才跳真值，同样 flicker）。
    /// 故 `doc` 改变（或首算）时直接重算，不走防抖。
    ///
    /// 统计浮层（`stats_view`）走 [`Self::current_stats`]（点击时才算、即时），
    /// 本方法只服务每帧的状态栏。
    pub fn current_stats_for_status_bar(&mut self) -> Option<StatsView> {
        let id = self.active_doc?;
        let revision = self
            .services
            .registry
            .read()
            .ok()
            .and_then(|registry| registry.revision_of(id))
            .unwrap_or(0);
        let now = self.services.markdown.now();

        // 首次 / 切换文档：立刻算，不防抖（避免开文先显 0）。
        if self.stats_pace.doc != Some(id) {
            let stats = self
                .services
                .documents
                .get_stats(GetStatsQuery { id })
                .ok()?;
            self.stats_pace.doc = Some(id);
            self.stats_pace.computed_revision = revision;
            self.stats_pace.last_seen_revision = revision;
            self.stats_pace.last_change_at = now;
            self.stats_pace.cached = Some(stats.clone());
            return Some(stats);
        }

        // 同一文档，且已算到当前版本：直接返回缓存。
        if self.stats_pace.computed_revision == revision {
            return self.stats_pace.cached.clone();
        }

        // 版本变了：只在首次见到这个新版本时刷新「变化时刻」。
        if self.stats_pace.last_seen_revision != revision {
            self.stats_pace.last_seen_revision = revision;
            self.stats_pace.last_change_at = now;
        }

        // 静止满窗口才重算；否则沿用上次结果（打字期不闪烁、不重算）。
        if now.elapsed_ms_since(self.stats_pace.last_change_at) >= super::STATS_DEBOUNCE_MS {
            let stats = self
                .services
                .documents
                .get_stats(GetStatsQuery { id })
                .ok()?;
            self.stats_pace.computed_revision = revision;
            self.stats_pace.cached = Some(stats.clone());
            Some(stats)
        } else {
            // 登记「窗口到期」这一刻（T11）：没有它，用户停手后不再有新帧，
            // 字数会一直停在上一次的值，直到碰巧动一下鼠标。编辑器失焦时
            // 连光标闪烁的兜底帧也没有，缺陷会一直可见。
            self.scheduler.arm(
                super::Deadline::StatsDebounce,
                self.stats_pace
                    .last_change_at
                    .millis()
                    .saturating_add(super::STATS_DEBOUNCE_MS),
            );
            self.stats_pace.cached.clone()
        }
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
    ///
    /// 走 [`Self::current_document_file`]（轻量摘要）：此前经
    /// `current_document_path()` 的**哨兵字符串**（「未命名文档」）
    /// 反推「有没有路径」——用展示文案当判据，一旦文案变了就误判。
    pub(crate) fn document_dir(&self) -> Option<std::path::PathBuf> {
        let path = self.current_document_file()?;
        path.parent()
            .as_ref()
            .map(|p| std::path::PathBuf::from(p.as_str()))
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

    /// 在光标处插入一段文本（符号面板 / 工具栏插入结构共用）。
    ///
    /// 走领域 `insert_text` 而非直接改 `editor_text`，理由与
    /// S6-07 的「三入口一致」同源：直接改 UI 文本会绕过一次撤销点，
    /// 让用户「插入一个 emoji 之后按 Ctrl+Z 什么也没撤销」。
    ///
    /// # 插入点必须取 **UI 光标**而非领域光标
    ///
    /// 两者是不同的坐标系：领域记的是**上一次领域命令后**的光标，
    /// UI 记的是 egui `TextEdit` 当前画出来的那个。而大纲跳转
    /// （[`Self::goto_byte_offset`]）、查找命中（`reveal_match`）
    /// 都只改 UI 光标——用户看到光标跳到了第 40 行，
    /// 领域那边还停在第 3 行。此时按「插入符号」若读领域光标，
    /// 内容会插到用户**看不见**的位置。
    /// （`move_line` / `run_format` 同样取 UI 光标，规则一致。）
    ///
    /// 光标停在插入内容**之后**：停在内容里意味着用户接着打字
    /// 会把文字插进代码块标记中间——这是插入类操作最常见的投诉。
    ///
    /// 有选区时**替换选区**：用户选中一段文字再插链接，
    /// 得到「选中文字链接选中文字」是几乎所有编辑器的行为，
    /// 也符合「插入」这个词的含义。
    pub fn insert_at_cursor(&mut self, text: &str) -> AppResult<()> {
        let Some(id) = self.ensure_document() else {
            return Ok(());
        };
        // 选区优先：无选区时 `selection()` 返回 `None`，退回光标。
        // 选区优先：无选区时 `selection()` 返回 `None`，退回光标。
        let (start, end) = self
            .editor
            .selection()
            .unwrap_or((self.editor.cursor, self.editor.cursor));
        let end = end.max(start);
        if start != end {
            self.services.documents.replace_range(ReplaceRangeCommand {
                id,
                range: TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
                text: text.to_string(),
            })?;
        } else {
            self.services.documents.insert_text(InsertTextCommand {
                id,
                offset: start,
                text: text.to_string(),
            })?;
        }
        self.refresh_text();
        let after = start + text.len();
        // 领域光标也要跟上：否则下一次插入仍会用旧位置（见上）。
        self.editor.set_selection(after, after);
        self.pending_cursor = Some((
            editor_ops::byte_to_char(&self.editor_text, after),
            editor_ops::byte_to_char(&self.editor_text, after),
        ));
        self.clear_undoer_next = true;
        Ok(())
    }

    /// 缩进单位（空格），来自设置 `tab_size`。
    pub(super) fn tab_unit(&self) -> String {
        " ".repeat(self.settings().editor.tab_size.max(1) as usize)
    }

    /// 编辑区 `TextEdit` 的固定 `Id`（用于读写 egui 内部光标 / 撤销栈状态）。
    ///
    /// R-03 B1：实现已迁至 `rustmark_ui::presentation::state::editor_ui_state::editor_id`
    /// （它是**无 `self` 的关联函数**，trait 方法没法在 `dyn` 上调），
    /// 本方法保留为转发，`EditorApp::editor_id()` 路径**逐字不变**。
    pub fn editor_id() -> egui::Id {
        crate::presentation::state::editor_ui_state::editor_id()
    }

    /// 从领域回读当前文档内容到 `editor_text`，并夹紧光标/选区、失效高亮缓存。
    ///
    /// 这是「领域是内容单一真相」的物理保证：每次领域命令后都调用它，
    /// `editor_text` 永远等于 `get_content` 的返回值，二者无法漂移。
    pub fn refresh_text(&mut self) {
        if let Some(id) = self.active_doc
            && let Ok(text) = self
                .services
                .documents
                .get_content(GetContentQuery { id, range: None })
        {
            self.editor_text = text;
            self.editor.clamp_to(self.editor_text.len());
        }
        // 文本被重新拉取 = 内容可能已变 → 重置预览防抖的计时起点。
        //
        // 刻意放在**这里**而不是各个编辑入口：编辑入口有十几处
        // （编辑区输入、插入、替换、整篇变换、查找替换、领域命令…），
        // 漏任何一处都会让防抖退化成「每 150ms 重解析一次」的节流。
        // 全部调用点都是命令 / 事件驱动的（不是每帧），故不会把计时无限推后。
        let now_ms = self.services.markdown.now().millis();
        self.preview.mark_edited(now_ms);
    }

    /// 由视图侧设置编辑区绑定串（`TextEdit` 直接绑定的 `String`）。
    ///
    /// 视图在「用户本帧真的改了文本」时，就地拼出与领域一致的最终串写回
    /// `editor_text`，再发领域命令——这样 [`Self::flush_edit`] 就不必再从
    /// 领域整篇回读（那次回读会在每一帧把绑定给 egui 的字符串整体替换一遍，
    /// 迫使 `TextEdit` 每敲一个键就重新推导光标 / 选区 / 撤销栈，表现为编辑区
    /// 闪烁）。R-12 把「视图不得直写 `app.editor_text`」机械化，故这条改由
    /// 方法入口收敛（见 `docs/architecture-constraints.md`）。
    pub(crate) fn set_editor_text(&mut self, text: String) {
        self.editor_text = text;
    }

    /// 当前活动文档 ID（**测试用**）。
    ///
    /// `active_doc` 是 `pub(crate)`，集成测试（`tests/` 下）读不到。
    /// 预览链路的探针用例需要它来直接发起 `render_preview` 查询，
    /// 以便断言「换行结构在解析后仍被保留」——那是用户可见缺陷
    /// （预览缺行）的最后一环，只能在应用层验证。
    pub fn active_doc_for_probe(&self) -> DocumentId {
        // 探针用例在 `focus_at_end` 之后调用，此时活动文档必然已建立。
        // 仍用 `unwrap_or_else` 而非 `expect`：规范 §四-15 禁止 `expect()`（V11-12）。
        self.active_doc.unwrap_or_else(|| DocumentId::from_u64(0))
    }

    /// 读取当前文档的完整内容（**从领域**，非 `editor_text` 副本）。
    ///
    /// 存在的理由：集成测试需要断言「三入口执行后文档内容字节一致」，
    /// 而 `active_doc` 与 `editor_text` 都是 `pub(crate)`，测试拿不到。
    /// 与其让测试去猜内部可见性，不如把「读当前内容」作为正式能力暴露——
    /// 它本来也是导出、查找替换、状态栏字数统计都要用的东西。
    pub fn current_content(&self) -> Option<String> {
        let id = self.active_doc?;
        self.services
            .documents
            .get_content(GetContentQuery { id, range: None })
            .ok()
    }

    /// 读取编辑区当前绑定串（`TextEdit` 直接绑定的那个 `String`）。
    /// 与 [`Self::current_content`]（从领域读）配套：两者**必须逐字节相等**，
    /// 这是「领域是内容单一真相」在编辑区的可观测不变量。
    /// 漂移意味着用户敲了键界面却没反应——下一帧才被某条路径悄悄补上。
    pub fn editor_bound_text(&self) -> &str {
        &self.editor_text
    }

    /// IME 组合期标志。
    ///
    /// 暴露给测试断言「守卫是否生效」——它是回车失效缺陷的核心状态，
    /// 又是 `pub(crate)` 字段，测试没有它就只能在行为层面绕圈子。
    pub fn ime_composing(&self) -> bool {
        self.ime_composing
    }

    /// 待回设到 egui 的光标位置。
    ///
    /// 暴露给测试断言「普通输入不该产生光标回设」：回设即多一次强制重算，
    /// 是界面闪烁的上游原因（见 [`Self::flush_edit`]）。
    pub fn pending_cursor_for_diag(&self) -> Option<(usize, usize)> {
        self.pending_cursor
    }

    /// 某份文档记下的光标 / 选区（**测试用**，R-06）。
    ///
    /// 断言「切走时真的记了、切回时真的取了」：只断言 `editor.cursor`
    /// 分不清「表里有值」与「恰好默认 0」。
    pub fn per_doc_cursor_for_diag(&self, id: DocumentId) -> Option<(usize, usize)> {
        self.per_doc_cursor.get(&id).copied()
    }
}

// ————————————————————————————————————————————————————————————
// R-11 子批 3（editor 组）：编辑区 UI 状态的写入口
// ————————————————————————————————————————————————————————————
//
// 表现层不再直写 `app.editor.*`，与 `frame::export` / `frame::diff` 同构。
// 每个方法只对应原先的一处直写，不顺手重构选项的组织方式。

impl EditorApp {
    /// 记下 egui 报上来的光标 / 选区（**已**从字符索引换算成字节偏移）。
    ///
    /// 两者总是一起来：只写光标会留下过期的锚点，下一次拖拽选区会
    /// 从旧锚点算起。
    pub fn set_editor_selection(&mut self, cursor: usize, anchor: Option<usize>) {
        self.editor.cursor = cursor;
        self.editor.anchor = anchor;
    }

    /// 关闭「你可能想输入」菜单。
    pub fn close_spell_menu(&mut self) {
        self.editor.spell_menu = None;
    }

    /// 右键落在错词上时开菜单；传 `None` 等同关闭。
    pub fn set_spell_menu(&mut self, menu: Option<SpellSuggestMenu>) {
        self.editor.spell_menu = menu;
    }

    /// 记下编辑区的滚动偏移（渲染后由 egui 的滚动容器报上来）。
    pub fn set_editor_scroll_y(&mut self, y: f32) {
        self.editor.scroll_y = y;
    }

    /// 行号显示开关。
    pub fn set_line_numbers(&mut self, on: bool) {
        self.editor.line_numbers = on;
    }

    /// 远程图片开关（PRIV-01）：会话状态与领域设置**一起写**（F-1，理由同
    /// [`Self::set_word_wrap`]）。
    ///
    /// 预览区按会话值决定是否外联，落盘值决定重启后是否还外联——**隐私承诺
    /// 必须跨会话成立**，所以这两半只能有一个入口。
    pub fn set_load_remote_images(&mut self, on: bool) {
        self.editor.load_remote_images = on;
        self.persist_editor_toggle("远程图片", |e| e.load_remote_images = on);
    }

    /// 自动换行开关：会话状态与领域设置**一起写**（F-1）。
    ///
    /// 设置页复选框与状态栏点击是同一个偏好的两个入口，任一处只写一半都会
    /// 让另一处与它矛盾：只写领域 → 当前会话不生效、重启才回读；只写会话 →
    /// 当前生效但重启丢失。两条入口都收敛到这里，写入即落盘。
    pub fn set_word_wrap(&mut self, on: bool) {
        self.editor.word_wrap = on;
        self.persist_editor_toggle("自动换行", |e| e.word_wrap = on);
    }

    /// 拼写检查开关：会话状态与领域设置一起写（F-1，理由同 [`Self::set_word_wrap`]）。
    pub fn set_spell_check(&mut self, on: bool) {
        self.editor.spell_check = on;
        self.persist_editor_toggle("拼写检查", |e| e.spell_check = on);
    }

    /// 切换拼写检查（状态栏点击）。
    pub fn toggle_spell_check(&mut self) {
        let next = !self.editor.spell_check;
        self.set_spell_check(next);
    }

    /// 切换自动换行（状态栏点击）。
    pub fn toggle_word_wrap(&mut self) {
        let next = !self.editor.word_wrap;
        self.set_word_wrap(next);
    }

    /// 把一个编辑器开关写进领域设置；失败报到状态栏。
    ///
    /// 刻意不复用 `apply_editor_field`：那条路把错误记进 `settings_ui.error`，
    /// 而状态栏点击时设置浮层通常关着——错误会无人看见（禁静默）。开关是
    /// 布尔值，领域不会因取值拒绝，真正会失败的只有落盘本身，文案按此如实写。
    fn persist_editor_toggle(&mut self, label: &str, mutate: impl FnOnce(&mut EditorSettingsView)) {
        let mut view = self.settings().editor;
        mutate(&mut view);
        if let Err(e) = self
            .services
            .preferences
            .set_editor(SetEditorCommand { editor: view })
        {
            self.set_status_error(&format!("{label}设置未能保存：{e}"));
        }
    }
}
