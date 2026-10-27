//! 文档统计的**单遍累加器**：字符 / 词 / 行级各项计数 + 围栏状态。
//!
//! 从 [`super`] 分出来的理由：它持有状态但**不持有 IO、不持有 `String`**——
//! 行内容以零拷贝 `&str` 切片在行尾传入 [`StatsAcc::finish_line`]
//! （建议三关键：单遍 + 零分配；旧实现每行重建 `line_buf`、每个西文 token
//! 重建 `current`，是 1MB 上约 4~6ms 重复遍历的主要来源）。状态机独立成模块
//! 后，「围栏内跳过整行」「西文 token 必须含字母数字」这两条规则可以直接喂
//! 字符 / 喂行来测，不必绕道 `compute_stats`。
//!
//! 行级判定（标题 / 围栏 / 任务）来自 [`super::classify`]，字符级判定（是否
//! 按字计的东亚文字）同理——本模块只负责「什么时候调、计数加几」。

use super::classify::{fence_token_of, is_atx_heading, is_cjk, task_marker_of};

pub(super) struct StatsAcc {
    // 字符 / 词级
    pub(super) chars: usize,
    pub(super) chars_no_space: usize,
    pub(super) cjk: usize,
    pub(super) western: usize,
    pub(super) in_token: bool,        // 当前是否在一个西文 token 内
    pub(super) token_has_alnum: bool, // 当前 token 是否至少含一个字母 / 数字
    // 行级
    pub(super) headings: usize,
    pub(super) code_blocks: usize,
    pub(super) tasks_done: usize,
    pub(super) tasks_total: usize,
    pub(super) paragraphs: usize,
    pub(super) fence: Option<&'static str>, // 当前所在的围栏标记（None 表示不在围栏内）
    pub(super) in_blank: bool,              // 上一个非空行是否为空行（判「新段落开始」）
}

impl Default for StatsAcc {
    fn default() -> Self {
        // `in_blank` 初始为 true：文档开头视为「处于空行之后」，
        // 首个非空行即开启第一个段落（与原实现 `let mut in_blank = true` 一致）。
        Self {
            chars: 0,
            chars_no_space: 0,
            cjk: 0,
            western: 0,
            in_token: false,
            token_has_alnum: false,
            headings: 0,
            code_blocks: 0,
            tasks_done: 0,
            tasks_total: 0,
            paragraphs: 0,
            fence: None,
            in_blank: true,
        }
    }
}

impl StatsAcc {
    /// 喂入一个普通字符：累计字符 / 非空格字符，并按词规则更新西文 token 状态机。
    ///
    /// 词规则（与旧实现逐字一致）：连续的 `[A-Za-z0-9_'-]` 构成一个 token，
    /// 当且仅当该 token 至少含一个字母 / 数字才计入字数（排除孤立的 `-`/`'`/`_`）。
    pub(super) fn feed(&mut self, ch: char) {
        self.chars += 1;
        if !ch.is_whitespace() {
            self.chars_no_space += 1;
        }
        if is_cjk(ch) {
            self.flush_western();
            self.cjk += 1;
        } else if ch.is_ascii_alphanumeric() || ch == '_' || ch == '\'' || ch == '-' {
            self.in_token = true;
            if ch.is_ascii_alphanumeric() {
                self.token_has_alnum = true;
            }
        } else {
            self.flush_western();
        }
    }

    /// 喂入一个换行：计入字符与行数、结算西文 token（换行本身不计入非空格字符）。
    pub(super) fn feed_newline(&mut self) {
        self.chars += 1;
        self.flush_western();
    }

    /// 把当前西文 token 结算为一个词（当且仅当含至少一个字母 / 数字）。
    pub(super) fn flush_western(&mut self) {
        if self.in_token && self.token_has_alnum {
            self.western += 1;
        }
        self.in_token = false;
        self.token_has_alnum = false;
    }

    /// 结算一行（行内容以零拷贝切片传入）：标题 / 围栏 / 任务 / 段落判定。
    ///
    /// 逻辑与原实现逐字一致；围栏内的整行内容一律跳过。
    pub(super) fn finish_line(&mut self, line: &str) {
        let trimmed = line.trim();
        let fence_token = fence_token_of(line);
        if let Some(open) = self.fence {
            if fence_token == Some(open) {
                self.fence = None;
            }
            return;
        }
        if let Some(token) = fence_token {
            self.fence = Some(token);
            self.code_blocks += 1;
            return;
        }
        if is_atx_heading(trimmed) {
            self.headings += 1;
        }
        if let Some(done) = task_marker_of(trimmed) {
            self.tasks_total += 1;
            if done {
                self.tasks_done += 1;
            }
        }
        if trimmed.is_empty() {
            self.in_blank = true;
        } else if self.in_blank {
            self.paragraphs += 1;
            self.in_blank = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 围栏内的整行一律跳过：标题 / 任务 / 段落都不计，**开启围栏的那一行
    /// 本身也不得另计段落**。
    ///
    /// 这条用例钉住「围栏状态机」的两道 `return`：删掉围栏内的那道会让围栏
    /// 里的标题 / 任务被计入；删掉开启围栏后那道会让 ```` ```rust ```` 自己
    /// 多算一个段落（前后的空行正是为了让这两处的段落数分得开）。
    #[test]
    fn fenced_body_is_skipped_and_the_fence_counts_once() {
        let mut acc = StatsAcc::default();
        acc.finish_line("正文");
        acc.finish_line("");
        acc.finish_line("```rust");
        acc.finish_line("# 不是标题");
        acc.finish_line("- [ ] 不是任务");
        acc.finish_line("```");
        acc.finish_line("");
        acc.finish_line("围栏外的正文");
        assert_eq!(
            (
                acc.code_blocks,
                acc.headings,
                acc.tasks_total,
                acc.paragraphs
            ),
            (1, 0, 0, 2),
            "开启围栏的那一行本身不得另计段落，围栏内的标题 / 任务也不得计入"
        );
        assert_eq!(acc.fence, None, "闭合后必须退出围栏状态");
    }

    /// 西文 token 必须至少含一个字母 / 数字：孤立的 `-` / `'` / `_` 不算词。
    #[test]
    fn western_tokens_need_at_least_one_alphanumeric() {
        let mut acc = StatsAcc::default();
        for ch in "well-known - ' _ 42".chars() {
            acc.feed(ch);
        }
        acc.flush_western();
        assert_eq!(acc.western, 2, "只有 well-known 与 42 是词");
        assert_eq!(acc.cjk, 0);
    }

    /// CJK 按字计，且遇到 CJK 会先把在途的西文 token 结算掉。
    #[test]
    fn cjk_counts_per_character_and_flushes_the_pending_token() {
        let mut acc = StatsAcc::default();
        for ch in "你好world".chars() {
            acc.feed(ch);
        }
        acc.flush_western();
        assert_eq!((acc.cjk, acc.western), (2, 1));
    }

    /// 换行计入字符数但不计入「非空格字符」。
    #[test]
    fn newline_counts_as_a_character_but_not_as_non_space() {
        let mut acc = StatsAcc::default();
        acc.feed('a');
        acc.feed_newline();
        assert_eq!((acc.chars, acc.chars_no_space), (2, 1));
    }
}
