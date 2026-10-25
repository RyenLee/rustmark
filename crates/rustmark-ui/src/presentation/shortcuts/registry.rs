//! 动作表与快捷键注册表（阶段 06 · S6-09）。
//!
//! # 这个文件存在的唯一理由：消灭「四处真相」
//!
//! 原型的快捷键有**四个**互不相干的来源：菜单项上写的文字、
//! `mock/shortcuts.ts` 的默认表、`useHotkeys.ts` 的匹配器、
//! 设置弹窗里的录制列表。改一个键要改四处，漏一处就静默失效
//! （原型缺陷 B-02：`Ctrl+Shift+S` 声明了但从没注册过）。
//!
//! 本模块把四合一：
//!
//! ```text
//!                ShortcutRegistry（唯一动作表+ 绑定）
//!                     │
//!   ┌─────────────────┼─────────────────┐
//!   ↓                 ↓                 ↓
//! 菜单显示         命令面板显示     keydown 分发
//! ```
//!
//! 三处**都**只能问 [`ShortcutRegistry`]，谁都不许自己写一份键位。
//! 注册遗漏在结构上不可能发生——因为「注册」本身就是「遍历这张表」。
//!
//! # 与领域层的关系
//!
//! 表现层**不得**依赖 `domain`（`check-arch` 会拦），所以这里：
//! - 用 [`UiAction`] 这套自有枚举表达动作，与领域 `ActionId` 通过
//!   [`UiAction::persisted_key`] 的**字符串契约**对齐，而���是类型依赖；
//! - 归一化 / 显示规则**逐位复刻**领域 `KeyCombo`（见 [`normalize`]），
//!   并由 `tests/presentation/shortcut_registry_test.rs` 做**差分测试**锁定两者一致。
//!
//! 绝不能「顺手在表现层 import 领域类型」——那会让整个分层失效，
//! 也会让「改键要改两处」的问题从四处变成两处。
//!
//! # 模块结构
//!
//! - `action`：[`UiAction`] 动作枚举与 [`MenuGroup`] 菜单分组（V11-57 起住在
//!   零依赖契约 crate `rustmark-actions`，本文件 re-export 拼回原路径）。
//! - `keys`：[`Modifiers`] / [`Key`] / [`NamedKey`] 表现层自有按键模型（刻意复刻领域）。
//! - `combo`：[`KeyCombo`]、[`ParseComboError`] 与 [`normalize`] 归一化。
//! - `palette`：[`ActionTable`] / [`ActionEntry`] / [`ScoredAction`] 与模糊检索。
//! - 本文件：[`ShortcutRegistry`] 唯一注册表（默认键位 + 用户覆盖的两轮构造）。

use std::collections::HashMap;

use rustmark_core::application::dto::ShortcutView;

mod combo;
mod keys;
mod palette;

pub use combo::{KeyCombo, ParseComboError, normalize};
pub use keys::{Key, Modifiers, NamedKey};
// V11-62 批次 2：state（UI State）搬入 `rustmark-ui` 后，命令面板状态
// （`CommandRow { entry: ActionEntry, .. }`）与快捷键总览的 `sections`
// 签名都要跨 crate 读写这两个类型——re-export 随升 `pub`（类型本身早已
// 是 `pub struct`，此前只是路径不可达）。`group` 字段同步公开（与
// action / label / combo 三个兄弟字段一致），已登记 field guard 白名单。
pub use palette::{ActionEntry, ActionTable};
// V11-57：动作词汇表抽成零依赖契约 crate `rustmark-actions`。连带 re-export
// `action` **模块本身**——`registry::action::…` 深层路径保持解析（存量零使用，
// 但「搬走就让路径 404」不该是抽取的代价）。`keywords` / `default_combo`
// 随之在该 crate 里升 `pub`：core 的命令面板与 `build` 都要跨 crate 读。
pub use rustmark_core::actions::{MenuGroup, UiAction, action};

// ============================================================================
// 注册表
// ============================================================================

/// 快捷键注册表：**唯一**动作表 + 当前生效绑定。
///
/// # 不变式
///
/// 1. 同一 [`KeyCombo`] 至多绑定一个动作（`HashMap` 键唯一，结构上保证）；
/// 2. [`Self::display_for`] 是唯一的键位显示来源；
/// 3. [`Self::resolve`] **不因修饰键缺失而拒绝**——`F8` / `Esc` 这类
///    无修饰键单键必须能触发（原型缺陷 B-01 的根治点）。
#[derive(Clone, Debug, Default)]
pub struct ShortcutRegistry {
    /// 归一化后的绑定：`KeyCombo` → 动作。
    bindings: HashMap<KeyCombo, UiAction>,
}

impl ShortcutRegistry {
    /// 以默认键位构造（应用层尚不可用时的兜底）。
    pub fn with_defaults() -> Self {
        Self::build(|_, _| None)
    }

    /// 以应用层下发的绑定构造（**权威来源**，用户可能已改键）。
    ///
    /// 未出现在 `views` 中的动作退回 [`UiAction::default_combo`]，
    /// 这样新增动作在旧配置下也立即可用，而不被静默禁用。
    pub fn from_settings(views: &[ShortcutView]) -> Self {
        Self::build(|action, default| {
            views
                .iter()
                .find(|view| view.key == action.persisted_key().unwrap_or("~none~"))
                .and_then(|view| normalize(&view.combo).ok())
                .filter(|user| *user != default)
                .map(|user| (default, user))
        })
    }

    /// 两处构造共用的骨架：先铺默认，再按覆盖项替换。
    ///
    /// `pick_override(action, default)` 返回 `Some((default, user))` 表示该动作
    /// 被用户改键，应以 `user` 为准。
    ///
    /// **必须两轮**：若在同一个循环里「边铺默认边应用覆盖」，那么当用户把
    /// `Save` 改到 `Ctrl+Alt+S`（`SaveAll` 的默认键）时，后遍历到的
    /// `SaveAll` 会把 `Ctrl+Alt+S` 又写回去，把用户的覆盖悄悄吃掉。
    /// 先铺满默认、再统一应用覆盖，覆盖永远是最后写入的一方。
    fn build<F>(mut pick_override: F) -> Self
    where
        F: FnMut(UiAction, KeyCombo) -> Option<(KeyCombo, KeyCombo)>,
    {
        let mut defaults: HashMap<KeyCombo, UiAction> = HashMap::new();
        for action in UiAction::ALL {
            let Some(raw) = action.default_combo() else {
                continue;
            };
            if raw.is_empty() {
                continue;
            }
            let Ok(default) = normalize(raw) else {
                continue;
            };
            defaults.insert(default, *action);
        }

        // 第二轮：应用用户覆盖。被覆盖动作的旧键位要让位，
        // 否则旧绑定会残留成「幽灵快捷键」。
        let mut bindings = defaults.clone();
        for action in UiAction::ALL {
            let Some(raw) = action.default_combo() else {
                continue;
            };
            if raw.is_empty() {
                continue;
            }
            let Ok(default) = normalize(raw) else {
                continue;
            };
            if let Some(user) = pick_override(*action, default).map(|(_, user)| user) {
                bindings.remove(&default);
                bindings.insert(user, *action);
            }
        }
        Self { bindings }
    }

    /// 查按键触发的动作。
    ///
    /// 精确匹配 `KeyCombo`，**不做**前缀匹配，也不按修饰键子集匹配——
    /// 否则只按 `S` 时会误触发 `Ctrl+S`。
    pub fn resolve(&self, combo: KeyCombo) -> Option<UiAction> {
        self.bindings.get(&combo).copied()
    }

    /// 查动作当前绑定的组合。
    pub fn combo_for(&self, action: UiAction) -> Option<KeyCombo> {
        self.bindings
            .iter()
            .find(|(_, a)| **a == action)
            .map(|(combo, _)| *combo)
    }

    /// 动作的显示用键位文本（无绑定时返回空串）。
    ///
    /// 菜单 / 命令面板 / 一览**三处都调本函数**——这是「三处同步」的实现点。
    pub fn display_for(&self, action: UiAction) -> String {
        self.combo_for(action)
            .map(|combo| combo.display())
            .unwrap_or_default()
    }

    /// 重绑定（录制新键后调用）。
    ///
    /// 返回被**挤掉**的动作：若 `combo` 已被别的动作占用，
    /// 原动作将变为「无绑定」，调用方必须把它一并展示给用户
    /// （原型缺陷 B-04：先注册者静默吞掉后者）。
    pub fn rebind(&mut self, action: UiAction, combo: KeyCombo) -> Option<UiAction> {
        // 同动作换键：先删旧的，再看新键是否撞了别人。
        if let Some(old) = self.combo_for(action)
            && old != combo
        {
            self.bindings.remove(&old);
        }
        let displaced = self.bindings.insert(combo, action);
        // 若撞的正是自己（改回原键），不算挤掉任何动作。
        (displaced != Some(action)).then_some(displaced).flatten()
    }

    /// 解除某动作的绑定（返回被解除的组合）。
    pub fn unbind(&mut self, action: UiAction) -> Option<KeyCombo> {
        let previous = self.combo_for(action)?;
        self.bindings.remove(&previous);
        Some(previous)
    }

    /// 当前所有绑定，按动作枚举序——保证「一览」表格顺序稳定。
    pub fn entries(&self) -> Vec<(UiAction, KeyCombo)> {
        let mut out: Vec<(UiAction, KeyCombo)> = self
            .bindings
            .iter()
            .map(|(combo, action)| (*action, *combo))
            .collect();
        out.sort_by_key(|(action, _)| *action);
        out
    }

    /// 动作表（菜单与命令面板渲染的唯一依据）。
    pub fn table(&self) -> ActionTable<'_> {
        ActionTable { registry: self }
    }
}
