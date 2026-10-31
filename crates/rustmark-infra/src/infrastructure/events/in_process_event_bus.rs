//! 进程内同步事件总线（S5-07）。
//!
//! # 为什么同步派发
//!
//! 备选方案是异步派发（后台线程 + 消息队列），但那样会引入通道、线程与生命周期
//! 管理，且事件处理时序变得不确定——而阶段 04 的跨上下文编排（`EventSubscribers`）
//! 依赖**确定的**顺序：例如 `DocumentSaved` 必须在 `TextModified` 之后被处理，
//! 否则「最近文件」会记上一份还没落盘的内容。编辑器的事件量级（每次按键 1 个）
//! 远低于异步方案能带来的吞吐收益，故选择同步。
//!
//! # 不 panic 的保证
//!
//! `panic = "abort"` 下`catch_unwind` **不可用**（没有 unwind 即没有栈展开），
//! 所以本总线**不做**任何 panic 隔离，而是靠约定保证：
//! **订阅者自己返回 `Result` 而不是 panic**。总线只负责收集 `Err` 并原样返回给
//! 调用方，由上层决定如何提示用户。这比「假装隔离了panic」更诚实——真出panic
//! 时进程本来就会终止，加一层徒劳的 `catch_unwind` 反而给人虚假的安全感。
//!
//! # 派发期间的订阅者变更
//!
//! 本实现**先克隆**订阅者快照再逐个调用，因此：
//! - 某个订阅者在处理中调用 `subscribe` → 新订阅者**本次不生效**（下个事件才收到）；
//! - 某个订阅者在处理中因异常退订（当前无退订 API）→ 本次仍会被调用。
//!
//! 这样「快照式派发」避免了迭代中修改容器的可变借用问题，也使行为可预测。

use std::sync::{Arc, RwLock};

use rustmark_core::shared::error::AppError;
use rustmark_ports::event::{DomainEvent, EventBus, EventHandler};

/// 进程内同步事件总线。
#[derive(Default)]
pub struct InProcessEventBus {
    handlers: RwLock<Vec<EventHandler>>,
}

impl InProcessEventBus {
    /// 构造一个没有订阅者的总线。
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前订阅者数量（供测试断言派发快照语义）。
    pub fn handler_count(&self) -> usize {
        self.handlers.read().map(|h| h.len()).unwrap_or(0)
    }

    /// 取订阅者快照。
    ///
    /// 锁中毒（某个线程在持锁期间 panic）时退化为空列表：此时进程已处于
    /// 不确定状态，让总线**继续工作**比传播 panic 更符合「不 panic」的基调。
    fn snapshot(&self) -> Vec<EventHandler> {
        self.handlers.read().map(|h| h.clone()).unwrap_or_default()
    }
}

impl EventBus for InProcessEventBus {
    fn publish(&self, event: DomainEvent) -> Vec<AppError> {
        let mut errors = Vec::new();
        // 快照式派发：克隆后再调用，使订阅者可以在处理中安全地注册新订阅者。
        for handler in self.snapshot() {
            if let Err(error) = handler(&event) {
                errors.push(error);
            }
        }
        errors
    }

    fn subscribe(&self, handler: EventHandler) {
        if let Ok(mut handlers) = self.handlers.write() {
            handlers.push(handler);
        }
    }
}

impl std::fmt::Debug for InProcessEventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InProcessEventBus")
            .field("handler_count", &self.handler_count())
            .finish()
    }
}

/// 编译期断言：总线必须能跨线程共享（UI 线程发布、后台线程订阅）。
const fn _assert_send_sync<T: Send + Sync>() {}
const _ASSERT_BUS: () = _assert_send_sync::<InProcessEventBus>();
const _ASSERT_ARC_BUS: () = _assert_send_sync::<Arc<dyn EventBus>>();
