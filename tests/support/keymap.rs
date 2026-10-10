//! 表现层测试共享工具：键位映射。
//!
//! # 为什么要共享
//!
//! 「表现层 `Key` → egui `Key`」的映射在 S6-07 的 `toolbar_test.rs` 里
//! 写了一份。S6-08 需要构造真实按键事件（验证 `Ctrl+H`、
//! 「查找框聚焦时快捷键让位」），若再抄一份就会有两份可能不同步的表——
//! 改了一处忘了另一处，症状是「某些键位的用例莫名其妙地红」。
//!
//! 这正是本项目在 S6-07 反复治的病（动作表 / 键位表 / 焦点规则各有份真相），
//! 测试代码同样适用。

#![allow(dead_code)]

use rustmark::presentation::shortcuts::registry::{Key as UiKey, Modifiers, NamedKey};

/// 表现层 `Key` → egui `Key`。
///
/// 遇到未覆盖的键**直接 panic**：静默返回一个错的键会让断言以
/// 「键位没注册」的形式失败，把排查方向带偏（S6-07 已踩过这个坑——
/// 手写的 `Modifiers` 映射漏置 `command`，症状是「测试里按快捷键没反应」，
/// 极易被误判成「快捷键表坏了」）。
pub fn egui_key_of(key: UiKey) -> egui::Key {
    match key {
        UiKey::Char(c) if c.is_ascii_alphabetic() => match c {
            'A' => egui::Key::A,
            'B' => egui::Key::B,
            'C' => egui::Key::C,
            'D' => egui::Key::D,
            'E' => egui::Key::E,
            'F' => egui::Key::F,
            'G' => egui::Key::G,
            'H' => egui::Key::H,
            'I' => egui::Key::I,
            'J' => egui::Key::J,
            'K' => egui::Key::K,
            'L' => egui::Key::L,
            'M' => egui::Key::M,
            'N' => egui::Key::N,
            'O' => egui::Key::O,
            'P' => egui::Key::P,
            'Q' => egui::Key::Q,
            'R' => egui::Key::R,
            'S' => egui::Key::S,
            'T' => egui::Key::T,
            'U' => egui::Key::U,
            'V' => egui::Key::V,
            'W' => egui::Key::W,
            'X' => egui::Key::X,
            'Y' => egui::Key::Y,
            'Z' => egui::Key::Z,
            _ => panic!("未覆盖的字母键 {c:?}"),
        },
        UiKey::Char('=') | UiKey::Char('+') => egui::Key::Equals,
        UiKey::Char('-') => egui::Key::Minus,
        UiKey::Char('`') => egui::Key::Backtick,
        UiKey::Char('0') => egui::Key::Num0,
        UiKey::Char('1') => egui::Key::Num1,
        UiKey::Char('2') => egui::Key::Num2,
        UiKey::Char('3') => egui::Key::Num3,
        UiKey::Char('4') => egui::Key::Num4,
        UiKey::Char('5') => egui::Key::Num5,
        UiKey::Char('6') => egui::Key::Num6,
        UiKey::Char('7') => egui::Key::Num7,
        UiKey::Char('8') => egui::Key::Num8,
        UiKey::Char('9') => egui::Key::Num9,
        UiKey::Named(n) => match n {
            NamedKey::Escape => egui::Key::Escape,
            NamedKey::Enter => egui::Key::Enter,
            NamedKey::Tab => egui::Key::Tab,
            NamedKey::Backspace => egui::Key::Backspace,
            NamedKey::Delete => egui::Key::Delete,
            NamedKey::Space => egui::Key::Space,
            NamedKey::Up => egui::Key::ArrowUp,
            NamedKey::Down => egui::Key::ArrowDown,
            NamedKey::Left => egui::Key::ArrowLeft,
            NamedKey::Right => egui::Key::ArrowRight,
            NamedKey::Home => egui::Key::Home,
            NamedKey::End => egui::Key::End,
            other => panic!("未覆盖的命名键 {other:?}"),
        },
        other => panic!("未覆盖的键 {other:?}"),
    }
}

/// 表现层修饰键 → egui 修饰键（**必须置 `command`**）。
///
/// 关键：egui 的 `Modifiers::command` 才是分发器读的字段；
/// `ctrl` 在 macOS 之外反而是「物理 Ctrl」。手写映射漏置 `command`
/// 会得到 `NoMatch`——症状是「测试里按快捷键没反应」，
/// 极易被误判成「快捷键表坏了」。
pub fn egui_modifiers_of(m: Modifiers) -> egui::Modifiers {
    use rustmark::presentation::shortcuts::dispatcher::modifiers_from;
    let out = egui::Modifiers {
        command: m.ctrl,
        shift: m.shift,
        alt: m.alt,
        ..Default::default()
    };
    // 反向转换自洽性检查：若生产代码的映射规则变了（例如加入 macOS 专属字段），
    // 这里会立刻发现，而不是让整套用例以「键位失灵」的形式失败。
    debug_assert_eq!(
        modifiers_from(out),
        m,
        "egui 修饰键 → 领域的反向映射与生产代码不自洽"
    );
    out
}

/// 造一个「刚按下」的 `egui::Event::Key`。
///
/// 刻意造 `Event::Key` 而非直接调 `dispatch_combo`：前者才真正经过
/// `handle` 的 `pressed` / `repeat` 过滤，后者绕过了
/// 「长按不该重复触发命令」这条规则。
pub fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
