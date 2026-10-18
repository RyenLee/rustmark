//! 设置对话框（打开 / 实时预览 / 取消回滚 / 确认 / 快捷键改绑）。
use super::*;
use crate::app::frame::actions::domain_action;

impl EditorApp {
    /// 当前设置快照（「取消」回滚的基准）。
    pub fn settings_snapshot(&self) -> Snapshot {
        let view = self.settings();
        Snapshot {
            theme_kind: view.theme.kind.clone(),
            font_family: view.theme.font_family.clone(),
            font_size: view.theme.font_size,
            line_height: view.theme.line_height,
            tab_size: view.editor.tab_size,
            word_wrap: view.editor.word_wrap,
            auto_save_interval: view.editor.auto_save_interval,
            spell_check: view.editor.spell_check,
            show_line_numbers: view.editor.show_line_numbers,
            lint: view.editor.lint,
            max_open_documents: view.editor.max_open_documents,
        }
    }

    /// 打开设置对话框（记录回滚基准）。
    pub(crate) fn open_settings(&mut self) {
        let snapshot = self.settings_snapshot();
        self.settings_ui.open(snapshot);
    }

    /// 写一个主题字段并立即生效（实时预览，S6-11 验收要求）。
    ///
    /// 失败时把错误记进设置状态而不是静默：领域会拒绝非法字号 / 行高，
    /// 静默吞掉的表现是「拖了滑块但字号没变」，用户无从判断是 bug 还是自己操作错了。
    pub fn apply_theme_field(&mut self, mutate: impl FnOnce(&mut ThemeView)) {
        let mut view = self.settings().theme;
        mutate(&mut view);
        match self
            .services
            .preferences
            .set_theme(SetThemeCommand { theme: view })
        {
            Ok(()) => {
                self.settings_ui.error = None;
                self.settings_ui.dirty = true;
                self.ui
                    .set_theme(builtin::resolve(&self.settings().theme, self.system_dark));
            }
            Err(e) => self.settings_ui.set_error(e.to_string()),
        }
    }

    /// 写一个编辑器设置字段并立即生效。
    pub fn apply_editor_field(&mut self, mutate: impl FnOnce(&mut EditorSettingsView)) {
        let mut view = self.settings().editor;
        mutate(&mut view);
        match self
            .services
            .preferences
            .set_editor(SetEditorCommand { editor: view })
        {
            Ok(()) => {
                self.settings_ui.error = None;
                self.settings_ui.dirty = true;
            }
            Err(e) => self.settings_ui.set_error(e.to_string()),
        }
    }

    /// 「恢复本组默认」：只重置**当前分区**（原型缺陷 3.6 根治）。
    pub fn reset_current_settings_section(&mut self) {
        let Some(section) = self.settings_ui.section.resettable_section() else {
            return;
        };
        match self
            .services
            .preferences
            .reset_section(ResetSectionCommand { section })
        {
            Ok(()) => {
                self.settings_ui.error = None;
                self.settings_ui
                    .set_notice(format!("已恢复「{}」默认值", section.label()));
                self.settings_ui.dirty = true;
                // 主题重置会改变生效主题，必须同步到 egui。
                self.ui
                    .set_theme(builtin::resolve(&self.settings().theme, self.system_dark));
                self.ui.set_split_ratio(self.settings().editor.split_ratio);
                // 侧栏开合同属编辑器设置（问题 062）：重置该分区后配置回到
                // 默认（收起），界面必须跟着走——否则出现「配置里是收起、
                // 屏幕上还开着」，下次启动又变收起，看起来像随机。
                self.ui.sidebar_visible = self.settings().editor.sidebar_visible;
            }
            Err(e) => self.settings_ui.set_error(e.to_string()),
        }
    }

    /// 「恢复默认快捷键」（快捷键分区的专属按钮）。
    ///
    /// 走分区重置而不是全量：全量会把用户改过的主题与缩进一起打回。
    pub fn reset_all_shortcuts(&mut self) {
        if let Err(e) = self
            .services
            .preferences
            .reset_section(ResetSectionCommand {
                section: SettingsSection::Shortcuts,
            })
        {
            self.settings_ui.set_error(e.to_string());
            return;
        }
        self.settings_ui.shortcut_rollback.clear();
        self.settings_ui.error = None;
        self.settings_ui.set_notice("已恢复默认快捷键");
        self.reload_shortcut_registry();
    }

    /// 「取消」：把**全部**未确认修改回滚到打开时的快照。
    ///
    /// 验收标准原文是「取消回滚全部未确认修改」——所以这里必须覆盖
    /// 主题、编辑器设置与快捷键三类，只回滚其中两类会让用户以为取消失败了。
    pub fn cancel_settings(&mut self) {
        let Some(snapshot) = self.settings_ui.take_snapshot() else {
            return;
        };
        let theme = ThemeView {
            kind: snapshot.theme_kind,
            font_family: snapshot.font_family,
            font_size: snapshot.font_size,
            line_height: snapshot.line_height,
            custom: None,
        };
        let editor = EditorSettingsView {
            tab_size: snapshot.tab_size,
            word_wrap: snapshot.word_wrap,
            auto_save_interval: snapshot.auto_save_interval,
            spell_check: snapshot.spell_check,
            show_line_numbers: snapshot.show_line_numbers,
            split_ratio: self.settings().editor.split_ratio,
            // 侧栏开合同理沿用当前值（问题 062）：「取消」回滚的是**设置页里
            // 改过的字段**，而开合是在界面上随手切的、不经过设置页，
            // 让它跟着取消一起跳变才是意外。
            sidebar_visible: self.settings().editor.sidebar_visible,
            // 设置页尚未提供写作目标输入框，保存时沿用当前值，避免被 0 覆盖。
            writing_goal: self.settings().editor.writing_goal,
            // 取消时把 lint 开关也回滚到打开设置那一刻（V11-08）。
            lint: snapshot.lint,
            // 打开文档上限同理：设置页改小了，「取消」要回到原值，
            // 否则用户会被困在一个「改小了却退不回去」的上限里。
            max_open_documents: snapshot.max_open_documents,
        };
        // 主题与编辑器各自独立失败：主题写失败不该让编辑器也回滚不了。
        let mut errors: Vec<String> = Vec::new();
        if let Err(e) = self
            .services
            .preferences
            .set_theme(SetThemeCommand { theme })
        {
            errors.push(e.to_string());
        }
        if let Err(e) = self
            .services
            .preferences
            .set_editor(SetEditorCommand { editor })
        {
            errors.push(e.to_string());
        }
        for (action, combo) in self.settings_ui.take_shortcut_rollback() {
            // 纯 UI 动作（如 `Revert`）没有领域对应项，跳过而不是报错——
            // 它本来就不可能被改键（设置页只列出可持久化动作）。
            let Some(domain) = domain_action(action) else {
                continue;
            };
            if let Err(e) = self.services.preferences.set_shortcut(SetShortcutCommand {
                action: domain,
                combo,
            }) {
                errors.push(e.to_string());
            }
        }
        self.settings_ui.error = (!errors.is_empty()).then(|| errors.join("；"));
        self.settings_ui.close();
        self.ui
            .set_theme(builtin::resolve(&self.settings().theme, self.system_dark));
        self.reload_shortcut_registry();
        // 行号栏的会话开关同样回到回滚基准（与主题/快捷键同理由：取消即还原）。
        self.editor.line_numbers = self.settings().editor.show_line_numbers;
        // **在这里**关浮层，而不是让调用方关：三条关闭路径
        // （取消按钮 / 标题栏 × / Esc）必须得到同样结果。
        // 放在调用方的话，Esc 那条路径（`CloseOverlay` 直接
        // `dialog.close_top`）会跳过回滚——用户改了字号按 Esc，
        // 面板关了但字号没回去，且**没有任何提示**。
        self.dialog.close(Overlay::Settings);
    }

    /// 「保存」：确认当前修改并关闭对话框。
    ///
    /// 刻意**不做任何写盘**：每个字段在改动时就已经落盘（实时预览的要求），
    /// 保存只是「关闭」这一下点击。若在这里再写一次，就出现两份落盘路径，
    /// 将来某条路径漏了字段会以「改了没生效」的形式暴露。
    pub fn confirm_settings(&mut self) {
        self.settings_ui.close();
        self.reload_shortcut_registry();
        self.dialog.close(Overlay::Settings);
    }

    /// 快捷键改绑（设置页录制路径）。
    ///
    /// 冲突由应用层 `set_shortcut` 拒绝并返回 `Err`，这里原样转成可见提示——
    /// 原型缺陷：「静默写入后被注册顺序在前的同名动作吞掉，界面毫无提示」。
    pub fn rebind_shortcut(&mut self, action: UiAction, combo: &str) {
        let Some(domain_action) = domain_action(action) else {
            self.settings_ui
                .set_error(format!("{action:?} 尚未映射到领域动作，无法改键"));
            return;
        };
        // 写之前记下原键位——**必须在这里**，而不是让调用方（视图）先记。
        //
        // 放在视图里的后果：任何绕过视图的调用方（命令面板、将来的导入功能、
        // 集成测试）改了键却没登记回滚，「取消」就静默地不回滚它。
        // 症状是「按了取消，某个键位没回来」——用户无从判断是不是自己记错了。
        let original = self
            .settings()
            .shortcuts
            .iter()
            .find(|s| s.key == domain_action.key())
            .map(|s| s.combo.clone())
            .unwrap_or_default();
        self.settings_ui.remember_shortcut(action, original);
        match self.services.preferences.set_shortcut(SetShortcutCommand {
            action: domain_action,
            combo: combo.to_string(),
        }) {
            Ok(()) => {
                self.settings_ui.error = None;
                self.settings_ui.recording = None;
                self.settings_ui.pending_conflict = None;
                self.settings_ui.dirty = true;
                self.reload_shortcut_registry();
            }
            Err(e) => self.settings_ui.set_error(e.to_string()),
        }
    }

    /// 从持久化配置重建快捷键注册表（改键后调用）。
    ///
    /// 不重建的后果：注册表还是旧的，用户在设置页看到新键位，
    /// 按下去却走旧动作——**界面上写着和实际生效的不一致**。
    pub fn reload_shortcut_registry(&mut self) {
        let settings = self.settings();
        self.ui
            .dispatcher
            .set_registry(ShortcutRegistry::from_settings(&settings.shortcuts));
    }

    /// 主题设置（`None` 表示运行期读取失败，保持当前主题不变）。
    pub(super) fn theme_view(&self) -> Option<ThemeView> {
        match self.services.preferences.get_settings(GetSettingsQuery) {
            Ok(settings) => Some(settings.theme),
            Err(_) => None,
        }
    }

    /// 刷新「跟随系统」主题。
    ///
    /// 只有设置确实是 `system` 时才重算。若不加这个判断，用户**显式**选定的
    /// 深/浅主题会被系统外观变化悄悄改回去——这是「跟随系统」最常见的实现错误。
    pub(super) fn refresh_system_theme(&mut self, ctx: &egui::Context) {
        if self.ui.theme.kind != BuiltinTheme::System {
            return;
        }
        // egui 每帧把系统偏好写进 `visuals.dark_mode`（随系统外观变化更新），
        // 直接读它即可，不必引入额外的探测 crate。
        let system_dark = ctx.style_of(ctx.theme()).visuals.dark_mode;
        if system_dark == self.system_dark {
            return;
        }
        self.system_dark = system_dark;
        let Some(view) = self.theme_view() else {
            return;
        };
        self.ui.set_theme(builtin::resolve(&view, system_dark));
    }

    /// 按原型顺序轮换主题：`深色 → 浅色 → 跟随系统 → 深色`。
    ///
    /// # 必须是**固定序列**，不能按当前值推导下一态
    ///
    /// 状态栏「一键切换主题」= 直接深↔浅互换，符合「点一下换深浅」的直觉。
    ///
    /// 原来三态轮换（`dark→light→system→dark`）会导致「浅色点回深色要点两次」，
    /// 与用户预期不符（#039 反馈）。改为以「当前**生效**的明暗」为基准取反：
    ///
    /// - 当前是深色 → 切浅色；当前是浅色 → 切深色（两向都是一次点击）。
    /// - `system` / `custom` 自身没有明暗，按当前解析/渲染出的明暗取反
    ///   （`system` 取系统偏好、`custom` 取其 baseline 的深浅）。
    ///
    /// 切换只动明暗、不删除种类：`system` / `custom` 仍可在「设置页」下拉选回
    /// （`THEME_KINDS` 含四项），不会因一键切换而变得不可达。
    pub(super) fn cycle_theme(&mut self) {
        let settings = self.settings();
        let mut view = settings.theme;
        // 以当前**正在显示**的主题明暗为准取反，而不是按 kind 字符串推导。
        let effective_dark = self.ui.theme.is_dark();
        view.kind = if effective_dark { "light" } else { "dark" }.to_string();
        // 写盘失败不回滚内存状态：主题是**显示偏好**而非数据，
        // 拦住用户切换只会让界面「看起来没反应」。
        let _ = self.services.preferences.set_theme(SetThemeCommand {
            theme: view.clone(),
        });
        self.ui.set_theme(builtin::resolve(&view, self.system_dark));
    }

    /// 当前生效的主题。
    pub fn theme(&self) -> &Theme {
        &self.ui.theme
    }

    /// 快捷键分发器（菜单 / 命令面板渲染共用同一份注册表）。
    pub fn dispatcher(&self) -> &ShortcutDispatcher {
        &self.ui.dispatcher
    }

    /// 读取持久化设置；失败时回退到 [`SettingsView::fallback`]。
    pub fn settings(&self) -> SettingsView {
        self.services
            .preferences
            .get_settings(GetSettingsQuery)
            .unwrap_or_else(|_| SettingsView::fallback())
    }

    /// 设置页「字体」下拉的可选项（启动时发现，含默认哨兵）。
    ///
    /// 表现层拿它填充下拉，**不接触文件系统**——发现字体是基础设施层的事
    /// （`infrastructure::fonts`），这里只转发已经发现的清单。
    pub fn editor_font_choices(&self) -> &[EditorFontChoice] {
        &self.editor_fonts
    }

    /// 系统当前是否偏好深色（设置页「跟随系统」提示、状态栏主题文案用）。
    ///
    /// 刻意暴露**系统偏好**而不是「当前生效主题」：后者在
    /// `BuiltinTheme::System` 下已经被解析过一次，拿它判断
    /// 「系统是深是浅」等于拿答案去验证答案。
    pub fn system_prefers_dark(&self) -> bool {
        self.system_dark
    }
}
