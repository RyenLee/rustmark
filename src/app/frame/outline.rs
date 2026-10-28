//! 大纲 / 查找拼写：侧栏的「大纲」「检查」标签与跳转通路。
use super::*;

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

    /// 跳到某个字节偏移（选中该处并滚到视口顶部）。
    ///
    /// 大纲跳转、搜索跳转、拼写建议跳转共用它——三条路径对「跳过去
    /// 之后要做什么」的理解必须一致（选中 + 滚动），否则用户会发现
    /// 「大纲跳转不选中、搜索跳转选中」，进而怀疑其中一个坏了。
    ///
    /// 滚动意图取 [`RevealIntent::Top`]（问题 066）：这些都是**离散
    /// 跳转**，目标行必须贴视口顶部；沿用查找替换的「已在视口内就
    /// 不动 / 落 1/4 处」会让点击目录后半数情况毫无反应、跳过去了
    /// 也与标题顶部差四分之一屏。
    pub fn goto_byte_offset(&mut self, offset: usize) {
        let at = offset.min(self.editor_text.len());
        self.editor.set_selection(at, at);
        let cursor = editor_ops::byte_to_char(&self.editor_text, at);
        self.pending_cursor = Some((cursor, cursor));
        self.scroll_to_offset(at, RevealIntent::Top);
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
    /// 结果**不**在 UI 状态里缓存：那会让「文件改了但搜索结果还是旧的」。
    /// 代价是每帧可能要重算，故视图只在「搜索标签可见」时调用，且一帧
    /// 只调一次（计数与列表共用同一份结果）。
    ///
    /// # 范围切换器决定读不读盘
    ///
    /// 「当前文档」只搜活动文档，「打开的文档」只搜标签栏里的文档——两者
    /// 的内容都已在内存里，**不读磁盘**。磁盘只在未限定范围时参与
    /// （`WorkspaceAppService::search` 里 `include_disk` 的那条分支），
    /// 本方法的两条 UI 路径都不会落到那里：按目录全盘检索会把用户没打开的
    /// 文件成百上千个都读一遍，一次搜索实测能到百毫秒级（真正卡顿的来源），
    /// 而且也不是用户要的。要找未打开的文件应走「快速打开」浮层。
    ///
    /// 「当前文档」范围下若**没有**活动文档，返回空结果而**不是**退化成
    /// 跨文档搜索——那会让界面声明的「当前文档」范围静默变成别的范围。
    /// （空状态的文案由视图负责，这里只保证「没有就没有」。）
    ///
    /// # 错误原样上抛
    ///
    /// 检索失败（如正则模式下模式无效）返回 `Err` 而不是 `None`：
    /// [`AppError::Domain`] 的 `Display` 就是可直接呈现给用户的文案
    /// （「正则表达式无效：括号未闭合」），视图原样显示即可。吞成
    /// `None` 会让用户只看到「搜索失败」，无从得知自己输错了什么。
    pub fn search_workspace(&self) -> AppResult<WorkspaceSearch> {
        if !self.sidebar.should_search() {
            return Ok(WorkspaceSearch::default());
        }
        let only_document = match self.sidebar.search_scope {
            SearchScope::CurrentDocument => match self.active_doc {
                None => return Ok(WorkspaceSearch::default()),
                Some(id) => Some(id),
            },
            SearchScope::OpenDocuments => None,
        };
        let result = self.services.workspace.search(SearchQuery {
            pattern: self.sidebar.search_query.clone(),
            regex: self.sidebar.search_options.regex,
            case_sensitive: self.sidebar.search_options.case_sensitive,
            whole_word: self.sidebar.search_options.whole_word,
            only_document,
            only_open_documents: self.sidebar.search_scope == SearchScope::OpenDocuments,
        })?;
        Ok(WorkspaceSearch {
            groups: group_search_hits(&result),
            truncated: result.truncated,
        })
    }
}
