//! Preferences 聚合根（S3-12）。
//!
//! 通用域：主题、编辑器设置、快捷键。以**事件**（`DomainEvent::PreferencesChanged`）
//! 向其它上下文广播变化——订阅方（预览缓存、高亮器）据此失效自己的缓存。

use serde::{Deserialize, Serialize};

use super::errors::PreferencesError;
use super::value_objects::{ActionId, Conflict, EditorSettings, KeyCombo, ShortcutTable, Theme};

/// 配置文件 schema 版本（ADR-003）：随结构变更递增，用于读取时迁移。
pub const PREFERENCES_SCHEMA_VERSION: u32 = 1;

/// 偏好设置聚合根。
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Preferences {
    schema_version: u32,
    theme: Theme,
    editor: EditorSettings,
    shortcuts: ShortcutTable,
}

impl Preferences {
    /// 构造偏好设置（会校验取值）。
    pub fn new(
        theme: Theme,
        editor: EditorSettings,
        shortcuts: ShortcutTable,
    ) -> Result<Self, PreferencesError> {
        let preferences = Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            theme,
            editor,
            shortcuts,
        };
        preferences.validate()?;
        Ok(preferences)
    }

    /// 默认偏好设置。
    ///
    /// 默认绑定表是常量解析，理论上不会失败；万一失败则退化为**空绑定表**
    /// （此时 `conflicts()` 为空，UI 仍可用，只是没有快捷键）。
    pub fn default_preferences() -> Self {
        let shortcuts =
            ShortcutTable::default_bindings().unwrap_or_else(|_| ShortcutTable::new(Vec::new()));
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            theme: Theme::default_theme(),
            editor: EditorSettings::default_settings(),
            shortcuts,
        }
    }

    /// schema 版本。
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// 主题。
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// 编辑器设置。
    pub const fn editor(&self) -> &EditorSettings {
        &self.editor
    }

    /// 快捷键表。
    pub const fn shortcuts(&self) -> &ShortcutTable {
        &self.shortcuts
    }

    /// 校验全部取值；非法时返回首个错误。
    pub fn validate(&self) -> Result<(), PreferencesError> {
        if !self.theme.is_valid() {
            return Err(PreferencesError::invalid_setting(
                "theme",
                "字体族不能为空，字号需在 8–48 之间，行高需在 1.0–3.0 之间",
            ));
        }
        if !self.editor.is_valid() {
            return Err(PreferencesError::invalid_setting(
                "editor.auto_save_interval",
                "自动保存间隔不能为 0 秒",
            ));
        }
        Ok(())
    }

    /// 替换主题（校验后生效）。
    pub fn set_theme(&mut self, theme: Theme) -> Result<(), PreferencesError> {
        if !theme.is_valid() {
            return Err(PreferencesError::invalid_setting(
                "theme",
                "字体族不能为空，字号需在 8–48 之间，行高需在 1.0–3.0 之间",
            ));
        }
        self.theme = theme;
        Ok(())
    }

    /// 替换编辑器设置（校验后生效）。
    pub fn set_editor(&mut self, editor: EditorSettings) -> Result<(), PreferencesError> {
        if !editor.is_valid() {
            return Err(PreferencesError::invalid_setting(
                "editor.auto_save_interval",
                "自动保存间隔不能为 0 秒",
            ));
        }
        self.editor = editor;
        Ok(())
    }

    /// 重绑定快捷键。
    pub fn rebind(&mut self, action: ActionId, combo: KeyCombo) {
        self.shortcuts.rebind(action, combo);
    }

    /// 由字符串形式重绑定（配置导入路径）。
    pub fn rebind_str(&mut self, action: ActionId, raw: &str) -> Result<(), PreferencesError> {
        let combo = KeyCombo::parse(raw)?;
        self.shortcuts.rebind(action, combo);
        Ok(())
    }

    /// 当前全部快捷键冲突（供设置页展示）。
    pub fn conflicts(&self) -> Vec<Conflict> {
        self.shortcuts.conflicts()
    }

    /// 只把**主题**恢复为默认值（不动编辑器设置与快捷键）。
    ///
    /// # 为什么需要分区重置
    ///
    /// 原型缺陷 3.6：设置页的「恢复默认」调的是全量 `reset()`，
    /// 一次把 13 个字段全部打回——用户只想把字体改回去，
    /// 主题、缩进、自动保存全被连坐，体验上像 bug。
    ///
    /// 分区粒度在**领域层**而不是 UI 层切：UI 若自己「读出来再拼回去」，
    /// 就得知道每个字段属于哪个分区，那是把领域结构复制一份到表现层，
    /// 加字段时必然漏改（与 S6-07 `editor_ops` / `EditingService` 双份真相同源）。
    pub fn reset_theme(&mut self) {
        self.theme = Theme::default_theme();
    }

    /// 只把**编辑器设置**恢复为默认值。
    ///
    /// 注意：主题与快捷键不在此列——原型把「自动换行 / 拼写检查」也算进
    /// 编辑器分区，故一并归它；而主题属外观、快捷键独立成分区。
    pub fn reset_editor(&mut self) {
        self.editor = EditorSettings::default_settings();
    }

    /// 只把**快捷键**恢复为默认绑定表。
    ///
    /// 失败（默认表本身解析不了，属不可能）时**保持原绑定不变**并返回 `Err`：
    /// 此时若顺手清空绑定，用户会发现「所有快捷键都没了」，
    /// 而那比保留旧绑定糟糕得多。
    pub fn reset_shortcuts(&mut self) -> Result<(), PreferencesError> {
        let defaults = ShortcutTable::default_bindings()?;
        self.shortcuts = defaults;
        Ok(())
    }

    /// 缩进字符串（由 `tab_size` 推导）。
    ///
    /// 表现层与 [`crate::domain::document::services::EditingService`] **共享此值**，
    /// 避免原型缺陷 2.2（缩进宽度与偏好设置不一致）。
    pub fn indent_unit(&self) -> String {
        self.editor.tab_size().indent_unit()
    }

    /// 撤销策略（由编辑器设置推导，供 `Document` 构造时注入）。
    pub fn undo_policy(&self) -> crate::domain::document::undo::UndoPolicy {
        crate::domain::document::undo::UndoPolicy::new(self.editor.undo_merge_window_ms(), 200)
    }
}

impl Default for Preferences {
    fn default() -> Self {
        Self::default_preferences()
    }
}

/// 编译期断言：偏好设置可跨线程共享（后台保存线程会读取）。
const fn _assert_preferences_send_sync<T: Send + Sync>() {}
const _ASSERT_PREFERENCES: () = _assert_preferences_send_sync::<Preferences>();
