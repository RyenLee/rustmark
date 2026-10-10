//! 强类型标识符。
//!
//! 各上下文的 ID 集中定义于此（Shared Kernel，见需求文档 §6.5），避免
//! `u64` 在不同上下文间被误传。全部 ID 由进程内单调计数器生成，
//! **不引入 `uuid`**（省体积；持久化时以 `u64` 落盘）。

use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(1);

/// 取下一个 ID 序号。从 1 开始，`0` 保留为「未初始化」哨兵值。
fn next_seq() -> u64 {
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

macro_rules! declare_id {
    ($name:ident) => {
        #[doc = concat!("强类型标识符 `", stringify!($name), "`。")]
        ///
        /// 与同类型的其它 ID 以值相等比较；**不同类型之间不可互转**（无 `From` 实现），
        /// 防止 `DocumentId` 被误当作 `WorkspaceId` 使用。
        #[derive(
            Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
        )]
        pub struct $name(u64);

        impl $name {
            /// 由进程内单调计数器生成新 ID。
            pub fn new() -> Self {
                Self(next_seq())
            }

            /// 从 `u64` 还原（用于持久化反序列化）。
            pub const fn from_u64(value: u64) -> Self {
                Self(value)
            }

            /// 取出内部 `u64`（用于持久化序列化）。
            pub const fn as_u64(self) -> u64 {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

declare_id!(DocumentId);
declare_id!(WorkspaceId);
declare_id!(AssetId);
declare_id!(JobId);
