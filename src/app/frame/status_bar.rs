//! 状态栏（`StatusBarUiState`）的写入口。
//!
//! # 为什么单独成文件
//!
//! R-11 子批 3 把 `status_bar_view` 对 `app.status_bar.*` 的直写收敛为方法调用，
//! 与 [`super::editor`] / [`super::sidebar`] / [`super::ui`] 同构：表现层只读
//! 状态、只提意图，写操作一律走这里。

use super::*;

impl EditorApp {
    /// 状态栏显示的文档编码。
    ///
    /// 每帧由 `status_bar_view::sync_encoding` 从「当前激活文档的真实编码」旁路
    /// 算出（一次 `HashMap` 查询，每帧可忽略），写回应用状态供状态栏渲染——
    /// 否则状态栏拿不到领域层才知道的编码。
    pub fn set_status_bar_encoding(&mut self, encoding: Option<TextEncoding>) {
        self.status_bar.encoding = encoding;
    }
}
