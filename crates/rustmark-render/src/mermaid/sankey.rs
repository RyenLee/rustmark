//! `sankey` / `sankey-beta` 桑基图：CSV 数据 → 分列节点 + 流带。
//!
//! 语法：
//!
//! ```text
//! sankey-beta
//!   A,B,10
//!   B,C,20
//!   A,C,5
//! ```
//!
//! 节点按首次出现顺序去重；沿边做最长路径松弛得到列号（迭代上限 = 节点数，
//! 有环也不会卡死）。每列节点竖直堆叠并整体居中，节点高度取「流入 / 流出」
//! 的较大者，流带用含透明度的凸四边形绘制。

use std::collections::HashMap;

use crate::render::{Pt, Rgb8, geom::Rect};

use super::graph_parse::leading_word;
use super::model::Prim;
use super::primitives::{collect_clicks, finalize, rect_corners};
use super::*;

/// 解析后的桑基图。
#[derive(Clone, Debug, Default)]
pub(super) struct SankeyModel {
    /// 节点名（首次出现顺序）。
    pub(super) nodes: Vec<String>,
    /// 边：`(源节点下标, 目标节点下标, 数值)`。
    pub(super) edges: Vec<(usize, usize, f32)>,
}

/// 去掉首尾空白与成对双引号。
fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim().to_string()
}

/// 按首次出现顺序登记节点，返回其下标。
fn intern(nodes: &mut Vec<String>, index: &mut HashMap<String, usize>, name: &str) -> usize {
    if let Some(&i) = index.get(name) {
        return i;
    }
    let i = nodes.len();
    nodes.push(name.to_string());
    index.insert(name.to_string(), i);
    i
}

/// 解析桑基图源码。
pub(super) fn parse_sankey(src: &str) -> Result<SankeyModel, MermaidError> {
    let mut model = SankeyModel::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut seen_header = false;
    for line in src.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        if !seen_header {
            seen_header = true;
            if leading_word(t).eq_ignore_ascii_case("sankey") {
                continue;
            }
            return Err(MermaidError::Malformed(format!(
                "桑基图首行必须是 `sankey` 或 `sankey-beta`：{t}"
            )));
        }
        let parts: Vec<&str> = t.split(',').collect();
        if parts.len() < 3 {
            return Err(MermaidError::Malformed(format!(
                "桑基图数据行应写成 `源,目标,数值`：{t}"
            )));
        }
        let src_name = unquote(parts[0]);
        let dst_name = unquote(parts[1]);
        // 数值必须为正（且有限），否则跳过该行。
        let Some(value) = unquote(parts[2])
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite() && *v > 0.0)
        else {
            continue;
        };
        if src_name.is_empty() || dst_name.is_empty() {
            continue;
        }
        let s = intern(&mut model.nodes, &mut index, &src_name);
        let d = intern(&mut model.nodes, &mut index, &dst_name);
        model.edges.push((s, d, value));
    }
    if model.edges.is_empty() {
        return Err(MermaidError::Malformed(
            "桑基图没有任何有效数据".to_string(),
        ));
    }
    Ok(model)
}

/// 沿边做最长路径松弛，得到每个节点的列号。
///
/// 迭代次数上限为节点数，且列号被钳制在 `n - 1`（无环图的最长路径上限）：
/// 有环时列号增长到上限即停止，不会死循环也不会无限膨胀。
pub(super) fn compute_ranks(n: usize, edges: &[(usize, usize, f32)]) -> Vec<usize> {
    let mut ranks = vec![0usize; n];
    let cap = n.saturating_sub(1);
    for _ in 0..n {
        let mut changed = false;
        for &(s, d, _) in edges {
            if s >= n || d >= n || s == d {
                continue;
            }
            let want = ranks[s].saturating_add(1).min(cap);
            if ranks[d] < want {
                ranks[d] = want;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    ranks
}

/// `sankey` 的布局入口。
pub(super) fn layout_sankey(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_sankey(src)?;
    let n = model.nodes.len();
    let ranks = compute_ranks(n, &model.edges);

    // 列号压缩：只保留实际出现的 rank，环造成的稀疏列号不会留下空列。
    let mut distinct = ranks.clone();
    distinct.sort_unstable();
    distinct.dedup();
    let col_of: Vec<usize> = ranks
        .iter()
        .map(|r| distinct.iter().position(|x| x == r).unwrap_or(0))
        .collect();
    let cols = distinct.len();

    // 每个节点的流入 / 流出总量决定竖条高度。
    let mut in_sum = vec![0.0f32; n];
    let mut out_sum = vec![0.0f32; n];
    for &(s, d, v) in &model.edges {
        out_sum[s] += v;
        in_sum[d] += v;
    }
    let unit: Vec<f32> = (0..n)
        .map(|i| in_sum[i].max(out_sum[i]).max(f32::EPSILON))
        .collect();

    let gap = size * 0.8;
    let target_h = size * 9.0;
    let mut per_col: Vec<Vec<usize>> = vec![Vec::new(); cols];
    for (i, &c) in col_of.iter().enumerate() {
        per_col[c].push(i);
    }
    // 全局比例：最高的一列（含间隙）正好铺满 target_h。
    let mut max_units = 0.0f32;
    for list in &per_col {
        let sum: f32 =
            list.iter().map(|&i| unit[i]).sum::<f32>() + gap * list.len().saturating_sub(1) as f32;
        max_units = max_units.max(sum);
    }
    let scale = if max_units > 0.0 {
        target_h / max_units
    } else {
        1.0
    };
    let node_h: Vec<f32> = unit.iter().map(|u| (u * scale).max(size * 0.3)).collect();

    let node_w = size * 1.2;
    let margin = size;
    // 列间距要容得下流带与最长的节点标签。
    let mut col_gap = node_w + size * 4.0;
    for node in &model.nodes {
        col_gap = col_gap.max(measure(node, size * 0.75) + size * 1.5);
    }
    let x_of = |c: usize| margin + c as f32 * col_gap;

    let top = size * 0.6;
    let center_y = top + target_h * 0.5;
    let mut node_rect = vec![Rect::from_min_size(Pt::zero(), Pt::zero()); n];
    for (c, list) in per_col.iter().enumerate() {
        let total: f32 = list.iter().map(|&i| node_h[i]).sum::<f32>()
            + gap * list.len().saturating_sub(1) as f32;
        let mut y = center_y - total * 0.5;
        for &i in list {
            node_rect[i] = Rect::from_min_size(Pt::new(x_of(c), y), Pt::new(node_w, node_h[i]));
            y += node_h[i] + gap;
        }
    }

    let mut prims: Vec<Prim> = Vec::new();

    // 流带先画：源竖条右边界的一段 → 目标竖条左边界的一段（4 点凸四边形）。
    let mut out_cursor: Vec<f32> = node_rect.iter().map(|r| r.top()).collect();
    let mut in_cursor: Vec<f32> = node_rect.iter().map(|r| r.top()).collect();
    for &(s, d, v) in &model.edges {
        // 同一列（含自环）退化为直线，跳过。
        if col_of[s] == col_of[d] {
            continue;
        }
        let thickness = (v * scale).max(1.0);
        let sy = out_cursor[s];
        let ty = in_cursor[d];
        out_cursor[s] = sy + thickness;
        in_cursor[d] = ty + thickness;
        let sr = node_rect[s];
        let dr = node_rect[d];
        let color = palette_color(s);
        prims.push(Prim::Poly {
            points: vec![
                Pt::new(sr.right(), sy),
                Pt::new(dr.left(), ty),
                Pt::new(dr.left(), ty + thickness),
                Pt::new(sr.right(), sy + thickness),
            ],
            radius: 0.0,
            fill: true,
            ticks: false,
            fill_color: Some(Rgb8::new(color.r, color.g, color.b, 110)),
            stroke_color: Some(Rgb8::new(color.r, color.g, color.b, 190)),
        });
    }

    // 节点竖条 + 上方标签（文字中心锚点，居中于竖条）。
    for (i, r) in node_rect.iter().enumerate() {
        let color = palette_color(i);
        prims.push(Prim::Poly {
            points: rect_corners(*r),
            radius: size * 0.1,
            fill: true,
            ticks: false,
            fill_color: Some(color),
            stroke_color: Some(color),
        });
        prims.push(Prim::Text {
            at: Pt::new(r.center().x, r.top() - size * 0.45),
            text: model.nodes[i].clone(),
            size: size * 0.75,
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
        layout_sankey(src, 14.0, &f).expect("应能布局")
    }

    #[test]
    fn parses_quoted_csv_and_skips_invalid_values() {
        let model = parse_sankey(
            "sankey-beta\n\
             \"A\",\"B\",10\n\
             B,C,not-a-number\n\
             A,C,-5\n\
             A,C,5\n",
        )
        .expect("解析桑基图");
        assert_eq!(model.nodes, ["A", "B", "C"]);
        assert_eq!(model.edges, [(0, 1, 10.0), (0, 2, 5.0)]);
    }

    #[test]
    fn nodes_are_deduped_in_first_seen_order() {
        let model = parse_sankey("sankey-beta\nB,A,3\nA,B,4\nA,B,5\n").expect("解析");
        assert_eq!(model.nodes, ["B", "A"]);
        assert_eq!(model.edges.len(), 3);
    }

    #[test]
    fn ranks_increase_along_edges() {
        let model = parse_sankey("sankey-beta\nA,B,10\nB,C,20\nA,C,5\n").expect("解析");
        let ranks = compute_ranks(model.nodes.len(), &model.edges);
        assert_eq!(ranks, [0, 1, 2]);
        for &(s, d, _) in &model.edges {
            assert!(ranks[d] > ranks[s]);
        }
    }

    #[test]
    fn layout_has_positive_size_and_prims() {
        let l = lay("sankey-beta\nA,B,10\nB,C,20\nA,C,5\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        assert!(prim_count(&l) > 0);
    }

    #[test]
    fn empty_data_is_malformed() {
        let f = m();
        assert!(matches!(
            layout_sankey("sankey-beta\n%% 没有数据\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
    }

    #[test]
    fn short_row_is_malformed() {
        let f = m();
        assert!(matches!(
            layout_sankey("sankey-beta\nA,B\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
    }

    /// 收集版面里的所有文字，顺序与图元顺序一致。
    fn texts(l: &MermaidLayout) -> Vec<String> {
        l.prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn self_loop_is_skipped_but_layout_is_valid() {
        let model = parse_sankey("sankey-beta\nA,A,5\n").expect("解析");
        assert_eq!(model.nodes, ["A"]);
        assert_eq!(model.edges, [(0, 0, 5.0)]);

        let l = lay("sankey-beta\nA,A,5\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        // 自环同列，退化为不画流带：只剩节点竖条一个填充多边形。
        assert_eq!(l.node_count(), 1);
    }

    #[test]
    fn cyclic_edges_terminate_with_bounded_ranks() {
        let model = parse_sankey("sankey-beta\nA,B,1\nB,A,1\n").expect("解析");
        let n = model.nodes.len();
        let ranks = compute_ranks(n, &model.edges);
        assert_eq!(ranks.len(), n);
        // 环上列号必须收敛在节点数以内，不会无限增长。
        assert!(
            ranks.iter().all(|&r| r <= n),
            "环上 rank 超过节点数：{ranks:?}"
        );

        // 布局同样能结束，且尺寸为正。
        let l = lay("sankey-beta\nA,B,1\nB,A,1\n");
        assert!(l.width > 0.0 && l.height > 0.0);
    }

    #[test]
    fn quoted_names_with_spaces_are_kept_whole() {
        let model = parse_sankey("sankey-beta\n\"New York\",B,3\n").expect("解析");
        assert_eq!(model.nodes, ["New York", "B"]);
        assert_eq!(model.edges, [(0, 1, 3.0)]);
    }

    #[test]
    fn skips_negative_nan_and_text_values() {
        let model = parse_sankey(
            "sankey-beta\n\
             A,B,-5\n\
             A,B,NaN\n\
             A,B,not-a-number\n\
             A,B,7\n",
        )
        .expect("解析");
        assert_eq!(model.nodes, ["A", "B"]);
        assert_eq!(model.edges, [(0, 1, 7.0)], "只保留唯一有效边");
    }

    #[test]
    fn node_names_are_drawn_as_text() {
        let l = lay("sankey-beta\nA,B,10\n");
        let texts = texts(&l);
        assert_eq!(texts.iter().filter(|t| t.as_str() == "A").count(), 1);
        assert_eq!(texts.iter().filter(|t| t.as_str() == "B").count(), 1);
    }

    #[test]
    fn header_without_data_is_malformed() {
        let f = m();
        assert!(matches!(
            layout_sankey("sankey-beta\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
    }
}
