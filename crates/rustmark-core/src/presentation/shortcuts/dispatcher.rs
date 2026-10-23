//! 快捷键分发器（阶段 06 · S6-09）。
//!
//! # 这里曾经是原型的致命bug（B-01）
//!
//! `useHotkeys.ts:16` 的匹配器对事件做了一件事：
//! 「没按到修饰键的按键一律忽略」。于是 `F8` 与 `Esc` 这类
//! **无修饰键单键**永远匹配不上——它们在设置里能录、能显示，
//! 但按下去什么也不发生。功能看起来「配了但没用」。
//!
//! 本模块的根治方式是**结构性的**，不是加特判：
//!
//! - 无修饰键单键**一律参与匹配**（[`is_global_safe_single_key`] 只用于
//!   *提示*，绝不用于*拒绝*）；
//! - 冲突判定交给应用层 `SetShortcutCommand`（领域层 `ShortcutTable::conflicts`），
//!   分发器只管「这个组合对应哪个动作」，不自行决定「这个组合该不该管」；
//! - 没有任何 `if key == Escape { ... }` 之类的硬编码分支（B-03 的根治点），
//!   所以把「关闭浮层」改绑到 `Ctrl+Q` 后，`Esc` 自然失效、无需改代码。

use egui::Event;
use egui::Key as EguiKey;

use super::registry::{Key, KeyCombo, Modifiers, NamedKey, ShortcutRegistry, UiAction, normalize};

/// 分发结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dispatch {
    /// 未匹配到任何动作。
    ///
    /// 关键：这是**正常**结果，不是错误。绝大多数按键都是打字，
    /// 不匹配才是常态。
    NoMatch,
    /// 命中一个动作。
    Triggered(UiAction),
}

impl Dispatch {
    /// 命中的动作（若有）。
    pub const fn action(self) -> Option<UiAction> {
        match self {
            Self::NoMatch => None,
            Self::Triggered(action) => Some(action),
        }
    }

    /// 是否命中。
    pub const fn is_triggered(self) -> bool {
        matches!(self, Self::Triggered(_))
    }
}

/// 快捷键分发器：把egui 事件翻译成 [`UiAction`]。
#[derive(Clone, Debug)]
pub struct ShortcutDispatcher {
    registry: std::sync::Arc<ShortcutRegistry>,
    /// 浮层打开时，「会打开浮层的动作」是否让位。
    ///
    /// 默认 `false`：应用启动时**没有任何浮层**，若默认为 `true`，
    /// 则 `Ctrl+,`（设置）、`Ctrl+F`（查找）等入口在首帧就全部失效——
    /// 这是个只在真机运行时才暴露的坑。调用方在浮层打开时置 `true`、
    /// 关闭时置回 `false`。
    ///
    /// 无论此值如何，[`UiAction::CloseOverlay`] 永远放行。
    modal: bool,
}

impl Default for ShortcutDispatcher {
    fn default() -> Self {
        Self::new(ShortcutRegistry::with_defaults())
    }
}

impl ShortcutDispatcher {
    /// 以给定注册表构造（非模态）。
    pub fn new(registry: ShortcutRegistry) -> Self {
        Self {
            registry: std::sync::Arc::new(registry),
            modal: false,
        }
    }

    /// 替换注册表（设置改键后调用）。
    pub fn set_registry(&mut self, registry: ShortcutRegistry) {
        self.registry = std::sync::Arc::new(registry);
    }

    /// 当前注册表（菜单 / 面板渲染共用同一份，见 [`ShortcutDispatcher::table`]）。
    pub fn registry(&self) -> &ShortcutRegistry {
        &self.registry
    }

    /// 动作表视图。
    pub fn table(&self) -> super::registry::ActionTable<'_> {
        self.registry.table()
    }

    /// 设置浮层模态状态。
    pub fn set_modal(&mut self, modal: bool) {
        self.modal = modal;
    }

    /// 把一个egui 按键事件翻译为组合键。
    ///
    /// 单独暴露以便单测直接喂字符串，不必构造 `egui::Event`。
    pub fn combo_of(modifiers: Modifiers, key: EguiKey) -> Option<KeyCombo> {
        key_of(key).map(|key| KeyCombo::new(modifiers, key))
    }

    /// 处理单个按键事件。
    pub fn handle(&self, event: &Event) -> Dispatch {
        let Event::Key {
            key,
            modifiers,
            pressed,
            repeat,
            ..
        } = event
        else {
            return Dispatch::NoMatch;
        };
        // 只处理「刚按下」；`repeat` 是长按自动重复，不能重复触发命令。
        if !*pressed || *repeat {
            return Dispatch::NoMatch;
        }

        let Some(key) = key_of(*key) else {
            return Dispatch::NoMatch;
        };
        let combo = KeyCombo::new(modifiers_from(*modifiers), key);
        self.dispatch_combo(combo)
    }

    /// 按组合键分发。
    pub fn dispatch_combo(&self, combo: KeyCombo) -> Dispatch {
        let Some(action) = self.registry.resolve(combo) else {
            return Dispatch::NoMatch;
        };
        // 浮层模态时，除「关闭浮层」外一律让位——但关闭浮层永远放行，
        // 否则用户会被困在对话框里（这正是硬编码 Esc 分支想解决的问题，
        // 但它的代价是「改绑 Esc 后Esc 失效」）。
        if self.modal && action != UiAction::CloseOverlay && is_overlay_action(action) {
            return Dispatch::NoMatch;
        }
        Dispatch::Triggered(action)
    }

    /// 录制：从一段文本生成组合键（设置页「录制新键」用）。
    ///
    /// 返回 `None` 表示这段输入**不可绑定**，调用方应拒绝并提示。
    pub fn record(raw: &str) -> Result<KeyCombo, super::registry::ParseComboError> {
        normalize(raw)
    }
}

/// 该动作是否会打开/关闭某个浮层。
///
/// 只有这些动作在浮层打开时需要让位；`Ctrl+S` 这类动作即使在对话框打开时
/// 也应照常工作（用户可能想先存盘再关对话框）。
fn is_overlay_action(action: UiAction) -> bool {
    matches!(
        action,
        UiAction::NewDocument
            | UiAction::OpenDocument
            | UiAction::Export
            | UiAction::Find
            | UiAction::Replace
            | UiAction::OpenSettings
            | UiAction::CommandPalette
            | UiAction::DocumentStats
            | UiAction::CompareDiff
            | UiAction::InsertSymbol
            | UiAction::ShortcutOverview
    )
}

/// egui 修饰键 → 领域约定的修饰键。
///
/// # macOS 上 ⌘ 必须映射到 `Meta` 而不是 `Ctrl`
///
/// 配置里写 `Cmd+K` 归一化成 `Meta+K`（领域层的约定）。
/// 若把 ⌘ 折成 `Ctrl`，则 macOS 上��� `Ctrl+K` 与 `Cmd+K` 会互相触发，
/// 而 `Cmd+K` 这条绑定**永远匹配不上**。
///
/// egui 的 `command` 字段已经做好了跨平台抽象：
/// macOS 上它表示 ⌘，其它平台表示 Ctrl。因此：
/// - macOS：`mac_cmd` → `Meta`，`command` 不再重复计入 `Ctrl`；
/// - 其它平台：`command` → `Ctrl`，`mac_cmd` 恒为 `false`，无需处理。
///
/// 用 `mac_cmd`（而非 `command`）判定平台，因为它只在 macOS 上为真。
pub fn modifiers_from(m: egui::Modifiers) -> Modifiers {
    if m.mac_cmd {
        Modifiers {
            ctrl: false,
            alt: m.alt,
            shift: m.shift,
            meta: true,
        }
    } else {
        Modifiers {
            ctrl: m.command,
            alt: m.alt,
            shift: m.shift,
            meta: false,
        }
    }
}

/// egui 按键 → 领域主键。
pub fn key_of(key: EguiKey) -> Option<Key> {
    Some(match key {
        EguiKey::ArrowDown => Key::Named(NamedKey::Down),
        EguiKey::ArrowLeft => Key::Named(NamedKey::Left),
        EguiKey::ArrowRight => Key::Named(NamedKey::Right),
        EguiKey::ArrowUp => Key::Named(NamedKey::Up),
        EguiKey::Escape => Key::Named(NamedKey::Escape),
        EguiKey::Tab => Key::Named(NamedKey::Tab),
        EguiKey::Backspace => Key::Named(NamedKey::Backspace),
        EguiKey::Enter => Key::Named(NamedKey::Enter),
        EguiKey::Space => Key::Named(NamedKey::Space),
        EguiKey::Insert => Key::Named(NamedKey::Insert),
        EguiKey::Delete => Key::Named(NamedKey::Delete),
        EguiKey::Home => Key::Named(NamedKey::Home),
        EguiKey::End => Key::Named(NamedKey::End),
        EguiKey::PageUp => Key::Named(NamedKey::PageUp),
        EguiKey::PageDown => Key::Named(NamedKey::PageDown),
        // 单字符标点。
        EguiKey::Colon => Key::Char(':'),
        EguiKey::Comma => Key::Char(','),
        EguiKey::Backslash => Key::Char('\\'),
        EguiKey::Slash => Key::Char('/'),
        EguiKey::Pipe => Key::Char('|'),
        EguiKey::Questionmark => Key::Char('?'),
        EguiKey::Exclamationmark => Key::Char('!'),
        EguiKey::OpenBracket => Key::Char('['),
        EguiKey::CloseBracket => Key::Char(']'),
        EguiKey::OpenCurlyBracket => Key::Char('{'),
        EguiKey::CloseCurlyBracket => Key::Char('}'),
        EguiKey::Backtick => Key::Char('`'),
        EguiKey::Minus => Key::Char('-'),
        EguiKey::Period => Key::Char('.'),
        EguiKey::Plus => Key::Char('+'),
        EguiKey::Equals => Key::Char('='),
        EguiKey::Semicolon => Key::Char(';'),
        EguiKey::Quote => Key::Char('\''),
        // 数字。
        EguiKey::Num0 => Key::Char('0'),
        EguiKey::Num1 => Key::Char('1'),
        EguiKey::Num2 => Key::Char('2'),
        EguiKey::Num3 => Key::Char('3'),
        EguiKey::Num4 => Key::Char('4'),
        EguiKey::Num5 => Key::Char('5'),
        EguiKey::Num6 => Key::Char('6'),
        EguiKey::Num7 => Key::Char('7'),
        EguiKey::Num8 => Key::Char('8'),
        EguiKey::Num9 => Key::Char('9'),
        _ => {
            // 字母与功能键按名字映射。
            let name = key.name();
            match name {
                "F1" => Key::Function(1),
                "F2" => Key::Function(2),
                "F3" => Key::Function(3),
                "F4" => Key::Function(4),
                "F5" => Key::Function(5),
                "F6" => Key::Function(6),
                "F7" => Key::Function(7),
                "F8" => Key::Function(8),
                "F9" => Key::Function(9),
                "F10" => Key::Function(10),
                "F11" => Key::Function(11),
                "F12" => Key::Function(12),
                _ => {
                    // egui 的 `name()` 对字母返回大写（A..Z）。
                    let mut chars = name.chars();
                    let first = chars.next()?;
                    if chars.next().is_some() {
                        return None;
                    }
                    Key::Char(first.to_ascii_uppercase())
                }
            }
        }
    })
}

/// 无修饰键单键是否**允许绑定**。
///
/// 用途是「设置页给提示」，**绝不是**分发器的准入条件——
/// 原型把提示当成了准入，于是 `F8` 永不触发（B-01）。
///
/// 规则与原型 `isGlobalSafeSingleKey` 一致：功能键与 `Esc` 放行，
/// 其余单键（字母、数字、标点）单独按下属于「打字」，提示加修饰键。
pub const fn is_global_safe_single_key(key: Key) -> bool {
    matches!(key, Key::Function(_) | Key::Named(NamedKey::Escape))
}
