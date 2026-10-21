//! 快捷键持久化键的**跨层一致性**守卫（R-08①）。
//!
//! # 这条用例守的是什么
//!
//! R-08① 把表现层 `UiAction::persisted_key` 从「`_` 兜底」改成**穷举
//! `match`**：新增 `UiAction` 变体时**编译失败**，作者必须当场表态
//! 「它能不能持久化」。这一半由编译器保证，不需要用例。
//!
//! 但**另一半编译器管不到**：表现层不得依赖领域（`check-arch` 拦
//! `crate::domain`），两侧只能靠**字符串契约**对齐——而字符串契约
//! **天然可能漂移**：
//!
//! - 领域 `ActionId` 加了变体，表现层忘了加 → 该动作**改键无法落盘**；
//! - 表现层给某动作写了 `Some("save")` 而领域是 `Some("save_all")` →
//!   用户改完键，配置写到**另一个动作**上。
//!
//! 这两类漂移**零报错**：`persisted_key` 返回的只是一个 `&'static str`，
//! 编译器无从知晓它对不对。唯一能查的地方是**两边放在一起比对**。
//!
//! # 为什么放 `tests/app/`
//!
//! 表现层测试目标**不得**引用 `crate::domain`（那是 `src/` 里的架构
//! 约束，测试目标虽不受 `check-arch` 扫，但把跨层断言放进
//! `tests/presentation/` 会误导后来者以为表现层可以依赖领域）。
//! `tests/app/` 是「唯一能同时看见四层的地方」——跨层断言归它。
//!
//! # 判据
//!
//! 领域 `ActionId::all()` 的 `key()` 集合，与表现层
//! `UiAction::ALL` 中 `persisted_key()` 返回 `Some` 的键集合，
//! **逐项相等**（数量相等 + 每个键都能在另一侧找到）。

use rustmark::domain::preferences::value_objects::ActionId;
use rustmark::presentation::shortcuts::registry::UiAction;

/// 领域侧的全部持久化键。
fn domain_keys() -> Vec<&'static str> {
    ActionId::all().iter().map(|id| id.key()).collect()
}

/// 表现侧的全部持久化键（`persisted_key()` 返回 `Some` 的那些）。
fn presentation_keys() -> Vec<&'static str> {
    UiAction::ALL
        .iter()
        .filter_map(|action| action.persisted_key())
        .collect()
}

/// 两侧的持久化键**逐项相等**。
///
/// 这条是 R-08① 的核心判据：领域能改键的 38 个动作，与表现层认为
/// 能改键的 38 个动作，必须是**同一批、同一批键名**。
///
/// # 为什么不能只比数量
///
/// 只比数量挡不住「键名写错」：两侧都是 38 个，但一个是 `save`、
/// 一个是 `svae`——数量相等，比对却查不出。必须**逐项**比。
#[test]
fn domain_and_presentation_persisted_keys_agree() {
    let domain = domain_keys();
    let presentation = presentation_keys();

    assert_eq!(
        domain.len(),
        presentation.len(),
        "领域可持久化动作 {} 个，表现层可持久化动作 {} 个——\
         两侧必须一致，否则有一侧的改键无法落盘。\
         领域多出的：{:?}；表现层多出的：{:?}",
        domain.len(),
        presentation.len(),
        domain
            .iter()
            .filter(|k| !presentation.contains(k))
            .collect::<Vec<_>>(),
        presentation
            .iter()
            .filter(|k| !domain.contains(k))
            .collect::<Vec<_>>(),
    );

    for key in &domain {
        assert!(
            presentation.contains(key),
            "领域动作的持久化键 {key} 在表现层没有对应项——\
             该动作的改键无法落盘"
        );
    }
    for key in &presentation {
        assert!(
            domain.contains(key),
            "表现层的持久化键 {key} 在领域没有对应项——\
             用户改键会写到不存在的动作上"
        );
    }
}

/// 领域 `ActionId::all()` 不漏登记任何变体。
///
/// `ActionId::all()` 是**手写清单**（`const fn` 返回 `&[ActionId]`），
/// 编译器管不到它：加变体忘了加进清单 → 该动作**不在命令面板**、
/// **不可改键**，而**零报错**。
///
/// 这条只能靠**数量对照**守住：把领域枚举的变体数钉死。
/// 变体数变化时这条会红——**那正是它该红的时候**
///（`ActionId::all()` 的清单需要同步更新）。
#[test]
fn domain_action_list_registers_every_variant() {
    // 领域 `ActionId` 的变体数（实测 38）。改这个数字时，
    // 必须同时确认 `ActionId::all()` 的清单已同步。
    const EXPECTED_VARIANTS: usize = 38;

    assert_eq!(
        ActionId::all().len(),
        EXPECTED_VARIANTS,
        "领域 `ActionId` 变体数变了：新增/删除变体时，\
         `ActionId::all()` 的清单必须同步更新，否则该动作\
         不会出现在命令面板里，也不可改键"
    );
}

/// 表现层 `UiAction::ALL` 不漏登记任何变体。
///
/// 同上：`ALL` 是手写清单，编译器管不到。加变体忘了登记 →
/// 命令面板少一项、快捷键一览少一项，**零报错**。
#[test]
fn presentation_action_list_registers_every_variant() {
    // 表现层 `UiAction` 的变体数（实测 57）。
    const EXPECTED_VARIANTS: usize = 57;

    assert_eq!(
        UiAction::ALL.len(),
        EXPECTED_VARIANTS,
        "表现层 `UiAction` 变体数变了：新增/删除变体时，\
         `UiAction::ALL` 的清单必须同步更新，否则该动作\
         不会出现在命令面板里"
    );
}

/// 每个动作的持久化决策都是**可预测**的。
///
/// # 判据
///
/// - 动作**有键** → 该键必须真的存在于领域，且**反查能回到同一个动作**
///   （键不得撞车：A 和 B 拿到同一个键时，`from_persisted_key` 只认先出现的
///   那个，另一个的键就是死的——用户改了它的键却写到了别人头上）；
/// - 领域每个键都**恰好**对应一个表现层动作（不缺、不多、不少）。
///
/// # 为什么第二条不能省
///
/// 「有键」侧的检查只覆盖**表现层有、领域有**的方向。若某动作被误判成
/// 不可持久化（`None`）而它在领域有对应项，集合比对会红——但若两侧
/// **同时**漏了同一个动作，集合比对仍然相等，只有「逐个键都要有归属」
/// 才查得出。
#[test]
fn every_action_makes_an_explicit_persisted_key_decision() {
    for &action in UiAction::ALL {
        if let Some(key) = action.persisted_key() {
            // 有键 → 必须在领域存在。
            assert!(
                domain_keys().contains(&key),
                "动作 {action:?} 的持久化键 {key} 在领域不存在——\
                 改键会写到不存在的动作上"
            );
            // 反查必须回到**同一个**动作（键不得撞车）。
            assert_eq!(
                UiAction::from_persisted_key(key),
                Some(action),
                "持久化键 {key} 反查回了别的动作（键撞车）"
            );
        }
    }

    // 领域的每个键都恰好对应一个表现层动作。
    for key in domain_keys() {
        let owner = UiAction::from_persisted_key(key);
        assert!(
            owner.is_some(),
            "领域动作的持久化键 {key} 在表现层没有归属动作——\
             该动作的改键无法落盘"
        );
        assert_eq!(
            owner.and_then(|action| action.persisted_key()),
            Some(key),
            "持久化键 {key} 的归属动作反查出的键不是它自己（键撞车）"
        );
    }
}

/// 不可持久化的动作清单是**显式**的 19 个。
///
/// 这条不是「不许变」——新增一个纯 UI 动作时**应该**改这条断言，
/// 提交 diff 里就能看见「又有一个动作不参与改键持久化」。
/// 它守的是「清单不被悄悄改大/改小」。
#[test]
fn the_unpersistable_action_list_is_explicit() {
    let unpersistable: Vec<UiAction> = UiAction::ALL
        .iter()
        .copied()
        .filter(|&action| action.persisted_key().is_none())
        .collect();

    assert_eq!(
        unpersistable,
        vec![
            UiAction::OpenFolder,
            UiAction::Revert,
            UiAction::InsertImage,
            UiAction::ToggleList,
            UiAction::ToggleOrderedList,
            UiAction::MoveLineUp,
            UiAction::MoveLineDown,
            UiAction::GotoTab1,
            UiAction::GotoTab2,
            UiAction::GotoTab3,
            UiAction::GotoTab4,
            UiAction::GotoTab5,
            UiAction::GotoTab6,
            UiAction::GotoTab7,
            UiAction::GotoTab8,
            UiAction::GotoTab9,
            UiAction::ShortcutOverview,
            UiAction::About,
            UiAction::ToggleTheme,
        ],
        "不可持久化的动作清单变了：新增/删除纯 UI 动作时必须同步更新本断言，\
         并在提交说明里交代为什么它不参与改键持久化"
    );
}
