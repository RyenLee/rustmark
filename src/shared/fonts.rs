//! 编辑区等宽字体的**跨层共享契约**（设置页字体选择，S6-06）。
//!
//! # 为什么这个契约只剩「哨兵 + 选项」两个东西
//!
//! 早期设计给编辑区建了一个**专用字体族**（`FontFamily::Name("editor")`），
//! 由基础设施层装配、表现层在 `FontId` 里按名字引用。该方案已废弃，原因是
//! egui 的行为与假设相反：
//!
//! ```text
//! epaint-0.33.3/src/text/fonts.rs:808
//!     .unwrap_or_else(|| panic!("FontFamily::Name({name:?}) is not bound to any fonts"))
//! ```
//!
//! 即「族没注册」**不是**静默回退，而是直接 panic——本项目
//! `panic = "abort"`，一次未注册引用就是窗口消失。只要存在任何
//! 「先排版、后装配字体」的路径（无头测试、将来的新入口），就是致命缺陷。
//! 实测：改用专用族后，6 条既有单测因测试上下文未装字体而 panic。
//!
//! 现行方案：**把用户选中的字体置顶进 egui 内置的 `Monospace` 族**
//! （基础设施层装配）。`FontId::monospace(..)` 永远有绑定，不存在未注册
//! 风险；代价是选择会连带影响所有等宽界面（编辑区、预览代码块、
//! 命令面板、差异对比）——这与 VS Code「编辑器字体」的语义一致，
//! 且比一个随时可能 panic 的自建族可靠得多。
//!
//! 本模块**不引入 egui**（见 `.agent/architecture.md`：`shared` 层禁止依赖
//! `egui`）。这里只提供字符串契约与纯数据。
//!
//! # 与「字体族设置」的来龙去脉
//!
//! 早期 `Theme::editor_font_family` 是个**死设置**：字符串写进了主题，却没有任何
//! `FontId` 消费它（编辑区一直硬编码 `FontId::monospace`）。设置页当时给的是
//! `system-ui` / `sans-serif` 这类 CSS 通用名，跟 egui 的字体族对不上，
//! 选了也不会生效。本模块与配套改造让这个设置真正可用：发现真实可用字体、
//! 置顶进等宽族、下拉里只列能生效的选项。

/// 编辑区等宽字体的**默认哨兵**：表示「用 egui 内置等宽族，不置顶任何字体」。
///
/// 它也是 `ThemeView.font_family` 与 `Theme.editor_font_family` 的默认值。
/// 与 egui 内置的 `Monospace` 族同源——选它就等价于「不加任何自定义字体」。
pub const DEFAULT_EDITOR_FONT_KEY: &str = "monospace";

/// 设置页「字体」下拉里的一个可选项。
///
/// 纯数据、跨层可用：基础设施层负责「发现并登记字体」（产出本结构），
/// 表现层（设置 UI）只消费 `key` / `display_name`，不接触文件系统，
/// 也不依赖 `infrastructure` 层（遵守表现层禁令）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorFontChoice {
    /// 写回偏好设置的取值：要么是 [`DEFAULT_EDITOR_FONT_KEY`] 哨兵，
    /// 要么是 `user:<字体名>` 形式的 egui 字体数据键。
    pub key: String,
    /// 下拉里展示给用户的名字（如「Consolas」「系统等宽（默认）」）。
    pub display_name: String,
}
