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
//!
//! # 模块结构
//!
//! facade 保留浮层状态 `DiffUiState` 与全部内联测试；
//! 差异数据模型（行 / 字符段 / 渲染行 / 阈值）在 `model`，
//! LCS 算法与渲染行合并在 `algorithm`。

mod algorithm;
mod model;

pub use algorithm::{diff_chars, diff_lines, diff_stat, exceeds_budget, to_display_rows};
pub use model::{CharSeg, CharSegKind, DiffOp, DiffRow, DiffStat, DisplayRow, MAX_CELLS};

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
