//! egui 绘制、finalize 收尾、坐标工具与热点解析。

use egui::text::LayoutJob;
use egui::{
    Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, TextFormat, Vec2,
};

use super::super::model::{MermaidLayout, Prim};

/// 图表配色（由调用方从主题取；本模块不依赖 `Theme`）。
#[derive(Clone, Copy, Debug)]
pub struct MermaidStyle {
    /// 节点填充色。
    pub node_fill: Color32,
    /// 节点描边色。
    pub node_stroke: Color32,
    /// 节点文字色。
    pub text: Color32,
    /// 连线色。
    pub edge: Color32,
    /// 边标签底色。
    pub label_bg: Color32,
}

/// 一个图元的外接框。
fn prim_bounds(p: &Prim) -> Rect {
    match p {
        Prim::Poly { points, .. } => Rect::from_points(points),
        Prim::Line { a, b, width, .. } => Rect::from_two_pos(*a, *b).expand(*width * 0.5),
        Prim::Arrow {
            tip,
            dir,
            len,
            spread,
        } => Rect::from_two_pos(*tip, *tip - *dir * *len).expand(*len * *spread),
        Prim::Cross { at, len, .. } => Rect::from_center_size(*at, Vec2::splat(*len * 2.0)),
        Prim::Text { at, text, size, .. } => {
            // 用「字符数 × 半字号」估外接框：这里只需要**足够大**。
            // 真实宽度由绘制阶段的字体度量决定，估算偏差只让版面多一点留白。
            let w = text.chars().count() as f32 * *size * 0.55;
            Rect::from_center_size(*at, Vec2::new(w.max(*size), *size * 1.5))
        }
    }
}

fn shift_prim(p: Prim, d: Vec2) -> Prim {
    match p {
        Prim::Poly {
            points,
            radius,
            fill,
            ticks,
            fill_color,
            stroke_color,
        } => Prim::Poly {
            points: points.into_iter().map(|q| q + d).collect(),
            radius,
            fill,
            ticks,
            fill_color,
            stroke_color,
        },
        Prim::Line {
            a,
            b,
            width,
            dotted,
        } => Prim::Line {
            a: a + d,
            b: b + d,
            width,
            dotted,
        },
        Prim::Arrow {
            tip,
            dir,
            len,
            spread,
        } => Prim::Arrow {
            tip: tip + d,
            dir,
            len,
            spread,
        },
        Prim::Cross { at, dir, len } => Prim::Cross {
            at: at + d,
            dir,
            len,
        },
        Prim::Text {
            at,
            text,
            size,
            plate,
            color,
        } => Prim::Text {
            at: at + d,
            text,
            size,
            plate,
            color,
        },
    }
}

/// 把版面按可用宽度缩放到 `rect` 里，返回实际占用高度。
///
/// 宽度不足时**整体等比缩小**（含字号与线宽）——宁可整张图小一点，
/// 也不要右侧被裁掉：图的信息全在结构上，被裁掉的结构等于没画。
pub fn paint(painter: &Painter, rect: Rect, layout: &MermaidLayout, style: &MermaidStyle) -> f32 {
    let scale = if layout.width > rect.width() && layout.width > 0.0 {
        rect.width() / layout.width
    } else {
        1.0
    };
    let origin = Pos2::new(
        rect.left() + (rect.width() - layout.width * scale).max(0.0) * 0.5,
        rect.top(),
    );
    let map = |p: Pos2| origin + p.to_vec2() * scale;

    for prim in &layout.prims {
        match prim {
            Prim::Poly {
                points,
                radius,
                fill,
                ticks,
                fill_color,
                stroke_color,
            } => {
                let pts: Vec<Pos2> = points.iter().map(|p| map(*p)).collect();
                let fill_c = fill_color.unwrap_or(style.node_fill);
                let stroke_c = stroke_color.unwrap_or(style.node_stroke);
                let stroke = Stroke::new(1.2 * scale, stroke_c);
                if *fill {
                    let r = Rect::from_points(&pts);
                    if pts.len() == 4 && *radius > 0.0 {
                        painter.rect_filled(
                            r,
                            CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8),
                            fill_c,
                        );
                    } else {
                        painter.add(Shape::convex_polygon(pts.clone(), fill_c, Stroke::NONE));
                    }
                }
                // 描边：圆角矩形用 `rect_stroke`，其余多边形闭合折线。
                let r = Rect::from_points(&pts);
                if pts.len() == 4 && *radius > 0.0 {
                    painter.rect_stroke(
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
                    painter.add(Shape::line(closed, stroke));
                }
                if *ticks {
                    // 子程序形：左右各一道内竖线。
                    for t in [0.25_f32, 0.75] {
                        let x = r.left() + r.width() * t;
                        painter.line_segment(
                            [Pos2::new(x, r.top()), Pos2::new(x, r.bottom())],
                            Stroke::new(1.0 * scale, stroke_c),
                        );
                    }
                }
            }
            Prim::Line {
                a,
                b,
                width,
                dotted,
            } => {
                let (a, b) = (map(*a), map(*b));
                let stroke = Stroke::new(width * scale, style.edge);
                if *dotted {
                    painter.extend(Shape::dashed_line(
                        &[a, b],
                        stroke,
                        4.0 * scale,
                        3.0 * scale,
                    ));
                } else {
                    painter.line_segment([a, b], stroke);
                }
            }
            Prim::Arrow {
                tip,
                dir,
                len,
                spread,
            } => {
                let tip = map(*tip);
                let len = len * scale;
                // 等比缩放不改变方向，故 `dir` 直接用原值。
                let side = Vec2::new(-dir.y, dir.x);
                let back = tip - *dir * len;
                painter.add(Shape::convex_polygon(
                    vec![
                        tip,
                        back + side * len * *spread,
                        back - side * len * *spread,
                    ],
                    style.edge,
                    Stroke::NONE,
                ));
            }
            Prim::Cross { at, dir, len } => {
                let at = map(*at);
                let len = len * scale;
                let side = Vec2::new(-dir.y, dir.x);
                for s in [1.0_f32, -1.0] {
                    painter.line_segment(
                        [at, at + (*dir + side * s) * len * 0.9],
                        Stroke::new(1.4 * scale, style.edge),
                    );
                }
            }
            Prim::Text {
                at,
                text,
                size,
                plate,
                color,
            } => {
                if text.is_empty() {
                    continue;
                }
                let at = map(*at);
                let text_c = color.unwrap_or(style.text);
                let mut job = LayoutJob::default();
                job.append(
                    text,
                    0.0,
                    TextFormat {
                        font_id: FontId::proportional(size * scale),
                        color: text_c,
                        ..Default::default()
                    },
                );
                let galley = painter.layout_job(job);
                let pos = at - galley.size() * 0.5;
                if *plate {
                    // 边标签：先铺一块底色，否则线从字缝里穿过去读不清。
                    painter.rect_filled(
                        Rect::from_center_size(at, galley.size() + Vec2::splat(4.0 * scale)),
                        CornerRadius::same(2),
                        style.label_bg,
                    );
                }
                painter.galley(pos, galley, style.text);
            }
        }
    }
    layout.height * scale
}

/// 按可用宽度等比缩放后占用的高度（调用方排布卡片时用）。
pub fn fitted_height(layout: &MermaidLayout, max_w: f32) -> f32 {
    let scale = if layout.width > max_w && layout.width > 0.0 {
        max_w / layout.width
    } else {
        1.0
    };
    layout.height * scale
}

/// 供测试与调用方查询图元数量（绘制结果是否为空的自检入口）。
pub fn prim_count(layout: &MermaidLayout) -> usize {
    layout.prims.len()
}

/// 把（已 emit 的）图元与热点坐标统一平移到左上角为原点。
///
/// - `id_rects`：带 id 的元素外接框（供 `click id href "url"` 按 id 命中）；
/// - `links`：`click` 指令解析出的 `(id, url)`；
/// - `direct`：直接框定的热点（如标签里的 markdown 链接，元素本身无稳定 id）。
pub fn finalize(
    prims: Vec<Prim>,
    id_rects: Vec<(String, Rect)>,
    links: Vec<(String, String)>,
    direct: Vec<(Rect, String)>,
) -> MermaidLayout {
    let mut bounds: Option<Rect> = None;
    for p in &prims {
        bounds = Some(match bounds {
            Some(b) => b.union(prim_bounds(p)),
            None => prim_bounds(p),
        });
    }
    for (_, r) in &id_rects {
        bounds = Some(union_opt(bounds, *r));
    }
    for (r, _) in &direct {
        bounds = Some(union_opt(bounds, *r));
    }
    let bounds = bounds.unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, Vec2::splat(1.0)));
    let offset = -bounds.min.to_vec2();
    let prims: Vec<Prim> = prims.into_iter().map(|p| shift_prim(p, offset)).collect();
    let mut hotspots: Vec<(Rect, String)> = Vec::new();
    for (id, r) in &id_rects {
        if let Some((_, url)) = links.iter().find(|(i, _)| i == id) {
            hotspots.push((r.translate(offset), url.clone()));
        }
    }
    for (r, url) in &direct {
        hotspots.push((r.translate(offset), url.clone()));
    }
    MermaidLayout {
        width: bounds.width(),
        height: bounds.height(),
        prims,
        hotspots,
    }
}

/// `Option<Rect>` 与 `Rect` 取并集的辅助。
fn union_opt(acc: Option<Rect>, r: Rect) -> Rect {
    match acc {
        Some(a) => a.union(r),
        None => r,
    }
}

/// 扫描全部 `click <id> href "url"` 指令，返回 `(id, url)`。
///
/// `click <id> call fn` 没有 URL，忽略；缺引号等畸形也忽略——链接是增强特性，
/// 缺失不该让整张图回退。
pub fn collect_clicks(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in src.lines() {
        if let Some(rest) = line.trim().strip_prefix("click") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 3
                && parts[1] == "href"
                && let Some(url) = extract_quoted(rest)
            {
                out.push((parts[0].to_string(), url));
            }
        }
    }
    out
}

/// 取出第一对双引号之间的内容（不处理转义，够用）。
fn extract_quoted(s: &str) -> Option<String> {
    let a = s.find('"')?;
    let b = s[a + 1..].find('"').map(|i| a + 1 + i)?;
    Some(s[a + 1..b].to_string())
}

/// 从文本里取第一个 markdown 链接 `[文本](url)` 的 url。
pub fn first_md_link(text: &str) -> Option<String> {
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let rest = &text[i..];
            if let Some(close) = rest.find(']')
                && rest[close..].starts_with("](")
            {
                let inner = &rest[close + 2..];
                if let Some(end) = inner.find(')') {
                    return Some(inner[..end].trim().to_string());
                }
            }
        }
        i += 1;
    }
    None
}

/// 轴对齐矩形的四个角点（用于 `Poly`）。
pub fn rect_corners(r: Rect) -> Vec<Pos2> {
    vec![
        Pos2::new(r.left(), r.top()),
        Pos2::new(r.right(), r.top()),
        Pos2::new(r.right(), r.bottom()),
        Pos2::new(r.left(), r.bottom()),
    ]
}

/// 矩形（中心 + 半尺寸）沿 `dir` 方向到边界的交点。
pub fn rect_boundary(center: Pos2, half: Vec2, dir: Vec2) -> Pos2 {
    let t = if dir.x.abs() > f32::EPSILON {
        half.x / dir.x.abs()
    } else {
        f32::INFINITY
    };
    let u = if dir.y.abs() > f32::EPSILON {
        half.y / dir.y.abs()
    } else {
        f32::INFINITY
    };
    let k = t.min(u);
    Pos2::new(center.x + dir.x * k, center.y + dir.y * k)
}

/// 在矩形里画一个对勾（两段 `Line`），表示「已完成」。
pub fn push_check(prims: &mut Vec<Prim>, r: Rect, w: f32) {
    let cx = r.left() + r.width() * 0.3;
    let cy = r.center().y;
    let s = r.height() * 0.25;
    prims.push(Prim::Line {
        a: Pos2::new(cx - s, cy),
        b: Pos2::new(cx, cy + s),
        width: w,
        dotted: false,
    });
    prims.push(Prim::Line {
        a: Pos2::new(cx, cy + s),
        b: Pos2::new(cx + s * 1.6, cy - s),
        width: w,
        dotted: false,
    });
}
