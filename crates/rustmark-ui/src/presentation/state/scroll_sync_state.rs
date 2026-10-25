//! 双向滚动同步的**状态**（阶段 06 · S6-06）。
//!
//! # 为什么需要「主动方」标记
//!
//! 两个 `ScrollArea` 互相驱动会形成反馈环：
//!
//! ```text
//!   用户滚编辑区 → 写预览区偏移 → 预览区「动了」→ 写编辑区偏移
//!              ↑                                        │
//!              └────────────────────────────────────────┘
//! ```
//!
//! 结果是两侧互相拉扯，表现为松手后界面还在抖动（阶段文档 §5 标记为「中风险」）。
//! 根治办法是**单一主动方**：某一时刻只有一侧有权驱动对侧，
//! 对侧的位移在抑制窗口内一律视为回声。
//!
//! # 为什么用「时间窗口」而不是「比对写入值」
//!
//! 更直觉的做法是「记住我刚写进去的值，位移等于它就判为回声」。
//! 但目标偏移会被 `ScrollArea` 夹紧（滚到底部时写 9999 实际落在 812），
//! 于是回声与写入值不等 → 被误判成用户手势 → 立刻反向驱动 → 抖动复现。
//! 时间窗口对夹紧天然免疫，故采用之。
//!
//! # 职责边界
//!
//! 本模块只有**数据**与状态转移，不含任何像素换算——
//! 几何映射见 root crate 的 `crate::presentation::views::scroll_sync`
//! （core 不反向依赖 root，仅文字指路不成链接），
//! 这样两侧都能被独立单元测试。

/// 抑制窗口（毫秒）。
///
/// 100ms 是阶段文档 §5 风险表给出的取值：短于一次连续滚动的帧间隔（~16ms）
/// 就无法覆盖回声，长于约 150ms 则会让用户「滚完编辑区立刻去滚预览」时
/// 明显感到前 0.1 秒没反应。
pub const SUPPRESS_MS: u64 = 100;

/// 判定「偏移真的变了」的阈值（点）。
///
/// 不能取 0：`ScrollArea` 会把偏移四舍五入到整像素，静止时也会在
/// ±0.5 内抖动；取 0 会把静止帧误判为「用户在滚」。
pub(crate) const MOVE_EPS: f32 = rustmark_core::shared::constants::SCROLL_MOVE_EPS;

/// 滚动同步的两侧。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// 编辑区（`TextEdit` 所在的 `ScrollArea`）。
    Editor,
    /// 预览区。
    Preview,
}

impl Side {
    /// 对侧。
    pub const fn other(self) -> Self {
        match self {
            Self::Editor => Self::Preview,
            Self::Preview => Self::Editor,
        }
    }

    /// 数组下标（内部用）。
    pub const fn index(self) -> usize {
        match self {
            Self::Editor => 0,
            Self::Preview => 1,
        }
    }
}

/// 一侧的几何量（点）。
///
/// `content_h < viewport_h` 时无需滚动，夹紧结果恒为 0。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaneGeometry {
    /// 滚动内容总高。
    pub content_h: f32,
    /// 视口可视高度。
    pub viewport_h: f32,
}

impl PaneGeometry {
    /// 可滚动的最大偏移（内容不足一屏时为 0）。
    pub fn max_offset(&self) -> f32 {
        (self.content_h - self.viewport_h).max(0.0)
    }
}

/// 预览区一个渲染块占据的纵向区间（点，相对**滚动内容顶部**）。
///
/// 由 root crate 的 `crate::presentation::views::preview_view`（文字指路不成链接）
/// 每帧重新采集：
/// 立即模式下块的 y 会随宽度、字号、图片解码结果变化，
/// 缓存跨帧的矩形比不缓存更危险（同步会跳到错误的块）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlockRect {
    /// 块 ID（与 `BlockView::id` 同源）。
    pub id: u64,
    /// 上边界。
    pub top: f32,
    /// 下边界。
    pub bottom: f32,
}

impl BlockRect {
    /// 块高（夹紧为非负）。
    pub fn height(&self) -> f32 {
        (self.bottom - self.top).max(0.0)
    }
}

/// 双向滚动同步状态机。
#[derive(Clone, Debug)]
pub struct ScrollSync {
    /// 当前主动方；`None` 表示尚无归属（首帧 / 重置后）。
    owner: Option<Side>,
    /// 主动方的有效期截止时刻（毫秒）。
    owner_until_ms: u64,
    /// 上一帧观测到的两侧偏移（`None` 表示尚未初始化）。
    last: [Option<f32>; 2],
    /// 两侧几何量。
    geom: [PaneGeometry; 2],
    /// 编辑区文本 Galley 顶端相对**滚动内容原点**的偏移（点）。
    ///
    /// `line_tops` 是 Galley 坐标系（第 0 行顶 = y 0），而滚动换算
    /// （`scroll_to_offset` / `drive_editor`）要的是内容坐标系——两者
    /// 差一个常量：egui 的 `TextEdit` atom frame 默认把
    /// `spacing.button_padding` 当内边距（见 egui
    /// `theme/default_style.rs`；本项目 `mapper` 将其设为
    /// `(space_md, space_xs)`），Galley 于是不从内容原点起画。
    ///
    /// 由编辑区**每帧实测**（`galley_pos.y - 内容原点`）后写入，故：
    /// ① 不被主题间距的具体数值绑死；② 换 egui 版本若改了 frame
    /// 语义，这里自动跟上；③ 不把「TextEdit 有内边距」这个 UI 结构
    /// 细节泄露进滚动数学。缺省 0 = 尚未测量（首帧），此时滚动换算
    /// 退化为不加补偿——与修复前完全一致，不会更坏。
    editor_text_top: f32,
    /// 是否启用同步。
    pub enabled: bool,
    /// 预览区点击某块后待执行的跳转（`Some(block_id)`）。
    pending_click: Option<u64>,
    /// 已执行的同步次数（调试面板）。
    pub drive_count: u64,
    /// 最近一次实际驱动的方向（调试面板）。
    pub last_driven: Option<Side>,
}

impl Default for ScrollSync {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollSync {
    /// 构造默认状态（启用同步）。
    pub fn new() -> Self {
        Self {
            owner: None,
            owner_until_ms: 0,
            last: [None, None],
            geom: [PaneGeometry::default(), PaneGeometry::default()],
            editor_text_top: 0.0,
            enabled: true,
            pending_click: None,
            drive_count: 0,
            last_driven: None,
        }
    }

    /// 换文档 / 关预览后重置。
    ///
    /// 必须清掉 `last`：换文档后两侧偏移都归零，若不重置，
    /// 这一帧会被判成「某一侧发生位移」并立刻驱动对侧，
    /// 表现为「打开新文件预览自己滚了一下」。
    pub fn reset(&mut self) {
        self.owner = None;
        self.owner_until_ms = 0;
        self.last = [None, None];
        self.pending_click = None;
        self.geom = [PaneGeometry::default(), PaneGeometry::default()];
    }

    /// 记录一侧的几何量（由视图每帧写入）。
    pub fn set_geometry(&mut self, side: Side, content_h: f32, viewport_h: f32) {
        self.geom[side.index()] = PaneGeometry {
            content_h,
            viewport_h,
        };
    }

    /// 取一侧的几何量。
    pub fn geometry(&self, side: Side) -> PaneGeometry {
        self.geom[side.index()]
    }

    /// 记录编辑区文本 Galley 顶端相对滚动内容原点的偏移（每帧由编辑区实测）。
    ///
    /// 为什么值得单独一个字段而不是塞进 [`PaneGeometry`]：它是**编辑器
    /// 特有**的排版常量（预览区没有「Galley 之外再套一层 frame」的问题），
    /// 且 [`Self::reset`] 刻意**不清**它——换文档不改 frame 内边距，
    /// 而清了它会让「reset 之后、下一帧编辑区渲染之前」发生的跳转
    /// 丢掉补偿。
    pub fn set_editor_text_top(&mut self, offset: f32) {
        if offset.is_finite() {
            self.editor_text_top = offset;
        }
    }

    /// 取编辑区文本顶端偏移（`line_tops` → 内容坐标系的加数）。
    pub fn editor_text_top(&self) -> f32 {
        self.editor_text_top
    }

    /// 预览区点击了某个块，请求编辑区跳转。
    ///
    /// 点击是**离散事件**，不走连续滚动的抑制窗口——
    /// 用户点一下就该跳，等 100ms 反而像卡顿。
    pub fn request_click(&mut self, block_id: u64) {
        self.pending_click = Some(block_id);
    }

    /// 取出待执行的点击跳转。
    pub fn take_click(&mut self) -> Option<u64> {
        self.pending_click.take()
    }

    /// 当前主动方（窗口已过期则返回 `None`）。
    pub fn owner(&self, now_ms: u64) -> Option<Side> {
        if self.owner_until_ms > now_ms {
            self.owner
        } else {
            None
        }
    }

    /// 该侧的位移此刻是否会被当作回声而忽略。
    ///
    /// 单独暴露出来是为了可测：抑制窗口是防抖动的唯一机制，
    /// 不写测试就等于没有（它坏掉的表现只是「偶尔抖一下」，极难复现）。
    pub fn is_suppressed(&self, now_ms: u64, side: Side) -> bool {
        matches!(self.owner(now_ms), Some(owner) if owner != side)
    }

    /// 本帧观测两侧偏移，返回**有权驱动对侧**的一侧（若有）。
    ///
    /// 判定顺序：先认「谁动了」，再用抑制窗口过滤回声。
    /// 两侧同时位移时的处理见代码注释：连续滚动下这是**常态**而非异常。
    pub fn observe(&mut self, now_ms: u64, editor: f32, preview: f32) -> Option<Side> {
        let e_moved = self.moved(Side::Editor, editor);
        let p_moved = self.moved(Side::Preview, preview);
        self.last[Side::Editor.index()] = Some(editor);
        self.last[Side::Preview.index()] = Some(preview);

        match (e_moved, p_moved) {
            (true, false) => {
                // 与下方 (false, true) 分支对称：编辑区的位移也可能是
                // 「程序驱动编辑区」的回声（用户正在滚预览、上一帧写进
                // 编辑区的目标偏移这一帧落地）。不抑制的话，回声会被当成
                // 用户手势反向驱动预览区——预览被拉回映射位置，其底部
                // （超出编辑区映射范围的部分）永远滚不到。
                if self.is_suppressed(now_ms, Side::Editor) {
                    None
                } else {
                    self.claim(Side::Editor, now_ms);
                    Some(Side::Editor)
                }
            }
            (false, true) => {
                if self.is_suppressed(now_ms, Side::Preview) {
                    None
                } else {
                    self.claim(Side::Preview, now_ms);
                    Some(Side::Preview)
                }
            }
            (true, true) => {
                // 两侧都有位移。真实场景里这**必然**发生：
                // 连续滚动时，上一帧写进给对侧的回声正好落在这一帧，
                // 于是「用户在滚 + 程序在写」同时出现。此时必须按主动方继续走
                // （对侧的位移是回声），否则同步会退化成隔帧才生效。
                match self.owner(now_ms) {
                    Some(Side::Editor) => {
                        self.claim(Side::Editor, now_ms);
                        Some(Side::Editor)
                    }
                    Some(Side::Preview) => {
                        self.claim(Side::Preview, now_ms);
                        Some(Side::Preview)
                    }
                    // 无归属时两侧同时在动（异常）→ 不动，避免互相打架。
                    None => None,
                }
            }
            (false, false) => None,
        }
    }

    /// 声明「该侧本帧的位移是**程序性**的，不是用户手势」。
    ///
    /// # 为什么需要它
    ///
    /// egui 的 `TextEdit` 一旦自行改了文本，就会调 `ui.scroll_to_rect` 把光标
    /// 滚进视口——这个滚动**带动画**、横跨好几帧（见 `egui` 的
    /// `text_edit::builder`：`if response.changed() { ui.scroll_to_rect(...) }`）。
    /// 它是程序为了「让光标可见」而滚的，用户并没有滚。
    ///
    /// 若不声明，滚动同步会把它判成「用户在滚编辑区」，于是**一帧一次**地把
    /// 预览区拉到映射位置：编辑区在平滑滚动，预览区却逐帧跳变，两栏同时在动
    /// ——观感就是整个界面在闪烁。光标即将滚出视口的那一帧按回车最容易触发，
    /// 而「在哪一行会滚出视口」取决于窗口高度与滚动位置，故用户感受是
    /// 「随机在某一行按回车会闪」。
    ///
    /// # 做法
    ///
    /// 与回声抑制同源：把**对侧**宣告为主动方，本侧位移在抑制窗口内一律视为
    /// 回声（`is_suppressed` 为真 → `observe` 返回 `None`）。
    pub fn note_programmatic_scroll(&mut self, side: Side, now_ms: u64) {
        self.claim(side.other(), now_ms);
    }

    /// 记一次已执行的驱动（调试面板）。
    pub fn record_drive(&mut self, side: Side) {
        self.drive_count += 1;
        self.last_driven = Some(side);
    }

    /// 该侧本帧是否发生了位移。
    fn moved(&self, side: Side, value: f32) -> bool {
        match self.last[side.index()] {
            // 首帧只做初始化，不产生驱动。
            None => false,
            Some(prev) => (value - prev).abs() > MOVE_EPS && value.is_finite(),
        }
    }

    /// 宣告主动方并刷新抑制窗口。
    fn claim(&mut self, side: Side, now_ms: u64) {
        self.owner = Some(side);
        self.owner_until_ms = now_ms.saturating_add(SUPPRESS_MS);
    }
}
