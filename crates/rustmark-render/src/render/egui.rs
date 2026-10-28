//! 唯一允许 `use egui` 的子模块：把 [`RenderCommand`] 翻译成 `Painter` 调用。
//!
//! 本文件之外的 `rich` 代码一律不碰 egui 的绘制 API（见 `mod` 模块文档）。

use egui::text::LayoutJob;
use egui::{
    CornerRadius, FontId, Painter, Pos2, Rect as EguiRect, Shape, Stroke, StrokeKind, TextFormat,
    Vec2,
};

use super::{HAlign, Pt, RenderBackend, RenderCommand, Rgb8, TextAnchor};

impl Rgb8 {
    /// 转 egui `Color32`（非预乘，与主题色同源）。放在本白名单文件，
    /// 保持 `mod` / `geom` 零 `egui` 引用。
    pub fn to_egui(self) -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(self.r, self.g, self.b, self.a)
    }
}

/// egui `Color32` → `Rgb8`（非预乘，与主题色同源）。放在本白名单文件，
/// 保持 `mod` / `geom` 零 `egui` 引用；消费方（如视图层）把主题色转成
/// `Rgb8` 后再交给端口层，使排版引擎不碰 egui。
pub fn rgb8_from_color32(c: egui::Color32) -> Rgb8 {
    Rgb8::new(c.r(), c.g(), c.b(), c.a())
}

/// egui 后端：持有 painter 与「原点 + 缩放」变换，逐条翻译命令。
///
/// 变换复刻旧 `math::paint` / `mermaid::paint` 的等比缩放 + 原点公式：
/// `abs = origin + coord * scale`。内容局部坐标经此线性映射到 `rect`。
pub struct EguiBackend<'a> {
    painter: &'a Painter,
    scale: f32,
    origin: Pt,
}

impl<'a> EguiBackend<'a> {
    /// 构造后端（持有 painter 与「原点 + 缩放」变换）。
    pub fn new(painter: &'a Painter, scale: f32, origin: Pt) -> Self {
        EguiBackend {
            painter,
            scale,
            origin,
        }
    }

    fn map(&self, p: Pt) -> Pos2 {
        let q = self.origin + p.scale(self.scale);
        Pos2::new(q.x, q.y)
    }
}

impl<'a> RenderBackend for EguiBackend<'a> {
    fn draw(&mut self, cmd: &RenderCommand) {
        match cmd {
            RenderCommand::Text {
                at,
                text,
                size,
                italic,
                anchor,
                plate,
                plate_bg,
                color,
            } => {
                if text.is_empty() {
                    return;
                }
                let at = self.map(*at);
                let c = color.to_egui();
                let mut job = LayoutJob::default();
                job.append(
                    text,
                    0.0,
                    TextFormat {
                        font_id: FontId::proportional(*size * self.scale),
                        color: c,
                        italics: *italic,
                        ..Default::default()
                    },
                );
                let galley = self.painter.layout_job(job);
                let pos = match anchor {
                    // 基线左端：先量一次拿到真实基线偏移（行顶 + 首字形基线位置），
                    // 再上移，避免每个公式整体上浮约 0.2em。
                    TextAnchor::BaselineLeft => at - Vec2::new(0.0, baseline_offset(&galley)),
                    // 中心：直接以锚点为中心放。
                    TextAnchor::Center => at - galley.size() * 0.5,
                };
                if *plate {
                    self.painter.rect_filled(
                        EguiRect::from_center_size(
                            at,
                            galley.size() + Vec2::splat(4.0 * self.scale),
                        ),
                        CornerRadius::same(2),
                        plate_bg.to_egui(),
                    );
                }
                self.painter.galley(pos, galley, c);
            }
            RenderCommand::Line {
                a,
                b,
                width,
                dotted,
                color,
            } => {
                let (a, b) = (self.map(*a), self.map(*b));
                let stroke = Stroke::new(*width * self.scale, color.to_egui());
                if *dotted {
                    self.painter.extend(Shape::dashed_line(
                        &[a, b],
                        stroke,
                        4.0 * self.scale,
                        3.0 * self.scale,
                    ));
                } else {
                    self.painter.line_segment([a, b], stroke);
                }
            }
            RenderCommand::Rect {
                pos,
                size,
                radius,
                color,
                fill,
            } => {
                let pos = self.map(*pos);
                let size = size.scale(self.scale);
                if *fill {
                    self.painter.rect_filled(
                        EguiRect::from_min_size(pos, Vec2::new(size.x, size.y)),
                        CornerRadius::same(*radius as u8),
                        color.to_egui(),
                    );
                } else {
                    self.painter.rect_stroke(
                        EguiRect::from_min_size(pos, Vec2::new(size.x, size.y)),
                        CornerRadius::same(*radius as u8),
                        Stroke::new(1.0 * self.scale, color.to_egui()),
                        StrokeKind::Inside,
                    );
                }
            }
            RenderCommand::Poly {
                points,
                radius,
                fill,
                ticks,
                fill_color,
                stroke_color,
            } => {
                let pts: Vec<Pos2> = points.iter().map(|p| self.map(*p)).collect();
                let fill_c = fill_color.to_egui();
                let stroke_c = stroke_color.to_egui();
                let stroke = Stroke::new(1.2 * self.scale, stroke_c);
                if *fill {
                    let r = EguiRect::from_points(&pts);
                    if pts.len() == 4 && *radius > 0.0 {
                        self.painter.rect_filled(
                            r,
                            CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8),
                            fill_c,
                        );
                    } else {
                        self.painter
                            .add(Shape::convex_polygon(pts.clone(), fill_c, Stroke::NONE));
                    }
                }
                // 描边：圆角矩形用 `rect_stroke`，其余多边形闭合折线。
                let r = EguiRect::from_points(&pts);
                if pts.len() == 4 && *radius > 0.0 {
                    self.painter.rect_stroke(
                        r,
                        CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8),
                        stroke,
                        StrokeKind::Inside,
                    );
                } else {
                    let mut closed = pts.clone();
                    if let Some(first) = pts.first() {
                        closed.push(*first);
                    }
                    self.painter.add(Shape::line(closed, stroke));
                }
                if *ticks {
                    // 子程序形：左右各一道内竖线。
                    for t in [0.25_f32, 0.75] {
                        let x = r.left() + r.width() * t;
                        self.painter.line_segment(
                            [Pos2::new(x, r.top()), Pos2::new(x, r.bottom())],
                            Stroke::new(1.0 * self.scale, stroke_c),
                        );
                    }
                }
            }
            RenderCommand::Arrow {
                tip,
                dir,
                len,
                spread,
                color,
            } => {
                let tip = self.map(*tip);
                let len = *len * self.scale;
                // 等比缩放不改变方向，故 `dir` 直接用原值（内容局部、未缩放）；
                // 方向偏移在绝对坐标里叠加（与旧 paint 一致）。
                let dir = Vec2::new(dir.x, dir.y);
                let side = Vec2::new(-dir.y, dir.x);
                let back = tip - dir * len;
                self.painter.add(Shape::convex_polygon(
                    vec![
                        tip,
                        back + side * len * *spread,
                        back - side * len * *spread,
                    ],
                    color.to_egui(),
                    Stroke::NONE,
                ));
            }
            RenderCommand::Cross {
                at,
                dir,
                len,
                color,
            } => {
                let at = self.map(*at);
                let len = *len * self.scale;
                let dir = Vec2::new(dir.x, dir.y);
                let side = Vec2::new(-dir.y, dir.x);
                for s in [1.0_f32, -1.0] {
                    let off = (dir + side * s) * len * 0.9;
                    self.painter.line_segment(
                        [at, at + off],
                        Stroke::new(1.4 * self.scale, color.to_egui()),
                    );
                }
            }
        }
    }
}

/// 量一次拿到真实基线偏移（行顶 + 首字形基线位置）。复刻旧 `math::paint` 的算法。
fn baseline_offset(galley: &egui::Galley) -> f32 {
    galley
        .rows
        .first()
        .map(|row| row.pos.y + row.row.glyphs.first().map_or(0.0, |glyph| glyph.pos.y))
        .unwrap_or(0.0)
}

/// 等比缩放 + 原点公式（复刻旧 `math::paint` / `mermaid::paint`），逐条绘制命令。
///
/// - `h_align`：水平落点（数学块公式居中、行内左对齐；图表恒居中）；
/// - `v_center`：是否垂直居中（数学 `true`、图表 `false`）；
/// - `nw` / `nh`：内容自然宽高（点），即 `MathLayout` / `MermaidLayout` 的
///   `width` / `height`，用来算缩放与留白。
pub fn apply_egui(
    painter: &Painter,
    rect: EguiRect,
    nw: f32,
    nh: f32,
    h_align: HAlign,
    v_center: bool,
    cmds: &[RenderCommand],
) {
    let scale = if nw > 0.0 && rect.width() < nw {
        rect.width() / nw
    } else {
        1.0
    };
    let used_w = nw * scale;
    let used_h = nh * scale;
    let h_off = match h_align {
        HAlign::Left => 0.0,
        HAlign::Center => (rect.width() - used_w).max(0.0) * 0.5,
        HAlign::Right => rect.width() - used_w,
    };
    let v_off = if v_center {
        (rect.height() - used_h).max(0.0) * 0.5
    } else {
        0.0
    };
    let origin = Pt::new(rect.left() + h_off, rect.top() + v_off);
    let mut be = EguiBackend::new(painter, scale, origin);
    for c in cmds {
        be.draw(c);
    }
}
