//! 图元生成：emit 入口 + 节点外形 + 边线/箭头/标签。
use egui::{Pos2, Vec2};

use super::super::model::{Edge, Graph, Head, LineStyle, NodeShape, Placed, Prim};
use super::super::*;

/// 发出图元：先把节点全部画出，再画边（箭头落在节点边界上，不会被节点盖住）。
pub(crate) fn emit(g: &Graph, placed: &[Placed], size: f32, measure: Measure<'_>) -> Vec<Prim> {
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
        fill_color: None,
        stroke_color: None,
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
            fill_color: None,
            stroke_color: None,
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
                fill_color: None,
                stroke_color: None,
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
                fill_color: None,
                stroke_color: None,
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
                fill_color: None,
                stroke_color: None,
            });
        }
    }
    prims.push(Prim::Text {
        at: c,
        text: text.to_string(),
        size,
        plate: false,
        color: None,
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
                color: None,
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
            color: None,
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
