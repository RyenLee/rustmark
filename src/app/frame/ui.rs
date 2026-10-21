//! 应用 UI 状态（`AppUiState`）的写入口。
//!
//! # 为什么单独成文件
//!
//! R-11 子批 3 把 `main_view` / `toolbar_view` 对 `app.ui.*` 的直写收敛为方法调用，
//! 与 [`super::editor`] / [`super::sidebar`] / [`super::preview`] 同构：表现层只读
//! 状态、只提意图，写操作一律走这里。
//!
//! # 只做「直写 → 方法调用」这一件事
//!
//! 这些都是**单纯的写入口**（拖拽中标志 / 标签条最后激活项 / 横向滚动偏移）。
//! 「基于旧值算新值」的演化（如切换类）不属于这里——本组四个字段都是外部
//! 算好直接给的值，视图只是把结果回写，故做成 setter 即可。

use super::*;

impl EditorApp {
    /// 分割线拖拽中标志。
    ///
    /// 每帧由 `main_view` 的拖拽手势跳变写一次：拖拽开始置 `true`、松手置
    /// `false`。值只在跳变那帧写，避免每帧无谓写回（见 `AppUiState` 注释）。
    pub fn set_split_dragging(&mut self, dragging: bool) {
        self.ui.split_dragging = dragging;
    }

    /// 侧栏宽度拖拽中标志。
    ///
    /// 与 `split_dragging` 对称但目的相反：那里用于「松手即落盘」，这里用于
    /// 「松手那帧宽度 ≠ 基准、自然不落盘」的门控（见 `tabs.rs` 的
    /// `persist_sidebar_width`）。同样是「只写跳变帧」。
    pub fn set_sidebar_dragging(&mut self, dragging: bool) {
        self.ui.sidebar_dragging = dragging;
    }

    /// 标签条最后激活文档。
    ///
    /// `main_view` 用它检测「激活项变化」以触发横向滚动居中（问题 042：
    /// 「◀ 能滚、▶ 点了没反应」）——所以存在一个应用状态里，而非每次重算。
    pub fn set_tabstrip_last_active(&mut self, id: Option<u64>) {
        self.ui.tabstrip_last_active = id;
    }

    /// 标签条横向滚动权威偏移。
    ///
    /// 每帧末尾写回；下一帧由 `scroll_offset` 构造器把它喂回滚动区（egui 0.37
    /// 的 `offset` 字段已私有，不再直写 `State`）。
    pub fn set_tabstrip_scroll_x(&mut self, x: f32) {
        self.ui.tabstrip_scroll_x = x;
    }
}
