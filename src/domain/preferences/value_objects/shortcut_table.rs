//! 快捷键表与冲突模型：[`ShortcutBinding`] / [`ConflictKind`] / [`Conflict`]
//! / [`ShortcutTable`]。
//!
//! [`ShortcutTable`] 是显示文本与冲突判定的**唯一真相源**：菜单、命令面板、
//! 快捷键一览均从本表读取；冲突只**报告**不拒绝。

use serde::{Deserialize, Serialize};

use super::{action::ActionId, keys::KeyCombo};

/// 一条快捷键绑定。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct ShortcutBinding {
    action: ActionId,
    combo: KeyCombo,
}

impl ShortcutBinding {
    /// 构造绑定。
    pub const fn new(action: ActionId, combo: KeyCombo) -> Self {
        Self { action, combo }
    }

    /// 由字符串组合构造（配置反序列化路径）。
    pub fn parse(
        action: ActionId,
        raw: &str,
    ) -> Result<Self, crate::domain::preferences::errors::PreferencesError> {
        Ok(Self::new(action, KeyCombo::parse(raw)?))
    }

    /// 动作。
    pub const fn action(&self) -> ActionId {
        self.action
    }

    /// 组合键。
    pub const fn combo(&self) -> KeyCombo {
        self.combo
    }
}

/// 冲突类型。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ConflictKind {
    /// 同一个组合被绑定到多个动作（原型缺陷 D）。
    ///
    /// 严重：触发时行为不确定，必须提示用户。
    Duplicate,
    /// 该组合被操作系统或宿主环境占用。
    Reserved,
    /// 无修饰键的字符键，会与文本输入抢按键（提示级）。
    TextInputClash,
}

impl ConflictKind {
    /// 是否为阻断级冲突（`Duplicate` / `Reserved`）。
    pub const fn is_blocking(&self) -> bool {
        matches!(self, Self::Duplicate | Self::Reserved)
    }

    /// 说明文案（可直接呈现给用户）。
    pub const fn description(&self) -> &'static str {
        match self {
            Self::Duplicate => "该快捷键被绑定到了多个动作，触发时行为不确定",
            Self::Reserved => "该快捷键被操作系统占用，可能无法生效",
            Self::TextInputClash => "无修饰键的字母键会与文本输入冲突，建议增加修饰键",
        }
    }
}

/// 一条快捷键冲突。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Conflict {
    kind: ConflictKind,
    combo: KeyCombo,
    /// 涉及的动作（重复冲突时会有多个）。
    actions: Vec<ActionId>,
}

impl Conflict {
    /// 构造冲突。
    pub fn new(kind: ConflictKind, combo: KeyCombo, actions: Vec<ActionId>) -> Self {
        Self {
            kind,
            combo,
            actions,
        }
    }

    /// 冲突类型。
    pub const fn kind(&self) -> ConflictKind {
        self.kind
    }

    /// 冲突的组合键。
    pub const fn combo(&self) -> KeyCombo {
        self.combo
    }

    /// 涉及的动作。
    pub fn actions(&self) -> &[ActionId] {
        &self.actions
    }

    /// 人类可读描述（含组合与动作名）。
    pub fn describe(&self) -> String {
        let names: Vec<&str> = self.actions.iter().map(|a| a.label()).collect();
        format!(
            "{}：{}（{}）",
            self.combo.display(),
            self.kind.description(),
            names.join("、")
        )
    }
}

/// 快捷键表。
///
/// **唯一真相源**：菜单、命令面板、快捷键一览全部从本表读取显示文本
/// （原型缺陷 C 的根治）。
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ShortcutTable {
    bindings: Vec<ShortcutBinding>,
}

/// 被操作系统占用的组合（跨平台保守取值）。
pub const RESERVED_COMBOS: &[&str] = &["Meta+Q", "Meta+W", "Alt+F4", "Ctrl+Alt+Delete"];

impl ShortcutTable {
    /// 由绑定列表构造。
    pub fn new(bindings: Vec<ShortcutBinding>) -> Self {
        Self { bindings }
    }

    /// 默认绑定表。
    ///
    /// 键位选择对齐原型 `mock/shortcuts.ts`，有两条必须如此：
    /// - `Ctrl+Shift+S` 归 [`ActionId::DocumentStats`]（原型的「文档统计」）。
    ///   领域层早期把它给了 [`ActionId::SaveAs`]，导致按验收用例②把统计改绑到
    ///   `Ctrl+Shift+S` 时被当成重复绑定而**拒绝**——用例根本走不通。
    /// - `F8` / `Esc` 必须有默认项。二者都是**无修饰键单键**，原型正是在这里
    ///   的匹配器把它们全拒了（B-01 / B-03），导致「声明了却永不触发」。
    ///
    /// 全表保证两两不重复，且不触碰 [`RESERVED_COMBOS`]。
    pub fn default_bindings() -> Result<Self, crate::domain::preferences::errors::PreferencesError>
    {
        use crate::domain::preferences::errors::PreferencesError;

        let raw: &[(ActionId, &str)] = &[
            // —— 文件 ——
            (ActionId::NewDocument, "Ctrl+N"),
            (ActionId::OpenDocument, "Ctrl+O"),
            (ActionId::Save, "Ctrl+S"),
            (ActionId::SaveAll, "Ctrl+Alt+S"),
            (ActionId::SaveAs, "Ctrl+Alt+Shift+S"),
            (ActionId::CloseTab, "Ctrl+Alt+W"),
            (ActionId::Export, "Ctrl+E"),
            // —— 编辑 ——
            (ActionId::Undo, "Ctrl+Z"),
            (ActionId::Redo, "Ctrl+Shift+Z"),
            (ActionId::Find, "Ctrl+F"),
            (ActionId::Replace, "Ctrl+H"),
            (ActionId::FindInFiles, "Ctrl+Shift+F"),
            // —— 格式 ——
            (ActionId::Bold, "Ctrl+B"),
            (ActionId::Italic, "Ctrl+I"),
            (ActionId::Strikethrough, "Ctrl+Shift+X"),
            (ActionId::InlineCode, "Ctrl+Shift+C"),
            (ActionId::InsertLink, "Ctrl+K"),
            (ActionId::CodeBlock, "Ctrl+Shift+K"),
            (ActionId::InsertTable, "Ctrl+Shift+T"),
            (ActionId::ToggleQuote, "Ctrl+Shift+Q"),
            (ActionId::PromoteHeading, "Ctrl+="),
            (ActionId::DemoteHeading, "Ctrl+-"),
            (ActionId::Indent, "Ctrl+]"),
            (ActionId::Outdent, "Ctrl+["),
            (ActionId::ToggleTask, "Ctrl+Shift+Enter"),
            // —— 视图 ——
            (ActionId::TogglePreview, "Ctrl+P"),
            (ActionId::ToggleOutline, "Ctrl+Shift+O"),
            (ActionId::ToggleSidebar, "Ctrl+\\"),
            (ActionId::DocumentStats, "Ctrl+Shift+S"),
            (ActionId::CommandPalette, "Ctrl+Shift+P"),
            (ActionId::InsertSymbol, "Ctrl+Shift+E"),
            (ActionId::CompareDiff, "Ctrl+Alt+D"),
            (ActionId::NextTab, "Ctrl+Shift+]"),
            (ActionId::PrevTab, "Ctrl+Shift+["),
            (ActionId::OpenSettings, "Ctrl+,"),
            // —— 无修饰键单键（原型在此处失效，见B-01 / B-03）——
            (ActionId::FocusMode, "F8"),
            (ActionId::CloseOverlay, "Esc"),
        ];
        let mut bindings = Vec::with_capacity(raw.len());
        for (action, combo) in raw {
            let parsed = KeyCombo::parse(combo).map_err(|_| PreferencesError::InvalidCombo {
                raw: combo.to_string(),
            })?;
            bindings.push(ShortcutBinding::new(*action, parsed));
        }
        Ok(Self::new(bindings))
    }

    /// 全部绑定。
    pub fn bindings(&self) -> &[ShortcutBinding] {
        &self.bindings
    }

    /// 查询某动作当前绑定的组合。
    pub fn binding_for(&self, action: ActionId) -> Option<KeyCombo> {
        self.bindings
            .iter()
            .find(|binding| binding.action() == action)
            .map(|binding| binding.combo())
    }

    /// 查询某组合触发的动作；未绑定时返回 `None`。
    pub fn action_for(&self, combo: KeyCombo) -> Option<ActionId> {
        self.bindings
            .iter()
            .find(|binding| binding.combo() == combo)
            .map(|binding| binding.action())
    }

    /// 重绑定某动作（已存在则替换，不存在则追加）。
    ///
    /// **不拒绝重复组合**——重复属于「可提示的冲突」，由 [`ShortcutTable::conflicts`]
    /// 统一报告（原型缺陷 D 的根治：判定集中在一处，而不是散落在 UI 里）。
    pub fn rebind(&mut self, action: ActionId, combo: KeyCombo) {
        match self
            .bindings
            .iter_mut()
            .find(|binding| binding.action() == action)
        {
            Some(binding) => *binding = ShortcutBinding::new(action, combo),
            None => self.bindings.push(ShortcutBinding::new(action, combo)),
        }
    }

    /// 恢复某动作为默认绑定（用户自定义后想还原时使用）。
    pub fn reset_action(
        &mut self,
        action: ActionId,
    ) -> Result<(), crate::domain::preferences::errors::PreferencesError> {
        let defaults = Self::default_bindings()?;
        match defaults.binding_for(action) {
            Some(combo) => {
                self.rebind(action, combo);
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// 检测全部冲突。
    ///
    /// 判定规则：
    /// 1. 同一组合绑定到 ≥2 个动作 → [`ConflictKind::Duplicate`]；
    /// 2. 组合属于 [`RESERVED_COMBOS`] → [`ConflictKind::Reserved`]；
    /// 3. 无修饰键的**字符键** → [`ConflictKind::TextInputClash`]（提示级，
    ///    功能键如 `Escape` / `F5` **不算**冲突——原型缺陷 A 的根治）。
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut out = Vec::new();

        let mut seen: Vec<(KeyCombo, Vec<ActionId>)> = Vec::new();
        for binding in &self.bindings {
            if let Some(entry) = seen.iter_mut().find(|(combo, _)| *combo == binding.combo()) {
                entry.1.push(binding.action());
            } else {
                seen.push((binding.combo(), vec![binding.action()]));
            }
        }
        for (combo, actions) in seen {
            let is_duplicate = actions.len() > 1;
            let displayed = combo.display();
            if is_duplicate {
                out.push(Conflict::new(
                    ConflictKind::Duplicate,
                    combo,
                    actions.clone(),
                ));
            }
            // 注意：重复冲突与保留/文本输入冲突可以叠加报出，故此处一律使用克隆，
            // 避免把 `actions` 移进第一个 `Conflict` 后无法复用。
            if RESERVED_COMBOS
                .iter()
                .any(|reserved| reserved.eq_ignore_ascii_case(&displayed))
            {
                out.push(Conflict::new(
                    ConflictKind::Reserved,
                    combo,
                    actions.clone(),
                ));
            }
            if combo.clashes_with_text_input() {
                out.push(Conflict::new(ConflictKind::TextInputClash, combo, actions));
            }
        }
        out
    }

    /// 是否只存在提示级冲突（没有阻断级冲突）。
    pub fn is_usable(&self) -> bool {
        !self.conflicts().iter().any(|c| c.kind().is_blocking())
    }

    /// 生成「快捷键一览」所需的全部条目（动作 + 规范显示文本）。
    ///
    /// 菜单与命令面板**必须**复用本函数的输出，避免第二处显示逻辑。
    pub fn display_all(&self) -> Vec<(ActionId, String)> {
        self.bindings
            .iter()
            .map(|binding| (binding.action(), binding.combo().display()))
            .collect()
    }
}

impl Default for ShortcutTable {
    fn default() -> Self {
        Self::default_bindings().unwrap_or_else(|_| Self::new(Vec::new()))
    }
}
