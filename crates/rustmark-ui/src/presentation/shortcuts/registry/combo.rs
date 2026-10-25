//! 组合键与归一化：把任意写法的快捷键字符串解析为唯一的 [`KeyCombo`]。

use super::*;

/// 一个归一化后的快捷键组合。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct KeyCombo {
    modifiers: Modifiers,
    key: Key,
}

impl KeyCombo {
    /// 构造组合（**不做**归一化，请用 [`normalize`]）。
    pub(crate) const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    /// 修饰键。
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// 主键。
    pub const fn key(&self) -> Key {
        self.key
    }

    /// **唯一**的显示形式。
    ///
    /// 菜单右侧文字、命令面板右侧文字、设置里的绑定框、
    /// 「快捷键一览」表格——四处全部调本函数，不得各写各的格式化。
    pub fn display(&self) -> String {
        format!("{}{}", self.modifiers.prefix(), self.key)
    }
}

impl std::fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display())
    }
}

/// 解析失败的原因。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ParseComboError {
    /// 空串或只有修饰键（缺主键）。
    MissingKey,
    /// 出现多个主键。
    MultipleKeys,
    /// 主键名无法识别。
    UnknownKey(String),
}

impl std::fmt::Display for ParseComboError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingKey => f.write_str("缺少主键"),
            Self::MultipleKeys => f.write_str("出现多个主键"),
            Self::UnknownKey(raw) => write!(f, "无法识别的主键：{raw}"),
        }
    }
}

/// 把任意写法的组合串归一化。
///
/// 接受 `Ctrl+Shift+S`、`ctrl + shift + s`、`Shift+Ctrl+s`，全部归一为同一值。
/// 修饰键顺序按 `Ctrl → Alt → Shift → Meta` 固定重排。
pub fn normalize(raw: &str) -> Result<KeyCombo, ParseComboError> {
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
            // `cmd`/`command` 必须归**Meta** 而非 Ctrl：macOS 上 Cmd 就是 ⌘，
            // 归到 Ctrl 会让 `Cmd+K` 与 `Ctrl+K` 判等——两者在 mac 上是不同的键。
            // （差分测试 `normalization_matches_domain_key_combo` 锁定了这条。）
            "meta" | "cmd" | "command" | "super" | "win" => modifiers.meta = true,
            other => {
                let parsed = Key::parse(other)
                    .ok_or_else(|| ParseComboError::UnknownKey(other.to_string()))?;
                if key.is_some() {
                    return Err(ParseComboError::MultipleKeys);
                }
                key = Some(parsed);
            }
        }
    }
    key.map(|k| KeyCombo::new(modifiers, k))
        .ok_or(ParseComboError::MissingKey)
}
