//! `gitGraph` 提交图：分支 lane + 提交圆点 + merge / cherry-pick 连线。
//!
//! 语法（命令大小写不敏感）：
//!
//! ```text
//! gitGraph
//!   commit
//!   commit id: "初始化"
//!   branch develop
//!   checkout develop
//!   commit id: "功能开发"
//!   checkout main
//!   merge develop
//! ```
//!
//! 布局是纯函数：全局提交序号决定横坐标，分支 lane 决定纵坐标；同名分支
//! 复用一个 lane。无法识别的行（`%%` 注释、`options` 等）一律忽略。

use crate::render::Pt;

use super::graph_parse::leading_word;
use super::model::Prim;
use super::primitives::{collect_clicks, finalize};
use super::*;

/// 解析后的提交图。
#[derive(Clone, Debug, Default)]
pub(super) struct GitModel {
    pub(super) title: Option<String>,
    /// 分支：`(名称, 建立时的 HEAD 提交下标)`。lane 即下标。
    pub(super) branches: Vec<(String, usize)>,
    /// 提交：`(分支下标, 显示文字)`。
    pub(super) commits: Vec<(usize, String)>,
    /// 额外连线：`(新提交下标, 目标提交下标)`（merge / cherry-pick）。
    pub(super) links: Vec<(usize, usize)>,
}

/// 取 `key: "..."` 形式属性里的引号内文字（`key` 大小写不敏感）。
///
/// 只做「找到 key → 跳过冒号 → 取第一对双引号」的线性扫描，读不懂返回
/// `None`；绝不按字节下标切中文，避免切在字符中间。
fn attr_quoted(line: &str, key: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find(key) {
        let at = from + rel + key.len();
        if let Some(rest) = line[at..].trim_start().strip_prefix(':') {
            let rest = rest.trim_start();
            if let Some(q) = rest.strip_prefix('"')
                && let Some(end) = q.find('"')
            {
                return Some(q[..end].to_string());
            }
        }
        from = at;
    }
    None
}

/// 按名称查分支 lane。
fn branch_index(branches: &[(String, usize)], name: &str) -> Option<usize> {
    branches.iter().position(|(n, _)| n == name)
}

/// 提交文字：优先 `id:`，其次 `msg:`，都没有时用递增的 `c1`、`c2`…
fn commit_label(line: &str, auto: &mut usize) -> String {
    if let Some(id) = attr_quoted(line, "id") {
        return id;
    }
    if let Some(msg) = attr_quoted(line, "msg") {
        return msg;
    }
    *auto += 1;
    format!("c{auto}")
}

/// 去掉标题前的冒号与成对引号。
fn title_text(rest: &str) -> String {
    let t = rest.trim();
    let t = t.strip_prefix(':').unwrap_or(t).trim();
    t.trim_matches('"').trim().to_string()
}

/// 解析提交图源码。
pub(super) fn parse_git_graph(src: &str) -> Result<GitModel, MermaidError> {
    let mut lines = src
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("%%"));
    let header = lines
        .next()
        .ok_or_else(|| MermaidError::Malformed("gitGraph 内容为空".to_string()))?;
    if !leading_word(header).eq_ignore_ascii_case("gitGraph") {
        return Err(MermaidError::Malformed(format!(
            "gitGraph 首行必须是 `gitGraph`：{header}"
        )));
    }

    let mut model = GitModel {
        // 首个分支默认 main，lane 0。
        branches: vec![("main".to_string(), 0)],
        ..GitModel::default()
    };
    let mut heads: Vec<Option<usize>> = vec![None];
    let mut cur = 0usize;
    let mut auto = 0usize;

    for line in lines {
        let word = leading_word(line);
        let rest = line[word.len()..].trim();
        match word.to_ascii_lowercase().as_str() {
            "title" => {
                let t = title_text(rest);
                if !t.is_empty() {
                    model.title = Some(t);
                }
            }
            "commit" => {
                let label = commit_label(line, &mut auto);
                let idx = model.commits.len();
                model.commits.push((cur, label));
                heads[cur] = Some(idx);
            }
            "branch" => {
                let name = rest.split_whitespace().next().unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                if let Some(idx) = branch_index(&model.branches, name) {
                    cur = idx;
                    continue;
                }
                let anchor = heads[cur].unwrap_or(model.commits.len());
                let inherited = heads[cur];
                model.branches.push((name.to_string(), anchor));
                heads.push(inherited);
                cur = model.branches.len() - 1;
            }
            "checkout" | "switch" => {
                let name = rest.split_whitespace().next().unwrap_or("");
                if let Some(idx) = branch_index(&model.branches, name) {
                    cur = idx;
                }
            }
            "merge" => {
                let name = rest.split_whitespace().next().unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let target = branch_index(&model.branches, name).and_then(|b| heads[b]);
                let label = attr_quoted(line, "id").unwrap_or_else(|| format!("merge {name}"));
                let idx = model.commits.len();
                model.commits.push((cur, label));
                if let Some(t) = target {
                    model.links.push((idx, t));
                }
                heads[cur] = Some(idx);
            }
            // `leading_word` 在 `-` 处截断，`cherry-pick` 的首词是 `cherry`。
            "cherry" | "cherrypick" => {
                // 先在已有提交里找被挑选的目标，再压入新提交，避免找到自己。
                let id = attr_quoted(line, "id");
                let target = id
                    .as_ref()
                    .and_then(|id| model.commits.iter().rposition(|(_, l)| l == id));
                let label = id.unwrap_or_else(|| {
                    auto += 1;
                    format!("c{auto}")
                });
                let idx = model.commits.len();
                model.commits.push((cur, label));
                if let Some(t) = target {
                    model.links.push((idx, t));
                }
                heads[cur] = Some(idx);
            }
            // `options` / `%%` / 空行等：忽略。
            _ => {}
        }
    }
    Ok(model)
}

/// `gitGraph` 的布局入口。
pub(super) fn layout_git_graph(
    src: &str,
    size: f32,
    measure: Measure<'_>,
) -> Result<MermaidLayout, MermaidError> {
    let model = parse_git_graph(src)?;
    if model.commits.is_empty() {
        return Err(MermaidError::Malformed("gitGraph 没有任何提交".to_string()));
    }

    let line_w = 1.4 * (size / 16.0).max(0.7);
    let margin = size * 1.5;
    let radius = size * 0.42;
    let lane_gap = size * 3.0;
    // 提交间距：至少 3.2 字号，且容得下最长的 id 文字。
    let mut commit_gap = size * 3.2;
    for (_, label) in &model.commits {
        commit_gap = commit_gap.max(measure(label, size * 0.9) + size * 1.6);
    }

    let top = if model.title.is_some() {
        size * 4.0
    } else {
        size * 2.6
    };
    let n = model.commits.len();
    let x_of = |seq: usize| margin + seq as f32 * commit_gap;
    let y_of = |lane: usize| top + lane as f32 * lane_gap;

    let mut prims: Vec<Prim> = Vec::new();

    // 标题居中于提交序列上方。
    if let Some(title) = &model.title {
        let cx = x_of(0) + (n - 1) as f32 * commit_gap * 0.5;
        prims.push(Prim::Text {
            at: Pt::new(cx, size * 1.0),
            text: title.clone(),
            size: size * 1.1,
            plate: false,
            color: None,
        });
    }

    // 同分支相邻提交之间的实线（lane 内水平走线）。
    for lane in 0..model.branches.len() {
        let mut prev: Option<usize> = None;
        for (i, (b, _)) in model.commits.iter().enumerate() {
            if *b != lane {
                continue;
            }
            if let Some(p) = prev {
                prims.push(Prim::Line {
                    a: Pt::new(x_of(p), y_of(lane)),
                    b: Pt::new(x_of(i), y_of(lane)),
                    width: line_w,
                    dotted: false,
                });
            }
            prev = Some(i);
        }
    }

    // merge / cherry-pick：新提交通往目标提交的直线。
    for (from, to) in &model.links {
        let (Some((fb, _)), Some((tb, _))) = (model.commits.get(*from), model.commits.get(*to))
        else {
            continue;
        };
        prims.push(Prim::Line {
            a: Pt::new(x_of(*from), y_of(*fb)),
            b: Pt::new(x_of(*to), y_of(*tb)),
            width: line_w,
            dotted: false,
        });
    }

    // 提交圆点 + 下方 id 文字。
    for (i, (b, label)) in model.commits.iter().enumerate() {
        let c = Pt::new(x_of(i), y_of(*b));
        let color = palette_color(*b);
        prims.push(Prim::Poly {
            points: vec![
                Pt::new(c.x - radius, c.y - radius),
                Pt::new(c.x + radius, c.y - radius),
                Pt::new(c.x + radius, c.y + radius),
                Pt::new(c.x - radius, c.y + radius),
            ],
            radius,
            fill: true,
            ticks: false,
            fill_color: Some(color),
            stroke_color: Some(color),
        });
        prims.push(Prim::Text {
            at: Pt::new(c.x, c.y + radius + size * 0.9),
            text: label.clone(),
            size: size * 0.9,
            plate: false,
            color: None,
        });
    }

    // 分支名画在该分支第一个提交的上方；没有提交时退回建支位置。
    for (lane, (name, anchor)) in model.branches.iter().enumerate() {
        let first = model
            .commits
            .iter()
            .position(|(b, _)| *b == lane)
            .unwrap_or_else(|| (*anchor).min(n - 1));
        prims.push(Prim::Text {
            at: Pt::new(x_of(first), y_of(lane) - size * 1.4),
            text: name.clone(),
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
        layout_git_graph(src, 14.0, &f).expect("应能布局")
    }

    const SAMPLE: &str = "gitGraph\n\
                          commit\n\
                          commit id: \"初始化\"\n\
                          branch develop\n\
                          checkout develop\n\
                          commit id: \"功能开发\"\n\
                          checkout main\n\
                          merge develop\n\
                          commit id: \"发布\"\n";

    #[test]
    fn parses_commit_branch_checkout_and_merge() {
        let model = parse_git_graph(SAMPLE).expect("解析提交图");
        // 4 个普通提交 + 1 个 merge 提交。
        assert_eq!(model.commits.len(), 5);
        assert_eq!(model.branches.len(), 2);
        assert_eq!(model.branches[0].0, "main");
        assert_eq!(model.branches[1].0, "develop");
        // merge develop：一条连回 develop 当前 HEAD 的线。
        assert_eq!(model.links.len(), 1);
        let (from, to) = model.links[0];
        assert_eq!(to, 2, "develop 的 HEAD 是第 3 个提交");
        assert_eq!(model.commits[from].1, "merge develop");
        assert_eq!(model.commits[0].1, "c1");
        assert_eq!(model.commits[1].1, "初始化");
    }

    #[test]
    fn layout_has_positive_size_and_prims() {
        let l = lay(SAMPLE);
        assert!(l.width > 0.0 && l.height > 0.0);
        assert!(prim_count(&l) > 0);
        // 每个提交恰好一个实心圆。
        assert_eq!(l.node_count(), 5);
    }

    #[test]
    fn empty_graph_is_malformed() {
        let f = m();
        assert!(matches!(
            layout_git_graph("gitGraph\n%% 只有注释\n", 14.0, &f),
            Err(MermaidError::Malformed(_))
        ));
    }

    #[test]
    fn chinese_commit_id_does_not_panic() {
        let l = lay("gitGraph\ncommit id: \"初始化\"\ncommit id: \"中文分支提交\"\n");
        assert!(l.width > 0.0);
    }

    #[test]
    fn cherry_pick_links_to_picked_commit() {
        let model = parse_git_graph(
            "gitGraph\n\
             commit id: \"base\"\n\
             branch feature\n\
             checkout feature\n\
             cherry-pick id: \"base\"\n",
        )
        .expect("解析");
        assert_eq!(model.commits.len(), 2);
        assert_eq!(model.links, vec![(1, 0)]);
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
    fn switch_behaves_like_checkout() {
        // `switch` 与 `checkout` 同义：切换后新提交必须落在 develop lane。
        let switched =
            parse_git_graph("gitGraph\ncommit\nbranch develop\nswitch develop\ncommit\n")
                .expect("解析 switch");
        let checked_out =
            parse_git_graph("gitGraph\ncommit\nbranch develop\ncheckout develop\ncommit\n")
                .expect("解析 checkout");
        assert_eq!(switched.commits, checked_out.commits);
        assert_eq!(switched.branches[1].0, "develop");
        assert_eq!(switched.commits[1].0, 1, "switch 后的提交属于 develop");
        assert_eq!(switched.branches[switched.commits[1].0].0, "develop");
    }

    #[test]
    fn branch_without_prior_commit_does_not_panic() {
        // 还没有任何提交时建分支：解析不 panic，分支被登记。
        let model = parse_git_graph("gitGraph\nbranch develop\n").expect("解析");
        assert!(model.commits.is_empty());
        assert_eq!(model.branches.len(), 2);
        assert_eq!(model.branches[1].0, "develop");

        // 随后补一个提交即可布局；两个 lane 的名字都应画成文字。
        let l = lay("gitGraph\nbranch develop\ncommit\n");
        assert!(l.width > 0.0 && l.height > 0.0);
        let texts = texts(&l);
        assert!(texts.iter().any(|t| t == "develop"));
        assert!(texts.iter().any(|t| t == "main"));
    }

    #[test]
    fn merge_unknown_branch_adds_commit_without_link() {
        let model = parse_git_graph("gitGraph\ncommit\nmerge ghost\n").expect("解析");
        // 仍新增一个 merge 提交，但没有可连的目标。
        assert_eq!(model.commits.len(), 2);
        assert_eq!(model.commits[1].1, "merge ghost");
        assert!(model.links.is_empty());
        assert!(lay("gitGraph\ncommit\nmerge ghost\n").width > 0.0);
    }

    #[test]
    fn ignores_options_comments_and_blank_lines() {
        let model = parse_git_graph(
            "gitGraph\n\
             options\n\
             %% 这是注释\n\
             \n\
             commit\n\
             commit\n",
        )
        .expect("解析");
        assert_eq!(model.commits.len(), 2, "忽略的行不应产生提交");
    }

    #[test]
    fn title_is_drawn_as_text() {
        let l = lay("gitGraph\ntitle \"提交流\"\ncommit\n");
        assert!(texts(&l).iter().any(|t| t == "提交流"));
    }

    #[test]
    fn falls_back_to_msg_when_id_missing() {
        let model = parse_git_graph(
            "gitGraph\n\
             commit msg: \"修复缺陷\"\n\
             commit id: \"v1\" msg: \"id 优先\"\n",
        )
        .expect("解析");
        assert_eq!(model.commits[0].1, "修复缺陷");
        assert_eq!(model.commits[1].1, "v1", "id 存在时优先于 msg");
    }
}
