//! 按键模型：修饰键、主键（字符 / 功能键 / 命名键）及其解析与显示。
//!
//! 注意：本套类型是领域 `KeyCombo` 的**刻意复刻**（表现层禁止依赖 `domain`），
//! 一致性由 `tests/presentation/shortcut_registry_test.rs` 的差分测试锁定，
//! 不得与领域类型合并。

// ============================================================================
// 组合键
// ============================================================================

/// 修饰键。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Modifiers {
    /// `Ctrl`（Windows/Linux）或 `Command`（macOS）。
    pub ctrl: bool,
    /// `Alt`（macOS 为 `Option`）。
    pub alt: bool,
    /// `Shift`。
    pub shift: bool,
    /// `Meta`（macOS `Command` / Windows `Win`）。
    pub meta: bool,
}

impl Modifiers {
    /// 无修饰键。
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    /// 按固定顺序 `Ctrl → Alt → Shift → Meta` 拼出显示前缀。
    ///
    /// 顺序固定是**归一化**的关键：只有顺序一致，
    /// `Ctrl+Shift+S` 与 `Shift+Ctrl+S` 才判等（原型缺陷 B-02/C）。
    pub(super) fn prefix(self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.meta {
            out.push_str("Meta+");
        }
        out
    }
}

/// 主键：字符键或功能键。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    /// 字符键（ASCII 字母已归一化为大写，故 `S` 与 `s` 判等）。
    Char(char),
    /// 功能键 `F1`–`F12`。
    Function(u8),
    /// 命名键。
    Named(NamedKey),
}

/// 命名键。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NamedKey {
    /// `Esc`
    Escape,
    /// `Enter`
    Enter,
    /// `Tab`
    Tab,
    /// `Space`
    Space,
    /// `Backspace`
    Backspace,
    /// `Delete`
    Delete,
    /// `↑`
    Up,
    /// `↓`
    Down,
    /// `←`
    Left,
    /// `→`
    Right,
    /// `Home`
    Home,
    /// `End`
    End,
    /// `PageUp`
    PageUp,
    /// `PageDown`
    PageDown,
    /// `Insert`
    Insert,
}

impl NamedKey {
    /// 由别名解析（大小写不敏感），别名集与领域 `NamedKey::parse` 一致。
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.to_ascii_lowercase().as_str() {
            "esc" | "escape" => Self::Escape,
            "enter" | "return" | "cr" => Self::Enter,
            "tab" => Self::Tab,
            "space" | " " => Self::Space,
            "backspace" => Self::Backspace,
            "delete" | "del" => Self::Delete,
            "up" => Self::Up,
            "down" => Self::Down,
            "left" => Self::Left,
            "right" => Self::Right,
            "home" => Self::Home,
            "end" => Self::End,
            "pageup" | "pgup" => Self::PageUp,
            "pagedown" | "pgdn" => Self::PageDown,
            "insert" | "ins" => Self::Insert,
            _ => return None,
        })
    }

    /// 规范显示名（与领域 `NamedKey::display` 逐字一致）。
    pub const fn display(self) -> &'static str {
        match self {
            Self::Escape => "Esc",
            Self::Enter => "Enter",
            Self::Tab => "Tab",
            Self::Space => "Space",
            Self::Backspace => "Backspace",
            Self::Delete => "Delete",
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Home => "Home",
            Self::End => "End",
            Self::PageUp => "PageUp",
            Self::PageDown => "PageDown",
            Self::Insert => "Insert",
        }
    }
}

impl Key {
    /// 解析主键字符串；失败返回 `None`。
    ///
    /// **大小写不敏感**：调用方（[`crate::presentation::shortcuts::registry::normalize`]）已把片段转成小写，
    /// 若这里只认大写 `F`，`F8` 会静默解析失败 → 功能键永远绑不上
    /// （这正是原型 B-01「`F8` 永不触发」的同款缺陷）。
    pub fn parse(raw: &str) -> Option<Self> {
        let lower = raw.to_ascii_lowercase();
        if lower.len() >= 2
            && lower.starts_with('f')
            && let Ok(n) = lower[1..].parse::<u8>()
            && (1..=12).contains(&n)
        {
            return Some(Self::Function(n));
        }
        if let Some(named) = NamedKey::parse(raw) {
            return Some(Self::Named(named));
        }
        let mut chars = raw.chars();
        let first = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        Some(Self::Char(normalize_char(first)))
    }
}

/// 字符键归一化：ASCII 字母一律大写。
fn normalize_char(c: char) -> char {
    if c.is_ascii_alphabetic() {
        c.to_ascii_uppercase()
    } else {
        c
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Char(c) => write!(f, "{c}"),
            Self::Function(n) => write!(f, "F{n}"),
            Self::Named(named) => f.write_str(named.display()),
        }
    }
}
