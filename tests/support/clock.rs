//! 可控时钟替身：让「时间窗口」类逻辑在无头测试里**确定地**走过期 / 未过期两条分支。
//!
//! # 为什么不能依赖真实时钟
//!
//! 应用侧凡带时间窗的逻辑（高亮防抖、IME 保护窗、滚动同步抑制、撤销合并）
//! 都以 `Clock::now()` 的差值为判据。真实 `SystemClock` 下：
//!
//! - 窗口是 150ms / 800ms 量级，而一个无头用例整体只跑几毫秒 →
//!   窗口**永不**过期，「已超时」那条分支从未被测到（等于没测）；
//! - 反过来若用例跑得慢（debug、CI 抖动），窗口可能在用例中途过期，
//!   被测行为**碰巧**改变 —— 这是随机失败，不是回归。
//!
//! 两者都违反「回归必须确定性」：所以凡涉及时间窗的用例一律用本替身，
//! 需要过期时显式 [`TestClock::advance`]。
//!
//! # 起点为什么不是 0
//!
//! 从 `1_700_000_000_000`（约 2023-11）起，免得「时间从 0 开始」这种现实中
//! 不存在的状态影响被测逻辑（如滚动同步的抑制窗口、撤销合并窗口等都以
//! 绝对毫秒做差）。

// `dead_code` 豁免：本模块与 `support.rs` 一起被**每个**测试目标用 `#[path]` 引入，
// 但只有 presentation 目标会真正构造 `TestClock`（时间窗用例），其余目标
// （domain / application / infrastructure / e2e …）用不到它。逐目标判断会误报，
// 理由与 `support.rs` 里 `run_ui` / `settle` 的豁免相同。
#![allow(dead_code)]

use rustmark::shared::time::{Clock, Timestamp};
use std::sync::atomic::{AtomicU64, Ordering};

/// 以毫秒为单位的可控时钟，初值取一个真实量级的时间戳。
#[derive(Debug)]
pub struct TestClock(AtomicU64);

impl Default for TestClock {
    fn default() -> Self {
        TestClock(AtomicU64::new(1_700_000_000_000))
    }
}

impl TestClock {
    /// 把时钟向前推进 `ms` 毫秒（用于让某个时间窗过期）。
    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for TestClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.0.load(Ordering::SeqCst))
    }
}
