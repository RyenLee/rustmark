//! `gantt` 甘特图的解析与布局。

use egui::{Pos2, Rect, Vec2};

use super::graph_parse::leading_word;
use super::model::Prim;
use super::primitives::{
    collect_clicks, finalize, first_md_link, push_check, push_rect_outline, rect_corners,
};
use super::*;

/// 一个任务。
#[derive(Clone, Debug)]
pub(super) struct GanttTask {
    pub(super) id: Option<String>,
    pub(super) name: String,
    pub(super) dur: f32,
    pub(super) done: bool,
    pub(super) crit: bool,
    pub(super) dep: Option<String>,
    pub(super) section: usize,
}

/// 解析后的甘特图。
#[derive(Clone, Debug)]
pub(super) struct GanttModel {
    pub(super) title: Option<String>,
    pub(super) sections: Vec<String>,
    pub(super) tasks: Vec<GanttTask>,
}

/// 识别「天数/周数」形式的时长令牌。
fn is_duration(tok: &str) -> Option<f32> {
    let t = tok.trim_end_matches(['d', 'w', 'D', 'W']);
    let n: f32 = t.parse().ok()?;
    let unit = tok.chars().last()?;
    let mult = match unit {
        'w' | 'W' => 7.0,
        _ => 1.0,
    };
    Some(n * mult)
}

/// 识别 `YYYY-MM-DD` 形式的日期令牌（仅用于跳过，不解析日历）。
fn is_date(tok: &str) -> bool {
    let b: Vec<&str> = tok.split('-').collect();
    b.len() == 3
        && b[0].len() == 4
        && b[1].len() == 2
        && b[2].len() == 2
        && b.iter().all(|s| s.chars().all(|c| c.is_ascii_digit()))
}

/// 解析甘特图源码。
pub(super) fn parse_gantt(src: &str) -> Result<GanttModel, MermaidError> {
    let mut title: Option<String> = None;
    let mut sections: Vec<String> = Vec::new();
    let mut tasks: Vec<GanttTask> = Vec::new();
    let mut cur_section: usize = 0;
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        let lw = leading_word(t);
        if lw == "gantt" || lw == "click" {
            continue;
        }
        if lw == "title" {
            let rest = t.strip_prefix("title").unwrap_or(t);
            title = Some(rest.trim().to_string());
            continue;
        }
        if matches!(
            lw.as_str(),
            "dateFormat" | "axisFormat" | "todayMarker" | "excludes"
        ) {
            continue;
        }
        if lw == "section" {
            let rest = t.strip_prefix("section").unwrap_or(t);
            sections.push(rest.trim().to_string());
            cur_section = sections.len() - 1;
            continue;
        }
        // 任务行：`Name : spec`
        if let Some(colon) = t.find(':') {
            let name = t[..colon].trim().to_string();
            let spec = t[colon + 1..].trim();
            let toks: Vec<&str> = spec
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let mut id: Option<String> = None;
            let mut dur = 0.0_f32;
            let mut done = false;
            let mut crit = false;
            let mut dep: Option<String> = None;
            for tok in toks {
                if tok == "done" {
                    done = true;
                } else if tok == "crit" {
                    crit = true;
                } else if let Some(rest) = tok.strip_prefix("after ") {
                    dep = Some(rest.trim().to_string());
                } else if is_date(tok) {
                    // 显式日期忽略，保留 id / 时长。
                } else if let Some(d) = is_duration(tok) {
                    dur = d;
                } else if id.is_none() {
                    id = Some(tok.to_string());
                }
            }
            if dur <= 0.0 {
                dur = 1.0;
            }
            tasks.push(GanttTask {
                id,
                name,
                dur,
                done,
                crit,
                dep,
                section: cur_section,
            });
        }
    }
    Ok(GanttModel {
        title,
        sections,
        tasks,
    })
}

/// 甘特图布局 + 发图元。
pub(super) fn layout_gantt(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_gantt(src)?;
    let day_w = size * 0.6;
    let label_w = if model.tasks.is_empty() {
        size * 8.0
    } else {
        let mut mw = size * 6.0;
        for t in &model.tasks {
            let w = measure(&t.name, size);
            if w > mw {
                mw = w;
            }
        }
        for s in &model.sections {
            let w = measure(s, size);
            if w > mw {
                mw = w;
            }
        }
        mw + size * 1.0
    };
    let margin = size;
    let top = if model.title.is_some() {
        size * 2.5
    } else {
        size
    };
    let row_h = size * 1.8;
    let bar_h = size * 1.0;

    let mut end_by_id: Vec<(String, f32)> = Vec::new();
    let mut starts: Vec<f32> = Vec::new();
    let mut ends: Vec<f32> = Vec::new();
    let mut section_cursor: Vec<f32> = vec![0.0; model.sections.len().max(1)];
    let mut total_days = 0.0_f32;
    for t in &model.tasks {
        let sec = t.section.min(section_cursor.len() - 1);
        let start = match &t.dep {
            Some(d) => end_by_id
                .iter()
                .find(|(i, _)| i == d)
                .map(|(_, e)| *e)
                .unwrap_or(0.0),
            None => section_cursor[sec],
        };
        let end = start + t.dur;
        starts.push(start);
        ends.push(end);
        if let Some(id) = &t.id {
            if let Some(pos) = end_by_id.iter().position(|(i, _)| i == id) {
                end_by_id[pos].1 = end;
            } else {
                end_by_id.push((id.clone(), end));
            }
        }
        section_cursor[sec] = section_cursor[sec].max(end);
        if end > total_days {
            total_days = end;
        }
    }
    if total_days <= 0.0 {
        total_days = 1.0;
    }

    let mut prims: Vec<Prim> = Vec::new();
    let mut id_rects: Vec<(String, Rect)> = Vec::new();
    let mut direct: Vec<(Rect, String)> = Vec::new();
    let clicks = collect_clicks(src);
    let line_w = 1.0 * (size / 16.0).max(0.7);

    // 标题。
    if let Some(title) = &model.title {
        prims.push(Prim::Text {
            at: Pos2::new(margin, size * 1.0),
            text: title.clone(),
            size: size * 1.1,
            plate: false,
        });
    }
    // 时间轴基线。
    let axis_y = top - size * 0.2;
    prims.push(Prim::Line {
        a: Pos2::new(label_w + margin, axis_y),
        b: Pos2::new(label_w + margin + total_days * day_w, axis_y),
        width: line_w,
        dotted: false,
    });

    // 任务行。
    let mut y = top;
    let mut last_section: Option<usize> = None;
    for (i, t) in model.tasks.iter().enumerate() {
        if last_section != Some(t.section) {
            prims.push(Prim::Line {
                a: Pos2::new(margin * 0.5, y),
                b: Pos2::new(label_w + margin + total_days * day_w + margin, y),
                width: line_w,
                dotted: true,
            });
            if let Some(s) = model.sections.get(t.section) {
                prims.push(Prim::Text {
                    at: Pos2::new(margin * 0.5, y - size * 0.2),
                    text: s.clone(),
                    size: size * 0.9,
                    plate: false,
                });
            }
            last_section = Some(t.section);
        }
        let x0 = label_w + margin + starts[i] * day_w;
        let w = (ends[i] - starts[i]).max(0.5) * day_w;
        let bar = Rect::from_min_size(
            Pos2::new(x0, y + (row_h - bar_h) * 0.5),
            Vec2::new(w, bar_h),
        );
        prims.push(Prim::Poly {
            points: rect_corners(bar),
            radius: 2.0,
            fill: true,
            ticks: false,
        });
        // 任务名（左侧）。
        prims.push(Prim::Text {
            at: Pos2::new(margin, y + row_h * 0.5),
            text: t.name.clone(),
            size: size * 0.85,
            plate: false,
        });
        // crit：描边；done：画对勾，区分两类任务。
        if t.crit {
            push_rect_outline(&mut prims, bar, line_w * 1.6);
        }
        if t.done {
            push_check(&mut prims, bar, line_w * 1.4);
        }
        if let Some(id) = &t.id {
            id_rects.push((id.clone(), bar));
        }
        if let Some(url) = first_md_link(&t.name) {
            direct.push((bar, url));
        }
        y += row_h;
    }

    Ok(finalize(prims, id_rects, clicks, direct))
}
