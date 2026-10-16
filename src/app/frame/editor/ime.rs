//! IME 组合期守卫：光标回设与撤销栈清理的冻结/解冻。
use super::*;
use crate::app::frame::{IME_ARM_MS, IME_GUARD_MS};

impl EditorApp {
    /// 组合期预编辑串在 `editor_text` 里的**字符区间**（无组合 / 读不到状态时为 `None`）。
    ///
    /// egui 0.37 把预编辑串真实插进 TextEdit 的缓冲区，并在处理完 `Preedit`
    /// 时把状态里的光标区间设成该串（`cursor_purpose = ImeComposition`）。
    /// 本方法由 [`Self::apply_pending_editor_state`] 调用、跑在 `TextEdit::show`
    /// **之前**，读到的正是上一帧留下的那个区间；`CharIndex` 是**字符**下标
    /// （epaint 0.37 明确区分 `CharIndex` / `ByteIndex`），可与 `chars()` 配对。
    fn composition_char_range(&self, ctx: &egui::Context) -> Option<std::ops::Range<usize>> {
        let state = egui::TextEdit::load_state(ctx, Self::editor_id())?;
        let range = state.cursor.char_range()?;
        let a = range.primary.index.0;
        let b = range.secondary.index.0;
        Some(a.min(b)..a.max(b))
    }

    /// 当前预编辑串的内容（无组合时为空串）。
    ///
    /// 供「裸按键废弃组合」兜底补发的 `Commit` 使用：egui 0.37 的 Commit 分支
    /// 会先删掉该区间再插回提交文本，故只有回提交**同一段文本**才能得到
    /// 「折叠选区、文本不变」的净效果（详见 `apply_pending_editor_state`）。
    fn composition_text(&self, ctx: &egui::Context) -> String {
        let Some(r) = self.composition_char_range(ctx) else {
            return String::new();
        };
        self.editor_text
            .chars()
            .skip(r.start)
            .take(r.end - r.start)
            .collect()
    }

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
    ///
    /// # armed 到期帧必须**同时抹平 repeat 回车**（2026-10-12 暴力扫描发现）
    ///
    /// egui 的 `remove_ime_incompatible_events` 位于 `TextEdit::show`
    /// **控件内部**（`text_edit/builder.rs` 的 `events()`），判据是当帧的
    /// `state.ime_enabled`——它在 show 内、于处理任何事件**之前**执行，
    /// 把所有 `repeat: true` 的按键连同 Backspace/方向键一起剔除；IME 事件
    /// 的「排到前面」发生在剔除**之后**。于是 armed 到期那一帧：
    /// `egui_ime_armed = false` 先执行 → 抹平条件不成立 → 本帧补发的
    /// `Disabled` 排在事件流尾部、排序后才生效 → `ime_enabled` 仍为真时
    /// repeat 回车已被删。实测：`Enabled` 后 release 丢失（Windows 上候选窗
    /// 抢焦点吞 keyup，`keys_down` 残留使下一次按下被判 repeat），第二次
    /// 回车在到期帧被整个吞掉，内容停在 `"hello\n"`——用户看到的正是
    /// 「连按回车只多出一个新行」。修复：抹平判据从「armed 仍为真」改为
    /// 「armed 为真 **或** 本帧即将补发 Disabled」。
    ///
    /// # 组合仍新鲜时**不得补发 Disabled**，裸按键要**废弃组合**（同次扫描）
    ///
    /// armed 窗（[`IME_ARM_MS`]）比组合窗（[`IME_GUARD_MS`]）短一个量级，
    /// 而真实组合里用户斟酌候选的击键间隔就超过 50ms——旧实现在这段间隔里
    /// 补发 `Disabled`，把 egui 的 `ime_enabled` 提前清零，**预编辑选区却
    /// 还挂在缓冲区里**：随后每一次回车/打字都先 `delete_selected` 把它整个
    /// 吃掉，而预编辑串早已通过 diff 路径落进领域、用户看得见——内容凭空
    /// 丢失。两处修复：
    ///
    /// 1. armed 到期时若 [`Self::ime_composing`] 仍为真，只把 armed 窗续到
    ///    组合窗到期，不补 `Disabled`；
    /// 2. 组合窗内到达**裸按键/裸文本**（本帧无任何 IME 事件——真实输入法
    ///    在组合期会自己吞掉按键转成 `Commit`，裸按键到达即组合已废），补
    ///    一个空 `Commit("")`：egui 的 Commit 分支对空串的处理正是「折叠
    ///    光标到 primary」，预编辑串由此转为普通文本；同时解除两扇守卫，
    ///    让本次按键的 `pending_cursor` 回设不再被冻结。
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
                    #[allow(deprecated)]
                    match ime {
                        egui::ImeEvent::Preedit { .. } => preedit = true,
                        egui::ImeEvent::Commit(_) => commit = true,
                        egui::ImeEvent::Disabled => ended = true,
                        egui::ImeEvent::Enabled => enabled = true,
                        egui::ImeEvent::DeleteSurrounding { .. } => {}
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
        //
        // 先记下**本帧清算之前**的守卫状态：下面的「裸按键废弃组合」兜底必须
        // 用它、而不是清算后的 `self.ime_composing`——否则「到期帧恰好来按键」
        // 时兜底会被跳过（见 `probe_enter_during_active_composition_preserves_preedit`
        // gap=900 的实测：预编辑串 "sheng" 被 TextEdit 的 delete_selected 吃掉）。
        let composing_before = self.ime_composing;
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
            if self.ime_composing {
                // 组合窗（[`IME_GUARD_MS`]）还没到期 → 真实的 `Preedit` 序列
                // 仍在继续（用户斟酌候选词的击键间隔就超过 [`IME_ARM_MS`]）。
                // 此时**绝不能**补 `Disabled`：egui 的预编辑选区还挂在缓冲区
                // 里，`ime_enabled` 被提前清零后，TextEdit 对后续按键的
                // `delete_selected` 会把选区里的预编辑串整个吃掉——而它早已
                // 通过 diff 路径落进领域、用户看得见（实测：preedit "ni" 后
                // 按回车 → `"hello\n"`，"ni" 凭空消失）。故只把 armed 窗续到
                // 组合窗到期，继续等 `Preedit`/`Commit`/`Disabled`。
                // （`ime_composing` 为真时 `ime_composing_until_ms` 必然
                // 大于 `now_ms`——上面的守卫分支刚做过到期清算。）
                self.egui_ime_armed_until_ms = self.ime_composing_until_ms;
            } else {
                self.egui_ime_armed = false;
                tell_egui_ime_over = true;
            }
        }
        // 给 egui 补一个 `Disabled`。放在事件流**尾部**：本帧若同时有
        // `Preedit`，必须让它先被处理（`Preedit` 会重新置 `ime_enabled = true`），
        // 否则这个 `Disabled` 会被随后的 `Preedit` 覆盖回去。
        if tell_egui_ime_over {
            // egui 0.37 起 `ImeEvent::Enabled`/`Disabled` 已被废弃、egui 不再消费
            // （IME 开关由 egui 自己管理）。这里保留「补一个 Disabled」的意图并
            // 显式压制告警：推送一个被忽略的事件对运行时无副作用，且若将来回退
            // 到会消费该事件的版本，这段复位逻辑可立即恢复作用。
            #[allow(deprecated)]
            ctx.input_mut(|i| i.events.push(egui::Event::Ime(egui::ImeEvent::Disabled)));
        }
        // 抹平 Enter 的 `repeat`，让 egui 当作一次普通按下处理。
        //
        // 为什么必须抹：egui 的 `remove_ime_incompatible_events` 是在
        // **`TextEdit::show` 控件内部**执行的（text_edit/builder.rs 的
        // `events()`），判据是当帧的 `state.ime_enabled`；本方法跑在 show
        // **之前**，改的事件流它能看见，所以抹平确实来得及——但**只在抹平
        // 条件成立时**。
        //
        // 放行时机（共用一个判据「本帧没有任何组合事件」）：
        //   1. armed 窗口内（`Enabled` 后 [`IME_ARM_MS`] 内没见到组合事件）；
        //   2. armed **刚刚到期**、本帧正要补发 `Disabled` 的那一帧。
        // 第 2 类曾被漏掉：旧实现先执行 `egui_ime_armed = false` 再检查抹平
        // 条件，到期帧上抹平被跳过；而剔除发生在 show 内部、按当帧仍为真的
        // `ime_enabled` 执行，补发的 `Disabled` 要等 IME 事件排序（排在剔除
        // **之后**）才生效，救不回那一帧。实测：release 丢失后的第二次回车
        // （`repeat: true`）在 armed 到期帧被整个吞掉——「连按回车只多出
        // 一个新行」的直接原因之一。
        //
        // 组合期（本帧有 `Preedit`/`Commit`/`Disabled`）不抹：那时 egui 的
        // 剔除是正当防护。注意 armed 在组合期内会被续期（见上），故
        // 「armed 为真」覆盖了组合期——组合期内的 repeat 回车由下面的
        // 「裸按键废弃组合」分支兜底放行，不会误伤。
        if !ime_preedit_now
            && !ime_commit_now
            && !ime_ended
            && (self.egui_ime_armed || tell_egui_ime_over)
        {
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
        // 组合窗内（或守卫刚到期的那一帧）到达「裸按键 / 裸文本」
        // （本帧无任何 IME 事件）：
        // 真实输入法在组合期会自己吞掉按键、转成 `Commit` 送达；裸按键能
        // 到这里，说明这次组合已经作废。预编辑串早已经 diff 路径落进领域
        // （用户看得见、领域也已认账），若放任不管，TextEdit 处理该按键时的
        // `delete_selected` 会把选区里的预编辑串整个吃掉——领域内容凭空丢失。
        //
        // 补救：补一个 `Commit(<预编辑串>)`，让 egui 自己把预编辑选区**折叠**成
        // 普通文本（选区被拆掉后，按键的 `delete_selected` 就无从下手）。
        //
        // # 为什么不再用空串（egui 0.37 语义变了）
        //
        // 0.33 的 Commit 分支对**空串**的处理是「只把光标折叠到 primary」、
        // 不碰文本，所以旧实现补 `Commit("")` 即可。0.37 改成先
        // `delete_selected(cursor_range)` 再插入 `commit_text`——组合期
        // `cursor_range` 正是预编辑区间，空串于是把预编辑串**真删掉**
        // （实测 `"helloni"` + 回车 → `"hello\n"`）。故必须回提交**与预编辑串
        // 等长的同一段文本**：delete 与 insert 相抵，净效果才等价于「折叠选区、
        // 文本不变」。串的内容与位置从 TextEdit 状态读回（见 `composition_text`）。
        //
        // # 为什么必须插到事件流**最前**
        //
        // 0.33 在 `ime_enabled` 时会把 IME 事件稳定排序到最前
        // （`events.sort_by_key(|e| !matches!(e, Event::Ime(_)))`），旧实现的
        // 注释据此写下「IME 事件必然先于按键处理」。0.37 删掉了这个排序，
        // 改为按列表顺序就地处理——同一个 Commit 若仍 push 到尾部，回车会
        // **先**执行（吃掉预编辑串），Commit 只是事后补刀。故这里显式插到 0。
        //
        // 同时解除两扇守卫：组合既已废弃，`ime_composing` 继续冻结
        // `pending_cursor` 只会让「本次按键改写了文本」（如列表续行）后的
        // 光标回设被卡住，重蹈 V11-14「光标停在旧位置、下一次回车插到同一处」
        // 的覆辙；`egui_ime_armed` 的使命（等 `Preedit` 出现）也已完成。
        //
        // 判据用 `composing_before`（本帧清算**之前**的守卫状态）而不是
        // 清算后的 `self.ime_composing`：守卫到期帧上后者已被上面的到期分支
        // 清零，用它会让兜底整段跳过——而那一刻 egui 的预编辑选区**仍在**
        // 缓冲区里（到期只说明我们等够了时间，不代表输入法已收尾）。实测：
        // `probe_enter_during_active_composition_preserves_preedit` 的 gap=900
        // （恰在 800ms 窗口之外那一帧）预编辑串被吃掉。用清算前的状态，
        // 「Preedit 之后无论隔多久按裸键」都能被兜住——这才是正确的判据：
        // 组合期的**结束方式**只有 Commit/Disabled 与「裸按键作废」两种，
        // 超时到期只是我们这侧的观察窗口，不是真实组合的结束信号。
        if composing_before && !ime_preedit_now && !ime_commit_now && !ime_ended {
            let has_text_input = ctx.input(|i| {
                i.events.iter().any(|event| match event {
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        ..
                    } => true,
                    egui::Event::Text(text) => !text.is_empty(),
                    _ => false,
                })
            });
            if has_text_input {
                let commit_text = self.composition_text(ctx);
                ctx.input_mut(|i| {
                    i.events
                        .insert(0, egui::Event::Ime(egui::ImeEvent::Commit(commit_text)));
                });
                self.ime_composing = false;
                self.ime_composing_until_ms = 0;
                self.egui_ime_armed = false;
            }
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
                index: egui::text::CharIndex(cursor),
                prefer_next_row: false,
            };
            let secondary = egui::text::CCursor {
                index: egui::text::CharIndex(anchor),
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
