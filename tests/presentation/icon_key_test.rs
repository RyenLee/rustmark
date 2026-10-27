//! R-05 收口（关千锤验收盲区）：钉死 `UiAction → IconKey` 的**恒等**映射，
//! 不只在「每个平铺按钮解析到非空图标」层面（见 `toolbar_visual_test.rs`）。
//!
//! 那条规格只锚定「动作能解析出图标」，但五枚键若「串台」互换
//! （Bold↔Italic 之类）它抓不到——本文件逐条钉死**具体**对应。

use rustmark::presentation::icon_key_for;
use rustmark::presentation::icons::IconKey;
use rustmark::presentation::shortcuts::registry::UiAction;

#[test]
fn icon_key_for_bold_is_bold() {
    assert_eq!(icon_key_for(UiAction::Bold), Some(IconKey::Bold));
}

#[test]
fn icon_key_for_italic_is_italic() {
    assert_eq!(icon_key_for(UiAction::Italic), Some(IconKey::Italic));
}

#[test]
fn icon_key_for_strikethrough_is_strikethrough() {
    assert_eq!(
        icon_key_for(UiAction::Strikethrough),
        Some(IconKey::Strikethrough)
    );
}

#[test]
fn icon_key_for_inline_code_is_inline_code() {
    assert_eq!(
        icon_key_for(UiAction::InlineCode),
        Some(IconKey::InlineCode)
    );
}

#[test]
fn icon_key_for_insert_link_is_insert_link() {
    assert_eq!(
        icon_key_for(UiAction::InsertLink),
        Some(IconKey::InsertLink)
    );
}
