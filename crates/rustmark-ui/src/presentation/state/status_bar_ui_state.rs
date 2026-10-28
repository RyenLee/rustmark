//! 状态栏 UI 状态（S6-11）。
//!
//! # 为什么不把提示塞进各视图自己的状态
//!
//! 状态栏是**全局唯一**的反馈出口：保存失败、导出完成、写作目标达成
//! 都该在这里出现，而不是各自弹一个 toast。若每个视图各存一份
//! `error: Option<String>`，两个视图同时失败时后写的会盖掉先写的，
//! 用户只看到一条、另一条失败被静默吞掉。
//!
//! # 瞬态消息必须会自己消失
//!
//! 消息带**到期时刻**而不是等下一个错误来覆盖：
//! - 不消失 → 「已保存」会一直挂着，用户过两分钟看文件时它还在说「已保存」；
//! - 被覆盖 → 连续两次操作只有最后一次可见，第一次的失败被吞掉。
//!
//! 二者都不对，所以按时间到期。
//!
//! 时间的注入走 [`rustmark_core::ports::time::Clock`]：直接调 `now_millis()`
//! 会让「3 秒后消失」这条规则无法被测试验证。

use rustmark_core::application::dto::GoalProgressView;
use rustmark_core::shared::encoding::TextEncoding;
use rustmark_core::shared::text::thousands;

/// 状态栏消息的默认存活时长（毫秒）。
///
/// 取 4 秒而非原型风格的「一直显示」：状态栏信息密度高，
/// 一条错误压在那里 30 秒会让用户以为程序卡住了。
pub const MESSAGE_TTL_MS: u64 = 4_000;

/// 一条状态栏消息。
#[derive(Clone, Debug, PartialEq)]
pub struct StatusMessage {
    /// 正文。
    pub text: String,
    /// 是否是错误（错误用告警色，正常用弱化色）。
    pub is_error: bool,
    /// 到期时刻（毫秒）。
    pub expires_at: u64,
}

impl StatusMessage {
    /// 构造一条会在 `ttl_ms` 后消失的消息。
    pub(crate) fn new(text: impl Into<String>, is_error: bool, now_ms: u64, ttl_ms: u64) -> Self {
        Self {
            text: text.into(),
            is_error,
            expires_at: now_ms.saturating_add(ttl_ms),
        }
    }

    /// 是否已过期。
    pub(crate) fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at
    }
}

/// 状态栏 UI 状态。
#[derive(Debug, Default)]
pub struct StatusBarUiState {
    /// 当前瞬态消息（无则 `None`）。
    pub message: Option<StatusMessage>,
    /// 当前文档的编码；新建未保存的文档为 `None`（V11-03 / DEF-05）。
    ///
    /// 类型取自共享内核 [`TextEncoding`]——它只有「这是什么编码」的语义，
    /// 不含解码实现，因此表现层引用它不会违反分层（禁止的是
    /// `use rustmark_core::infrastructure::*`，不是禁止「知道编码名」）。
    ///
    /// 取值由 `EditorApp` 在打开 / 切换 / 关闭文档时向应用层
    /// `DocumentAppService::encoding_of` 查询后写入。
    pub encoding: Option<TextEncoding>,
    /// 写作目标进度（V11-05）；未设目标时 [`GoalProgressView::is_set`] 为 `false`，
    /// 状态栏据此**整项隐藏**——画一条恒为 0% 的进度条只是噪音。
    pub goal: GoalProgressView,
    /// 达成提示的去重游标；`None` 表示当前没有可播报的目标上下文。
    goal_watch: Option<GoalWatch>,
}

/// 写作目标达成提示的**去重游标**（V11-05）。
///
/// # 为什么只播报「向上跨越」
///
/// 提示若每帧播报，达成之后它会每 4 秒闪一次、永不消失（TTL 每次被刷新）。
/// 但也不能简单用「播过就不再播」的 `bool`：
/// - 打开一篇**本来就超标**的文档也会弹一次「达成」——用户此刻并没写完；
/// - 用户改了目标值后，新目标达成不会有任何反馈。
///
/// 因此记录 `(文档, 目标)` 作为身份，并要求**先见过未达成**才播报：
/// 打开即达标 → 不弹；写到最后一句 → 弹一次；删到线下再写回来 → 再弹一次
/// （那确实是又达成了一次）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GoalWatch {
    doc: u64,
    goal: u32,
    /// 是否已观察到「未达成」状态（只有在此之后达成才值得播报）。
    seen_below: bool,
    /// 本次跨越是否已播报过。
    announced: bool,
}

impl GoalWatch {
    fn new(doc: u64, goal: u32) -> Self {
        Self {
            doc,
            goal,
            seen_below: false,
            announced: false,
        }
    }
}

impl StatusBarUiState {
    /// 构造默认状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 编码显示名；尚未落盘的文档显示 `—`（V11-03 / DEF-05）。
    ///
    /// **不显示 `UTF-8`**：新建文档还没有落过盘，说它是 UTF-8 等于把
    /// 「将来的默认值」当成既定事实。`—` 才是此刻的真实状态。
    pub fn encoding_label(&self) -> &'static str {
        match self.encoding {
            Some(enc) => enc.label(),
            None => "—",
        }
    }

    /// 编码项的悬停提示（说明自动识别 + 保存写回原编码）。
    pub fn encoding_hint(&self) -> &'static str {
        match self.encoding {
            Some(enc) => enc.hint(),
            None => "尚未保存到磁盘；首次保存按 UTF-8 写入",
        }
    }

    /// 写作目标项的文案（V11-05）。
    ///
    /// 达成时说「目标达成」而不是「目标 100%」：100% 是**数字**，
    /// 用户还得自己判断「满了」；达成是**结论**，一眼就够。
    pub fn goal_label(&self) -> String {
        match self.goal.percent {
            Some(_) if self.goal.reached => "目标达成".to_string(),
            Some(pct) => format!("目标 {pct}%"),
            None => "目标 未设置".to_string(),
        }
    }

    /// 写作目标项的悬停提示。
    pub fn goal_hint(&self) -> String {
        if !self.goal.is_set() {
            return "未设置写作目标；可在设置页填写目标字数".to_string();
        }
        format!(
            "写作目标：{} / {} 字（点击查看完整统计）",
            thousands(self.goal.words),
            thousands(self.goal.goal as usize)
        )
    }

    /// 每帧结算写作目标进度（V11-05），并在**向上跨越**时播报一次。
    ///
    /// `doc` 为 `None`（没有活动文档）或目标未设置时清空游标：
    /// 上一条文档的达标状态不该泄漏到下一篇。
    pub fn note_goal(&mut self, progress: GoalProgressView, doc: Option<u64>, now_ms: u64) {
        self.goal = progress;
        let (Some(doc), true) = (doc, progress.is_set()) else {
            self.goal_watch = None;
            return;
        };

        // 先把「要不要播报」算完再动消息槽：`watch` 借用的是 `self.goal_watch`，
        // 与 `self.notice(..)` 的可变借用冲突。
        let watch = self
            .goal_watch
            .get_or_insert_with(|| GoalWatch::new(doc, progress.goal));
        if watch.doc != doc || watch.goal != progress.goal {
            *watch = GoalWatch::new(doc, progress.goal);
        }
        let announce = if progress.reached {
            let due = watch.seen_below && !watch.announced;
            if due {
                watch.announced = true;
            }
            due
        } else {
            watch.seen_below = true;
            watch.announced = false;
            false
        };
        if announce {
            self.notice(
                format!(
                    "写作目标达成：{} / {} 字",
                    thousands(progress.words),
                    thousands(progress.goal as usize)
                ),
                now_ms,
            );
        }
    }

    /// 展示一条错误。
    pub fn error(&mut self, text: impl Into<String>, now_ms: u64) {
        self.set(text, true, now_ms);
    }

    /// 展示一条普通提示。
    pub fn notice(&mut self, text: impl Into<String>, now_ms: u64) {
        self.set(text, false, now_ms);
    }

    fn set(&mut self, text: impl Into<String>, is_error: bool, now_ms: u64) {
        self.message = Some(StatusMessage::new(text, is_error, now_ms, MESSAGE_TTL_MS));
    }

    /// 取出当前未过期的消息，顺带清理过期的那条。
    ///
    /// **返回 `None` 时消息槽已被清空** —— 不能只读不删，
    /// 否则过期消息会永远占着 `Some`，调用方每帧都要自己判过期，
    /// 漏一处就留下一条永远不消失的旧提示。
    pub fn take_message(&mut self, now_ms: u64) -> Option<StatusMessage> {
        let expired = self.message.as_ref().is_some_and(|m| m.is_expired(now_ms));
        if expired {
            self.message = None;
        }
        self.message.clone()
    }

    /// 清除消息（用户点了某处、或者新操作覆盖旧消息时用）。
    pub fn clear_message(&mut self) {
        self.message = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_is_unknown_before_the_first_save() {
        let state = StatusBarUiState::new();
        assert_eq!(state.encoding_label(), "—", "未落盘的文档不得谎称 UTF-8");
        assert!(state.encoding_hint().contains("尚未保存"));
    }

    #[test]
    fn encoding_label_shows_the_detected_encoding() {
        let mut state = StatusBarUiState::new();
        state.encoding = Some(TextEncoding::Gbk);
        assert_eq!(state.encoding_label(), "GBK");
        assert!(state.encoding_hint().contains("写回原编码"));
    }

    #[test]
    fn messages_expire_on_their_own() {
        let mut state = StatusBarUiState::new();
        state.notice("已保存", 1_000);
        assert!(state.take_message(2_000).is_some(), "未到期时可见");
        assert!(
            state.take_message(1_000 + MESSAGE_TTL_MS).is_none(),
            "到期那一刻就该消失"
        );
    }

    #[test]
    fn expired_message_is_cleared_not_just_hidden() {
        let mut state = StatusBarUiState::new();
        state.error("失败", 0);
        assert!(state.take_message(MESSAGE_TTL_MS + 1).is_none());
        assert!(
            state.message.is_none(),
            "过期后槽位必须清空，否则调用方每帧都要自己判过期"
        );
    }

    #[test]
    fn errors_and_notices_replace_each_other() {
        let mut state = StatusBarUiState::new();
        state.error("保存失败", 0);
        state.notice("已保存", 1);
        let msg = state.take_message(2).expect("后写的应可见");
        assert!(!msg.is_error);
        assert_eq!(msg.text, "已保存");
    }

    // =======================================================================
    // 写作目标进度（V11-05）
    // =======================================================================

    /// 未设目标时整项隐藏，且不播报任何提示。
    #[test]
    fn goal_hidden_and_silent_when_unset() {
        let mut state = StatusBarUiState::new();
        state.note_goal(GoalProgressView::default(), Some(1), 100);
        assert!(!state.goal.is_set(), "未设目标不该被当成有目标");
        assert!(!state.goal_label().contains('%'), "未设目标不应出现百分比");
        assert!(state.take_message(101).is_none(), "没设目标不该弹出提示");
    }

    /// 打开一篇**本来就超标**的文档：不算「刚达成」，不弹提示。
    #[test]
    fn goal_does_not_announce_when_opened_already_reached() {
        let mut state = StatusBarUiState::new();
        state.note_goal(progress(100, 200), Some(7), 100);
        assert!(
            state.take_message(101).is_none(),
            "一开始就达标不该弹「达成」（用户并没刚写完）"
        );
    }

    /// 向上跨越只播报一次；回落后再达成再播报一次。
    ///
    /// 判据用消息的**到期时刻**而不是 `take_message`：`take_message` 在 TTL
    /// 内会一直返回同一条提示，无法区分「没再弹」与「上一条还在显示」。
    /// 只有重新播报才会把到期时刻后推；故「到期时刻不变」即等价于「没再弹」。
    #[test]
    fn goal_announces_once_per_upward_crossing() {
        let mut state = StatusBarUiState::new();
        // 连续多帧未达标：静默。
        for t in 0..5 {
            state.note_goal(progress(100, t * 10), Some(7), t as u64);
        }
        assert!(state.message.is_none(), "未达标不该弹");

        // 第 6 帧跨越：弹一次。
        state.note_goal(progress(100, 120), Some(7), 6);
        let expiry = state.message.as_ref().expect("跨越那一刻应播报").expires_at;
        assert_eq!(expiry, 6 + MESSAGE_TTL_MS);

        // 多帧持续达标：不重新播报（否则到期时刻被不断后推，等于永不消失）。
        for t in 7..12 {
            state.note_goal(progress(100, t * 20), Some(7), t as u64);
        }
        assert_eq!(
            state.message.as_ref().unwrap().expires_at,
            expiry,
            "达标期间重复播报会让提示永不消失"
        );

        // 回落到线下：清空去重状态。
        state.note_goal(progress(100, 40), Some(7), 20);
        // 再次跨越：再弹一次（确实是又达成了一次）。
        state.note_goal(progress(100, 100), Some(7), 21);
        assert_eq!(
            state.message.as_ref().unwrap().expires_at,
            21 + MESSAGE_TTL_MS,
            "回落后再达成应重新播报"
        );
    }

    /// 切换文档会重置去重：上一篇的达标状态不泄漏到下一篇。
    #[test]
    fn goal_watch_is_reset_on_document_switch() {
        let mut state = StatusBarUiState::new();
        // 文档 7：先线下后达标，一次真正的跨越。
        state.note_goal(progress(100, 50), Some(7), 1);
        state.note_goal(progress(100, 120), Some(7), 2);
        let expiry7 = state
            .message
            .as_ref()
            .expect("文档 7 达标应弹一次")
            .expires_at;
        assert_eq!(expiry7, 2 + MESSAGE_TTL_MS);

        // 切到文档 8（已达标）：未见过「线下」，不弹新提示 → 到期时刻不变。
        state.note_goal(progress(100, 200), Some(8), 4);
        assert_eq!(
            state.message.as_ref().unwrap().expires_at,
            expiry7,
            "切到另一篇已达标文档不该弹「达成」"
        );

        // 再切回文档 7 仍达标：同样不弹。
        state.note_goal(progress(100, 130), Some(7), 6);
        assert_eq!(
            state.message.as_ref().unwrap().expires_at,
            expiry7,
            "没再次跨越不该弹"
        );
    }

    /// 没有活动文档时清空目标上下文。
    #[test]
    fn goal_is_cleared_without_an_active_document() {
        let mut state = StatusBarUiState::new();
        state.note_goal(progress(100, 120), Some(7), 1);
        state.note_goal(GoalProgressView::default(), None, 2);
        assert!(
            state.take_message(3).is_none(),
            "无活动文档时不应在状态栏残留达标提示"
        );
    }

    /// 构造一个*已设目标*的进度视图（测试辅助）。
    fn progress(goal: u32, words: usize) -> GoalProgressView {
        // 直接填好字段：测试只关心「是否达标 / 百分比」，不依赖领域实现。
        let percent = if words >= goal as usize {
            Some(100)
        } else {
            Some(((words as u128 * 100) / goal as u128) as u8)
        };
        GoalProgressView {
            goal,
            words,
            percent,
            reached: words >= goal as usize,
        }
    }
}
