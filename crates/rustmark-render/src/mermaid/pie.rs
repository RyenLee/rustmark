//! `pie` 饼图：扇形切片 + 图例。
//!
//! 语法：
//!
//! ```text
//! pie showData title 我喜欢的动物
//!   "狗" : 386
//!   "猫" : 85
//! ```
//!
//! 切片用「圆心 + 采样圆弧点」围成的多边形近似；每片的百分比写在扇形内，
//! 右侧图例按固定调色板着色。数值总和必须为正，否则报 `Malformed` 回退源码。

use std::f32::consts::{PI, TAU};

use crate::render::{Pt, Rgb8, geom::Rect};

use super::model::Prim;
use super::primitives::{collect_clicks, finalize, rect_corners};
use super::*;

/// 一个数据项。
#[derive(Clone, Debug)]
pub(super) struct PieSlice {
    pub(super) label: String,
    pub(super) value: f32,
}

/// 解析后的饼图。
#[derive(Clone, Debug)]
pub(super) struct PieModel {
    pub(super) title: Option<String>,
    pub(super) show_data: bool,
    pub(super) slices: Vec<PieSlice>,
}

/// 去掉首尾空白与成对的双引号。
fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim().to_string()
}

/// 解析饼图源码。
pub(super) fn parse_pie(src: &str) -> Result<PieModel, MermaidError> {
    let mut model = PieModel {
        title: None,
        show_data: false,
        slices: Vec::new(),
    };
    let mut seen_header = false;
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        if !seen_header {
            seen_header = true;
            // 头部：`pie` 后可跟 `showData` 与 `title ...`（任意顺序）。
            let mut words = t.split_whitespace();
            let _ = words.next();
            let tail: Vec<&str> = words.collect();
            if tail
                .first()
                .is_some_and(|w| w.eq_ignore_ascii_case("showData"))
            {
                model.show_data = true;
            }
            if let Some(pos) = tail.iter().position(|w| w.eq_ignore_ascii_case("title")) {
                let title = tail[pos + 1..].join(" ");
                model.title = Some(unquote(&title)).filter(|s| !s.is_empty());
            }
            continue;
        }
        // 数据项：`"标签" : 数值`
        let Some((label, val)) = t.split_once(':') else {
            return Err(MermaidError::Malformed(format!("饼图数据项缺少 `:`：{t}")));
        };
        let val = val.trim();
        let value: f32 = val
            .parse()
            .map_err(|_| MermaidError::Malformed(format!("饼图数值不是数字：{val}")))?;
        if !value.is_finite() || value < 0.0 {
            return Err(MermaidError::Malformed(format!("饼图数值必须非负：{val}")));
        }
        model.slices.push(PieSlice {
            label: unquote(label),
            value,
        });
    }
    Ok(model)
}

/// 图例文字。
fn legend_text(s: &PieSlice, show_data: bool, total: f32) -> String {
    let pct = if total > 0.0 {
        s.value / total * 100.0
    } else {
        0.0
    };
    if show_data {
        format!("{}  {}  ({pct:.0}%)", s.label, fmt_num(s.value))
    } else {
        format!("{}  {pct:.0}%", s.label)
    }
}

/// 数值的紧凑写法（整数不带小数点）。
fn fmt_num(v: f32) -> String {
    if (v - v.round()).abs() < 1e-3 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// 饼图布局 + 发图元。
pub(super) fn layout_pie(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_pie(src)?;
    if model.slices.is_empty() {
        return Err(MermaidError::Malformed("饼图没有任何数据项".to_string()));
    }
    let total: f32 = model.slices.iter().map(|s| s.value).sum();
    if total <= 0.0 {
        return Err(MermaidError::Malformed(
            "饼图的数值总和必须大于 0".to_string(),
        ));
    }

    let margin = size * 1.2;
    let radius = size * 3.4;
    let title_h = if model.title.is_some() {
        size * 2.0
    } else {
        0.0
    };
    let center = Pt::new(margin + radius, margin + title_h + radius);

    // 图例文字与宽度。
    let texts: Vec<String> = model
        .slices
        .iter()
        .map(|s| legend_text(s, model.show_data, total))
        .collect();
    let mut legend_w = 0.0_f32;
    for t in &texts {
        legend_w = legend_w.max(measure(t, size * 0.9));
    }
    let swatch = size * 0.9;
    let legend_x = center.x + radius + size * 1.5;

    let mut prims: Vec<Prim> = Vec::new();
    if let Some(title) = &model.title {
        let right = legend_x + swatch + size * 0.6 + legend_w;
        prims.push(Prim::Text {
            at: Pt::new((margin + right) * 0.5, margin + size * 0.5),
            text: title.clone(),
            size: size * 1.1,
            plate: false,
            color: None,
        });
    }

    // 切片：从正上方（-90°）顺时针铺开。
    let white = Rgb8::opaque(255, 255, 255);
    let single = model.slices.len() == 1;
    let mut angle = -PI * 0.5;
    for (i, s) in model.slices.iter().enumerate() {
        let sweep = TAU * s.value / total;
        if single {
            // 一整圆：用「圆角=半宽」的矩形画正圆（多边形采样会有接缝）。
            prims.push(Prim::Poly {
                points: rect_corners(Rect::from_center_size(center, Pt::splat(radius * 2.0))),
                radius,
                fill: true,
                ticks: false,
                fill_color: Some(palette_color(i)),
                stroke_color: None,
            });
            continue;
        }
        if sweep <= 1e-4 {
            continue;
        }
        let steps = ((sweep / (PI / 24.0)).ceil() as usize).max(2);
        let mut points = Vec::with_capacity(steps + 2);
        points.push(center);
        for k in 0..=steps {
            let a = angle + sweep * k as f32 / steps as f32;
            points.push(Pt::new(
                center.x + radius * a.cos(),
                center.y + radius * a.sin(),
            ));
        }
        prims.push(Prim::Poly {
            points,
            radius: 0.0,
            fill: true,
            ticks: false,
            fill_color: Some(palette_color(i)),
            stroke_color: None,
        });
        // 百分比写在扇形里；太窄的片放不下就不写。
        if sweep > 0.5 {
            let mid = angle + sweep * 0.5;
            prims.push(Prim::Text {
                at: Pt::new(
                    center.x + radius * 0.62 * mid.cos(),
                    center.y + radius * 0.62 * mid.sin(),
                ),
                text: format!("{:.0}%", s.value / total * 100.0),
                size: size * 0.85,
                plate: false,
                color: Some(white),
            });
        }
        angle += sweep;
    }

    // 右侧图例。
    let row_h = size * 1.7;
    let top = center.y - texts.len() as f32 * row_h * 0.5;
    for (i, text) in texts.iter().enumerate() {
        let y = top + row_h * (i as f32 + 0.5);
        let swatch_rect =
            Rect::from_center_size(Pt::new(legend_x + swatch * 0.5, y), Pt::splat(swatch));
        prims.push(Prim::Poly {
            points: rect_corners(swatch_rect),
            radius: 2.0,
            fill: true,
            ticks: false,
            fill_color: Some(palette_color(i)),
            stroke_color: None,
        });
        let text_w = measure(text, size * 0.9);
        prims.push(Prim::Text {
            at: Pt::new(legend_x + swatch + size * 0.6 + text_w * 0.5, y),
            text: text.clone(),
            size: size * 0.9,
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
    fn parses_title_show_data_and_slices() {
        let model = parse_pie("pie showData title 我喜欢的动物\n\"狗\" : 386\n\"猫\" : 85\n")
            .expect("解析饼图");
        assert_eq!(model.title.as_deref(), Some("我喜欢的动物"));
        assert!(model.show_data);
        assert_eq!(model.slices.len(), 2);
        assert_eq!(model.slices[0].label, "狗");
        assert!((model.slices[0].value - 386.0).abs() < 1e-3);
    }

    #[test]
    fn layout_has_positive_size_and_prims() {
        let l = lay("pie title 占比\n\"A\" : 40\n\"B\" : 60\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        assert!(prim_count(&l) > 0);
        // 两个切片 + 两个图例色块 + 两段文字 + 标题。
        assert!(l.prims.len() >= 8);
    }

    #[test]
    fn single_slice_draws_a_full_circle() {
        let l = lay("pie title 全部\n\"唯一\" : 10\n");
        assert!(l.width > 0.0 && l.height > 0.0);
    }

    #[test]
    fn non_numeric_or_zero_total_is_malformed() {
        let f = m();
        assert!(matches!(
            layout("pie\n\"A\" : 很多\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
        assert!(matches!(
            layout("pie\n\"A\" : 0\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
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
    fn legend_shows_percentages() {
        let l = lay("pie\n\"A\" : 40\n\"B\" : 60\n");
        let all = texts(&l);
        assert!(all.iter().any(|t| t.contains("40%")), "{all:?}");
        assert!(all.iter().any(|t| t.contains("60%")), "{all:?}");
    }

    #[test]
    fn show_data_puts_values_in_legend() {
        let l = lay("pie showData\n\"A\" : 40\n\"B\" : 60\n");
        let all = texts(&l);
        // 图例形如 `A  40  (40%)`，同时含具体数值与百分比。
        assert!(
            all.iter()
                .any(|t| t.starts_with('A') && t.contains("40  ") && t.contains("(40%)")),
            "{all:?}"
        );
        assert!(
            all.iter()
                .any(|t| t.starts_with('B') && t.contains("60  ") && t.contains("(60%)")),
            "{all:?}"
        );
    }

    #[test]
    fn slices_use_consecutive_palette_colors() {
        let l = lay("pie\n\"A\" : 40\n\"B\" : 60\n");
        let fills: Vec<Rgb8> = l
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
        // 先两片扇形，后两个图例色块。
        assert!(fills.len() >= 2, "{fills:?}");
        assert_eq!(fills[0], palette_color(0));
        assert_eq!(fills[1], palette_color(1));
        assert_ne!(fills[0], fills[1]);
    }

    #[test]
    fn negative_or_non_finite_value_is_malformed() {
        let f = m();
        assert!(matches!(
            layout("pie\n\"A\" : -5\n\"B\" : 10\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
        assert!(matches!(
            layout("pie\n\"A\" : NaN\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
    }

    #[test]
    fn labels_keep_spaces_and_chinese() {
        let model = parse_pie("pie\n\"每周 任务\" : 3\n\"Build Server\" : 7\n").expect("解析饼图");
        assert_eq!(model.slices[0].label, "每周 任务");
        assert_eq!(model.slices[1].label, "Build Server");
    }

    #[test]
    fn layout_without_title_works() {
        let model = parse_pie("pie\n\"A\" : 1\n\"B\" : 2\n").expect("解析饼图");
        assert!(model.title.is_none());
        let l = lay("pie\n\"A\" : 1\n\"B\" : 2\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        // 无标题时只剩扇形内百分比（2 段）+ 图例行（2 段）。
        assert_eq!(texts(&l).len(), 4, "{:?}", texts(&l));
    }
}
