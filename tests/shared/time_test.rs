//! `Timestamp` 与 `Clock` 单测（S3-A）。
//!
//! 时间必须**可注入**（`.agent/conventions.md` 第 21 条），故领域逻辑只依赖
//! [`Clock`] trait；这里验证时间戳的比较与饱和语义。

use rustmark::shared::time::{Clock, SystemClock, Timestamp};

#[test]
fn within_is_inclusive_at_window_boundary() {
    let earlier = Timestamp::from_millis(1_000);
    let at_boundary = Timestamp::from_millis(1_400);
    let beyond = Timestamp::from_millis(1_401);

    assert!(
        at_boundary.within(earlier, 400),
        "边界（恰好 400ms）应判定为窗口内"
    );
    assert!(!beyond.within(earlier, 400));
}

#[test]
fn elapsed_saturates_instead_of_wrapping() {
    let earlier = Timestamp::from_millis(5_000);
    let later = Timestamp::from_millis(1_000);
    assert_eq!(
        later.elapsed_ms_since(earlier),
        0,
        "时间倒流应饱和为 0，不得回绕"
    );
    assert!(!earlier.within(later, 0));
}

#[test]
fn timestamps_are_ordered() {
    assert!(Timestamp::from_millis(1) < Timestamp::from_millis(2));
    assert_eq!(Timestamp::default().millis(), 0);
}

#[test]
fn system_clock_is_monotonic_across_calls() {
    let clock = SystemClock;
    let first = clock.now();
    let second = clock.now();
    assert!(second.millis() >= first.millis(), "系统时钟不得倒流");
    assert!(first.millis() > 0, "系统时钟应返回真实纪元毫秒");
}
