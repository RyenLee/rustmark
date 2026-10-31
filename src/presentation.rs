//! 表现层：egui 视图、UI State、主题与快捷键（阶段 06）。

use crate::presentation::icons::IconKey;
use crate::presentation::shortcuts::registry::UiAction;

/// 顶层视图（依赖组合根 `app`，故只能留在 root）。
pub mod views;

// D-3 阶段 4a：非视图子模块 re-export 拼回 `crate::presentation::<子模块>`
// 原路径（views 依赖组合根 `app`，只能留 root）。V11-62（动作 2，批次 1-5，
// 2026-10-25 收官）把五个非视图子层 `widgets` / `state` / `shortcuts` / `theme` /
// `rich` **全部**抽去独立 crate `rustmark-ui`——core 内零消费者，root 经本文件
// 把它们 re-export 回 `crate::presentation::<子模块>` 原路径，存量视图与集成测试零改动。
// 桥接路径与 ui 侧同形（`rustmark_ui::presentation::<子模块>`）。
//
// R-03 B2（2026-10-27）：**逐项显式**，不再用整组 `::{…}` 转发。整组形式的
// 隐患是「新增 ui 子模块被静默带入 root 命名空间」——编译与测试都不报，但
// root 的清单与 §10.1 crate 地图已经失配。逐项后，增删都必须落到本文件；
// `check-arch` 的 `check_ui_submodule_sync` 会拿 ui 侧 `pub mod`、本清单与
// `docs/spec/architecture-constraints.md` §10.1 三方对账（缺一即报）。
pub use rustmark_ui::presentation::rich;
pub use rustmark_ui::presentation::shortcuts;
pub use rustmark_ui::presentation::state;
pub use rustmark_ui::presentation::theme;
pub use rustmark_ui::presentation::view_context;
pub use rustmark_ui::presentation::widgets;

// V11-58：向量图标资源（16×16 规格数据 + Painter 渲染器）抽去独立 crate
// `rustmark-icons`——core 内零消费者，住 core 里没有 core 的职责。
// 把 crate 本身 re-export 成 `icons` 模块：`crate::presentation::icons::*`
// 公开路径不变，存量视图（toolbar_view）与测试零改动。
pub use rustmark_icons as icons;

/// 动作 → 图标键 的映射（R-05，2026-10-27）。
// R-05：动作 → 图标键 的映射从 `rustmark-icons` 上移到此处。
//
// 原 `icons` 直接 `use rustmark_actions::UiAction`，资源 crate 反向依赖动作
// 词汇表。`icons` 现在只认自己的 [`IconKey`] 词汇表（见
// `crate::presentation::icons::IconKey`）；「哪个动作对应哪枚图标」是表现层
// 工具栏渲染的职责，留在这里，不在资源 crate 里绑定动作域。
//
// 返回 `None` 表示该动作没有向量图标——调用方据此跳过绘制，与旧行为一致
// （`spec` 对未注册键返回空表、`paint_icon` 直接 return）。本函数直接可达
// `rustmark::presentation::icon_key_for`，集成测试据此断言「每个平铺图标按钮
// 都画出了向量图标」而无需重写映射逻辑。
pub fn icon_key_for(action: UiAction) -> Option<IconKey> {
    match action {
        UiAction::Bold => Some(IconKey::Bold),
        UiAction::Italic => Some(IconKey::Italic),
        UiAction::Strikethrough => Some(IconKey::Strikethrough),
        UiAction::InlineCode => Some(IconKey::InlineCode),
        UiAction::InsertLink => Some(IconKey::InsertLink),
        _ => None,
    }
}
