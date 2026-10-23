//! 查找替换 UI 状态与匹配器（S6-08）。
//!
//! # 为什么自己实现匹配而不用 `regex` crate
//!
//! 1. **体积**：S5-03 刚把 syntect 压到 17 KiB，此时引入 `regex` +
//!    `regex-syntax` 会成为新的体积大头，而「全字匹配 + 大小写 + 正则」
//!    三者的组合用一个手写匹配器就够；
//! 2. **UTF-8 安全**：手写扫描天然按 `char` 走，不会切在多字节字符中间
//!    （正则库的正则会，但用户写的 `regex` 模式仍可能切——这一点
//!    在 [`FindOptions::validate`] 里显式拒绝跨字符边界的字面量模式）。
//!
//! # 匹配模式
//!
//! | 组合 | 行为 |
//! |---|---|
//! | 默认 | 大小写不敏感的**字面量**匹配 |
//! | `Aa` | 大小写敏感的字面量匹配 |
//! | `.*` | 正则匹配（支持 `^ $ . * + ? [] () \|` 与转义） |
//! | 全字 | 命中的两侧必须是「词边界」 |
//!
//! 三个开关**正交**，可任意组合（原型 `FindReplacePanel.vue` 的行为）。
//!
//! # 模块结构
//!
//! facade 保留查找选项、错误类型、输入框常量与面板状态 `FindReplaceUiState`；
//! 迷你正则引擎（AST / 编译 / 回溯）本体在 [`crate::shared::pattern`]——
//! 它同时服务侧栏跨文件搜索（应用层），下沉是分层依赖的硬要求；
//! 本目录的 `pattern` 只留依赖 [`FindOptions`] 的编译适配，
//! 扫描、循环定位与字面量匹配算法在 `engine`。

mod engine;
mod pattern;

/// 模式校验结果：引擎本体移入 [`crate::shared::pattern`]（侧栏跨文件搜索
/// 同样要报它），此处重导出以保持原引用路径不变。
pub use crate::shared::pattern::PatternError;
pub use engine::{Finder, Match, ReplaceSummary};
pub use pattern::{Pattern, Regex, compile};

/// 查找选项。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FindOptions {
    /// 区分大小写（`Aa`）。
    pub case_sensitive: bool,
    /// 正则模式（`.*`）。
    pub regex: bool,
    /// 全字匹配。
    pub whole_word: bool,
}

impl FindOptions {
    /// 是否处于「纯字面量、不区分大小写」（默认状态）。
    pub const fn is_plain(&self) -> bool {
        !self.case_sensitive && !self.regex && !self.whole_word
    }

    /// 活跃开关数（UI 显示成角标）。
    pub const fn active_count(&self) -> usize {
        self.case_sensitive as usize + self.regex as usize + self.whole_word as usize
    }
}

/// 查找输入框的 egui `Id` 源串。
///
/// 单独提出常量是因为**三处**必须用同一个值：视图里绑定控件、
/// root crate 的 `crate::app::frame::EditorApp::collect_actions`
/// 判断「焦点是否在输入框」（core 不反向依赖 root，仅文字指路不成链接）。
/// 写成两处字面量的话，改视图里的那处就会让「快捷键让位」静默失效——
/// 用户在查找框里按 Ctrl+B 又把文档加粗了，且没有任何报错。
pub const FIND_INPUT_ID: &str = "rustmark.find.input";

/// 替换输入框的 egui `Id` 源串（理由同 [`FIND_INPUT_ID`]）。
pub const REPLACE_INPUT_ID: &str = "rustmark.find.replace";

/// 查找替换面板状态。
#[derive(Clone, Debug, Default)]
pub struct FindReplaceUiState {
    /// 查找框内容。
    pub find_text: String,
    /// 替换框内容。
    pub replace_text: String,
    /// 查找选项。
    pub options: FindOptions,
    /// 是否展开替换框。
    pub replace_open: bool,
    /// 当前命中的索引（`None` = 未选中）。
    pub current: Option<usize>,
    /// 上次扫描得到的查找器（`None` = 还没扫过 / 模式无效）。
    ///
    /// 存**编译后的模式**而非只存命中列表：正则模式编译一次即可复用，
    /// 而「下一个 / 上一个」需要在命中列表上二分/顺序查找，命中列表本身
    /// 就是 [`Finder`] 的一部分，拆开存只会多一份可能不同步的副本。
    ///
    /// 它是**派生状态**：由 `find_text` + `options` + 文档内容唯一决定，
    /// 任何一项变了都必须 [`Self::rescan`] 重扫，否则面板显示的计数
    /// 会与实际能跳转到的位置不符。
    pub finder: Option<Finder>,
    /// 面板错误提示（如正则无效）。
    pub error: Option<String>,
    /// 查找框是否拥有焦点（查找期间编辑区仍可编辑，A-28）。
    pub find_focused: bool,
    /// 上次查找的文档 ID——跨文档切换时据此决定是否保留状态。
    pub last_doc_id: Option<crate::shared::ids::DocumentId>,
    /// 跨文档切换时是否保留查找状态。
    ///
    /// 定义（验收要求「明确定义并写进文档」）：**保留**查询与选项，
    /// 但**重置**命中选中与滚动位置。理由：用户常在多个文件里
    /// 找同一个词；但命中位置属于旧文档，带过去会跳到错误位置。
    pub keep_query_across_documents: bool,
    /// 上次「全部替换」的替换处数（状态栏提示用；`None` = 本次会话没替换过）。
    pub last_replace_count: Option<usize>,
    /// 上次「全部替换」后是否仍会出现新命中（`aa` → `a` 这类无限增生）。
    ///
    /// 必须让用户看见：否则反复点「全部替换」会让文档指数膨胀，
    /// 而每一遍都提示「已替换 N 处」，看起来完全正常。
    pub last_replace_grows: bool,
}

impl FindReplaceUiState {
    /// 写入查找框内容（D-3 阶段 3：视图 clone → 就地编辑 → 变化才值写回，
    /// 字段的可变引用不出 state）。写回后调用方须触发重扫（`sync_find`）。
    pub fn set_find_text(&mut self, find_text: String) {
        self.find_text = find_text;
    }

    /// 写入替换框内容（同 `set_find_text` 的方法化理由）。
    pub fn set_replace_text(&mut self, replace_text: String) {
        self.replace_text = replace_text;
    }

    /// 构造默认状态。
    pub fn new() -> Self {
        Self {
            keep_query_across_documents: true,
            ..Default::default()
        }
    }

    /// 打开面板。
    pub fn open(&mut self) {
        self.replace_open = false;
    }

    /// 打开面板并展开替换框（`Replace` 动作）。
    pub fn open_with_replace(&mut self) {
        self.replace_open = true;
    }

    /// 打开面板（`with_replace` 决定是否展开替换框）。
    ///
    /// 取代 [`Self::open`] / [`Self::open_with_replace`] 成对使用：
    /// 两个几乎一样的公开方法很容易让调用方「挑一个顺手的用」，
    /// 而 `Ctrl+F` 展开替换框并不是什么不可饶恕的错误——只是多占一行。
    pub fn open_panel(&mut self, with_replace: bool) {
        self.replace_open = with_replace;
        // 打开即聚焦查找框：用户按 Ctrl+F 的意图是「马上打字」，
        // 焦点若留在编辑区，就得再点一次面板。
        self.find_focused = true;
    }

    /// 关闭面板并清空错误。
    pub fn close(&mut self) {
        self.error = None;
    }

    /// 对给定文档内容重扫，并更新命中计数 / 当前项 / 错误。
    ///
    /// 这是面板状态的**唯一**写入口：改 `find_text` 或 `options` 之后
    /// 必须调它，否则会拿旧模式去报数。
    pub fn rescan(&mut self, text: &str) {
        if self.find_text.is_empty() {
            // 空查询：清空而不是保留旧命中。否则用户清空输入框后
            // 面板还显示「3/12」，点「下一个」却跳到旧位置。
            self.finder = None;
            self.error = None;
            self.current = None;
            return;
        }
        match Finder::scan(text, &self.find_text, self.options) {
            Ok(finder) => {
                let count = finder.count();
                self.finder = Some(finder);
                self.error = None;
                if count == 0 {
                    self.current = None;
                } else if self.current.is_none_or(|index| index >= count) {
                    self.current = Some(0);
                }
            }
            Err(error) => {
                // 模式无效：保留**上一次**的 finder 吗？刻意不保留——
                // 那样「下一个」会跳到与当前输入框内容无关的位置。
                self.finder = None;
                self.error = Some(error.to_string());
                self.current = None;
            }
        }
    }

    /// 当前选中的命中（`None` 表示无命中或未选中）。
    pub fn current_match(&self) -> Option<Match> {
        let finder = self.finder.as_ref()?;
        let index = self.current?;
        finder.matches().get(index).copied()
    }

    /// 命中总数。
    pub fn match_count(&self) -> usize {
        self.finder.as_ref().map_or(0, Finder::count)
    }

    /// 计数文本（`1/12` / `无结果` / 空串）。
    ///
    /// 与原型 `FindReplacePanel.vue` 的 `{{ findIndex + 1 }}/{{ matches.length }}`
    /// 逐字对齐，包括「无命中时显示『无结果』而不是 `0/0`」这个细节，
    /// 以及「查询为空时什么都不显示」（此时报「无结果」会让人以为文档里
    /// 真的没有匹配，可明明还没输入任何东西）。
    pub fn count_label(&self) -> String {
        if self.find_text.is_empty() {
            return String::new();
        }
        let total = self.match_count();
        if total == 0 {
            return "无结果".to_string();
        }
        // `current` 为 `None` 只可能出现在「无命中」（上面已处理），
        // 兜底成第 1 项，避免 UI 出现 `0/12`。
        format!("{}/{}", self.current.unwrap_or(0) + 1, total)
    }

    /// 跳到下一个 / 上一个命中（`forward = true` 为下一个）。
    ///
    /// 循环语义由 [`Finder::next_after`] / [`Finder::prev_before`] 提供，
    /// 这里只负责算起点：从**当前命中的终点**往后找，反向则从**起点**往前找。
    /// 用「终点」而非「起点」是关键——否则连点两次「下一个」会在同一个
    /// 命中上打转（起点仍在命中内，`end > from` 立刻又选中它）。
    pub fn step(&mut self, forward: bool) -> Option<Match> {
        let finder = self.finder.as_ref()?;
        let total = finder.count();
        if total == 0 {
            return None;
        }
        let from = match self.current.and_then(|i| finder.matches().get(i)) {
            Some(m) if forward => m.end,
            Some(m) => m.start,
            // 没有当前项：正向从头找，反向从末尾找（与主流编辑器一致——
            // 「上一个」在刚打开面板时跳到最后一个命中）。
            None => {
                if forward {
                    0
                } else {
                    usize::MAX
                }
            }
        };
        let found = if forward {
            finder.next_after(from)
        } else {
            finder.prev_before(from)
        };
        let found = found?;
        // 反向查找用 `usize::MAX` 起点时 `prev_before` 会返回最后一项，
        // 靠 `matches().iter().position` 反查索引——O(n)，但只在
        // 「无当前项」这一次发生，不是热路径。
        let index = if forward {
            finder.matches().iter().position(|m| *m == found)?
        } else {
            finder.matches().iter().rposition(|m| *m == found)?
        };
        self.current = Some(index);
        Some(found)
    }

    /// 选项变更后必须清空当前命中（旧的 `current` 指向的是旧模式的第 N 项）。
    pub fn set_options(&mut self, options: FindOptions) {
        if self.options != options {
            self.options = options;
            self.current = None;
        }
    }

    /// 切换某个选项。
    pub fn toggle(&mut self, which: OptionKind) {
        let mut options = self.options;
        match which {
            OptionKind::CaseSensitive => options.case_sensitive = !options.case_sensitive,
            OptionKind::Regex => options.regex = !options.regex,
            OptionKind::WholeWord => options.whole_word = !options.whole_word,
        }
        self.set_options(options);
    }

    /// 记录一次查找结果。
    ///
    /// 已被 [`Self::rescan`] 取代并删除：它与 `rescan` 是同一件事的两种写法
    /// （前者接收「已编译好的 `Finder`」，后者自己扫），两份入口必然出现
    /// 「调用方用了旧的那份、忘了更新 `finder` 字段」的分歧——
    /// 这正是 S6-07 在 `editor_ops` 与 `EditingService` 之间踩过的坑。
    /// 单一入口 [`Self::rescan`] 既是唯一真相，也让「模式变了会自动重扫」
    /// 成为结构性保证而非调用方的自觉。
    ///
    /// 文档切换：按 `keep_query_across_documents` 决定保留什么。
    pub fn on_document_switched(&mut self, doc_id: crate::shared::ids::DocumentId) {
        match self.last_doc_id {
            None => self.last_doc_id = Some(doc_id),
            Some(previous) if previous == doc_id => {}
            Some(_) => {
                // 跨文档：保留查询与选项，重置命中选中。
                self.current = None;
                self.error = None;
                // 命中列表**必须**清空：它的字节偏移属于旧文档，
                // 带到新文档上会跳到完全无关的位置（甚至越界）。
                // 保留查询的用户随后 [`Self::rescan`] 一次就能得到新命中。
                self.finder = None;
                if !self.keep_query_across_documents {
                    self.find_text.clear();
                    self.replace_text.clear();
                    self.options = FindOptions::default();
                }
                self.last_doc_id = Some(doc_id);
            }
        }
    }
}

/// 可切换的查找选项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OptionKind {
    /// 区分大小写。
    CaseSensitive,
    /// 正则。
    Regex,
    /// 全字。
    WholeWord,
}
