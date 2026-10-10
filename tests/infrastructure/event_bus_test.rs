//! `InProcessEventBus` 单测（S5-07）。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rustmark::infrastructure::events::in_process_event_bus::InProcessEventBus;
use rustmark::shared::error::AppError;
use rustmark::shared::event::{DomainEvent, EventBus, EventHandler};
use rustmark::shared::ids::{DocumentId, JobId};

fn event() -> DomainEvent {
    DomainEvent::DocumentSaved(DocumentId::new())
}

#[test]
fn handlers_receive_event_in_subscription_order() {
    let bus = InProcessEventBus::new();
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));

    for tag in ["first", "second", "third"] {
        let order = order.clone();
        let handler: EventHandler = Arc::new(move |_event| {
            order.lock().unwrap().push(tag);
            Ok(())
        });
        bus.subscribe(handler);
    }

    assert_eq!(bus.handler_count(), 3);
    assert!(bus.publish(event()).is_empty(), "全部成功时不应有错误");
    assert_eq!(
        *order.lock().unwrap(),
        vec!["first", "second", "third"],
        "订阅者必须按注册顺序收到事件"
    );
}

#[test]
fn failing_handler_does_not_block_others() {
    let bus = InProcessEventBus::new();
    let reached = Arc::new(AtomicUsize::new(0));

    bus.subscribe(Arc::new(|_| Ok(())));
    bus.subscribe(Arc::new(|_| Err(AppError::domain("订阅者 A 失败"))));
    bus.subscribe(Arc::new(|_| Ok(())));
    // 后置订阅者：必须证明前面的失败没有中断派发。
    let reached_after = reached.clone();
    bus.subscribe(Arc::new(move |_| {
        reached_after.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));

    let errors = bus.publish(event());
    assert_eq!(errors.len(), 1, "只应收集到 1 个错误");
    assert_eq!(errors[0].to_string(), "订阅者 A 失败");
    assert_eq!(
        reached.load(Ordering::SeqCst),
        1,
        "失败之后的订阅者仍必须被调用"
    );
}

#[test]
fn multiple_errors_are_returned_in_dispatch_order() {
    let bus = InProcessEventBus::new();
    for tag in ["一", "二", "三"] {
        bus.subscribe(Arc::new(move |_| Err(AppError::domain(tag))));
    }
    let errors = bus.publish(event());
    let messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
    assert_eq!(messages, vec!["一", "二", "三"], "错误应按派发顺序返回");
}

#[test]
fn handler_subscribing_during_publish_takes_effect_next_time() {
    let inner_hits = Arc::new(AtomicUsize::new(0));

    // 第一个订阅者在自己被调用的过程中注册第二个订阅者。
    let bus_for_handler = Arc::new(InProcessEventBus::new());
    let bus2 = bus_for_handler.clone();
    let hits = inner_hits.clone();
    bus_for_handler.subscribe(Arc::new(move |_event| {
        let hits = hits.clone();
        bus2.subscribe(Arc::new(move |_event| {
            hits.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        Ok(())
    }));

    bus_for_handler.publish(event());
    assert_eq!(
        inner_hits.load(Ordering::SeqCst),
        0,
        "派发期间新增的订阅者本次不生效（快照式派发）"
    );
    bus_for_handler.publish(event());
    assert_eq!(
        inner_hits.load(Ordering::SeqCst),
        1,
        "新订阅者从下一个事件开始生效"
    );
}

#[test]
fn all_event_variants_are_delivered_unchanged() {
    let bus = InProcessEventBus::new();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    bus.subscribe(Arc::new(move |event| {
        sink.lock().unwrap().push(*event);
        Ok(())
    }));

    let events = [
        DomainEvent::DocumentOpened(DocumentId::new()),
        DomainEvent::TextModified {
            document_id: DocumentId::new(),
            revision: 7,
        },
        DomainEvent::SelectionChanged {
            document_id: DocumentId::new(),
            selection: None,
        },
        DomainEvent::DocumentSaved(DocumentId::new()),
        DomainEvent::DocumentClosed(DocumentId::new()),
        DomainEvent::WorkspaceChanged(rustmark::shared::ids::WorkspaceId::new()),
        DomainEvent::PreferencesChanged,
        DomainEvent::ExportFinished(JobId::new()),
    ];
    for event in events {
        assert!(bus.publish(event).is_empty());
    }
    assert_eq!(
        *seen.lock().unwrap(),
        events.to_vec(),
        "全部事件变体都应原样送达订阅者"
    );
}

#[test]
fn arc_cloned_bus_can_publish_across_threads() {
    let bus: Arc<dyn EventBus> = Arc::new(InProcessEventBus::new());
    let hits = Arc::new(AtomicUsize::new(0));

    for _ in 0..4 {
        let hits = hits.clone();
        bus.subscribe(Arc::new(move |_event| {
            hits.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
    }

    // 4 个线程各发布一次，总计应被调用 4 × 4 次。
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let bus = bus.clone();
            std::thread::spawn(move || {
                bus.publish(DomainEvent::PreferencesChanged);
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("发布线程不应 panic");
    }

    assert_eq!(
        hits.load(Ordering::SeqCst),
        16,
        "跨线程并发发布必须送达全部订阅者"
    );
}

#[test]
fn concurrent_subscribe_while_publishing_is_safe() {
    let bus: Arc<dyn EventBus> = Arc::new(InProcessEventBus::new());
    let hits = Arc::new(AtomicUsize::new(0));

    // 先注册一个「常驻」订阅者：它是本用例的**确定性**断言基准。
    // 若只在并发循环里注册，发布线程可能先跑完 200 轮再开始注册，
    // 于是 `hits == 0`——那是测试自身的竞态，而非被测代码的问题。
    let resident = hits.clone();
    bus.subscribe(Arc::new(move |_event| {
        resident.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));

    // 一个线程持续注册订阅者，另一个持续发布，检验 RwLock 无死锁 / 无数据竞争。
    let sub_bus = bus.clone();
    let subscriber = std::thread::spawn(move || {
        for _ in 0..200 {
            sub_bus.subscribe(Arc::new(|_event| Ok(())));
        }
    });

    let pub_bus = bus.clone();
    let publisher = std::thread::spawn(move || {
        for _ in 0..200 {
            pub_bus.publish(DomainEvent::PreferencesChanged);
        }
    });

    subscriber.join().expect("订阅线程不应 panic");
    publisher.join().expect("发布线程不应 panic");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        200,
        "常驻订阅者必须恰好收到全部 200 次发布"
    );
}
