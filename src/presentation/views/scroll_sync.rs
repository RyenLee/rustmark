//! 双向滚动同步的**几何映射与驱动**（阶段 06 · S6-06 / FR-02）。
//!
//! # 两段式映射
//!
//! 两侧都只知道像素，而 `MapScrollQuery` 只认**块 ID 与字节偏移**，
//! 所以每个方向都要做两次换算：
//!
//! ```text
//!   编辑区 ──► 预览区
//!     ScrollArea 偏移（点）
//!       → 由行顶表反查行号 → 该行起始字节偏移
//!       → MapScrollQuery → (块 ID, 块内比例)
//!       → 由块矩形插值出 y → 预览区 ScrollArea 偏移
//!
//!   预览区 ──► 编辑区
//!     ScrollArea 偏移 → 命中块 ID → MapScrollQuery → 块源码起始字节
//!       → 反查行号 → 行顶 y → 编辑区 ScrollArea 偏移
//! ```
//!
//! # 为什么不用「按比例同步」
//!
//! 最简单的实现是 `预览偏移 = 编辑偏移 / 编辑最大偏移 × 预览最大偏移`。
//! 它零成本，但一遇到高度分布不均就失效：文档前半段是代码（行矮）、
//! 后半段是图片（块高），比例同步会让预览停在完全无关的段落上，
//! 直接违反验收标准「误差 ≤ 1 块」。块映射 + 块内插值才有这个保证。
//!
//! # 本模块的测试策略
//!
//! 所有换算都是**纯函数**（不碰 egui），故可单元测试；
//! 只有 [`apply`] 需要 `EditorApp`，它本身不含判定逻辑。

use crate::app::frame::EditorApp;
use crate::application::markdown::queries::{MapScrollQuery, ScrollSource, ScrollTarget};
use crate::presentation::state::scroll_sync_state::{BlockRect, PaneGeometry, Side};
use crate::presentation::theme::mapper;

/// 目标点与视口顶部之间留出的余量（点）。
///
/// 贴到 0 会让目标块紧贴视口上沿，视觉上「顶到头」；
/// 留一点余量既好看，也让上一块的末尾露出来，便于确认上下文。
pub const TARGET_MARGIN: f32 = 8.0;

/// 「把某个偏移滚进视口」的两种意图（问题 066）。
///
/// 查找「下一个」与大纲 / 搜索跳转对「跳过去之后要做什么」的理解
/// **不一样**，硬套同一策略必坏一头：
///
/// - [`Self::IntoViewport`] 连续触发（连按 F3），每次都把命中顶到视口
///   上部会让画面剧烈跳动，故「已在视口内就不动」；
/// - [`Self::Top`] 是离散跳转，用户的期望就是「目标出现在我眼前最
///   上方」——套用「已可见就不动」会让点击看起来毫无反应，套用
///   「落 1/4 处」则与标题顶部差四分之一屏。
///
/// 落点与「预览区点击 → 编辑区」的 [`drive_editor`] 对齐（同为
/// `行顶 − TARGET_MARGIN`）：两条跳转路径的手感必须一致，否则用户
/// 会发现「点预览跳得准、点大纲对不齐」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RevealIntent {
    /// 一律滚到该行顶部（贴视口顶，留 [`TARGET_MARGIN`]）。
    Top,
    /// 已在视口内则不动；否则落到视口上部 1/4 处（查找防抖）。
    IntoViewport,
}

/// 命中判定时从视口顶部再往下探的距离（点）。
///
/// 块与块之间有 4px 间隔（预览区 `add_space(4.0)`）；
/// 不偏移的话，视口顶恰好落在间隔里会命中**上一块**，
/// 表现为「滚过去了一点，编辑区却还停在上一段」。
pub const HIT_BIAS: f32 = 2.0;

/// 把滚动偏移夹到合法区间。
///
/// 非有限值（NaN / ∞）一律归零：`ScrollArea` 的偏移一旦被写成 NaN，
/// 会被持久化进 egui 的状态，此后**再也回不到正常值**（夹紧对 NaN 无效）。
pub fn clamp_scroll(target: f32, geom: PaneGeometry) -> f32 {
    if !target.is_finite() {
        return 0.0;
    }
    target.clamp(0.0, geom.max_offset())
}

/// 「块 ID + 块内比例」→ 预览区滚动偏移。
///
/// 块内按比例插值：编辑区在一块内部往下滚时，预览区也线性跟随，
/// 而不是「进了这块就吸顶、块内不动」——后者在长段落上会显得卡顿。
///
/// 块 ID 不在当前矩形表里时返回 `None`（文档刚重解析，矩形还没采到）。
pub fn offset_for_block(
    rects: &[BlockRect],
    id: u64,
    ratio: f32,
    geom: PaneGeometry,
    margin: f32,
) -> Option<f32> {
    let rect = rects.iter().find(|r| r.id == id)?;
    let ratio = if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let y = rect.top + rect.height() * ratio;
    Some(clamp_scroll(y - margin, geom))
}

/// 预览区某个偏移处命中的块（自上而下第一个下边界超过该偏移的块）。
///
/// 用「下边界 > offset」而不是「区间包含」：块与块之间有空隙，
/// 落在空隙里时应算作**下一块**（视口顶已经越过上一块的底了）。
pub fn block_at_offset(rects: &[BlockRect], offset: f32) -> Option<u64> {
    rects.iter().find(|r| r.bottom > offset).map(|r| r.id)
}

/// 每行起始的字节偏移。
///
/// 末元素是文本长度哨兵，保证「行号 = 行数」时仍能安全取到偏移
/// （滚动到底部时反查出的行号就等于行数；没有哨兵会越界，
/// 而 `panic = "abort"` 下越界等于直接终止进程）。
///
/// **哨兵只在需要时补**。文本以 `\n` 结尾时，`split('\n')` 拆出的
/// 末行本身就是空行、其行首恰好等于 `text.len()`，此时再补一个就会
/// 出现重复元素，而 [`line_of_offset`] 用的是 `partition_point`：
///
/// ```text
///   starts = [0, 3, 6, 6]   offset = 6 → 命中 4 个 → 行号 3
///   starts = [0, 3, 6]      offset = 6 → 命中 3 个 → 行号 2  ✅
/// ```
///
/// 三行文档算出「第 4 行」，跳转与滚动同步都会落到不存在的行上。
pub fn line_start_offsets(text: &str) -> Vec<usize> {
    LineIndex::new(text).into_starts()
}

/// 行索引：**一遍扫描**同时得到「行起点表」与「行数」（问题 053）。
///
/// # 为什么合并成一个类型
///
/// 状态栏一帧里要三样东西：光标的行列、选区的行列、右端的「N 行」；编辑区还要
/// 一次行数（`desired_rows` 与行号栏宽度）。旧写法各调各的——`line_start_offsets`
/// 一帧最多两次、`line_count` 三次，1MB 上实测每帧**恒定 1.1ms**（见
/// `docs/metrics.md` §3.2.5 的 S3 0.82 + S4 0.28），而它们要的是**同一份信息**。
/// 合并后由 `EditorApp` 每帧只构建一次并缓存（文本不变时为零成本）。
///
/// # 口径与两个旧函数逐例相等
///
/// `split('\n')` 产出 `k + 1` 段（`k` = `\n` 的个数），每段一个行首——所以
/// **不含哨兵**的起点表长度**恰好等于行数**，不必额外存两个计数：
///
/// - [`Self::starts`] 与 [`line_start_offsets`] 逐元素相等（含文末哨兵）；
/// - [`Self::count`] 与 `shared::text::line_count` 逐例相等；
/// - [`Self::status_starts`] 是不含哨兵的前半段，即状态栏口径
///   （哨兵为何会让状态栏「多出一行」，见 `EditorApp::cursor_line_col` 的注释）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineIndex {
    /// 行起点表；含可能的文末哨兵，语义与 [`line_start_offsets`] 一致。
    starts: Vec<usize>,
    /// `shared::text::line_count` 口径的行数（恒 ≥ 1）。
    count: usize,
}

impl LineIndex {
    /// 一遍扫描文本，同时填行起点表与计数。
    ///
    /// 容量按「平均行长 32 字节」估：估算只影响**是否多一次翻倍**（不影响正确性），
    /// 而换成「先 `line_count` 定容再拆一遍」就是**两遍**全文扫描——正是本次要消掉的
    /// 东西。典型 Markdown 正文行远长于 32 字节，故绝大多数文档一次到位。
    pub fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut starts = Vec::with_capacity(bytes.len() / 32 + 2);
        starts.push(0);
        let mut count = 1usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
                count += 1;
            }
        }
        // 末行起点已经等于文本长度时**不重复补**哨兵（见 `line_start_offsets`）。
        if starts.last() != Some(&bytes.len()) {
            starts.push(bytes.len());
        }
        Self { starts, count }
    }

    /// 行数（`split('\n').count()` 口径，恒 ≥ 1）。
    pub const fn count(&self) -> usize {
        self.count
    }

    /// 行起点表（含文末哨兵）——与 [`line_start_offsets`] 的返回值逐元素相等。
    pub fn starts(&self) -> &[usize] {
        &self.starts
    }

    /// 取走行起点表（供 [`line_start_offsets`] 这类只想要表的调用方，零拷贝）。
    pub fn into_starts(self) -> Vec<usize> {
        self.starts
    }

    /// 状态栏口径的行起点表：**摘掉文末哨兵**。
    ///
    /// `line_start_offsets` 在文本不以 `\n` 结尾时会补一个长度哨兵供 y 反查用；
    /// 状态栏不能要它——光标停在单行文档行尾时会被算成「行 2 列 1」，凭空多一行。
    /// 不含哨兵的部分长度恰好等于 [`Self::count`]，故直接按计数截断。
    pub fn status_starts(&self) -> &[usize] {
        &self.starts[..self.count]
    }

    /// 由字节偏移反查所在行号（语义同 [`line_of_offset`]）。
    pub fn line_of_offset(&self, offset: usize) -> usize {
        line_of_offset(self.status_starts(), offset)
    }
}

/// 由 y（点）反查所在行号。
///
/// `tops` 末元素是总高，故结果可能等于行数——调用方用它取
/// [`line_start_offsets`] 的哨兵（文本长度），不会越界。
pub fn line_at_y(tops: &[f32], y: f32) -> usize {
    if tops.is_empty() {
        return 0;
    }
    let idx = tops.partition_point(|t| *t <= y);
    idx.saturating_sub(1).min(tops.len() - 1)
}

/// 由字节偏移反查所在行号（语义同 [`line_at_y`]）。
pub fn line_of_offset(starts: &[usize], offset: usize) -> usize {
    if starts.is_empty() {
        return 0;
    }
    let idx = starts.partition_point(|s| *s <= offset);
    idx.saturating_sub(1).min(starts.len() - 1)
}

// ============================================================================
// 驱动
// ============================================================================

/// 每帧在**两侧都渲染完之后**调用一次，完成一次同步。
///
/// 放在两侧之后是因为：同步要读两侧**本帧真实**的偏移，
/// 而 `ScrollArea` 的偏移只有在它的 `show` 结束后才确定
/// （用户滚轮的增量是在 `show` 内部才叠加的）。
pub fn apply(app: &mut EditorApp, now_ms: u64) {
    // 无论是否同步都要观测：`last` 一旦停更，下次开启同步时
    // 累积的位移会被当成一次巨大的用户手势，预览会猛跳一下。
    let driver = app
        .ui
        .sync
        .observe(now_ms, app.editor.scroll_y, app.preview.scroll_y);

    // 点击跳转**独立于**「连续滚动同步」开关（见 S6-05 #12）：即便用户关掉同步，
    // 点目录 / 标题 / 脚注仍应跳到编辑区对应行。故先取点击、先行处理，
    // 再判定连续同步是否启用——原有的顺序会在关闭同步时把点击吞掉，
    // 表现为「点了大纲 / 目录没反应」。
    if let Some(block_id) = app.ui.sync.take_click() {
        if drive_editor(app, Some(block_id)) {
            app.ui.sync.record_drive(Side::Preview);
        }
        return;
    }

    // 预览关掉 / 专注模式 / 大文件降级时没有可同步的对侧。
    let enabled = app.ui.sync.enabled
        && app.ui.preview_visible
        && !app.ui.focus_mode
        && !app.preview.degraded;
    if !enabled {
        return;
    }

    let Some(side) = driver else {
        return;
    };
    let done = match side {
        Side::Editor => drive_preview(app),
        Side::Preview => drive_editor(app, None),
    };
    if done {
        app.ui.sync.record_drive(side);
    }
}

/// 编辑区 → 预览区。
fn drive_preview(app: &mut EditorApp) -> bool {
    let Some(doc_id) = app.active_doc else {
        return false;
    };
    let theme = app.theme().clone();
    let Some(tops) = mapper::line_tops(&theme, &app.editor_text) else {
        return false;
    };
    let starts = line_start_offsets(&app.editor_text);
    let line = line_at_y(&tops, app.editor.scroll_y);
    let offset = starts.get(line).copied().unwrap_or(0);

    let Ok(ScrollTarget::Preview { block_id, ratio }) =
        app.services.markdown.map_scroll(MapScrollQuery {
            doc_id,
            source: ScrollSource::Editor { offset },
        })
    else {
        return false;
    };

    let geom = app.ui.sync.geometry(Side::Preview);
    let Some(target) = offset_for_block(&app.preview.rects, block_id, ratio, geom, TARGET_MARGIN)
    else {
        return false;
    };
    app.preview.scroll_y = target;
    true
}

/// 预览区 → 编辑区。
///
/// `block_id` 为 `Some` 时来自**点击**（标题），为 `None` 时取视口顶部命中的块。
fn drive_editor(app: &mut EditorApp, block_id: Option<u64>) -> bool {
    let Some(doc_id) = app.active_doc else {
        return false;
    };
    let id = match block_id {
        Some(id) => id,
        None => match block_at_offset(&app.preview.rects, app.preview.scroll_y + HIT_BIAS) {
            Some(id) => id,
            None => return false,
        },
    };

    let Ok(ScrollTarget::Editor { offset }) = app.services.markdown.map_scroll(MapScrollQuery {
        doc_id,
        source: ScrollSource::Preview { block_id: id },
    }) else {
        return false;
    };

    let theme = app.theme().clone();
    let Some(tops) = mapper::line_tops(&theme, &app.editor_text) else {
        return false;
    };
    let starts = line_start_offsets(&app.editor_text);
    let line = line_of_offset(&starts, offset);
    // Galley 坐标 → 内容坐标：加每帧实测的文本顶端偏移（TextEdit atom
    // frame 的竖向内边距，见 [`ScrollSync::set_editor_text_top`]）。
    // 不加这一项，预览点击跳转会系统性偏高该偏移（实测 4pt）——
    // 与大纲跳转（问题 066）同源，两条路径必须一起修。
    let y = tops.get(line).copied().unwrap_or(0.0) + app.ui.sync.editor_text_top();

    let geom = app.ui.sync.geometry(Side::Editor);
    app.set_editor_scroll_y(clamp_scroll(y - TARGET_MARGIN, geom));
    true
}
