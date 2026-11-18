//! `quadrantChart` 象限图：十字坐标轴 + 四个象限名 + 散点。
//!
//! 语法：
//!
//! ```text
//! quadrantChart
//!   title 优先级
//!   x-axis 低 --> 高
//!   y-axis 低 --> 高
//!   quadrant-1 立即做
//!   quadrant-2 计划
//!   quadrant-3 丢弃
//!   quadrant-4 委派
//!   任务A: [0.3, 0.6]
//! ```
//!
//! 点坐标是 `[0, 1]` 的归一化值，`y` 轴向上为正（与屏幕坐标相反）。

use crate::render::{Pt, geom::Rect};

use super::model::Prim;
use super::primitives::{collect_clicks, finalize, rect_corners};
use super::*;

/// 一个散点。
#[derive(Clone, Debug)]
pub(super) struct QuadPoint {
    pub(super) label: String,
    pub(super) x: f32,
    pub(super) y: f32,
}

/// 解析后的象限图。
#[derive(Clone, Debug)]
pub(super) struct QuadModel {
    pub(super) title: Option<String>,
    pub(super) x_low: String,
    pub(super) x_high: String,
    pub(super) y_low: String,
    pub(super) y_high: String,
    /// `quadrant-1` … `quadrant-4` 的名字（1 右上、2 左上、3 左下、4 右下）。
    pub(super) quads: [String; 4],
    pub(super) points: Vec<QuadPoint>,
}

/// 去掉首尾空白与成对的双引号。
fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim().to_string()
}

/// 解析 `低 --> 高` 形式的轴端点。
fn axis_ends(rest: &str) -> Option<(String, String)> {
    let (lo, hi) = rest.split_once("-->")?;
    Some((unquote(lo), unquote(hi)))
}

/// 解析 `[0.3, 0.6]`。
fn point_coords(rest: &str) -> Option<(f32, f32)> {
    let a = rest.find('[')?;
    let b = rest.rfind(']')?;
    if b <= a {
        return None;
    }
    let mut it = rest[a + 1..b].split(',');
    let x: f32 = it.next()?.trim().parse().ok()?;
    let y: f32 = it.next()?.trim().parse().ok()?;
    Some((x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)))
}

/// 解析象限图源码。
pub(super) fn parse_quadrant(src: &str) -> Result<QuadModel, MermaidError> {
    let mut model = QuadModel {
        title: None,
        x_low: "低".to_string(),
        x_high: "高".to_string(),
        y_low: "低".to_string(),
        y_high: "高".to_string(),
        quads: std::array::from_fn(|_| String::new()),
        points: Vec::new(),
    };
    let mut seen_header = false;
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        let kw = t.split_whitespace().next().unwrap_or("");
        if !seen_header {
            seen_header = true;
            if kw.eq_ignore_ascii_case("quadrantChart") {
                continue;
            }
        }
        if kw.eq_ignore_ascii_case("title") {
            let rest = unquote(&t[kw.len()..]);
            model.title = (!rest.is_empty()).then_some(rest);
        } else if kw.eq_ignore_ascii_case("x-axis") {
            if let Some((lo, hi)) = axis_ends(&t[kw.len()..]) {
                model.x_low = lo;
                model.x_high = hi;
            }
        } else if kw.eq_ignore_ascii_case("y-axis") {
            if let Some((lo, hi)) = axis_ends(&t[kw.len()..]) {
                model.y_low = lo;
                model.y_high = hi;
            }
        } else if let Some(n) = kw.strip_prefix("quadrant-") {
            if let Ok(idx) = n.parse::<usize>()
                && (1..=4).contains(&idx)
            {
                model.quads[idx - 1] = t[kw.len()..].trim().to_string();
            }
        } else if let Some((label, rest)) = t.split_once(':') {
            let Some((x, y)) = point_coords(rest) else {
                return Err(MermaidError::Malformed(format!(
                    "象限图坐标应形如 `名称: [0.3, 0.6]`：{t}"
                )));
            };
            model.points.push(QuadPoint {
                label: label.trim().to_string(),
                x,
                y,
            });
        }
    }
    Ok(model)
}

/// 紧凑数值写法。
fn fmt_num(v: f32) -> String {
    if (v - v.round()).abs() < 1e-3 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// 象限图布局 + 发图元。
pub(super) fn layout_quadrant(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_quadrant(src)?;
    if model.points.is_empty() {
        return Err(MermaidError::Malformed("象限图没有任何数据点".to_string()));
    }

    let margin = size * 1.2;
    let title_h = if model.title.is_some() {
        size * 2.0
    } else {
        0.0
    };
    // 左侧给 y 轴标题与刻度留白。
    let left = margin + size * 3.0;
    let top = margin + title_h;
    let side = size * 10.0;
    let plot = Rect::from_min_size(Pt::new(left, top), Pt::splat(side));
    let line_w = 1.4 * (size / 16.0).max(0.7);

    let mut prims: Vec<Prim> = Vec::new();
    if let Some(title) = &model.title {
        prims.push(Prim::Text {
            at: Pt::new(plot.center().x, margin + size * 0.5),
            text: title.clone(),
            size: size * 1.1,
            plate: false,
            color: None,
        });
    }

    // 外框 + 十字轴。
    prims.push(Prim::Poly {
        points: rect_corners(plot),
        radius: 2.0,
        fill: false,
        ticks: false,
        fill_color: None,
        stroke_color: None,
    });
    let mid_x = plot.left() + side * 0.5;
    let mid_y = plot.top() + side * 0.5;
    prims.push(Prim::Line {
        a: Pt::new(mid_x, plot.top()),
        b: Pt::new(mid_x, plot.bottom()),
        width: line_w,
        dotted: true,
    });
    prims.push(Prim::Line {
        a: Pt::new(plot.left(), mid_y),
        b: Pt::new(plot.right(), mid_y),
        width: line_w,
        dotted: true,
    });

    // 轴端点文字。
    prims.push(Prim::Text {
        at: Pt::new(plot.left() + size * 0.6, plot.bottom() + size * 0.8),
        text: model.x_low.clone(),
        size: size * 0.85,
        plate: false,
        color: None,
    });
    prims.push(Prim::Text {
        at: Pt::new(plot.right() - size * 0.6, plot.bottom() + size * 0.8),
        text: model.x_high.clone(),
        size: size * 0.85,
        plate: false,
        color: None,
    });
    prims.push(Prim::Text {
        at: Pt::new(plot.left() - size * 0.6, plot.bottom() - size * 0.6),
        text: model.y_low.clone(),
        size: size * 0.85,
        plate: false,
        color: None,
    });
    prims.push(Prim::Text {
        at: Pt::new(plot.left() - size * 0.6, plot.top() + size * 0.4),
        text: model.y_high.clone(),
        size: size * 0.85,
        plate: false,
        color: None,
    });

    // 象限名：贴着各自角落（quadrant-1 右上、2 左上、3 左下、4 右下）。
    let corner_pad = size * 0.8;
    let quad_pos = [
        Pt::new(plot.right() - corner_pad, plot.top() + corner_pad),
        Pt::new(plot.left() + corner_pad, plot.top() + corner_pad),
        Pt::new(plot.left() + corner_pad, plot.bottom() - corner_pad),
        Pt::new(plot.right() - corner_pad, plot.bottom() - corner_pad),
    ];
    for (i, name) in model.quads.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        let w = measure(name, size * 0.85);
        // 文本锚点在中心：按角落把锚点往内推半个字宽。
        let inward = match i {
            0 | 3 => -w * 0.5,
            _ => w * 0.5,
        };
        prims.push(Prim::Text {
            at: Pt::new(quad_pos[i].x + inward, quad_pos[i].y),
            text: name.clone(),
            size: size * 0.85,
            plate: false,
            color: None,
        });
    }

    // 散点：位置 + 标签。
    let dot = size * 0.4;
    for (i, p) in model.points.iter().enumerate() {
        let x = plot.left() + p.x * side;
        let y = plot.bottom() - p.y * side;
        prims.push(Prim::Poly {
            points: rect_corners(Rect::from_center_size(Pt::new(x, y), Pt::splat(dot * 2.0))),
            radius: dot,
            fill: true,
            ticks: false,
            fill_color: Some(palette_color(i)),
            stroke_color: None,
        });
        prims.push(Prim::Text {
            at: Pt::new(x, y - dot - size * 0.5),
            text: format!("{} ({}, {})", p.label, fmt_num(p.x), fmt_num(p.y)),
            size: size * 0.8,
            plate: false,
            color: None,
        });
    }

    Ok(finalize(prims, Vec::new(), collect_clicks(src), Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> impl Fn(&str, f32) -> f32 {
        |s: &str, size: f32| s.chars().count() as f32 * size * 0.5
    }

    fn lay(src: &str) -> MermaidLayout {
        let f = m();
        layout(src, 14.0, &f).expect("应能布局")
    }

    #[test]
    fn parses_axes_quadrants_and_points() {
        let model = parse_quadrant(
            "quadrantChart\n\
             title 优先级\n\
             x-axis 不重要 --> 重要\n\
             y-axis 不紧急 --> 紧急\n\
             quadrant-1 立即做\n\
             quadrant-3 丢弃\n\
             任务A: [0.3, 0.6]\n",
        )
        .expect("解析象限图");
        assert_eq!(model.title.as_deref(), Some("优先级"));
        assert_eq!(model.x_low, "不重要");
        assert_eq!(model.y_high, "紧急");
        assert_eq!(model.quads[0], "立即做");
        assert_eq!(model.quads[2], "丢弃");
        assert_eq!(model.points.len(), 1);
        assert!((model.points[0].x - 0.3).abs() < 1e-6);
        assert!((model.points[0].y - 0.6).abs() < 1e-6);
    }

    #[test]
    fn layout_has_positive_size_and_prims() {
        let l = lay("quadrantChart\nquadrant-1 A\nA: [0.2, 0.8]\nB: [0.7, 0.4]\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        assert!(prim_count(&l) > 0);
    }

    #[test]
    fn coordinates_are_clamped() {
        let model = parse_quadrant("quadrantChart\nA: [1.8, -0.5]\n").expect("解析");
        assert!((model.points[0].x - 1.0).abs() < 1e-6);
        assert!(model.points[0].y.abs() < 1e-6);
    }

    #[test]
    fn point_without_coordinates_is_malformed() {
        let f = m();
        assert!(matches!(
            layout("quadrantChart\nA: 右上\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
    }

    /// 绘图区外框（唯一的非填充多边形）。
    fn frame_rect(l: &MermaidLayout) -> Rect {
        l.prims
            .iter()
            .find_map(|p| match p {
                Prim::Poly {
                    fill: false,
                    points,
                    ..
                } => Some(Rect::from_points(points)),
                _ => None,
            })
            .expect("应画出绘图区外框")
    }

    /// 某个文字图元的锚点。
    fn text_pos(l: &MermaidLayout, want: &str) -> Pt {
        l.prims
            .iter()
            .find_map(|p| match p {
                Prim::Text { at, text, .. } if text == want => Some(*at),
                _ => None,
            })
            .unwrap_or_else(|| panic!("缺少文字 {want}"))
    }

    /// 散点（填充多边形）的外接框，按出现顺序。
    fn dots(l: &MermaidLayout) -> Vec<Rect> {
        l.prims
            .iter()
            .filter_map(|p| match p {
                Prim::Poly {
                    fill: true, points, ..
                } => Some(Rect::from_points(points)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn missing_axes_fall_back_to_defaults() {
        let src = "quadrantChart\nA: [0.2, 0.8]\n";
        let model = parse_quadrant(src).expect("解析象限图");
        assert_eq!(model.x_low, "低");
        assert_eq!(model.x_high, "高");
        assert_eq!(model.y_low, "低");
        assert_eq!(model.y_high, "高");
        let l = lay(src);
        // 四个端点文字仍然画出来。
        for want in ["低", "高"] {
            assert!(
                l.prims
                    .iter()
                    .any(|p| matches!(p, Prim::Text { text, .. } if text == want))
            );
        }
    }

    #[test]
    fn quadrant_names_map_to_corners() {
        let src = "quadrantChart\n\
                   quadrant-1 右上\n\
                   quadrant-2 左上\n\
                   quadrant-3 左下\n\
                   quadrant-4 右下\n\
                   P: [0.5, 0.5]\n";
        let model = parse_quadrant(src).expect("解析象限图");
        assert_eq!(model.quads, ["右上", "左上", "左下", "右下"]);
        let l = lay(src);
        let c = frame_rect(&l).center();
        let q1 = text_pos(&l, "右上");
        let q2 = text_pos(&l, "左上");
        let q3 = text_pos(&l, "左下");
        let q4 = text_pos(&l, "右下");
        assert!(q1.x > c.x && q1.y < c.y, "quadrant-1 应在右上");
        assert!(q2.x < c.x && q2.y < c.y, "quadrant-2 应在左上");
        assert!(q3.x < c.x && q3.y > c.y, "quadrant-3 应在左下");
        assert!(q4.x > c.x && q4.y > c.y, "quadrant-4 应在右下");
    }

    #[test]
    fn origin_point_is_top_left_and_opposite_is_bottom_right() {
        let l = lay("quadrantChart\nA: [0, 1]\nB: [1, 0]\n");
        let d = dots(&l);
        assert_eq!(d.len(), 2);
        assert!(d[0].center().x < d[1].center().x);
        assert!(d[0].center().y < d[1].center().y);
    }

    #[test]
    fn four_points_use_distinct_palette_colors() {
        let l = lay("quadrantChart\nA: [0.1, 0.9]\nB: [0.9, 0.9]\nC: [0.9, 0.1]\nD: [0.1, 0.1]\n");
        let fills: Vec<crate::render::Rgb8> = l
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Poly {
                    fill: true,
                    fill_color: Some(c),
                    ..
                } => Some(*c),
                _ => None,
            })
            .collect();
        assert_eq!(fills.len(), 4);
        for (i, c) in fills.iter().enumerate() {
            assert_eq!(*c, palette_color(i));
        }
        for i in 0..fills.len() {
            for j in i + 1..fills.len() {
                assert_ne!(fills[i], fills[j]);
            }
        }
    }

    #[test]
    fn axis_without_arrow_falls_back_to_defaults() {
        let src = "quadrantChart\nx-axis 只有一端\nA: [0.5, 0.5]\n";
        let model = parse_quadrant(src).expect("解析象限图");
        assert_eq!(model.x_low, "低");
        assert_eq!(model.x_high, "高");
        // 不报错，照常出图。
        let l = lay(src);
        assert!(l.width > 0.0 && l.height > 0.0);
    }

    #[test]
    fn out_of_range_coordinates_are_clamped_in_layout() {
        // 越界点应被夹到 (1, 1)，与显式 (1, 1) 的点重合。
        let l = lay("quadrantChart\n远: [2, 3]\n角: [1, 1]\n");
        let d = dots(&l);
        assert_eq!(d.len(), 2);
        assert!((d[0].center().x - d[1].center().x).abs() < 1e-6);
        assert!((d[0].center().y - d[1].center().y).abs() < 1e-6);
    }
}
