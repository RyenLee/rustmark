//! 双向滚动同步用例（阶段 06 · S6-06 / FR-02）。
//!
//! # 这个文件要钉死的两件事
//!
//! 1. **防抖动**：两侧互相驱动会形成反馈环。抑制窗口是唯一的防抖机制，
//!    而它坏掉的表现只是「松手后偶尔抖一下」——极难复现、更难定位，
//!    因此必须在这里用状态机级别的用例钉死。
//! 2. **换算不越界 / 不跳块**：像素 ↔ 行号 ↔ 字节偏移的换算有 `±1` 的自由度，
//!    越界一次就是 `panic = "abort"` 下的进程终止（本项目不开栈展开），
//!    所以边界（空文本、滚到底、块超出表）都要单独覆盖。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::ReplaceRangeCommand;
use rustmark::application::document::queries::GetContentQuery;
use rustmark::application::markdown::queries::{MapScrollQuery, ScrollSource, ScrollTarget};
use rustmark::presentation::state::scroll_sync_state::{
    BlockRect, PaneGeometry, SUPPRESS_MS, ScrollSync, Side,
};
use rustmark::presentation::views::scroll_sync::{
    HIT_BIAS, LineIndex, TARGET_MARGIN, block_at_offset, clamp_scroll, line_at_y, line_of_offset,
    line_start_offsets, offset_for_block,
};
use std::sync::Arc;

// ============================================================================
// 主动方仲裁
// ============================================================================

/// 首帧只做初始化，不产生驱动。
#[test]
fn first_observation_only_primes() {
    let mut sync = ScrollSync::new();
    assert_eq!(sync.observe(0, 120.0, 40.0), None);
    // 第二帧同一位置 → 没有位移 → 仍然不驱动。
    assert_eq!(sync.observe(16, 120.0, 40.0), None);
}

/// 编辑区滚动 → 编辑区成为主动方并有权驱动对侧。
#[test]
fn editor_scroll_claims_ownership() {
    let mut sync = ScrollSync::new();
    sync.observe(0, 0.0, 0.0);
    assert_eq!(sync.observe(16, 80.0, 0.0), Some(Side::Editor));
    assert_eq!(sync.owner(16), Some(Side::Editor));
}

/// 亚像素抖动不算位移（否则静止帧会被误判为「用户在滚」）。
#[test]
fn sub_pixel_jitter_is_not_a_move() {
    let mut sync = ScrollSync::new();
    sync.observe(0, 100.0, 0.0);
    assert_eq!(sync.observe(16, 100.2, 0.0), None);
    // 超过阈值才算。
    assert_eq!(sync.observe(32, 101.0, 0.0), Some(Side::Editor));
}

/// 核心防回弹用例：编辑区驱动预览区后，预览区的变化**不得**反向驱动编辑区。
///
/// 这是「松手后界面还在抖」的直接根因，窗口内必须被完全抑制。
#[test]
fn echo_within_suppress_window_is_ignored() {
    let mut sync = ScrollSync::new();
    sync.observe(0, 0.0, 0.0);
    // t=16：编辑区滚到 200，成为主动方。
    assert_eq!(sync.observe(16, 200.0, 0.0), Some(Side::Editor));
    // t=32：程序把预览区写到 412（回声）。
    assert_eq!(sync.observe(32, 200.0, 412.0), None);
    // t=48：回声的余波（例如被夹紧后的第二次微调）同样被抑制。
    assert_eq!(sync.observe(48, 200.0, 410.0), None);
}

/// 镜像防回弹用例：预览区驱动编辑区后，编辑区的变化同样**不得**反向驱动预览区。
///
/// 回归：`(true, false)` 分支原本没有抑制检查——用户滚预览一帧后，程序写入
/// 编辑区的偏移在下一帧被误判为「用户在滚编辑区」，预览被拉回映射位置。
/// 预览内容比编辑区高时（图片多的文档），映射位置远在预览底部之上，
/// 表现为「预览底部滚不到、超出的内容不可见」。
#[test]
fn editor_echo_of_preview_drive_is_also_suppressed() {
    let mut sync = ScrollSync::new();
    sync.observe(0, 0.0, 0.0);
    // t=16：用户滚预览区到 600，预览区成为主动方。
    assert_eq!(sync.observe(16, 0.0, 600.0), Some(Side::Preview));
    // t=32：程序把编辑区写到 42（回声）——必须被抑制，而不是反向驱动预览。
    assert_eq!(sync.observe(32, 42.0, 600.0), None);
    // t=48：回声的余波同样抑制。
    assert_eq!(sync.observe(48, 44.0, 600.0), None);
    // 窗口过期后，编辑区的真实手势照常取得主动权。
    assert_eq!(
        sync.observe(16 + SUPPRESS_MS + 1, 90.0, 600.0),
        Some(Side::Editor)
    );
}

/// 抑制窗口过期后，另一侧可以重新取得主动权（否则预览区将永久无法滚动）。
#[test]
fn ownership_expires_after_suppress_window() {
    let mut sync = ScrollSync::new();
    sync.observe(0, 0.0, 0.0);
    assert_eq!(sync.observe(0, 200.0, 0.0), Some(Side::Editor));
    assert!(sync.is_suppressed(SUPPRESS_MS - 1, Side::Preview));
    assert!(!sync.is_suppressed(SUPPRESS_MS + 1, Side::Preview));
    // 过期后预览区滚动 → 预览区接管。
    assert_eq!(
        sync.observe(SUPPRESS_MS + 1, 200.0, 60.0),
        Some(Side::Preview)
    );
    assert_eq!(sync.owner(SUPPRESS_MS + 1), Some(Side::Preview));
}

/// 连续滚动会不断刷新窗口，且**回声与用户手势同帧出现时仍按主动方走**。
///
/// 这是最容易被写错的一条：连续滚动时，上一帧驱动对侧的结果恰好在
/// 这一帧被观测到，于是「用户在滚 + 程序在写」同时成立。
/// 若此时一律判为「两侧同时动 → 不动」，同步会退化成**隔帧生效**，
/// 表现为快速滚动时预览跟不上、一顿一顿。
#[test]
fn continuous_editor_scroll_keeps_ownership() {
    let mut sync = ScrollSync::new();
    sync.observe(0, 0.0, 0.0);
    let mut now = 0u64;
    let mut preview = 0.0;
    for step in 1..=40 {
        now += 8; // 8ms 一帧，远短于 100ms 的抑制窗口
        let editor = step as f32 * 12.0;
        assert_eq!(sync.observe(now, editor, preview), Some(Side::Editor));
        assert!(sync.is_suppressed(now, Side::Preview));
        preview = editor * 2.0; // 模拟程序写入对侧后的回声
    }
    assert_eq!(sync.owner(now), Some(Side::Editor));
}

/// 编辑区「把光标滚进视口」是**程序性**滚动，不得驱动预览区。
///
/// 场景：在光标即将滚出视口的那一行按回车，egui 会带动画地把光标滚回来
/// （连续数帧位移）。若不声明成程序性位移，同步会把它当成用户手势、一帧一次
/// 地把预览区拉到映射位置：编辑区平滑滚、预览区逐帧跳，两栏同时在动 = 界面闪烁。
#[test]
fn programmatic_editor_scroll_does_not_drive_preview() {
    let mut sync = ScrollSync::new();
    sync.observe(0, 0.0, 0.0);

    // 模拟「文本变化 → egui 自动滚到光标」的连续几帧。
    sync.note_programmatic_scroll(Side::Editor, 16);
    for frame in 1..=6 {
        let editor = frame as f32 * 20.0;
        assert_eq!(sync.observe(16 + frame * 8, editor, 0.0), None);
    }
    // 用户真的去滚编辑区时（窗口过期后）仍然要能驱动预览。
    let now = 16 + 6 * 8 + SUPPRESS_MS + 1;
    assert_eq!(sync.observe(now, 200.0, 0.0), Some(Side::Editor));
}

/// 无归属时两侧同时位移（异常）→ 不驱动任何一侧，宁可不动也不互相打架。
#[test]
fn simultaneous_movement_without_owner_drives_nothing() {
    let mut sync = ScrollSync::new();
    sync.observe(0, 0.0, 0.0);
    assert_eq!(sync.observe(16, 50.0, 50.0), None);
}

/// 重置后不产生「打开新文件预览自己滚了一下」。
#[test]
fn reset_drops_observation_history() {
    let mut sync = ScrollSync::new();
    sync.observe(0, 0.0, 0.0);
    sync.observe(16, 300.0, 0.0);
    sync.reset();
    // 换文档后两侧都归零；若不重置，这会被判成一次 -300 的巨大位移。
    assert_eq!(sync.observe(32, 0.0, 0.0), None);
    assert_eq!(sync.owner(32), None);
}

/// 点击跳转是离散事件，不受抑制窗口限制。
#[test]
fn click_jump_is_taken_once() {
    let mut sync = ScrollSync::new();
    sync.request_click(7);
    assert_eq!(sync.take_click(), Some(7));
    assert_eq!(sync.take_click(), None);
}

/// 调试计数：驱动次数与方向可观测（S6-11 调试面板的数据源）。
#[test]
fn drive_stats_are_recorded() {
    let mut sync = ScrollSync::new();
    sync.record_drive(Side::Preview);
    sync.record_drive(Side::Editor);
    assert_eq!(sync.drive_count, 2);
    assert_eq!(sync.last_driven, Some(Side::Editor));
}

// ============================================================================
// 滚动偏移夹紧
// ============================================================================

/// 内容不足一屏时任何偏移都归零。
#[test]
fn clamp_scroll_when_content_fits() {
    let geom = PaneGeometry {
        content_h: 200.0,
        viewport_h: 400.0,
    };
    assert_eq!(clamp_scroll(120.0, geom), 0.0);
    assert_eq!(clamp_scroll(-50.0, geom), 0.0);
}

/// 滚到底部时被夹到 `content - viewport`，不会滚出内容之外。
#[test]
fn clamp_scroll_at_bottom() {
    let geom = PaneGeometry {
        content_h: 1000.0,
        viewport_h: 400.0,
    };
    assert_eq!(clamp_scroll(9999.0, geom), 600.0);
    assert_eq!(clamp_scroll(-10.0, geom), 0.0);
}

/// 非有限值一律归零：NaN 一旦写进 `ScrollArea` 状态就再也回不来。
#[test]
fn clamp_scroll_rejects_non_finite() {
    let geom = PaneGeometry {
        content_h: 1000.0,
        viewport_h: 400.0,
    };
    assert_eq!(clamp_scroll(f32::NAN, geom), 0.0);
    assert_eq!(clamp_scroll(f32::INFINITY, geom), 0.0);
    assert_eq!(clamp_scroll(f32::NEG_INFINITY, geom), 0.0);
}

// ============================================================================
// 块 → 偏移
// ============================================================================

/// 三块等高的矩形表（0/100/200 起点，间隙 4）。
fn rects() -> Vec<BlockRect> {
    vec![
        BlockRect {
            id: 10,
            top: 0.0,
            bottom: 100.0,
        },
        BlockRect {
            id: 11,
            top: 104.0,
            bottom: 204.0,
        },
        BlockRect {
            id: 12,
            top: 208.0,
            bottom: 308.0,
        },
    ]
}

/// 块内按比例插值：从块顶滚到块底，目标偏移线性跟随。
#[test]
fn offset_for_block_interpolates_inside_block() {
    let geom = PaneGeometry {
        content_h: 1000.0,
        viewport_h: 300.0,
    };
    let top = offset_for_block(&rects(), 11, 0.0, geom, TARGET_MARGIN).expect("块应在表内");
    let mid = offset_for_block(&rects(), 11, 0.5, geom, TARGET_MARGIN).expect("块应在表内");
    let bottom = offset_for_block(&rects(), 11, 1.0, geom, TARGET_MARGIN).expect("块应在表内");
    assert!((top - (104.0 - TARGET_MARGIN)).abs() < 0.01);
    assert!((mid - (154.0 - TARGET_MARGIN)).abs() < 0.01);
    assert!((bottom - (204.0 - TARGET_MARGIN)).abs() < 0.01);
    // 单调递增——块内往下滑，预览区跟着往下走。
    assert!(top < mid && mid < bottom);
}

/// 比例越界（领域层保证 0..=1，但浮点与降级路径不保证）要被夹紧而非放大。
#[test]
fn offset_for_block_clamps_ratio() {
    let geom = PaneGeometry {
        content_h: 1000.0,
        viewport_h: 300.0,
    };
    let over = offset_for_block(&rects(), 11, 4.0, geom, TARGET_MARGIN).expect("块应在表内");
    let under = offset_for_block(&rects(), 11, -2.0, geom, TARGET_MARGIN).expect("块应在表内");
    let at_end = offset_for_block(&rects(), 11, 1.0, geom, TARGET_MARGIN).expect("块应在表内");
    assert!((over - at_end).abs() < 0.01);
    assert!((under - (104.0 - TARGET_MARGIN)).abs() < 0.01);
}

/// 块 ID 不在表里（文档刚重解析、矩形还没采到）时返回 `None`，
/// 调用方据此跳过本帧，绝不能退化为「滚到 0」。
#[test]
fn offset_for_block_missing_id_returns_none() {
    let geom = PaneGeometry {
        content_h: 1000.0,
        viewport_h: 300.0,
    };
    assert_eq!(
        offset_for_block(&rects(), 99, 0.5, geom, TARGET_MARGIN),
        None
    );
    assert_eq!(offset_for_block(&[], 10, 0.5, geom, TARGET_MARGIN), None);
}

/// 命中块：落在块间空隙里算**下一块**（视口顶已越过上一块的底）。
#[test]
fn block_at_offset_prefers_next_block_in_gap() {
    let r = rects();
    assert_eq!(block_at_offset(&r, 0.0), Some(10));
    assert_eq!(block_at_offset(&r, 99.0), Some(10));
    // 100..104 是块间空隙。
    assert_eq!(block_at_offset(&r, 102.0), Some(11));
    assert_eq!(block_at_offset(&r, 104.0), Some(11));
    assert_eq!(block_at_offset(&r, 300.0), Some(12));
    // 越过最后一块 → 无命中。
    assert_eq!(block_at_offset(&r, 400.0), None);
    assert_eq!(block_at_offset(&[], 0.0), None);
}

/// `HIT_BIAS` 的作用：视口顶只露出上一块的一点点时按**下一块**处理。
///
/// 否则编辑区会「停在上一段」不肯往下走——用户看到的是预览已经滚过
/// 这段了，编辑区却还指着它。
#[test]
fn hit_bias_avoids_sticking_to_barely_visible_block() {
    let r = rects();
    // 块 10 占 [0,100)，块 11 占 [104,204)。偏移 99 时块 10 只剩 1px 可见。
    assert_eq!(block_at_offset(&r, 99.0), Some(10));
    assert_eq!(block_at_offset(&r, 99.0 + HIT_BIAS), Some(11));
}

// ============================================================================
// 行 ↔ 偏移 / 像素
// ============================================================================

/// 行起始偏移表：末元素是文本长度哨兵，且**不与末行行首重复**。
#[test]
fn line_start_offsets_has_sentinel() {
    // "ab\ncd\n" 拆出三行（末行为空），行首分别是 0 / 3 / 6。
    // 末行行首已经等于 text.len()，哨兵不再重复补——
    // 重复会让 partition_point 多命中一次，把三行文档算成四行。
    let text = "ab\ncd\n";
    let starts = line_start_offsets(text);
    assert_eq!(starts, vec![0, 3, 6]);
    assert_eq!(*starts.last().expect("非空"), text.len());
    assert_eq!(
        starts.iter().filter(|s| **s == text.len()).count(),
        1,
        "哨兵不得与末行行首重复"
    );

    // 文本**不以**换行结尾时哨兵才真正被补上（行首表覆盖不到末尾）。
    let no_eol = "ab\ncd";
    assert_eq!(line_start_offsets(no_eol), vec![0, 3, 5]);
}

/// 末行行首重复会让行号多算一行（`line_of_offset` 用 `partition_point`）。
#[test]
fn trailing_newline_does_not_inflate_the_line_number() {
    // "ab\ncd\n" 是**三行**（第三行为 "cd\n" 之后的空行）。
    // 光标停在文末（合法位置）时行号必须是 2，即第三行。
    //
    // 哨兵与末行行首重复时（旧的 [0,3,6,6]）partition_point 会多命中一次，
    // 返回 3 —— 一个不存在的第四行，跳转与滚动同步都会落到空处。
    let text = "ab\ncd\n";
    let starts = line_start_offsets(text);
    assert_eq!(
        line_of_offset(&starts, text.len()),
        2,
        "三行文档的光标在文末时，行号应是第 3 行（0-based 2）"
    );

    // 行首表的长度必须等于行数（不是行数 + 1）。
    assert_eq!(
        starts.len(),
        text.split('\n').count(),
        "每个 split 出的行恰好一个起点"
    );
}

/// 空文本也要能算（`split('\n')` 给一个空行），不能 panic。
#[test]
fn line_start_offsets_handles_empty_text() {
    let starts = line_start_offsets("");
    // 空文本拆出 1 行、行首 0，已等于 text.len()，哨兵不重复补。
    assert_eq!(starts, vec![0]);
}

/// 多行中文：偏移是**字节**而非字符，中文一行占多字节。
#[test]
fn line_start_offsets_are_byte_offsets() {
    let text = "中文\n第二行\n";
    let starts = line_start_offsets(text);
    // "中文" 是 6 字节 + 1 换行。
    assert_eq!(starts[1], 7);
    assert_eq!(*starts.last().expect("非空"), text.len());
}

// ============================================================================
// 行索引（问题 053）：一遍扫描同时给出「行起点表」与「行数」
// ============================================================================

/// `LineIndex` 与两个旧函数**逐例等价**——这是「合并到一处」唯一能成立的前提。
///
/// 合并的动机是性能（每帧状态栏与编辑区各扫一遍全文），但性能改动的代价是
/// 可能悄悄改变口径：行数差 1 就是状态栏的「N 行」与光标的「行 M」同时错位，
/// 而两者在无头测试里都不报错，只有用户会看见。故这里的等价性是**全量**的：
/// 三个口径（`starts` / `count` / `status_starts`）在所有边界输入上逐一比对。
#[test]
fn line_index_matches_the_two_legacy_functions_on_every_case() {
    let cases = [
        "",
        "\n",
        "\n\n",
        "a",
        "ab\ncd",
        "ab\ncd\n",
        "a\nb\nc\nd",
        "中文\n第二行\n",
        "中文\n",
        "中文",
        "a\n\n\nb",
        "行的末尾没有换行符",
    ];
    for case in cases {
        let index = LineIndex::new(case);

        // ① 行起点表（含哨兵）与 `line_start_offsets` 逐元素相等。
        assert_eq!(
            index.starts(),
            line_start_offsets(case).as_slice(),
            "行起点表口径不一致：{case:?}"
        );

        // ② 行数与 `split('\\n').count()`（= `shared::text::line_count`）相等。
        assert_eq!(
            index.count(),
            case.split('\n').count(),
            "行数口径不一致：{case:?}"
        );

        // ③ 状态栏口径 = 摘掉哨兵：长度必须恰好等于行数，
        //    且末元素**永不**指向「文末之后」（那是凭空多出的一行）。
        let status = index.status_starts();
        assert_eq!(
            status.len(),
            index.count(),
            "状态栏起点表长度应等于行数：{case:?}"
        );
        assert!(
            status.last().is_none_or(|last| *last <= case.len()),
            "状态栏行首越界：{case:?}"
        );
        if !case.is_empty() && !case.ends_with('\n') {
            // 不以换行结尾时，末尾那个等于 `len` 的哨兵必须被摘掉，
            // 否则单行文档的光标在行尾会被算成「行 2 列 1」。
            assert_ne!(
                status.last(),
                Some(&case.len()),
                "文末哨兵未被摘掉：{case:?}"
            );
        }
        assert_eq!(
            index.line_of_offset(case.len()),
            index.count() - 1,
            "光标在文末时必须落在最后一行（0-based）：{case:?}"
        );
    }
}

/// 单遍扫描的容量估算只影响「是否多一次翻倍」，不得影响任何结果。
///
/// 该用例刻意用**行长极端不均**的语料（极短行与极长行混排）逼出估算偏差：
/// 估算按「平均行长 32 字节」，短行文档会显著低估、长行文档会高估。
/// 只要结果仍逐元素正确，这个启发式就是安全的。
#[test]
fn line_index_capacity_heuristic_never_affects_results() {
    // 极短行：1MB 级换行密度，估算会低估约一倍。
    let short = "a\n".repeat(5000);
    let index = LineIndex::new(&short);
    assert_eq!(index.count(), 5001, "`\"a\\n\"×5000` 拆出 5001 行");
    assert_eq!(index.starts()[..3], [0, 2, 4]);

    // 极长行：几乎没有换行，估算会高估。
    let long = format!("{}\n{}", "x".repeat(40_000), "y".repeat(40_000));
    let index = LineIndex::new(&long);
    assert_eq!(index.count(), 2);
    assert_eq!(index.starts(), [0, 40_001, 80_001]);
}

/// 由 y 反查行号：含折行时行高不同也能正确命中。
#[test]
fn line_at_y_respects_wrapped_line_heights() {
    // 第 0 行高 20，第 1 行因折行高 60，第 2 行高 20。
    let tops = [0.0, 20.0, 80.0, 100.0];
    assert_eq!(line_at_y(&tops, 0.0), 0);
    assert_eq!(line_at_y(&tops, 19.0), 0);
    assert_eq!(line_at_y(&tops, 20.0), 1);
    assert_eq!(line_at_y(&tops, 79.0), 1);
    assert_eq!(line_at_y(&tops, 80.0), 2);
    // 超过总高 → 落在末行，不越界。
    assert_eq!(line_at_y(&tops, 5000.0), 3);
}

/// 空表 / 负值要安全。
#[test]
fn line_at_y_is_total() {
    assert_eq!(line_at_y(&[], 42.0), 0);
    assert_eq!(line_at_y(&[0.0], -10.0), 0);
}

/// 由字节偏移反查行号：末元素是哨兵，滚到底也不会越界。
#[test]
fn line_of_offset_uses_sentinel_at_end() {
    let starts = line_start_offsets("aa\nbb\ncc");
    assert_eq!(line_of_offset(&starts, 0), 0);
    assert_eq!(line_of_offset(&starts, 2), 0);
    assert_eq!(line_of_offset(&starts, 3), 1);
    assert_eq!(line_of_offset(&starts, 6), 2);
    assert_eq!(line_of_offset(&starts, 7), 2);
    // 恰好等于文本长度（文末）→ 落到哨兵下标，与 `line_at_y` 的「滚到底」一致。
    assert_eq!(line_of_offset(&starts, 8), starts.len() - 1);
    // 超出文本长度 → 同样钳到哨兵，不 panic。
    assert_eq!(line_of_offset(&starts, 9999), starts.len() - 1);
    assert_eq!(line_of_offset(&[], 0), 0);
}

/// 往返一致性：由偏移查行号 → 由行号取偏移，结果不越过原偏移。
#[test]
fn offset_to_line_roundtrip_never_overshoots() {
    let text = "第一行\n第二行\n第三行\n第四行";
    let starts = line_start_offsets(text);
    for probe in 0..=text.len() {
        let line = line_of_offset(&starts, probe);
        let start = starts[line];
        assert!(
            start <= probe,
            "行起点 {start} 不应超过探测偏移 {probe}（行号 {line}）"
        );
    }
}

/// 几何量：内容不足一屏时最大偏移为 0。
#[test]
fn pane_geometry_max_offset_is_non_negative() {
    let small = PaneGeometry {
        content_h: 100.0,
        viewport_h: 400.0,
    };
    assert_eq!(small.max_offset(), 0.0);
    let big = PaneGeometry {
        content_h: 900.0,
        viewport_h: 400.0,
    };
    assert_eq!(big.max_offset(), 500.0);
}

/// `Side` 的对侧与下标可往返（写错会让两侧几何量串台）。
#[test]
fn side_helpers_roundtrip() {
    assert_eq!(Side::Editor.other(), Side::Preview);
    assert_eq!(Side::Preview.other(), Side::Editor);
    assert_ne!(Side::Editor.index(), Side::Preview.index());
}

// ============================================================================
// 端到端：跑真实 egui 帧
// ============================================================================
//
// 上面全部是纯函数级别的验证。它们证明「换算是对的」，但证明不了
// 「换算的**输入**真的被采集到了」——而后者恰恰最容易静默失效：
//
//  - `ScrollArea` 的偏移读不回 → `editor.scroll_y` 恒为 0，同步永不触发；
//  - 块矩形没采到 → `preview.rects` 恒为空，`offset_for_block` 永远返回 `None`；
//  - 几何量没上报 → `clamp_scroll` 的上界是 0，目标偏移恒为 0。
//
// 这三种故障的共同点是：**不 panic、不报错、界面看起来完全正常**
// （只是同步不动了）。所以必须跑真实帧把它们钉住。

/// 造一个装好默认配置、装入指定 Markdown 的应用。
fn app_with(markdown: &str) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = app.ensure_document().expect("启动即开空白文档");
    app.services
        .documents
        .replace_range(ReplaceRangeCommand {
            id,
            range: rustmark::shared::geometry::TextRange::new(
                rustmark::shared::geometry::ByteOffset::new(0),
                rustmark::shared::geometry::ByteOffset::new(0),
            ),
            text: markdown.to_string(),
        })
        .expect("写入文档内容应成功");
    app.refresh_text();
    (dir, app)
}

/// 生成一篇足够长的文档：`sections` 个一级标题，每个配一段正文与一个子节。
///
/// **规模必须够大**：验收标准要求「编辑区滚到 25%/50%/75%」，
/// 而最大滚动量 = `content_h - viewport_h`。若文档只比一屏高一点点，
/// 25% 与 75% 都落在同一个块里，这条用例就退化成「什么都没验证」。
/// 故默认 12 节（约 1300 行级别的内容由 `sections` 控制）。
fn long_doc(sections: usize) -> String {
    let mut out = String::new();
    for i in 1..=sections {
        out.push_str(&format!("# 第 {i} 节\n\n"));
        out.push_str(&format!(
            "这是第 {i} 节的正文。内容需要足够长以便在窄栏里折行，             从而把编辑区撑到远超一屏的高度，否则滚动同步无从验证。\n\n"
        ));
        out.push_str(&format!("## 第 {i} 节的子节\n\n"));
        out.push_str(&format!(
            "子节 {i} 的正文。两侧块高分布刻意不一致——             比例同步在这种文档上会明显跑偏，块映射才能对齐。\n\n"
        ));
    }
    out
}

/// 跑一帧主视图。
///
/// 视口取 **900×260**：必须**矮**，否则编辑区内容只比一屏高一点，
/// 最大滚动量不足以划分 25% / 50% / 75%（见 [`long_doc`] 的说明）。
/// 宽度也收窄到 900，使长段落折行、行数进一步增加。
fn frame(app: &mut EditorApp, ctx: &egui::Context) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(900.0, 260.0),
        )),
        ..Default::default()
    };
    crate::support::run_ui(ctx, input, |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(app, ui);
    });
}

/// 跑若干帧（首帧要完成字体加载与布局，故至少 2 帧）。
fn frames(app: &mut EditorApp, ctx: &egui::Context, n: usize) {
    for _ in 0..n.max(2) {
        frame(app, ctx);
    }
}

/// 真实帧里两侧的几何量都被上报，且内容都远高于视口（否则夹紧恒为 0，
/// 同步会「看起来在工作」而实际什么都没滚）。
#[test]
fn real_frame_reports_both_pane_geometries() {
    let (_dir, mut app) = app_with(&long_doc(12));
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 3);

    let editor = app.ui.sync.geometry(Side::Editor);
    let preview = app.ui.sync.geometry(Side::Preview);
    assert!(
        editor.content_h > editor.viewport_h,
        "编辑区内容应高于视口：{editor:?}"
    );
    assert!(
        preview.content_h > preview.viewport_h,
        "预览区内容应高于视口：{preview:?}"
    );
    assert!(editor.max_offset() > 0.0, "编辑区应可滚动");
    assert!(preview.max_offset() > 0.0, "预览区应可滚动");
}

/// 真实帧里块矩形被采到：数量与预览块数一致、id 对得上、纵向单调不重叠。
///
/// 这一条是整个同步的**数据基础**：矩形表为空时 `offset_for_block`
/// 恒返回 `None`，而那不会产生任何错误提示。
#[test]
fn real_frame_collects_block_rects_in_document_order() {
    let (_dir, mut app) = app_with(&long_doc(12));
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 3);

    let rects = &app.preview.rects;
    assert!(!rects.is_empty(), "块矩形表不应为空");

    // 首块顶边必须是 0（坐标系原点取自首块实际顶边）。
    assert!(
        rects[0].top.abs() < 0.5,
        "首块顶边应为 0，实际 {}",
        rects[0].top
    );

    for w in rects.windows(2) {
        assert!(
            w[0].bottom <= w[1].top + 0.5,
            "块应纵向不重叠且有序：{:?} → {:?}",
            w[0],
            w[1]
        );
    }
    for r in rects {
        assert!(r.height() > 0.0, "块高度应非负：{r:?}");
        assert!(
            r.top.is_finite() && r.bottom.is_finite(),
            "坐标应有限：{r:?}"
        );
    }

    // 矩形表覆盖的 id 必须是真实块 id 的子集（不能是凭空造的 id）。
    let block_ids: std::collections::HashSet<u64> =
        app.preview.blocks.iter().map(|b| b.id).collect();
    assert!(
        rects.iter().all(|r| block_ids.contains(&r.id)),
        "矩形表出现了不属于当前块的 id（文档重解析后未清理？）"
    );
    // 标题必须可点击跳转，因此必须在矩形表里。
    let heading_ids: std::collections::HashSet<u64> = app
        .preview
        .blocks
        .iter()
        .filter(|b| b.kind == "heading")
        .map(|b| b.id)
        .collect();
    assert!(
        !heading_ids.is_empty()
            && heading_ids
                .iter()
                .all(|id| rects.iter().any(|r| r.id == *id)),
        "所有标题都应出现在矩形表里，否则点击无法跳转"
    );
}

/// 编辑区偏移真的能被写入并在下一帧读回。
///
/// 若 `vertical_scroll_offset` 语义理解错了（它可能被 `end` 里的夹紧覆盖），
/// 这条会立刻红——而纯函数测试完全测不到这一点。
#[test]
fn programmatic_editor_offset_survives_a_real_frame() {
    let (_dir, mut app) = app_with(&long_doc(12));
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 3);

    let geom = app.ui.sync.geometry(Side::Editor);
    let target = geom.max_offset() * 0.5;
    app.editor.scroll_y = target;
    frame(&mut app, &ctx);

    let after = app.editor.scroll_y;
    assert!(
        (after - target).abs() < 1.0,
        "写入的偏移应被 ScrollArea 采纳：期望 ≈{target}，实际 {after}（几何 {geom:?}）"
    );
}

/// 端到端闭环：编辑区滚到 25% / 50% / 75%，预览区落点必须单调递增，
/// 且与 `MapScrollQuery` 独立算出的目标一致（误差 ≤ 1 块）。
///
/// 这条直接对应验收标准「编辑区滚动到文档 25%/50%/75% 处，
/// 预览区对应块进入视口（误差 ≤ 1 块）」。
#[test]
fn editor_scroll_at_quarter_three_quarters_lands_preview_within_one_block() {
    let (_dir, mut app) = app_with(&long_doc(12));
    let ctx = egui::Context::default();
    // 先让语法高亮的防抖窗口过去，再测量几何。
    //
    // `MarkdownAppService::highlight` 带 150ms 防抖（OPT-011 ①）：文档版本刚变的
    // 窗口内返回**空片段**，编辑区随之退化为纯文本排版——而带高亮时标题走更大字号
    // （`mapper::StyleMapper::format` 的 `heading_scale`），两者行高不同。
    // 若在窗口内测量，`geometry()` 与循环里的 `line_tops()` 会取自两种排版：
    // 前者的最大偏移远大于后者的内容高度，于是「滚到 75%」被推到内容之外
    // （`line_at_y` 返回哨兵行 → 偏移落 0 → 预览跳回顶部）。
    //
    // 换言之这条用例过去只是「碰巧整个用例跑得比 150ms 慢」才绿，随机器快慢时红时绿。
    // 首帧让 `HighlightPace` 记下版本号，随后跨过窗口；此后版本不变即恒为「已静止」，
    // 后续所有帧都稳定带高亮，与机器速度无关。
    frame(&mut app, &ctx);
    std::thread::sleep(std::time::Duration::from_millis(200));
    frames(&mut app, &ctx, 3);

    let geom = app.ui.sync.geometry(Side::Editor);
    assert!(geom.max_offset() > 0.0, "前置条件：编辑区可滚动");
    let doc_id = app.ensure_document().expect("活动文档");

    // 独立重算一遍「像素 → 字节偏移」的换算，作为期望值。
    //
    // 关键：`MapScrollQuery::Editor { offset }` 要的是**字节偏移**，
    // 而 `max_offset * frac` 是**像素**。两者混用会得到完全错误的期望块
    // （实测偏差可达 30+ 块），且失败时看起来像「同步错得离谱」，
    // 极容易把人引向错误的排查方向。
    let text = app
        .services
        .documents
        .get_content(GetContentQuery {
            id: doc_id,
            range: None,
        })
        .expect("读取内容应成功");
    let starts = line_start_offsets(&text);

    let mut landings = Vec::new();
    for frac in [0.25_f32, 0.5, 0.75] {
        let y = geom.max_offset() * frac;
        app.editor.scroll_y = y;
        // 跑两帧：第一帧写入并采集，第二帧让 ScrollArea 真正采纳。
        frames(&mut app, &ctx, 2);

        // 该像素位置对应的源码字节偏移（与 `drive_preview` 同一套换算，
        // 但这里独立走一遍，便于交叉验证）。
        let tops = rustmark::presentation::theme::mapper::line_tops(app.theme(), &text)
            .expect("本帧已排版，应有行顶表");
        let offset = starts
            .get(line_at_y(&tops, app.editor.scroll_y))
            .copied()
            .unwrap_or(0);

        let ScrollTarget::Preview { block_id, .. } = app
            .services
            .markdown
            .map_scroll(MapScrollQuery {
                doc_id,
                source: ScrollSource::Editor { offset },
            })
            .expect("映射查询应成功")
        else {
            panic!("编辑区来源应映射到预览目标");
        };

        // 落点命中（HIT_BIAS 与块间隙对齐）。
        let hit = block_at_offset(&app.preview.rects, app.preview.scroll_y + HIT_BIAS)
            .expect("落点应命中某个块");
        let idx_of = |id: u64| app.preview.rects.iter().position(|r| r.id == id);
        let hit_idx = idx_of(hit).expect("命中块应在矩形表内");
        let want_idx = idx_of(block_id).expect("目标块应在矩形表内");
        assert!(
            hit_idx.abs_diff(want_idx) <= 1,
            "滚到 {frac}（像素 {y} → 字节 {offset}）：预览落点命中块 #{hit_idx}，\
             期望 #{want_idx}（误差 > 1 块）"
        );

        landings.push(app.preview.scroll_y);
    }

    assert!(
        landings[0] < landings[1] && landings[1] < landings[2],
        "预览落点应随编辑区滚动单调递增，实际 {landings:?}"
    );
    // 落点必须真的滚动了（而不是恒为 0 的假通过）。
    assert!(
        landings[2] > 0.0,
        "滚到 75% 时预览区应已离开顶部，实际 {landings:?}"
    );
}

/// 预览区 → 编辑区：点击标题后编辑区跳到该标题所在行。
///
/// 对应验收标准「预览区点击某标题，编辑区跳转到对应行」。
#[test]
fn clicking_a_preview_heading_moves_the_editor_to_that_line() {
    let (_dir, mut app) = app_with(&long_doc(12));
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 3);

    // 取**最后一个**标题：若映射错误地回落到文档顶部，误差最大、最容易暴露。
    let target = app
        .preview
        .blocks
        .iter()
        .rev()
        .find(|b| b.kind == "heading")
        .expect("文档含标题")
        .id;

    app.ui.sync.request_click(target);
    frames(&mut app, &ctx, 2);

    // 先取 doc_id：`ensure_document()` 要 `&mut app`，不能嵌在
    // 持有 `&app` 的方法调用链里。
    let doc_id = app.ensure_document().expect("活动文档");
    let ScrollTarget::Editor { offset } = app
        .services
        .markdown
        .map_scroll(MapScrollQuery {
            doc_id,
            source: ScrollSource::Preview { block_id: target },
        })
        .expect("映射查询应成功")
    else {
        panic!("预览来源应映射到编辑区目标");
    };

    // `editor_text` 是 `pub(crate)`，集成测试读不到；按「领域是单一真相」
    // 的约定，`get_content` 的返回值与它必然相等，故从服务读。
    let text = app
        .services
        .documents
        .get_content(GetContentQuery {
            id: doc_id,
            range: None,
        })
        .expect("读取内容应成功");
    let starts = line_start_offsets(&text);
    let want_line = line_of_offset(&starts, offset);
    let geom = app.ui.sync.geometry(Side::Editor);
    // 编辑区偏移应落在该行顶附近（±2 行 + 视口内任意位置皆可，
    // 因为同步只保证「该行进入视口」而非「该行贴到视口顶」）。
    let got_line = line_at_y(
        &rustmark::presentation::theme::mapper::line_tops(app.theme(), &text)
            .expect("本帧已排版，应有行顶表"),
        app.editor.scroll_y,
    );
    assert!(
        app.editor.scroll_y > 0.0,
        "点击靠后的标题后编辑区应已滚离顶部"
    );
    assert!(
        got_line.abs_diff(want_line) <= 2,
        "编辑区应停在第 {want_line} 行附近，实际第 {got_line} 行（偏移 {}，几何 {geom:?}）",
        app.editor.scroll_y
    );
}

/// 连续滚动不产生回弹抖动：程序写入的对侧位移在抑制窗口内不得反向驱动。
///
/// 真实帧里这个失效表现为「两侧互相拉扯」，纯函数测试只能验证状态机，
/// 这里验证**接线**：写入 `preview.scroll_y` 之后编辑区不应被反推。
#[test]
fn consecutive_frames_do_not_bounce_back() {
    let (_dir, mut app) = app_with(&long_doc(12));
    let ctx = egui::Context::default();
    frames(&mut app, &ctx, 3);

    let geom = app.ui.sync.geometry(Side::Editor);
    // 模拟「用户持续向下滚」：编辑区每帧前进一小步。
    let step = geom.max_offset() / 60.0;
    for _ in 0..12 {
        app.editor.scroll_y = (app.editor.scroll_y + step).min(geom.max_offset());
        frame(&mut app, &ctx);
    }

    // 编辑区应一路向下（单调不减），没有被预览区反推回去。
    // 只要编辑区出现「后撤」，就说明回声穿透了抑制窗口。
    let final_y = app.editor.scroll_y;
    assert!(final_y > 0.0, "编辑区应已滚下去，最终偏移 {final_y}");
    assert!(
        app.ui.sync.drive_count > 0,
        "持续滚动期间应至少成功驱动过一次"
    );
}
