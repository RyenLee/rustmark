//! 时间抽象：时间戳与可注入时钟。
//!
//! 设计动机（`.agent/conventions.md` 第 21 条）：撤销合并窗口、自动保存间隔等
//! 时间相关行为**必须通过 [`Clock`] 注入**，禁止在领域逻辑里直接读系统时间，
//! 否则该行为无法单测。
//!
//! 不使用 `chrono`：领域层只需要「毫秒级标量 + 比较」，无需时区与历法运算，
//! 引入 `chrono` 只会增加体积（`opt-level = "z"` 下仍会进入二进制）。

/// 自 UNIX 纪元起的毫秒数。
///
/// 该值为**标量**，不携带时区信息；持久化时以 `u64` 落盘，与平台无关。
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Debug,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct Timestamp(u64);

impl Timestamp {
    /// 由毫秒数构造。
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// 取出毫秒数（用于持久化）。
    pub const fn millis(self) -> u64 {
        self.0
    }

    /// 距 `earlier` 经过的毫秒数。
    ///
    /// `self` 早于 `earlier` 时**饱和到 0**，不 panic、不回绕。
    pub fn elapsed_ms_since(self, earlier: Self) -> u64 {
        self.0.saturating_sub(earlier.0)
    }

    /// 是否落在 `earlier` 之后的 `window_ms` 毫秒之内（含端点）。
    ///
    /// 边界语义：`elapsed == window_ms` 判定为「在窗口内」。
    /// 用于撤销合并窗口等「连续操作是否算作一次」的判定。
    pub fn within(self, earlier: Self, window_ms: u64) -> bool {
        self.elapsed_ms_since(earlier) <= window_ms
    }
}

/// 时钟抽象：由基础设施层或测试注入具体实现。
///
/// 实现必须 `Send + Sync`（跨平台异步初始化与后台线程会跨线程使用）。
pub trait Clock: Send + Sync {
    /// 读取当前时间。
    fn now(&self) -> Timestamp;
}

/// 真实系统时钟。
///
/// 位于 `rustmark-ports`（R-10 项 A 从 shared 抽出）而非 `infrastructure`：
/// 它不触碰文件系统或网络，仅读取单调时钟源，属于共享内核的一部分。
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        // u128 → u64：超出范围时饱和（不 panic），此类时间戳仅用于排序与差值。
        Timestamp::from_millis(u64::try_from(millis).unwrap_or(u64::MAX))
    }
}

/// 编译期断言：`Clock` 的实现必须可跨线程共享。
const fn _assert_clock_send_sync<T: Clock + Send + Sync>() {}
const _ASSERT_SYSTEM_CLOCK: () = _assert_clock_send_sync::<SystemClock>();
