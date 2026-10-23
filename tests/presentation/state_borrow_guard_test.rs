//! D-3 阶段 2：视图对 `app.<state>.<field>` 的**字段级可变借用**白名单（R-12 续）。
//!
//! # 这条守卫处在 R-12 的哪个位置
//!
//! R-12（`check-arch` 内，锚 `R-12-ANCHOR`）拦的是**赋值形态**
//! `app.x.y = …`——存量为 0，且注入必报（fixture 四条用例）。但 egui 的
//! 控件 API 必须拿**可变引用**：`ui.checkbox(&mut app.diff_ui.only_changes, …)`、
//! `TextEdit::singleline(&mut app.find.find_text)`。那是**借用**不是赋值，
//! R-12 按设计放行（fixture 也有一条「借用必须放行」的反向用例）。
//!
//! 放行不等于无监管：字段级借用让控件绕过 state 方法直接读写字段值，
//! 属于**隐性穿透**。散落在十几个视图里时，「谁在写这个字段」没人说得清。
//! 本测试把它们从隐性变成**可枚举的清单**：
//!
//! - 新增一处字段级借用 → **优先方法化**（阶段 3 的「clone（或读出）→ 就地
//!   编辑 → 变化才值写回 + state setter」模板，`mem::take` 类则收敛为
//!   state 的 `take_*` 方法）；清单已归零，想登记就得先动本文件的清单
//!   （改测试文件 = 评审必经）；
//! - 删掉代码忘清清单 → 双向断言同样报红（清单与现实强制同步）；
//! - 清单**只减不增**（基线 0）：集合相等（实际 == 清单）本身就是判据，
//!   往清单里堆条目必然改变判据本身，评审无法绕过。
//!
//! # 为什么用校验器的词法，而不是正则扫源码
//!
//! 注释与文档字符串里出现 `&mut app.x.y` 的**示例**会被正则误收
//! （本文件头自己就写着这个形状）。[`arch_check::lex_dump`] 剔除
//! 注释与字符串——它是 R-01 起已用十余条用例钉过的词法真源，
//! 与 `check-arch` 对「什么是代码」的口径一致。
//!
//! # 边界（设计取舍，不是遗漏）
//!
//! - **只收三级** `&mut app.<state>.<field>`：两级 `&mut app.<state>` 是把
//!   整个 state 句柄传给子视图 / 方法，那是正常的分层协作，全仓上百处，
//!   收进来会让清单失去意义；
//! - **范围 = `src/presentation/`**（与 R-12 的表现层口径一致）：
//!   `state/` / `widgets/` 等子层同样可能借用，实测存量全部在 `views/`；
//! - 四级及更深（`&mut app.x.y.z`）按前缀 `app.x.y` 记账——清单粒度到
//!   「哪个 state 的哪个字段被外部借走」，不追踪字段的子路径。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 校验器词法的本地接入（与 `tests/arch_violation_fixture.rs` 同款先例）。
///
/// `allow(dead_code)` 是必要的：本测试只消费 `lex_dump`，校验器的
/// `scan` / `check_file` 等入口在本编译单元不可达——测试目标的门禁是
/// `-D warnings`，没有这条豁免会在死代码告警上翻车。
#[allow(dead_code)]
mod arch_check {
    include!("../../tools/arch_check.rs");
}

/// 已登记的字段级可变借用：`<相对 src/presentation/ 的路径>::<访问路径>`。
///
/// **阶段 3（2026-10-23）已完成并清零**：原 7 处穿透全部方法化——
/// - 6 处 egui 控件（`checkbox` / `TextEdit`）走「clone（或读出）→ 就地编辑
///   → 变化才值写回」+ state setter（`set_only_changes` / `set_output_dir` /
///   `set_file_stem` / `set_find_text` / `set_replace_text` /
///   `set_writing_goal_text`）；
/// - 1 处 `std::mem::take(&mut …)` 收敛为 `PreviewUiState::take_blocks()`
///   （与 `store_preview_render` 的放回对称）。
///
/// 现在清单为空——**任何新条目都是回退信号**，须先动本清单本身（评审必经）；
/// 清单只减不增、基线 0，由集合相等断言（实际 == 清单）双向钉死。
const APPROVED_BORROWS: &[&str] = &[];

/// 递归收集 `.rs` 文件。
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// 解析 `lex_dump` 的一行 `12:Ident(app)` → `(行号, kind, text)`。
///
/// `allow@…`（豁免指令行）不是 token，跳过——它是词法的副产物，
/// 混进 token 流会让窗口匹配错位。
fn parse_dump_line(line: &str) -> Option<(usize, String, String)> {
    if line.starts_with("allow@") {
        return None;
    }
    let (no, rest) = line.split_once(':')?;
    let line_no: usize = no.parse().ok()?;
    let (kind, after_kind) = rest.split_once('(')?;
    let (text, _) = after_kind.rsplit_once(')')?;
    Some((line_no, kind.to_string(), text.to_string()))
}

/// 从一份源码里收集字段级可变借用（词法级，注释与字符串不产生 token）。
///
/// 匹配形状：`&` `mut` `app` `.` `ident` `.` `ident`。
/// 两级 `&mut app.x`（后无第二段点访问）与 `&mut` 的其它目标均不命中。
fn borrows_in(source: &str) -> Vec<String> {
    let dump = arch_check::lex_dump(source);
    let toks: Vec<(usize, String, String)> = dump.lines().filter_map(parse_dump_line).collect();
    let mut out = Vec::new();
    for w in toks.windows(7) {
        let is_field_borrow = w[0].1 == "Amp"
            && w[1].1 == "Ident"
            && w[1].2 == "mut"
            && w[2].1 == "Ident"
            && w[2].2 == "app"
            && w[3].1 == "Dot"
            && w[4].1 == "Ident"
            && w[5].1 == "Dot"
            && w[6].1 == "Ident";
        if is_field_borrow {
            out.push(format!("app.{}.{}", w[4].2, w[6].2));
        }
    }
    out
}

/// 匹配逻辑自身的**注入用例**（与 `arch_violation_fixture` 同思路）：
///
/// 真实工程的存量扫描只验证「现状与清单相等」——若 `borrows_in` 的匹配
/// 本身写错了（把注释里的示例也收进来、或两级借用漏判成三级），
/// 现状扫描照样「通过」。判据的正确性只能靠注入现形。
mod borrows_in_injection {
    use super::borrows_in;

    /// 必收：真实字段级借用；必不收：注释、文档、字符串字面量里的同形示例。
    #[test]
    fn collects_real_borrows_and_ignores_non_code_shapes() {
        let src = r#"
            // 行注释示例：&mut app.line_comment.field
            /// 文档示例：`&mut app.doc_comment.field`
            let s = "&mut app.string_literal.field";
            ui.checkbox(&mut app.real.field, "x");
            let t = egui::TextEdit::singleline(&mut app.edit.text);
        "#;
        assert_eq!(
            borrows_in(src),
            vec!["app.real.field", "app.edit.text"],
            "只收代码里的字段级借用，注释与字符串必须被词法剔除"
        );
    }

    /// 必不收：两级整体借用（传 state 句柄，正常分层）、比较、赋值——
    /// 后两者分别归 R-12 的放行边界与赋值规则管，不归本清单。
    #[test]
    fn skips_two_level_borrows_comparisons_and_assignments() {
        let src = r#"
            show_panel(&mut app.sidebar_ui);
            if app.a.b == other { }
            app.state_a.field = 1;
            let ok = &mut app;
        "#;
        assert!(
            borrows_in(src).is_empty(),
            "两级借用 / 比较 / 赋值都不该进清单：{:?}",
            borrows_in(src)
        );
    }
}

#[test]
fn field_level_mut_borrows_match_the_registry() {
    // 两根扫描（D-3 阶段 4a workspace 化）：非视图子模块（state/theme/…）
    // 在 core，视图在 root。键仍按各自 presentation 根的相对路径记，
    // 与清单格式（`state/xxx.rs::…`）完全一致。
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let roots = [
        base.join("src").join("presentation"),
        base.join("crates/rustmark-core/src/presentation"),
    ];
    let mut files = Vec::new();
    for root in &roots {
        let mut buf = Vec::new();
        collect_rs(root, &mut buf);
        files.extend(buf.into_iter().map(|f| (root.clone(), f)));
    }
    files.sort();
    assert!(!files.is_empty(), "两侧 presentation 下未找到任何 .rs 文件");

    // 实际存量（键 = 相对 src/presentation 的路径 :: 访问路径；BTreeSet 去重
    // ——同一字段在同文件被借多次仍是一个管理条目，清单不追踪出现次数）。
    let mut actual: BTreeSet<String> = BTreeSet::new();
    for (root, path) in &files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        for borrow in borrows_in(&source) {
            actual.insert(format!("{rel}::{borrow}"));
        }
    }

    let expected: BTreeSet<String> = APPROVED_BORROWS.iter().map(|s| s.to_string()).collect();

    let missing: Vec<_> = actual.difference(&expected).collect();
    let stale: Vec<_> = expected.difference(&actual).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "字段级可变借用清单与实际不一致（清单只减不增，当前基线 0）。\n\
         \n\
         未登记的新穿透：清单不接受条目——正确出路是按「clone（或读出）→ 就地编辑 →\
         变化才值写回 + state setter」模板方法化（`mem::take` 类收敛为 `take_*` 方法），\
         而非往 APPROVED_BORROWS 加条目（那必须先改判据，评审必经）：{missing:?}\n\
         \n\
         清单中的过期项（代码已删，清单须同步移除——双向同步是本测试的判据）：{stale:?}\n\
         \n\
         当前实际存量：{actual:?}"
    );
}
