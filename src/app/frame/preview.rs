//! 预览区的状态写入口。
//!
//! # 为什么单独成文件
//!
//! R-11 子批 3 把 `preview_view` / `scroll_sync` 对 `app.preview.*` 的直写
//! 收敛为方法调用，与 [`super::export`] / [`super::diff`] / [`super::sidebar`]
//! 同构：表现层只读状态、只提意图，写操作一律走这里。
//!
//! # 只做「直写 → 方法调用」这一件事
//!
//! `PreviewUiState` 自身已有的行为（`sync_document` / `should_render` /
//! `record_render` / `set_degraded` …）继续由视图直接调用——那些是状态
//! 结构体自己的方法，不是「视图直写字段」，不在 R-11 范围内。
//!
//! 另有一个**刻意不收**的形态：`preview_view.rs` 渲染前的
//! `std::mem::take(&mut app.preview.blocks)`。那是**可变借用**而非赋值
//! （取走 blocks 以避开闭包内 `&mut app` 的重 Borrow 冲突，渲染结束再由
//! [`EditorApp::store_preview_render`] 放回），与 `TextEdit(&mut String)`
//! 同属「控件/借用机制要求的形态」，R-11 只收口赋值。

use super::*;

use crate::presentation::state::scroll_sync_state::BlockRect;
use rustmark_core::application::dto::BlockView;

impl EditorApp {
    /// 写入一次预览渲染的块序列（缓存未命中、`render_preview` 返回时）。
    ///
    /// 与 [`EditorApp::store_preview_render`] 的分工：这里只写 `blocks`
    /// （`degraded` 由状态结构体自己的 `set_degraded` 另设）；那次是**每帧**
    /// 渲染结束后把 blocks / rects / scroll_y 三样一起落定。
    pub fn set_preview_blocks(&mut self, blocks: Vec<BlockView>) {
        self.preview.blocks = blocks;
    }

    /// 落定一次帧末渲染的产出：块序列放回 + 本帧块矩形 + 滚动偏移。
    ///
    /// 三者在语义上是**同一事件**的三个侧面（这一帧预览画完了），散成三行
    /// 直写会让「漏写 rects」这类缺陷无声发生——滚动同步拿着上一帧的矩形
    /// 换算下一帧的偏移，大图刚解码那一帧就会跳错块。
    pub fn store_preview_render(
        &mut self,
        blocks: Vec<BlockView>,
        rects: Vec<BlockRect>,
        scroll_y: f32,
    ) {
        self.preview.blocks = blocks;
        self.preview.rects = rects;
        self.preview.scroll_y = scroll_y;
    }

    /// 写入预览区垂直滚动量（帧末回写与滚动同步驱动共用一条路径，
    /// 与 `set_editor_scroll_y` 同族）。
    pub fn set_preview_scroll_y(&mut self, y: f32) {
        self.preview.scroll_y = y;
    }
}
