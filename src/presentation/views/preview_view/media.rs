//! 图片块：纹理缓存取用与失败占位（缓存本体在父模块）。

use crate::shared::system;

use super::*;

/// 图片：三种来源统一经 `AssetLoader`；失败显示占位而非崩溃。
pub(super) fn draw_image(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, block: &BlockView) {
    let url = block.url.clone().unwrap_or_default();
    let alt = block.alt.clone().unwrap_or_default();
    if url.is_empty() {
        ui.weak(format!("图片地址为空：{alt}"));
        return;
    }
    // 图片按「预览区可用宽度」与上限 `IMAGE_MAX_W` 的较小值缩放：窄面板里图片
    // 缩到区域宽（不再溢出被裁），宽面板里也不超过上限（问题 2：预览未适配区域宽度）。
    let max_w = ui.available_width().min(IMAGE_MAX_W);
    let entry = image_entry(ui, app, &url);
    match entry.texture {
        Some(texture) => {
            let [w, h] = entry.size;
            // 等比缩放到面板宽度内。
            let scale = (max_w / w.max(1.0)).min(1.0);
            ui.add(
                egui::Image::new(egui::load::SizedTexture::from_handle(&texture))
                    .fit_to_exact_size(egui::vec2(w * scale, h * scale)),
            );
        }
        None => {
            // 占位高度**固定**（见 [`IMAGE_PLACEHOLDER_H`]）：坏图与未就绪的图
            // 都占同样的版面，滚动同步的块矩形才不会随加载结果跳变。
            let (rect, _) = ui
                .allocate_exact_size(egui::vec2(max_w, IMAGE_PLACEHOLDER_H), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(4), theme.bg_tertiary);
            let reason = entry.error.unwrap_or_default();
            // 文案画在占位框内（`allocate_ui_at_rect` 不推进光标，故不改变高度）。
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(rect.shrink(theme.metrics.space_md)),
                |ui| {
                    ui.label(
                        RichText::new(format!(
                            "图片无法加载：{}\n{}",
                            if alt.is_empty() { &url } else { &alt },
                            reason
                        ))
                        .size(theme.metrics.ui_font_size)
                        .color(theme.warning),
                    );
                    // 远程地址（http/https）下载失败（网络受限 / 超时 / 原图
                    // 已删除）时，仍提供「在浏览器中打开原图」的出口，而不是
                    // 只留一句报错——用户文档里的远程图片常见于此。
                    if (url.starts_with("http://") || url.starts_with("https://"))
                        && ui
                            .add(
                                egui::Button::new(
                                    RichText::new("在浏览器中打开 ↗")
                                        .size(theme.metrics.ui_font_size - 1.0),
                                )
                                .small()
                                .fill(theme.bg_primary)
                                .stroke(egui::Stroke::new(1.0_f32, theme.border)),
                            )
                            .clicked()
                    {
                        let _ = system::open_external(&url);
                    }
                },
            );
        }
    }
}
