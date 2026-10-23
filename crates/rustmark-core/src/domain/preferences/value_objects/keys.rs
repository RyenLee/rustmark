//! 按键模型值对象：[`Modifiers`] / [`NamedKey`] / [`Key`] / [`KeyCombo`]。
//!
//! [`KeyCombo::parse`] + [`KeyCombo::display`] 是**唯一**的字符串 ↔ 组合转换
//! 与显示源（菜单、命令面板、快捷键一览共用）；归一化保证组合与书写顺序无关。

use std::fmt;

use serde::{Deserialize, Serialize};

/// 修饰键组合（归一化：**与书写顺序无关**）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
pub struct Modifiers {
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
}

impl Modifiers {
    /// 无修饰键。
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    /// 构造修饰键组合。
    pub const fn new(ctrl: bool, alt: bool, shift: bool, meta: bool) -> Self {
        Self {
            ctrl,
            alt,
            shift,
            meta,
        }
    }

    /// 是否不含任何修饰键。
    pub const fn is_empty(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.meta
    }

    /// 按固定顺序（Ctrl → Alt → Shift → Meta）拼出前缀，形如 `"Ctrl+Alt+"`。
    fn prefix(&self) -> String {
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

/// 功能键（非字符键）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedKey {
    /// `Esc`
    Escape,
    /// 回车
    Enter,
    /// 制表
    Tab,
    /// 空格
    Space,
    /// 退格
    Backspace,
    /// 删除
    Delete,
    /// 方向键上
    Up,
    /// 方向键下
    Down,
    /// 方向键左
    Left,
    /// 方向键右
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
    /// `F1`
    F1,
    /// `F2`
    F2,
    /// `F3`
    F3,
    /// `F4`
    F4,
    /// `F5`
    F5,
    /// `F6`
    F6,
    /// `F7`
    F7,
    /// `F8`
    F8,
    /// `F9`
    F9,
    /// `F10`
    F10,
    /// `F11`
    F11,
    /// `F12`
    F12,
}

impl NamedKey {
    /// 由常见别名解析（大小写不敏感）。
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw.to_ascii_lowercase().as_str() {
            "esc" | "escape" => Some(Self::Escape),
            "enter" | "return" | "cr" => Some(Self::Enter),
            "tab" => Some(Self::Tab),
            "space" | " " => Some(Self::Space),
            "backspace" => Some(Self::Backspace),
            "delete" | "del" => Some(Self::Delete),
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "home" => Some(Self::Home),
            "end" => Some(Self::End),
            "pageup" | "pgup" => Some(Self::PageUp),
            "pagedown" | "pgdn" => Some(Self::PageDown),
            "insert" | "ins" => Some(Self::Insert),
            "f1" => Some(Self::F1),
            "f2" => Some(Self::F2),
            "f3" => Some(Self::F3),
            "f4" => Some(Self::F4),
            "f5" => Some(Self::F5),
            "f6" => Some(Self::F6),
            "f7" => Some(Self::F7),
            "f8" => Some(Self::F8),
            "f9" => Some(Self::F9),
            "f10" => Some(Self::F10),
            "f11" => Some(Self::F11),
            "f12" => Some(Self::F12),
            _ => None,
        }
    }

    /// 规范显示名。
    pub(crate) const fn display(&self) -> &'static str {
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
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::F3 => "F3",
            Self::F4 => "F4",
            Self::F5 => "F5",
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::F10 => "F10",
            Self::F11 => "F11",
            Self::F12 => "F12",
        }
    }
}

/// 组合键的主键：字符键或功能键。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Key {
    /// 字符键。**ASCII 字母一律归一化为大写**，故 `s` 与 `S` 判等。
    Char(char),
    /// 功能键。
    Named(NamedKey),
}

impl Key {
    /// 由字符串解析；失败时返回 `None`。
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        if let Some(named) = NamedKey::parse(raw) {
            return Some(Self::Named(named));
        }
        let mut chars = raw.chars();
        let first = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        let normalized = if first.is_ascii_alphabetic() {
            first.to_ascii_uppercase()
        } else {
            first
        };
        Some(Self::Char(normalized))
    }

    /// 规范显示名。
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Char(c) => c.to_string(),
            Self::Named(named) => named.display().to_string(),
        }
    }
}

/// 一个快捷键组合（修饰键 + 主键）。
///
/// 归一化保证：`Ctrl+Shift+S` 与 `Shift+Ctrl+s` **判等**（原型缺陷 C 的根治）。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct KeyCombo {
    modifiers: Modifiers,
    key: Key,
}

impl KeyCombo {
    /// 构造组合。
    pub(crate) const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    /// 无修饰键的组合（如 `Escape`、`F5`）。
    pub const fn plain(key: Key) -> Self {
        Self::new(Modifiers::NONE, key)
    }

    /// 解析字符串形式，如 `"Ctrl+Shift+S"`。
    ///
    /// 规则：以 `+` 分隔；任一片段若匹配修饰键别名（大小写不敏感）则记为修饰键，
    /// 否则视为主键；出现多个主键或没有主键时返回 [`PreferencesError`]。
    pub fn parse(raw: &str) -> Result<Self, crate::domain::preferences::errors::PreferencesError> {
        use crate::domain::preferences::errors::PreferencesError;

        let mut modifiers = Modifiers::NONE;
        let mut key: Option<Key> = None;
        for token in raw.split('+') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" | "option" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                "meta" | "cmd" | "command" | "super" | "win" => modifiers.meta = true,
                other => {
                    let parsed =
                        Key::parse(other).ok_or_else(|| PreferencesError::InvalidCombo {
                            raw: raw.to_string(),
                        })?;
                    if key.is_some() {
                        return Err(PreferencesError::InvalidCombo {
                            raw: raw.to_string(),
                        });
                    }
                    key = Some(parsed);
                }
            }
        }
        match key {
            Some(key) => Ok(Self::new(modifiers, key)),
            None => Err(PreferencesError::InvalidCombo {
                raw: raw.to_string(),
            }),
        }
    }

    /// **唯一**的显示形式（菜单 / 命令面板 / 快捷键一览三处共用此函数）。
    ///
    /// 形如 `Ctrl+Shift+S`、`Esc`、`F5`。
    pub fn display(&self) -> String {
        format!("{}{}", self.modifiers.prefix(), self.key.display())
    }

    /// 是否可能与文本输入冲突：无修饰键的字符键（如裸 `A`）。
    ///
    /// 注意：这是**提示**而非禁止——`Escape` / `F1`–`F12` 等无修饰键的功能键
    /// 不属于冲突（原型缺陷 A 的根治：匹配层不得拒绝纯单键）。
    pub(crate) const fn clashes_with_text_input(&self) -> bool {
        self.modifiers.is_empty() && matches!(self.key, Key::Char(_))
    }
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

impl serde::Serialize for KeyCombo {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.display())
    }
}

impl<'de> serde::Deserialize<'de> for KeyCombo {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}
