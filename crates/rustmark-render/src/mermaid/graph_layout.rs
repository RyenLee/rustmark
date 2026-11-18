//! 流程图布局：分层（Sugiyama 骨架）→ 层内排序 → 定位。

use crate::render::{Pt, geom::Rect};

use super::model::{Edge, Graph, Head, LineStyle, NodeShape, Placed, Prim};
use super::primitives::{collect_clicks, emit, finalize, first_md_link, push_group};
use super::{graph_parse::parse_graph, *};

/// 分组框内边距（乘基准字号）。
const CLUSTER_PAD: f32 = 1.05;
/// 分组框标题占用的高度（乘基准字号）。
const CLUSTER_TITLE: f32 = 1.7;

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
    // 有 `subgraph` 时走「组内独立分层 + 组间分层」的两级布局，
    // 否则保持原来的单级分层——没有分组时两级布局的产物没有收益。
    let (placed, boxes) = if g.groups.is_empty() {
        let ranks = rank_nodes(&g);
        let layers = order_within_rank(&g, &ranks);
        (place(&g, &layers, size, measure), Vec::new())
    } else {
        place_clustered(&g, size, measure)
    };
    // 分组框必须最先发出：它要被所有节点与连线盖住。
    let mut prims: Vec<Prim> = Vec::new();
    for (rect, label) in &boxes {
        push_group(&mut prims, *rect, label, size, measure);
    }
    prims.extend(emit(&g, &placed, size, measure));
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

/// 一个分组的布局结果：组外框尺寸与组内（已平移到外框内）的节点位置。
struct Cluster {
    /// `None` 表示这是「未分组节点」的单节点簇，不画框。
    label: Option<String>,
    members: Vec<usize>,
    local: Vec<Placed>,
    size: Pt,
}

/// 分组情况下的两级布局。
///
/// 第一级：每个 `subgraph` 内部只按**组内边**分层，得到组尺寸；
/// 第二级：把每个组当成一个节点（未分组节点各自成节点），按**跨组边**分层。
/// 最后把组内坐标平移到组在第二级里的位置。
///
/// 这样分组框天然互不重叠，跨组连线直接连到具体节点上。
fn place_clustered(
    g: &Graph,
    size: f32,
    measure: Measure<'_>,
) -> (Vec<Placed>, Vec<(Rect, String)>) {
    let pad = size * CLUSTER_PAD;
    let title = size * CLUSTER_TITLE;

    // 节点归属：嵌套分组时取**最内层**（后创建的分组）。
    let mut owner = vec![usize::MAX; g.nodes.len()];
    for (gi, group) in g.groups.iter().enumerate() {
        for v in &group.members {
            owner[*v] = gi;
        }
    }

    let mut clusters: Vec<Cluster> = Vec::new();
    let mut cluster_of_node = vec![0usize; g.nodes.len()];

    for (gi, group) in g.groups.iter().enumerate() {
        let members: Vec<usize> = (0..g.nodes.len()).filter(|v| owner[*v] == gi).collect();
        if members.is_empty() {
            continue;
        }
        let local = layout_cluster(g, &members, size, measure);
        let bbox = cluster_bounds(&local);
        let local: Vec<Placed> = local
            .into_iter()
            .map(|mut p| {
                p.center = Pt::new(
                    p.center.x - bbox.min.x + pad,
                    p.center.y - bbox.min.y + pad + title,
                );
                p
            })
            .collect();
        let cluster_size = Pt::new(bbox.width() + pad * 2.0, bbox.height() + pad * 2.0 + title);
        for v in &members {
            cluster_of_node[*v] = clusters.len();
        }
        clusters.push(Cluster {
            label: Some(group.label.clone()),
            members,
            local,
            size: cluster_size,
        });
    }

    // 不在任何 `subgraph` 里的节点：各自成一个无框簇，照常参与外层分层。
    for v in 0..g.nodes.len() {
        if owner[v] != usize::MAX {
            continue;
        }
        let s = node_size(&g.nodes[v].0, g.nodes[v].1, size, measure);
        cluster_of_node[v] = clusters.len();
        clusters.push(Cluster {
            label: None,
            members: vec![v],
            local: vec![Placed {
                center: Pt::new(s.x * 0.5, s.y * 0.5),
                size: s,
                shape: g.nodes[v].1,
            }],
            size: s,
        });
    }

    // 外层超图：簇当节点，只保留跨簇边（去重，且不画箭头）。
    let mut super_g = Graph {
        horizontal: g.horizontal,
        reversed: g.reversed,
        nodes: clusters
            .iter()
            .map(|c| (c.label.clone().unwrap_or_default(), NodeShape::Rect))
            .collect(),
        node_ids: (0..clusters.len())
            .map(|i| format!("__cluster{i}"))
            .collect(),
        edges: Vec::new(),
        groups: Vec::new(),
    };
    let mut seen: Vec<(usize, usize)> = Vec::new();
    for e in &g.edges {
        let a = cluster_of_node[e.from];
        let b = cluster_of_node[e.to];
        if a == b || seen.contains(&(a, b)) {
            continue;
        }
        seen.push((a, b));
        super_g.edges.push(Edge {
            from: a,
            to: b,
            label: None,
            line: LineStyle::Solid,
            head: Head::None,
            bidirectional: false,
        });
    }

    let sizes: Vec<Pt> = clusters.iter().map(|c| c.size).collect();
    let ranks = rank_nodes(&super_g);
    let layers = order_within_rank(&super_g, &ranks);
    // 外层间距同样要给**跨组**标签留宽度：`super_g` 的边刻意不带标签，
    // 故这里按原图的跨组边取列号对，再交给同一套逐列间距计算。
    let cluster_rank = rank_of(&layers, super_g.nodes.len());
    let pairs: Vec<(usize, usize, Option<&str>)> = g
        .edges
        .iter()
        .filter(|e| cluster_of_node[e.from] != cluster_of_node[e.to])
        .map(|e| {
            (
                cluster_rank[cluster_of_node[e.from]],
                cluster_rank[cluster_of_node[e.to]],
                e.label.as_deref(),
            )
        })
        .collect();
    let outer_gaps = labeled_gaps(g.horizontal, &pairs, layers.len(), size, measure);
    let placed_clusters = place_with_gaps(&super_g, &layers, &sizes, size, &outer_gaps);

    let mut out = vec![
        Placed {
            center: Pt::zero(),
            size: Pt::zero(),
            shape: NodeShape::Rect,
        };
        g.nodes.len()
    ];
    let mut boxes: Vec<(Rect, String)> = Vec::new();
    for (ci, cluster) in clusters.iter().enumerate() {
        let pc = &placed_clusters[ci];
        let origin = Pt::new(
            pc.center.x - cluster.size.x * 0.5,
            pc.center.y - cluster.size.y * 0.5,
        );
        for (k, v) in cluster.members.iter().enumerate() {
            let mut p = cluster.local[k].clone();
            p.center = p.center + origin;
            out[*v] = p;
        }
        if let Some(label) = &cluster.label {
            boxes.push((Rect::from_min_size(origin, cluster.size), label.clone()));
        }
    }
    (out, boxes)
}

/// 组内布局：只保留两端都在本组内的边，按整图流向分层。
fn layout_cluster(g: &Graph, members: &[usize], size: f32, measure: Measure<'_>) -> Vec<Placed> {
    let mut map = vec![usize::MAX; g.nodes.len()];
    for (k, v) in members.iter().enumerate() {
        map[*v] = k;
    }
    let mut sub = Graph {
        horizontal: g.horizontal,
        reversed: g.reversed,
        nodes: members.iter().map(|v| g.nodes[*v].clone()).collect(),
        node_ids: members.iter().map(|v| g.node_ids[*v].clone()).collect(),
        edges: Vec::new(),
        groups: Vec::new(),
    };
    for e in &g.edges {
        if map[e.from] != usize::MAX && map[e.to] != usize::MAX {
            sub.edges.push(Edge {
                from: map[e.from],
                to: map[e.to],
                label: e.label.clone(),
                line: e.line,
                head: e.head,
                bidirectional: e.bidirectional,
            });
        }
    }
    let ranks = rank_nodes(&sub);
    let layers = order_within_rank(&sub, &ranks);
    place(&sub, &layers, size, measure)
}

/// 一组已定位节点的外接框。
fn cluster_bounds(placed: &[Placed]) -> Rect {
    let mut bounds: Option<Rect> = None;
    for p in placed {
        let r = Rect::from_center_size(p.center, p.size);
        bounds = Some(match bounds {
            Some(b) => b.union(r),
            None => r,
        });
    }
    bounds.unwrap_or_else(|| Rect::from_min_size(Pt::zero(), Pt::splat(1.0)))
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
fn node_size(text: &str, shape: NodeShape, size: f32, measure: Measure<'_>) -> Pt {
    // 标签里的 `<br/>` 会被拆成多行：宽度取最长行，高度按行数累加。
    let lines = split_label_lines(text);
    let tw = lines
        .iter()
        .map(|l| measure(l, size))
        .fold(0.0_f32, f32::max);
    let pad_x = size * NODE_PAD_X;
    let pad_y = size * NODE_PAD_Y;
    let mut w = tw + 2.0 * pad_x;
    let mut h =
        size + lines.len().saturating_sub(1) as f32 * size * LABEL_LINE_HEIGHT + 2.0 * pad_y;
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
        // 状态图的 `[*]`：固定小圆点，不跟文本量走。
        NodeShape::Dot => return Pt::splat(size * 0.6),
        NodeShape::Stadium => {
            w += h;
        }
        _ => {}
    }
    Pt::new(w.max(size * 1.2), h.max(size * 1.5))
}

/// 定位：rank → 主坐标，rank 内次序 → 次坐标。
///
/// 内部一律按「流向水平」计算，需要时在末尾把主轴镜像 / 交换——
/// 若为横竖两种流向各写一套定位，两套的分支点迟早会不一致。
fn place(g: &Graph, layers: &[Vec<usize>], size: f32, measure: Measure<'_>) -> Vec<Placed> {
    let sizes: Vec<Pt> = g
        .nodes
        .iter()
        .map(|(t, s)| node_size(t, *s, size, measure))
        .collect();
    let rank_of = rank_of(layers, g.nodes.len());
    let pairs: Vec<(usize, usize, Option<&str>)> = g
        .edges
        .iter()
        .map(|e| (rank_of[e.from], rank_of[e.to], e.label.as_deref()))
        .collect();
    let gaps = labeled_gaps(g.horizontal, &pairs, layers.len(), size, measure);
    place_with_gaps(g, layers, &sizes, size, &gaps)
}

/// 每个节点所在的列号（`layers` 的逆映射）。
fn rank_of(layers: &[Vec<usize>], n: usize) -> Vec<usize> {
    let mut out = vec![0usize; n];
    for (r, layer) in layers.iter().enumerate() {
        for v in layer {
            out[*v] = r;
        }
    }
    out
}

/// 逐列间距：默认值之上，给**相邻两列之间**的边标签留出宽度。
///
/// 只加宽被标签跨越的那一段，而不是全图统一加宽——一条长标签把每列都
/// 推开，会让整图变宽、预览缩放后字更小（`C3 <-->|"HTTPS 激活/心跳"| P5`
/// 这类跨组短边是实测会压字的场景）。
fn labeled_gaps(
    horizontal: bool,
    pairs: &[(usize, usize, Option<&str>)],
    count: usize,
    size: f32,
    measure: Measure<'_>,
) -> Vec<f32> {
    let mut gaps = vec![size * RANK_GAP; count];
    if !horizontal {
        return gaps;
    }
    for &(a, b, label) in pairs {
        let Some(label) = label else { continue };
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        if hi == lo + 1 && lo < count {
            gaps[lo] = gaps[lo].max(measure(label, size * 0.85) + size * 1.6);
        }
    }
    gaps
}

/// 定位（节点尺寸由调用方提供）：分组布局的外层需要按「组尺寸」摆位，
/// 而组尺寸不是由文本量出来的，故把尺寸与逐列间距作为参数。
fn place_with_gaps(
    g: &Graph,
    layers: &[Vec<usize>],
    sizes: &[Pt],
    size: f32,
    rank_gaps: &[f32],
) -> Vec<Placed> {
    let gap = size * NODE_GAP;

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

    // 每列的主向起点（最后一列后不再加间距，`acc` 即总宽 / 总高）。
    let mut col_start = vec![0.0_f32; layers.len()];
    let mut acc = 0.0_f32;
    for r in 0..layers.len() {
        col_start[r] = acc;
        acc += col_thick[r];
        if r + 1 < layers.len() {
            acc += rank_gaps.get(r).copied().unwrap_or(0.0);
        }
    }

    let mut out = vec![
        Placed {
            center: Pt::zero(),
            size: Pt::zero(),
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
                center: Pt::new(px + s.x * 0.5, py + s.y * 0.5),
                size: s,
                shape: g.nodes[*v].1,
            };
        }
    }

    // 反向（`RL` / `BT`）：把主坐标镜像。
    if g.reversed {
        let total = acc;
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
