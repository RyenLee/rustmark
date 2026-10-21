//! 图片块：URL 尺寸参数解析、纹理缓存取用与失败占位（缓存本体在父模块）。
//!
//! # 为什么要解析尺寸参数（问题 3）
//!
//! 同一张原图配不同参数就是不同的展示效果，解析不到参数时所有图片只能
//! 按纹理实际尺寸渲染——参数变化反映不到视图上。参数有**两个来源**：
//!
//! 1. **源码语法**（主路径）：`![alt](url#pic_center =30x30)` 里的
//!    `#pic_*` 对齐标记与 ` =宽x高` 尺寸后缀（Typora / CSDN 私有扩展）。
//!    它们经解析层上行到 [`BlockView::image_width`] 等字段，
//!    表达的是用户对**这一处引用**的显示意图。
//! 2. **URL 查询参数**（兜底）：图床（阿里云 OSS / CSDN 的 `imgconvert`）
//!    用 `?x-oss-process=image/resize,w_50`（缩到 50px 宽）、
//!    `?x-oss-process=image/resize,m_fill,w_600,h_200`（裁剪填充）等
//!    描述服务端处理，同一原图配不同参数同样是不同效果。
//!
//! 加载失败时同样按参数生成不同大小的占位，让「参数区分」在降级路径上
//! 依然成立。

use egui::{Align, Layout};

use crate::application::dto::ImageSpecView;

use super::*;

/// 从图片 URL 解析出的**尺寸参数**（像素）。
///
/// 两个字段独立可选：图床参数可以只给宽（`w_50`）或只给高。
/// 均不到值时表示「URL 未声明尺寸，按纹理实际尺寸渲染」。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct ImageSizeParam {
    /// 声明的目标宽（像素）。
    pub width: Option<f32>,
    /// 声明的目标高（像素）。
    pub height: Option<f32>,
}

impl ImageSizeParam {
    /// 是否声明了任一尺寸。
    pub fn any(&self) -> bool {
        self.width.is_some() || self.height.is_some()
    }

    /// 按 `w×h` 或 `w` / `h` 展示（占位文案用）。
    pub fn describe(&self) -> String {
        match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{w}×{h}"),
            (Some(w), None) => format!("{w}px 宽"),
            (None, Some(h)) => format!("{h}px 高"),
            (None, None) => String::new(),
        }
    }
}

/// 解析图片 URL 的尺寸参数。
///
/// 支持两类形态：
///
/// 1. **阿里云 OSS 图片处理**（CSDN `imgconvert` 图床在用）：
///    `x-oss-process=image/resize,w_<W>[,h_<H>]`。`resize` 与 `w`/`h`
///    之间可能夹着模式参数（`m_fill` / `m_lfit` / `m_pad` 等），逐个
///    令牌扫描，只认 `w` / `h` 前缀的数字令牌。
/// 2. **通用查询参数**：`?w=<W>&h=<H>`（或只给一个）。
///
/// 解析不到返回 [`ImageSizeParam::default`]（全 `None`，非 `Option`）：
/// 「没写参数」与「写了认不出」对渲染是同一语义——按实际尺寸走。
pub(super) fn parse_image_size_params(url: &str) -> ImageSizeParam {
    let mut param = ImageSizeParam::default();
    // 形态 1：x-oss-process=image/resize,...
    if let Some(rest) = url.split("x-oss-process=").nth(1) {
        // 取到下一个 `&` 为止（oss 处理串内部用 `,` 分隔，不允许跨 `&`）。
        let ops = rest.split('&').next().unwrap_or("");
        for token in ops.split(',') {
            let token = token.trim();
            if let Some(v) = token.strip_prefix("w") {
                param.width = first_number(v);
            } else if let Some(v) = token.strip_prefix("h") {
                param.height = first_number(v);
            }
        }
        if param.any() {
            return param;
        }
    }
    // 形态 2：?w=..&h=..
    for (key, value) in query_pairs(url) {
        match key.as_str() {
            "w" => param.width = first_number(&value),
            "h" => param.height = first_number(&value),
            _ => {}
        }
    }
    param
}

/// 拆分 URL 查询串的 `key=value` 对（不含 `?`；值已做百分号解码的逆向——
/// 尺寸值是纯数字，无需解码，直接取数字前缀）。
fn query_pairs(url: &str) -> Vec<(String, String)> {
    let Some(query) = url.split('?').nth(1) else {
        return Vec::new();
    };
    query
        .split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// 取字符串中的**第一段数字**为 `f32`（`50` / `_50` / `600x200` / `50.5` 均可；
/// 纯非数字返回 `None`）。
///
/// 前导非数字字符先跳过：OSS 参数写作 `w_50`（下划线分隔），
/// 通用查询参数写作 `w=50`（无分隔），两种都要认。
fn first_number(s: &str) -> Option<f32> {
    let digits: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit() && *c != '.')
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse::<f32>().ok().filter(|v| *v > 0.0)
}

/// 图片：三种来源统一经 `AssetLoader`；失败显示占位而非崩溃。
///
/// 渲染尺寸按「源码语法参数（`=30x30`）→ URL 查询参数（OSS 处理串）→
/// 纹理实际尺寸 → 面板宽度上限」的优先级计算（问题 3）；对齐按
/// `#pic_center` / `#pic_left` / `#pic_right` 选择，未写标记时左对齐。
pub(super) fn draw_image(app: &mut EditorApp, ui: &mut Ui, theme: &Theme, block: &BlockView) {
    // 图片块只是「一张图 + 块级字段」的组合，真正的绘制在
    // [`draw_image_spec`] 里——混排在文字里的图片复用同一条路径
    // （问题 032：否则行内图片与图片块的渲染效果会分叉）。
    draw_image_spec(
        app,
        ui,
        theme,
        &ImageSpecView {
            url: block.url.clone().unwrap_or_default(),
            alt: block.alt.clone().unwrap_or_default(),
            width: block.image_width,
            height: block.image_height,
            align: block
                .image_align
                .clone()
                .unwrap_or_else(|| "left".to_string()),
        },
    );
}

/// 按一张图的参数绘制图片（图片块与行内图片共用）。
///
/// 参数来源有两个：源码私有扩展（`=30x30` / `#pic_center`）与 URL 查询参数
/// （OSS 处理串），前者优先（见 [`effective_size_param`]）。
pub(super) fn draw_image_spec(
    app: &mut EditorApp,
    ui: &mut Ui,
    theme: &Theme,
    spec: &ImageSpecView,
) {
    let url = spec.url.clone();
    let ImageSpecView {
        alt,
        width,
        height,
        align,
        ..
    } = spec;
    let alt = alt.clone();
    if url.is_empty() {
        ui.weak(format!("图片地址为空：{alt}"));
        return;
    }
    // 图片按「预览区可用宽度」与上限 `IMAGE_MAX_W` 的较小值缩放：窄面板里图片
    // 缩到区域宽（不再溢出被裁），宽面板里也不超过上限（问题 2：预览未适配区域宽度）。
    let max_w = ui.available_width().min(IMAGE_MAX_W);
    let entry = image_entry(ui, app, &url);
    let param = effective_size_param(*width, *height, &url);
    let align = image_align_of(Some(align.as_str()));
    // 整块按 `#pic_*` 标记对齐；未写标记时左对齐——与 CSDN 预览一致
    // （效果截图里无标记图片贴在左缘），写 `#pic_center` 才居中。
    ui.with_layout(Layout::top_down(align), |ui| {
        match entry.texture {
            Some(texture) => {
                let [w, h] = entry.size;
                let (dw, dh) = fitted_size(w, h, param, max_w);
                ui.add(
                    egui::Image::new(egui::load::SizedTexture::from_handle(&texture))
                        .fit_to_exact_size(egui::vec2(dw, dh)),
                );
            }
            None if entry.loading => {
                // 远程图片后台下载中（F-3）：占位高度与失败占位**一致**
                // （滚动同步的块矩形不随加载状态跳变），只换文案。
                let param_w = param
                    .width
                    .filter(|w| *w > 0.0)
                    .map_or(max_w, |w| w.min(max_w));
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(param_w, IMAGE_PLACEHOLDER_H),
                    egui::Sense::hover(),
                );
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(4), theme.bg_tertiary);
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(rect.shrink(theme.metrics.space_md)),
                    |ui| {
                        ui.weak(format!("图片加载中：{alt}"));
                    },
                );
            }
            None => {
                // 占位高度**固定**（见 [`IMAGE_PLACEHOLDER_H`]）：坏图与未就绪的图
                // 都占同样的版面，滚动同步的块矩形才不会随加载结果跳变。
                // 宽度仍按尺寸参数区分（问题 3）：`=30x30` 的窄条与
                // `=600x200` 的宽条在加载失败时依然可辨。
                let param_w = param
                    .width
                    .filter(|w| *w > 0.0)
                    .map_or(max_w, |w| w.min(max_w));
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(param_w, IMAGE_PLACEHOLDER_H),
                    egui::Sense::hover(),
                );
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(4), theme.bg_tertiary);
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
                        // 解析到尺寸参数时显式告知：用户改了 `=30x30`
                        // 却看不出区别时，至少能确认参数被读到了（问题 3）。
                        if param.any() {
                            ui.weak(format!("请求尺寸：{}", param.describe()));
                        }
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
                            // 同 inline.rs 的行内链接：走 opener 端口而非
                            // 直调 shared 的自由函数（R-03 把进程调用移出 shared）。
                            let _ = app.services.opener.open(&url);
                        }
                    },
                );
            }
        }
    });
}

/// 生效的尺寸参数：源码语法（`=宽x高`）优先，URL 查询参数兜底。
///
/// 语法参数是用户对**这一处引用**的显示意图，URL 参数描述服务端处理，
/// 前者更具体。任一维度声明了语法值就**整体**以语法为准（不混用——
/// 混用会产出谁都不认识的尺寸）；两者都没有时按纹理实际尺寸渲染。
fn effective_size_param(
    declared_w: Option<u32>,
    declared_h: Option<u32>,
    url: &str,
) -> ImageSizeParam {
    let syntax = ImageSizeParam {
        width: declared_w.map(|w| w as f32),
        height: declared_h.map(|h| h as f32),
    };
    if syntax.any() {
        syntax
    } else {
        parse_image_size_params(url)
    }
}

/// DTO 对齐字符串 → egui 布局对齐。
///
/// 无法识别的值（含 `None`，即未写标记）一律**左对齐**：CSDN / Typora 的
/// 无标记图片就是左对齐（效果截图），`#pic_*` 标记的存在意义正是把默认
/// 的左对齐改掉——要居中必须显式写 `#pic_center`。
fn image_align_of(align: Option<&str>) -> Align {
    match align {
        Some("center") => Align::Center,
        Some("right") => Align::RIGHT,
        _ => Align::LEFT,
    }
}

/// 计算图片的**显示尺寸**（问题 3）。
///
/// 优先级：URL 尺寸参数先收紧（不放大，只允许缩小），再受面板宽度
/// `max_w` 限制。宽高都给时按 contain（整体缩进参数框）；只给一个时
/// 按纹理宽高比换算另一个。无参数时退化为「等比缩放进面板」。
fn fitted_size(tex_w: f32, tex_h: f32, param: ImageSizeParam, max_w: f32) -> (f32, f32) {
    let (mut dw, mut dh) = (tex_w.max(1.0), tex_h.max(1.0));
    match (param.width, param.height) {
        (Some(pw), Some(ph)) => {
            // contain：取更小的缩放比，整体落进参数框。
            let s = (pw / dw).min(ph / dh).min(1.0);
            dw *= s;
            dh *= s;
        }
        (Some(pw), None) => {
            let s = (pw / dw).min(1.0);
            dw *= s;
            dh *= s;
        }
        (None, Some(ph)) => {
            let s = (ph / dh).min(1.0);
            dw *= s;
            dh *= s;
        }
        (None, None) => {}
    }
    // 面板宽度上限：参数再大也不越出面板（问题 2 的宽度适配依然生效）。
    let s = (max_w / dw).min(1.0);
    (dw * s, dh * s)
}

#[cfg(test)]
mod tests {
    use super::{
        ImageSizeParam, effective_size_param, first_number, fitted_size, image_align_of,
        parse_image_size_params,
    };
    use egui::Align;

    /// CSDN / 阿里云 OSS 的 `x-oss-process=image/resize,w_50`：只给宽。
    #[test]
    fn parses_oss_resize_width_only() {
        let p = parse_image_size_params(
            "https://imgconvert.csdnimg.cn/aHR0cHM?x-oss-process=image/resize,w_50",
        );
        assert_eq!(p.width, Some(50.0));
        assert_eq!(p.height, None);
        assert!(p.any());
    }

    /// `m_fill,w_600,h_200`：模式参数夹在中间，宽高都要认出来。
    #[test]
    fn parses_oss_resize_with_mode_and_both_sides() {
        let p = parse_image_size_params(
            "https://imgconvert.csdnimg.cn/aHR0cHM?x-oss-process=image/resize,m_fill,w_600,h_200",
        );
        assert_eq!(p.width, Some(600.0));
        assert_eq!(p.height, Some(200.0));
    }

    /// 通用 `?w=&h=` 形态。
    #[test]
    fn parses_plain_query_params() {
        let p = parse_image_size_params("https://example.com/a.png?w=320&h=240");
        assert_eq!(p.width, Some(320.0));
        assert_eq!(p.height, Some(240.0));
        // 只给一个也认。
        let h_only = parse_image_size_params("https://example.com/a.png?h=99");
        assert_eq!(h_only.width, None);
        assert_eq!(h_only.height, Some(99.0));
    }

    /// 无参数（本地路径 / 干净 URL）→ 全 None，按实际尺寸渲染。
    #[test]
    fn no_params_yields_default() {
        assert_eq!(
            parse_image_size_params("images/local.png"),
            ImageSizeParam::default()
        );
        assert_eq!(
            parse_image_size_params("https://example.com/a.png"),
            ImageSizeParam::default()
        );
        // 查询串里没有 w/h 不算参数。
        assert_eq!(
            parse_image_size_params("https://example.com/a.png?token=abc&v=2"),
            ImageSizeParam::default()
        );
    }

    /// 参数只收紧不放大：`w_50` 对 600px 宽的图生效，对 40px 宽的图不放大。
    #[test]
    fn params_shrink_but_never_enlarge() {
        // 600×400 的图声明 w_50 → 50×33.33。
        let (w, h) = fitted_size(
            600.0,
            400.0,
            ImageSizeParam {
                width: Some(50.0),
                height: None,
            },
            620.0,
        );
        assert!((w - 50.0).abs() < 0.01);
        assert!((h - 400.0 * 50.0 / 600.0).abs() < 0.01);
        // 40×30 的图声明 w_50 → 保持 40×30（不放大）。
        let (w2, h2) = fitted_size(
            40.0,
            30.0,
            ImageSizeParam {
                width: Some(50.0),
                height: None,
            },
            620.0,
        );
        assert!((w2 - 40.0).abs() < 0.01);
        assert!((h2 - 30.0).abs() < 0.01);
    }

    /// 宽高都给时按 contain 落进参数框。
    #[test]
    fn both_sides_use_contain() {
        // 600×400 的图声明 200×200 → 200×133.33（宽先到限）。
        let (w, h) = fitted_size(
            600.0,
            400.0,
            ImageSizeParam {
                width: Some(200.0),
                height: Some(200.0),
            },
            620.0,
        );
        assert!((w - 200.0).abs() < 0.01);
        assert!((h - 400.0 * 200.0 / 600.0).abs() < 0.01);
    }

    /// 面板上限优先于参数：`w_600` 在 300px 宽的面板里收到 300。
    #[test]
    fn panel_width_caps_everything() {
        let (w, _h) = fitted_size(
            600.0,
            400.0,
            ImageSizeParam {
                width: Some(600.0),
                height: Some(400.0),
            },
            300.0,
        );
        assert!((w - 300.0).abs() < 0.01);
        // 无参数时同样受面板限制（问题 2 的适配不回退）。
        let (w2, _) = fitted_size(900.0, 100.0, ImageSizeParam::default(), 300.0);
        assert!((w2 - 300.0).abs() < 0.01);
    }

    /// 前导数字提取：小数、垃圾后缀、纯非数字。
    #[test]
    fn first_number_takes_leading_digits() {
        assert_eq!(first_number("50"), Some(50.0));
        assert_eq!(first_number("50.5"), Some(50.5));
        assert_eq!(first_number("600x200"), Some(600.0));
        assert_eq!(first_number("abc"), None);
        assert_eq!(first_number(""), None);
    }

    /// 语法参数优先：`=30x30` 与 URL 查询参数同时存在时以语法为准
    /// （语法是这一处引用的显示意图，URL 参数只描述服务端处理）。
    #[test]
    fn syntax_params_win_over_url_params() {
        let p = effective_size_param(
            Some(30),
            Some(30),
            "https://imgconvert.csdnimg.cn/x?x-oss-process=image/resize,w_600",
        );
        assert_eq!(p.width, Some(30.0));
        assert_eq!(p.height, Some(30.0));
    }

    /// 无语法参数时回落到 URL 查询参数（OSS 处理串兜底）。
    #[test]
    fn url_params_used_when_no_syntax_params() {
        let p = effective_size_param(
            None,
            None,
            "https://imgconvert.csdnimg.cn/x?x-oss-process=image/resize,w_50",
        );
        assert_eq!(p.width, Some(50.0));
        assert_eq!(p.height, None);
    }

    /// 半组语法参数（只给宽）也整体优先，不与 URL 参数混用。
    #[test]
    fn partial_syntax_params_still_win() {
        let p = effective_size_param(Some(30), None, "https://x.test/a.png?w=600&h=200");
        assert_eq!(p.width, Some(30.0));
        assert_eq!(p.height, None);
    }

    /// 两者皆无 → 默认（按纹理实际尺寸渲染）。
    #[test]
    fn no_params_anywhere_yields_default() {
        assert_eq!(
            effective_size_param(None, None, "images/local.png"),
            ImageSizeParam::default()
        );
    }

    /// 对齐字符串 → 布局对齐；无法识别的值（含 `None`）一律左对齐
    /// （与「未写标记」同义，效果截图里无标记图片贴在左缘）。
    #[test]
    fn align_of_maps_known_values_and_defaults_left() {
        assert_eq!(image_align_of(Some("center")), Align::Center);
        assert_eq!(image_align_of(Some("left")), Align::LEFT);
        assert_eq!(image_align_of(Some("right")), Align::RIGHT);
        assert_eq!(image_align_of(None), Align::LEFT);
        assert_eq!(image_align_of(Some("garbage")), Align::LEFT);
    }
}
