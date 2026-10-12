//! 主题设置的运行时解析与「配色变更签名」。

use crate::application::dto::ThemeView;

use super::*;

/// 解析主题设置 → 具体主题。
///
/// `system_preference` 由调用方从系统读取（`true` = 系统是深色）。
/// 抽成参数而非在内部读系统，是为了让本函数**可测**——
/// CI 上没有「跟随系统」的真实环境。
pub fn resolve(view: &ThemeView, system_preference_dark: bool) -> Theme {
    // 自定义主题：以调色板为准。调色板缺失（旧配置 / 异常）时退回默认调色板，
    // 保证首次切到 custom（尚无任何配色改动）不崩、也不丢配色。
    if view.kind.trim().eq_ignore_ascii_case("custom") {
        let default_palette = CustomThemePaletteView::default();
        let palette = view.custom.as_ref().unwrap_or(&default_palette);
        return Theme::from_custom_palette(
            palette,
            &view.font_family,
            view.font_size,
            view.line_height,
        );
    }
    let kind = BuiltinTheme::parse(&view.kind);
    let base = match kind {
        BuiltinTheme::Light => BuiltinTheme::Light,
        BuiltinTheme::Dark => BuiltinTheme::Dark,
        BuiltinTheme::System => {
            if system_preference_dark {
                BuiltinTheme::Dark
            } else {
                BuiltinTheme::Light
            }
        }
    };
    let mut theme =
        Theme::with_font_settings(base, &view.font_family, view.font_size, view.line_height);
    // 解析后 `kind` 必须反映**实际生效**的那套，否则 `is_dark()` 会误报。
    theme.kind = base;
    theme
}

/// 主题变更签名：内容不变时返回 `None`，让预览缓存免于重建。
///
/// 验收标准要求「主题变更触发预览缓存失效（断言 Galley 重建）」，
/// 但**无关字段**（如字体）变化不该让预览重排——那是纯浪费。
/// 用签名把「配色变了」与「只是字号变了」区分开。
pub fn palette_signature(theme: &Theme) -> Option<[u8; 4]> {
    let mut hasher: u32 = 2166136261;
    for color in [
        theme.bg_primary,
        theme.bg_secondary,
        theme.bg_tertiary,
        theme.border,
        theme.text_primary,
        theme.text_secondary,
        theme.accent,
        theme.code_bg,
        theme.selection_bg,
        // 全部语法高亮色都纳入（V11-09）：任一语法色变化都必须让预览缓存失效，
        // 否则用户改了「关键字色」却发现预览区没变——那是最难解释的一类 bug。
        theme.syn_heading,
        theme.syn_bold,
        theme.syn_italic,
        theme.syn_code,
        theme.syn_link,
        theme.syn_quote,
        theme.syn_marker,
        theme.syn_meta,
        theme.syn_table,
        theme.syn_other,
    ] {
        let [r, g, b, a] = color.to_array();
        for byte in [r, g, b, a] {
            hasher ^= u32::from(byte);
            hasher = hasher.wrapping_mul(16777619);
        }
    }
    // `u32 → 4 字节` 折叠：签名只需「变了 / 没变」，不需密码学强度。
    let folded = hasher.to_le_bytes();
    Some([folded[0], folded[1], folded[2], folded[3]])
}
