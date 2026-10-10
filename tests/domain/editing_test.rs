//! `EditingService` 纯函数单测（S3-05，≥25 条）。

use rustmark::domain::document::services::EditingService;
use rustmark::domain::document::value_objects::{Edit, EditPlan};
use rustmark::shared::geometry::{ByteOffset, TextRange};

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(ByteOffset::new(start), ByteOffset::new(end))
}

/// 把编辑计划应用到文本（测试辅助）。
fn apply(text: &str, plan: EditPlan) -> String {
    plan.edit().apply(text).expect("编辑应用失败")
}

// ---------- wrap_selection ----------

#[test]
fn wrap_selection_empty_inserts_marker_pair() {
    let plan = EditingService::wrap_selection("hello", range(0, 0), "**", "**");
    assert_eq!(apply("hello", plan), "****hello");
}

#[test]
fn wrap_selection_empty_places_selection_between_markers() {
    let plan = EditingService::wrap_selection("", range(0, 0), "**", "**");
    assert_eq!(
        plan.selection(),
        Some(range(2, 2)),
        "光标应落在两个标记之间"
    );
}

#[test]
fn wrap_selection_wraps_selected_text() {
    let plan = EditingService::wrap_selection("hello world", range(0, 5), "**", "**");
    let selection = plan.selection();
    assert_eq!(apply("hello world", plan), "**hello** world");
    assert_eq!(selection, Some(range(2, 7)), "选区应覆盖被包裹的文本");
}

#[test]
fn wrap_selection_unwraps_when_already_wrapped() {
    let plan = EditingService::wrap_selection("**hello** world", range(2, 7), "**", "**");
    let selection = plan.selection();
    assert_eq!(apply("**hello** world", plan), "hello world");
    assert_eq!(selection, Some(range(0, 5)));
}

#[test]
fn wrap_selection_handles_cjk_boundaries() {
    let text = "中文abc";
    let plan = EditingService::wrap_selection(text, range(6, 9), "`", "`");
    assert_eq!(apply(text, plan), "中文`abc`");
}

#[test]
fn wrap_selection_clamps_out_of_range() {
    // 故意传入越界区间：应被裁剪到文本范围内，而不是 panic。
    let plan = EditingService::wrap_selection("abc", range(0, 99), "**", "**");
    assert_eq!(apply("abc", plan), "**abc**");
}

// ---------- 行级操作 ----------

#[test]
fn toggle_line_prefix_adds_prefix() {
    let plan = EditingService::toggle_line_prefix("a\nb\n", range(0, 4), "> ");
    assert_eq!(apply("a\nb\n", plan), "> a\n> b\n");
}

#[test]
fn toggle_line_prefix_removes_existing_prefix() {
    let plan = EditingService::toggle_line_prefix("> a\n> b\n", range(0, 8), "> ");
    assert_eq!(apply("> a\n> b\n", plan), "a\nb\n");
}

#[test]
fn indent_lines_respects_tab_size_two() {
    let plan = EditingService::indent_lines("a\nb\n", range(0, 4), "  ");
    assert_eq!(apply("a\nb\n", plan), "  a\n  b\n");
}

#[test]
fn indent_lines_respects_tab_size_four() {
    let plan = EditingService::indent_lines("a\nb\n", range(0, 4), "    ");
    assert_eq!(apply("a\nb\n", plan), "    a\n    b\n");
}

#[test]
fn indent_lines_respects_tab_size_eight() {
    let plan = EditingService::indent_lines("a\n", range(0, 2), "        ");
    assert_eq!(apply("a\n", plan), "        a\n");
}

#[test]
fn indent_lines_uses_tab_when_unit_is_tab() {
    let plan = EditingService::indent_lines("a\nb\n", range(0, 4), "\t");
    assert_eq!(apply("a\nb\n", plan), "\ta\n\tb\n");
}

#[test]
fn indent_lines_skips_empty_lines_without_trailing_space() {
    let text = "a\n\nb\n";
    let plan = EditingService::indent_lines(text, range(0, 5), "  ");
    assert_eq!(apply(text, plan), "  a\n\n  b\n", "空行不得产生尾随空白");
}

#[test]
fn outdent_lines_removes_one_indent() {
    let plan = EditingService::outdent_lines("    a\n    b\n", range(0, 12), "  ");
    assert_eq!(apply("    a\n    b\n", plan), "  a\n  b\n");
}

#[test]
fn outdent_lines_removes_tab() {
    let plan = EditingService::outdent_lines("\ta\n", range(0, 3), "    ");
    assert_eq!(apply("\ta\n", plan), "a\n");
}

#[test]
fn outdent_lines_on_unindented_line_is_safe() {
    let plan = EditingService::outdent_lines("a\n", range(0, 2), "    ");
    assert_eq!(apply("a\n", plan), "a\n");
}

#[test]
fn line_transforms_preserve_crlf() {
    let text = "a\r\nb\r\n";
    let plan = EditingService::indent_lines(text, range(0, 6), "  ");
    assert_eq!(
        apply(text, plan),
        "  a\r\n  b\r\n",
        "CRLF 必须被保留，行内容不含 \\r"
    );
}

#[test]
fn expand_to_full_lines_covers_partial_range() {
    let expanded = EditingService::expand_to_full_lines("aaa\nbbb\nccc", range(5, 6));
    assert_eq!(expanded, range(4, 7));
}

#[test]
fn line_range_excludes_newline() {
    assert_eq!(EditingService::line_range("abc\ndef", 5), range(4, 7));
    assert_eq!(EditingService::line_range("abc", 1), range(0, 3));
}

// ---------- 列表延续 ----------

#[test]
fn continue_list_unordered() {
    let plan = EditingService::continue_list_on_newline("- item", ByteOffset::new(6))
        .expect("列表行应产生延续编辑");
    assert_eq!(apply("- item", plan), "- item\n- ");
}

#[test]
fn continue_list_ordered_increments() {
    let plan = EditingService::continue_list_on_newline("3. x", ByteOffset::new(4))
        .expect("有序列表应产生延续编辑");
    assert_eq!(apply("3. x", plan), "3. x\n4. ");
}

#[test]
fn continue_list_task_resets_to_unchecked() {
    let plan = EditingService::continue_list_on_newline("- [x] done", ByteOffset::new(10))
        .expect("任务列表应产生延续编辑");
    assert_eq!(apply("- [x] done", plan), "- [x] done\n- [ ] ");
}

#[test]
fn continue_list_empty_marker_exits_list() {
    let plan = EditingService::continue_list_on_newline("- ", ByteOffset::new(2))
        .expect("空列表项应产生退出编辑");
    assert_eq!(apply("- ", plan), "", "标记后无内容时回车应退出列表");
}

#[test]
fn continue_list_plain_line_returns_none() {
    assert!(EditingService::continue_list_on_newline("hello", ByteOffset::new(5)).is_none());
}

// ---------- 整行移动 ----------

#[test]
fn move_line_up_swaps_with_previous() {
    let plan = EditingService::move_line_up("a\nb\nc", range(3, 4)).expect("应可上移");
    assert_eq!(apply("a\nb\nc", plan), "b\na\nc");
}

#[test]
fn move_line_up_on_first_line_returns_none() {
    assert!(EditingService::move_line_up("a\nb", range(0, 1)).is_none());
}

#[test]
fn move_line_down_swaps_with_next() {
    let plan = EditingService::move_line_down("a\nb\nc", range(0, 1)).expect("应可下移");
    assert_eq!(apply("a\nb\nc", plan), "b\na\nc");
}

#[test]
fn move_line_down_on_last_line_returns_none() {
    assert!(EditingService::move_line_down("a\nb", range(2, 3)).is_none());
}

// ---------- 复制 / 删除行 ----------

#[test]
fn duplicate_line_inserts_copy_below() {
    let plan = EditingService::duplicate_line("a\nb", ByteOffset::new(0));
    assert_eq!(apply("a\nb", plan), "a\na\nb");
}

#[test]
fn delete_line_removes_line_with_newline() {
    let plan = EditingService::delete_line("a\nb\nc", ByteOffset::new(0));
    assert_eq!(apply("a\nb\nc", plan), "b\nc");
}

#[test]
fn delete_line_on_last_line_merges_with_previous() {
    let plan = EditingService::delete_line("a\nb\nc", ByteOffset::new(4));
    assert_eq!(apply("a\nb\nc", plan), "a\nb");
}

// ---------- 任务列表 ----------

#[test]
fn toggle_task_checks_and_unchecks() {
    let plan = EditingService::toggle_task("- [ ] a", ByteOffset::new(0));
    assert_eq!(apply("- [ ] a", plan), "- [x] a");

    let plan = EditingService::toggle_task("- [x] a", ByteOffset::new(0));
    assert_eq!(apply("- [x] a", plan), "- [ ] a");
}

#[test]
fn toggle_task_from_plain_list_item() {
    let plan = EditingService::toggle_task("- a", ByteOffset::new(0));
    assert_eq!(apply("- a", plan), "- [ ] a");
}

#[test]
fn toggle_task_from_plain_line() {
    let plan = EditingService::toggle_task("todo", ByteOffset::new(0));
    assert_eq!(apply("todo", plan), "- [ ] todo");
}

// ---------- 插入片段 ----------

#[test]
fn insert_link_selects_url_placeholder() {
    let plan = EditingService::insert_link("text", range(0, 4));
    let selection = plan.selection();
    assert_eq!(apply("text", plan), "[text](url)");
    assert_eq!(selection, Some(range(7, 10)), "选区应落在 url 占位符上");
}

#[test]
fn insert_link_with_empty_selection_uses_placeholder_label() {
    let plan = EditingService::insert_link("", range(0, 0));
    assert_eq!(apply("", plan), "[链接文本](url)");
}

#[test]
fn insert_code_block_selects_body() {
    let plan = EditingService::insert_code_block("code", range(0, 4));
    let selection = plan.selection();
    assert_eq!(apply("code", plan), "```\ncode\n```");
    assert_eq!(selection, Some(range(4, 8)));
}

#[test]
fn insert_table_builds_gfm_skeleton() {
    let plan = EditingService::insert_table(ByteOffset::new(0), 2, 2);
    assert_eq!(
        apply("", plan),
        "| 列 1 | 列 2 |\n| --- | --- |\n|  |  |\n|  |  |"
    );
}

// ===========================================================================
// 标题升降级
//
// 这一组全部是**回归钉子**：`retarget_heading` 曾用「剥旧前缀 → 加新前缀」
// 两步实现，步骤间靠固定步长平移选区，导致降级改错行；且「剥」用的是
// 切换语义，把选区里的正文行也加了前缀。两处都**不报错**，只是结果错了。
// ===========================================================================

/// 每一行的行首字节偏移。
fn line_starts(text: &str) -> Vec<usize> {
    let mut v = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

/// 第 `line` 行的行首单点光标（大纲点击跳转后的典型状态）。
fn cursor_at_line(text: &str, line: usize) -> TextRange {
    let s = line_starts(text)[line];
    TextRange::new(ByteOffset::new(s), ByteOffset::new(s))
}

/// 返回 `text` 里实际被改动的行号（升序）。
fn changed_lines(before: &str, after: &str) -> Vec<usize> {
    before
        .split('\n')
        .zip(after.split('\n'))
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn demoting_a_heading_changes_only_the_cursor_line() {
    // 曾经的缺陷：光标恰好落在标题行首时，「起点在被改区间内就左移」会左移，
    // 于是第二步把 `# ` 加到了**上一行**（`needle 文档一` 变成标题）。
    let text = "# 指南\n\nneedle 文档一\n\n## 第二节\n\nneedle 文档二\n";
    let after = apply(
        text,
        EditingService::demote_heading(text, cursor_at_line(text, 4)),
    );
    assert_eq!(
        changed_lines(text, &after),
        vec![4],
        "只应改光标所在那一行，实际改了：{:?}\n{after}",
        changed_lines(text, &after)
    );
    assert!(
        after.contains("# 第二节"),
        "`## 第二节` 降一级应是 `# 第二节`，实际：\n{after}"
    );
}

#[test]
fn demoting_replaces_the_prefix_instead_of_stacking_it() {
    let text = "### 标题";
    let after = apply(text, EditingService::demote_heading(text, range(0, 0)));
    assert_eq!(
        after, "## 标题",
        "降级是替换而非叠加（叠加后再点一次也还原不回来）"
    );
}

#[test]
fn demoting_a_multi_line_selection_keeps_body_lines_untouched() {
    // 曾经的缺陷：「剥 `## `」用的是切换语义，对没有该前缀的正文行会**加上**它，
    // 两步之后正文变成 `# ## 正文`。`demote_heading` 的文档明确承诺
    // 「已经是正文的行保持原样」。
    let text = "## 标题\n\n正文段落\n";
    // 选中全部三行（从行首到文末）。
    let after = apply(
        text,
        EditingService::demote_heading(text, range(0, text.len())),
    );
    assert_eq!(
        after, "# 标题\n\n正文段落\n",
        "只有标题行该降级；正文段落必须原样保留"
    );
}

#[test]
fn demoting_leaves_empty_lines_alone() {
    // 空行加 `# ` 会得到一个视觉上的空标题，且再次降级时它会被当成真标题。
    let text = "## 标题\n\n## 下一节\n";
    let after = apply(text, EditingService::demote_heading(text, range(0, 5)));
    assert_eq!(after, "# 标题\n\n## 下一节\n", "空行不该被加上 `# `");
}

#[test]
fn demoting_leaves_whitespace_only_lines_alone() {
    // 只含空格的行在渲染上同样是空行（编辑器常留下尾随空格）。
    // 若只判 `is_empty()`，它会被加上 `# `，得到一个「有内容」的假标题
    // ——比空标题更难发现，因为那一行看起来确实有东西。
    //
    // 场景选「光标停在这行上升级」：这是唯一能让 `is_empty()` 与
    // `trim().is_empty()` 得出不同结果的路径（光标行不受
    // 「非光标行需本来就是标题」那条保护）。
    let text = "## 标题\n   \n## 下一节\n";
    let blank = line_starts(text)[1] + 1;
    let after = apply(
        text,
        EditingService::promote_heading(
            text,
            TextRange::new(ByteOffset::new(blank), ByteOffset::new(blank)),
        ),
    );
    assert_eq!(
        after, "## 标题\n   \n## 下一节\n",
        "光标停在纯空白行上升级，不该给这行加 `# `（实际：{after:?}）"
    );
}

#[test]
fn an_empty_line_under_the_cursor_is_not_promoted_to_a_heading() {
    // 同一类问题的空行版本，也是用户更容易撞上的：
    // 光标停在两个标题之间的空行上，误按 Ctrl+= 就多出一个空标题。
    let text = "## 甲\n\n## 乙\n";
    let blank = line_starts(text)[1];
    let after = apply(
        text,
        EditingService::promote_heading(
            text,
            TextRange::new(ByteOffset::new(blank), ByteOffset::new(blank)),
        ),
    );
    assert_eq!(after, text, "空行不该被提升成标题（实际：{after:?}）");
}

#[test]
fn promoting_a_plain_line_turns_it_into_a_heading() {
    // 光标所在行总是要改——用户把光标放在正文上点「提升」
    // 就是想把它变成标题，这条不能被上一条「正文保持原样」误伤。
    let text = "正文段落\n";
    let after = apply(
        text,
        EditingService::promote_heading(text, cursor_at_line(text, 0)),
    );
    assert_eq!(after, "# 正文段落\n");
}

#[test]
fn promoting_cycles_h1_back_to_h6() {
    let text = "# 一级";
    let after = apply(text, EditingService::promote_heading(text, range(0, 0)));
    assert_eq!(after, "###### 一级", "h1 再提升应包回 h6（形成循环）");
}

#[test]
fn demoting_an_h1_turns_it_into_body_text() {
    let text = "# 标题\n\n正文\n";
    let after = apply(
        text,
        EditingService::demote_heading(text, cursor_at_line(text, 0)),
    );
    assert_eq!(after, "标题\n\n正文\n", "h1 降级应变成正文（而不是引用）");
}

#[test]
fn demoting_twice_returns_to_the_original_text() {
    // 幂等性反查：升降级若不是「替换」，两次降级回不到原文。
    let text = "# 指南\n\nneedle 文档一\n\n## 第二节\n\nneedle 文档二\n";
    let once = apply(
        text,
        EditingService::demote_heading(text, cursor_at_line(text, 4)),
    );
    let twice = apply(
        &once,
        EditingService::demote_heading(&once, cursor_at_line(&once, 4)),
    );
    assert_eq!(
        changed_lines(&once, &twice),
        vec![4],
        "第二次降级仍只应改那一行"
    );
    assert!(
        twice.contains("\n第二节\n"),
        "两次降级后应回到正文，实际：\n{twice}"
    );
}

#[test]
fn heading_levels_ignore_a_hash_without_a_space() {
    // `###tag` 不是标题（CommonMark 要求 `#` 后有空格），
    // 故它既不该被当成 h3，也不该被剥前缀。
    let text = "###tag 不是标题";
    let level = EditingService::heading_level_at(text, 0);
    assert_eq!(level, 0, "`###tag` 不该被识别成标题");
    let after = apply(
        text,
        EditingService::promote_heading(text, cursor_at_line(text, 0)),
    );
    assert_eq!(
        after, "# ###tag 不是标题",
        "光标在它上面点提升应加一层新前缀"
    );
}

#[test]
fn heading_level_is_capped_at_six() {
    // 7 个 `#` 仍按 h6 处理，否则 `min(6)` 的钳制一旦被去掉，
    // 就会冒出 Markdown 不支持的 h7。
    let text = "####### 七个井号";
    assert_eq!(EditingService::heading_level_at(text, 0), 6);
}

#[test]
fn heading_level_change_preserves_crlf_line_endings() {
    // CRLF 文档在 Windows 上是常态。行尾的 `\r` 若被当成内容，
    // 前缀剥离就会留下孤零零的 `\r`，整个文件格式被破坏。
    let text = "## 标题\r\n\r\n正文\r\n";
    let after = apply(
        text,
        EditingService::demote_heading(text, cursor_at_line(text, 0)),
    );
    assert_eq!(after, "# 标题\r\n\r\n正文\r\n", "`\\r` 必须原样保留");
}

// ---------- 字符边界 ----------

#[test]
fn char_floor_and_ceil_align_cjk() {
    let text = "中文";
    assert_eq!(EditingService::char_floor(text, 1), 0);
    assert_eq!(EditingService::char_ceil(text, 1), 3);
    assert_eq!(EditingService::align_range(text, range(1, 4)), range(0, 6));
}

#[test]
fn apply_edit_rejects_non_char_boundary() {
    let result = Edit::insertion(ByteOffset::new(1), "!").apply("中文");
    assert!(result.is_err(), "非字符边界必须返回错误而不是 panic");
}
