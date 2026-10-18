//! Preferences 用例编排服务（阶段 04 · S4-08）。
//!
//! 编排主题 / 编辑器 / 快捷键读写。**改键必须先做冲突检测**：在副本上试绑后调用领域层
//! [`crate::domain::preferences::value_objects::ShortcutTable::conflicts`]，若存在阻断级
//! 冲突（重复 / 保留键）则**返回 `Err` 且不提交**，根治原型「静默覆盖」缺陷。
//!
//! 全部修改经 [`Self::commit_change`] 统一落盘（[`PreferencesRepository::save`]）并发布
//! [`DomainEvent::PreferencesChanged`]，订阅方（如 Markdown 缓存）据此刷新。设置页、菜单、
//! 命令面板共用 [`Self::get_settings`] 返回的同一份 [`SettingsView`]，杜绝「四处真相」。

use std::sync::{Arc, RwLock};

use crate::domain::preferences::aggregate::Preferences;
use crate::domain::preferences::errors::PreferencesError;
use crate::domain::preferences::repository::PreferencesRepository;
use crate::domain::preferences::value_objects::{
    Conflict, CustomThemePalette, EditorSettings, KeyCombo, LintSettings, TabSize, Theme,
    ThemeKind, WritingGoal,
};

use crate::shared::error::AppError;
use crate::shared::event::{DomainEvent, EventBus};
use crate::shared::result::AppResult;

use crate::application::dto::{
    ConflictView, CustomThemePaletteView, EditorSettingsView, GoalProgressView, LintRulesView,
    SettingsView, ShortcutView, ThemeView,
};

use super::commands::*;
use super::queries::*;

/// Preferences 用例服务。
pub struct PreferencesAppService {
    repository: Arc<dyn PreferencesRepository>,
    bus: Arc<dyn EventBus>,
    prefs: Arc<RwLock<Preferences>>,
}

impl PreferencesAppService {
    /// 构造服务并从仓储加载配置（不存在时载入默认配置）。
    pub fn new(
        repository: Arc<dyn PreferencesRepository>,
        bus: Arc<dyn EventBus>,
    ) -> AppResult<Self> {
        let prefs = repository.load()?;
        Ok(Self {
            repository,
            bus,
            prefs: Arc::new(RwLock::new(prefs)),
        })
    }

    /// 用给定配置构造服务（测试注入已知状态）。
    pub fn with_preferences(
        repository: Arc<dyn PreferencesRepository>,
        bus: Arc<dyn EventBus>,
        prefs: Preferences,
    ) -> Self {
        Self {
            repository,
            bus,
            prefs: Arc::new(RwLock::new(prefs)),
        }
    }

    /// 读取全部设置（主题 + 编辑器 + 快捷键一览，三处共用同一份数据）。
    pub fn get_settings(&self, _cmd: GetSettingsQuery) -> AppResult<SettingsView> {
        let prefs = self.lock_read()?;
        Ok(to_settings_view(&prefs))
    }

    /// 按当前字数投影写作目标进度（V11-05）。
    ///
    /// 单独开一个**只返回标量**的入口而不是复用 [`Self::get_settings`]：
    /// 状态栏每帧都要它，而 `get_settings` 会连快捷键表一起克隆
    /// （`Vec<ShortcutView>`），在 60fps 下是纯粹的浪费。
    ///
    /// 读锁失败（理论上仅在 poisoning 时发生，而本项目 `panic = "abort"`）
    /// 按「未设置目标」返回：进度条是锦上添花，不值得为此让整帧渲染失败。
    pub fn goal_progress(&self, words: usize) -> GoalProgressView {
        let goal = self
            .prefs
            .read()
            .map(|prefs| prefs.editor().writing_goal())
            .unwrap_or_default();
        GoalProgressView {
            goal: goal.words(),
            words,
            percent: goal.progress_percent(words),
            reached: goal.is_reached(words),
        }
    }

    /// 是否设置了写作目标（问题 053）。
    ///
    /// [`Self::goal_progress`] 要**当前字数**才谈得上进度，而字数是整篇统计；
    /// 状态栏每帧结算目标时，未设目标的那条路径不该为一条不画的进度条付这个成本，
    /// 故单开一个只读设置的入口（一次读锁 + 一个整数比较）。
    ///
    /// 读锁失败按「未设置」返回，与 [`Self::goal_progress`] 同一理由：
    /// 进度条是锦上添花，不值得为此让整帧渲染失败。
    pub fn has_writing_goal(&self) -> bool {
        self.prefs
            .read()
            .map(|prefs| prefs.editor().writing_goal().words() > 0)
            .unwrap_or(false)
    }

    /// 撤销策略（合并窗口 + 栈深度，由编辑器设置推导，F-1）。
    ///
    /// 供组合根在装配 [`DocumentAppService`] 时快照注入：修复
    /// 「撤销合并窗口」设置在领域层有值、文档构造却走默认策略的断线。
    /// 读锁失败按默认策略返回（与 [`Self::goal_progress`] 同一理由：
    /// 排障信息不值得让启动失败）。
    ///
    /// [`DocumentAppService`]: crate::application::document::service::DocumentAppService
    pub fn undo_policy(&self) -> crate::domain::document::undo::UndoPolicy {
        self.prefs
            .read()
            .map(|prefs| prefs.undo_policy())
            .unwrap_or_default()
    }

    /// 读取当前快捷键冲突列表。
    pub fn get_conflicts(&self, _cmd: GetConflictsQuery) -> AppResult<Vec<ConflictView>> {
        let prefs = self.lock_read()?;
        Ok(to_conflict_views(prefs.conflicts()))
    }

    /// 设置主题；非法取值（字体/行高越界）返回 `Err` 且不落库。
    pub fn set_theme(&self, cmd: SetThemeCommand) -> AppResult<()> {
        let theme = to_theme(&cmd.theme)?;
        self.lock_write()?
            .set_theme(theme)
            .map_err(|e: PreferencesError| AppError::domain(e.to_string()))?;
        self.commit_change()
    }

    /// 设置编辑器选项；非法取值返回 `Err` 且不落库。
    pub fn set_editor(&self, cmd: SetEditorCommand) -> AppResult<()> {
        let editor = to_editor(&cmd.editor)?;
        self.lock_write()?
            .set_editor(editor)
            .map_err(|e: PreferencesError| AppError::domain(e.to_string()))?;
        self.commit_change()
    }

    /// 重绑定快捷键：**先检测阻断级冲突**，冲突则 `Err` 且不提交；否则落库并广播。
    pub fn set_shortcut(&self, cmd: SetShortcutCommand) -> AppResult<()> {
        let combo = KeyCombo::parse(&cmd.combo)
            .map_err(|e: PreferencesError| AppError::domain(e.to_string()))?;

        // 在副本上试绑，检测是否引入阻断级冲突（重复 / 保留键）。
        let has_blocking = {
            let prefs = self.lock_read()?;
            let mut table = prefs.shortcuts().clone();
            table.rebind(cmd.action, combo);
            table
                .conflicts()
                .iter()
                .any(|c| c.kind().is_blocking() && c.combo() == combo)
        };
        if has_blocking {
            return Err(AppError::domain(format!(
                "快捷键 {} 存在阻断级冲突，已拒绝绑定",
                combo.display()
            )));
        }

        self.lock_write()?.rebind(cmd.action, combo);
        self.commit_change()
    }

    /// 恢复出厂设置（删除配置文件并重建默认配置），随后广播变更。
    pub fn reset(&self, _cmd: ResetPreferencesCommand) -> AppResult<()> {
        let prefs = self.repository.reset()?;
        *self.lock_write()? = prefs;
        self.bus.publish(DomainEvent::PreferencesChanged);
        Ok(())
    }

    /// 只恢复**一个分区**的默认值，其余分区原样保留（原型缺陷 3.6 根治）。
    ///
    /// 与 [`Self::reset`] 的区别是本方法的**唯一**理由：全量重置会把
    /// 13 个字段一起打回，用户只想重置字体却连主题与缩进都被改掉。
    ///
    /// 走 `commit_change`（而非 `repository.reset`）是关键：全量重置靠
    /// 「删配置文件」实现，在只重置一个分区时用它会**丢掉用户的其他配置**——
    /// 这正是缺陷本身。分区重置必须是「读当前值 → 改一个分区 → 写回」。
    pub fn reset_section(&self, cmd: ResetSectionCommand) -> AppResult<()> {
        {
            let mut prefs = self.lock_write()?;
            match cmd.section {
                SettingsSection::Appearance => prefs.reset_theme(),
                SettingsSection::Editor | SettingsSection::SpellCheck => prefs.reset_editor(),
                SettingsSection::Shortcuts => {
                    // 解析失败时领域保持原绑定不变，这里原样上抛，
                    // 不吞成「重置成功」——否则用户会发现快捷键全没了却没报错。
                    prefs
                        .reset_shortcuts()
                        .map_err(|e: PreferencesError| AppError::domain(e.to_string()))?;
                }
            }
        }
        self.commit_change()
    }

    // ---------- 内部 ----------

    /// 落盘当前配置并广播 `PreferencesChanged`。
    fn commit_change(&self) -> AppResult<()> {
        let prefs = self.lock_read()?.clone();
        self.repository.save(&prefs)?;
        self.bus.publish(DomainEvent::PreferencesChanged);
        Ok(())
    }

    fn lock_read(&self) -> AppResult<std::sync::RwLockReadGuard<'_, Preferences>> {
        self.prefs
            .read()
            .map_err(|_| AppError::io("偏好设置读锁已损坏"))
    }

    fn lock_write(&self) -> AppResult<std::sync::RwLockWriteGuard<'_, Preferences>> {
        self.prefs
            .write()
            .map_err(|_| AppError::io("偏好设置写锁已损坏"))
    }
}

// ---------- DTO ↔ 领域 转换 ----------

/// 由 DTO 构造主题；未知种类或非法的字体/行高返回 `Err`。
fn to_theme(view: &ThemeView) -> AppResult<Theme> {
    let kind = match view.kind.as_str() {
        "light" => ThemeKind::Light,
        "dark" => ThemeKind::Dark,
        "system" => ThemeKind::System,
        "custom" => ThemeKind::Custom,
        other => {
            return Err(AppError::domain(format!(
                "未知主题种类：{other}（应为 light/dark/system/custom）"
            )));
        }
    };
    let mut theme = Theme::new(
        kind,
        view.font_family.clone(),
        view.font_size,
        view.line_height,
    );
    // 调色板处理分两种语义，缺一不可：
    // - `custom` 种类：必须有调色板，缺失时退回默认调色板（保持原行为，
    //   否则首次切到 custom 且无任何配色改动时会崩 / 丢配色）。
    // - 非 `custom` 种类：仅当视图里**确实带上了**自定义配色才保留它，
    //   避免从 custom 切到 light/dark 后自定义配色被静默丢弃（#039：
    //   用户从 custom 切走，再次启动由 `to_theme` 重建领域对象时调色板被丢，
    //   等于无声改了用户设置）。`resolve` 只在 kind 为 custom 时应用调色板，
    //   故保留它不改变当前渲染，只是不再丢失数据——用户仍可在设置页把
    //   kind 选回 custom 来重新应用这套配色。
    match kind {
        ThemeKind::Custom => {
            let palette = view
                .custom
                .clone()
                .map(CustomThemePalette::from)
                .unwrap_or_default();
            theme.custom = Some(palette);
        }
        _ => {
            if let Some(custom_view) = &view.custom {
                theme.custom = Some(CustomThemePalette::from(custom_view.clone()));
            }
        }
    }
    Ok(theme)
}

/// 由 DTO 构造编辑器设置；缩进宽度非法时返回 `Err`。
fn to_editor(view: &EditorSettingsView) -> AppResult<EditorSettings> {
    let tab_size = TabSize::from_spaces(view.tab_size as usize).ok_or_else(|| {
        AppError::domain(format!("非法缩进宽度：{}（应为 2/4/8）", view.tab_size))
    })?;
    Ok(EditorSettings::new(
        tab_size,
        view.word_wrap,
        view.auto_save_interval,
        view.spell_check,
        view.show_line_numbers,
        400,
        view.split_ratio,
        view.sidebar_visible,
        // 目标在领域层饱和到上限：界面填多大都不会让设置保存失败（V11-05）。
        WritingGoal::new(view.writing_goal),
        to_lint_settings(&view.lint),
        // 0 在领域构造时收敛到 1；这里再兜一层是因为 DTO 可能来自
        // 手改的配置文件（反序列化不经过 `new`）。
        view.max_open_documents.max(1),
    ))
}

/// 领域 [`Preferences`] → [`SettingsView`]。
fn to_settings_view(prefs: &Preferences) -> SettingsView {
    SettingsView {
        theme: to_theme_view(prefs.theme()),
        editor: to_editor_view(prefs.editor()),
        shortcuts: prefs
            .shortcuts()
            .display_all()
            .iter()
            .map(|(action, combo)| ShortcutView {
                key: action.key().to_string(),
                action: action.label().to_string(),
                combo: combo.clone(),
            })
            .collect(),
    }
}

/// 领域 [`Theme`] → [`ThemeView`]。
fn to_theme_view(theme: &Theme) -> ThemeView {
    ThemeView {
        kind: theme.kind().as_str().to_string(),
        font_family: theme.font_family().to_string(),
        font_size: theme.font_size(),
        line_height: theme.line_height(),
        custom: theme.palette().cloned().map(CustomThemePaletteView::from),
    }
}

/// 领域 [`EditorSettings`] → [`EditorSettingsView`]。
fn to_editor_view(editor: &EditorSettings) -> EditorSettingsView {
    EditorSettingsView {
        tab_size: editor.tab_size().spaces() as u8,
        word_wrap: editor.word_wrap(),
        auto_save_interval: editor.auto_save_interval(),
        spell_check: editor.spell_check(),
        show_line_numbers: editor.show_line_numbers(),
        split_ratio: editor.split_ratio(),
        sidebar_visible: editor.sidebar_visible(),
        writing_goal: editor.writing_goal().words(),
        lint: to_lint_rules_view(editor.lint()),
        // 兜底收敛：领域值只可能经 serde / `new` 两条路变 0，前者已收敛，
        // 后者会被 `is_valid` 拒绝——这里再钳一次，保证视图永远拿得到 ≥1。
        max_open_documents: editor.max_open_documents().max(1),
    }
}

/// 领域 [`LintSettings`] → [`LintRulesView`]（V11-08）。
fn to_lint_rules_view(lint: LintSettings) -> LintRulesView {
    LintRulesView {
        heading_jump: lint.heading_jump,
        list_marker_mix: lint.list_marker_mix,
        trailing_whitespace: lint.trailing_whitespace,
        unclosed_fence: lint.unclosed_fence,
    }
}

/// [`LintRulesView`] → 领域 [`LintSettings`]（V11-08）。
fn to_lint_settings(view: &LintRulesView) -> LintSettings {
    LintSettings {
        heading_jump: view.heading_jump,
        list_marker_mix: view.list_marker_mix,
        trailing_whitespace: view.trailing_whitespace,
        unclosed_fence: view.unclosed_fence,
    }
}

/// 领域 [`Conflict`] 列表 → [`ConflictView`] 列表。
fn to_conflict_views(conflicts: Vec<Conflict>) -> Vec<ConflictView> {
    conflicts
        .iter()
        .map(|c| ConflictView {
            description: c.describe(),
            combo: c.combo().display(),
            actions: c.actions().iter().map(|a| a.label().to_string()).collect(),
        })
        .collect()
}

/// 编译期断言：`PreferencesAppService` 可跨线程共享。
const fn _assert_send_sync<T: Send + Sync>() {}
const _ASSERT_PREFS_SERVICE: () = _assert_send_sync::<PreferencesAppService>();
