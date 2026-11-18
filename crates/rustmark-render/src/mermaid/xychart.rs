//! `xychart` / `xychart-beta` 柱线图：分类 X 轴 + 数值 Y 轴 + 柱 / 线序列。
//!
//! 语法：
//!
//! ```text
//! xychart-beta
//!   title "销售额"
//!   x-axis [一月, 二月, 三月]
//!   y-axis "金额" 0 --> 100
//!   bar [10, 20, 30]
//!   line [12, 18, 35]
//! ```
//!
//! 柱用固定调色板第 1 色，线用第 2 色（点标记）；Y 轴缺省范围按数据自动
//! 取 `0 → 最大值 × 1.1`。

use crate::render::{Pt, geom::Rect};

use super::model::Prim;
use super::primitives::{collect_clicks, finalize, rect_corners};
use super::*;

/// 解析后的柱线图。
#[derive(Clone, Debug, Default)]
pub(super) struct XyModel {
    pub(super) title: Option<String>,
    pub(super) categories: Vec<String>,
    pub(super) y_title: Option<String>,
    pub(super) y_min: Option<f32>,
    pub(super) y_max: Option<f32>,
    pub(super) bars: Vec<f32>,
    pub(super) line: Vec<f32>,
}

/// 去掉首尾空白与成对的双引号。
fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim().to_string()
}

/// 取 `[...]` 里的逗号分隔项。
fn bracket_list(s: &str) -> Option<Vec<String>> {
    let a = s.find('[')?;
    let b = s.rfind(']')?;
    if b <= a {
        return None;
    }
    Some(
        s[a + 1..b]
            .split(',')
            .map(unquote)
            .filter(|x| !x.is_empty())
            .collect(),
    )
}

/// 取 `[...]` 里的数值。
fn bracket_numbers(s: &str) -> Option<Vec<f32>> {
    bracket_list(s)?.iter().map(|x| x.parse().ok()).collect()
}

/// 解析柱线图源码。
pub(super) fn parse_xychart(src: &str) -> Result<XyModel, MermaidError> {
    let mut model = XyModel::default();
    let mut seen_header = false;
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        let kw = t.split_whitespace().next().unwrap_or("");
        if !seen_header {
            seen_header = true;
            if kw.eq_ignore_ascii_case("xychart") {
                continue;
            }
        }
        if kw.eq_ignore_ascii_case("title") {
            let rest = unquote(&t[kw.len()..]);
            model.title = (!rest.is_empty()).then_some(rest);
        } else if kw.eq_ignore_ascii_case("x-axis") {
            let rest = &t[kw.len()..];
            let Some(cats) = bracket_list(rest) else {
                return Err(MermaidError::Malformed(format!(
                    "x 轴应写成 `x-axis [类别, ...]`：{t}"
                )));
            };
            model.categories = cats;
        } else if kw.eq_ignore_ascii_case("y-axis") {
            let rest = &t[kw.len()..];
            // 引号里是轴标题，其余是 `min --> max`。
            let mut tail = rest;
            if let Some(q) = rest.trim_start().strip_prefix('"')
                && let Some(close) = q.find('"')
            {
                model.y_title = Some(q[..close].to_string());
                tail = &q[close + 1..];
            }
            if let Some((lo, hi)) = tail.split_once("-->") {
                model.y_min = lo.trim().parse().ok();
                model.y_max = hi.trim().parse().ok();
            }
        } else if kw.eq_ignore_ascii_case("bar") {
            let Some(vals) = bracket_numbers(t) else {
                return Err(MermaidError::Malformed(format!(
                    "柱序列应写成 `bar [数值, ...]`：{t}"
                )));
            };
            model.bars = vals;
        } else if kw.eq_ignore_ascii_case("line") {
            let Some(vals) = bracket_numbers(t) else {
                return Err(MermaidError::Malformed(format!(
                    "线序列应写成 `line [数值, ...]`：{t}"
                )));
            };
            model.line = vals;
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

/// 柱线图布局 + 发图元。
pub(super) fn layout_xychart(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_xychart(src)?;
    let n = model
        .categories
        .len()
        .max(model.bars.len())
        .max(model.line.len());
    if n == 0 {
        return Err(MermaidError::Malformed(
            "柱线图既没有类别轴也没有数据序列".to_string(),
        ));
    }

    // Y 轴范围：显式优先，否则按数据自动。
    let mut data_max = 0.0_f32;
    let mut data_min = 0.0_f32;
    for v in model.bars.iter().chain(model.line.iter()) {
        data_max = data_max.max(*v);
        data_min = data_min.min(*v);
    }
    let y_min = model.y_min.unwrap_or_else(|| data_min.min(0.0));
    let mut y_max = model
        .y_max
        .unwrap_or(if data_max > 0.0 { data_max * 1.1 } else { 1.0 });
    if y_max <= y_min {
        y_max = y_min + 1.0;
    }

    let margin = size * 1.2;
    let title_h = if model.title.is_some() {
        size * 2.0
    } else {
        0.0
    };
    // 左侧：y 轴标题 + 刻度文字。
    let left = margin + size * 2.0 + measure(&fmt_num(y_max), size * 0.8);
    let top = margin + title_h;
    let plot_w = size * 12.0;
    let plot_h = size * 7.0;
    let plot = Rect::from_min_size(Pt::new(left, top), Pt::new(plot_w, plot_h));
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
    if let Some(y_title) = &model.y_title {
        prims.push(Prim::Text {
            at: Pt::new(margin + size * 0.4, plot.top() - size * 0.8),
            text: y_title.clone(),
            size: size * 0.8,
            plate: false,
            color: None,
        });
    }

    let to_y = |v: f32| plot.bottom() - (v - y_min) / (y_max - y_min) * plot_h;

    // 横向网格 + 刻度。
    const TICKS: usize = 5;
    for i in 0..=TICKS {
        let frac = i as f32 / TICKS as f32;
        let v = y_min + (y_max - y_min) * frac;
        let y = to_y(v);
        prims.push(Prim::Line {
            a: Pt::new(plot.left(), y),
            b: Pt::new(plot.right(), y),
            width: line_w * 0.6,
            dotted: true,
        });
        let label = fmt_num(v);
        prims.push(Prim::Text {
            at: Pt::new(
                plot.left() - size * 0.4 - measure(&label, size * 0.8) * 0.5,
                y,
            ),
            text: label,
            size: size * 0.8,
            plate: false,
            color: None,
        });
    }

    // 轴基线。
    prims.push(Prim::Line {
        a: Pt::new(plot.left(), plot.bottom()),
        b: Pt::new(plot.right(), plot.bottom()),
        width: line_w,
        dotted: false,
    });
    prims.push(Prim::Line {
        a: Pt::new(plot.left(), plot.top()),
        b: Pt::new(plot.left(), plot.bottom()),
        width: line_w,
        dotted: false,
    });

    let slot = plot_w / n as f32;
    let zero_y = to_y(0.0_f32.clamp(y_min, y_max));

    // 柱：从 0 基线长到数值。
    let bar_w = slot * 0.55;
    for (i, v) in model.bars.iter().enumerate() {
        let cx = plot.left() + slot * (i as f32 + 0.5);
        let top_y = to_y(*v);
        let rect = Rect::from_min_max(
            Pt::new(cx - bar_w * 0.5, top_y.min(zero_y)),
            Pt::new(cx + bar_w * 0.5, top_y.max(zero_y)),
        );
        prims.push(Prim::Poly {
            points: rect_corners(rect),
            radius: 2.0,
            fill: true,
            ticks: false,
            fill_color: Some(palette_color(0)),
            stroke_color: None,
        });
    }

    // 线：折线 + 点标记。
    let pts: Vec<Pt> = model
        .line
        .iter()
        .enumerate()
        .map(|(i, v)| Pt::new(plot.left() + slot * (i as f32 + 0.5), to_y(*v)))
        .collect();
    for w in pts.windows(2) {
        prims.push(Prim::Line {
            a: w[0],
            b: w[1],
            width: line_w * 1.4,
            dotted: false,
        });
    }
    let dot = size * 0.24;
    for p in &pts {
        prims.push(Prim::Poly {
            points: rect_corners(Rect::from_center_size(*p, Pt::splat(dot * 2.0))),
            radius: dot,
            fill: true,
            ticks: false,
            fill_color: Some(palette_color(1)),
            stroke_color: None,
        });
    }

    // X 轴类别标签。
    for i in 0..n {
        let Some(label) = model.categories.get(i) else {
            break;
        };
        prims.push(Prim::Text {
            at: Pt::new(
                plot.left() + slot * (i as f32 + 0.5),
                plot.bottom() + size * 0.9,
            ),
            text: label.clone(),
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
    fn parses_categories_axes_and_series() {
        let model = parse_xychart(
            "xychart-beta\n\
             title \"销售额\"\n\
             x-axis [一月, 二月, 三月]\n\
             y-axis \"金额\" 0 --> 100\n\
             bar [10, 20, 30]\n\
             line [12, 18, 35]\n",
        )
        .expect("解析柱线图");
        assert_eq!(model.title.as_deref(), Some("销售额"));
        assert_eq!(model.categories, ["一月", "二月", "三月"]);
        assert_eq!(model.y_title.as_deref(), Some("金额"));
        assert_eq!((model.y_min, model.y_max), (Some(0.0), Some(100.0)));
        assert_eq!(model.bars, [10.0, 20.0, 30.0]);
        assert_eq!(model.line, [12.0, 18.0, 35.0]);
    }

    #[test]
    fn layout_has_positive_size_and_prims() {
        let l = lay("xychart-beta\nx-axis [a, b]\nbar [3, 5]\nline [4, 2]\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        assert!(prim_count(&l) > 0);
    }

    #[test]
    fn auto_y_range_from_data() {
        let model = parse_xychart("xychart-beta\nx-axis [a]\nbar [10]\n").expect("解析");
        assert!(model.y_max.is_none(), "未写 y-axis 时不伪造显式范围");
        let l = lay("xychart-beta\nx-axis [a]\nbar [10]\n");
        assert!(l.height > 0.0);
    }

    #[test]
    fn empty_chart_is_malformed() {
        let f = m();
        assert!(matches!(
            layout("xychart-beta\ntitle 空\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
    }

    /// 第一根柱子的外接框（填充多边形里第一个）。
    fn first_bar(l: &MermaidLayout) -> Rect {
        l.prims
            .iter()
            .find_map(|p| match p {
                Prim::Poly {
                    fill: true, points, ..
                } => Some(Rect::from_points(points)),
                _ => None,
            })
            .expect("应画出柱子")
    }

    /// 版面里所有文字图元的文本。
    fn texts(l: &MermaidLayout) -> Vec<&str> {
        l.prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn bar_only_and_line_only_layout() {
        let with_bar = lay("xychart-beta\nx-axis [a, b]\nbar [3, 5]\n");
        let with_line = lay("xychart-beta\nx-axis [a, b]\nline [3, 5]\n");
        assert!(with_bar.width > 0.0 && with_bar.height > 0.0);
        assert!(with_line.width > 0.0 && with_line.height > 0.0);
        assert!(prim_count(&with_bar) > 0);
        assert!(prim_count(&with_line) > 0);
    }

    #[test]
    fn fewer_categories_than_series_does_not_panic() {
        let l = lay("xychart-beta\nx-axis [a]\nbar [1, 2, 3]\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        // 只有 1 个类别标签，多余数据不补标签。
        assert_eq!(texts(&l).iter().filter(|t| **t == "a").count(), 1);
        // 3 根柱子。
        assert_eq!(
            l.prims
                .iter()
                .filter(|p| matches!(
                    p,
                    Prim::Poly {
                        fill: true,
                        fill_color: Some(c),
                        ..
                    } if *c == palette_color(0)
                ))
                .count(),
            3
        );
    }

    #[test]
    fn negative_data_auto_range_still_lays_out() {
        let l = lay("xychart-beta\nx-axis [a, b]\nbar [-5, 3]\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        // 自动范围下界取到数据最小值 -5。
        assert!(texts(&l).contains(&"-5"), "{:?}", texts(&l));
    }

    #[test]
    fn smaller_y_max_makes_taller_bar() {
        let lo = lay("xychart-beta\nx-axis [a]\ny-axis 0 --> 100\nbar [50]\n");
        let hi = lay("xychart-beta\nx-axis [a]\ny-axis 0 --> 200\nbar [50]\n");
        assert!(
            first_bar(&lo).height() > first_bar(&hi).height(),
            "同样 50，在 0..100 里应比在 0..200 里更高"
        );
    }

    #[test]
    fn title_is_drawn_as_text() {
        let l = lay("xychart-beta\ntitle \"销售额\"\nx-axis [a]\nbar [1]\n");
        assert!(
            l.prims
                .iter()
                .any(|p| matches!(p, Prim::Text { text, .. } if text == "销售额"))
        );
    }

    #[test]
    fn invalid_bar_values_are_malformed() {
        let f = m();
        assert!(matches!(
            layout("xychart-beta\nx-axis [a]\nbar [x]\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
    }
}
