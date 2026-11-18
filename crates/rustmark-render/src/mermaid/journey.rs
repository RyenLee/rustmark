//! `journey` 用户旅程图：按 `section` 分行、按分数纵向定位的任务卡片。
//!
//! 语法：
//!
//! ```text
//! journey
//!   title 我的上班旅程
//!   section 出门
//!     起床: 3: 我
//!     通勤: 2: 我, 同事
//!   section 工作
//!     写代码: 5: 我
//! ```
//!
//! 分数（0–5）越高，任务卡片在同一条 `section` 带内越靠上；颜色随分数
//! 从红（1）过渡到绿（5）。解析失败（缺分数、分数越界）返回 `Malformed`，
//! 由调用方回退到源码显示，而不是画半张图。

use crate::render::{Pt, Rgb8, geom::Rect};

use super::graph_parse::leading_word;
use super::model::Prim;
use super::primitives::{collect_clicks, finalize, rect_corners};
use super::*;

/// 任务框的左右内边距（乘基准字号）。
const JOURNEY_PAD_X: f32 = 0.8;
/// 任务框高度（乘基准字号）。
const JOURNEY_BOX_H: f32 = 2.4;
/// 同一 section 内分数 0 与 5 的纵向落差（乘基准字号）。
const JOURNEY_BAND_SPAN: f32 = 2.4;
/// 相邻任务之间的水平间距（乘基准字号）。
const JOURNEY_TASK_GAP: f32 = 1.4;

/// 一个任务。
#[derive(Clone, Debug)]
pub(super) struct JourneyTask {
    pub(super) name: String,
    pub(super) score: f32,
    pub(super) actors: Vec<String>,
}

/// 一个分区（`section`）。
#[derive(Clone, Debug)]
pub(super) struct JourneySection {
    pub(super) name: String,
    pub(super) tasks: Vec<JourneyTask>,
}

/// 解析后的旅程图。
#[derive(Clone, Debug)]
pub(super) struct JourneyModel {
    pub(super) title: Option<String>,
    pub(super) sections: Vec<JourneySection>,
}

/// 解析旅程图源码。
pub(super) fn parse_journey(src: &str) -> Result<JourneyModel, MermaidError> {
    let mut model = JourneyModel {
        title: None,
        sections: Vec::new(),
    };
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        let lw = leading_word(t);
        if lw == "journey" {
            continue;
        }
        if lw == "title" {
            let title = t.strip_prefix("title").unwrap_or(t).trim();
            model.title = Some(title.to_string()).filter(|s| !s.is_empty());
            continue;
        }
        if lw == "section" {
            let name = t.strip_prefix("section").unwrap_or(t).trim();
            model.sections.push(JourneySection {
                name: name.to_string(),
                tasks: Vec::new(),
            });
            continue;
        }
        // 任务行：`任务名: 分数[: 参与者, ...]`
        let mut parts = t.split(':');
        let name = parts.next().unwrap_or(t).trim();
        if name.is_empty() {
            return Err(MermaidError::Malformed(format!("旅程任务缺少名称：{t}")));
        }
        let Some(score_text) = parts.next() else {
            return Err(MermaidError::Malformed(format!("旅程任务缺少分数：{t}")));
        };
        let score: f32 = score_text.trim().parse().map_err(|_| {
            MermaidError::Malformed(format!("旅程任务分数不是数字：{}", score_text.trim()))
        })?;
        if !score.is_finite() || !(0.0..=5.0).contains(&score) {
            return Err(MermaidError::Malformed(format!(
                "旅程任务分数必须在 0–5 之间：{}",
                score_text.trim()
            )));
        }
        let actors: Vec<String> = parts
            .next()
            .map(|a| {
                a.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if model.sections.is_empty() {
            model.sections.push(JourneySection {
                name: String::new(),
                tasks: Vec::new(),
            });
        }
        if let Some(section) = model.sections.last_mut() {
            section.tasks.push(JourneyTask {
                name: name.to_string(),
                score,
                actors,
            });
        }
    }
    if model.sections.iter().all(|s| s.tasks.is_empty()) {
        return Err(MermaidError::Malformed("旅程图没有任何任务".to_string()));
    }
    Ok(model)
}

/// 分数 →（填充色, 文字色）。1 红、2 橙、3 黄、4 浅绿、5 绿。
fn score_style(score: f32) -> (Rgb8, Rgb8) {
    let level = score.round().clamp(0.0, 5.0) as u8;
    match level {
        0 | 1 => (Rgb8::opaque(0xd6, 0x3a, 0x3a), Rgb8::opaque(255, 255, 255)),
        2 => (Rgb8::opaque(0xf2, 0x8e, 0x2b), Rgb8::opaque(255, 255, 255)),
        3 => (
            Rgb8::opaque(0xed, 0xc9, 0x48),
            Rgb8::opaque(0x3a, 0x32, 0x14),
        ),
        4 => (
            Rgb8::opaque(0x9b, 0xc9, 0x6e),
            Rgb8::opaque(0x24, 0x3a, 0x14),
        ),
        _ => (Rgb8::opaque(0x59, 0xa1, 0x4f), Rgb8::opaque(255, 255, 255)),
    }
}

/// 分数的紧凑写法（整数不带小数点）。
fn fmt_score(score: f32) -> String {
    if (score - score.round()).abs() < 1e-3 {
        format!("{}", score.round() as i64)
    } else {
        format!("{score:.1}")
    }
}

/// 旅程图布局 + 发图元。
pub(super) fn layout_journey(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_journey(src)?;
    let margin = size;
    let line_w = 1.4 * (size / 16.0).max(0.7);
    let title_h = if model.title.is_some() {
        size * 2.0
    } else {
        size * 0.6
    };

    // 左侧 section 名称列宽（没有名字时不占位）。
    let mut label_w = 0.0_f32;
    for section in &model.sections {
        if !section.name.is_empty() {
            label_w = label_w.max(measure(&section.name, size * 0.9));
        }
    }
    if label_w > 0.0 {
        label_w += size * 1.2;
    }
    let divider_x = margin + label_w + size * 0.4;
    let tasks_start = if label_w > 0.0 {
        divider_x + size * 0.6
    } else {
        margin
    };

    // 每个任务框的宽度（第一遍量好，标题 / 总宽都要用）。
    let mut widths: Vec<Vec<f32>> = Vec::with_capacity(model.sections.len());
    for section in &model.sections {
        let row: Vec<f32> = section
            .tasks
            .iter()
            .map(|task| {
                (measure(&task.name, size * 0.9) + size * JOURNEY_PAD_X * 2.0).max(size * 3.0)
            })
            .collect();
        widths.push(row);
    }

    let mut content_w = margin;
    for (si, section) in model.sections.iter().enumerate() {
        if section.tasks.is_empty() {
            continue;
        }
        let mut row_w = 0.0_f32;
        for w in &widths[si] {
            row_w += *w + size * JOURNEY_TASK_GAP;
        }
        row_w -= size * JOURNEY_TASK_GAP;
        content_w = content_w.max(tasks_start + row_w);
    }
    if let Some(title) = &model.title {
        content_w = content_w.max(measure(title, size * 1.1) + margin * 2.0);
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

    let box_h = size * JOURNEY_BOX_H;
    let band_span = size * JOURNEY_BAND_SPAN;
    let band_h = band_span + box_h + size * 1.1;
    let mut y = margin + title_h;
    for (si, section) in model.sections.iter().enumerate() {
        // 每个 section 顶部一条横向虚线。
        prims.push(Prim::Line {
            a: Pt::new(margin, y),
            b: Pt::new(content_w, y),
            width: line_w,
            dotted: true,
        });
        if !section.name.is_empty() {
            prims.push(Prim::Text {
                at: Pt::new(margin + label_w * 0.5, y + band_h * 0.5),
                text: section.name.clone(),
                size: size * 0.9,
                plate: false,
                color: None,
            });
            // section 名与任务区之间的短竖线。
            prims.push(Prim::Line {
                a: Pt::new(divider_x, y + size * 0.3),
                b: Pt::new(divider_x, y + band_h - size * 0.3),
                width: line_w,
                dotted: false,
            });
        }
        let mut x = tasks_start;
        let mut prev: Option<Rect> = None;
        for (ti, task) in section.tasks.iter().enumerate() {
            let w = widths[si][ti];
            // 分数越高越靠上：5 分在 section 带顶部，0 分在底部。
            let top = y + (5.0 - task.score) / 5.0 * band_span;
            let rect = Rect::from_min_size(Pt::new(x, top), Pt::new(w, box_h));
            let (fill, text_color) = score_style(task.score);
            prims.push(Prim::Poly {
                points: rect_corners(rect),
                radius: size * 0.4,
                fill: true,
                ticks: false,
                fill_color: Some(fill),
                stroke_color: None,
            });
            // 分数在框内上半，任务名在下半。
            prims.push(Prim::Text {
                at: Pt::new(rect.center().x, rect.center().y - size * 0.5),
                text: fmt_score(task.score),
                size: size * 1.0,
                plate: false,
                color: Some(text_color),
            });
            prims.push(Prim::Text {
                at: Pt::new(rect.center().x, rect.center().y + size * 0.55),
                text: task.name.clone(),
                size: size * 0.8,
                plate: false,
                color: Some(text_color),
            });
            if !task.actors.is_empty() {
                prims.push(Prim::Text {
                    at: Pt::new(rect.center().x, rect.bottom() + size * 0.5),
                    text: task.actors.join(", "),
                    size: size * 0.7,
                    plate: false,
                    color: None,
                });
            }
            if let Some(prev) = prev {
                prims.push(Prim::Line {
                    a: Pt::new(prev.right(), prev.center().y),
                    b: Pt::new(rect.left(), rect.center().y),
                    width: line_w,
                    dotted: false,
                });
            }
            prev = Some(rect);
            x += w + size * JOURNEY_TASK_GAP;
        }
        y += band_h + size * 0.9;
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
        layout_journey(src, 14.0, &f).expect("应能布局")
    }

    #[test]
    fn parses_title_sections_and_scores() {
        let model = parse_journey(
            "journey\n\
             title 我的上班旅程\n\
             section 出门\n\
             起床: 3: 我\n\
             通勤: 2: 我, 同事\n\
             section 工作\n\
             写代码: 5\n",
        )
        .expect("解析旅程图");
        assert_eq!(model.title.as_deref(), Some("我的上班旅程"));
        assert_eq!(model.sections.len(), 2);
        assert_eq!(model.sections[0].name, "出门");
        assert_eq!(model.sections[0].tasks.len(), 2);
        assert!((model.sections[0].tasks[0].score - 3.0).abs() < 1e-3);
        assert_eq!(model.sections[0].tasks[1].actors, vec!["我", "同事"]);
        assert_eq!(model.sections[1].tasks.len(), 1);
        assert!((model.sections[1].tasks[0].score - 5.0).abs() < 1e-3);
    }

    #[test]
    fn layout_has_positive_size_and_prims() {
        let l = lay("journey\n title 旅程\n section A\n 起床: 3: 我\n 通勤: 2\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        assert!(prim_count(&l) > 0);
        assert!(l.node_count() >= 2);
    }

    #[test]
    fn invalid_score_is_malformed() {
        let f = m();
        for src in [
            "journey\n section A\n 起床: 很多\n",
            "journey\n section A\n 起床: 9\n",
            "journey\n 起床\n",
            "journey\n",
        ] {
            assert!(
                matches!(
                    layout_journey(src, 14.0, &f),
                    Err(MermaidError::Malformed(_))
                ),
                "{src} 应报 Malformed"
            );
        }
    }

    #[test]
    fn tasks_without_section_use_implicit_group() {
        let model = parse_journey("journey\n 起床: 3\n 通勤: 2\n").expect("解析旅程图");
        assert_eq!(model.sections.len(), 1, "无 section 时应有一个隐式分组");
        assert!(model.sections[0].name.is_empty());
        assert_eq!(model.sections[0].tasks.len(), 2);
    }

    #[test]
    fn higher_score_is_placed_above_lower_score() {
        let l = lay("journey\n section A\n 低分: 0\n 高分: 5\n");
        let high_fill = score_style(5.0).0;
        let low_fill = score_style(0.0).0;
        let mut high_y = None;
        let mut low_y = None;
        for p in &l.prims {
            if let Prim::Poly {
                points,
                fill: true,
                fill_color: Some(color),
                ..
            } = p
            {
                let y = Rect::from_points(points).center().y;
                if *color == high_fill {
                    high_y = Some(y);
                } else if *color == low_fill {
                    low_y = Some(y);
                }
            }
        }
        let high_y = high_y.expect("应画出 5 分任务框");
        let low_y = low_y.expect("应画出 0 分任务框");
        assert!(high_y < low_y, "5 分应在 0 分上方：{high_y} < {low_y}");
    }

    #[test]
    fn actors_are_drawn_as_text() {
        let l = lay("journey\n section A\n 通勤: 2: 我, 同事\n");
        assert!(
            l.prims
                .iter()
                .any(|p| matches!(p, Prim::Text { text, .. } if text == "我, 同事")),
            "参与者应拼成一段文字画出"
        );
    }

    #[test]
    fn lays_out_without_title() {
        let l = lay("journey\n section A\n 起床: 3\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        assert_eq!(l.node_count(), 1, "一个任务对应一个框");
    }

    #[test]
    fn empty_journey_is_malformed() {
        let f = m();
        assert!(matches!(
            layout_journey("journey\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
    }
}
