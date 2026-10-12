//! IME 组合期守卫：光标回设与撤销栈清理的冻结/解冻。
use super::*;
use crate::app::frame::{IME_ARM_MS, IME_GUARD_MS};

impl EditorApp {
    /// 把上一帧领域编辑留下的「重设光标 / 清撤销栈」意图落到 egui 内部状态。
    ///
    /// 必须在 `TextEdit::show` **之前**调用（写入的状态会在本帧 show 时被读走）。
    /// 放在编辑区视图顶部，由视图持 `ctx` 调用。
    ///
    /// # IME 组合期守卫（为什么这里要扫一遍输入事件）
    ///
    /// egui 0.33 的 `Preedit` 处理会把预编辑串**真实写进文本缓冲区**
    /// （`text.insert_text_at`——「组合期文本不进缓冲区」的旧假设不成立），
    /// 并靠 `state.cursor` 保持着「(组合锚点, 预编辑串尾)」这一选区：
    /// 下一次 `Preedit` 靠它删掉旧串，`Commit` 还会校验
    /// `cursor_range.secondary`（锚点）是否仍在组合起点，不在则**整个
    /// 提交被静默忽略**。而本方法回设的 `pending_cursor` 是折叠光标
    /// （secondary = 串尾），恰好打掉这个选区——实测症状：
    /// 预编辑串逐帧累积（"s"→"ssa"→"ssasadsadaf"），候选词确认后中文
    /// 根本进不来，之后每次回车只是把残渣往下推。
    ///
    /// 故组合期间（以及恰好携带 IME 事件的帧）**冻结**两个动作：
    /// 不回设光标、不清撤销栈；组合结束（`Commit` / `Disabled`）后的
    /// 下一帧恢复正常回设——彼时 `pending_cursor` 已被提交帧的
    /// `flush_edit` 覆写为「中文之后」的正确位置。
    ///
    /// # 为什么冻结必须**有界**（V11-14：连按回车只出一个新行）
    ///
    /// 旧实现用粘性标志 `ime_composing` 表示组合期，只有 `Commit`/`Disabled`
    /// 能清它。真实输入法在编辑区获得焦点时就发 `Ime::Enabled`，而 winit 的
    /// `Disabled` 只在失焦时发——切换输入法、直接关候选窗、焦点落到浮层等
    /// 路径都可能收不到。标志一旦卡在 `true`：
    ///
    /// ```text
    /// 回车 → flush_edit 写 pending_cursor → 守卫冻结，光标不更新
    ///      → 领域多了一个 \n，光标却仍在旧位置
    /// 再回车 → egui 把 \n 插到旧位置 → 再 flush_edit → 又是旧位置……
    /// ```
    ///
    /// 于是**连按回车只有第一个生效**，界面只多出一个新行；直到用户敲了别的
    /// 字符，`prev != editor_text` 的判定路径把它「带出来」，观感就是
    /// 「必须再输入字符才会真正换行」。实测逐字复现（见
    /// `enter_diag_test::diag_enter_after_ime_enabled_never_disabled`：
    /// 第 2、3 次回车后 `domain` 纹丝不动停在 `"hello\n"`）。
    ///
    /// 改为**有界窗口**后，组合期最长只冻结 [`IME_GUARD_MS`] 毫秒：
    /// 正常中文输入（逐字母 `Preedit`、几十到几百毫秒内 `Commit`）完全
    /// 落在窗口内、行为不变；而「标志卡死」在结构上不再可能——超时自动恢复。
    ///
    /// # 超时时还必须**顺带复位 egui 自己的 IME 标志**（这才是回车被吞的真凶）
    ///
    /// 光有「自己的守卫解冻」是不够的：`Ime::Enabled` 会让 egui 的
    /// `TextEdit` 把 `state.ime_enabled` 置真，此后它按「候选窗开着」的
    /// 假设处理键盘——`remove_ime_incompatible_events` 会剔除它认为
    /// 「IME 期间会打架」的按键（`repeat: true` 的全部按键 +
    /// Backspace/方向键）。该标志**只能**由 `Preedit` / `Commit` /
    /// `Disabled` 或焦点变化复位，而 `Disabled` 在真实环境里常常收不到。
    ///
    /// 于是窗口到期时，除了解冻自己的守卫，还要给 egui **补一个**
    /// `Ime::Disabled`——语义上完全正确（应用确实判定「组合已结束」），
    /// 且这是 egui 唯一公开的复位入口（`ime_enabled` 是 `pub(crate)`，
    /// `request_focus` 对已聚焦控件幂等、不产生 `gained_focus`，都试过了）。
    /// 补发后实测连按回车立刻恢复正常（见
    /// `enter_diag_test::diag_synthetic_disabled_resets`）。
    ///
    /// # Commit 不得续期冻结窗（2026-10-11「行号递增但内容不换行」复报的根因）
    ///
    /// 旧实现把 `Commit` 与 `Preedit` 同列为「组合进行中」，收到 `Commit` 会把
    /// [`IME_GUARD_MS`] 保护窗**再续 800ms**。但 `Commit` 的语义是「组合结束」，
    /// egui 在它的处理分支里已经把 `ime_enabled` 置回 false 并删掉了预编辑
    /// 选区。续期的后果：
    ///
    /// ```text
    /// 中文提交 → 守卫被 Commit 续到 +800ms
    /// 用户立刻在列表项上回车（transformed=true）→ flush_edit 写下
    ///     pending_cursor（"2. " 之后），但守卫冻结、光标回不去
    /// 下一帧 egui 光标仍停在 "\n" 之后、"2. " 之前
    /// 再回车 → "\n" 插到 "2. " 前面 → transform_insert 又续一个标记
    ///        → "1. 中文\n2. \n2. "…… 标记/空行逐次堆积
    /// ```
    ///
    /// 症状正是「行号正常递增，光标附近却不见新行」。修复：`Commit` 与
    /// `Disabled` 同样**立即**解冻（仅保留事件帧当帧的冻结，供 egui 的
    /// Commit 处理器依赖未碰的选区），组合结束后光标回设最迟下一帧执行。
    pub fn apply_pending_editor_state(&mut self, ctx: &egui::Context) {
        let now_ms = self.services.markdown.now().millis();
        // 本帧 IME 事件的四分类：
        //   · `Preedit` → 组合**确实在进行**：缓冲区里有实打实的预编辑串，
        //     egui 正靠那个选区删旧换新，我们的守卫必须冻结。
        //   · `Commit`/`Disabled` → 组合**收尾**，立即解冻。`Commit` 不得续期
        //     冻结窗——它不是「组合进行中」，续期会让提交后 800ms 内所有
        //     transformed 编辑的光标回设被冻结，见函数文档的堆积推演。
        //   · `Enabled` → 只表示「egui 已把 `ime_enabled` 置真」，本身不代表有
        //     组合在进行，但它**会**改变 egui 处理键盘的方式，故要单独跟踪
        //     （见 `egui_ime_armed`）。
        let (ime_preedit_now, ime_commit_now, ime_ended, ime_enabled_now) = ctx.input(|i| {
            let mut preedit = false;
            let mut commit = false;
            let mut ended = false;
            let mut enabled = false;
            for event in &i.events {
                if let egui::Event::Ime(ime) = event {
                    match ime {
                        egui::ImeEvent::Preedit(_) => preedit = true,
                        egui::ImeEvent::Commit(_) => commit = true,
                        egui::ImeEvent::Disabled => ended = true,
                        egui::ImeEvent::Enabled => enabled = true,
                    }
                }
            }
            (preedit, commit, ended, enabled)
        });
        // ---- 守卫侧：组合期是否有界 ----
        //   · 收到 Commit/Disabled → 立即解除冻结（组合确实结束了）；
        //   · 收到 Preedit → 续上/开启一个 IME_GUARD_MS 的保护窗；
        //   · 只收到 Enabled 或什么都没收到 → 不进入（或退出）组合期。
        // 于是「守卫标志永久为 true」这条路径在结构上不再存在；且 Commit 不再
        // 续期——组合结束后 `pending_cursor` 的回设最迟下一帧就能执行。
        if ime_ended || ime_commit_now {
            self.ime_composing = false;
            self.ime_composing_until_ms = 0;
        } else if ime_preedit_now {
            self.ime_composing = true;
            self.ime_composing_until_ms = now_ms.saturating_add(IME_GUARD_MS);
        } else if self.ime_composing && now_ms >= self.ime_composing_until_ms {
            self.ime_composing = false;
            self.ime_composing_until_ms = 0;
        }
        // ---- egui 侧：复位它的 `ime_enabled` ----
        // 这个标志由 `Ime::Enabled` 置真，**同样只由** `Preedit`/`Commit`/
        // `Disabled`/焦点变化复位。`Disabled` 收不到时它就一直为真，
        // 于是 egui 按「候选窗开着」剔除按键（`remove_ime_incompatible_events`），
        // 回车被吞——这才是「连按回车只多出一个新行」的直接原因。
        //
        // 复位时机刻意取得**很激进**（[`IME_ARM_MS`]，比组合期窗口短一个量级）：
        // 真实组合中，`Enabled` 之后**紧接着**就是第一个 `Preedit`
        // （winit 在 `Preedit` 分支里也会调 `ime_event_enable`），二者间隔
        // 不到一帧（~16ms）。所以只要「`Enabled` 之后短时间没有任何
        // `Preedit`」，就说明并没有组合在进行——这是判据，不是猜测。
        // 用长窗口反而会重蹈「标志卡住」的覆辙：800ms 内用户的每一次回车
        // 都会被吞，而用户连按回车的间隔恰好在这个量级。
        //
        // 误伤成本也可控：万一真在组合中而我们补了 `Disabled`，egui 只是把
        // `ime_enabled` 提前清掉；下一个 `Preedit` 会重新置位并继续组合，
        // 最多丢一次预编辑串的锚点——而这正是旧实现**常态性**发生的损害。
        //
        // `Commit` 同样解除 armed：egui 在 Commit 分支里自己就把 `ime_enabled`
        // 置回了 false（text_edit/builder.rs 的 `ImeEvent::Commit` 臂），
        // 之后无需、也不应再补发 Disabled。
        let mut tell_egui_ime_over = false;
        if ime_ended || ime_commit_now {
            self.egui_ime_armed = false;
        } else if ime_preedit_now || ime_enabled_now {
            // 刚 `Enabled`：给一个短窗口等紧随其后的 `Preedit`。
            self.egui_ime_armed = true;
            self.egui_ime_armed_until_ms = now_ms.saturating_add(IME_ARM_MS);
        } else if self.egui_ime_armed && now_ms >= self.egui_ime_armed_until_ms {
            self.egui_ime_armed = false;
            tell_egui_ime_over = true;
        }
        // 给 egui 补一个 `Disabled`。放在事件流**尾部**：本帧若同时有
        // `Preedit`，必须让它先被处理（`Preedit` 会重新置 `ime_enabled = true`），
        // 否则这个 `Disabled` 会被随后的 `Preedit` 覆盖回去。
        if tell_egui_ime_over {
            ctx.input_mut(|i| i.events.push(egui::Event::Ime(egui::ImeEvent::Disabled)));
        }
        // Enabled 之后尚未见到任何组合事件（Preedit/Commit）的窗口期内，
        // egui 的 `ime_enabled` 为真，`remove_ime_incompatible_events` 会剔除
        // 全部 `repeat: true` 的按键——它的假设是「候选窗开着」。但我们知道
        // **没有组合在进行**（armed 判据的存在意义），此时 repeat 的回车只会
        // 来自「上一帧 release 丢失后 `keys_down` 残留」（Windows 上候选窗或
        // 其他窗口抢焦点时会这样），或用户真的按住回车——两种情况用户都
        // 期待换行。把 repeat 抹平，让 egui 当作一次普通按下处理。
        // 只抹 Enter、且仅在「无组合」窗口内：组合期（候选窗真开着）的
        // repeat 剔除是 egui 的正当防护，不能碰。
        if !ime_preedit_now && !ime_commit_now && !ime_ended && self.egui_ime_armed {
            ctx.input_mut(|i| {
                for event in &mut i.events {
                    if let egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        repeat,
                        ..
                    } = event
                    {
                        *repeat = false;
                    }
                }
            });
        }
        // 组合帧自身也要冻结：`Preedit`/`Commit` 处理器在这一帧稍后执行，
        // 依赖的正是那个未被我们碰过的选区。
        //
        // 注意：**刚补发的 `Disabled` 不参与冻结判定**——它是我们自己
        // 判定「组合已结束」后才补的，若把它算进去会让本帧凭空多冻结一次，
        // 回车又要晚一帧才生效。
        let ime_guard = ime_preedit_now || ime_commit_now || self.ime_composing;

        if self.clear_undoer_next
            && !ime_guard
            && let Some(mut state) = egui::TextEdit::load_state(ctx, Self::editor_id())
        {
            self.clear_undoer_next = false;
            state.clear_undoer();
            state.store(ctx, Self::editor_id());
        }
        if !ime_guard
            // 注意 `&&` 的顺序：必须先拿到 `state` 再 `take()`。
            // 写成 `take()` 在前时，`load_state` 返回 `None`（TextEdit 尚未
            // 建立状态的那一帧）会把 `pending_cursor` 白白吞掉，光标从此失联。
            && let Some(mut state) = egui::TextEdit::load_state(ctx, Self::editor_id())
            && let Some((cursor, anchor)) = self.pending_cursor.take()
        {
            let primary = egui::text::CCursor {
                index: cursor,
                prefer_next_row: false,
            };
            let secondary = egui::text::CCursor {
                index: anchor,
                prefer_next_row: false,
            };
            state.cursor.set_char_range(Some(egui::text::CCursorRange {
                primary,
                secondary,
                h_pos: None,
            }));
            state.store(ctx, Self::editor_id());
        }
    }
}
