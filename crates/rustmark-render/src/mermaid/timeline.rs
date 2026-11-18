//! `timeline` 时间线：左侧周期、中央竖轴 + 圆点、右侧事件卡片。
//!
//! 语法：
//!
//! ```text
//! timeline
//!   title 项目历史
//!   2024 : 立项 : 完成调研
//!   2025 : 上线
//!   section 未来
//!   2026 : 扩展
//! ```
//!
//! 竖向排布避免周期 / 事件变多后横向超宽；`section` 画成横向分隔虚线 +
//! 左侧标签。周期或事件缺失时返回 `Malformed`。

use crate::render::{Pt, geom::Rect};

use super::graph_parse::leading_word;
use super::model::Prim;
use super::primitives::{collect_clicks, finalize, rect_corners};
use super::*;

/// 一个周期及其事件。
#[derive(Clone, Debug)]
pub(super) struct TimelineEntry {
    pub(super) period: String,
    pub(super) events: Vec<String>,
}

/// 时间线上的一项：分组标签或周期。
#[derive(Clone, Debug)]
pub(super) enum TimelineNode {
    Section(String),
    Period(TimelineEntry),
}

/// 解析后的时间线。
#[derive(Clone, Debug)]
pub(super) struct TimelineModel {
    pub(super) title: Option<String>,
    pub(super) nodes: Vec<TimelineNode>,
}

/// 解析时间线源码。
pub(super) fn parse_timeline(src: &str) -> Result<TimelineModel, MermaidError> {
    let mut model = TimelineModel {
        title: None,
        nodes: Vec::new(),
    };
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        let lw = leading_word(t);
        if lw == "timeline" {
            continue;
        }
        if lw == "title" {
            let title = t.strip_prefix("title").unwrap_or(t).trim();
            model.title = Some(title.to_string()).filter(|s| !s.is_empty());
            continue;
        }
        if lw == "section" {
            let name = t.strip_prefix("section").unwrap_or(t).trim();
            if name.is_empty() {
                return Err(MermaidError::Malformed(
                    "时间线 section 缺少名称".to_string(),
                ));
            }
            model.nodes.push(TimelineNode::Section(name.to_string()));
            continue;
        }
        // 周期行：`周期 : 事件1 : 事件2 ...`
        let mut parts = t.split(':');
        let period = parts.next().unwrap_or(t).trim();
        let events: Vec<String> = parts
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if period.is_empty() || events.is_empty() {
            return Err(MermaidError::Malformed(format!(
                "时间线周期缺少周期或事件：{t}"
            )));
        }
        model.nodes.push(TimelineNode::Period(TimelineEntry {
            period: period.to_string(),
            events,
        }));
    }
    if !model
        .nodes
        .iter()
        .any(|n| matches!(n, TimelineNode::Period(_)))
    {
        return Err(MermaidError::Malformed("时间线没有任何周期".to_string()));
    }
    Ok(model)
}

/// 一个周期块的高度（事件卡片纵向堆叠）。
fn period_height(events: usize, card_h: f32, card_gap: f32) -> f32 {
    let n = events.max(1) as f32;
    (n * card_h + (n - 1.0) * card_gap).max(card_h)
}

/// 时间线布局 + 发图元。
pub(super) fn layout_timeline(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_timeline(src)?;
    let margin = size;
    let line_w = 1.4 * (size / 16.0).max(0.7);
    let card_h = size * 1.6;
    let card_gap = size * 0.5;
    let row_gap = size * 0.7;
    let title_h = if model.title.is_some() {
        size * 2.0
    } else {
        size * 0.6
    };

    // 宽度：左侧周期列、右侧卡片列（统一卡片宽度，视觉更整齐）。
    let mut period_w = 0.0_f32;
    let mut event_w = 0.0_f32;
    for node in &model.nodes {
        if let TimelineNode::Period(entry) = node {
            period_w = period_w.max(measure(&entry.period, size * 0.9));
            for event in &entry.events {
                event_w = event_w.max(measure(event, size * 0.9));
            }
        }
    }
    let period_col = (period_w + size * 0.8).max(size * 2.6);
    let card_w = (event_w + size * 1.6).max(size * 4.0);
    let x_line = margin + period_col;
    let x_card = x_line + size * 0.9;
    let content_right = x_card + card_w;
    let mut content_w = content_right.max(margin * 2.0);
    if let Some(title) = &model.title {
        content_w = content_w.max(measure(title, size * 1.1) + margin * 2.0);
    }

    let content_top = margin + title_h;
    let mut content_bottom = content_top;
    for node in &model.nodes {
        content_bottom += match node {
            TimelineNode::Section(_) => size * 1.6,
            TimelineNode::Period(entry) => {
                period_height(entry.events.len(), card_h, card_gap) + row_gap
            }
        };
    }

    let mut prims: Vec<Prim> = Vec::new();
    if let Some(title) = &model.title {
        prims.push(Prim::Text {
            at: Pt::new(content_w * 0.5, margin),
            text: title.clone(),
            size: size * 1.1,
            plate: false,
            color: None,
        });
    }
    // 中央竖轴：先发，后续圆点 / 卡片覆盖在其上。
    prims.push(Prim::Line {
        a: Pt::new(x_line, content_top),
        b: Pt::new(x_line, content_bottom),
        width: line_w,
        dotted: false,
    });

    let mut y = content_top;
    let mut period_idx = 0usize;
    for node in &model.nodes {
        match node {
            TimelineNode::Section(name) => {
                let line_y = y + size * 0.6;
                prims.push(Prim::Line {
                    a: Pt::new(margin, line_y),
                    b: Pt::new(content_right, line_y),
                    width: line_w,
                    dotted: true,
                });
                prims.push(Prim::Text {
                    at: Pt::new(margin + period_col * 0.5, line_y - size * 0.55),
                    text: name.clone(),
                    size: size * 0.85,
                    plate: false,
                    color: None,
                });
                y += size * 1.6;
            }
            TimelineNode::Period(entry) => {
                let block_h = period_height(entry.events.len(), card_h, card_gap);
                let dot_y = y + card_h * 0.5;
                let dot_r = size * 0.28;
                prims.push(Prim::Poly {
                    points: rect_corners(Rect::from_center_size(
                        Pt::new(x_line, dot_y),
                        Pt::splat(dot_r * 2.0),
                    )),
                    radius: dot_r,
                    fill: true,
                    ticks: false,
                    fill_color: Some(palette_color(period_idx)),
                    stroke_color: Some(palette_color(period_idx)),
                });
                prims.push(Prim::Text {
                    at: Pt::new(margin + period_col * 0.5, dot_y),
                    text: entry.period.clone(),
                    size: size * 0.9,
                    plate: false,
                    color: None,
                });
                let mut card_y = y;
                for event in &entry.events {
                    let rect =
                        Rect::from_min_size(Pt::new(x_card, card_y), Pt::new(card_w, card_h));
                    prims.push(Prim::Poly {
                        points: rect_corners(rect),
                        radius: size * 0.35,
                        fill: true,
                        ticks: false,
                        fill_color: None,
                        stroke_color: None,
                    });
                    prims.push(Prim::Text {
                        at: rect.center(),
                        text: event.clone(),
                        size: size * 0.9,
                        plate: false,
                        color: None,
                    });
                    card_y += card_h + card_gap;
                }
                period_idx += 1;
                y += block_h + row_gap;
            }
        }
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
        layout_timeline(src, 14.0, &f).expect("应能布局")
    }

    #[test]
    fn parses_title_sections_periods_and_events() {
        let model = parse_timeline(
            "timeline\n\
             title 项目历史\n\
             2024 : 立项 : 完成调研\n\
             2025 : 上线\n\
             section 未来\n\
             2026 : 扩展\n",
        )
        .expect("解析时间线");
        assert_eq!(model.title.as_deref(), Some("项目历史"));
        assert_eq!(model.nodes.len(), 4);
        let sections = model
            .nodes
            .iter()
            .filter(|n| matches!(n, TimelineNode::Section(_)))
            .count();
        assert_eq!(sections, 1);
        let periods: Vec<&TimelineEntry> = model
            .nodes
            .iter()
            .filter_map(|n| match n {
                TimelineNode::Period(entry) => Some(entry),
                TimelineNode::Section(_) => None,
            })
            .collect();
        assert_eq!(periods.len(), 3);
        assert_eq!(periods[0].period, "2024");
        assert_eq!(periods[0].events.len(), 2);
        assert_eq!(periods[1].events, vec!["上线"]);
    }

    #[test]
    fn layout_has_positive_size_and_prims() {
        let l = lay("timeline\n title 历史\n 2024 : 立项 : 调研\n 2025 : 上线\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        assert!(prim_count(&l) > 0);
        // 一个周期一个圆点 + 每事件一张卡片。
        assert!(l.node_count() >= 3);
    }

    #[test]
    fn missing_period_or_event_is_malformed() {
        let f = m();
        for src in [
            "timeline\n 2024\n",
            "timeline\n : 事件\n",
            "timeline\n 2024 :\n",
            "timeline\n",
        ] {
            assert!(
                matches!(
                    layout_timeline(src, 14.0, &f),
                    Err(MermaidError::Malformed(_))
                ),
                "{src} 应报 Malformed"
            );
        }
    }

    #[test]
    fn tab_and_empty_lines_do_not_break_parsing() {
        let model =
            parse_timeline("timeline\n\n  2024 : A\n\n%% 注释\n  2025 : B\n").expect("解析时间线");
        assert_eq!(model.nodes.len(), 2);
    }

    #[test]
    fn one_period_with_multiple_events_draws_all_boxes() {
        let l = lay("timeline\n 2024 : 立项 : 完成调研\n");
        // 1 个周期圆点 + 2 张事件卡片。
        assert!(l.node_count() >= 3, "实际框数 {}", l.node_count());
    }

    #[test]
    fn section_draws_divider_line_and_label() {
        let l = lay("timeline\n 2024 : A\n section 未来\n 2025 : B\n");
        assert!(
            l.prims
                .iter()
                .any(|p| matches!(p, Prim::Line { dotted: true, .. })),
            "section 应画一条虚线分隔"
        );
        assert!(
            l.prims
                .iter()
                .any(|p| matches!(p, Prim::Text { text, .. } if text == "未来")),
            "section 名称应作为文字画出"
        );
    }

    #[test]
    fn period_dot_colors_cycle_through_palette() {
        let l = lay("timeline\n 2024 : A\n 2025 : B\n 2026 : C\n");
        let colors: Vec<_> = l
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
        assert_eq!(colors.len(), 3, "三个周期各一个圆点");
        assert_eq!(colors[0], palette_color(0));
        assert_eq!(colors[1], palette_color(1));
        assert_eq!(colors[2], palette_color(2));
    }

    #[test]
    fn section_without_period_is_malformed() {
        let f = m();
        assert!(matches!(
            layout_timeline("timeline\n section 未来\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
    }

    #[test]
    fn period_text_is_drawn() {
        let l = lay("timeline\n 2024 : A\n");
        assert!(
            l.prims
                .iter()
                .any(|p| matches!(p, Prim::Text { text, .. } if text == "2024")),
            "周期文字应画出"
        );
    }
}
