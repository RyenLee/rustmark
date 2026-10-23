//! 预览区 UI 状态（滚动、缓存键、防抖）。
//!
//! # 缓存键的构成
//!
//! 预览重建代价高（重新排版成 `Galley`），但触发它的事件很多：
//! 切换文档、文档改动、主题切换、字号调整、区块勾选……
//! 若只用「内容版本」做键，改字号不会重建 → 字号设置看起来无效；
//! 若只用「主题」做键，改内容不会重建 → 预览永远停在旧内容；
//! 若漏掉「文档标识」，切 tab 不会重建 → 预览永远停在**第一份**文档
//! （版本号是每份文档各自的计数器，两份未编辑的文档版本号天然相同）。
//! 故本模块把这三者（连同代码块高亮状态）合成一个 [`PreviewCacheKey`]。

use crate::application::dto::BlockView;
use crate::presentation::state::scroll_sync_state::BlockRect;
use crate::presentation::theme::builtin::palette_signature;

/// 防抖时长（毫秒）。
///
/// FR-01 要求 150ms。太短会让打字时每敲一下就重排全文，
/// 表现为「输入一顿一顿」；太长则松手后预览迟迟不更新。
pub const PREVIEW_DEBOUNCE_MS: u64 = 150;

/// 预览缓存键：**换了文档**、内容变了或配色变了都要重建。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PreviewCacheKey {
    /// 源文档标识。
    ///
    /// **必须参与键，且优先级高于版本号**：`content_version` 是
    /// [`Document::revision`](crate::domain::document::aggregate::Document::revision)，
    /// 那是**每份文档各自**的计数器——两份刚打开、都还没编辑过的文档
    /// 版本号天然相同（都是初始值）。若按「版本号相同 ⇒ 内容没变 ⇒ 不用重排」
    /// 推断，切 tab 时预览会**一直显示第一份文档**，
    /// 而真相是「版本号相同但根本不是同一篇文档」。
    /// 文档标识进键后，换文档必然判定为「需重建」。
    pub doc_id: u64,
    /// 源文档版本号（**每份文档各自计数**，不可跨文档比较）。
    pub content_version: u64,
    /// 配色签名（由 `palette_signature` 产生）。
    pub palette: [u8; 4],
    /// 字号（整数化，避免浮点抖动导致每次都判定「变了」）。
    pub font_size_q: u32,
}

impl PreviewCacheKey {
    /// 由文档标识、文档版本与主题构造。
    pub fn new(
        doc_id: u64,
        content_version: u64,
        theme: &crate::presentation::theme::builtin::Theme,
    ) -> Self {
        Self {
            doc_id,
            content_version,
            palette: palette_signature(theme).unwrap_or([0; 4]),
            font_size_q: (theme.metrics.preview_font_size * 100.0).round() as u32,
        }
    }

    /// 是否换了一份文档（需要**立即**重建，不等防抖）。
    pub fn document_changed(&self, other: &Self) -> bool {
        self.doc_id != other.doc_id
    }

    /// 内容是否变化（需要重建）。仅当仍是同一份文档时才有意义。
    pub fn content_changed(&self, other: &Self) -> bool {
        self.content_version != other.content_version
    }

    /// 外观是否变化（需要重建）。
    pub fn appearance_changed(&self, other: &Self) -> bool {
        self.palette != other.palette || self.font_size_q != other.font_size_q
    }
}

/// 预览区 UI 状态。
#[derive(Clone, Debug)]
pub struct PreviewUiState {
    /// 垂直滚动量。
    pub scroll_y: f32,
    /// 上次完成渲染的缓存键。
    pub last_key: Option<PreviewCacheKey>,
    /// 上次渲染所依据的**文档标识**（用于识别「换了文档」）。
    ///
    /// 不复用 `last_key.doc_id` 来判定：`last_key` 会被主题切换等场景
    /// 主动清空（[`PreviewUiState::invalidate`]），那一刻若靠它判断
    /// 「是否换文档」就会漏判，于是上一份文档的滚动偏移会被带进新文档。
    pub last_doc: Option<u64>,
    /// 上次编辑时刻（用于防抖）。
    pub last_edit_at_ms: u64,
    /// 最近一次渲染出的块序列（S6-05：预览重排代价高，缓存到键失效为止）。
    ///
    /// 存的是 DTO 视图而非渲染产物（`Galley`）——egui 是立即模式，
    /// 每帧都要重新下发绘制指令，`Galley` 由 `egui` 自己的字体缓存负责。
    pub blocks: Vec<BlockView>,
    /// 每个块本帧占据的纵向区间（S6-06 滚动同步的换算依据）。
    ///
    /// **每帧重新采集，不跨帧复用**：立即模式下块的位置会随面板宽度、
    /// 字号、图片解码结果变化，用上一帧的矩形去换算下一帧的偏移，
    /// 在大图片刚解码完成的那一帧会直接跳错块。
    pub rects: Vec<BlockRect>,
    /// 预览重新解析次数（调试面板观察 FR-01 的防抖是否生效）。
    pub render_count: u64,
    /// 因文档过大而降级（FR-06）。
    pub degraded: bool,
}

impl Default for PreviewUiState {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewUiState {
    /// 取出块序列（D-3 阶段 3：替代视图里的 `std::mem::take(&mut …)` 直取，
    /// 字段的可变引用不出 state）。零拷贝；渲染结束后由
    /// `EditorApp::store_preview_render` 放回——取出与放回是对称的。
    pub fn take_blocks(&mut self) -> Vec<BlockView> {
        std::mem::take(&mut self.blocks)
    }

    /// 构造默认状态。
    pub fn new() -> Self {
        Self {
            scroll_y: 0.0,
            last_key: None,
            last_doc: None,
            last_edit_at_ms: 0,
            blocks: Vec::new(),
            rects: Vec::new(),
            render_count: 0,
            degraded: false,
        }
    }

    /// 告知预览区「本帧要渲染的是哪份文档」；若与上次不是同一份，则重置。
    ///
    /// 返回是否发生了文档切换。
    ///
    /// # 为什么要有这个「唯一入口」
    ///
    /// `active_doc` 在 `EditorApp` 里有 7 处赋值（tab 点击、快捷键轮换、
    /// 关闭标签后改选、新建、打开文件……），其中只有 3 处走 `switch_document`。
    /// 逐个补 `reset_for_new_document()` 必然漏——事实上原先**一个都没有调用**，
    /// 这正是「切 tab 后预览仍是第一份文档」的直接成因。
    /// 改成「拿本次要渲染的文档标识与上次比一比」，就与切换动作从哪来无关了。
    pub fn sync_document(&mut self, doc_id: u64) -> bool {
        if self.last_doc == Some(doc_id) {
            return false;
        }
        self.last_doc = Some(doc_id);
        self.reset_for_new_document();
        true
    }

    /// 是否该重新渲染。
    ///
    /// 四个条件任一满足即需要：换了文档、外观变了、内容变了（过了防抖）、强制刷新。
    ///
    /// # 为什么「换文档」不走防抖
    ///
    /// 防抖是为了合并**连续击键**（同一份文档里 150ms 内的多次改动只需重排一次）。
    /// 切换 tab 不是击键而是**显式导航**：用户点下 tab 的那一刻就期望看到那份文档。
    /// 若也压进防抖窗口，预览区会继续显示上一份文档最多 150ms——
    /// 观感正是「预览没跟着 tab 走」，也正是本用例要防的回归。
    pub fn should_render(&self, key: PreviewCacheKey, now_ms: u64, force: bool) -> bool {
        if force {
            return true;
        }
        match self.last_key {
            None => true,
            Some(last) => {
                // 换文档立即生效——版本号不可跨文档比较（见 `PreviewCacheKey::doc_id`）。
                if last.document_changed(&key) {
                    return true;
                }
                // 外观变化立即生效（不等防抖）——否则切主题会有 150ms 的延迟，
                // 表现为「点了主题按钮，界面顿一下才变」。
                let appearance = last.appearance_changed(&key);
                if appearance {
                    return true;
                }
                if last.content_changed(&key) {
                    // 内容变化需等防抖窗口。
                    return now_ms.saturating_sub(self.last_edit_at_ms) >= PREVIEW_DEBOUNCE_MS;
                }
                false
            }
        }
    }

    /// 记录一次**编辑**发生的时刻（防抖的计时起点）。
    ///
    /// 必须在**编辑**时打戳，防抖才是防抖：`should_render` 判的是
    /// 「距上次编辑过了多久」。若只在渲染完成时打戳（原实现），计时起点会随
    /// 每次渲染被刷新，持续输入时便退化成「每 150 ms 重解析一次」的**节流**——
    /// 实测连续输入 20 字符（1.2 s）重解析 **7 次**，远超验收上限 2 次
    /// （`tests/integration/preview_debounce_test.rs`）。
    pub fn mark_edited(&mut self, now_ms: u64) {
        self.last_edit_at_ms = now_ms;
    }

    /// 记录一次渲染完成。
    pub fn record_render(&mut self, key: PreviewCacheKey, now_ms: u64) {
        self.last_key = Some(key);
        self.last_edit_at_ms = now_ms;
        self.render_count += 1;
    }

    /// 文档切换后重置（缓存键必失效）。
    pub fn reset_for_new_document(&mut self) {
        self.last_key = None;
        self.scroll_y = 0.0;
        self.blocks.clear();
        self.rects.clear();
    }

    /// 主题切换后重置（强制下次重建）。
    pub fn invalidate(&mut self) {
        self.last_key = None;
    }

    /// 文档过大降级（FR-06：> 500KB 只渲染可视区）。
    pub fn set_degraded(&mut self, degraded: bool) {
        self.degraded = degraded;
    }
}
