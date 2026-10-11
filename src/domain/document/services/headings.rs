//! [`EditingService`] 的 ATX 标题能力：层级读取、提升 / 降级 / 重定目标。

use std::cell::Cell;

use crate::shared::geometry::TextRange;

use super::super::value_objects::{Edit, EditPlan};
use super::EditingService;

impl EditingService {
    /// 提升标题级别：h6 → h1 → h2 …（在 h1 后再点会包回 h6，形成循环）。
    ///
    /// # 为什么不能「逐层试探」
    ///
    /// [`Self::toggle_line_prefix`] 是**切换**语义：前缀不存在时它会「加上」，
    /// 不是无操作。所以「从 6 级往下试，剥到干净的为止」会得到
    /// `###### ## 标题` 这种把 6 级**加**上去的结果。
    /// 正确做法是：先**读出**当前实际层级，再做一次精确的切换。
    pub fn heading_level_at(text: &str, offset: usize) -> usize {
        let line = Self::line_range(text, offset);
        let body = text.get(line.as_usize_range()).unwrap_or("");
        // `atx_prefix_len` 对非标题返回 0，故先判零再减 1（否则 usize 下溢）。
        // 层级上限 6，故 7 个以上 `#` 按 6 算。
        match Self::atx_prefix_len(body) {
            0 => 0,
            len => (len - 1).min(6),
        }
    }

    /// 提升标题级别。
    pub fn promote_heading(text: &str, range: TextRange) -> EditPlan {
        let level = Self::heading_level_at(text, range.start().value());
        // 目标层级：0（正文）→ 1；1（h1）→ 6（循环）；其余 → level - 1。
        let target = match level {
            0 => 1,
            1 => 6,
            n => n - 1,
        };
        Self::retarget_heading(text, range, target)
    }

    /// 降低标题级别：剥掉一层（`### x` → `## x`）。
    ///
    /// 已经是正文（无前缀）的行**保持原样**——再降就该变引用了，
    /// 而「降级变引用」不是用户点击「降级标题」时的预期。
    ///
    /// 光标所在行为正文时整条编辑**无效果**（`level == 0` 直接返回），
    /// 因为无从「再降一层」。
    pub fn demote_heading(text: &str, range: TextRange) -> EditPlan {
        let level = Self::heading_level_at(text, range.start().value());
        if level == 0 {
            // 无标题前缀 → 无效果编辑。用 `insertion(at, "")` 而非
            // `deletion(range)`：后者会清掉选区内容。
            return EditPlan::plain(Edit::insertion(range.start(), ""));
        }
        Self::retarget_heading(text, range, level - 1)
    }

    /// 把选区内各行的标题层级改成 `to`（0..=6，0 表示正文）。
    ///
    /// # 为什么是「一次替换」而不是「剥旧 → 加新」两步
    ///
    /// 两步写法有两个各自独立的缺陷，故改为对每行做**一次**替换：
    ///
    /// 1. **步骤间坐标系漂移**。第一步让文本变短，入参选区不再指向原来的
    ///    位置。旧代码用固定步长左移，两种算法都错：「减去 `from + 1`」
    ///    假设每行都缩短同样多（但 `toggle_line_prefix` 逐行切换，`## A`
    ///    减 3 字节、`正文` 却**加** 3 字节，净值为 0）；「起点在被改
    ///    区间内就左移」在光标**恰好落在区间起点**时也左移，可那里根本
    ///    没有字节消失。实测（光标在 `## 第二节` 行首降级）会把 `# `
    ///    加到**上一行**，症状是「降级改错了行」且不报错。
    /// 2. **正文行被污染**。`toggle_line_prefix` 是「有则删、无则加」，
    ///    所以对正文行执行「剥 `## `」会**加上** `## `，两步之后正文行
    ///    变成 `# ## 正文`。而 [`EditingService::demote_heading`] 明确承诺
    ///    「已经是正文的行保持原样」。
    ///
    /// 一次替换同时消掉两个问题：每行读出自己**已有**的 `#` 数量，
    /// 换成目标层级（`to == 0` 则换成无前缀），没有「加」这个动作，
    /// 故正文行必然原样保留。
    ///
    /// # 多行选区里哪些行该改
    ///
    /// 判据是「**该行本来就是标题**」或「**该行是光标所在行**」：
    ///
    /// * 光标所在行总是要改——用户把光标放在正文上点「提升」就是
    ///   想把它变成标题（`promote_heading` 从正文加 h1 靠的就是这条）。
    /// * 选区里**其余**的正文行不改。把「标题 + 下面的正文段落」一起
    ///   选中再点降级，用户期待的是标题降级、段落仍是段落；
    ///   把段落变成标题是另一种操作（大纲拖拽新建标题），不该搭便车。
    ///
    /// 返回的 `Edit` 只覆盖选区涉及的那些整行——与其他行级规则
    /// （`toggle_line_prefix` / `indent_lines`）一致，撤销栈里存的是
    /// 改动的那几行而非整篇。旧的两步写法必须返回整篇（中间结果的
    /// 坐标系已不是入参的坐标系），单步之后这个限制自然消失。
    fn retarget_heading(text: &str, range: TextRange, to: usize) -> EditPlan {
        let new_prefix = if to > 0 {
            format!("{} ", "#".repeat(to))
        } else {
            String::new()
        };
        // `transform_lines` 从 `expand_to_full_lines` 的起点开始逐行处理，
        // 而那个起点正是 `line_start(range.start())` —— 故**第一个**被
        // 变换的行就是光标所在行，无需再比对偏移。
        // 标志位用 `Cell` 而非普通 `bool`：`transform_lines` 收的是
        // `Fn` 闭包（内部不需可变状态），捕获可变变量会编译不过。
        let at_cursor_line = Cell::new(true);
        Self::transform_lines(text, range, |line| {
            let is_cursor_line = at_cursor_line.replace(false);
            // 空行不该变成 `# `（视觉上多了一个空标题）。
            if line.trim().is_empty() {
                return line.to_string();
            }
            let existing = Self::atx_prefix_len(line);
            // 选区里的非光标行：只有本来就是标题的才改。
            if existing == 0 && !is_cursor_line {
                return line.to_string();
            }
            format!("{new_prefix}{}", &line[existing..])
        })
    }

    /// 行首 ATX 标题前缀的**字节长度**（`### ` → 4）；不是标题则 0。
    ///
    /// 与 [`EditingService::heading_level_at`] 的判定保持一致：
    /// `#` 后必须跟空格才算标题，故 `###tag` 不算（返回 0）。
    fn atx_prefix_len(line: &str) -> usize {
        let hashes = line.chars().take_while(|c| *c == '#').count();
        if hashes == 0 || !line[hashes..].starts_with(' ') {
            return 0;
        }
        // `#` 是单字节 ASCII，故字符数即字节数；再加一个空格。
        hashes + 1
    }
}
