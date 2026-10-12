//! 查找替换：面板打开后的扫描、跳转、替换通路。
use super::*;

impl EditorApp {
    /// 按当前查询 / 选项重扫文档，并把命中写入面板状态。
    ///
    /// 所有会改变「查询 × 选项 × 内容」三者之一的操作 afterwards 都必须调它，
    /// 否则面板显示的 `n/m` 与实际能跳转到的位置不符——这类不一致
    /// 表现为「点下一个跳到了一个奇怪的地方」，极难定位。
    pub fn sync_find(&mut self) {
        let text = self.current_content().unwrap_or_default();
        self.find.rescan(&text);
    }

    /// 跳到下一个 / 上一个命中（`forward = true` 为下一个），并选中它。
    ///
    /// 选中而非只滚动：用户接着多半要按 `Ctrl+H` 替换当前这条，
    /// 选区就是「当前命中」的定义。反之若只滚动光标，用户无法知道
    /// 「当前」是哪一条。
    pub fn find_step(&mut self, forward: bool) {
        if self.find.finder.is_none() {
            // 没扫过就先扫一次：面板刚打开、用户还没动输入框就按 F3
            // 是常见操作（沿用上次的查询继续找）。
            self.sync_find();
        }
        let Some(found) = self.find.step(forward) else {
            return;
        };
        self.reveal_match(found);
    }

    /// 选中某个命中并把它滚进视口。
    pub(super) fn reveal_match(
        &mut self,
        found: crate::presentation::state::find_replace_ui_state::Match,
    ) {
        self.editor.set_selection(found.start, found.end);
        // egui 的 `TextEdit` 记的是**字符**位置，不设的话本帧
        // `TextEdit` 读回自己的旧光标，又把我们的选区覆盖掉。
        let (cs, ce) = (
            editor_ops::byte_to_char(&self.editor_text, found.start),
            editor_ops::byte_to_char(&self.editor_text, found.end),
        );
        self.pending_cursor = Some((ce, cs));
        // 滚动到可见：命中在文档很后面时，不滚屏的话「计数显示 8/12」
        // 而光标还在开头，用户完全看不出跳到了哪。
        self.scroll_to_offset(found.start);
    }

    /// 把某个字节偏移滚进编辑区视口（已在视口内则不动）。
    ///
    /// 复用 S6-06 的行顶表（[`mapper::line_tops`]）把字节偏移换成 y，
    /// 再用与滚动同步同一套夹紧，避免「跳转后又被同步逻辑拖回去」。
    pub fn scroll_to_offset(&mut self, byte_offset: usize) {
        let theme = self.theme().clone();
        let Some(tops) = mapper::line_tops(&theme, &self.editor_text) else {
            // 行布局缓存未命中（首帧 / 刚换文档）：不猜行高——
            // 猜了会让跳转停在一个看似合理实则错误的位置。
            return;
        };
        let starts = crate::presentation::views::scroll_sync::line_start_offsets(&self.editor_text);
        let line = crate::presentation::views::scroll_sync::line_of_offset(&starts, byte_offset);
        let Some(&top) = tops.get(line) else {
            return;
        };
        let bottom = tops.get(line + 1).copied().unwrap_or(top);
        let geom = self.ui.sync.geometry(Side::Editor);
        if geom.viewport_h <= 0.0 {
            return;
        }
        let view_top = self.editor.scroll_y;
        let view_bottom = view_top + geom.viewport_h;
        // 已在视口内就**不动**：每次跳转都把命中顶到视口上部会让长文档里
        // 连续「下一个」时画面剧烈跳动，反而更难看清自己在哪。
        // 只有完全看不见（命中是折行成多行的高亮段落时会超出视口）才滚。
        if top >= view_top && bottom <= view_bottom {
            return;
        }
        // 目标：让该行落在视口**上部**（留 1/4 屏余量），
        // 而不是刚好贴边——贴边时用户分不清「跳到这里了」还是「本来就在这」。
        let target = top - geom.viewport_h * 0.25;
        let clamped = crate::presentation::views::scroll_sync::clamp_scroll(target, geom);
        if (clamped - self.editor.scroll_y).abs() > 0.5 {
            self.editor.scroll_y = clamped;
        }
    }

    /// 替换当前命中并前进到下一条（`Ctrl+H` 面板的「替换」按钮）。
    pub fn find_replace_current(&mut self) {
        let Some(found) = self.find.current_match() else {
            return;
        };
        let replacement = self.find.replace_text.clone();
        let len = replacement.len();
        self.replace_match(found, &replacement);
        // 光标落在替换文本之后（与原型 `replaceCurrent` 一致：
        // `at = m.start + replaceText.length`），并选中新插入的内容——
        // 用户连点几次时，每次替换的是「刚插进去的那段」而不是跳过它。
        let at = found.start + len;
        self.editor.set_selection(at, at);
        self.pending_cursor = Some((
            editor_ops::byte_to_char(&self.editor_text, at),
            editor_ops::byte_to_char(&self.editor_text, at),
        ));
        self.sync_find();
        // 替换后内容长度变了，命中列表整体左移；重新选中「原位置之后的
        // 第一条」才能继续替换下一条。不重选的话用户点第二次「替换」
        // 会替换到已经替换过的那条上（或干脆没反应）。
        if let Some(next) = self.find.step(true) {
            self.reveal_match(next);
        }
    }

    /// 全部替换。
    ///
    /// 走**一次** `replace_range(0..len)` 而不是逐条替换：后者会在撤销栈里
    /// 留下 N 个撤销点，用户按一次 Ctrl+Z 只撤掉一处，还得按 N 次——
    /// 而「全部替换」在用户心智里是**一个**动作。
    pub fn find_replace_all(&mut self) {
        let Some(finder) = self.find.finder.clone() else {
            return;
        };
        let text = self.current_content().unwrap_or_default();
        let summary = finder.preview_replace_all(&text, &self.find.replace_text);
        if summary.count == 0 {
            return;
        }
        self.find.last_replace_count = Some(summary.count);
        self.find.last_replace_grows = summary.will_match_again;
        self.replace_all(summary.result);
        // 替换后重扫：命中列表已完全失效（长度都变了）。
        self.sync_find();
    }

    /// 把一条命中替换掉（写领域 + 刷新副本）。
    pub(super) fn replace_match(&mut self, found: Match, replacement: &str) {
        let Some(id) = self.active_doc else {
            return;
        };
        let range = TextRange::new(ByteOffset::new(found.start), ByteOffset::new(found.end));
        if self
            .services
            .documents
            .replace_range(ReplaceRangeCommand {
                id,
                range,
                text: replacement.to_string(),
            })
            .is_err()
        {
            return;
        }
        self.refresh_text();
        // 查找替换走领域撤销栈 → egui 内置栈必须让位，
        // 否则 Ctrl+Z 会先撤掉 egui 自己的输入历史，替换撤不回来。
        self.clear_undoer_next = true;
    }
}
