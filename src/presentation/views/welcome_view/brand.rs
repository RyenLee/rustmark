//! 品牌区与顶部光晕。

use egui::{RichText, Ui, Vec2};

use crate::presentation::theme::builtin::Theme;

use super::*;

/// 品牌标识位图（编译期内联的 128px PNG，4 KB）。
///
/// 与窗口图标、exe 资源段同源（`assets/logo/build_logo.py` 生成整套尺寸），
/// 因此应用内标识与任务栏图标永远同一张图，不会各自漂移。
const LOGO_PNG: &[u8] = include_bytes!("../../../../assets/logo/png/rustmark-icon-128.png");
/// Logo 显示边长（pt）。128px 源图在 2x 屏上正好一一对应。
const LOGO_EDGE: f32 = 64.0;
/// Logo 纹理的缓存键。
///
/// 欢迎页每帧都画：不缓存就会每帧新建一张纹理，帧率会一路下滑到显存耗尽。
const LOGO_TEXTURE_KEY: &str = "welcome_view::brand::logo";

/// 品牌区。
pub(super) fn brand(ui: &mut Ui, theme: &Theme) {
    // 有 Logo 就画：它是品牌区的主视觉，文字标题只作为无障碍/读屏的兜底。
    if let Some(texture) = logo_texture(ui) {
        ui.add(egui::Image::new(&texture).fit_to_exact_size(Vec2::splat(LOGO_EDGE)));
        ui.add_space(theme.metrics.space_md);
    }
    ui.label(
        RichText::new(LOGO)
            .size(30.0)
            .strong()
            .color(theme.text_primary),
    );
    ui.add_space(theme.metrics.space_md);
    ui.label(RichText::new(SLOGAN).size(13.0).color(theme.text_secondary));
    ui.add_space(theme.metrics.space_lg);
    // 版本号做成一个圆角小标签（原型 `.version`）。
    let galley = ui.painter().layout_no_wrap(
        format!("v{VERSION}"),
        egui::FontId::proportional(11.0),
        theme.text_secondary,
    );
    let pad = Vec2::new(16.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad, egui::Sense::hover());
    let box_rect = egui::Rect::from_center_size(rect.center(), galley.size() + pad);
    ui.painter().rect_stroke(
        box_rect,
        egui::CornerRadius::same(10),
        egui::Stroke::new(1.0_f32, theme.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        box_rect.left_top() + Vec2::new(8.0, 2.0),
        galley,
        theme.text_secondary,
    );
}

/// 取 Logo 纹理（首次解码，之后命中缓存）。
///
/// 解码走 `image` crate，与预览区的位图路径同源；纹理挂在 `ctx` 的临时数据里，
/// 因此重绘 / 换主题 / 缩窗口都不重复解码，且每个 `Context` 各自持有自己的纹理
/// （测试里一个用例一个 `Context`，共享静态缓存会拿到失效的 `TextureId`）。
fn logo_texture(ui: &Ui) -> Option<egui::TextureHandle> {
    let key = egui::Id::unique(LOGO_TEXTURE_KEY);
    if let Some(texture) = ui
        .ctx()
        .data_mut(|data| data.get_temp::<egui::TextureHandle>(key))
    {
        return Some(texture);
    }
    let rgba = image::load_from_memory(LOGO_PNG).ok()?.to_rgba8();
    let (width, height) = rgba.dimensions();
    let pixels =
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba.as_raw());
    let texture = ui
        .ctx()
        .load_texture(LOGO_TEXTURE_KEY, pixels, egui::TextureOptions::LINEAR);
    ui.ctx()
        .data_mut(|data| data.insert_temp(key, texture.clone()));
    Some(texture)
}

/// 顶部两团光晕。
pub(super) fn paint_glow(painter: &egui::Painter, rect: egui::Rect, theme: &Theme) {
    let center1 = egui::pos2(rect.center().x, rect.top() + rect.height() * 0.22);
    let center2 = egui::pos2(
        rect.left() + rect.width() * 0.12,
        rect.top() + rect.height() * 0.88,
    );
    for (center, size) in [
        (center1, Vec2::new(rect.width() * 0.6, rect.height() * 0.45)),
        (center2, Vec2::new(rect.width() * 0.4, rect.height() * 0.3)),
    ] {
        // 用一个实心椭圆 + 低透明度模拟径向渐变：egui 没有渐变 API，
        // 而画 20 个递减透明度的椭圆去逼近渐变，在深色主题下
        // 能看到明显的同心环。实心低透明是更诚实的近似。
        // `ellipse` 在 egui 0.33 里叫 `circle`，且**只**接受半径。
        // 要椭圆得用 `add(Shape::ellipse)`；这里取短半轴画正圆，
        // 视觉上仍是柔和光晕（原型是椭圆，差异只在轮廓形状）。
        painter.circle_filled(
            center,
            size.x.min(size.y) / 2.0,
            theme.accent_soft.gamma_multiply(0.3),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内联 Logo 必须能被 `image` 解码：解码失败时品牌区**静默不画图**
    /// （`logo_texture` 返回 `None`），这条用例把「素材在、运行时却解不出来」
    /// 挡在 CI 里，避免又退回「代码里有 logo、界面没有」的状态。
    #[test]
    fn inline_logo_decodes_to_a_square_bitmap() {
        let decoded = image::load_from_memory(LOGO_PNG);
        assert!(decoded.is_ok(), "内联的 Logo PNG 应能解码");
        if let Ok(bitmap) = decoded {
            assert_eq!((bitmap.width(), bitmap.height()), (128, 128));
        }
    }
}
