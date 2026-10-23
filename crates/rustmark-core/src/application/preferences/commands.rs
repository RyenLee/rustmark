//! Preferences 用例命令（阶段 04 · S4-08）。
//!
//! 命令携带 DTO（而非领域值对象），由服务转换为领域类型，确保 AppService 公开签名
//! 不暴露聚合根。改键命令携带**字符串形式的组合键**，解析失败由服务返回 `Err`。

use crate::domain::preferences::value_objects::ActionId;

/// 设置主题命令（携带 DTO）。
#[derive(Clone, Debug)]
pub struct SetThemeCommand {
    /// 主题 DTO。
    pub theme: crate::application::dto::ThemeView,
}

/// 设置编辑器命令（携带 DTO）。
#[derive(Clone, Debug)]
pub struct SetEditorCommand {
    /// 编辑器设置 DTO。
    pub editor: crate::application::dto::EditorSettingsView,
}

/// 重绑定快捷键命令。
#[derive(Clone, Debug)]
pub struct SetShortcutCommand {
    /// 目标动作。
    pub action: ActionId,
    /// 组合键的规范字符串形式（如 `Ctrl+Shift+S`）。
    pub combo: String,
}

/// 恢复出厂设置（**全量**：删除配置文件并重建默认配置）。
#[derive(Clone, Copy, Debug, Default)]
pub struct ResetPreferencesCommand;

/// 设置页分区（用于「恢复本组默认」的粒度）。
///
/// 对齐原型 `SettingsDialog.vue` 的 `resetCategorySafe`：用户只想重置
/// 一个分区，全量重置会连坐其余字段（原型缺陷 3.6）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SettingsSection {
    /// 外观（主题 / 字体 / 字号 / 行高）。
    #[default]
    Appearance,
    /// 编辑器（缩进 / 自动换行 / 自动保存间隔 / 写作目标）。
    Editor,
    /// 快捷键。
    Shortcuts,
    /// 语法检查（拼写检查开关）。
    ///
    /// 原型把它单列一区，但落到 `EditorSettings` 上与自动保存同属一个字段组，
    /// 故本枚举暂不提供该分区——**分区是 UI 概念，字段归属是领域概念**，
    /// 两者不必一一对应。留在这里是为了将来 `spell_check` 独立成字段时
    /// 有确定的落点，而不必再改枚举（改枚举会让已持久化的配置失效）。
    SpellCheck,
}

impl SettingsSection {
    /// 中文显示名（用于 toast / 无结果提示）。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "外观",
            Self::Editor => "编辑器",
            Self::Shortcuts => "快捷键",
            Self::SpellCheck => "语法检查",
        }
    }
}

/// 只恢复某个分区的默认值（其余分区**原样保留**）。
#[derive(Clone, Copy, Debug)]
pub struct ResetSectionCommand {
    /// 目标分区。
    pub section: SettingsSection,
}
