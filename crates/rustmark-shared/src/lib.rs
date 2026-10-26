//! # rustmark-shared —— 共享内核（Shared Kernel，TD-04 抽出，2026-10-26）
//!
//! 可被任意层依赖的**纯值 / 端口 / 唯一真源**（零 IO）。分层规则见
//! `docs/architecture-constraints.md` §3 / §4.5：本 crate 只有 serde /
//! thiserror / ahash 三个纯数据依赖，反向依赖任何层都会在 Cargo 解析期成环
//! 报错——「shared 不依赖任何层」从词法规则升级为结构性事实。
//!
//! ## 为什么保留 `shared/` 路径前缀
//!
//! 文件住在 `src/shared/*.rs` 而不是平铺在 crate 根（与 `rustmark-ui` 保留
//! `presentation/` 前缀同理）：`layer_of`、§4.5 登记表、可见性白名单与各
//! 专项守卫**全部按根内相对路径**判定。保留前缀 = 这些规则与登记键
//! (`shared/<文件名>`) 随搬迁零改动，既有的「未登记即违规」守卫继续咬合。
//!
//! ## 桥接
//!
//! `rustmark-core` 以 `pub use rustmark_shared::shared;` 把模块拼回原位：
//! `rustmark_core::shared::…`（root / ui / tests 的公开路径）与 core 内
//! `crate::shared::…` 逐字不变（与 V11-57 actions、V11-58 icons、V11-62 ui
//! 同款桥接）。将来出现非 UI 消费者时，可直接依赖本 crate，不必拉 core。

// 测试代码允许 unwrap / expect（规范 §七-20）；生产代码保持 deny
// （与搬迁前 `shared.rs` 的 `#![deny(clippy::unwrap_used, clippy::expect_used)]`
// 等价，覆盖范围从模块升为整个 crate）。
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod shared;
