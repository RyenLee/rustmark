//! 主题种类的唯一真源用例（R-09）。
//!
//! # 这份用例守的是什么
//!
//! R-09 收敛掉的 `BuiltinTheme` 是**两份同构枚举靠字符串对齐**。
//! 那种结构没有任何机制阻止它复活：任何人新增一个枚举、写一份自己的
//! `as_str()`，编译器不会吭声，而两份枚举会从「刚建时一致」慢慢漂移到
//! 「不一致」——**没有任何报错**，用户看到的是「选了深色却显示浅色」。
//!
//! 行为断言抓不住这类漂移：两份枚举**刚建时行为完全一致**，
//! 于是任何行为用例在漂移前都是绿的。要抓住「又多了一份定义」，
//! 只能**扫源码**（与 `tools/arch_check.rs` 的思路同源，但这是
//! 「同一概念只有一处定义」的规则，不涉及依赖方向，故独立成文件）。
//!
//! # 持久化兼容是硬约束
//!
//! 主题种类以**字符串**落盘（`light` / `dark` / `system` / `custom`）。
//! 改这些字符串 = 用户配置失效（重启后主题被重置）。
//! 故本文件有专门的往返用例守着。

use rustmark::shared::theme::ThemeKind;

/// 每个主题种类的持久化字符串**往返**。
///
/// 这条同时守住**持久化兼容**：字符串值一旦改动，本用例即红。
#[test]
fn theme_kind_round_trips_through_its_persisted_string() {
    for &kind in ThemeKind::ALL {
        let raw = kind.as_str();
        assert_eq!(
            ThemeKind::parse(raw),
            Some(kind),
            "主题种类 {kind:?} 的持久化字符串 {raw} 解析不回自身"
        );
    }
}

/// 持久化字符串的**具体值**固定不变。
///
/// 上一条守「往返」，本条守「**具体是什么**」。
/// 两者都要：往返能守住「加变体时同步加解析」，但守不住
/// 「把 `dark` 改成 `darker`」——那会让用户的旧配置全部失效。
#[test]
fn the_persisted_strings_stay_unchanged() {
    assert_eq!(ThemeKind::Light.as_str(), "light");
    assert_eq!(ThemeKind::Dark.as_str(), "dark");
    assert_eq!(ThemeKind::System.as_str(), "system");
    assert_eq!(ThemeKind::Custom.as_str(), "custom");
}

/// 变体清单恰为四项。
///
/// 钉住「真源有几种主题」。加了新变体时这条会红——**那正是它该红的时候**
/// （要顺带确认下面的显示名表、解析分支、设置页顺序都已跟上）。
#[test]
fn theme_kind_has_exactly_four_variants() {
    assert_eq!(ThemeKind::ALL.len(), 4, "主题种类应恰为四项");
}

/// 解析大小写不敏感、忽略首尾空白。
///
/// 配置里的值可能来自手改的 JSON，也可能来自旧版本。
#[test]
fn parsing_is_forgiving_but_strict() {
    assert_eq!(ThemeKind::parse("LIGHT"), Some(ThemeKind::Light));
    assert_eq!(ThemeKind::parse("  dark  "), Some(ThemeKind::Dark));
    assert_eq!(ThemeKind::parse("System"), Some(ThemeKind::System));
    assert_eq!(ThemeKind::parse("CUSTOM"), Some(ThemeKind::Custom));
    // 不认得的**如实**返回 `None`（不擅自兜底）——
    // 兜底责任在调用点：表现层要「照常启动」，应用层要「报出配置错误」。
    assert_eq!(ThemeKind::parse("sepia"), None);
    assert_eq!(ThemeKind::parse(""), None);
}

/// `FromStr` trait 与 inherent `parse` 的分工。
///
/// - `parse` → `Option`：调用方自己决定兜底策略；
/// - `from_str` → `Result`：错误信息自带**可操作**的提示。
///
/// 两者并存不是重复——clippy 的 `should_implement_trait` 要求
/// 与 `std::str::FromStr` 同名的方法必须是 trait 实现，
/// 故 inherent 版本只能叫别的名字。
#[test]
fn the_two_parsing_entry_points_agree() {
    for &kind in ThemeKind::ALL {
        let raw = kind.as_str();
        assert_eq!(raw.parse::<ThemeKind>().ok(), Some(kind));
    }
    let err = "sepia".parse::<ThemeKind>().unwrap_err();
    assert!(
        err.contains("sepia") && err.contains("light"),
        "错误信息应指出输入值与可选值，实际：{err}"
    );
}

/// 明暗判定的语义边界。
///
/// `System` 与 `Custom` **不算**深色：它们尚未解析成具体配色
/// （`System` 要等系统偏好，`Custom` 要等调色板主背景亮度）。
/// 此时断言明暗会得到「取决于运行环境」的答案。
#[test]
fn only_dark_counts_as_dark() {
    assert!(ThemeKind::Dark.is_dark());
    assert!(!ThemeKind::Light.is_dark());
    assert!(!ThemeKind::System.is_dark());
    assert!(!ThemeKind::Custom.is_dark());
}

/// 三个语义判据各自只认一个变体。
#[test]
fn the_predicates_are_disjoint() {
    assert!(ThemeKind::System.is_system());
    assert!(!ThemeKind::Dark.is_system());
    assert!(ThemeKind::Custom.is_custom());
    assert!(!ThemeKind::Light.is_custom());
}

/// `Display` 与 `as_str` 一致。
///
/// `Display` 是 `as_str` 的便利包装，输出必须**逐字相同**——
/// 否则日志、错误信息、导出文件名里的主题字符串会不一致。
#[test]
fn display_matches_as_str() {
    for &kind in ThemeKind::ALL {
        assert_eq!(kind.to_string(), kind.as_str());
    }
}
