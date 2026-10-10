//! 行级 / 字符级差异引擎与 Diff 浮层状态（S6-11f）。
//!
//! # 为什么自己实现而不引第三方 diff crate
//!
//! 与 S6-08 的查找匹配器同源：`similar` / `dissimilar` 会引入一套正则依赖，
//! 而本项目对差异的需求只有「按行对齐 + 标出增删」这一条。
//! 更关键的是**降级策略必须自己写**：LCS 的动态规划是 `O(n·m)`，
//! 两个 5000 行的文件就是 2500 万个格子（100 MB 的 `u32` 表），
//! 在 UI 线程里跑会直接卡死窗口。原型的 `lib/diff.ts` 用
//! `MAX_CELLS = 2_000_000` 退化，本次沿用同一阈值与同一退化语义。
//!
//! # 为什么这个文件在 `presentation/state` 而不是 `domain`
//!
//! 差异计算是**纯函数**，不涉及任何聚合与不变量，`domain` 的收益为零。
//! 而 `find_replace_ui_state`（查找匹配器）已经在同一位置立了先例，
//! 两处保持一致比「新东西放对地方」更重要。
//!
//! # 字符级 diff 只用于「替换」场景
//!
//! 行级 diff 只能说明「这一行变了」，说不出变了哪几个字。
//! 原型的做法是：把**紧邻**的删除行与新增行配对成「替换」，
//! 再对这一对做字符级 LCS。只有这样用户才看得到
//! 「`Ctrl+S` 改成了 `Ctrl+Shift+S`」而不是整行红绿。

/// 触发降级的最大比较规模（行数乘积）。
///
/// 与原型 `lib/diff.ts` 的 `MAX_CELLS` 取同值：约 1400×1400 行。
/// 再大就该换 Myers 算法而不是继续等。
pub const MAX_CELLS: usize = 2_000_000;

/// 行级差异类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiffOp {
    /// 两侧相同。
    Equal,
    /// 仅右侧（当前文档）有。
    Add,
    /// 仅左侧（对照文件）有。
    Remove,
}

/// 一行差异结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiffRow {
    /// 差异类型。
    pub op: DiffOp,
    /// 行文本。
    pub text: String,
    /// 左侧行号（1-based；`Add` 行为 `None`）。
    pub old_no: Option<usize>,
    /// 右侧行号（1-based；`Remove` 行为 `None`）。
    pub new_no: Option<usize>,
}

impl DiffRow {
    /// 构造相等行。
    fn equal(text: &str, old_no: usize, new_no: usize) -> Self {
        Self {
            op: DiffOp::Equal,
            text: text.to_string(),
            old_no: Some(old_no),
            new_no: Some(new_no),
        }
    }

    /// 构造删除行（仅左侧有）。
    fn remove(text: &str, old_no: usize) -> Self {
        Self {
            op: DiffOp::Remove,
            text: text.to_string(),
            old_no: Some(old_no),
            new_no: None,
        }
    }

    /// 构造新增行（仅右侧有）。
    fn add(text: &str, new_no: usize) -> Self {
        Self {
            op: DiffOp::Add,
            text: text.to_string(),
            old_no: None,
            new_no: Some(new_no),
        }
    }
}

/// 增删行数统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DiffStat {
    /// 新增行数。
    pub added: usize,
    /// 删除行数。
    pub removed: usize,
}

/// 字符级差异段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharSegKind {
    /// 两侧共有。
    Equal,
    /// 仅新串有。
    Add,
    /// 仅旧串有。
    Remove,
}

/// 一段字符级差异。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CharSeg {
    /// 段类型。
    pub kind: CharSegKind,
    /// 段文本。
    pub text: String,
}

impl CharSeg {
    fn new(kind: CharSegKind, text: &str) -> Self {
        Self {
            kind,
            text: text.to_string(),
        }
    }
}

/// 渲染用的一行：把紧邻的删除 + 新增合并成「替换」，并带上字符级差异。
///
/// 与原型 `DiffDialog.vue` 的 `DisplayRow` 同构。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DisplayRow {
    /// 两侧相同。
    Equal {
        /// 行文本。
        text: String,
        /// 左侧行号。
        old_no: usize,
        /// 右侧行号。
        new_no: usize,
    },
    /// 删除行。
    Remove {
        /// 行文本。
        text: String,
        /// 左侧行号。
        old_no: usize,
    },
    /// 新增行。
    Add {
        /// 行文本。
        text: String,
        /// 右侧行号。
        new_no: usize,
    },
    /// 替换行：同一位置上的旧行 → 新行，逐字符标出变化。
    Replace {
        /// 旧行文本。
        old_text: String,
        /// 新行文本。
        new_text: String,
        /// 左侧行号。
        old_no: usize,
        /// 右侧行号。
        new_no: usize,
        /// 旧行的字符级差异段。
        old_segs: Vec<CharSeg>,
        /// 新行的字符级差异段。
        new_segs: Vec<CharSeg>,
    },
}

impl DisplayRow {
    /// 该行属于哪一侧有 / 两侧都有的哪一类（只关心「是不是新增」）。
    pub const fn is_add(&self) -> bool {
        matches!(self, Self::Add { .. } | Self::Replace { .. })
    }

    /// 该行是否含删除内容。
    pub const fn is_remove(&self) -> bool {
        matches!(self, Self::Remove { .. } | Self::Replace { .. })
    }

    /// 左侧行号（若有）。
    pub fn old_no(&self) -> Option<usize> {
        match self {
            Self::Equal { old_no, .. }
            | Self::Remove { old_no, .. }
            | Self::Replace { old_no, .. } => Some(*old_no),
            Self::Add { .. } => None,
        }
    }

    /// 右侧行号（若有）。
    pub fn new_no(&self) -> Option<usize> {
        match self {
            Self::Equal { new_no, .. }
            | Self::Add { new_no, .. }
            | Self::Replace { new_no, .. } => Some(*new_no),
            Self::Remove { .. } => None,
        }
    }
}

/// 两侧是否超出降级阈值。
///
/// 单独暴露而不是让 `diff_lines` 顺带返回：界面要**先**知道会降级，
/// 才能在渲染前就显示提示，而 [`diff_lines`] 只返回行。
pub fn exceeds_budget(left: &str, right: &str) -> bool {
    let n = if left.is_empty() {
        0
    } else {
        left.split('\n').count()
    };
    let m = if right.is_empty() {
        0
    } else {
        right.split('\n').count()
    };
    n * m > MAX_CELLS
}

/// 行级文本差异（基于最长公共子序列）。
///
/// 空文档对空文档返回**空**而不是一行空串：`"".split('\n')` 产出 `[""]`，
/// 不特判就会显示一行「两侧都有的空行」，用户会以为文件里真有一行。
pub fn diff_lines(a: &str, b: &str) -> Vec<DiffRow> {
    if a == b {
        // 两侧相同：逐行标 `Equal`。仍走一遍而非直接返回空——
        // 用户需要看到「未改动的行」才能确认比较确实覆盖了全文。
        if a.is_empty() {
            return Vec::new();
        }
        let mut rows = Vec::new();
        for (i, line) in a.split('\n').enumerate() {
            rows.push(DiffRow::equal(line, i + 1, i + 1));
        }
        return rows;
    }
    if a.is_empty() || b.is_empty() {
        let mut rows = Vec::new();
        if !a.is_empty() {
            for (i, line) in a.split('\n').enumerate() {
                rows.push(DiffRow::remove(line, i + 1));
            }
        }
        if !b.is_empty() {
            for (j, line) in b.split('\n').enumerate() {
                rows.push(DiffRow::add(line, j + 1));
            }
        }
        return rows;
    }

    let left: Vec<&str> = a.split('\n').collect();
    let right: Vec<&str> = b.split('\n').collect();
    let (n, m) = (left.len(), right.len());

    if n * m > MAX_CELLS {
        // 降级：整段替换。至少能看出「变了」，但看不出变了哪里——
        // 这一点必须由界面**明说**，否则用户会以为没有差异。
        let mut rows = Vec::with_capacity(n + m);
        for (i, line) in left.iter().enumerate() {
            rows.push(DiffRow::remove(line, i + 1));
        }
        for (j, line) in right.iter().enumerate() {
            rows.push(DiffRow::add(line, j + 1));
        }
        return rows;
    }

    // dp[i * width + j] = left[i..] 与 right[j..] 的 LCS 长度。
    // 展平成一维而非二维：二维 `Vec<Vec<u32>>` 有 n+1 次额外分配，
    // 而这正是要在大文档上跑的路径。
    let width = m + 1;
    let mut dp = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * width + j] = if left[i] == right[j] {
                dp[(i + 1) * width + j + 1] + 1
            } else {
                dp[(i + 1) * width + j].max(dp[i * width + j + 1])
            };
        }
    }

    let mut rows = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    let (mut old_no, mut new_no) = (1usize, 1usize);
    while i < n && j < m {
        if left[i] == right[j] {
            rows.push(DiffRow::equal(left[i], old_no, new_no));
            i += 1;
            j += 1;
            old_no += 1;
            new_no += 1;
        } else if dp[(i + 1) * width + j] >= dp[i * width + j + 1] {
            rows.push(DiffRow::remove(left[i], old_no));
            i += 1;
            old_no += 1;
        } else {
            rows.push(DiffRow::add(right[j], new_no));
            j += 1;
            new_no += 1;
        }
    }
    while i < n {
        rows.push(DiffRow::remove(left[i], old_no));
        i += 1;
        old_no += 1;
    }
    while j < m {
        rows.push(DiffRow::add(right[j], new_no));
        j += 1;
        new_no += 1;
    }
    rows
}

/// 增删行数统计（**基于行级原始结果**，不受渲染合并影响）。
pub fn diff_stat(rows: &[DiffRow]) -> DiffStat {
    rows.iter().fold(DiffStat::default(), |mut acc, row| {
        match row.op {
            DiffOp::Add => acc.added += 1,
            DiffOp::Remove => acc.removed += 1,
            DiffOp::Equal => {}
        }
        acc
    })
}

/// 字符级差异（按 `char` 而非字节）。
///
/// 按字节会在多字节字符上算错：「你」是 3 字节，删掉中间那个字节
/// 会得到两个无法解码的片段，且后续按段绘制时排版宽度也对不上。
pub fn diff_chars(a: &str, b: &str) -> (Vec<CharSeg>, Vec<CharSeg>) {
    let left: Vec<char> = a.chars().collect();
    let right: Vec<char> = b.chars().collect();
    let (n, m) = (left.len(), right.len());
    if n == 0 || m == 0 {
        let old = if n == 0 {
            Vec::new()
        } else {
            vec![CharSeg::new(CharSegKind::Remove, a)]
        };
        let new = if m == 0 {
            Vec::new()
        } else {
            vec![CharSeg::new(CharSegKind::Add, b)]
        };
        return (old, new);
    }

    // 单行字符级 diff 的规模远小于行级，但仍设上限：
    // 两个 5000 字符的单行（压缩后的 JSON）会算出 2500 万个格子。
    if n * m > MAX_CELLS {
        return (
            vec![CharSeg::new(CharSegKind::Remove, a)],
            vec![CharSeg::new(CharSegKind::Add, b)],
        );
    }

    let width = m + 1;
    let mut dp = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * width + j] = if left[i] == right[j] {
                dp[(i + 1) * width + j + 1] + 1
            } else {
                dp[(i + 1) * width + j].max(dp[i * width + j + 1])
            };
        }
    }

    let mut old_segs: Vec<CharSeg> = Vec::new();
    let mut new_segs: Vec<CharSeg> = Vec::new();
    let push = |segs: &mut Vec<CharSeg>, kind: CharSegKind, ch: char| match segs.last_mut() {
        Some(last) if last.kind == kind => last.text.push(ch),
        _ => segs.push(CharSeg {
            kind,
            text: ch.to_string(),
        }),
    };
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if left[i] == right[j] {
            push(&mut old_segs, CharSegKind::Equal, left[i]);
            push(&mut new_segs, CharSegKind::Equal, right[j]);
            i += 1;
            j += 1;
        } else if dp[(i + 1) * width + j] >= dp[i * width + j + 1] {
            push(&mut old_segs, CharSegKind::Remove, left[i]);
            i += 1;
        } else {
            push(&mut new_segs, CharSegKind::Add, right[j]);
            j += 1;
        }
    }
    while i < n {
        push(&mut old_segs, CharSegKind::Remove, left[i]);
        i += 1;
    }
    while j < m {
        push(&mut new_segs, CharSegKind::Add, right[j]);
        j += 1;
    }
    (old_segs, new_segs)
}

/// 把行级结果整理成渲染用的行：紧邻的删除 + 新增合并成「替换」。
///
/// # 为什么只在**紧邻**时合并
///
/// 行级 LCS 的输出天然把一段删除与一段新增排在一起，但它们**不一定**
/// 对应同一处修改。跨过相等行去配对会把两处无关的改动说成一处，
/// 用户会照着「这一行从 A 改成了 B」去找，实际上 A 变成了别的东西。
///
/// 行数不等时按位置配对，多出来的一侧退化为纯删除 / 纯新增——
/// 宁可少合并，也不要错合并。
pub fn to_display_rows(rows: &[DiffRow]) -> Vec<DisplayRow> {
    let mut out: Vec<DisplayRow> = Vec::with_capacity(rows.len());
    let mut i = 0usize;
    while i < rows.len() {
        if rows[i].op != DiffOp::Remove {
            out.push(match rows[i].op {
                DiffOp::Equal => DisplayRow::Equal {
                    text: rows[i].text.clone(),
                    old_no: rows[i].old_no.unwrap_or_default(),
                    new_no: rows[i].new_no.unwrap_or_default(),
                },
                _ => DisplayRow::Add {
                    text: rows[i].text.clone(),
                    new_no: rows[i].new_no.unwrap_or_default(),
                },
            });
            i += 1;
            continue;
        }
        // 从这里收集连续的删除，再看紧随其后连续的**新增**。
        let remove_start = i;
        while i < rows.len() && rows[i].op == DiffOp::Remove {
            i += 1;
        }
        let removes = &rows[remove_start..i];
        let add_start = i;
        while i < rows.len() && rows[i].op == DiffOp::Add {
            i += 1;
        }
        let adds = &rows[add_start..i];
        // 没有对应的新增：纯删除。
        if adds.is_empty() {
            for row in removes {
                out.push(DisplayRow::Remove {
                    text: row.text.clone(),
                    old_no: row.old_no.unwrap_or_default(),
                });
            }
            continue;
        }
        // 两侧行数不等时，尾部多出来的一侧按纯增 / 纯删出。
        // `get(..)` 而非 `[..]`：当删除侧比新增侧**少**时
        // `adds[removes.len()..]` 的起点越界，直接切会 panic
        // （`panic = "abort"` 下等于终止进程）。
        for row in removes.get(adds.len()..).unwrap_or_default() {
            out.push(DisplayRow::Remove {
                text: row.text.clone(),
                old_no: row.old_no.unwrap_or_default(),
            });
        }
        for row in adds.get(removes.len()..).unwrap_or_default() {
            out.push(DisplayRow::Add {
                text: row.text.clone(),
                new_no: row.new_no.unwrap_or_default(),
            });
        }
        for (old, new) in removes.iter().zip(adds.iter()) {
            let (old_segs, new_segs) = diff_chars(&old.text, &new.text);
            out.push(DisplayRow::Replace {
                old_text: old.text.clone(),
                new_text: new.text.clone(),
                old_no: old.old_no.unwrap_or_default(),
                new_no: new.new_no.unwrap_or_default(),
                old_segs,
                new_segs,
            });
        }
    }
    out
}

/// Diff 浮层状态。
///
/// 只存**选择与结果**，不缓存文件内容：内容随时可能被编辑区改掉，
/// 缓存必然出现「对比的是三秒前的版本」而界面毫无提示。
#[derive(Clone, Debug, Default)]
pub struct DiffUiState {
    /// 右侧（对照）文件的路径；`None` 表示尚未选择。
    pub right_path: Option<String>,
    /// 左侧（当前）文件标题，仅显示用。
    pub left_title: String,
    /// 右侧文件标题，仅显示用。
    pub right_title: String,
    /// 渲染用行。
    pub rows: Vec<DisplayRow>,
    /// 增删统计。
    pub stat: DiffStat,
    /// 右侧文件读取失败的原因（保留选择，让用户知道是「这一个文件」的问题）。
    pub error: Option<String>,
    /// 差异是否因规模超限而降级（界面必须明说）。
    pub degraded: bool,
    /// 是否只显示有差异的行。
    pub only_changes: bool,
}

impl DiffUiState {
    /// 构造空状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 打开（或切换）对照文件。
    pub fn open(&mut self, left_title: String, right_path: String, right_title: String) {
        self.left_title = left_title;
        self.right_path = Some(right_path);
        self.right_title = right_title;
        self.rows.clear();
        self.stat = DiffStat::default();
        self.error = None;
        self.degraded = false;
        // `only_changes` 不重置：它是用户的查看偏好，不是这次打开的一部分。
    }

    /// 用两侧文本重算差异。
    pub fn recompute(&mut self, left: &str, right: &str) {
        let raw = diff_lines(left, right);
        self.stat = diff_stat(&raw);
        self.rows = to_display_rows(&raw);
        // 「本次比较是否降级」覆盖上一轮的结果：重算一次就把状态清干净，
        // 否则用户换了个小文件之后仍被上一次的降级提示纠缠。
        self.degraded = exceeds_budget(left, right);
    }

    /// 可见行（按「只显示差异」过滤）。
    pub fn visible_rows(&self) -> Vec<&DisplayRow> {
        self.rows
            .iter()
            .filter(|row| !self.only_changes || !matches!(row, DisplayRow::Equal { .. }))
            .collect()
    }

    /// 是否两侧完全一致。
    pub fn is_identical(&self) -> bool {
        self.stat.added == 0 && self.stat.removed == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops(rows: &[DiffRow]) -> Vec<DiffOp> {
        rows.iter().map(|r| r.op).collect()
    }

    #[test]
    fn two_empty_documents_produce_no_rows() {
        assert_eq!(diff_lines("", ""), Vec::new());
    }

    #[test]
    fn empty_left_yields_only_additions() {
        let rows = diff_lines("", "a\nb");
        assert_eq!(ops(&rows), vec![DiffOp::Add, DiffOp::Add]);
        assert_eq!(
            diff_stat(&rows),
            DiffStat {
                added: 2,
                removed: 0
            }
        );
    }

    #[test]
    fn identical_documents_are_all_equal() {
        let rows = diff_lines("a\nb\nc", "a\nb\nc");
        assert!(rows.iter().all(|r| r.op == DiffOp::Equal));
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].old_no, Some(3));
        assert_eq!(rows[2].new_no, Some(3));
    }

    #[test]
    fn one_line_replaced_marks_both_sides() {
        let rows = diff_lines("a\nold\nc", "a\nnew\nc");
        assert_eq!(
            ops(&rows),
            vec![DiffOp::Equal, DiffOp::Remove, DiffOp::Add, DiffOp::Equal]
        );
        let stat = diff_stat(&rows);
        assert_eq!(stat.added, 1);
        assert_eq!(stat.removed, 1);
    }

    #[test]
    fn line_numbers_are_independent_per_side() {
        let rows = diff_lines("a\nb\nc\nd", "a\nX\nc\nd");
        let removed = rows.iter().find(|r| r.op == DiffOp::Remove).unwrap();
        assert_eq!(removed.old_no, Some(2));
        assert_eq!(removed.new_no, None, "删除行不应有右侧行号");
        let added = rows.iter().find(|r| r.op == DiffOp::Add).unwrap();
        assert_eq!(added.new_no, Some(2));
    }

    #[test]
    fn degenerate_when_cell_budget_exceeded() {
        // 构造一个必然超预算的**行数**规模：MAX_CELLS 的平方根约 1415。
        // 必须逐行拼接——不换行的话 `split('\n')` 只有 1 行，
        // 表格大小是 1 而非 2000²，压根不会触发降级。
        let n = 2000;
        let a: Vec<String> = (0..n).map(|i| (i % 2).to_string()).collect();
        let b: Vec<String> = (0..n).map(|i| (i % 3).to_string()).collect();
        let (a, b) = (a.join("\n"), b.join("\n"));
        assert!(exceeds_budget(&a, &b), "前提：这份规模必须超预算");
        let rows = diff_lines(&a, &b);
        // 降级时全部标为增删，没有任何相等行。
        assert!(rows.iter().all(|r| r.op != DiffOp::Equal));
        assert_eq!(rows.len(), n + n);
    }

    #[test]
    fn char_diff_marks_only_changed_characters() {
        let (old_segs, new_segs) = diff_chars("abc", "abd");
        assert_eq!(old_segs.len(), 2);
        assert_eq!(old_segs[1].kind, CharSegKind::Remove);
        assert_eq!(old_segs[1].text, "c");
        assert_eq!(new_segs[1].kind, CharSegKind::Add);
        assert_eq!(new_segs[1].text, "d");
    }

    #[test]
    fn char_diff_is_multibyte_safe() {
        let (old_segs, _) = diff_chars("你好", "你呀");
        let rebuilt: String = old_segs.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(rebuilt, "你好");
        assert_eq!(old_segs.last().unwrap().kind, CharSegKind::Remove);
    }

    #[test]
    fn char_diff_of_empty_side_is_a_single_segment() {
        let (old_segs, new_segs) = diff_chars("", "x");
        assert!(old_segs.is_empty());
        assert_eq!(new_segs.len(), 1);
        assert_eq!(new_segs[0].kind, CharSegKind::Add);
    }

    #[test]
    fn adjacent_remove_and_add_become_one_replace_row() {
        let raw = diff_lines("a\nold\nc", "a\nnew\nc");
        let display = to_display_rows(&raw);
        assert_eq!(display.len(), 3);
        match &display[1] {
            DisplayRow::Replace {
                old_text,
                new_text,
                old_segs,
                ..
            } => {
                assert_eq!(old_text, "old");
                assert_eq!(new_text, "new");
                assert!(old_segs.iter().any(|s| s.kind == CharSegKind::Remove));
            }
            other => panic!("应为替换行，实际 {other:?}"),
        }
    }

    #[test]
    fn pure_removal_does_not_become_replace() {
        let raw = diff_lines("a\nb\nc", "a\nc");
        let display = to_display_rows(&raw);
        assert!(matches!(display[1], DisplayRow::Remove { .. }));
    }

    #[test]
    fn unbalanced_sides_keep_the_extra_rows_pure() {
        // 删 2 行、加 1 行：配对后多出的那行删除必须保持 Remove。
        let raw = diff_lines("a\nb1\nb2\nc", "a\nx\nc");
        let display = to_display_rows(&raw);
        let replaced = display
            .iter()
            .filter(|r| matches!(r, DisplayRow::Replace { .. }))
            .count();
        assert_eq!(replaced, 1);
        assert!(
            display
                .iter()
                .any(|r| matches!(r, DisplayRow::Remove { .. }))
        );
    }

    /// 反向不平衡：删 1 行、加 2 行。
    ///
    /// 上一条只测了「删除侧**多于**新增侧」。这一侧才是当初
    /// `adds[removes.len()..]` 越界 panic 的真实场景：
    /// 起点 `adds.len() = 2` 落在长度只有 1 的 `removes` 之后。
    /// 缺了它，把 `get(..)` 改回 `[..]` 也能全绿（变异测试实测）。
    #[test]
    fn more_additions_than_removals_does_not_overrun_the_remove_side() {
        let raw = diff_lines("a\nold\nc", "a\nx\ny\nc");
        let display = to_display_rows(&raw);
        assert_eq!(
            display
                .iter()
                .filter(|r| matches!(r, DisplayRow::Replace { .. }))
                .count(),
            1
        );
        // 多出来的那行新增必须是纯 Add，不能被吞掉。
        assert_eq!(
            display
                .iter()
                .filter(|r| matches!(r, DisplayRow::Add { .. }))
                .count(),
            1,
            "删除侧比新增侧短时，多出的新增行必须原样保留：{display:?}"
        );
    }

    /// 手工构造的极端形态：删除侧完全为空（只有新增）。
    ///
    /// 走 `diff_lines` 时「无删除」会在 `adds.is_empty()` 那条
    /// 提前返回里绕开，触不到越界那行；这里直接喂 `DiffRow`
    /// 才能把 `to_display_rows` 的边界单独钉住。
    #[test]
    fn a_remove_block_with_no_matching_add_never_overruns() {
        let raw = vec![DiffRow::add("n1", 1), DiffRow::remove("r1", 1)];
        let display = to_display_rows(&raw);
        assert_eq!(display.len(), 2);
    }

    #[test]
    fn replace_never_pairs_across_an_equal_row() {
        let raw = vec![
            DiffRow::remove("x", 1),
            DiffRow::equal("mid", 2, 2),
            DiffRow::add("y", 2),
        ];
        let display = to_display_rows(&raw);
        assert!(
            display
                .iter()
                .all(|r| !matches!(r, DisplayRow::Replace { .. }))
        );
    }

    #[test]
    fn budget_check_agrees_with_the_actual_degradation() {
        let small = "a\nb";
        assert!(!exceeds_budget(small, small));
        // 超预算时 `diff_lines` 必须真的不含相等行，否则提示与结果自相矛盾。
        let n = 2000;
        let a: Vec<String> = (0..n).map(|i| (i % 2).to_string()).collect();
        let b: Vec<String> = (0..n).map(|i| (i % 3).to_string()).collect();
        let (a, b) = (a.join("\n"), b.join("\n"));
        assert!(exceeds_budget(&a, &b));
        assert!(diff_lines(&a, &b).iter().all(|r| r.op != DiffOp::Equal));
    }

    #[test]
    fn recompute_resets_the_degraded_flag() {
        let n = 2000;
        let a: Vec<String> = (0..n).map(|i| (i % 2).to_string()).collect();
        let b: Vec<String> = (0..n).map(|i| (i % 3).to_string()).collect();
        let (a, b) = (a.join("\n"), b.join("\n"));
        let mut state = DiffUiState::new();
        state.recompute(&a, &b);
        assert!(state.degraded);
        state.recompute("x", "y");
        assert!(!state.degraded, "换小文件后降级提示必须消失");
        assert!(!state.is_identical());
        state.recompute("same", "same");
        assert!(state.is_identical());
    }

    #[test]
    fn only_changes_filters_equal_rows() {
        let raw = diff_lines("a\nold\nc", "a\nnew\nc");
        let mut state = DiffUiState::new();
        state.rows = to_display_rows(&raw);
        assert_eq!(state.visible_rows().len(), 3);
        state.only_changes = true;
        assert_eq!(state.visible_rows().len(), 1);
    }

    #[test]
    fn stat_comes_from_line_level_not_merged_rows() {
        let raw = diff_lines("a\nb1\nb2\nc", "a\nx\ny\nc");
        let display = to_display_rows(&raw);
        let from_rows = display.iter().filter(|r| r.is_add()).count();
        // 合并成 2 个 Replace 后行数变少，但统计必须仍是原始的 2 新增 2 删除。
        assert_eq!(from_rows, 2);
        assert_eq!(
            diff_stat(&raw),
            DiffStat {
                added: 2,
                removed: 2
            }
        );
    }
}
