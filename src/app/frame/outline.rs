//! 大纲 / 查找拼写：侧栏的「大纲」「检查」标签与跳转通路。
use super::*;
use crate::app::frame::workspace::group_search_hits;

impl EditorApp {
    /// 取当前文档的大纲（`None` 表示没有活动文档或解析失败）。
    pub fn current_outline(&self) -> Vec<OutlineItemView> {
        let Some(id) = self.active_doc else {
            return Vec::new();
        };
        self.services
            .markdown
            .rebuild_outline(RebuildOutlineQuery { doc_id: id })
            .map(|view| view.0)
            .unwrap_or_default()
    }

    /// 取当前文档的格式问题（V11-08 侧栏「检查」标签）。
    ///
    /// 规则开关取自**当前设置**（`settings().editor.lint`）：这样用户在设置页
    /// 关掉某条规则后，切回侧栏立即生效，不需要重启或显式刷新。
    /// 没有活动文档时返回空集（与 [`Self::current_outline`] 一致）。
    pub fn lint_current_document(&self) -> Vec<LintIssueView> {
        let Some(id) = self.active_doc else {
            return Vec::new();
        };
        let rules = self.settings().editor.lint;
        self.services
            .markdown
            .lint_document(LintQuery { doc_id: id, rules })
    }

    /// 跳到大纲某一项（把行号换成字节偏移后复用查找替换的跳转通路）。
    ///
    /// 为什么不直接按行号滚：`TextEdit` 与滚动同步全都以**字节偏移**为坐标，
    /// 这里若自己换算一遍就会与 `scroll_to_offset` 各算各的，迟早漂移。
    pub fn goto_outline_line(&mut self, line: usize) {
        let starts = crate::presentation::views::scroll_sync::line_start_offsets(&self.editor_text);
        let Some(&offset) = starts.get(line) else {
            return;
        };
        self.goto_byte_offset(offset);
    }

    /// 跳到某个字节偏移（选中该处并滚进视口）。
    ///
    /// 大纲跳转与搜索跳转共用它——两条路径对「跳过去之后要做什么」
    /// 的理解必须一致（选中 + 滚动），否则用户会发现「大纲跳转不选中、
    /// 搜索跳转选中」，进而怀疑其中一个坏了。
    pub fn goto_byte_offset(&mut self, offset: usize) {
        let at = offset.min(self.editor_text.len());
        self.editor.set_selection(at, at);
        let cursor = editor_ops::byte_to_char(&self.editor_text, at);
        self.pending_cursor = Some((cursor, cursor));
        self.scroll_to_offset(at);
    }

    /// 用一条拼写建议替换 `[start, end)` 区间（V11-07 右键菜单）。
    ///
    /// 公开而非在视图里直接调 `documents.replace_range`：替换必须连带
    /// `refresh_text()` 与「清 egui 撤销栈」两件事，漏一件就是
    /// 「界面显示已改、领域没改」或「Ctrl+Z 撤不回来」。让视图各写一遍
    /// 等于把这些不变式复制出去（S6-08 的教训）。
    ///
    /// 区间越界时**静默返回 `Err`**（由 `TextRange::new` 校验），不 panic：
    /// 建议菜单里存的是上一帧的字节区间，用户可能在菜单开着时删了那段文本。
    pub fn replace_spelling(
        &mut self,
        start: usize,
        end: usize,
        replacement: &str,
    ) -> AppResult<()> {
        let Some(id) = self.active_doc else {
            return Err(AppError::domain("没有活动文档"));
        };
        let range = TextRange::new(ByteOffset::new(start), ByteOffset::new(end));
        self.services.documents.replace_range(ReplaceRangeCommand {
            id,
            range,
            text: replacement.to_string(),
        })?;
        self.refresh_text();
        self.clear_undoer_next = true;
        // 光标落在替换后的词尾：用户多半要接着改下一个错词，
        // 停在新词末尾比跳到文首更省事。
        self.goto_byte_offset(start + replacement.len());
        Ok(())
    }

    /// 在工作区里搜索。
    ///
    /// 每次调用都**真的**走一遍磁盘（`WorkspaceAppService::search`），
    /// 绝不在 UI 状态里缓存结果：那会让「文件改了但搜索结果还是旧的」。
    ///
    /// 代价是每帧可能全盘扫描，故视图只在「搜索标签可见」时调用。
    /// 真要优化应加在应用层（那里才知道文件有多大），而不是让表现层
    /// 偷偷少搜几次——那会让「搜到了」与「搜全了」不再等价。
    ///
    /// # 范围切换器
    ///
    /// 「当前文档」只搜活动文档；此时若**没有**活动文档，返回空结果而
    /// **不是**退化成跨文档搜索——那会让界面声明的「当前文档」范围
    /// 静默变成别的范围，结果与用户看到的范围标签互相矛盾。
    /// （空状态的文案由视图负责，这里只保证「没有就没有」。）
    ///
    /// 「打开的文档」只搜**标签栏里已打开的文档**：既与用户的工作上下文
    /// 一致，也避免一次搜索读遍整个工作区目录（未打开的文件成百上千时，
    /// 全盘读盘会让界面卡住；要找未打开的文件应走「快速打开」浮层）。
    pub fn search_workspace(&self) -> Option<WorkspaceSearch> {
        if !self.sidebar.should_search() {
            return Some(WorkspaceSearch::default());
        }
        let only_document = match self.sidebar.search_scope {
            SearchScope::CurrentDocument => match self.active_doc {
                None => return Some(WorkspaceSearch::default()),
                Some(id) => Some(id),
            },
            SearchScope::OpenDocuments => None,
        };
        let result = self
            .services
            .workspace
            .search(SearchQuery {
                pattern: self.sidebar.search_query.clone(),
                regex: self.sidebar.search_options.regex,
                case_sensitive: self.sidebar.search_options.case_sensitive,
                whole_word: self.sidebar.search_options.whole_word,
                only_document,
                only_open_documents: self.sidebar.search_scope == SearchScope::OpenDocuments,
            })
            .ok()?;
        Some(WorkspaceSearch {
            groups: group_search_hits(
                &result,
                &self.sidebar.search_query,
                self.sidebar.search_options.case_sensitive,
            ),
            truncated: result.truncated,
        })
    }
}
