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
            // 特有初始化（建议文件名 / 主题 / 默认输出目录）已下沉到
            // `frame::export::open_export_overlay`——路由表只做路由，
            // 不再顺手初始化浮层状态。
            self.open_export_overlay();
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
            // 特有初始化已下沉到 `frame::diff::open_diff_overlay`——
            // 路由表只做路由。保留上一次选的对照文件是刻意的，
            // 理由与「只在没选文件时清错误」都写在那个方法上。
            self.open_diff_overlay();
        }
    }

    /// 执行一个动作。
    ///
    /// 三个入口（菜单 / 快捷键 / 命令面板）**全部**汇聚到这里，
    /// 因此不可能出现「菜单里有的操作，快捷键触发的结果不一样」。
    ///
    /// # 分发方式：路由表 + 薄分发器（T6）
    ///
    /// 原实现是 46 分支的 `match` + `_ => {}` 兜底。兜底的真正代价不是
    /// 「多写一行」，而是**新增动作时编译器不会提醒你接线**——历史上
    /// `Ctrl+S` / `Ctrl+N` / `Ctrl+Shift+S` / `Ctrl+H` / `Ctrl+Shift+F`
    /// 都这样静默失效过（动作表里有、菜单里有、快捷键也注册了，
    /// 按下去毫无反应，零报错）。
    ///
    /// 现在这里只剩分发，每个动作的去向在 [`action_route`] 里**穷尽声明**：
    /// `UiAction` 每加一个变体，不同时更新那张表就**编译不过**。
    /// 确实暂不接线的动作必须显式写 [`ActionRoute::Unbound`]。
    pub fn execute(&mut self, action: UiAction) {
        match action_route(action) {
            // 浮层类：具体打开哪个浮层由 `Overlay::opened_by` /
            // `also_opened_by` 唯一确定，不在这张表里重复登记——
            // 两处事实必然漂移，而不一致的症状正是「某个动作静默失效」。
            ActionRoute::Overlay => {
                if let Some(overlay) = openable_overlay(action) {
                    self.open_overlay(overlay, action);
                }
            }
            // 格式类：`apply_format` 自己按 action 分派 `FormatKind`。
            ActionRoute::Format => self.apply_format(action),
            ActionRoute::GotoTab(index) => self.goto_document(index),
            ActionRoute::Handler(handler) => handler(self),
            // 显式声明的「暂不接线」，见 [`ActionRoute::Unbound`] 的文档。
            // R-04 后清单为空，此臂为机制保留。
            ActionRoute::Unbound => {}
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
            m.has_focus(egui::Id::unique(FIND_INPUT_ID))
                || m.has_focus(egui::Id::unique(REPLACE_INPUT_ID))
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

// ============================================================================
// 动作路由表（T6）
// ============================================================================

/// 一个动作的去向。
enum ActionRoute {
    /// 浮层类动作：具体打开哪个浮层由 `Overlay::opened_by` /
    /// `also_opened_by` 唯一确定，本表**不重复登记**——两处事实必然漂移，
    /// 而不一致的症状正是「某个动作静默失效」。
    Overlay,
    /// 格式类动作：统一交 [`EditorApp::apply_format`]，它自己按 action
    /// 分派 `FormatKind`。
    Format,
    /// 标签跳转（`Ctrl+1..9`）。
    GotoTab(usize),
    /// 单一处理器（统一签名的 trampoline）。
    Handler(fn(&mut EditorApp)),
    /// 动作表里有、但**尚未接线**。
    ///
    /// R-04 把 `Revert` / `InsertImage` 接线后，本变体当前**没有实例**
    /// （清单为空，见 `unbound_actions_are_explicit`）。仍保留而不是删除：
    /// 「本期确有接不了的线」是需要一个显式出口的——写在路由表里 +
    /// 在测试清单里登记，提交 diff 即可见；删掉变体只会把这条路
    /// 变成「路由到一个空 trampoline」，那正是静默失效本身。
    /// `#[allow(dead_code)]` 是「机制预留、实例为零」的显式声明，
    /// 不是对警告的静音（与 `tests/support/fake_dialog.rs` 同款理由）。
    #[allow(dead_code)]
    Unbound,
}

/// 动作 → 去向。
///
/// # 这张表**故意不写 `_ =>` 兜底**
///
/// 它的唯一价值就是「新增 `UiAction` 变体时编译失败，逼你选一个去向」。
/// 一旦写了兜底，漏接线的动作会重新退化成静默失效——那正是本表要根除的
/// 缺陷类型（历史上 `Ctrl+S` / `Ctrl+H` / `Ctrl+Shift+F` 都这样丢过）。
fn action_route(action: UiAction) -> ActionRoute {
    use UiAction as A;
    match action {
        // —— 浮层（11）——
        A::OpenDocument
        | A::Find
        | A::Replace
        | A::OpenSettings
        | A::Export
        | A::ShortcutOverview
        | A::About
        | A::DocumentStats
        | A::CommandPalette
        | A::InsertSymbol
        | A::CompareDiff => ActionRoute::Overlay,

        // —— 文件 ——
        A::Save => ActionRoute::Handler(route_save),
        A::SaveAs => ActionRoute::Handler(route_save_as),
        A::SaveAll => ActionRoute::Handler(route_save_all),
        A::NewDocument => ActionRoute::Handler(route_new_document),
        A::OpenFolder => ActionRoute::Handler(route_open_folder),
        A::CloseTab => ActionRoute::Handler(route_close_tab),

        // —— 编辑 ——
        A::Undo => ActionRoute::Handler(route_undo),
        A::Redo => ActionRoute::Handler(route_redo),
        A::Revert => ActionRoute::Handler(route_revert),
        A::FindInFiles => ActionRoute::Handler(route_find_in_files),
        A::InsertImage => ActionRoute::Handler(route_insert_image),
        A::Indent => ActionRoute::Handler(route_indent),
        A::Outdent => ActionRoute::Handler(route_outdent),
        A::MoveLineUp => ActionRoute::Handler(route_move_line_up),
        A::MoveLineDown => ActionRoute::Handler(route_move_line_down),

        // —— 格式（13）——
        A::Bold
        | A::Italic
        | A::Strikethrough
        | A::InlineCode
        | A::InsertLink
        | A::CodeBlock
        | A::InsertTable
        | A::ToggleQuote
        | A::ToggleList
        | A::ToggleOrderedList
        | A::PromoteHeading
        | A::DemoteHeading
        | A::ToggleTask => ActionRoute::Format,

        // —— 视图 / 布局 ——
        A::ToggleSidebar => ActionRoute::Handler(route_toggle_sidebar),
        A::TogglePreview => ActionRoute::Handler(route_toggle_preview),
        A::ToggleStatusbar => ActionRoute::Handler(route_toggle_statusbar),
        A::ToggleOutline => ActionRoute::Handler(route_toggle_outline),
        A::ToggleFocusMode => ActionRoute::Handler(route_toggle_focus_mode),
        A::CloseOverlay => ActionRoute::Handler(route_close_overlay),
        A::ToggleTheme => ActionRoute::Handler(route_toggle_theme),

        // —— 标签 ——
        A::NextTab => ActionRoute::Handler(route_next_tab),
        A::PrevTab => ActionRoute::Handler(route_prev_tab),
        A::GotoTab1 => ActionRoute::GotoTab(1),
        A::GotoTab2 => ActionRoute::GotoTab(2),
        A::GotoTab3 => ActionRoute::GotoTab(3),
        A::GotoTab4 => ActionRoute::GotoTab(4),
        A::GotoTab5 => ActionRoute::GotoTab(5),
        A::GotoTab6 => ActionRoute::GotoTab(6),
        A::GotoTab7 => ActionRoute::GotoTab(7),
        A::GotoTab8 => ActionRoute::GotoTab(8),
        A::GotoTab9 => ActionRoute::GotoTab(9),
    }
}

// ---------------------------------------------------------------------------
// 处理器 trampoline
//
// 统一签名 `fn(&mut EditorApp)`：路由表存函数指针而不是闭包——闭包捕获
// 环境后是匿名类型，写不进枚举字段。每个函数只有一两行，名字即语义；
// 搜 `route_save` 能同时命中路由与实现。
// ---------------------------------------------------------------------------

fn route_save(app: &mut EditorApp) {
    app.run_save(false);
}

fn route_save_as(app: &mut EditorApp) {
    app.run_save(true);
}

fn route_save_all(app: &mut EditorApp) {
    app.save_all_documents();
}

fn route_new_document(app: &mut EditorApp) {
    app.run_new_document();
}

/// 「打开文件夹」：请求工作区目录选择（后台对话框），结果由
/// `apply_dialog_outcome` 扫盘装载。入口从侧栏上移到「文件」菜单后，
/// 菜单 / 命令面板仍共用这一条路径（三入口一致）。
fn route_open_folder(app: &mut EditorApp) {
    app.request_dialog(PendingDialog::PickFolder);
}

fn route_close_tab(app: &mut EditorApp) {
    app.close_active_tab();
}

fn route_undo(app: &mut EditorApp) {
    app.undo_edit();
}

fn route_redo(app: &mut EditorApp) {
    app.redo_edit();
}

/// 「还原修改」（R-04a）：从磁盘重新加载当前文档。与 `route_undo` 是
/// 两个动作——撤销回退一步编辑，还原回到磁盘版本（脏文档先确认）。
fn route_revert(app: &mut EditorApp) {
    app.revert_document();
}

/// 「插入图片」（R-04b）：起图片选择对话框，结果回填时复制进资源目录
/// 并在光标处插入相对链接。与 `ActionRoute::Format` 的那些插入动作
/// 不同——它要弹对话框，不能同步完成，故只能是独立 Handler。
fn route_insert_image(app: &mut EditorApp) {
    app.insert_image();
}

/// 全局「在文件中查找」：**不打开浮层**——全局搜索是侧栏的 Search 标签，
/// 直接切到该标签并展开侧栏，与 `ToggleOutline` 同一思路。
fn route_find_in_files(app: &mut EditorApp) {
    app.ui.toggle_sidebar_tab(SidebarTab::Search);
}

fn route_indent(app: &mut EditorApp) {
    app.indent_selection();
}

fn route_outdent(app: &mut EditorApp) {
    app.outdent_selection();
}

fn route_move_line_up(app: &mut EditorApp) {
    app.move_line(-1);
}

fn route_move_line_down(app: &mut EditorApp) {
    app.move_line(1);
}

fn route_toggle_sidebar(app: &mut EditorApp) {
    app.ui.toggle_sidebar();
}

fn route_toggle_preview(app: &mut EditorApp) {
    app.ui.toggle_preview();
}

fn route_toggle_statusbar(app: &mut EditorApp) {
    app.ui.toggle_statusbar();
}

fn route_toggle_outline(app: &mut EditorApp) {
    app.ui.toggle_sidebar_tab(SidebarTab::Outline);
}

fn route_toggle_focus_mode(app: &mut EditorApp) {
    app.ui.toggle_focus_mode();
}

fn route_toggle_theme(app: &mut EditorApp) {
    app.cycle_theme();
}

fn route_next_tab(app: &mut EditorApp) {
    app.cycle_document(1);
}

fn route_prev_tab(app: &mut EditorApp) {
    app.cycle_document(-1);
}

/// Esc 的仲裁点在浮层状态里：先给浮层一次机会关自己，没有浮层可关时
/// 才退出专注模式。否则 Esc 会「关掉对话框顺便把专注模式也退掉」——
/// 一次按键做两件事几乎总是 bug。
fn route_close_overlay(app: &mut EditorApp) {
    if app.dialog.is_open(Overlay::Settings) {
        // Esc 语义 = 取消：与面板上的「取消」按钮、与标题栏 × 一致。
        app.cancel_settings();
    } else if app.dialog.close_top().is_none() {
        app.ui.exit_focus_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 路由表与浮层表必须描述同一件事。
    ///
    /// 两边各自维护一份「哪些动作打开浮层」就会漂移：漏的那一项在
    /// `execute` 里既不被浮层分支拦走、也没有 Handler，最终落进 Unbound
    /// 静默失效——S6-07 的 `Ctrl+H` 就是这么丢的。
    #[test]
    fn route_table_agrees_with_overlay_table() {
        for &action in UiAction::ALL {
            let routed_as_overlay = matches!(action_route(action), ActionRoute::Overlay);
            let opens_overlay = openable_overlay(action).is_some();
            assert_eq!(
                routed_as_overlay, opens_overlay,
                "动作 {action:?} 的路由（overlay={routed_as_overlay}）与 \
                 Overlay::opened_by/also_opened_by（overlay={opens_overlay}）不一致"
            );
        }
    }

    /// 「未接线」必须是一份**显式清单**，不能悄悄变长。
    ///
    /// 这条断言的作用不是禁止新增 Unbound，而是让每一次新增都必须在
    /// 测试里改一行——提交 diff 里就能看见「又有一个动作没接线」。
    ///
    /// R-04 接线 `Revert` / `InsertImage` 后清单**清零**；清零不等于
    /// 删机制——`ActionRoute::Unbound` 变体与 `execute` 的对应臂都留着，
    /// 下一位「本期接不了」的动作从那里显式登场。
    #[test]
    fn unbound_actions_are_explicit() {
        let unbound: Vec<UiAction> = UiAction::ALL
            .iter()
            .copied()
            .filter(|&action| matches!(action_route(action), ActionRoute::Unbound))
            .collect();
        assert_eq!(
            unbound,
            Vec::<UiAction>::new(),
            "未接线动作清单变了：新增 Unbound 时必须同步更新本断言，\
             并在提交说明里交代为什么暂不接线"
        );
    }

    /// 每个动作都必须有去向，且 `Unbound` 之外的分支都能真正执行。
    ///
    /// 穷尽性本身由编译器保证（`action_route` 没有 `_` 兜底）；
    /// 这里额外钉住「`UiAction::ALL` 与枚举变体同步」——`ALL` 漏登记
    /// 会让命令面板少一项，而它不会报错。
    #[test]
    fn every_action_is_routed() {
        assert_eq!(UiAction::ALL.len(), 57, "UiAction::ALL 与枚举变体数不一致");
    }
}
