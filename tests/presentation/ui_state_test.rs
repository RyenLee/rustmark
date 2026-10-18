//! UI State 分层与查找替换用例（S6-12、S6-08）。
//!
//! S6-08 验收要求「功能用例 ≥ 10 条」，覆盖：
//! 普通 / 正则 / 大小写 / 全字四种模式命中数正确、全部替换后计数与
//! 文档实际替换数一致、无效正则给出错误提示而非崩溃、
//! 跨文档切换后查找状态的重置规则。
//!
//! 另含S6-12 的分层断言：UI State 不得持有业务类型。

use rustmark::presentation::state::app_ui_state::{AppUiState, SPLIT_MAX, SPLIT_MIN, SidebarTab};
use rustmark::presentation::state::dialog_ui_state::{DialogUiState, Overlay};
use rustmark::presentation::state::editor_ui_state::EditorUiState;
use rustmark::presentation::state::find_replace_ui_state::{
    FindOptions, FindReplaceUiState, Finder, OptionKind, PatternError,
};
use rustmark::presentation::state::preview_ui_state::{
    PREVIEW_DEBOUNCE_MS, PreviewCacheKey, PreviewUiState,
};
use rustmark::presentation::state::sidebar_ui_state::{SearchOptions, SidebarUiState};
use rustmark::presentation::theme::builtin::Theme;
use rustmark::shared::ids::DocumentId;

/// 便捷构造：默认选项（不区分大小写、不正则、不全字）。
const PLAIN: FindOptions = FindOptions {
    case_sensitive: false,
    regex: false,
    whole_word: false,
};

// ============================================================================
// S6-08 用例 1：普通查找（默认不区分大小写）
// ============================================================================

#[test]
fn plain_find_is_case_insensitive_by_default() {
    let text = "Hello hello HELLO";
    let finder = Finder::scan(text, "hello", PLAIN).expect("应可扫描");
    assert_eq!(finder.count(), 3, "默认应不区分大小写");

    let sensitive = FindOptions {
        case_sensitive: true,
        ..PLAIN
    };
    let finder = Finder::scan(text, "hello", sensitive).expect("应可扫描");
    assert_eq!(finder.count(), 1, "开启区分大小写后只应命中 1 处");

    // 反过来用大写模式查小写文本。
    let finder = Finder::scan(text, "HELLO", sensitive).expect("应可扫描");
    assert_eq!(finder.count(), 1);
}

// ============================================================================
// S6-08 用例 2：命中位置精确
// ============================================================================

#[test]
fn match_ranges_are_exact_byte_offsets() {
    let text = "abc hello def hello";
    let finder = Finder::scan(text, "hello", PLAIN).expect("应可扫描");
    let matches = finder.matches();
    assert_eq!(matches.len(), 2);
    assert_eq!(&text[matches[0].start..matches[0].end], "hello");
    assert_eq!(&text[matches[1].start..matches[1].end], "hello");
    assert_eq!(matches[0].start, 4);
    assert_eq!(matches[1].start, 14);
}

// ============================================================================
// S6-08 用例 3：中文查找（多字节偏移）
// ============================================================================

#[test]
fn chinese_find_uses_byte_offsets_not_char_offsets() {
    let text = "中文测试中文";
    let finder = Finder::scan(text, "中文", PLAIN).expect("应可扫描");
    assert_eq!(finder.count(), 2);

    let matches = finder.matches();
    // 「中文」两个字符 = 6 字节。
    assert_eq!(matches[0].start, 0);
    assert_eq!(matches[0].end, 6);
    assert_eq!(matches[1].start, 12, "第二处起点应为 12 字节");
    assert_eq!(matches[1].end, 18);

    // 切出来的必须正好是「中文」——若按字符索引切，会切出乱码。
    assert_eq!(&text[matches[1].start..matches[1].end], "中文");
}

#[test]
fn mixed_chinese_and_ascii_find_works() {
    let text = "第1章 introduction 第2章 method";
    let finder = Finder::scan(text, "第2章", PLAIN).expect("应可扫描");
    assert_eq!(finder.count(), 1);
    let m = finder.matches()[0];
    assert_eq!(&text[m.start..m.end], "第2章");

    let finder = Finder::scan(text, "method", PLAIN).expect("应可扫描");
    assert_eq!(finder.count(), 1);
}

// ============================================================================
// S6-08 用例 4：全字匹配
// ============================================================================

#[test]
fn whole_word_match_respects_boundaries() {
    let text = "cat cats concat cat.";
    let whole = FindOptions {
        whole_word: true,
        ..PLAIN
    };

    let finder = Finder::scan(text, "cat", whole).expect("应可扫描");
    assert_eq!(finder.count(), 2, "只应命中独立的 cat：{finder:?}");
    for m in finder.matches() {
        assert_eq!(&text[m.start..m.end], "cat");
    }

    // 不开全字则命中全部子串。
    let finder = Finder::scan(text, "cat", PLAIN).expect("应可扫描");
    assert_eq!(finder.count(), 4, "子串应命中 cat/cats/concat/cat.");
}

#[test]
fn whole_word_uses_underscore_as_word_char() {
    let text = "foo_bar foo bar";
    let whole = FindOptions {
        whole_word: true,
        ..PLAIN
    };
    let finder = Finder::scan(text, "foo", whole).expect("应可扫描");
    // `foo_bar` 中的 foo 右侧是 `_`（词字符），故不算全字；
    // `bar` 前面的 foo 左右都是空格，是唯一满足全词的命中。
    assert_eq!(finder.count(), 1, "下划线应视为词字符");
}

// ============================================================================
// S6-08 用例 5：正则模式
// ============================================================================

#[test]
fn regex_supports_basic_operators() {
    let cases: &[(&str, &str, usize)] = &[
        // `.` 任意字符。
        ("a.c", "abc a c adc", 3),
        // `*` 零次或多次。
        ("ab*c", "ac abc abbc", 3),
        // `+` 一次或多次。
        ("ab+c", "ac abc abbc", 2),
        // `?` 零次或一次。
        ("ab?c", "ac abc", 2),
        // 字符类。
        ("[0-9]+", "a1 b22 c333", 3),
        // 取反字符类。
        ("[^0-9]", "a1b", 2),
        // 或。
        ("cat|dog", "cat dog bird", 2),
        // 分组。
        ("(ab)+", "ab abab x", 2),
        // 行首锚点。
        ("^abc", "abc\nxabc", 1),
    ];

    for (pattern, text, expected) in cases {
        let regex = FindOptions {
            regex: true,
            ..PLAIN
        };
        let finder = Finder::scan(text, pattern, regex)
            .unwrap_or_else(|e| panic!("正则 {pattern:?} 应可编译：{e}"));
        assert_eq!(
            finder.count(),
            *expected,
            "正则 {pattern:?} 在 {text:?} 上的命中数不符"
        );
    }
}

#[test]
fn regex_supports_escaped_special_characters() {
    let regex = FindOptions {
        regex: true,
        ..PLAIN
    };
    // 转义的点应按字面量匹配。
    let finder = Finder::scan("a.c abc", r"a\.c", regex).expect("应可编译");
    assert_eq!(finder.count(), 1, "转义的点只应匹配字面量点号");
}

#[test]
fn regex_can_combine_with_other_options() {
    let text = "Foo foo FOO";
    let options = FindOptions {
        regex: true,
        case_sensitive: true,
        ..PLAIN
    };
    let finder = Finder::scan(text, "F..", options).expect("应可编译");
    // `F..` 区分大小写：`Foo`、`FOO` 都匹配（小写 `foo` 的 `F` 不匹配）。
    assert_eq!(
        finder.count(),
        2,
        "区分大小写 + 正则应只命中首字母大写的两处"
    );
}

// ============================================================================
// S6-08 用例 6：无效正则给出错误提示而非崩溃
// ============================================================================

#[test]
fn invalid_regex_returns_error_not_panic() {
    let regex = FindOptions {
        regex: true,
        ..PLAIN
    };
    let cases = [
        "[unclosed",
        "(unclosed",
        "*nothing",
        "+nothing",
        "a**b*",
        "\\",
    ];
    for pattern in cases {
        let result = Finder::scan("some text", pattern, regex);
        assert!(
            result.is_err(),
            "无效正则 {pattern:?} 应返回 Err，实际得到 {:?}",
            result.map(|f| f.count())
        );
        // 错误信息必须能直接展示给用户（不得为空）。
        let message = result.unwrap_err().to_string();
        assert!(
            !message.is_empty(),
            "无效正则 {pattern:?} 的错误信息不得为空"
        );
    }
}

#[test]
fn empty_pattern_is_rejected_with_friendly_message() {
    let error = Finder::scan("text", "", PLAIN).unwrap_err();
    assert_eq!(error, PatternError::Empty);
    assert_eq!(error.to_string(), "请输入查找内容");
}

#[test]
fn pattern_error_message_is_human_readable() {
    let regex = FindOptions {
        regex: true,
        ..PLAIN
    };
    let error = Finder::scan("x", "[bad", regex).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("正则"),
        "错误信息应说明是正则问题：{message}"
    );
}

// ============================================================================
// S6-08 用例 7：全部替换后计数与实际替换数一致
// ============================================================================

#[test]
fn replace_all_count_matches_actual_replacements() {
    let text = "one two one two one";
    let finder = Finder::scan(text, "one", PLAIN).expect("应可扫描");
    assert_eq!(finder.count(), 3);

    let summary = finder.preview_replace_all(text, "1");
    assert_eq!(summary.count, 3, "替换次数应等于命中数");
    assert_eq!(summary.count, finder.count());
    assert_eq!(summary.result, "1 two 1 two 1");
    // 替换后不再有「one」。
    assert!(!summary.will_match_again);
}

#[test]
fn replace_all_with_regex_uses_actual_matches() {
    let text = "a1 b22 c333";
    let regex = FindOptions {
        regex: true,
        ..PLAIN
    };
    let finder = Finder::scan(text, "[0-9]+", regex).expect("应可编译");
    assert_eq!(finder.count(), 3);

    let summary = finder.preview_replace_all(text, "#");
    assert_eq!(summary.count, 3);
    assert_eq!(summary.result, "a# b# c#");
}

/// 替换若会**制造新命中**，必须被标记出来。
///
/// 用「`a`」去替换「`aa`」是收敛的（`aa` → `a`，下一轮匹配不到 `aa`）；
/// 反过来用「`aa`」替换「`a`」才会无限增生（`a` → `aa` → `aaaa` → …）。
/// 两种方向都测，否则很容易只测了对的那半、放过错的。
#[test]
fn replace_all_flags_only_when_it_would_grow_forever() {
    // 增生方向：1 个 a → 2 个 → 4 个……
    let grow_text = "a";
    let grow = Finder::scan(grow_text, "a", PLAIN).expect("应可扫描");
    let grow_summary = grow.preview_replace_all(grow_text, "aa");
    assert_eq!(grow_summary.count, 1);
    assert_eq!(grow_summary.result, "aa");
    assert!(
        grow_summary.will_match_again,
        "用 aa 替换 a 后仍能匹配 a，必须警告用户（否则反复点会无限增生）"
    );

    // 收敛方向：aa → a 之后再也匹配不到 aa。
    let shrink_text = "aa";
    let shrink = Finder::scan(shrink_text, "aa", PLAIN).expect("应可扫描");
    let shrink_summary = shrink.preview_replace_all(shrink_text, "a");
    assert_eq!(shrink_summary.count, 1);
    assert_eq!(shrink_summary.result, "a");
    assert!(
        !shrink_summary.will_match_again,
        "用 a 替换 aa 是收敛的，不该报警；误报会让用户以为替换不干净"
    );
}

#[test]
fn replace_all_with_empty_replacement_deletes() {
    let text = "a-b-c";
    let finder = Finder::scan(text, "-", PLAIN).expect("应可扫描");
    let summary = finder.preview_replace_all(text, "");
    assert_eq!(summary.count, 2);
    assert_eq!(summary.result, "abc");
}

// ============================================================================
// S6-08 用例 8：循环查找（上一个 / 下一个）
// ============================================================================

#[test]
fn next_and_prev_wrap_around() {
    let text = "aXaXa";
    let finder = Finder::scan(text, "a", PLAIN).expect("应可扫描");
    let all = finder.matches().to_vec();
    assert_eq!(all.len(), 3);

    // 正向循环。
    assert_eq!(finder.next_after(0), Some(all[0]));
    assert_eq!(finder.next_after(all[0].end), Some(all[1]));
    assert_eq!(finder.next_after(all[1].end), Some(all[2]));
    // 末尾之后回到开头。
    assert_eq!(finder.next_after(all[2].end), Some(all[0]));

    // 反向循环。
    assert_eq!(finder.prev_before(5), Some(all[2]));
    assert_eq!(finder.prev_before(all[2].start), Some(all[1]));
    assert_eq!(finder.prev_before(all[1].start), Some(all[0]));
    // 开头之前回到末尾。
    assert_eq!(finder.prev_before(0), Some(all[2]));
}

#[test]
fn navigation_on_empty_result_returns_none() {
    let finder = Finder::scan("text", "zzz", PLAIN).expect("应可扫描");
    assert_eq!(finder.count(), 0);
    assert_eq!(finder.next_after(0), None);
    assert_eq!(finder.prev_before(0), None);
}

// ============================================================================
// S6-08 用例 9：跨文档切换的状态处理（规则已定义）
// ============================================================================

#[test]
fn switching_document_keeps_query_but_resets_selection() {
    let mut state = FindReplaceUiState::new();
    state.find_text = "查找词".to_string();
    state.replace_text = "替换词".to_string();
    state.toggle(OptionKind::CaseSensitive);
    state.current = Some(3);

    let first = DocumentId::new();
    state.on_document_switched(first);
    state.current = Some(3);

    let second = DocumentId::new();
    state.on_document_switched(second);

    assert_eq!(state.find_text, "查找词", "跨文档应保留查询");
    assert_eq!(state.replace_text, "替换词", "跨文档应保留替换词");
    assert!(state.options.case_sensitive, "跨文档应保留选项");
    assert_eq!(state.current, None, "命中选中必须重置（属于旧文档）");
    assert_eq!(state.error, None, "旧文档的错误提示不应带到新文档");
}

#[test]
fn switching_document_can_clear_everything_when_configured() {
    let mut state = FindReplaceUiState::new();
    state.keep_query_across_documents = false;
    state.find_text = "查找词".to_string();
    state.toggle(OptionKind::Regex);

    state.on_document_switched(DocumentId::new());
    state.on_document_switched(DocumentId::new());

    assert!(state.find_text.is_empty(), "配置为不保留时应清空");
    assert!(!state.options.regex, "配置为不保留时应重置选项");
}

#[test]
fn same_document_switch_is_a_no_op() {
    let mut state = FindReplaceUiState::new();
    let doc = DocumentId::new();
    state.find_text = "keep".to_string();
    state.current = Some(2);

    state.on_document_switched(doc);
    state.on_document_switched(doc);

    assert_eq!(state.current, Some(2), "同一文档不应重置命中选中");
    assert_eq!(state.find_text, "keep");
}

// ============================================================================
// S6-08 用例 10：面板状态与选项联动
// ============================================================================

#[test]
fn option_toggle_resets_current_match() {
    let mut state = FindReplaceUiState::new();
    state.current = Some(5);

    state.toggle(OptionKind::CaseSensitive);
    assert_eq!(
        state.current, None,
        "选项变更后旧的命中索引已失效，必须重置"
    );

    // 再点回来。
    state.toggle(OptionKind::CaseSensitive);
    assert_eq!(state.current, None, "无实际变化时也不应留下脏索引");
}

#[test]
fn record_scan_updates_error_and_current() {
    let text = "aXaXa";
    let mut state = FindReplaceUiState::new();

    // 无效正则 → 记录错误提示，清空命中选中。
    state.find_text = "[bad".to_string();
    state.options = FindOptions {
        regex: true,
        ..PLAIN
    };
    state.rescan(text);
    let error = state.error.as_deref().unwrap_or_default();
    assert!(error.contains("正则"), "无效正则必须给出可见提示：{error}");
    assert_eq!(state.current, None);
    assert!(
        state.finder.is_none(),
        "模式无效时不得保留旧 finder——否则「下一个」会跳到与输入框无关的位置"
    );

    // 有效扫描 → 清除错误，选中首个命中。
    state.find_text = "a".to_string();
    state.options = PLAIN;
    state.rescan(text);
    assert_eq!(state.error, None, "有效查找应清除上次的错误");
    assert_eq!(state.current, Some(0), "首次扫描应选中首个命中");
    assert_eq!(state.match_count(), 3);

    // 无命中 → 不选中。
    state.find_text = "zzz".to_string();
    state.rescan(text);
    assert_eq!(state.current, None, "无命中时不应有选中项");
    assert_eq!(state.count_label(), "无结果");

    // 命中数变少后旧索引越界 → 重置。
    state.find_text = "a".to_string();
    state.rescan(text);
    state.current = Some(2);
    state.find_text = "aX".to_string();
    state.rescan(text);
    assert!(
        state
            .current
            .is_none_or(|index| index < state.match_count()),
        "命中数减少后旧索引不得越界：{:?} vs {}",
        state.current,
        state.match_count()
    );

    // 清空查询 → 计数与命中一并清空（否则会显示上一次的 `n/m`）。
    state.find_text.clear();
    state.rescan(text);
    assert!(state.finder.is_none());
    assert_eq!(state.count_label(), "", "查询为空时不该显示「无结果」");
}

#[test]
fn find_options_report_active_count() {
    assert_eq!(PLAIN.active_count(), 0);
    assert!(PLAIN.is_plain());

    let all = FindOptions {
        case_sensitive: true,
        regex: true,
        whole_word: true,
    };
    assert_eq!(all.active_count(), 3);
    assert!(!all.is_plain());
}

// ============================================================================
// S6-12：UI State 分层断言
// ============================================================================

/// 布局开关互不干扰（改侧边栏不应影响预览区）。
#[test]
fn layout_toggles_are_independent() {
    let mut state = AppUiState::new(Theme::dark());
    // 默认收起（问题 061）：开关语义与初始态无关，先展开到「可见」基线。
    state.sidebar_visible = true;
    assert!(state.sidebar_visible);

    state.toggle_sidebar();
    assert!(!state.sidebar_visible);
    assert!(state.preview_visible, "侧边栏开关不应影响预览区");

    state.toggle_preview();
    assert!(!state.preview_visible);
    assert!(!state.sidebar_visible, "预览开关不应影响侧边栏");

    state.toggle_sidebar();
    assert!(state.sidebar_visible);
    assert!(!state.preview_visible);
}

/// 再次点击同一侧边栏标签应收起（对齐原型 `toggleSidebar`）。
#[test]
fn clicking_same_sidebar_tab_collapses_it() {
    let mut state = AppUiState::new(Theme::dark());
    // 默认收起（问题 061）：本用例钉的是「可见时再点同一标签 → 收起」，
    // 先展开到可见基线（顶栏按钮 / Ctrl+\ 的路径）。
    state.sidebar_visible = true;
    assert!(state.sidebar_visible);
    assert_eq!(state.sidebar_tab, SidebarTab::Files);

    // 已在文件标签上再点一次 → 收起。
    state.toggle_sidebar_tab(SidebarTab::Files);
    assert!(!state.sidebar_visible);

    // 收起状态下再点 → 展开并选中。
    state.toggle_sidebar_tab(SidebarTab::Files);
    assert!(state.sidebar_visible);
    assert_eq!(state.sidebar_tab, SidebarTab::Files);

    // 切到别的标签 → 展开并切换。
    state.toggle_sidebar_tab(SidebarTab::Outline);
    assert!(state.sidebar_visible);
    assert_eq!(state.sidebar_tab, SidebarTab::Outline);
}

/// 分割比例必须夹紧，否则分割条能被拖没。
#[test]
fn split_ratio_is_clamped() {
    let mut state = AppUiState::new(Theme::dark());

    state.set_split_ratio(0.5);
    assert!((state.editor_ratio() - 0.5).abs() < 1e-6);
    assert!((state.preview_ratio() - 0.5).abs() < 1e-6);

    state.set_split_ratio(0.0);
    assert_eq!(state.split_ratio, SPLIT_MIN, "不能拖到 0（编辑区会消失）");

    state.set_split_ratio(1.0);
    assert_eq!(state.split_ratio, SPLIT_MAX, "不能拖到 1（预览区会消失）");

    state.set_split_ratio(-5.0);
    assert_eq!(state.split_ratio, SPLIT_MIN);

    state.set_split_ratio(0.35);
    assert!((state.split_ratio - 0.35).abs() < 1e-6, "合法值不应被改动");

    // 编辑区与预览区比例之和恒为 1。
    for ratio in [0.2_f32, 0.35, 0.5, 0.75, 0.8] {
        state.set_split_ratio(ratio);
        assert!((state.editor_ratio() + state.preview_ratio() - 1.0).abs() < 1e-6);
    }
}

/// 专注模式：进入与退出。
#[test]
fn focus_mode_toggles_and_exits() {
    let mut state = AppUiState::new(Theme::dark());
    assert!(!state.focus_mode);

    state.toggle_focus_mode();
    assert!(state.focus_mode);

    // 专注模式下外围 UI 隐藏，但编辑区与预览区的开关状态保留。
    state.exit_focus_mode();
    assert!(!state.focus_mode);
    assert!(state.preview_visible, "退出专注模式后预览区状态应保持");
}

/// 标签页循环。
#[test]
fn tab_cycling_wraps() {
    let mut state = AppUiState::new(Theme::dark());
    state.tab_count = 3;

    assert_eq!(state.active_tab, 0);
    assert!(state.cycle_tab(1));
    assert_eq!(state.active_tab, 1);
    assert!(state.cycle_tab(1));
    assert_eq!(state.active_tab, 2);
    assert!(state.cycle_tab(1), "末尾之后应回到开头");
    assert_eq!(state.active_tab, 0);

    assert!(state.cycle_tab(-1), "开头之前应回到末尾");
    assert_eq!(state.active_tab, 2);
}

#[test]
fn cycling_with_single_or_zero_tabs_reports_no_change() {
    let mut state = AppUiState::new(Theme::dark());

    // 零个标签。
    assert!(!state.cycle_tab(1));
    assert_eq!(state.active_tab, 0);

    // 一个标签：切了也还是自己，应报告「没切」。
    state.tab_count = 1;
    assert!(
        !state.cycle_tab(1),
        "只有一个标签时切了等于没切，应返回 false"
    );
    assert_eq!(state.active_tab, 0);
}

#[test]
fn goto_tab_is_one_based_and_bounds_checked() {
    let mut state = AppUiState::new(Theme::dark());
    state.tab_count = 3;

    assert!(state.goto_tab(1));
    assert_eq!(state.active_tab, 0);
    assert!(state.goto_tab(3));
    assert_eq!(state.active_tab, 2);

    assert!(!state.goto_tab(0), "0 不是合法序号");
    assert!(!state.goto_tab(4), "超出标签数应被拒绝");
    assert_eq!(state.active_tab, 2, "失败的跳转不应改变当前标签");
}

// ============================================================================
// S6-12：编辑区状态
// ============================================================================

#[test]
fn selection_is_normalized_regardless_of_drag_direction() {
    let mut state = EditorUiState::new();

    // 从后往前拖选。
    state.set_selection(20, 5);
    assert_eq!(state.selection(), Some((5, 20)));
    assert_eq!(state.selected_len(), 15);
    assert!(state.has_selection());

    // 从前往后拖选。
    state.set_selection(5, 20);
    assert_eq!(state.selection(), Some((5, 20)));
    assert_eq!(state.selected_len(), 15);
}

#[test]
fn empty_selection_is_detected() {
    let mut state = EditorUiState::new();
    assert_eq!(state.selection(), None);
    assert!(!state.has_selection());

    // 光标与锚点重合 = 空选区。
    state.set_selection(7, 7);
    assert_eq!(state.selection(), Some((7, 7)));
    assert!(!state.has_selection());
    assert_eq!(state.selected_len(), 0);
}

#[test]
fn moving_cursor_clears_selection_but_shift_extends_it() {
    let mut state = EditorUiState::new();
    state.set_selection(0, 10);

    state.move_cursor(3);
    assert_eq!(state.selection(), None, "普通移动光标应清除选区");

    state.set_selection(0, 10);
    state.extend_to(15);
    assert_eq!(
        state.selection(),
        Some((0, 15)),
        "Shift+移动应保留锚点并延长选区"
    );
}

#[test]
fn cursor_is_clamped_after_content_shrink() {
    let mut state = EditorUiState::new();
    state.set_selection(50, 80);

    // 文档被大幅缩短。
    state.clamp_to(10);

    assert!(state.cursor <= 10, "光标必须夹紧到新长度内");
    assert!(
        state.selection().is_some_and(|(s, e)| s <= 10 && e <= 10),
        "选区两端都必须夹紧，否则切片会 panic（panic=abort 下直接终止进程）"
    );
}

/// S6-03 的最高风险项：IME 组字期间不得产生撤销快照。
#[test]
fn ime_composing_does_not_emit_text_until_commit() {
    let mut state = EditorUiState::new();

    // 开始组字（拼音未上屏）。
    state.begin_composing("n");
    assert!(state.is_composing());
    assert!(
        state.should_skip_snapshot(),
        "组字期间必须跳过撤销快照，否则一次输入产生多个撤销步骤"
    );

    // 候选变化。
    state.update_composing("ni");
    state.update_composing("nihao");
    assert_eq!(state.composing.as_deref(), Some("nihao"));
    assert!(state.should_skip_snapshot());

    // 上屏：返回最终文本。
    let committed = state.end_composing();
    assert_eq!(committed.as_deref(), Some("nihao"));
    assert!(!state.is_composing());
    assert!(!state.should_skip_snapshot(), "上屏后应恢复正常快照");
}

#[test]
fn cancelled_composing_emits_nothing() {
    let mut state = EditorUiState::new();
    state.begin_composing("ni");
    state.update_composing("ni hao");
    // Esc 取消：组字文本被整体丢弃，不上屏。
    state.cancel_composing();
    assert!(!state.is_composing(), "取消后应退出组字状态");
    // 再结束一次也不该冒出内容——状态已被清空。
    assert_eq!(
        state.end_composing(),
        None,
        "取消组字不得产生任何文本，否则会留下空撤销步骤"
    );
}

#[test]
fn empty_composing_emits_nothing() {
    let mut state = EditorUiState::new();
    // 有些输入法会先给一个空串再更新。
    state.begin_composing("");
    assert!(state.is_composing());
    assert_eq!(state.end_composing(), None, "空组字不应产生空提交");
}

#[test]
fn switching_document_resets_editor_state() {
    let mut state = EditorUiState::new();
    state.set_selection(10, 20);
    state.scroll_y = 100.0;
    state.begin_composing("ni");

    state.reset_for_new_document();

    assert_eq!(state.cursor, 0);
    assert_eq!(state.selection(), None);
    assert_eq!(state.scroll_y, 0.0, "切文档后应回到顶部");
    assert!(!state.is_composing(), "组字状态不得跨文档残留");
}

// ============================================================================
// S6-12：预览缓存与防抖（FR-01）
// ============================================================================

/// 单文档用例里的文档标识。值与真实 `DocumentId` 无关，
/// 只要同一用例内前后一致即可——缓存键只做「相等 / 不等」比较。
const DOC: u64 = 1;

/// 另一份文档的标识（用于「切 tab」类用例）。
const OTHER_DOC: u64 = 2;

#[test]
fn preview_debounce_coalesces_rapid_edits() {
    let theme = Theme::dark();
    let mut state = PreviewUiState::new();

    let key_v1 = PreviewCacheKey::new(DOC, 1, &theme);
    state.record_render(key_v1, 1000);

    // 快速连续编辑：每次版本都变，但都在防抖窗口内。
    let mut renders = 0usize;
    for (offset, version) in [(1050_u64, 2_u64), (1100, 3), (1140, 4)] {
        let key = PreviewCacheKey::new(DOC, version, &theme);
        if state.should_render(key, offset, false) {
            state.record_render(key, offset);
            renders += 1;
        }
    }
    assert_eq!(renders, 0, "150ms 内的连续编辑应被合并，不应触发重排");

    // 超过防抖窗口后应渲染一次。
    let key = PreviewCacheKey::new(DOC, 5, &theme);
    assert!(
        state.should_render(key, 1000 + PREVIEW_DEBOUNCE_MS, false),
        "超过 {}ms 后应渲染",
        PREVIEW_DEBOUNCE_MS
    );
}

/// 换文档必须**立即**重建，即便两份文档的版本号完全相同。
///
/// 这是 FR「预览联动当前 tab」的核心回归：
/// `revision` 是**每份文档各自**的计数器，两份刚打开、都没编辑过的文档
/// 版本号天然相同（都是初始值）。只比版本号就会判成「内容没变」，
/// 预览于是一直显示**第一份打开的**文档。
#[test]
fn switching_document_rebuilds_even_when_versions_match() {
    let theme = Theme::dark();
    let mut state = PreviewUiState::new();

    // 文档 A（版本 1）已渲染。
    let key_a = PreviewCacheKey::new(DOC, 1, &theme);
    state.record_render(key_a, 1000);
    assert!(!state.should_render(key_a, 1000, false), "同一键不应重排");

    // 切到文档 B，版本号**恰好相同**。
    let key_b = PreviewCacheKey::new(OTHER_DOC, 1, &theme);
    assert!(
        state.should_render(key_b, 1000, false),
        "换文档必须重建：版本号相同不代表内容相同（每份文档各自计数）"
    );
    // 且不能等防抖——否则预览会继续显示上一份文档 150ms。
    assert!(
        state.should_render(key_b, 1001, false),
        "换文档是显式导航，必须立即生效而不进防抖窗口"
    );
    assert!(key_a.document_changed(&key_b));
    assert!(
        !key_a.content_changed(&key_b),
        "用例前提：两份文档版本号相同"
    );

    // 视图在 `should_render` 为真之后会真的渲染并记录（`record_render`），
    // 这里必须照做——`should_render` 只是**询问**，不改变状态。
    state.record_render(key_b, 1001);

    // 切回 A 同样要重建：此时「上次渲染的是 B」，与 A 不是同一份文档。
    assert!(
        state.should_render(key_a, 1002, false),
        "切回原文档也要重建"
    );
}

/// `sync_document` 是「换文档即重置」的唯一入口：块、滚动偏移、缓存键一起清。
#[test]
fn sync_document_resets_blocks_and_scroll_on_switch() {
    let theme = Theme::dark();
    let mut state = PreviewUiState::new();

    assert!(state.sync_document(DOC), "首次同步即视为切换（从无到有）");
    assert_eq!(state.last_doc, Some(DOC));
    state.record_render(PreviewCacheKey::new(DOC, 1, &theme), 1000);
    state.scroll_y = 420.0;

    assert!(!state.sync_document(DOC), "同一份文档不算切换");
    assert_eq!(state.scroll_y, 420.0, "同一份文档不得清滚动位置");

    assert!(state.sync_document(OTHER_DOC), "换成另一份文档即为切换");
    assert_eq!(state.scroll_y, 0.0, "换文档后应回到顶部");
    assert!(state.last_key.is_none(), "换文档后缓存键必须失效");
    assert!(state.last_doc == Some(OTHER_DOC));
}

#[test]
fn unchanged_content_does_not_re_render() {
    let theme = Theme::dark();
    let mut state = PreviewUiState::new();
    let key = PreviewCacheKey::new(DOC, 7, &theme);
    state.record_render(key, 1000);

    assert!(
        !state.should_render(key, 5000, false),
        "内容与外观都没变时不应重排（否则每帧都白做一次排版）"
    );
}

#[test]
fn theme_change_triggers_immediate_rebuild_without_waiting_for_debounce() {
    let dark = Theme::dark();
    let light = Theme::light();
    let mut state = PreviewUiState::new();

    let dark_key = PreviewCacheKey::new(DOC, 1, &dark);
    state.record_render(dark_key, 1000);

    // 切主题：版本号没变，但配色变了。
    let light_key = PreviewCacheKey::new(DOC, 1, &light);
    assert!(
        state.should_render(light_key, 1010, false),
        "主题变更必须立即触发重建，不能等防抖（否则会有 150ms 的视觉延迟）"
    );
}

#[test]
fn font_size_change_triggers_preview_rebuild() {
    let mut theme = Theme::dark();
    let mut state = PreviewUiState::new();
    let key = PreviewCacheKey::new(DOC, 1, &theme);
    state.record_render(key, 1000);

    // 改预览字号。
    theme.metrics.preview_font_size = 20.0;
    let new_key = PreviewCacheKey::new(DOC, 1, &theme);
    assert!(
        state.should_render(new_key, 1010, false),
        "字号变化必须触发重排，否则设置里的字号看起来无效"
    );
}

#[test]
fn render_count_tracks_actual_rebuilds() {
    let theme = Theme::dark();
    let mut state = PreviewUiState::new();
    assert_eq!(state.render_count, 0);

    let key = PreviewCacheKey::new(DOC, 1, &theme);
    if state.should_render(key, 0, false) {
        state.record_render(key, 0);
    }
    assert_eq!(state.render_count, 1);

    // 同样的键再来一次，不应增加。
    if state.should_render(key, 1000, false) {
        state.record_render(key, 1000);
    }
    assert_eq!(state.render_count, 1, "未变化时不得重复计数");
}

// ============================================================================
// S6-12：侧边栏状态
// ============================================================================

#[test]
fn directory_expansion_toggles() {
    let mut state = SidebarUiState::new();
    assert!(!state.is_expanded("/docs"));

    assert!(state.toggle_expanded("/docs"));
    assert!(state.is_expanded("/docs"));

    assert!(!state.toggle_expanded("/docs"), "再次切换应收起");
    assert!(!state.is_expanded("/docs"));

    // 多个目录可同时展开。
    state.toggle_expanded("/a");
    state.toggle_expanded("/b");
    assert_eq!(state.expanded_dirs.len(), 2);

    state.collapse_all();
    assert!(state.expanded_dirs.is_empty());
}

#[test]
fn outline_level_is_clamped() {
    let mut state = SidebarUiState::new();
    assert_eq!(state.outline_max_level, 6);

    state.set_outline_max_level(0);
    assert_eq!(state.outline_max_level, 1, "0 级不是合法的标题级别");

    state.set_outline_max_level(99);
    assert_eq!(state.outline_max_level, 6, "标题最多 6 级");

    state.set_outline_max_level(3);
    assert!(state.outline_level_visible(1));
    assert!(state.outline_level_visible(3));
    assert!(!state.outline_level_visible(4));
    assert!(!state.outline_level_visible(0), "0 级不是合法标题");
}

#[test]
fn empty_search_query_does_not_trigger_search() {
    let mut state = SidebarUiState::new();
    assert!(!state.should_search(), "空查询不应触发全盘扫描");

    state.search_query = "   ".to_string();
    assert!(!state.should_search(), "纯空白同样不触发");

    state.search_query = "rust".to_string();
    assert!(state.should_search());
}

#[test]
fn search_options_report_active_count() {
    assert!(SearchOptions::default().is_plain());
    assert_eq!(SearchOptions::default().active_count(), 0);

    let options = SearchOptions {
        case_sensitive: true,
        regex: true,
        whole_word: false,
    };
    assert_eq!(options.active_count(), 2);
    assert!(!options.is_plain());
}

// ============================================================================
// S6-12：浮层栈（模态）
// ============================================================================

#[test]
fn overlay_stack_is_last_in_first_out() {
    let mut state = DialogUiState::new();
    assert!(!state.any_open());
    assert!(!state.is_modal());

    state.open(Overlay::Settings);
    assert!(state.is_open(Overlay::Settings));
    assert!(state.any_open());
    assert!(state.is_modal(), "有浮层打开时必须模态（背景不可交互）");
    assert_eq!(state.top(), Some(Overlay::Settings));

    state.open(Overlay::About);
    assert_eq!(state.top(), Some(Overlay::About), "后开的应在栈顶");

    let closed = state.close_top().expect("应能关闭栈顶");
    assert_eq!(closed, Overlay::About);
    assert_eq!(state.top(), Some(Overlay::Settings));

    state.close_top().expect("应能关闭设置");
    assert!(!state.any_open());
    assert!(!state.is_modal());
}

#[test]
fn reopening_same_overlay_does_not_duplicate_it() {
    let mut state = DialogUiState::new();
    state.open(Overlay::Find);
    state.open(Overlay::Find);
    state.open(Overlay::Find);

    // 栈里只应有一项，否则 Esc 一次关不掉。
    state.close_top().expect("应能关闭");
    assert!(
        !state.any_open(),
        "重复打开同一浮层不应产生多个栈项（Esc 一次关不掉）"
    );
}

#[test]
fn reopening_moves_existing_overlay_to_top() {
    let mut state = DialogUiState::new();
    state.open(Overlay::Settings);
    state.open(Overlay::Find);
    // 再次打开设置 → 提到栈顶。
    state.open(Overlay::Settings);
    assert_eq!(state.top(), Some(Overlay::Settings));
    // 栈里仍只有两项。
    state.close_top();
    state.close_top().expect("应能关闭查找");
    assert!(!state.any_open());
}

#[test]
fn closing_non_top_overlay_keeps_stack_consistent() {
    let mut state = DialogUiState::new();
    state.open(Overlay::Settings);
    state.open(Overlay::Find);
    state.open(Overlay::About);

    // 关掉栈底的设置。
    assert!(state.close(Overlay::Settings));
    assert!(!state.is_open(Overlay::Settings));
    assert_eq!(state.top(), Some(Overlay::About));

    // 关掉栈顶后应露出查找，而不是关闭失败。
    let closed = state.close_top().expect("应能关闭");
    assert_eq!(closed, Overlay::About);
    assert_eq!(state.top(), Some(Overlay::Find), "栈底移除后栈必须仍然一致");
}

#[test]
fn close_top_on_empty_stack_returns_none() {
    let mut state = DialogUiState::new();
    assert_eq!(state.close_top(), None, "空栈关闭应返回 None 而非 panic");
    assert!(
        !state.close(Overlay::Settings),
        "关闭未打开的浮层应返回 false"
    );
}

#[test]
fn escape_close_request_is_queued_then_consumed() {
    let mut state = DialogUiState::new();
    state.open(Overlay::Find);

    state.request_close();
    assert_eq!(
        state.take_pending_close(),
        Some(Overlay::Find),
        "Esc 应排队一个关闭请求"
    );
    assert_eq!(
        state.take_pending_close(),
        None,
        "请求只能被消费一次（否则一次 Esc 会关掉多个浮层）"
    );
}

#[test]
fn overlay_titles_are_non_empty() {
    // 刻意遍历 `Overlay::ALL` 而非手写清单：
    // 手写清单在**新增**变体时会漏改（本次就漏了 `Symbol` / `Diff`），
    // 于是新浮层可以没有标题而测试照样绿。
    for overlay in Overlay::ALL.iter().copied() {
        assert!(!overlay.title().is_empty(), "{overlay:?} 缺少标题");
        assert!(
            overlay.opened_by().is_some(),
            "{overlay:?} 应能追溯到开启它的动作（用于焦点归还）"
        );
    }
}

/// 每个浮层动作都真的能打开**某个**浮层，且动作与浮层一一对应。
///
/// 这条钉住「`opened_by` 与 `openable_overlay` 互为逆映射」这条不变量。
/// 它坏掉时的表现是**静默**的：动作被 `execute` 判为「打开浮层」后
/// 找不到对应浮层，直接 `return` 而什么都不做——菜单里点了没反应，
/// 快捷键也没反应，且没有任何报错（S6-07 修的正是这个）。
#[test]
fn overlay_actions_roundtrip_through_the_inverse_mapping() {
    for overlay in Overlay::ALL.iter().copied() {
        let action = overlay.opened_by().expect("每个浮层都应有开启动作");
        // 反向：给定动作，必须能找回该浮层。
        let back = Overlay::ALL
            .iter()
            .copied()
            .find(|o| o.opened_by() == Some(action));
        assert_eq!(
            back,
            Some(overlay),
            "{action:?} 反查不到 {overlay:?}——逆映射不成立，该动作会静默失效"
        );
    }
    // 不同的浮层不得共用同一个动作（否则后一个永远打不开）。
    let mut seen = std::collections::HashSet::new();
    for overlay in Overlay::ALL.iter().copied() {
        let action = overlay.opened_by().expect("每个浮层都应有开启动作");
        assert!(
            seen.insert(action),
            "{action:?} 打开了多个浮层，后一个将永远打不开"
        );
    }
}

#[test]
fn switching_document_closes_all_overlays() {
    let mut state = DialogUiState::new();
    state.open(Overlay::Find);
    state.open(Overlay::Settings);
    state.reset_for_new_document();
    assert!(!state.any_open(), "切文档后不应残留浮层");
}

// ============================================================================
// 分层断言（S6-12 验收：UI State 中不含业务类型）
// ============================================================================

/// `Debug` 输出里不得出现业务类型特征词。
///
/// **这条检查的边界要先说清楚**：`Debug` 只打印字段名与**字段值**，
/// 并不打印字段的**类型名**。所以它能抓到的是「UI State 里存了一份
/// 业务对象，其`Debug` 泄漏到了状态转储里」——例如某个字段被改成
/// `Arc<DocumentView>`，转储里就会出现 `DocumentView { … }`。
///
/// 抓不到的是「字段是业务类型但`Debug` 恰好不泄漏」（例如存了
/// `Arc<dyn DocumentSource>`），那一层由 `ui_state_has_no_dto_fields`
/// 的编译期类型断言和 `check-arch` 的依赖禁令兜底。
///
/// 检查词表刻意**不含**裸 `Document`：`UiAction::NewDocument` /
/// `OpenDocument` / `DocumentStats` 是**表现层自己的动作名**，
/// 它们出现在注册表转储里完全合法。用裸词会得到一个永远失败、
/// 逼人去放宽规则的用例——这类用例跑久了就没人看了。
#[test]
fn ui_state_debug_output_contains_no_business_types() {
    let forbidden = [
        "Rope",
        "DocumentView",
        "RenderedView",
        "Preferences",
        "Workspace",
        "ExportJob",
        "BufferFactory",
        "MarkdownApp",
        "TextBuffer",
    ];

    let app = AppUiState::new(Theme::dark());
    let editor = EditorUiState::new();
    let preview = PreviewUiState::new();
    let sidebar = SidebarUiState::new();
    let dialog = DialogUiState::new();
    let find = FindReplaceUiState::new();

    for (name, dump) in [
        ("AppUiState", format!("{app:?}")),
        ("EditorUiState", format!("{editor:?}")),
        ("PreviewUiState", format!("{preview:?}")),
        ("SidebarUiState", format!("{sidebar:?}")),
        ("DialogUiState", format!("{dialog:?}")),
        ("FindReplaceUiState", format!("{find:?}")),
    ] {
        for word in forbidden {
            assert!(
                !dump.contains(word),
                "{name} 的字段里出现了业务类型「{word}」，违反 UI State 分层"
            );
        }
    }
}

/// `check-arch` 已保证表现层不依赖 domain / infrastructure；
/// 本用例补上「UI State 不持有 DTO 副本」这一层的检查。
#[test]
fn ui_state_has_no_dto_fields() {
    // 用字段访问而非类型断言：任何 `pub` 字段若被改成 DTO 类型，
    // 下面这些读取会因类型不匹配而编译失败。
    let state = AppUiState::new(Theme::dark());
    let _: bool = state.sidebar_visible;
    let _: SidebarTab = state.sidebar_tab;
    let _: bool = state.preview_visible;
    let _: f32 = state.split_ratio;
    let _: bool = state.focus_mode;
    let _: usize = state.active_tab;
    let _: usize = state.tab_count;
}
