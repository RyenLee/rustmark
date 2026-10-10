//! 快捷键注册表与分发器用例（S6-09 验收标准 ① ~ ⑤）。
//!
//! 五条验收标准逐条对应：
//! - ① 绑定 `F8` 为专注模式后按F8 生效；
//! - ② 绑定 `Ctrl+Shift+S` 为文档统计后生效（原型 B-02）；
//! - ③ 把「关闭浮层」改为 `Ctrl+Q` 后 `Ctrl+Q` 生效、`Esc` **不再**触发（原型 B-03）；
//! - ④ 改成已占用的 `Ctrl+S` 时能检出冲突，不静默覆盖（原型 B-04）；
//! - ⑤ 改键后菜单 / 命令面板 / 快捷键一览三处显示同步（原型 B-08）。
//!
//! 外加一组**差分测试**：表现层的 [`normalize`] 必须与领域 `KeyCombo`
//! 逐字一致。这是分层隔离的代价——两份实现，必须用测试锁死。

use rustmark::application::dto::ShortcutView;
use rustmark::presentation::shortcuts::dispatcher::{
    Dispatch, ShortcutDispatcher, is_global_safe_single_key, key_of, modifiers_from,
};
use rustmark::presentation::shortcuts::registry::{
    Key, Modifiers, NamedKey, ShortcutRegistry, UiAction, normalize,
};

// ============================================================================
// 差分测试：表现层 normalize ≡ 领域 KeyCombo
// ============================================================================

/// 领域侧的对照实现（测试内联，避免测试依赖表现层）。
mod domain_ref {
    /// 领域 `NamedKey` 的别名集（逐字抄自
    /// `src/domain/preferences/value_objects.rs`，故意重复以形成对照）。
    pub fn named_display(raw: &str) -> Option<&'static str> {
        Some(match raw.to_ascii_lowercase().as_str() {
            "esc" | "escape" => "Esc",
            "enter" | "return" | "cr" => "Enter",
            "tab" => "Tab",
            "space" | " " => "Space",
            "backspace" => "Backspace",
            "delete" | "del" => "Delete",
            "up" => "Up",
            "down" => "Down",
            "left" => "Left",
            "right" => "Right",
            "home" => "Home",
            "end" => "End",
            "pageup" | "pgup" => "PageUp",
            "pagedown" | "pgdn" => "PageDown",
            "insert" | "ins" => "Insert",
            "f1" => "F1",
            "f2" => "F2",
            "f3" => "F3",
            "f4" => "F4",
            "f5" => "F5",
            "f6" => "F6",
            "f7" => "F7",
            "f8" => "F8",
            "f9" => "F9",
            "f10" => "F10",
            "f11" => "F11",
            "f12" => "F12",
            _ => return None,
        })
    }

    /// 领域 `KeyCombo::display()` 的等价实现。
    pub fn display(raw: &str) -> Option<String> {
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut meta = false;
        let mut key: Option<String> = None;

        for token in raw.split('+') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "alt" | "option" => alt = true,
                "shift" => shift = true,
                "meta" | "cmd" | "command" | "super" | "win" => meta = true,
                other => {
                    let parsed = named_display(other).map(str::to_string).or_else(|| {
                        let mut chars = other.chars();
                        let first = chars.next()?;
                        if chars.next().is_some() {
                            return None;
                        }
                        Some(if first.is_ascii_alphabetic() {
                            first.to_ascii_uppercase().to_string()
                        } else {
                            first.to_string()
                        })
                    })?;
                    if key.is_some() {
                        return None;
                    }
                    key = Some(parsed);
                }
            }
        }

        let key = key?;
        let mut out = String::new();
        if ctrl {
            out.push_str("Ctrl+");
        }
        if alt {
            out.push_str("Alt+");
        }
        if shift {
            out.push_str("Shift+");
        }
        if meta {
            out.push_str("Meta+");
        }
        out.push_str(&key);
        Some(out)
    }
}

/// 归一化必须与领域逐字一致（含拒绝集合）。
///
/// 这是本次分层最脆弱的一环：表现层为了不依赖 domain 而复刻了
/// `KeyCombo`，两份代码一旦漂移，用户看到的键位就与实际触发的键位不符——
/// 而且**没有任何测试会失败**，只是功能静默失灵。
#[test]
fn normalization_matches_domain_key_combo() {
    let cases = [
        // 基本形态。
        "Ctrl+S",
        "Ctrl+Shift+S",
        "Ctrl+Alt+S",
        "Ctrl+Alt+Shift+S",
        "Ctrl+,",
        "Alt+Up",
        "Alt+1",
        "Esc",
        "F8",
        "F12",
        "Enter",
        "Tab",
        "Backspace",
        "Delete",
        "PageUp",
        "PageDown",
        // 大小写与空格：书写差异必须归一。
        "ctrl+s",
        "CTRL+SHIFT+S",
        "Ctrl + Shift + S",
        "ctrl + shift + s",
        // 修饰键顺序：判等的关键。
        "Shift+Ctrl+S",
        "Alt+Ctrl+S",
        "Shift+Alt+Ctrl+S",
        //别名。
        "Escape",
        "escape",
        "Return",
        "Del",
        "PgUp",
        "Ins",
        // 大小写字母归一。
        "Ctrl+b",
        "Ctrl+B",
        // 标点。
        "Ctrl+[",
        "Ctrl+]",
        "Ctrl+\\",
        "Ctrl+/",
        "Ctrl+=",
        "Ctrl+-",
        // Meta 系。
        "Meta+W",
        "Cmd+K",
        "Super+J",
    ];

    for raw in cases {
        let ours = normalize(raw)
            .unwrap_or_else(|e| panic!("表现层应能解析 {raw:?}，实际错误：{e}"))
            .display();
        let theirs = domain_ref::display(raw).unwrap_or_else(|| panic!("领域侧应能解析 {raw:?}"));
        assert_eq!(ours, theirs, "归一化结果不一致：输入 {raw:?}");
    }
}

/// 非法输入的拒绝行为也必须一致。
///
/// 注意 `Ctrl++A` **不在**此列：空片段两侧都被跳过，`Ctrl++A` 等价于
/// `Ctrl+A`（领域层同样如此，见`KeyCombo::parse` 里`if token.is_empty() { continue }`）。
/// 这里比的是「两侧判定一致」，而不是「谁更严」。
#[test]
fn normalization_rejects_same_inputs_as_domain() {
    let rejected = ["", "Ctrl+", "Ctrl+S+A", "!!!", "Ctrl+NotAKey"];
    for raw in rejected {
        assert!(
            normalize(raw).is_err(),
            "表现层不应接受 {raw:?}，但解析成功了"
        );
        assert!(
            domain_ref::display(raw).is_none(),
            "领域侧接受 {raw:?}，两侧判定不一致"
        );
    }

    // 空片段被跳过：两侧都把 `Ctrl++A` 当成 `Ctrl+A`。
    assert_eq!(
        normalize("Ctrl++A").expect("应可解析").display(),
        domain_ref::display("Ctrl++A").expect("领域侧也应可解析")
    );
}

/// 大小写与顺序必须归一到同一值（原型缺陷 B-02/C 的根治）。
#[test]
fn equivalent_spellings_produce_identical_display() {
    let groups: &[&[&str]] = &[
        &[
            "Ctrl+Shift+S",
            "ctrl+shift+s",
            "Shift+Ctrl+S",
            "Ctrl + Shift + s",
        ],
        &["Esc", "Escape", "escape"],
        &["Ctrl+B", "ctrl+b", "Ctrl+b"],
    ];
    for group in groups {
        let first = normalize(group[0]).expect("应可解析").display();
        for raw in &group[1..] {
            let got = normalize(raw).expect("应可解析").display();
            assert_eq!(got, first, "{raw:?} 应与 {:?} 归一到同一值", group[0]);
        }
    }
}

// ============================================================================
// 验收 ①：F8 绑定专注模式后按 F8 生效（原型 B-01）
// ============================================================================

/// `F8` 无修饰键也必须能触发——原型的匹配器在这里把功能键全拒了。
#[test]
fn bare_function_key_triggers_bound_action() {
    let mut registry = ShortcutRegistry::with_defaults();
    // 先把默认的 F8 绑定清掉，再显式绑到专注模式，模拟用户改键。
    let f8 = normalize("F8").expect("F8 应可解析");
    registry.unbind(UiAction::ToggleFocusMode);
    let displaced = registry.rebind(UiAction::ToggleFocusMode, f8);
    assert!(displaced.is_none(), "F8 此时不应撞上任何动作");

    let dispatcher = ShortcutDispatcher::new(registry);
    assert_eq!(
        dispatcher.dispatch_combo(f8),
        Dispatch::Triggered(UiAction::ToggleFocusMode),
        "F8 必须能触发专注模式（原型 B-01 的直接回归用例）"
    );
}

/// `Esc` 同样是无修饰键单键，必须可触发。
#[test]
fn bare_escape_triggers_bound_action() {
    let dispatcher = ShortcutDispatcher::default();
    let esc = normalize("Esc").expect("Esc 应可解析");
    assert_eq!(
        dispatcher.dispatch_combo(esc),
        Dispatch::Triggered(UiAction::CloseOverlay),
        "Esc 必须能触发关闭浮层"
    );
}

/// 无修饰键单键**不得**被匹配层拒绝（这是原型 B-01 的结构性根治点）。
#[test]
fn single_keys_are_never_rejected_by_the_matcher() {
    let dispatcher = ShortcutDispatcher::default();
    // 未绑定的裸单键应返回 NoMatch（而不是被「忽略」逻辑吞掉后
    // 走另一条分支）；关键在于它确实进入了匹配流程。
    for raw in ["A", "Z", "1", "9", "/", "["] {
        let combo = normalize(raw).expect("应可解析");
        assert_eq!(
            dispatcher.dispatch_combo(combo),
            Dispatch::NoMatch,
            "{raw} 未绑定任何动作，应为NoMatch"
        );
    }
    // 功能键与 Esc 被判定为「适合全局绑定」，但这只是**提示**。
    assert!(is_global_safe_single_key(Key::Function(8)));
    assert!(is_global_safe_single_key(Key::Named(NamedKey::Escape)));
    // 普通单键会被提示加修饰键——同样只是提示，不影响分发。
    assert!(!is_global_safe_single_key(Key::Char('A')));
    // 关键：`A` 依然能被显式绑定并触发（原型禁止了这一点）。
    let mut registry = ShortcutRegistry::with_defaults();
    let a = normalize("A").expect("应可解析");
    registry.rebind(UiAction::Bold, a);
    let dispatcher = ShortcutDispatcher::new(registry);
    assert_eq!(
        dispatcher.dispatch_combo(a),
        Dispatch::Triggered(UiAction::Bold),
        "裸单键可绑定即可触发，提示不得升级为拒绝"
    );
}

// ============================================================================
// 验收 ②：Ctrl+Shift+S 绑给文档统计（原型 B-02）
// ============================================================================

/// `Ctrl+Shift+S` 必须是文档统计的默认键，且不被任何其它动作占用。
#[test]
fn ctrl_shift_s_belongs_to_document_stats() {
    let registry = ShortcutRegistry::with_defaults();
    let combo = normalize("Ctrl+Shift+S").expect("应可解析");
    assert_eq!(
        registry.resolve(combo),
        Some(UiAction::DocumentStats),
        "Ctrl+Shift+S 应触发文档统计（原型声明了却从不注册，B-02）"
    );
    // 「声明了但没注册」的另一种表现：注册表里有这个动作，但组合键映射缺失。
    assert!(
        registry.combo_for(UiAction::DocumentStats).is_some(),
        "文档统计必须有实际注册的组合键"
    );
}

/// 应用层下发的绑定必须被采纳（用户改键后重启仍生效）。
#[test]
fn settings_override_replaces_default_binding() {
    let views = vec![
        ShortcutView {
            key: "save".to_string(),
            action: "保存".to_string(),
            combo: "Ctrl+Alt+S".to_string(),
        },
        ShortcutView {
            key: "bold".to_string(),
            action: "加粗".to_string(),
            combo: "F9".to_string(),
        },
    ];
    let registry = ShortcutRegistry::from_settings(&views);
    let dispatcher = ShortcutDispatcher::new(registry);

    // 改绑后新键生效。
    assert_eq!(
        dispatcher.dispatch_combo(normalize("Ctrl+Alt+S").unwrap()),
        Dispatch::Triggered(UiAction::Save)
    );
    // 旧键不再触发——否则会留下「两个键都能存盘」的幽灵绑定。
    assert_eq!(
        dispatcher.dispatch_combo(normalize("Ctrl+S").unwrap()),
        Dispatch::NoMatch,
        "改绑后旧键位必须失效"
    );
    // 改成裸功能键也必须生效。
    assert_eq!(
        dispatcher.dispatch_combo(normalize("F9").unwrap()),
        Dispatch::Triggered(UiAction::Bold)
    );
}

// ============================================================================
// 验收 ③：改绑关闭浮层后 Esc 失效（原型 B-03）
// ============================================================================

/// 把「关闭浮层」改绑到 `Ctrl+Q` 后：`Ctrl+Q` 生效、`Esc` 不再触发。
///
/// 这条用例同时证明**没有硬编码 Esc 分支**——若代码里写了
/// `if key == Escape { close_overlay() }`，本用例必然失败。
#[test]
fn rebinding_close_overlay_disables_escape() {
    let views = vec![ShortcutView {
        key: "close_overlay".to_string(),
        action: "关闭浮层".to_string(),
        combo: "Ctrl+Q".to_string(),
    }];
    let registry = ShortcutRegistry::from_settings(&views);
    let dispatcher = ShortcutDispatcher::new(registry);

    assert_eq!(
        dispatcher.dispatch_combo(normalize("Ctrl+Q").unwrap()),
        Dispatch::Triggered(UiAction::CloseOverlay),
        "改绑后 Ctrl+Q 必须生效"
    );
    assert_eq!(
        dispatcher.dispatch_combo(normalize("Esc").unwrap()),
        Dispatch::NoMatch,
        "改绑后 Esc 必须失效——若仍生效，说明存在硬编码 Esc 分支（B-03）"
    );
}

// ============================================================================
// 验收 ④：冲突检出而非静默覆盖（原型 B-04）
// ============================================================================

/// 绑到已被占用的组合时，必须**报告**被挤掉的动作。
#[test]
fn rebinding_onto_occupied_combo_reports_displaced_action() {
    let mut registry = ShortcutRegistry::with_defaults();
    let ctrl_s = normalize("Ctrl+S").expect("应可解析");

    // 默认 Ctrl+S 属于保存。
    assert_eq!(registry.resolve(ctrl_s), Some(UiAction::Save));

    let displaced = registry.rebind(UiAction::Bold, ctrl_s);
    assert_eq!(
        displaced,
        Some(UiAction::Save),
        "把加粗绑到 Ctrl+S 必须报告保存被挤掉，否则用户会以为存盘键坏了"
    );
    assert_eq!(registry.resolve(ctrl_s), Some(UiAction::Bold), "后绑的生效");
    // 被挤掉的动作变为「无绑定」，UI 必须能展示这一点。
    assert_eq!(registry.combo_for(UiAction::Save), None);
    assert_eq!(registry.display_for(UiAction::Save), "");
}

/// 改回自己原来的键不应报告「挤掉自己」。
#[test]
fn rebinding_to_own_combo_is_not_a_conflict() {
    let mut registry = ShortcutRegistry::with_defaults();
    let ctrl_s = normalize("Ctrl+S").unwrap();
    assert_eq!(registry.rebind(UiAction::Save, ctrl_s), None);
}

// ============================================================================
// 验收 ⑤：三处显示同步（原型 B-08）
// ============================================================================

/// 菜单、命令面板、快捷键一览三处**必须**读到同一个改键结果。
#[test]
fn three_surfaces_show_identical_shortcut_text() {
    let views = vec![ShortcutView {
        key: "document_stats".to_string(),
        action: "文档统计".to_string(),
        combo: "Ctrl+Alt+Shift+9".to_string(),
    }];
    let dispatcher = ShortcutDispatcher::new(ShortcutRegistry::from_settings(&views));
    let table = dispatcher.table();

    // 三处各自的取法（模拟三个 UI 组件）。
    let from_menu = table.entry(UiAction::DocumentStats).combo;
    let from_palette = table
        .all()
        .into_iter()
        .find(|e| e.action == UiAction::DocumentStats)
        .expect("命令面板应含该动作")
        .combo;
    let from_overview = dispatcher.registry().display_for(UiAction::DocumentStats);

    assert_eq!(from_menu, "Ctrl+Alt+Shift+9");
    assert_eq!(from_palette, from_menu, "命令面板与菜单显示不一致");
    assert_eq!(from_overview, from_menu, "一览与菜单显示不一致");
}

/// 「快捷键一览」应能列出全部已绑定动作，且顺序稳定。
#[test]
fn shortcut_overview_lists_all_bindings_in_stable_order() {
    let registry = ShortcutRegistry::with_defaults();
    let entries = registry.entries();

    // 至少覆盖文件/编辑/格式/视图/帮助五组的代表动作。
    for action in [
        UiAction::NewDocument,
        UiAction::Save,
        UiAction::Undo,
        UiAction::Bold,
        UiAction::TogglePreview,
        UiAction::ToggleFocusMode,
        UiAction::CloseOverlay,
        UiAction::OpenSettings,
    ] {
        assert!(
            entries.iter().any(|(a, _)| *a == action),
            "一览缺少动作：{action:?}"
        );
    }

    // 顺序稳定：连续两次调用结果一致（按动作枚举序）。
    assert_eq!(entries, registry.entries());
}

// ============================================================================
// 动作表完整性
// ============================================================================

/// 默认绑定表内部不得有重复组合。
#[test]
fn default_bindings_have_no_duplicates() {
    let registry = ShortcutRegistry::with_defaults();
    let entries = registry.entries();
    let mut seen = std::collections::HashSet::new();
    for (action, combo) in entries {
        assert!(
            seen.insert(combo),
            "{action:?} 的 {combo} 与其它动作重复；HashMap 会静默丢弃后者"
        );
    }
}

/// 每个可持久化动作的键必须与领域 `ActionId::key()` 对齐，
/// 否则用户改键无法落盘。
#[test]
fn persisted_keys_match_domain_action_keys() {
    // 领域侧的键名（抄自 `ActionId::key()`，故意重复形成对照）。
    let domain_keys = [
        "new_document",
        "open_document",
        "save",
        "save_as",
        "save_all",
        "close_tab",
        "export",
        "undo",
        "redo",
        "find",
        "replace",
        "find_in_files",
        "bold",
        "italic",
        "strikethrough",
        "inline_code",
        "insert_link",
        "code_block",
        "insert_table",
        "toggle_quote",
        "promote_heading",
        "demote_heading",
        "indent",
        "outdent",
        "toggle_task",
        "toggle_preview",
        "toggle_outline",
        "toggle_sidebar",
        "toggle_statusbar",
        "command_palette",
        "open_settings",
        "focus_mode",
        "close_overlay",
        "document_stats",
        "insert_symbol",
        "compare_diff",
        "next_tab",
        "prev_tab",
    ];
    for key in domain_keys {
        assert!(
            UiAction::from_persisted_key(key).is_some(),
            "领域动作 {key} 在表现层没有对应项，改键将无法落盘"
        );
    }
}

/// 命令面板必须能搜到全部动作（空查询返回全集）。
#[test]
fn empty_query_returns_every_action() {
    let registry = ShortcutRegistry::with_defaults();
    let hits = registry.table().search("");
    assert_eq!(hits.len(), UiAction::ALL.len(), "空查询应返回全部命令");
}

/// 模糊检索：中文与英文都能命中。
#[test]
fn fuzzy_search_matches_chinese_and_english() {
    let registry = ShortcutRegistry::with_defaults();
    let table = registry.table();

    let chinese = table.search("保存");
    assert!(
        chinese.iter().any(|h| h.entry.action == UiAction::Save),
        "中文检索应命中「保存」"
    );

    let english = table.search("bold");
    assert!(
        english.iter().any(|h| h.entry.action == UiAction::Bold),
        "英文检索应命中加粗"
    );

    // 子序列匹配：`bd` 是 `bold` 的子序列（跳过 o、l）。
    let subsequence = table.search("bd");
    assert!(
        subsequence.iter().any(|h| h.entry.action == UiAction::Bold),
        "子序列检索 bd 应命中 bold"
    );

    // 无匹配返回空。
    assert!(table.search("zzzzqqq").is_empty());
}

/// 命中索引必须落在 `char` 边界上（按字节切会把中文劈开）。
#[test]
fn fuzzy_hits_are_char_aligned() {
    let registry = ShortcutRegistry::with_defaults();
    let hits = registry.table().search("存");
    let found = hits
        .iter()
        .find(|h| h.entry.action == UiAction::Save)
        .expect("应命中保存");
    let label: Vec<char> = UiAction::Save.label().chars().collect();
    for index in &found.hits {
        assert!(
            *index < label.len(),
            "命中索引 {index} 超出标签字符数 {}",
            label.len()
        );
    }
}

/// 标签命中应排在关键词命中之前（搜「保存」先出「保存」而非「保存全部」）。
#[test]
fn label_matches_outrank_keyword_matches() {
    let registry = ShortcutRegistry::with_defaults();
    let hits = registry.table().search("保存");
    let save_index = hits
        .iter()
        .position(|h| h.entry.action == UiAction::Save)
        .expect("应命中保存");
    let save_all_index = hits
        .iter()
        .position(|h| h.entry.action == UiAction::SaveAll);
    if let Some(other) = save_all_index {
        assert!(
            save_index < other,
            "「保存」应排在「保存全部」之前（标签命中权重更高）"
        );
    }
}

// ============================================================================
// 分发器：egui 事件翻译
// ============================================================================

/// egui 按键必须能翻译成与配置一致的组合键。
#[test]
fn egui_keys_translate_to_registered_combos() {
    use egui::Key as EguiKey;

    let dispatcher = ShortcutDispatcher::default();

    // Ctrl+S
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..egui::Modifiers::NONE
    };
    let combo = ShortcutDispatcher::combo_of(modifiers_from(modifiers), EguiKey::S).unwrap();
    assert_eq!(
        dispatcher.dispatch_combo(combo),
        Dispatch::Triggered(UiAction::Save)
    );

    // F8 无修饰键。
    let combo = ShortcutDispatcher::combo_of(Modifiers::NONE, EguiKey::F8).unwrap();
    assert_eq!(
        dispatcher.dispatch_combo(combo),
        Dispatch::Triggered(UiAction::ToggleFocusMode)
    );

    // Esc。
    let combo = ShortcutDispatcher::combo_of(Modifiers::NONE, EguiKey::Escape).unwrap();
    assert_eq!(
        dispatcher.dispatch_combo(combo),
        Dispatch::Triggered(UiAction::CloseOverlay)
    );
}

/// macOS 的 `⌘` 必须映射到 `Meta`（配置里写 `Cmd+K` 的约定），
/// 而不是折成 `Ctrl`——否则 `Cmd+K` 这条绑定永远匹配不上。
#[test]
fn mac_command_maps_to_meta_not_ctrl() {
    use egui::Key as EguiKey;

    let modifiers = egui::Modifiers {
        mac_cmd: true,
        command: true,
        ..egui::Modifiers::NONE
    };
    let folded = modifiers_from(modifiers);
    assert!(folded.meta, "macOS 的 ⌘ 应映射到 Meta");
    assert!(
        !folded.ctrl,
        "⌘ 不得同时计入 Ctrl，否则 Cmd+K 与 Ctrl+K 会互相触发"
    );

    let combo = ShortcutDispatcher::combo_of(folded, EguiKey::K).unwrap();
    assert_eq!(combo.display(), "Meta+K");

    // 该组合确实能命中 Cmd+K 的绑定。
    let mut registry = ShortcutRegistry::with_defaults();
    registry.rebind(UiAction::InsertLink, combo);
    // 默认即非模态：应用启动时没有任何浮层，浮层动作必须可用。
    let dispatcher = ShortcutDispatcher::new(registry);
    assert_eq!(
        dispatcher.dispatch_combo(combo),
        Dispatch::Triggered(UiAction::InsertLink)
    );
}

/// 默认必须是非模态——否则启动时 `Ctrl+,`（设置）等入口在首帧就全部失效。
#[test]
fn dispatcher_starts_non_modal() {
    let dispatcher = ShortcutDispatcher::default();
    assert_eq!(
        dispatcher.dispatch_combo(normalize("Ctrl+,").unwrap()),
        Dispatch::Triggered(UiAction::OpenSettings),
        "启动时设置入口必须可用"
    );
    assert_eq!(
        dispatcher.dispatch_combo(normalize("Ctrl+F").unwrap()),
        Dispatch::Triggered(UiAction::Find),
        "启动时查找入口必须可用"
    );
}

/// 非macOS 平台：`command`（= Ctrl）折成 `Ctrl`，`Meta` 保持为空。
#[test]
fn non_mac_command_maps_to_ctrl() {
    use egui::Key as EguiKey;

    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..egui::Modifiers::NONE
    };
    let folded = modifiers_from(modifiers);
    assert!(folded.ctrl);
    assert!(!folded.meta);

    let combo = ShortcutDispatcher::combo_of(folded, EguiKey::S).unwrap();
    assert_eq!(combo.display(), "Ctrl+S");

    let dispatcher = ShortcutDispatcher::default();
    assert_eq!(
        dispatcher.dispatch_combo(combo),
        Dispatch::Triggered(UiAction::Save)
    );
}

/// egui 的全部功能键都要能翻译（否则 F9/F12 绑不上）。
#[test]
fn all_function_keys_translate() {
    use egui::Key as EguiKey;

    let pairs = [
        (EguiKey::F1, 1u8),
        (EguiKey::F2, 2),
        (EguiKey::F3, 3),
        (EguiKey::F4, 4),
        (EguiKey::F5, 5),
        (EguiKey::F6, 6),
        (EguiKey::F7, 7),
        (EguiKey::F8, 8),
        (EguiKey::F9, 9),
        (EguiKey::F10, 10),
        (EguiKey::F11, 11),
        (EguiKey::F12, 12),
    ];
    for (egui_key, number) in pairs {
        assert_eq!(
            key_of(egui_key),
            Some(Key::Function(number)),
            "{egui_key:?} 翻译错误"
        );
    }
}

/// 浮层模态时，打开浮层的动作让位，但关闭浮层永远可用。
#[test]
fn modal_blocks_overlay_openers_but_never_closes() {
    let mut dispatcher = ShortcutDispatcher::default();
    dispatcher.set_modal(true);

    // 打开设置这类动作应让位。
    assert_eq!(
        dispatcher.dispatch_combo(normalize("Ctrl+,").unwrap()),
        Dispatch::NoMatch,
        "浮层打开时，切换设置页面的动作应让位"
    );
    // 但关闭浮层必须永远可用，否则用户会被困住。
    assert_eq!(
        dispatcher.dispatch_combo(normalize("Esc").unwrap()),
        Dispatch::Triggered(UiAction::CloseOverlay),
        "关闭浮层永远可用（这是去掉硬编码 Esc 后仍不困住用户的关键）"
    );
    // 非浮层动作（如保存）照常工作。
    assert_eq!(
        dispatcher.dispatch_combo(normalize("Ctrl+S").unwrap()),
        Dispatch::Triggered(UiAction::Save),
        "保存不属浮层动作，应照常触发"
    );

    // 关闭模态后一切恢复。
    dispatcher.set_modal(false);
    assert_eq!(
        dispatcher.dispatch_combo(normalize("Ctrl+,").unwrap()),
        Dispatch::Triggered(UiAction::OpenSettings)
    );
}

/// 解除绑定后该组合不再触发。
#[test]
fn unbind_removes_trigger() {
    let mut registry = ShortcutRegistry::with_defaults();
    let combo = normalize("Ctrl+N").unwrap();
    assert!(registry.combo_for(UiAction::NewDocument).is_some());

    registry.unbind(UiAction::NewDocument);
    assert_eq!(registry.combo_for(UiAction::NewDocument), None);

    let dispatcher = ShortcutDispatcher::new(registry);
    assert_eq!(dispatcher.dispatch_combo(combo), Dispatch::NoMatch);
}

/// 配置里的组合无法解析时，必须退回默认绑定而不是让整个面板瘫掉。
#[test]
fn malformed_setting_falls_back_to_default() {
    let views = vec![ShortcutView {
        key: "save".to_string(),
        action: "保存".to_string(),
        combo: "这不是一个快捷键".to_string(),
    }];
    let registry = ShortcutRegistry::from_settings(&views);
    // 保存退回 Ctrl+S，而不是变成「无快捷键」。
    assert_eq!(
        registry.resolve(normalize("Ctrl+S").unwrap()),
        Some(UiAction::Save)
    );
}

/// 配置里的未知键必须被忽略（旧版读新配置是常态）。
#[test]
fn unknown_action_key_in_settings_is_ignored() {
    let views = vec![
        ShortcutView {
            key: "some_future_action".to_string(),
            action: "未来功能".to_string(),
            combo: "Ctrl+Alt+F12".to_string(),
        },
        ShortcutView {
            key: "save".to_string(),
            action: "保存".to_string(),
            combo: "Ctrl+Alt+S".to_string(),
        },
    ];
    let registry = ShortcutRegistry::from_settings(&views);
    assert_eq!(
        registry.resolve(normalize("Ctrl+Alt+S").unwrap()),
        Some(UiAction::Save)
    );
    assert_eq!(
        registry.resolve(normalize("Ctrl+Alt+F12").unwrap()),
        None,
        "未知动作不应凭空获得快捷键"
    );
}

// ============================================================================
// 动作表分组
// ============================================================================

/// 每个动作都必须能归到一个分组，且分组标签非空。
#[test]
fn every_action_belongs_to_a_group() {
    for action in UiAction::ALL {
        assert!(
            !action.group().label().is_empty(),
            "{action:?} 缺少分组标签"
        );
        assert!(!action.label().is_empty(), "{action:?} 缺少显示名");
    }
}

/// 菜单渲染：每个分组都应能取到条目，且条目带键位文本。
#[test]
fn menu_groups_render_with_combo_text() {
    let registry = ShortcutRegistry::with_defaults();
    let table = registry.table();
    for group in rustmark::presentation::shortcuts::registry::MenuGroup::ALL {
        let entries = table.group(*group);
        assert!(!entries.is_empty(), "分组 {:?} 不应为空", group.label());
        // 每个分组至少有一个带键位的条目。
        assert!(
            entries.iter().any(|e| !e.combo.is_empty()),
            "分组 {:?} 中没有任何带快捷键的条目",
            group.label()
        );
    }
}

/// 格式类动作需要编辑区焦点（命令面板触发后要先聚焦编辑区）。
#[test]
fn format_actions_require_editor_focus() {
    assert!(UiAction::Bold.needs_editor_focus());
    assert!(UiAction::InsertTable.needs_editor_focus());
    assert!(!UiAction::Save.needs_editor_focus());
    assert!(!UiAction::TogglePreview.needs_editor_focus());
}
