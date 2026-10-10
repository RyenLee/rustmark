//! Document 上下文的领域服务（S3-02 `TextBuffer` trait、S3-05 `EditingService`）。
//!
//! - [`TextBuffer`]：文本缓冲的**领域抽象**，由基础设施层用 `ropey` 实现（S5-01）。
//!   所有方法返回 `Result` 或饱和值，**不 panic**（`panic = "abort"` 下 panic 即进程终止）。
//! - [`EditingService`]：**无状态纯函数**集合，输入 `&str` 输出 [`EditPlan`]，
//!   不触碰缓冲区，因而全部可单测。

use std::borrow::Cow;
use std::cell::Cell;

use crate::shared::geometry::{ByteOffset, LineCol, TextRange};

use super::errors::TextBufferError;
use super::value_objects::{Edit, EditPlan};

/// 文本缓冲区的领域接口。
///
/// 一切偏移以**字节**为单位；实现必须 `Send + Sync`（跨线程异步加载大文件）。
///
/// 越界约定：`offset == len_bytes()` 合法（表示末尾）；`offset > len_bytes()` 返回
/// [`TextBufferError::OutOfRange`]。
pub trait TextBuffer: Send + Sync {
    /// 缓冲区字节长度。
    fn len_bytes(&self) -> usize;

    /// 缓冲区字符（Unicode 标量值）数量。
    fn len_chars(&self) -> usize;

    /// 行数。约定：空文本为 1 行；`"a\n"` 为 2 行（末行是空行）。
    fn len_lines(&self) -> usize;

    /// 在 `offset` 处插入文本（不换行语义由调用方决定）。
    fn insert(&mut self, offset: ByteOffset, text: &str) -> Result<(), TextBufferError>;

    /// 删除 `range` 区间，返回被删除的内容。
    fn remove(&mut self, range: TextRange) -> Result<String, TextBufferError>;

    /// 取出 `range` 区间的文本。
    ///
    /// 连续存储时返回借用的 [`Cow::Borrowed`]，需要拼接时返回 [`Cow::Owned`]。
    fn slice(&self, range: TextRange) -> Result<Cow<'_, str>, TextBufferError>;

    /// 全量文本快照。
    ///
    /// 用于撤销快照与保存；大文件场景下由阶段 08 优化为增量快照。
    fn snapshot(&self) -> String;

    /// 字节偏移 → 行列坐标（均为 0-based）。
    fn byte_to_line_col(&self, offset: ByteOffset) -> Result<LineCol, TextBufferError>;

    /// 行列坐标 → 字节偏移。行号越界时返回该缓冲区末尾偏移量。
    fn line_col_to_byte(&self, line_col: LineCol) -> Result<ByteOffset, TextBufferError>;

    /// 第 `line` 行（0-based）起始字节偏移。
    fn line_start_byte(&self, line: usize) -> Result<ByteOffset, TextBufferError>;

    /// 第 `line` 行（0-based）结束字节偏移（**不含**换行符）。
    fn line_end_byte(&self, line: usize) -> Result<ByteOffset, TextBufferError>;

    /// 向下取到最近的 UTF-8 字符边界（`offset` 已在边界上时原样返回）。
    fn char_boundary_floor(&self, offset: ByteOffset) -> ByteOffset;

    /// 向上取到最近的 UTF-8 字符边界（`offset` 已在边界上时原样返回）。
    fn char_boundary_ceil(&self, offset: ByteOffset) -> ByteOffset;
}

/// 编辑规则领域服务（无状态纯函数）。
///
/// 所有函数只依赖传入的文本与坐标，不持有缓冲区、不读系统时间、不做 IO，
/// 因此可被表现层（生成编辑命令）与测试直接复用。
///
/// # 换行约定
///
/// 全部函数同时支持 `LF` 与 `CRLF`：按 `\n` 切分，行尾可能残留的 `\r`
/// 会在处理时剥离并在输出时恢复，因此**不会**把 `\r` 当作行内容的一部分。
pub struct EditingService;

impl EditingService {
    /// 向下取到最近的 UTF-8 字符边界。
    ///
    /// 用于 IME 输入 / 中文选区（`FR-05`）：字节偏移可能落在字符中间，
    /// 直接在非边界处切片会 panic，故所有对外 API 先做对齐。
    pub fn char_floor(text: &str, offset: usize) -> usize {
        let mut i = offset.min(text.len());
        while i > 0 && !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    /// 向上取到最近的 UTF-8 字符边界（语义同 [`EditingService::char_floor`]）。
    pub fn char_ceil(text: &str, offset: usize) -> usize {
        let mut i = offset.min(text.len());
        while !text.is_char_boundary(i) {
            i += 1;
        }
        i
    }

    /// 把区间对齐到字符边界（起点向下、终点向上），避免切出半个字符。
    pub fn align_range(text: &str, range: TextRange) -> TextRange {
        TextRange::new(
            ByteOffset::new(Self::char_floor(text, range.start().value())),
            ByteOffset::new(Self::char_ceil(text, range.end().value())),
        )
    }

    /// `offset` 所在行的起始字节偏移（`offset` 恰为 `\n` 时属于该 `\n` 所在的行）。
    pub fn line_start(text: &str, offset: usize) -> usize {
        let offset = offset.min(text.len());
        match text.as_bytes()[..offset].iter().rposition(|b| *b == b'\n') {
            Some(idx) => idx + 1,
            None => 0,
        }
    }

    /// `offset` 所在行的结束字节偏移（**不含**换行符）。
    pub fn line_end(text: &str, offset: usize) -> usize {
        let offset = offset.min(text.len());
        match text.as_bytes()[offset..].iter().position(|b| *b == b'\n') {
            Some(idx) => offset + idx,
            None => text.len(),
        }
    }

    /// `offset` 所在行的区间（不含换行符）。
    pub fn line_range(text: &str, offset: usize) -> TextRange {
        let start = Self::line_start(text, offset);
        let end = Self::line_end(text, offset);
        TextRange::new(ByteOffset::new(start), ByteOffset::new(end))
    }

    /// 把区间**向外扩展**到覆盖完整的行（不含行尾换行符）。
    ///
    /// 行编辑类操作（缩进、列表前缀、整行移动）必须先调用本函数，
    /// 否则会在行中间插入内容。
    pub fn expand_to_full_lines(text: &str, range: TextRange) -> TextRange {
        let start = Self::line_start(text, range.start().value());
        let probe = if range.is_empty() {
            range.start().value()
        } else {
            range.end().value().saturating_sub(1)
        };
        let end = Self::line_end(text, probe.max(start));
        TextRange::new(ByteOffset::new(start), ByteOffset::new(end))
    }

    /// 用 `prefix` / `suffix` 包裹选区；若已被包裹则**解除**包裹。
    ///
    /// - 空选区：插入 `prefix + suffix`，光标落在两者之间；
    /// - 非空选区：包裹选中文本，返回的选区仍覆盖原文本（不含标记）；
    /// - 已包裹：删除两侧标记，返回的选区覆盖内部文本。
    ///
    /// 越界区间会先被裁剪到文本范围内（不 panic）。
    pub fn wrap_selection(text: &str, range: TextRange, prefix: &str, suffix: &str) -> EditPlan {
        let range = Self::align_range(text, range);
        let start = range.start().value().min(text.len());
        let end = range.end().value().min(text.len()).max(start);
        let selected = text.get(start..end).unwrap_or("");

        let inner_start = start.saturating_sub(prefix.len());
        let already_wrapped = start >= prefix.len()
            && end + suffix.len() <= text.len()
            && text.get(inner_start..start) == Some(prefix)
            && text.get(end..end + suffix.len()) == Some(suffix);

        if already_wrapped {
            let edit = Edit::new(
                TextRange::new(
                    ByteOffset::new(inner_start),
                    ByteOffset::new(end + suffix.len()),
                ),
                selected,
            );
            let selection = TextRange::new(
                ByteOffset::new(inner_start),
                ByteOffset::new(inner_start + selected.len()),
            );
            return EditPlan::new(edit, Some(selection));
        }

        let mut replacement = String::with_capacity(prefix.len() + selected.len() + suffix.len());
        replacement.push_str(prefix);
        replacement.push_str(selected);
        replacement.push_str(suffix);
        let edit = Edit::new(
            TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
            replacement,
        );
        let selection = TextRange::new(
            ByteOffset::new(start + prefix.len()),
            ByteOffset::new(start + prefix.len() + selected.len()),
        );
        EditPlan::new(edit, Some(selection))
    }

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

    /// 对区间覆盖的每一行**切换**行首前缀（有则删、无则加）。
    ///
    /// 典型用法：`> `（引用）、`# ` / `## `（标题）、`- `（列表）。
    pub fn toggle_line_prefix(text: &str, range: TextRange, prefix: &str) -> EditPlan {
        Self::transform_lines(text, range, |line| {
            let stripped = line.strip_prefix(prefix);
            match stripped {
                Some(rest) => rest.to_string(),
                None => format!("{prefix}{line}"),
            }
        })
    }

    /// 对区间覆盖的每一行增加一级缩进。
    ///
    /// `indent_unit` 由调用方从偏好设置的 `tabSize` 推导（`"  "` / `"    "` /
    /// `"        "` / `"\t"`），**不在此处硬编码**，以根治原型缺陷 2.2。
    pub fn indent_lines(text: &str, range: TextRange, indent_unit: &str) -> EditPlan {
        Self::transform_lines(text, range, |line| {
            // 空行不产生尾随空白（验收标准明确要求）。
            if line.is_empty() {
                return String::new();
            }
            format!("{indent_unit}{line}")
        })
    }

    /// 对区间覆盖的每一行减少一级缩进。
    ///
    /// 删除规则：优先删除**恰好一个** `indent_unit`；若行首是制表符则删除一个 `\t`；
    /// 否则删除行首至多 `indent_unit.len()` 个空格。
    pub fn outdent_lines(text: &str, range: TextRange, indent_unit: &str) -> EditPlan {
        Self::transform_lines(text, range, |line| {
            if let Some(rest) = line.strip_prefix(indent_unit) {
                return rest.to_string();
            }
            let mut idx = 0;
            let mut spaces_removed = 0;
            while idx < line.len() && spaces_removed < indent_unit.len().max(1) {
                match line[idx..].chars().next() {
                    // 行首是制表符：无论 `indent_unit` 多宽，都只吃掉一个 `\t`。
                    Some('\t') => {
                        idx += 1;
                        break;
                    }
                    Some(' ') => {
                        idx += 1;
                        spaces_removed += 1;
                    }
                    _ => break,
                }
            }
            line[idx..].to_string()
        })
    }

    /// 在 `cursor` 处按回车时，是否应自动延续当前行的列表标记。
    ///
    /// 返回 `None` 表示当前行不是列表项（调用方按普通回车处理）。
    ///
    /// 边界语义：
    /// - 行内容为 `- `（标记后无内容）→ 返回**删除标记**的编辑（退出列表），而非新增一行；
    /// - 有序列表 `3. ` → 下一行为 `4. `；
    /// - 任务列表 `- [x] ` → 下一行为 `- [ ] `（新任务默认未完成）。
    pub fn continue_list_on_newline(text: &str, cursor: ByteOffset) -> Option<EditPlan> {
        let offset = cursor.value().min(text.len());
        let line = Self::line_range(text, offset);
        let raw = text.get(line.start().value()..line.end().value())?;
        let body = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed_start = body.len() - body.trim_start().len();
        let indent = body.get(..trimmed_start).unwrap_or("");
        let content = body.get(trimmed_start..).unwrap_or("");

        let marker = list_marker_of(content)?;
        if content.len() == marker.len() {
            // 只有标记、没有内容：退出列表而不是继续。
            let start = line.start().value() + trimmed_start;
            let edit = Edit::deletion(TextRange::new(
                ByteOffset::new(start),
                ByteOffset::new(start + marker.len()),
            ));
            return Some(EditPlan::plain(edit));
        }

        let next_marker = next_list_marker(marker);
        let insertion = format!("\n{indent}{next_marker}");
        Some(EditPlan::plain(Edit::insertion(cursor, insertion)))
    }

    /// 把区间覆盖的整块行向上移动一行；已在首行时返回 `None`。
    pub fn move_line_up(text: &str, range: TextRange) -> Option<EditPlan> {
        let block = Self::expand_to_full_lines(text, range);
        if block.start().value() == 0 {
            return None;
        }
        let prev_start = Self::line_start(text, block.start().value().saturating_sub(1));
        let region = TextRange::new(ByteOffset::new(prev_start), block.end());
        let region_text = text.get(region.as_usize_range())?;
        let mut lines: Vec<&str> = region_text.split('\n').collect();
        if lines.len() < 2 {
            return None;
        }
        let block_count = block_line_count(text, &block);
        if block_count >= lines.len() {
            return None;
        }
        // 原顺序：[上一行] [块…] [其余] → [块…] [上一行] [其余]
        let mut reordered: Vec<&str> = Vec::with_capacity(lines.len());
        reordered.extend(lines.iter().take(1 + block_count).skip(1).copied());
        reordered.push(lines[0]);
        reordered.extend(lines.iter().skip(1 + block_count).copied());
        lines = reordered;

        let replacement = lines.join("\n");
        let prev_len = lines[block_count].len() + 1;
        let new_start = block.start().value().saturating_sub(prev_len);
        let selection = TextRange::new(
            ByteOffset::new(new_start),
            ByteOffset::new(new_start + block.len()),
        );
        Some(EditPlan::new(
            Edit::new(region, replacement),
            Some(selection),
        ))
    }

    /// 把区间覆盖的整块行向下移动一行；已在末行时返回 `None`。
    pub fn move_line_down(text: &str, range: TextRange) -> Option<EditPlan> {
        let block = Self::expand_to_full_lines(text, range);
        let next_line_start = block.end().value() + 1; // 跳过本块的 '\n'
        if next_line_start > text.len() {
            return None;
        }
        let next_line_end = Self::line_end(text, next_line_start.min(text.len()));
        let region = TextRange::new(block.start(), ByteOffset::new(next_line_end));
        let region_text = text.get(region.as_usize_range())?;
        let mut lines: Vec<&str> = region_text.split('\n').collect();
        let block_count = block_line_count(text, &block);
        if block_count + 1 > lines.len() {
            return None;
        }
        // 原顺序：[块…] [下一行] → [下一行] [块…]
        let moved = lines[block_count];
        let mut reordered: Vec<&str> = Vec::with_capacity(lines.len());
        reordered.push(moved);
        reordered.extend(lines.iter().take(block_count).copied());
        reordered.extend(lines.iter().skip(block_count + 1).copied());
        lines = reordered;

        let replacement = lines.join("\n");
        let new_start = block.start().value() + moved.len() + 1;
        let selection = TextRange::new(
            ByteOffset::new(new_start),
            ByteOffset::new(new_start + block.len()),
        );
        Some(EditPlan::new(
            Edit::new(region, replacement),
            Some(selection),
        ))
    }

    /// 在光标所在行下方复制该行（光标停留在新行同一列）。
    pub fn duplicate_line(text: &str, cursor: ByteOffset) -> EditPlan {
        let line = Self::line_range(text, cursor.value());
        let raw = text.get(line.as_usize_range()).unwrap_or("");
        let line_text = raw.strip_suffix('\r').unwrap_or(raw);
        let insert_at = line.end();
        let insertion = format!("\n{line_text}");
        EditPlan::plain(Edit::insertion(insert_at, insertion))
    }

    /// 删除光标所在的整行（含其换行符；末行则只删行内容）。
    pub fn delete_line(text: &str, cursor: ByteOffset) -> EditPlan {
        let line = Self::line_range(text, cursor.value());
        // 非末行：连同行尾换行符一起删除；
        // 末行：改为删除其前一个换行符（与上一行合并）；
        // 全文仅一行：删除该行内容。
        let (start, end) = if line.end().value() < text.len() {
            (line.start().value(), line.end().value() + 1)
        } else if line.start().value() > 0 {
            (line.start().value() - 1, line.end().value())
        } else {
            (line.start().value(), line.end().value())
        };
        EditPlan::plain(Edit::deletion(TextRange::new(
            ByteOffset::new(start),
            ByteOffset::new(end),
        )))
    }

    /// 切换光标所在行的任务列表勾选状态。
    ///
    /// 语义：`- [ ] ` ⇄ `- [x] `；普通列表项 `- ` → `- [ ] `；非列表行 → `- [ ] `。
    pub fn toggle_task(text: &str, cursor: ByteOffset) -> EditPlan {
        let line = Self::line_range(text, cursor.value());
        let raw = text.get(line.as_usize_range()).unwrap_or("");
        let body = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed_start = body.len() - body.trim_start().len();
        let indent = body.get(..trimmed_start).unwrap_or("");
        let content = body.get(trimmed_start..).unwrap_or("");

        let new_content = if let Some(rest) = content.strip_prefix("- [ ] ") {
            format!("- [x] {rest}")
        } else if let Some(rest) = content.strip_prefix("- [x] ") {
            format!("- [ ] {rest}")
        } else if let Some(rest) = content.strip_prefix("- ") {
            format!("- [ ] {rest}")
        } else {
            format!("- [ ] {content}")
        };
        let replacement = format!("{indent}{new_content}");
        EditPlan::plain(Edit::new(line, replacement))
    }

    /// 把选区（或占位文本）转为 Markdown 链接，并把选区落在 URL 上。
    pub fn insert_link(text: &str, range: TextRange) -> EditPlan {
        let range = Self::align_range(text, range);
        let start = range.start().value().min(text.len());
        let end = range.end().value().min(text.len()).max(start);
        let selected = text.get(start..end).unwrap_or("");
        let label = if selected.is_empty() {
            "链接文本"
        } else {
            selected
        };
        let replacement = format!("[{label}](url)");
        let url_start = start + 1 + label.len() + 2;
        let selection = TextRange::new(ByteOffset::new(url_start), ByteOffset::new(url_start + 3));
        EditPlan::new(
            Edit::new(
                TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
                replacement,
            ),
            Some(selection),
        )
    }

    /// 把选区包进围栏代码块，并把选区落在代码内容上。
    pub fn insert_code_block(text: &str, range: TextRange) -> EditPlan {
        let range = Self::align_range(text, range);
        let start = range.start().value().min(text.len());
        let end = range.end().value().min(text.len()).max(start);
        let selected = text.get(start..end).unwrap_or("");
        let replacement = format!("```\n{selected}\n```");
        let selection = TextRange::new(
            ByteOffset::new(start + 4),
            ByteOffset::new(start + 4 + selected.len()),
        );
        EditPlan::new(
            Edit::new(
                TextRange::new(ByteOffset::new(start), ByteOffset::new(end)),
                replacement,
            ),
            Some(selection),
        )
    }

    /// 在 `at` 处插入一个 GFM 表格骨架（不含表体内容）。
    pub fn insert_table(at: ByteOffset, rows: usize, cols: usize) -> EditPlan {
        let rows = rows.clamp(1, 100);
        let cols = cols.clamp(1, 32);
        let mut header = String::from("|");
        let mut divider = String::from("|");
        for c in 0..cols {
            header.push_str(&format!(" 列 {} |", c + 1));
            divider.push_str(" --- |");
        }
        let mut out = header;
        out.push('\n');
        out.push_str(&divider);
        for _ in 0..rows {
            out.push('\n');
            out.push('|');
            for _ in 0..cols {
                out.push_str("  |");
            }
        }
        EditPlan::plain(Edit::insertion(at, out))
    }

    /// 逐行变换区间覆盖的所有行（内部处理 `CRLF`），返回替换编辑。
    ///
    /// `transform` 收到的是**已剥离行尾 `\r`** 的行内容。
    fn transform_lines(
        text: &str,
        range: TextRange,
        transform: impl Fn(&str) -> String,
    ) -> EditPlan {
        let region = Self::expand_to_full_lines(text, range);
        let region_text = text.get(region.as_usize_range()).unwrap_or("");
        let mut out = String::with_capacity(region_text.len() + 16);
        for (idx, raw) in region_text.split('\n').enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            let (body, carriage) = match raw.strip_suffix('\r') {
                Some(b) => (b, true),
                None => (raw, false),
            };
            out.push_str(&transform(body));
            if carriage {
                out.push('\r');
            }
        }
        EditPlan::plain(Edit::new(region, out))
    }
}

/// 识别行内容的列表标记（含其后空格）；不是列表行时返回 `None`。
fn list_marker_of(content: &str) -> Option<&str> {
    for marker in ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "+ "] {
        if let Some(rest) = content.get(..marker.len())
            && rest.eq_ignore_ascii_case(marker)
        {
            return content.get(..marker.len());
        }
    }
    // 有序列表：`1. ` / `10) `
    let bytes = content.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == 0 || idx >= bytes.len() {
        return None;
    }
    // `1. ` 这类带尾随空格的标记长度为「数字 + 分隔符 + 空格」，故分隔符部分计 2。
    let delim_len = match bytes[idx] {
        b'.' | b')' if bytes.len() > idx + 1 && bytes[idx + 1] == b' ' => 2,
        b'.' | b')' => 1,
        _ => return None,
    };
    let end = idx + delim_len;
    content.get(..end).filter(|marker| !marker.is_empty())
}

/// 生成下一行的列表标记（有序列表递增，任务列表重置为未完成）。
fn next_list_marker(marker: &str) -> String {
    if marker.eq_ignore_ascii_case("- [x] ") {
        return "- [ ] ".to_string();
    }
    let trimmed = marker.trim_end_matches(['.', ')', ' ']);
    if let Ok(number) = trimmed.parse::<u64>() {
        let delim = marker.chars().nth(trimmed.len()).unwrap_or('.');
        return format!("{}{} ", number.saturating_add(1), delim);
    }
    marker.to_string()
}

/// 计算 `block`（已扩展到整行）覆盖的行数，至少为 1。
fn block_line_count(text: &str, block: &TextRange) -> usize {
    let slice = text.get(block.as_usize_range()).unwrap_or("");
    slice.split('\n').count().max(1)
}

/// 缓冲区句柄别名（与聚合根内定义一致，重导出以集中引用）。
///
/// 应用层据此持有「可替换的缓冲区实现」，不直接依赖 `ropey`。
pub use super::aggregate::BufferHandle;

/// 缓冲区工厂：由基础设施层注入具体的 [`TextBuffer`] 实现（如 `RopeTextBuffer`）。
///
/// 应用层需把文本装进缓冲区后再构造 `Document`，但**不得**直接依赖 `ropey`，
/// 故通过本 trait 在组合根处注入实现，满足「应用层 → 领域，基础设施反向注入」的依赖矩阵。
pub trait BufferFactory: Send + Sync {
    /// 用给定内容创建一个缓冲区句柄。
    fn create(&self, content: &str) -> BufferHandle;
}
