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
//! | `math` | `layout(&str, size: f32, display: bool, Measure) -> Result<MathLayout, MathError>` | `paint(&Painter, Pos2, &MathLayout, f32, Color32)` | `fitted_height` | `MathError` |
//! | `mermaid` | `layout(&str, size: f32, Measure) -> Result<MermaidLayout, MermaidError>` | `paint(&Painter, Rect, &MermaidLayout, &MermaidStyle)` | `fitted_height` | `MermaidError` |
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
//! - 唯一的跨模块依赖是 `egui` 的 `Pos2` / `Rect` / `Vec2` / `Align` /
//!   `LayoutJob`——**不依赖 `presentation` 的任何其他模块**（不碰主题、
//!   不碰 icons、不碰 state），配色由调用方通过参数传入。
//!
//! # 为什么没有拆成独立 crate（2026-10-19 评估）
//!
//! 结论：**负收益**。
//!
//! 1. **编译隔离不成立**：Rust 以 crate 为编译单元。`rustmark` 依赖
//!    `rustmark-math` 之后，改一行 math 仍要重编主 crate（依赖元数据变了，
//!    rustc 不做 API 兼容判定），总工作量反而比现在更大；
//! 2. **没有复用收益**：唯一消费方就是本 crate；
//! 3. **会削弱架构语义**：`egui` 依赖会扩散到非 presentation 的
//!    `Cargo.toml`，而「egui 只在表现层」是 `check-arch` 的一条守卫；
//! 4. **门禁要跟着改**：新成员的单测需要 `cargo test --workspace` 才会跑，
//!    漏改就是静默降低覆盖率。
//!
//! 触发物理拆分的条件（满足任一再动）：
//! - 出现第二个消费方（CLI 批量导出 / 插件系统）；
//! - `rich` 需要独立发布或独立版本节奏；
//! - 主 crate 编译时间成为日常瓶颈，且实测证明拆出去能缩短（而不是变长）。

pub mod math;
pub mod mermaid;
