//! `gantt` 甘特图的解析与布局（对齐 mermaid.js 官方渲染）。
//!
//! 与旧版相比的关键升级：
//! - 解析真实日期（`dateFormat` 默认 `YYYY-MM-DD`）并转成「自项目起点第几天」，
//!   显式日期的任务按其日历位置排布，而非从 0 顺序累加；
//! - 标题居中；任务名画在条内白字；
//! - 底部日期轴 + 自动刻度（日 / 周 / 月 / 年）+ 竖向网格线；
//! - 按状态着色（done 灰 / active 蓝 / crit 红 / 默认深灰）+ 浅描边；
//! - 域右端留白，最后一条任务不会贴着右边界。

use std::collections::HashMap;

use egui::{Color32, Pos2, Rect, Vec2};

use super::graph_parse::leading_word;
use super::model::Prim;
use super::primitives::{collect_clicks, finalize, first_md_link, push_check, rect_corners};
use super::*;

/// 任务状态（决定条的颜色）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GanttStatus {
    Done,
    Active,
    Crit,
    Default,
}

/// 一个任务。
#[derive(Clone, Debug)]
pub(super) struct GanttTask {
    pub(super) id: Option<String>,
    pub(super) name: String,
    pub(super) status: GanttStatus,
    pub(super) dep: Option<String>,
    /// 显式开始日期（自纪元天数），无则 `None`。
    pub(super) start_date: Option<i64>,
    /// 显式结束日期（自纪元天数），无则 `None`。
    pub(super) end_date: Option<i64>,
    /// 显式时长（天），无则 `None`（布局时回落 1 天）。
    pub(super) dur: Option<f32>,
    pub(super) section: usize,
}

/// 解析后的甘特图。
#[derive(Clone, Debug)]
pub(super) struct GanttModel {
    pub(super) title: Option<String>,
    pub(super) sections: Vec<String>,
    pub(super) tasks: Vec<GanttTask>,
}

/// 把 `(年, 月, 日)` 转成自纪元（1970-01-01）的天数（Howard Hinnant 算法，
/// 与 chrono 的 `NaiveDate::from_ymd` 等价，但零依赖）。
fn civil_to_days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// 天数转回 `(年, 月, 日)`。
fn days_to_civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// 把一个 token 解析为日期序数（支持任意非数字分隔符：`2024-01-01` /
/// `2024/01/01` / `2024.01.01` 都行）。无法识别返回 `None`。
fn parse_ymd(tok: &str) -> Option<i64> {
    let nums: Option<Vec<i64>> = tok
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().ok())
        .collect();
    let nums = nums?;
    if nums.len() != 3 {
        return None;
    }
    let (y, m, d) = (nums[0], nums[1], nums[2]);
    // 宽松边界校验：月份 / 日期越界直接视为不是日期（避免误吞正常 token）。
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(1..=9999).contains(&y) {
        return None;
    }
    Some(civil_to_days(y, m, d))
}

/// 识别「天数 / 周数」形式的时长令牌（如 `5d` / `2w`）。
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
            title = Some(t.strip_prefix("title").unwrap_or(t).trim().to_string());
            continue;
        }
        // 纯描述性指令：忽略而非报错（同其它图种的 `style` / `classDef` 处理）。
        if matches!(
            lw.as_str(),
            "dateFormat" | "axisFormat" | "todayMarker" | "excludes"
        ) {
            continue;
        }
        if lw == "section" {
            sections.push(t.strip_prefix("section").unwrap_or(t).trim().to_string());
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
            let mut status = GanttStatus::Default;
            let mut dep: Option<String> = None;
            let mut start_date: Option<i64> = None;
            let mut end_date: Option<i64> = None;
            let mut dur: Option<f32> = None;
            for tok in toks {
                if tok == "done" {
                    status = GanttStatus::Done;
                } else if tok == "active" {
                    status = GanttStatus::Active;
                } else if tok == "crit" {
                    status = GanttStatus::Crit;
                } else if let Some(rest) = tok.strip_prefix("after ") {
                    dep = Some(rest.trim().to_string());
                } else if let Some(d) = parse_ymd(tok) {
                    // 第一个日期是开始、第二个是结束（mermaid 约定）。
                    if start_date.is_none() {
                        start_date = Some(d);
                    } else if end_date.is_none() {
                        end_date = Some(d);
                    }
                } else if let Some(d) = is_duration(tok) {
                    dur = Some(d);
                } else if id.is_none() {
                    id = Some(tok.to_string());
                }
            }
            tasks.push(GanttTask {
                id,
                name,
                status,
                dep,
                start_date,
                end_date,
                dur,
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

/// 任务状态 →（填充色, 描边色）。描边取填充色向白混合 45%，即「浅描边」。
fn status_colors(s: GanttStatus) -> (Color32, Color32) {
    let fill = match s {
        GanttStatus::Done => Color32::from_rgb(0x80, 0x80, 0x80),
        GanttStatus::Active => Color32::from_rgb(0x1f, 0x6f, 0xd6),
        GanttStatus::Crit => Color32::from_rgb(0xd6, 0x3a, 0x3a),
        GanttStatus::Default => Color32::from_rgb(0x55, 0x55, 0x55),
    };
    (fill, mix_white(fill, 0.45))
}

/// 把颜色向白色混合 `t`（0=原色，1=白）。
fn mix_white(c: Color32, t: f32) -> Color32 {
    let r = (c.r() as f32 + (255.0 - c.r() as f32) * t) as u8;
    let g = (c.g() as f32 + (255.0 - c.g() as f32) * t) as u8;
    let b = (c.b() as f32 + (255.0 - c.b() as f32) * t) as u8;
    Color32::from_rgb(r, g, b)
}

/// 把天数序号格式化成日期轴标签（按刻度步长决定精度）。
fn date_label(ord: i64, step: f32) -> String {
    let (y, m, d) = days_to_civil(ord);
    if step >= 365.0 {
        format!("{y}")
    } else if step >= 30.0 {
        format!("{y}-{m:02}")
    } else {
        format!("{y}-{m:02}-{d:02}")
    }
}

/// 甘特图布局 + 发图元。
pub(super) fn layout_gantt(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_gantt(src)?;
    let day_w = size * 0.62;

    // 左侧标签列宽度：取各 section 名宽度 + 余量，并设最小值。
    let mut label_w = size * 4.0;
    for s in &model.sections {
        let w = measure(s, size * 0.9);
        if w + size * 1.0 > label_w {
            label_w = w + size * 1.0;
        }
    }
    let margin = size;
    let top_pad = if model.title.is_some() {
        size * 2.2
    } else {
        size
    };

    // 日期基准：所有显式日期的最小天序号（没有显式日期则相对 0 起算）。
    let mut base: Option<i64> = None;
    for t in &model.tasks {
        if let Some(s) = t.start_date {
            base = Some(base.map_or(s, |b| b.min(s)));
        }
        if let Some(e) = t.end_date {
            base = Some(base.map_or(e, |b| b.min(e)));
        }
    }
    let base = base.unwrap_or(0);

    // 计算每个任务的起止（天序号，相对 base）。
    let mut end_by_id: HashMap<String, f32> = HashMap::new();
    let mut section_cursor = vec![0.0_f32; model.sections.len().max(1)];
    let mut starts: Vec<f32> = Vec::with_capacity(model.tasks.len());
    let mut ends: Vec<f32> = Vec::with_capacity(model.tasks.len());
    let mut total_days = 0.0_f32;
    for t in &model.tasks {
        let sec = t.section.min(section_cursor.len() - 1);
        let start = if let Some(s) = t.start_date {
            (s - base) as f32
        } else if let Some(d) = &t.dep {
            end_by_id.get(d).copied().unwrap_or(section_cursor[sec])
        } else {
            section_cursor[sec]
        };
        let end = if let Some(e) = t.end_date {
            (e - base) as f32
        } else {
            start + t.dur.unwrap_or(1.0)
        };
        let end = end.max(start + 0.5);
        starts.push(start);
        ends.push(end);
        if let Some(id) = &t.id {
            end_by_id.insert(id.clone(), end);
        }
        section_cursor[sec] = section_cursor[sec].max(end);
        total_days = total_days.max(end);
    }
    if total_days <= 0.0 {
        total_days = 1.0;
    }

    // 自动刻度步长：按跨度在 日 / 周 / 月 / 年 间切换。
    let step = if total_days <= 14.0 {
        1.0
    } else if total_days <= 70.0 {
        7.0
    } else if total_days <= 365.0 {
        30.0
    } else {
        365.0
    };
    // 域右端留白：延伸到下一个整刻度之外再空一格。
    let domain_days = (total_days / step).ceil() * step + step;

    let chart_left = label_w + margin;
    let chart_right = chart_left + domain_days * day_w;

    let row_h = size * 1.8;
    let bar_h = size * 1.0;
    let line_w = 1.0 * (size / 16.0).max(0.7);
    // 没有任何显式日期时，轴标签只标「第几天」而非伪造日期。
    let no_dates = model
        .tasks
        .iter()
        .all(|t| t.start_date.is_none() && t.end_date.is_none());

    let mut prims: Vec<Prim> = Vec::new();
    let mut id_rects: Vec<(String, Rect)> = Vec::new();
    let mut direct: Vec<(Rect, String)> = Vec::new();
    let clicks = collect_clicks(src);

    // 标题（居中于图表区）。
    if let Some(title) = &model.title {
        let cx = (chart_left + chart_right) * 0.5;
        prims.push(Prim::Text {
            at: Pos2::new(cx, size * 1.0),
            text: title.clone(),
            size: size * 1.1,
            plate: false,
            color: None,
        });
    }

    // 任务行。
    let mut y = top_pad;
    let mut last_section: Option<usize> = None;
    for (i, t) in model.tasks.iter().enumerate() {
        if last_section != Some(t.section) {
            // section 分隔线 + 标签（左侧）。
            prims.push(Prim::Line {
                a: Pos2::new(margin * 0.5, y),
                b: Pos2::new(chart_right, y),
                width: line_w,
                dotted: true,
            });
            if let Some(s) = model.sections.get(t.section) {
                prims.push(Prim::Text {
                    at: Pos2::new(margin * 0.5, y + row_h * 0.5),
                    text: s.clone(),
                    size: size * 0.9,
                    plate: false,
                    color: None,
                });
            }
            last_section = Some(t.section);
        }
        let x0 = chart_left + starts[i] * day_w;
        let w = (ends[i] - starts[i]).max(0.5) * day_w;
        let bar = Rect::from_min_size(
            Pos2::new(x0, y + (row_h - bar_h) * 0.5),
            Vec2::new(w, bar_h),
        );
        let (fill, stroke) = status_colors(t.status);
        prims.push(Prim::Poly {
            points: rect_corners(bar),
            radius: size * 0.2,
            fill: true,
            ticks: false,
            fill_color: Some(fill),
            stroke_color: Some(stroke),
        });
        // 任务名：条内白字（居中）。
        prims.push(Prim::Text {
            at: bar.center(),
            text: t.name.clone(),
            size: size * 0.8,
            plate: false,
            color: Some(Color32::WHITE),
        });
        // done：画对勾（沿用主题线色，条内可见）。
        if t.status == GanttStatus::Done {
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

    // 底部日期轴：竖向网格 + 刻度标签。
    let axis_y = y + row_h * 0.3;
    let mut tick = 0.0_f32;
    while tick <= domain_days + 1e-3 {
        let gx = chart_left + tick * day_w;
        // 竖向网格线（虚线、细）。
        prims.push(Prim::Line {
            a: Pos2::new(gx, top_pad),
            b: Pos2::new(gx, axis_y),
            width: line_w * 0.6,
            dotted: true,
        });
        // 刻度标签。
        let label = if no_dates {
            format!("{}", tick as i64)
        } else {
            date_label(base + tick as i64, step)
        };
        prims.push(Prim::Text {
            at: Pos2::new(gx, axis_y + size * 0.25),
            text: label,
            size: size * 0.7,
            plate: false,
            color: None,
        });
        tick += step;
    }
    // 轴基线（实线）。
    prims.push(Prim::Line {
        a: Pos2::new(chart_left, axis_y),
        b: Pos2::new(chart_right, axis_y),
        width: line_w,
        dotted: false,
    });

    Ok(finalize(prims, id_rects, clicks, direct))
}
