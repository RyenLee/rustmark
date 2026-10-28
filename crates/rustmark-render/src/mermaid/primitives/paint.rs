//! 图元收尾工具、热点解析，与「`Prim` → `RenderCommand`」的几何翻译（问题 F / O4）。
//!
//! 本文件不再 `use egui`：几何用 `render` 端口层的 `Pt` / `geom::Rect` / `Rgb8`，
//! 绘制由唯一 egui 后端 `render::apply_egui` 完成。

use crate::render::{Pt, RenderCommand, Rgb8, TextAnchor, geom::Rect};

use super::super::model::{MermaidLayout, Prim};

/// 图表配色（由调用方从主题取；本模块不依赖 `Theme`）。
#[derive(Clone, Copy, Debug)]
pub struct MermaidStyle {
    /// 节点填充色。
    pub node_fill: Rgb8,
    /// 节点描边色。
    pub node_stroke: Rgb8,
    /// 节点文字色。
    pub text: Rgb8,
    /// 连线色。
    pub edge: Rgb8,
    /// 边标签底色。
    pub label_bg: Rgb8,
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
        Prim::Cross { at, len, .. } => Rect::from_center_size(*at, Pt::splat(*len * 2.0)),
        Prim::Text { at, text, size, .. } => {
            // 用「字符数 × 半字号」估外接框：这里只需要**足够大**。
            // 真实宽度由绘制阶段的字体度量决定，估算偏差只让版面多一点留白。
            let w = text.chars().count() as f32 * *size * 0.55;
            Rect::from_center_size(*at, Pt::new(w.max(*size), *size * 1.5))
        }
    }
}

fn shift_prim(p: Prim, d: Pt) -> Prim {
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

/// 把版面翻译成 egui-free 的绘制命令序列（着色由 `style` 提供）。
///
/// 原点与缩放由下游 `render::apply_egui` 负责——本函数只在**内容局部坐标**
/// 里把每个 `Prim` 翻译成对应的 `RenderCommand`，与旧 `paint` 的居中 / 等比
/// 缩放公式（见 `render::apply_egui`）解耦。
pub fn commands(layout: &MermaidLayout, style: &MermaidStyle) -> Vec<RenderCommand> {
    layout
        .prims
        .iter()
        .map(|p| prim_to_command(p, style))
        .collect()
}

/// 单个 `Prim` → `RenderCommand`。
fn prim_to_command(p: &Prim, style: &MermaidStyle) -> RenderCommand {
    match p {
        Prim::Poly {
            points,
            radius,
            fill,
            ticks,
            fill_color,
            stroke_color,
        } => RenderCommand::Poly {
            points: points.clone(),
            radius: *radius,
            fill: *fill,
            ticks: *ticks,
            fill_color: fill_color.unwrap_or(style.node_fill),
            stroke_color: stroke_color.unwrap_or(style.node_stroke),
        },
        Prim::Line {
            a,
            b,
            width,
            dotted,
        } => RenderCommand::Line {
            a: *a,
            b: *b,
            width: *width,
            dotted: *dotted,
            color: style.edge,
        },
        Prim::Arrow {
            tip,
            dir,
            len,
            spread,
        } => RenderCommand::Arrow {
            tip: *tip,
            dir: *dir,
            len: *len,
            spread: *spread,
            color: style.edge,
        },
        Prim::Cross { at, dir, len } => RenderCommand::Cross {
            at: *at,
            dir: *dir,
            len: *len,
            color: style.edge,
        },
        Prim::Text {
            at,
            text,
            size,
            plate,
            color,
        } => RenderCommand::Text {
            at: *at,
            text: text.clone(),
            size: *size,
            italic: false,
            anchor: TextAnchor::Center,
            plate: *plate,
            plate_bg: style.label_bg,
            color: color.unwrap_or(style.text),
        },
    }
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
    let bounds = bounds.unwrap_or_else(|| Rect::from_min_size(Pt::zero(), Pt::splat(1.0)));
    let offset = -bounds.min;
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
pub fn rect_corners(r: Rect) -> Vec<Pt> {
    vec![
        Pt::new(r.left(), r.top()),
        Pt::new(r.right(), r.top()),
        Pt::new(r.right(), r.bottom()),
        Pt::new(r.left(), r.bottom()),
    ]
}

/// 矩形（中心 + 半尺寸）沿 `dir` 方向到边界的交点。
pub fn rect_boundary(center: Pt, half: Pt, dir: Pt) -> Pt {
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
    Pt::new(center.x + dir.x * k, center.y + dir.y * k)
}

/// 在矩形里画一个对勾（两段 `Line`），表示「已完成」。
pub fn push_check(prims: &mut Vec<Prim>, r: Rect, w: f32) {
    let cx = r.left() + r.width() * 0.3;
    let cy = r.center().y;
    let s = r.height() * 0.25;
    prims.push(Prim::Line {
        a: Pt::new(cx - s, cy),
        b: Pt::new(cx, cy + s),
        width: w,
        dotted: false,
    });
    prims.push(Prim::Line {
        a: Pt::new(cx, cy + s),
        b: Pt::new(cx + s * 1.6, cy - s),
        width: w,
        dotted: false,
    });
}
