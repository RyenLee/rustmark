//! 主题标识与设置字符串之间的转换。

/// 主题标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BuiltinTheme {
    /// 跟随系统。
    #[default]
    System,
    /// 深色。
    Dark,
    /// 浅色。
    Light,
}

impl BuiltinTheme {
    /// 由设置里的字符串解析（`light` / `dark` / `system`）。
    ///
    /// 未知值退回 [`BuiltinTheme::System`] 而非报错：旧配置里出现
    /// 将来才引入的取值时，程序应当照常启动。
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dark" => Self::Dark,
            "light" => Self::Light,
            _ => Self::System,
        }
    }

    /// 规范字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// 中文显示名（设置页三选一）。
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "跟随系统",
            Self::Dark => "深色",
            Self::Light => "浅色",
        }
    }
}
