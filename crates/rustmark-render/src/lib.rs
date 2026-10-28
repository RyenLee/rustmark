//! 可替换的富内容渲染引擎（数学公式 / Mermaid 图表）。
//!
//! # 为什么它们不在 `views/` 里
//!
//! 这两个模块是**渲染引擎**，不是视图：它们不持有 UI 状态、不读写
//! `EditorApp`，只做「源码 → 版面（纯函数）→ 交给 `Painter` 画」。
//! 放在 `views/` 旁边会模糊这条界线，也会让「换掉整个引擎」看起来
//! 像「改一个视图」。
//!
//! # 替换契约（将来接 KaTeX / Mermaid.js 时看这里）
//!
//! 引擎与消费方（`views::preview_view`）之间只有下面这些接口。替换实现时
//! **只需保持这些签名，视图侧一行不用动**：
//!
//! | 引擎 | 排版（纯函数） | 绘制 | 尺寸 | 错误 |
//! |---|---|---|---|---|
//! | `math` | `layout(&str, size: f32, display: bool, Measure) -> Result<MathLayout, MathError>` | `emit(&MathLayout, Rgb8) -> Vec<RenderCommand>`（绘制交给 `render::apply_egui`） | `fitted_height` | `MathError` |
//! | `mermaid` | `layout(&str, size: f32, Measure) -> Result<MermaidLayout, MermaidError>` | `commands(&MermaidLayout, &MermaidStyle) -> Vec<RenderCommand>`（绘制交给 `render::apply_egui`） | `fitted_height` | `MermaidError` |
//!
//! 两个 `layout` 都**不碰 `egui`**：度量通过
//! `Measure = &dyn Fn(&str, f32) -> f32` 注入，所以「分数比分母高」
//! 「矩阵列宽取最宽项」这类排版规则可以在无头环境断言。
//! 这也意味着替换可以分两步走——先换排版、保留现有绘制，或整体替掉。
//!
//! # 边界由什么保证
//!
//! - 实现模块（`layout` / `model` / `parser` / `graph_parse` / `primitives`…）
//!   一律是私有 `mod`，外部只能看见 facade 里的那几项；
//! - `math` 与 `mermaid` 都已端口化：排版 / 图元结果经 `render` 端口层
//!   （`RenderCommand` / `Rgb8` / `Pt` / `geom::Rect`）描述几何，**不**
//!   `use egui`；唯一允许 `use egui` 的是 `render::egui`（`RenderBackend`
//!   实现与 `apply_egui`）。配色由调用方经 `Rgb8`（`MermaidStyle` / `emit`
//!   的颜色参数）传入，不依赖 `presentation` 的其他模块（不碰主题、不碰
//!   icons、不碰 state）。这条由 `check-arch` 的 render-egui 白名单断言守护。
//!
//! # 现在是独立 crate（R-04 升 crate，2026-10-28）
//!
//! 原 `rustmark-ui::presentation::rich` 于此物理搬出，成为 `rustmark-render`。
//! 设计要点：
//!
//! 1. **默认依赖面 egui-free**：唯一 egui 后端（`render::egui`）走
//!    `feature = "egui"`；`rustmark-ui` 经 `pub use rustmark_render::*;`
//!    拼回 `rustmark_ui::presentation::rich::*` 原路径，全部消费方
//!    （`views::preview_view` / `views::inline::math`）零代码改动；
//! 2. **触发拆分的条件**（2026-10-19 评估时尚未满足，R-04 因 `rich/`
//!    实测超 5.6k 行阈值而执行）：出现第二个消费方（CLI 批量导出 / 插件
//!    系统）、需要独立发布或独立版本节奏、或主 crate 编译时间成为日常
//!    瓶颈且实测证明拆出去能缩短。

// 测试代码豁免 unwrap / expect / panic（D-6 守卫惯例：与 `rustmark-ui` 等 crate
// 的 lib.rs 一致）。富内容引擎的单测用 `expect` 断言排版结果，属有意许可。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod math;
pub mod mermaid;
/// 渲染端口层（egui-free 的几何命令 + 唯一 egui 后端）。见 `render` 模块文档。
pub mod render;
