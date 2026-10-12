//! `EditorApp::execute` / `collect_actions` 与相关 free fn。
use super::*;

impl EditorApp {
    /// 打开某个浮层并做该浮层特有的初始化。
    ///
    /// 「特有初始化」目前只有查找面板需要（`Replace` 要展开替换框、
    /// `Find` 要收起），但把它放在一处而不是散在 `execute` 的分支里，
    /// 是为了让「打开浮层」这件事只有**一个**入口——将来加遮罩、
    /// 动画、焦点陷阱都在这里加，不会漏。
    pub(super) fn open_overlay(&mut self, overlay: Overlay, action: UiAction) {
        self.dialog.open(overlay);
        // 打开面板即重新扫描：文档内容可能在面板打开期间被改过
        // （面板非模态，用户可以切标签、改内容），沿用旧命中会跳到错误位置。
        if overlay == Overlay::Export {
            // 建议文件名取当前文档标题去扩展名；导出主题取当前生效主题的种类。
            let stem = self
                .active_document_title_stem()
                .unwrap_or_else(|| "document".to_string());
            let theme_kind = match self.ui.theme.kind {
                crate::presentation::theme::builtin::BuiltinTheme::Dark => "dark",
                crate::presentation::theme::builtin::BuiltinTheme::Light => "light",
                crate::presentation::theme::builtin::BuiltinTheme::System => "system",
            };
            self.export_ui.theme = theme_kind.to_string();
            self.export_ui.file_stem = stem;
            // 默认输出目录取文档所在目录；未保存的新文档没有目录，
            // 此时留空并由 `can_submit` 拦下，提示用户先保存或填目录。
            self.export_ui.output_dir = self.active_document_dir().unwrap_or_default();
            self.export_ui.open();
        }
        if overlay == Overlay::Settings {
            // 必须在这里取快照：「取消」要回到的是**打开那一刻**的值，
            // 而设置字段是实时落盘的（实时预览要求），等到视图里再取
            // 就取到的是「用户已经改过」的值，回滚等于没回滚。
            self.open_settings();
        }
        if overlay == Overlay::Find {
            // `Ctrl+F` 在面板已打开时**不**收起替换框：用户的意图是
            // 「回到查找框继续找」，把它变回单行反而是刁难。
            self.find
                .open_panel(self.find.replace_open || action == UiAction::Replace);
            self.sync_find();
        }
        // S6-11f：以下四个浮层有「打开即重置」的状态。
        //
        // 放在**打开那一刻**重置而不是视图里，否则用户第二次打开
        // 会看到上一次的过滤词 / 搜索词 / 选中项——而他从没在
        // 这个面板里做过那个操作。
        if overlay == Overlay::QuickOpen {
            self.quick_open_ui.open();
        }
        if overlay == Overlay::CommandPalette {
            // 命令面板同样「打开即重置」：上一次的查询词对
            // 这次打开毫无意义（面板语境每次都不同）。
            self.command_palette_ui.open();
        }
        if overlay == Overlay::ShortcutOverview {
            self.shortcut_overview_ui.open();
        }
        if overlay == Overlay::Symbol {
            self.symbol_ui.open();
        }
        if overlay == Overlay::Diff {
            // 保留上一次选的对照文件：用户对照两个文件看了半天，
            // 关掉再打开大概率还是想看同一对。**结果**行会重算，
            // 因为左侧内容可能已经变了。
            if self.diff_ui.right_path.is_none() {
                self.diff_ui.error = None;
            }
        }
    }

    /// 执行一个动作。
    ///
    /// 三个入口（菜单 / 快捷键 / 命令面板）**全部**汇聚到这里，
    /// 因此不可能出现「菜单里有的操作，快捷键触发的结果不一样」。
    ///
    /// 未接线动作用 `_ => {}` 兜底而非 `unreachable!()`：
    /// 动作表先于视图实现（动作表有 60 项，视图分批落地），
    /// 若在这里 panic，用户按下任何尚未接线的快捷键都会**直接丢掉窗口**。
    pub fn execute(&mut self, action: UiAction) {
        // 浮层类动作先判：它们占 14 项，逐个写 match 分支会让下面的
        // 布局分支被淹没。判定用两张表的**并集**（`opened_by` 与
        // `also_opened_by`），二者都在 `Overlay` 上，不可能算出不一致的结果。
        if let Some(overlay) = openable_overlay(action) {
            self.open_overlay(overlay, action);
            return;
        }

        match action {
            // —— 文件 ——
            // `Save` / `NewDocument` / `SaveAs` 此前落在 `_ => {}`：动作表里有、
            // 快捷键能触发，但没有任何分支处理，`Ctrl+S` / `Ctrl+N` /
            // `Ctrl+Shift+S` 全部静默失效。状态栏的「未保存 → 点击保存」
            // 与欢迎页的三个主操作也走这里，故**必须**接线，
            // 不能让各视图各写一套保存逻辑。
            UiAction::Save => self.run_save(false),
            UiAction::SaveAs => self.run_save(true),
            UiAction::NewDocument => self.run_new_document(),
            // 「打开文件夹」：请求工作区目录选择（后台对话框），结果由
            // `apply_dialog_outcome` 扫盘装载。入口从侧栏上移到「文件」菜单后，
            // 菜单 / 命令面板仍共用这一条路径（三入口一致）。
            UiAction::OpenFolder => self.request_dialog(PendingDialog::PickFolder),
            // `OpenDocument` **故意不在这里**：`Ctrl+O` 是快速打开浮层的
            // 开启动作（`Overlay::QuickOpen.opened_by`），已经被上面的
            // `openable_overlay` 分支拦走。若在此再写一个 `request_dialog`，
            // 它永远不可达——一段**读起来像接线、实际是死代码**的分支，
            // 比没有分支更容易误导（它会让「Ctrl+O 打不开文件」这件事
            // 看起来已经处理过了）。系统文件对话框的入口在快速打开
            // 面板内的「浏览本机文件…」按钮。
            // —— 布局 ——
            UiAction::ToggleSidebar => self.ui.toggle_sidebar(),
            UiAction::TogglePreview => self.ui.toggle_preview(),
            UiAction::ToggleStatusbar => self.ui.toggle_statusbar(),
            UiAction::ToggleOutline => self.ui.toggle_sidebar_tab(SidebarTab::Outline),
            // `FindInFiles`（Ctrl+Shift+F，全局「在文件中查找」）此前落在 `_ => {}`：
            // 动作表里有、快捷键能触发，但没有任何分支处理，于是按下去毫无反应。
            // 它**不**打开浮层——全局搜索是侧栏的 Search 标签，故直接切到该标签
            // 并展开侧栏，与 `ToggleOutline` 同一思路。
            UiAction::FindInFiles => {
                self.ui.toggle_sidebar_tab(SidebarTab::Search);
            }
            UiAction::ToggleFocusMode => self.ui.toggle_focus_mode(),
            // —— 标签 ——
            UiAction::CloseTab => self.close_active_tab(),
            UiAction::NextTab => self.cycle_document(1),
            UiAction::PrevTab => self.cycle_document(-1),
            UiAction::GotoTab1 => self.goto_document(1),
            UiAction::GotoTab2 => self.goto_document(2),
            UiAction::GotoTab3 => self.goto_document(3),
            UiAction::GotoTab4 => self.goto_document(4),
            UiAction::GotoTab5 => self.goto_document(5),
            UiAction::GotoTab6 => self.goto_document(6),
            UiAction::GotoTab7 => self.goto_document(7),
            UiAction::GotoTab8 => self.goto_document(8),
            UiAction::GotoTab9 => self.goto_document(9),
            // —— 主题 ——
            UiAction::ToggleTheme => self.cycle_theme(),
            // —— 编辑（领域权威：撤销/重做走领域，不走 egui 内置）——
            UiAction::Undo => self.undo_edit(),
            UiAction::Redo => self.redo_edit(),
            UiAction::Indent => self.indent_selection(),
            UiAction::Outdent => self.outdent_selection(),
            UiAction::MoveLineUp => self.move_line(-1),
            UiAction::MoveLineDown => self.move_line(1),
            // —— 格式（S6-07）——
            // 全部汇入 `apply_format`：三入口（工具栏 / 快捷键 / 命令面板）
            // 都经由 `execute` 落到这里，故行为**按构造**一致，
            // 不依赖「记得三处都写对」。
            UiAction::Bold
            | UiAction::Italic
            | UiAction::Strikethrough
            | UiAction::InlineCode
            | UiAction::InsertLink
            | UiAction::CodeBlock
            | UiAction::InsertTable
            | UiAction::ToggleQuote
            | UiAction::ToggleList
            | UiAction::ToggleOrderedList
            | UiAction::PromoteHeading
            | UiAction::DemoteHeading
            | UiAction::ToggleTask => self.apply_format(action),
            // —— 关闭浮层 ——
            // Esc 的仲裁点在浮层状态里：先给浮层一次机会关自己，
            // 没有浮层可关时才退出专注模式。否则 Esc 会「关掉对话框顺便
            // 把专注模式也退掉」——一次按键做两件事几乎总是 bug。
            UiAction::CloseOverlay if self.dialog.is_open(Overlay::Settings) => {
                // Esc 语义 = 取消：与面板上的「取消」按钮、与标题栏 × 一致。
                self.cancel_settings();
            }
            UiAction::CloseOverlay if self.dialog.close_top().is_none() => {
                self.ui.exit_focus_mode();
            }
            // 有浮层被关掉时落到这里：什么都不做。
            UiAction::CloseOverlay => {}
            // —— 其余动作在对应子任务中接线 ——
            _ => {}
        }
    }

    /// 收集本帧触发的动作。
    ///
    /// 用 `i.events` 而非 `i.keys_down`：`keys_down` 是**当前按住**的集合，
    /// 长按会在每一帧都报告一次，用它会导致「按住 Ctrl+S 连存几十次」。
    /// `events` 只含本帧新增的按键事件，且分发器已过滤 `repeat`。
    ///
    /// # 为什么是 `pub`
    ///
    /// 它是「按键事件 → 动作」这条通路的**唯一**实现，`update` 与测试共用。
    /// 曾经它是私有的，于是集成测试只能自己在测试侧重写一遍
    /// 「焦点在输入框时让位」的规则——重写出来的副本与生产代码各改各的，
    /// 变异测试立刻证明了这一点：把生产代码里的让位逻辑删掉，
    /// 测试仍然全绿（它验证的是自己的副本）。
    ///
    /// 公开它，测试就能走**真实**通路，删掉生产逻辑必然变红。
    pub fn collect_actions(&mut self, ctx: &egui::Context) -> Vec<UiAction> {
        self.ui.dispatcher.set_modal(self.dialog.is_modal());
        // 查找/替换输入框有焦点时，全局快捷键必须**让位**。
        //
        // 症状：用户在查找框里输入「Ctrl+B」想找字符串 "b"，
        // 结果文档里的字被加粗了——因为分发器只看按键事件，
        // 不管焦点在哪（编辑区的 `TextEdit` 也要靠快捷键，故不能一律
        // 用 `wants_keyboard_input()` 一刀切）。
        //
        // 只放行 `CloseOverlay`：Esc 必须仍然能关掉面板，
        // 否则用户会被困在查找框里（这与 S6-09「关浮层永远放行」同源）。
        let find_input_focused = ctx.memory(|m| {
            m.has_focus(egui::Id::new(FIND_INPUT_ID))
                || m.has_focus(egui::Id::new(REPLACE_INPUT_ID))
        });
        let mut out = Vec::new();
        let events = ctx.input(|i| i.events.clone());
        for event in &events {
            if let Dispatch::Triggered(action) = self.ui.dispatcher.handle(event) {
                if find_input_focused && action != UiAction::CloseOverlay {
                    continue;
                }
                out.push(action);
            }
        }
        out
    }
}

/// `UiAction` → 领域 `ActionId`（改键持久化用）。
///
/// # 为什么放在组合根而不是注册表里
///
/// 注册表在 `presentation` 层，而 `ActionId` 在 `domain`——表现层不得依赖领域
/// （`build.rs` 架构守卫会拦）。而这层映射**本来就只在组合根用得上**
/// （`EditorApp::rebind_shortcut` 一处），放这里既合法又不会让表现层沾上领域类型。
///
/// 用 `persisted_key()` 逐字比对而不是写一张对照表：那张表一旦有动作漏登记，
/// 症状是「这个动作改了键却不生效」——零报错。对照表会与 `persisted_key`
/// 一致地漏，把两种机制的错误绑在一起。
pub(crate) fn domain_action(action: UiAction) -> Option<ActionId> {
    let key = action.persisted_key()?;
    ActionId::all()
        .iter()
        .copied()
        .find(|domain| domain.key() == key)
}

/// 动作是否打开某个浮层（`UiAction` → `Overlay` 的反向映射）。
/// 与 [`Overlay::opened_by`] / [`Overlay::also_opened_by`] 互为逆映射，
/// 故用**同一张表**推导：二者不可能不一致，而不一致的后果是
/// 「某个动作落进 `_ => {}` 静默失效」——S6-07 的 `Ctrl+H`（替换）就是这样
/// 丢掉的：它既不是任何浮层的 `opened_by`，也没人接线，按下去毫无反应。
pub(super) fn openable_overlay(action: UiAction) -> Option<Overlay> {
    Overlay::ALL.iter().copied().find(|overlay| {
        overlay.opened_by() == Some(action) || overlay.also_opened_by() == Some(action)
    })
}
