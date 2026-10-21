//! 状态栏（`StatusBarUiState`）的写入口。
//!
//! # 为什么单独成文件
//!
//! R-11 子批 3 把 `status_bar_view` 对 `app.status_bar.*` 的直写收敛为方法调用，
//! 与 [`super::editor`] / [`super::sidebar`] / [`super::ui`] 同构：表现层只读
//! 状态、只提意图，写操作一律走这里。
//!
//! R-21 把 `set_status_error` / `set_status_notice` 从 `files.rs` 并入本文件：
//! 二者同样只写 `self.status_bar`，与 `set_status_bar_encoding` 同属
//! 「状态栏写入口」，散在文件操作模块里会让「状态栏怎么写」有两个家。

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

    /// 在状态栏显示一条错误。
    ///
    /// 时钟取自 `services.markdown`（所有应用服务共用同一个 `Clock`），
    /// 不直接调 `SystemClock`：消息的「4 秒后消失」是可测规则，
    /// 用系统时钟就没法写测试验证它。
    pub fn set_status_error(&mut self, text: &str) {
        let now = self.services.markdown.now().millis();
        self.status_bar.error(text, now);
    }

    /// 在状态栏显示一条普通提示。
    pub fn set_status_notice(&mut self, text: impl Into<String>) {
        let now = self.services.markdown.now().millis();
        self.status_bar.notice(text, now);
    }
}
