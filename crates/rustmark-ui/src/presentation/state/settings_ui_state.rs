//! 设置对话框 UI 状态（分区、快照、快捷键录制、错误提示）。
//!
//! # 硬边界：这里只放**视图状态**
//!
//! 允许：当前分区、打开时的快照、正在录制的动作、待确认的冲突、错误文案。
//! 禁止：`SettingsView` / `Preferences` 等 DTO 或领域对象的**副本**。
//!
//! # 为什么「快照」可以放在这里而 DTO 不行
//!
//! 快照是**打开那一瞬间读到的值的副本**，而 DTO 是「业务数据的当前值」。
//! 前者是视图状态（「用户按了取消要回到哪」），后者必须每帧从服务读，
//! 否则就变成第二份真相。这条区分与 `app_ui_state` 里「切换标签回到开头」
//! 的处理同源：视图可以记住「上一次的意图」，但不能记住「业务现在是什么」。
//!
//! 快照本身存的是**纯数据字段**（`Snapshot` 结构体）而不是 `SettingsView`，
//! 这样「快照里多了哪些字段」由本模块自己负责，将来 `SettingsView` 加字段时
//! 编译期就会提醒（`Snapshot` 由 `Snapshot::capture(&SettingsView)` 构造，
//! 少一个字段不会报错——所以额外用 `snapshot_covers_every_field` 风格的
//! 测试在应用层钉住）。

use crate::presentation::shortcuts::registry::UiAction;
use rustmark_core::application::dto::LintRulesView;
use rustmark_core::application::preferences::commands::SettingsSection;

/// 设置页标签（**视图**概念：导航栏上能看到的那几项）。
///
/// # 为什么与 [`SettingsSection`] 分成两个枚举
///
/// 原型导航栏有 5 项（外观 / 编辑器 / 快捷键 / 语法检查 / 关于），
/// 但「可恢复默认的分区」只有 4 个——「关于」没有任何设置项。
/// 若把它们塞进同一个枚举，就得靠「值相等但语义不同」来区分，
/// 而 `can_reset_section` 对「关于」也返回 true，
/// 于是「关于」页会挂一个点了没反应的「恢复默认」按钮——
/// **按钮存在但无效比按钮不存在更糟**（用户会以为程序坏了）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SettingsTab {
    /// 外观。
    #[default]
    Appearance,
    /// 编辑器。
    Editor,
    /// 快捷键。
    Shortcuts,
    /// 语法检查。
    SpellCheck,
    /// 关于（无设置项，不可重置）。
    About,
}

impl SettingsTab {
    /// 全部标签（导航栏顺序即此顺序）。
    pub const ALL: &'static [SettingsTab] = &[
        SettingsTab::Appearance,
        SettingsTab::Editor,
        SettingsTab::Shortcuts,
        SettingsTab::SpellCheck,
        SettingsTab::About,
    ];

    /// 中文显示名。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "外观",
            Self::Editor => "编辑器",
            Self::Shortcuts => "快捷键",
            Self::SpellCheck => "语法检查",
            Self::About => "关于",
        }
    }

    /// 该标签对应的**可重置分区**（`None` = 无设置项，不可重置）。
    pub const fn resettable_section(self) -> Option<SettingsSection> {
        match self {
            Self::Appearance => Some(SettingsSection::Appearance),
            Self::Editor => Some(SettingsSection::Editor),
            Self::Shortcuts => Some(SettingsSection::Shortcuts),
            Self::SpellCheck => Some(SettingsSection::SpellCheck),
            Self::About => None,
        }
    }
}

/// 打开设置时的设置快照（用于「取消」回滚）。
///
/// 只存**会被设置页改到的字段**，刻意不存 `shortcuts`：
/// 快捷键的取消回滚走另一条路（逐条改回，见 [`SettingsUiState::shortcut_rollback`]），
/// 因为快捷键表可能有几十项，为它们做整体快照会让这个结构体随绑定数膨胀。
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// 主题种类。
    pub theme_kind: String,
    /// 主题字体族。
    pub font_family: String,
    /// 主题字号。
    pub font_size: f32,
    /// 主题行高。
    pub line_height: f32,
    /// 缩进空格数。
    pub tab_size: u8,
    /// 自动换行。
    pub word_wrap: bool,
    /// 自动保存间隔（秒）。
    pub auto_save_interval: Option<u32>,
    /// 拼写检查。
    pub spell_check: bool,
    /// 是否显示行号栏。
    pub show_line_numbers: bool,
    /// 格式检查规则开关（V11-08）。
    pub lint: LintRulesView,
    /// 打开文档数量上限（问题 038）。
    pub max_open_documents: u32,
    /// 写作目标（字数；0 = 未设）。
    pub writing_goal: u32,
}

/// 待确认的快捷键冲突。
#[derive(Clone, Debug, PartialEq)]
pub struct PendingConflict {
    /// 想绑定到动作上的组合键（规范显示文本）。
    pub combo: String,
    /// 占用该键的动作名（中文）。
    pub with_label: String,
    /// 占用该键的动作稳定标识。
    pub with_key: String,
    /// 占用者**原来的**组合键。
    ///
    /// 「解除原绑定」需要它：只知道「谁占着」不足以恢复，
    /// 而恢复不了就等于把用户的键位弄丢了。
    pub with_combo: String,
}

/// 设置对话框状态。
#[derive(Clone, Debug, Default)]
pub struct SettingsUiState {
    /// 当前标签（导航栏选中项）。
    pub section: SettingsTab,
    /// 打开时的快照（`None` 表示设置页从未打开过）。
    pub snapshot: Option<Snapshot>,
    /// 正在录制的动作（`Some` 时下一个组合键会写给它）。
    pub recording: Option<UiAction>,
    /// 待确认的快捷键冲突（未确认前不写盘）。
    pub pending_conflict: Option<PendingConflict>,
    /// 错误提示（如非法取值被领域拒绝）。
    pub error: Option<String>,
    /// 底部提示（保存成功 / 已恢复某分区默认）。
    pub notice: Option<String>,
    /// 「取消」回滚时的快捷键还原清单（动作标识 → 原组合键）。
    pub shortcut_rollback: Vec<(UiAction, String)>,
    /// 本次会话是否改过任何设置（决定「取消」是否有事可做）。
    pub dirty: bool,
    /// 写作目标输入框的临时文本（R-05）。
    ///
    /// 放这里而不是视图局部变量：egui 的 `TextEdit` 需要**稳定**的 `String`
    /// 持源串，否则每帧重建会让用户刚敲的字符在下一帧被「从设置值重算」覆盖。
    /// 这是「视图状态」不是「业务副本」——它只描述「输入框里现在敲了什么」，
    /// 落盘值仍由 `EditorSettingsView::writing_goal` 独占；本字段只是它的编辑缓冲。
    pub writing_goal_text: String,
}

impl SettingsUiState {
    /// 构造默认状态（默认停在「外观」分区）。
    pub fn new() -> Self {
        Self {
            section: SettingsTab::Appearance,
            writing_goal_text: String::new(),
            ..Default::default()
        }
    }

    /// 打开设置：记录当前分区并准备回滚清单。
    pub fn open(&mut self, snapshot: Snapshot) {
        self.snapshot = Some(snapshot);
        self.error = None;
        self.notice = None;
        self.pending_conflict = None;
        self.recording = None;
        self.shortcut_rollback.clear();
        self.dirty = false;
    }

    /// 关闭设置：丢弃一切临时状态。
    pub fn close(&mut self) {
        self.snapshot = None;
        self.recording = None;
        self.pending_conflict = None;
        self.error = None;
        self.notice = None;
        self.shortcut_rollback.clear();
        self.dirty = false;
        self.writing_goal_text.clear();
    }

    /// 写入写作目标输入框的编辑缓冲（D-3 阶段 3：视图 clone → 就地编辑 →
    /// 变化才值写回，字段的可变引用不出 state）。
    pub fn set_writing_goal_text(&mut self, writing_goal_text: String) {
        self.writing_goal_text = writing_goal_text;
    }

    /// 是否有未确认的修改（「取消」按钮据此禁用）。
    #[cfg(test)]
    pub(crate) fn has_pending_changes(&self) -> bool {
        self.dirty
    }

    /// 当前标签是否支持「恢复本组默认」。
    ///
    /// 「快捷键」返回 `false`：它的重置走页脚的「恢复默认快捷键」按钮
    /// （问题 038 从分区列表底部移来），文案与分区都不同——
    /// 「恢复本组默认」的文案对快捷键分区会显得不知所云。
    pub fn can_reset_section(&self) -> bool {
        matches!(
            self.section.resettable_section(),
            Some(
                SettingsSection::Appearance | SettingsSection::Editor | SettingsSection::SpellCheck
            )
        )
    }

    /// 开始录制某个动作的快捷键。
    pub fn start_recording(&mut self, action: UiAction) {
        self.recording = Some(action);
        self.pending_conflict = None;
        self.error = None;
    }

    /// 取消录制。
    pub fn stop_recording(&mut self) {
        self.recording = None;
    }

    /// 记录一个错误提示。
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
    }

    /// 记录一条底部提示（成功类）。
    pub fn set_notice(&mut self, message: impl Into<String>) {
        self.notice = Some(message.into());
    }

    /// 记住某个动作的原始键位（「取消」时逐条改回）。
    pub fn remember_shortcut(&mut self, action: UiAction, original: impl Into<String>) {
        // 只记**第一次**的原值：同一动作被连改两次时，
        // 第二次记下的已是中间值，还原回去会停在中间——用户看到的是
        // 「取消了但键位还是我改过的那个」。
        if !self.shortcut_rollback.iter().any(|(a, _)| *a == action) {
            self.shortcut_rollback.push((action, original.into()));
        }
    }

    /// 是否需要回滚快捷键。
    #[cfg(test)]
    pub(crate) fn needs_shortcut_rollback(&self) -> bool {
        !self.shortcut_rollback.is_empty()
    }

    /// 取走回滚清单（并清空，避免二次回滚）。
    pub fn take_shortcut_rollback(&mut self) -> Vec<(UiAction, String)> {
        std::mem::take(&mut self.shortcut_rollback)
    }

    /// 取走快照（关闭时用）。
    pub fn take_snapshot(&mut self) -> Option<Snapshot> {
        self.snapshot.take()
    }
}

/// 写作目标输入框文本 → 落盘值（R-05）。
///
/// 纯函数、无副作用，单独抽出是为了让「0 / 空串 = 未设」的归一逻辑
/// 能被单元测试与变异验证直接钉住，不必借 egui 渲染去驱动整个设置页。
///
/// - 空串 → 0（未设）；
/// - `"0"` → 0（未设）：显式敲 0 与留空同义；
/// - 其它数字 → 该数字（领域以 `WritingGoal::is_set` 判定 0 = 未设，存 0 即正确）；
/// - 非数字且非空 → 保留 `current`，避免一次手滑把已设目标清掉。
pub fn normalize_writing_goal(raw: &str, current: u32) -> u32 {
    let raw = raw.trim();
    if raw.is_empty() {
        return 0;
    }
    match raw.parse::<u32>() {
        Ok(0) => 0,
        Ok(words) => words,
        Err(_) => current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot {
            theme_kind: "dark".into(),
            font_family: "system-ui".into(),
            font_size: 14.0,
            line_height: 1.6,
            tab_size: 4,
            word_wrap: true,
            auto_save_interval: Some(2),
            spell_check: false,
            show_line_numbers: false,
            lint: LintRulesView::default(),
            max_open_documents: 12,
            writing_goal: 0,
        }
    }

    #[test]
    fn opening_records_snapshot_and_clears_dirty() {
        let mut state = SettingsUiState::new();
        state.dirty = true;
        state.open(snap());
        assert!(state.snapshot.is_some());
        assert!(!state.has_pending_changes());
    }

    #[test]
    fn about_tab_offers_no_reset() {
        let mut state = SettingsUiState::new();
        for tab in SettingsTab::ALL {
            state.section = *tab;
            let expected = !matches!(tab, SettingsTab::About | SettingsTab::Shortcuts);
            assert_eq!(
                state.can_reset_section(),
                expected,
                "{tab:?} 的「恢复本组默认」可见性不对"
            );
        }
    }

    #[test]
    fn every_tab_maps_to_a_known_section_or_none() {
        for tab in SettingsTab::ALL {
            if *tab == SettingsTab::About {
                assert!(
                    tab.resettable_section().is_none(),
                    "「关于」没有任何设置项，不该映射到可重置分区"
                );
            } else {
                assert!(
                    tab.resettable_section().is_some(),
                    "{tab:?} 应映射到一个可重置分区，否则「恢复本组默认」永远无效"
                );
            }
        }
    }

    #[test]
    fn shortcut_rollback_keeps_the_first_original() {
        let mut state = SettingsUiState::new();
        state.remember_shortcut(UiAction::Bold, "Ctrl+B");
        state.remember_shortcut(UiAction::Bold, "Ctrl+Alt+B");
        let taken = state.take_shortcut_rollback();
        assert_eq!(taken, vec![(UiAction::Bold, "Ctrl+B".to_string())]);
        assert!(!state.needs_shortcut_rollback(), "取走后应清空");
    }

    #[test]
    fn normalize_writing_goal_treats_zero_and_empty_as_unset() {
        // 0 与空串都归一到 0（未设）；正数原样；非数字保留原值。
        assert_eq!(normalize_writing_goal("0", 1000), 0, "显式 0 = 未设");
        assert_eq!(normalize_writing_goal("", 1000), 0, "空串 = 未设");
        assert_eq!(normalize_writing_goal("   ", 1000), 0, "纯空白 = 未设");
        assert_eq!(normalize_writing_goal("1000", 0), 1000, "正数原样");
        assert_eq!(normalize_writing_goal("abc", 500), 500, "非数字保留原目标");
        assert_eq!(
            normalize_writing_goal("12x", 500),
            500,
            "半截数字保留原目标"
        );
    }
}
