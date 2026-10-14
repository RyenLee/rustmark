//! 流程图布局：分层（Sugiyama 骨架）→ 层内排序 → 定位。

use egui::{Pos2, Rect, Vec2};

use super::model::{Graph, NodeShape, Placed};
use super::primitives::{collect_clicks, emit, finalize, first_md_link};
use super::{graph_parse::parse_graph, *};

/// `graph` / `flowchart` 的布局（原 `layout` 的「节点-边」路径）。
pub(super) fn layout_graph(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let g = parse_graph(src)?;
    if g.nodes.is_empty() {
        return Err(MermaidError::Malformed(
            "图里没有任何节点（只有 `graph LR` 一行？）".to_string(),
        ));
    }
    layout_from_graph(g, src, size, measure)
}

/// 解析产物 → 版面：分层、排序、定位、图元、热点。
///
/// Mermaid 流程图与 flowchart.js（见 [`super::flow_js`]）的「定义 + 连线」
/// 都落成同一个 [`Graph`]，故后半段布局管线完全共用；差别只在解析与流向
/// （flowchart.js 恒为自上而下）。`src` 仅用于提取 `click` 指令的热点。
pub(super) fn layout_from_graph(
    g: Graph,
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let ranks = rank_nodes(&g);
    let layers = order_within_rank(&g, &ranks);
    let placed = place(&g, &layers, size, measure);
    let prims = emit(&g, &placed, size, measure);
    // 节点外接框（供 `click` 与标签内 markdown 链接命中热点）。
    let mut id_rects: Vec<(String, Rect)> = Vec::new();
    let mut direct: Vec<(Rect, String)> = Vec::new();
    let clicks = collect_clicks(src);
    for (v, p) in placed.iter().enumerate() {
        let r = Rect::from_center_size(p.center, p.size);
        let id = &g.node_ids[v];
        id_rects.push((id.clone(), r));
        if let Some(url) = first_md_link(&g.nodes[v].0) {
            direct.push((r, url));
        }
    }
    Ok(finalize(prims, id_rects, clicks, direct))
}

/// 最长路径分层：`rank[v] = max(rank[pred] + 1)`。
///
/// 迭代**有上界**（最多 `节点数` 轮），故**有环也不会卡死**：
/// 环上的节点会稳定在某个较大 rank 上（图仍画得出来，只是不最优）。
pub(super) fn rank_nodes(g: &Graph) -> Vec<usize> {
    let n = g.nodes.len();
    let mut rank = vec![0usize; n];
    for _ in 0..n.max(1) {
        let mut changed = false;
        for e in &g.edges {
            // 自环必须排除：`A-->A` 若参与松弛会把自身一轮轮推高，
            // 白跑满所有轮次（结果虽然被上界兜住，但徒增开销且层次难看）。
            if e.from == e.to {
                continue;
            }
            let want = rank[e.from] + 1;
            if want > rank[e.to] && want <= n {
                rank[e.to] = want;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    rank
}

/// 同 rank 内排序：按重心迭代两轮，减少边交叉。
///
/// 第一轮按「前驱的平均次序」，第二轮按「后继的平均次序」——Sugiyama 的
/// 相邻交换启发式的最简形式。不做的话 `A-->C, B-->D` 这类图会出现大量交叉线。
fn order_within_rank(g: &Graph, ranks: &[usize]) -> Vec<Vec<usize>> {
    let max_rank = ranks.iter().copied().max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    // 初始次序 = 首次出现的次序（稳定且可预期）。
    for v in 0..g.nodes.len() {
        layers[ranks[v]].push(v);
    }
    for pass in 0..2 {
        let forward = pass == 0;
        let mut range: Vec<usize> = if forward {
            (1..=max_rank).collect()
        } else {
            (0..max_rank).collect()
        };
        if !forward {
            range.reverse();
        }
        for r in range {
            // 注意：先只读 `layers` 算出重心，再写回 `layers[r]`。
            // 若在 `.map()` 里直接改 `layers[r]`，闭包会同时持有可变与不可变借用。
            let mut scored: Vec<(f32, usize)> = layers[r]
                .iter()
                .map(|v| {
                    let mut sum = 0.0_f32;
                    let mut count = 0usize;
                    for e in &g.edges {
                        let (near, far) = if forward {
                            (e.to, e.from)
                        } else {
                            (e.from, e.to)
                        };
                        if near != *v || ranks[far] == r {
                            continue;
                        }
                        if let Some(p) = layers[ranks[far]].iter().position(|x| *x == far) {
                            sum += p as f32;
                            count += 1;
                        }
                    }
                    let key = if count == 0 {
                        // 无邻居的节点保持原位次（用自己在层内的位置兜底）。
                        layers[r]
                            .iter()
                            .position(|x| x == v)
                            .map_or(0.0, |p| p as f32)
                    } else {
                        sum / count as f32
                    };
                    (key, *v)
                })
                .collect();
            scored.sort_by(|a, b| a.0.total_cmp(&b.0));
            layers[r] = scored.into_iter().map(|(_, v)| v).collect();
        }
    }
    layers
}

/// 节点尺寸：由文本量 + 外形决定。
fn node_size(text: &str, shape: NodeShape, size: f32, measure: Measure<'_>) -> Vec2 {
    let tw = measure(text, size);
    let pad_x = size * NODE_PAD_X;
    let pad_y = size * NODE_PAD_Y;
    let mut w = tw + 2.0 * pad_x;
    let mut h = size + 2.0 * pad_y;
    match shape {
        // 菱形 / 六边形的可用内切区只有一半左右，文本要留双倍余量，
        // 否则文字会顶出斜边。
        NodeShape::Rhombus => {
            w *= DIAMOND_SCALE;
            h *= DIAMOND_SCALE;
        }
        NodeShape::Hexagon | NodeShape::Parallelogram => {
            w *= 1.45;
            h *= 1.1;
        }
        NodeShape::Circle => {
            let d = w.max(h) * CIRCLE_D;
            w = d;
            h = d;
        }
        NodeShape::Stadium => {
            w += h;
        }
        _ => {}
    }
    Vec2::new(w.max(size * 1.2), h.max(size * 1.5))
}

/// 定位：rank → 主坐标，rank 内次序 → 次坐标。
///
/// 内部一律按「流向水平」计算，需要时在末尾把主轴镜像 / 交换——
/// 若为横竖两种流向各写一套定位，两套的分支点迟早会不一致。
fn place(g: &Graph, layers: &[Vec<usize>], size: f32, measure: Measure<'_>) -> Vec<Placed> {
    let sizes: Vec<Vec2> = g
        .nodes
        .iter()
        .map(|(t, s)| node_size(t, *s, size, measure))
        .collect();
    let gap = size * NODE_GAP;
    let rank_gap = size * RANK_GAP;

    // 每列的主向厚度与次向总长。
    let mut col_thick = vec![0.0_f32; layers.len()];
    let mut col_span = vec![0.0_f32; layers.len()];
    for (r, layer) in layers.iter().enumerate() {
        for v in layer {
            let s = sizes[*v];
            col_thick[r] = col_thick[r].max(if g.horizontal { s.x } else { s.y });
        }
        col_span[r] = layer
            .iter()
            .map(|v| {
                if g.horizontal {
                    sizes[*v].y
                } else {
                    sizes[*v].x
                }
            })
            .sum::<f32>()
            + gap * layer.len().saturating_sub(1) as f32;
    }
    let max_span = col_span.iter().copied().fold(0.0_f32, f32::max);

    // 每列的主向起点。
    let mut col_start = vec![0.0_f32; layers.len()];
    let mut acc = 0.0_f32;
    for r in 0..layers.len() {
        col_start[r] = acc;
        acc += col_thick[r] + rank_gap;
    }

    let mut out = vec![
        Placed {
            center: Pos2::ZERO,
            size: Vec2::ZERO,
            shape: NodeShape::Rect,
        };
        g.nodes.len()
    ];
    for (r, layer) in layers.iter().enumerate() {
        // 本列在主向上整体居中。
        let mut secondary = (max_span - col_span[r]) * 0.5;
        for v in layer {
            let s = sizes[*v];
            let (sec_extent, thick_extent) = if g.horizontal { (s.y, s.x) } else { (s.x, s.y) };
            let primary = col_start[r] + (col_thick[r] - thick_extent) * 0.5;
            let (px, py) = if g.horizontal {
                (primary, secondary)
            } else {
                (secondary, primary)
            };
            secondary += sec_extent + gap;
            out[*v] = Placed {
                center: Pos2::new(px + s.x * 0.5, py + s.y * 0.5),
                size: s,
                shape: g.nodes[*v].1,
            };
        }
    }

    // 反向（`RL` / `BT`）：把主坐标镜像。
    if g.reversed {
        let total = acc - rank_gap;
        for p in &mut out {
            if g.horizontal {
                p.center.x = total - p.center.x;
            } else {
                p.center.y = total - p.center.y;
            }
        }
    }
    out
}
