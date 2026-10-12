//! 图元生成、坐标归一化收尾与 egui 绘制；各图种共用。

use egui::text::LayoutJob;
use egui::{
    Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, TextFormat, Vec2,
};

use super::model::{Edge, Graph, Head, LineStyle, MermaidLayout, NodeShape, Placed, Prim};
use super::*;

/// 发出图元：先把节点全部画出，再画边（箭头落在节点边界上，不会被节点盖住）。
pub(super) fn emit(g: &Graph, placed: &[Placed], size: f32, measure: Measure<'_>) -> Vec<Prim> {
    let mut prims: Vec<Prim> = Vec::new();
    for (i, p) in placed.iter().enumerate() {
        push_node(&mut prims, p, &g.nodes[i].0, size);
    }
    for e in &g.edges {
        push_edge(&mut prims, g, placed, e, size, measure);
    }
    prims
}

/// 一个节点的外形与文字。
fn push_node(prims: &mut Vec<Prim>, p: &Placed, text: &str, size: f32) {
    let c = p.center;
    let (hw, hh) = (p.size.x * 0.5, p.size.y * 0.5);
    let rect_pts = |w: f32, h: f32| {
        vec![
            Pos2::new(c.x - w, c.y - h),
            Pos2::new(c.x + w, c.y - h),
            Pos2::new(c.x + w, c.y + h),
            Pos2::new(c.x - w, c.y + h),
        ]
    };
    let corners = |hw: f32, hh: f32, radius: f32, ticks: bool| Prim::Poly {
        points: rect_pts(hw, hh),
        radius,
        fill: true,
        ticks,
    };
    match p.shape {
        NodeShape::Rect => prims.push(corners(hw, hh, 4.0, false)),
        NodeShape::Rounded => prims.push(corners(hw, hh, hh.min(12.0), false)),
        // 机场跑道：圆角半径等于半高。
        NodeShape::Stadium => prims.push(corners(hw, hh, hh, false)),
        NodeShape::Subroutine => prims.push(corners(hw, hh, 2.0, true)),
        // 数据库：用大圆角矩形近似真圆柱——epaint 没有椭圆填充图元，
        // 而圆角矩形在视觉上同样表达「存储」。
        NodeShape::Cylinder => prims.push(corners(hw, hh, hh * 0.7, false)),
        // 圆：尺寸阶段已把宽高调成正方形，故「圆角=半宽」即圆。
        NodeShape::Circle => prims.push(corners(hw, hh, hw.min(hh), false)),
        NodeShape::Rhombus => prims.push(Prim::Poly {
            points: vec![
                Pos2::new(c.x, c.y - hh),
                Pos2::new(c.x + hw, c.y),
                Pos2::new(c.x, c.y + hh),
                Pos2::new(c.x - hw, c.y),
            ],
            radius: 0.0,
            fill: true,
            ticks: false,
        }),
        NodeShape::Hexagon => {
            let inset = hw * 0.5;
            prims.push(Prim::Poly {
                points: vec![
                    Pos2::new(c.x - hw + inset, c.y - hh),
                    Pos2::new(c.x + hw - inset, c.y - hh),
                    Pos2::new(c.x + hw, c.y),
                    Pos2::new(c.x + hw - inset, c.y + hh),
                    Pos2::new(c.x - hw + inset, c.y + hh),
                    Pos2::new(c.x - hw, c.y),
                ],
                radius: 0.0,
                fill: true,
                ticks: false,
            });
        }
        NodeShape::Parallelogram => {
            let skew = hw * 0.4;
            prims.push(Prim::Poly {
                points: vec![
                    Pos2::new(c.x - hw + skew, c.y - hh),
                    Pos2::new(c.x + hw, c.y - hh),
                    Pos2::new(c.x + hw - skew, c.y + hh),
                    Pos2::new(c.x - hw, c.y + hh),
                ],
                radius: 0.0,
                fill: true,
                ticks: false,
            });
        }
        NodeShape::Asymmetric => {
            prims.push(Prim::Poly {
                points: vec![
                    Pos2::new(c.x - hw, c.y - hh),
                    Pos2::new(c.x + hw - hh * 0.6, c.y - hh),
                    Pos2::new(c.x + hw, c.y),
                    Pos2::new(c.x + hw - hh * 0.6, c.y + hh),
                    Pos2::new(c.x - hw, c.y + hh),
                ],
                radius: 0.0,
                fill: true,
                ticks: false,
            });
        }
    }
    prims.push(Prim::Text {
        at: c,
        text: text.to_string(),
        size,
        plate: false,
    });
}

/// 一条边：走线 + 箭头 + 标签。
fn push_edge(
    prims: &mut Vec<Prim>,
    g: &Graph,
    placed: &[Placed],
    e: &Edge,
    size: f32,
    measure: Measure<'_>,
) {
    let a = &placed[e.from];
    let b = &placed[e.to];
    // 线宽随字号缩放，但保底 0.7 点，否则小字号下图会「消失」。
    let width = match e.line {
        LineStyle::Thick => 2.4,
        _ => 1.4,
    } * (size / 16.0).max(0.7);
    let dotted = e.line == LineStyle::Dotted;

    // 自环：在节点上方绕一个小圈（`A --> A` 在状态图里很常见）。
    if e.from == e.to {
        let top = Pos2::new(a.center.x, a.center.y - a.size.y * 0.5);
        let r = size * 0.7;
        let pts = [
            top,
            Pos2::new(top.x - r, top.y - r),
            Pos2::new(top.x + r, top.y - r),
            top,
        ];
        for w in pts.windows(2) {
            prims.push(Prim::Line {
                a: w[0],
                b: w[1],
                width,
                dotted,
            });
        }
        prims.push(Prim::Arrow {
            tip: top,
            dir: Vec2::new(0.0, 1.0),
            len: size * ARROW_LEN,
            spread: ARROW_SPREAD,
        });
        if let Some(label) = &e.label {
            prims.push(Prim::Text {
                at: Pos2::new(top.x, top.y - r * 1.6),
                text: label.clone(),
                size: size * 0.85,
                plate: false,
            });
        }
        return;
    }

    // 出 / 入方向：沿主轴由 a 指向 b，另一轴上则互相靠近（正交走线）。
    let (out_dir, in_dir) = if g.horizontal {
        let sign = if b.center.x >= a.center.x { 1.0 } else { -1.0 };
        (Vec2::new(sign, 0.0), Vec2::new(sign, 0.0))
    } else {
        let sign = if b.center.y >= a.center.y { 1.0 } else { -1.0 };
        (Vec2::new(0.0, sign), Vec2::new(0.0, sign))
    };

    // 从节点**边界**出发而不是中心：否则线会被节点盖住一半，箭头也看不见。
    let start = boundary_point(a, out_dir);
    let end = boundary_point(b, -in_dir);
    let route = route_points(start, end, out_dir, in_dir);
    for w in route.windows(2) {
        prims.push(Prim::Line {
            a: w[0],
            b: w[1],
            width,
            dotted,
        });
    }
    if e.bidirectional {
        prims.push(Prim::Arrow {
            tip: start,
            dir: -out_dir,
            len: size * ARROW_LEN,
            spread: ARROW_SPREAD,
        });
    }
    match e.head {
        Head::Arrow => prims.push(Prim::Arrow {
            tip: end,
            dir: -in_dir,
            len: size * ARROW_LEN,
            spread: ARROW_SPREAD,
        }),
        Head::Cross => prims.push(Prim::Cross {
            at: end,
            dir: in_dir,
            len: size * ARROW_LEN * 0.7,
        }),
        // `--o`：空心圆头用「很扁的三角」不行，改画一个小线段交叉的近似——
        // 这里用短而宽的三角，视觉上读作圆点（见 paint 的填充色）。
        Head::Circle => prims.push(Prim::Arrow {
            tip: end,
            dir: -in_dir,
            len: size * ARROW_LEN * 0.45,
            spread: 1.0,
        }),
        Head::None => {}
    }

    if let Some(label) = &e.label {
        // 标签压在线上读不清，故 `plate = true` 铺一块底色把线挡住（见 paint）。
        let _ = measure;
        prims.push(Prim::Text {
            at: polyline_midpoint(&route),
            text: label.clone(),
            size: size * 0.85,
            plate: true,
        });
    }
}

/// 取节点边界上沿 `dir` 方向的交点。
fn boundary_point(p: &Placed, dir: Vec2) -> Pos2 {
    let (hw, hh) = (p.size.x * 0.5, p.size.y * 0.5);
    // 菱形 / 圆按更小的内切半径算，让线不要插进斜边里。
    let (sx, sy) = match p.shape {
        NodeShape::Rhombus => (hw * 0.62, hh * 0.62),
        NodeShape::Circle => (hw * 0.8, hh * 0.8),
        _ => (hw, hh),
    };
    let t = if dir.x.abs() > f32::EPSILON {
        sx / dir.x.abs()
    } else {
        f32::INFINITY
    };
    let u = if dir.y.abs() > f32::EPSILON {
        sy / dir.y.abs()
    } else {
        f32::INFINITY
    };
    let k = t.min(u);
    Pos2::new(p.center.x + dir.x * k, p.center.y + dir.y * k)
}

/// 走线：同轴对齐时走直线，否则走「出边 → 中点干 → 入边」的折线。
fn route_points(start: Pos2, end: Pos2, out_dir: Vec2, in_dir: Vec2) -> Vec<Pos2> {
    let aligned = (out_dir.x.abs() > 0.5 && (end.y - start.y).abs() < 1.0)
        || (out_dir.y.abs() > 0.5 && (end.x - start.x).abs() < 1.0);
    if aligned {
        return vec![start, end];
    }
    if out_dir.x.abs() > 0.5 {
        let mid_x = (start.x + end.x) * 0.5;
        vec![
            start,
            Pos2::new(start.x + out_dir.x * ROUTE_STUB, start.y),
            Pos2::new(mid_x, start.y),
            Pos2::new(mid_x, end.y),
            Pos2::new(end.x - in_dir.x * ROUTE_STUB, end.y),
            end,
        ]
    } else {
        let mid_y = (start.y + end.y) * 0.5;
        vec![
            start,
            Pos2::new(start.x, start.y + out_dir.y * ROUTE_STUB),
            Pos2::new(start.x, mid_y),
            Pos2::new(end.x, mid_y),
            Pos2::new(end.x, end.y - in_dir.y * ROUTE_STUB),
            end,
        ]
    }
}

/// 折线的中点（用于放标签）。
fn polyline_midpoint(pts: &[Pos2]) -> Pos2 {
    let total: f32 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    let mut want = total * 0.5;
    for w in pts.windows(2) {
        let seg = (w[1] - w[0]).length();
        if seg >= want {
            let t = if seg > 0.0 { want / seg } else { 0.0 };
            return w[0] + (w[1] - w[0]) * t;
        }
        want -= seg;
    }
    *pts.last().unwrap_or(&Pos2::ZERO)
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
        } => Prim::Poly {
            points: points.into_iter().map(|q| q + d).collect(),
            radius,
            fill,
            ticks,
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
        } => Prim::Text {
            at: at + d,
            text,
            size,
            plate,
        },
    }
}

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
            } => {
                let pts: Vec<Pos2> = points.iter().map(|p| map(*p)).collect();
                let stroke = Stroke::new(1.2 * scale, style.node_stroke);
                if *fill {
                    let r = Rect::from_points(&pts);
                    if pts.len() == 4 && *radius > 0.0 {
                        painter.rect_filled(
                            r,
                            CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8),
                            style.node_fill,
                        );
                    } else {
                        painter.add(Shape::convex_polygon(
                            pts.clone(),
                            style.node_fill,
                            Stroke::NONE,
                        ));
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
                            Stroke::new(1.0 * scale, style.node_stroke),
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
            } => {
                if text.is_empty() {
                    continue;
                }
                let at = map(*at);
                let mut job = LayoutJob::default();
                job.append(
                    text,
                    0.0,
                    TextFormat {
                        font_id: FontId::proportional(size * scale),
                        color: style.text,
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
pub(super) fn finalize(
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
pub(super) fn collect_clicks(src: &str) -> Vec<(String, String)> {
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
pub(super) fn first_md_link(text: &str) -> Option<String> {
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
pub(super) fn rect_corners(r: Rect) -> Vec<Pos2> {
    vec![
        Pos2::new(r.left(), r.top()),
        Pos2::new(r.right(), r.top()),
        Pos2::new(r.right(), r.bottom()),
        Pos2::new(r.left(), r.bottom()),
    ]
}

/// 矩形（中心 + 半尺寸）沿 `dir` 方向到边界的交点。
pub(super) fn rect_boundary(center: Pos2, half: Vec2, dir: Vec2) -> Pos2 {
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

/// 给矩形描一圈边（四段 `Line`，沿用 `edge` 配色）。
pub(super) fn push_rect_outline(prims: &mut Vec<Prim>, r: Rect, w: f32) {
    let c = rect_corners(r);
    for k in 0..4 {
        let a = c[k];
        let b = c[(k + 1) % 4];
        prims.push(Prim::Line {
            a,
            b,
            width: w,
            dotted: false,
        });
    }
}

/// 在矩形里画一个对勾（两段 `Line`），表示「已完成」。
pub(super) fn push_check(prims: &mut Vec<Prim>, r: Rect, w: f32) {
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
