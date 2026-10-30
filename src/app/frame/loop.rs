//! eframe 帧循环、启动入口与字体装配——「一帧做什么」的唯一定义处。
//!
//! # 为什么单独成文件
//!
//! 帧内步骤的**顺序本身就是约束**：字体要在本帧任何排版之前装好
//! （`set_fonts` 晚了会先画一帧方框）、截图回路必须在绘制之后
//! （egui 帧末才把 `Screenshot` 交给后端）、帧末预约必须等所有 `arm` 都登记完。
//! 这些只能靠一口气读完这段代码来确认，不能和 40 多个字段的定义混在一起。
//!
//! 启动入口 `run` 与字体装配也在这里：三者共享同一个前提「egui 上下文已就绪」，
//! 且 `ensure_editor_fonts` 由帧循环每帧调用。模块名用 raw identifier
//! （`r#loop`），因为 `loop` 是关键字。

use super::*;

/// IME 组合期保护窗口（毫秒）。
///
/// # 取值理由
///
/// 它必须同时满足两个约束：
///
/// 1. **够长**：真实中文输入从敲下第一个字母到按下候选确认，人可以慢到
///    几百毫秒。每个字母都是一次 `Preedit`，会把窗口**续上**（不是从首次
///    `Enabled` 起一次性计时），故单次只需覆盖「两次击键的间隔」。
///    取 800ms：远大于普通击键间隔（~100–300ms）。
/// 2. **够短**：一旦 `Commit`/`Disabled` 丢失（切输入法、直接关候选窗、焦点
///    落到浮层），冻结必须在可感知的短时间内自行解除，否则用户的回车会被吞
///    ——那正是本常量要根治的缺陷。见 [`EditorApp::apply_pending_editor_state`]。
pub(crate) const IME_GUARD_MS: u64 = 800;

/// 「egui 已 armed IME」的保护窗口（毫秒）。
///
/// 与 [`IME_GUARD_MS`] 分开取值，因为两者要防的故障相反：
///
/// * [`IME_GUARD_MS`] 管**我们自己的守卫**——宁长勿短，冻结久一点最多让
///   光标晚一帧回设，绝不能中途解冻（中途解冻会打掉 egui 的预编辑选区）。
/// * 本常量管**复位 egui 的 `ime_enabled`**——宁短勿长。窗口过长就等于
///   「冻结回车」，而回车正是用户最高频的按键。
///
/// 取 50ms：真实组合里 `Enabled` 与首个 `Preedit` 之间的间隔不到一帧
/// （~16ms），50ms 留了三帧余量；连按回车的人手间隔 ≥80ms，
/// 故第一次回车就已出窗、立即恢复。
///
/// 出窗那一帧由 [`EditorApp::apply_pending_editor_state`] 补发 `Disabled`
/// 复位 egui，并**同时抹平本帧 Enter 的 `repeat`**——两者缺一不可：
/// `remove_ime_incompatible_events` 在 `TextEdit::show` 内部、按当帧仍为真
/// 的 `ime_enabled` 剔除 repeat 按键，晚一步的 `Disabled` 救不回它。
pub(crate) const IME_ARM_MS: u64 = 50;
/// 浮层关闭后无需显式归还焦点：早期版本曾实现「焦点归还到触发按钮」机制，
/// 但对应的 `overlay_focus_sink` 控件标识从未在任何视图里注册、
/// `take_focus_return` 也从未被调用——是一套空转的半套机制。现已移除，
/// 浮层关闭后焦点按 egui 默认行为处理即可。
impl eframe::App for EditorApp {
    // egui 0.37 把旧的 `update` 拆成 `logic`（窗口隐藏时也要跑的状态更新）
    // 与 `ui`（仅绘制）。本项目所有状态更新都依赖绘制结果（如拖放命中、快捷键
    // 触发、截图回传），且旧 `update` 就是「每帧当要重绘时调用」，与 `ui`
    // 的语义一致，故全部逻辑统一收进 `ui`，不实现空的 `logic`。
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 状态更新阶段：需要 `ctx`。这段借用必须**在 `main_view::show` 之前结束**，
        // 因为绘制阶段要的可变 `ui` 与这里的不可变 `ctx` 借用冲突。
        {
            let ctx = ui.ctx();

            // 0) 帧首清空到期点登记表（T11）。必须在任何 `arm` 之前：
            //    到期点是本帧依据当前状态重新算出的，不是跨帧状态。
            self.scheduler.begin(self.services.markdown.now().millis());

            // 1) 「跟随系统」在主题落地**之前**刷新：本帧必须用最终主题画，
            //    否则系统外观变化的那一帧会闪一下旧配色。
            self.refresh_system_theme(ctx);
            mapper::apply(ctx, &self.ui.theme);

            // 1.5) 编辑区字体：设置页改过（或首帧）时重装。
            //      放在任何控件布局之前——`set_fonts` 要赶在本帧排版之前生效。
            //      内部按「键没变就跳过」短路，未改字体时每帧只多一次字符串比对。
            self.ensure_editor_fonts(ctx);

            // 2) 收后台对话框的结果。
            //
            //    放在快捷键**之前**：用户刚在系统对话框里选了文件，
            //    这一帧就应该把文件打开，而不是先让窗口里残留的按键事件
            //    触发别的动作。
            if let Some(outcome) = self.dialogs.poll() {
                self.apply_dialog_outcome(outcome);
            }

            // 2.1) 工作区后台扫描结果轮询（PickFolder 首次扫描路径异步化）。
            //
            //     扫描是磁盘 IO，大目录会阻塞 UI 帧循环——用户选完文件夹后窗口
            //     会「冻住」几百毫秒到几秒（视目录规模而定）。改成后台线程后，
            //     用户选完文件夹立刻回到 UI 继续操作，侧栏短暂显示 loading 态。
            //     领域层 scan_tree（RwLock 写入 + EventBus publish）线程安全，
            //     放后台无副作用。其他扫描路径（reload / refresh）保持同步。
            self.poll_scan_result();

            // 2.2) 高亮后台补算结果轮询（P1）。
            //
            //     停手后那次全文 syntect 解析（10KB ≈ 64ms）已交给后台线程
            //     （`core/application/markdown/service/highlight_worker`）。这里每帧
            //     非阻塞取一次结果：取到就落进高亮缓存，本帧绘制即可上色；
            //     没取到立即返回（**不做任何等待**）。
            //
            //     放在绘制之前是关键：`editor_view` 在本帧稍后才会向服务要高亮
            //     片段，结果先进缓存，颜色就赶得上这一帧，而不是下一帧。
            self.services.markdown.poll_highlight_results();

            // 2.5) 文件拖放：把 .md 文件拖入窗口即打开（P2 拖拽打开）。
            //      放在快捷键之前，与对话框结果同级——拖入文件即开，不被残留按键干扰。
            self.handle_dropped_files(ctx);

            // 3) 快捷键 → 动作。
            for action in self.collect_actions(ctx) {
                self.execute(action);
            }

            // 3.5) 自动保存（DEF-02 修复；问题 068 补「到期帧」调度）：
            //      周期把脏文档写入草稿，崩溃后可恢复；没有输入事件时由
            //      `request_repaint_after` 保证到期时刻仍有一帧来执行。
            self.run_autosave();

            // 3.6) 高亮的「到期帧」（T11）：`highlight` 在防抖窗口内返回空片段，
            //      必须有人在窗口到期时再跑一帧才补得上颜色。编辑器失焦后没有
            //      光标闪烁兜底，缺这行语法色会一直停在无色状态。
            //      P1 起这条还兼顾「后台补算在途」：结果没回来就按固定间隔
            //      预约下一次轮询（否则颜色要等下一次输入才出现）。
            //      到期点由 `HighlightPace` 与在途请求数共同给出。
            if let Some(due_ms) = self.services.markdown.highlight_next_due_in() {
                self.scheduler.arm(Deadline::HighlightDebounce, due_ms);
            }

            // 3.9) 耗时归因的第一段边界（D-5）：从帧首到这里都算「状态更新」。
            //      放在块尾是唯一没有遗漏的位置——块内每一步都可能 `arm`，
            //      也都在绘制之前。
            self.scheduler.lap(FramePhase::StateUpdate);
        }

        // 4) 绘制。视图层在 S6-02~S6-11 逐步接入。
        //    egui 0.37 起根 `Ui` 已自带全窗口可用区，Panel 全部从 `ui` 切分。
        // 侧栏开合的落盘在主视图帧末完成（见 `main_view::show`）——
        // 那里才是「本帧所有点击都处理完」的位置。
        crate::presentation::views::main_view::show(self, ui);

        // 4.05) 耗时归因的第二段边界（D-5）：`main_view::show` 整棵视图树 = 绘制。
        //       下一行是 `flush`，两者之间的差就是本帧的「未归因」尾巴。
        self.scheduler.lap(FramePhase::Draw);

        // 4.07) 绘制期**刚发起**的高亮补算也要预约（P1）。
        //
        //       3.6 的登记在绘制**之前**，那时编辑区还没查询高亮，本帧新发起的
        //       补算不在表里。对「停手补算」不要紧——那一刻防抖窗口本身就到点了，
        //       3.6 已按窗口登记；但**首次见某文档**（打开 / 切换）既没有到期窗口、
        //       也没有更早的在途请求，3.6 什么都不登记。少了这一处，那次补算的
        //       结果就只能等下一次输入带来的帧才被取走——用户打开文件后走开，
        //       语法色会一直停在无色。
        //
        //       下一帧起由 3.6 接管（在途 > 0 → 4ms 后轮询），结果落库即自动消失。
        //       这处补登记是对「egui 首帧自带若干重绘请求」的兜底：不能依赖它。
        if let Some(due_ms) = self.services.markdown.highlight_next_due_in() {
            self.scheduler.arm(Deadline::HighlightDebounce, due_ms);
        }

        // 4.1) 帧末统一预约（T11）：本帧所有 `arm` 过的到期点在此换算成
        //      **一次** `request_repaint_after`。位置必须在 `main_view::show`
        //      之后——状态栏的统计防抖与消息 TTL 是在绘制阶段登记的。
        // 返回值只在「本帧超预算且未被限流」时给出归因；告警已由调度器自己写日志，
        // 组合根不需要再消费（`Option` 是 `must_use`，故显式丢弃）。
        let _ = self.scheduler.flush(ui.ctx());

        // 4.5) 草稿恢复提示（DEF-02）：启动发现遗留草稿时居中提示。
        {
            let ctx = ui.ctx();
            self.render_recovery_prompt(ctx);

            // 4.6) 图片导出回路（P2）。必须在**绘制之后**：egui 在帧末才把
            //      `ViewportCommand::Screenshot` 交给后端，帧内发命令、
            //      收上一帧的回传，两件事分处两帧才闭环。
            self.pump_capture(ctx);
        }

        // 4.7) 冷启动埋点（S8-03）：首帧画完 = 用户看到窗口。
        // 未开启环境变量时是空操作，不影响正常启动路径。
        crate::app::startup_timing::mark_first_frame_and_report();
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        // 窗口几何与面板开关在 S6-02 用 eframe storage 持久化；
        // 业务配置由 `PreferencesAppService` 自行落盘，不走这里。
    }
}

/// 窗口 / 任务栏图标。
///
/// 与 exe 资源段（`assets/logo/rustmark.rc` → `rustmark.ico`）同源，都由
/// `assets/logo/build_logo.py` 生成：资源段管资源管理器与「固定到任务栏」，
/// 这里管窗口标题栏、任务栏按钮与 alt-tab。取 256px——系统从这一份缩放出
/// 各档尺寸。解码失败回落到 eframe 默认图标而不是 panic：图标是体验项。
fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!(
        "../../../assets/logo/png/rustmark-icon-256.png"
    ))
    .unwrap_or_default()
}

/// 以给定 `Services` 启动窗口。
pub fn run(services: Services) -> eframe::Result {
    let system_dark = false;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("rustmark")
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "rustmark",
        options,
        Box::new(move |cc| {
            // 这个闭包被调用时，事件循环 / 窗口 / 渲染器都已就绪（`cc` 就是凭证）。
            // 因此「装配完成 → 窗口就绪」这一段 = eframe 建窗口 + wgpu 初始化，
            // 「窗口就绪 → 应用构造完成」= `EditorApp::new` 自身的开销。
            // 不拆这一刀的话，冷启动的大头会被误记到 `EditorApp::new` 头上。
            crate::app::startup_timing::mark("窗口就绪");

            // 应用先构造（它持有持久化配置，字体选择要以配置为权威来源），
            // 再装字体——但**仍然在第一帧绘制之前**：eframe 绘制首帧前会先
            // 走完创建闭包，故用户上次选的字体会从第一帧就生效，不会出现
            // 「先用默认字体画一帧（中文全为方框）、第二帧再跳变」。
            let mut app = EditorApp::new(services, system_dark);
            crate::app::startup_timing::mark("应用构造完成");
            app.ensure_editor_fonts(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

impl EditorApp {
    /// 登记状态栏消息的 TTL 到期点（T11）。
    ///
    /// 由状态栏视图在**取出未过期消息之后**调用——那一刻它才知道消息还剩多久。
    /// 做成方法而不是把 `scheduler` 字段开给视图：`arm` 是帧内可以开放的动作，
    /// `flush` 不是，字段私有才能保证「预约只有一个出口」。
    ///
    /// 消息槽为空时是空操作：没有消息就没有要叫醒的时刻。
    pub(crate) fn arm_status_message_deadline(&mut self) {
        if let Some(expires_at) = self.status_bar.message.as_ref().map(|m| m.expires_at) {
            self.scheduler.arm(Deadline::StatusMessage, expires_at);
        }
    }

    /// 本帧已登记的到期点 `(短名, 到期毫秒)`（测试与调试用）。
    ///
    /// 存在理由与 `highlight_pace_snapshot` 同类：回答「这个定时功能到底有没有
    /// 接进帧循环」。没有它，只能靠真机等 150ms 看字色有没有恢复来间接推断。
    pub fn scheduled_deadlines(&self) -> Vec<(&'static str, u64)> {
        self.scheduler.entries()
    }

    /// 装配界面字体：在 egui 默认字体族上追加系统 CJK 字体，并把用户选中的
    /// 等宽字体置顶进内置 `Monospace` 族。
    ///
    /// # 为什么必须做
    ///
    /// egui 内置字体**不含任何 CJK 字形**，不追加中文字体时界面上的中文
    /// 会全部渲染为占位方框。这是必然结果，不是偶发故障。
    ///
    /// # 缺字体时的取舍
    ///
    /// 找不到中文字体时**不让应用启动失败**——界面仍可正常使用，只是中文显示为方框。
    /// 但要留下**明确的警告**：否则用户只会看到一屏方框而无从归因，
    /// 更想不到是"系统缺中文字体"这种罕见原因。
    ///
    /// 走 `log::warn!`：F-2 起 `main` 注册了 env_logger 后端（默认级别 warn），
    /// 该告警默认可见；`RUST_LOG` 可调级别。**仅在失败时出声**——
    /// 正常启动（找得到中文字体时）没有任何输出。
    ///
    /// # 何时重装
    ///
    /// 每帧调用（帧循环开头与启动各一次）：内部只做一次字符串比对，
    /// 命中即返回。重装有真实代价——`set_fonts` 会触发 egui 整体重建
    /// 字体图集（一次可见卡顿），故绝不在「没变」时触发。
    fn ensure_editor_fonts(&mut self, ctx: &egui::Context) {
        let selected = self.settings().theme.font_family.clone();
        if self.installed_font_family.as_deref() == Some(selected.as_str()) {
            return;
        }
        self.install_editor_fonts(ctx, &selected);
    }

    /// 真正重装字体（选中项变化 / 首帧时）。
    fn install_editor_fonts(&mut self, ctx: &egui::Context, selected: &str) {
        let setup = crate::infrastructure::fonts::font_definitions(Some(selected));
        if setup.cjk_source.is_none() {
            // 系统没中文字体不等于中文变方框了——内置 Noto Sans SC 子集
            // （GB2312）会兜底。仍要出声：用户该知道观感来自内置子集，
            // 且 GB2312 之外的生僻字在此环境下仍会是方框。
            log::warn!(
                "未找到系统中文字体，已改用内置 Noto Sans SC 子集（GB2312）渲染中文。\
                 常用汉字不受影响；生僻字在此环境下仍可能显示为方框。\
                 装一副中文字体即可获得完整覆盖（Windows 通常自带微软雅黑；\
                 Linux 可安装 fonts-noto-cjk；macOS 应自带苹方）"
            );
        }
        ctx.set_fonts(to_egui_font_definitions(setup.definitions));
        // 清单同步刷新：用户可能往项目 fonts/ 目录放了新字体后重启，
        // 那次重启会走这里，下拉里立刻能选到。
        self.editor_fonts = setup.editor_fonts;
        self.installed_font_family = Some(selected.to_string());
    }
}
