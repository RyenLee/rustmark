use super::class::{RelKind, parse_class};
use super::gantt::{GanttStatus, parse_gantt};
use super::graph_layout::rank_nodes;
use super::graph_parse::parse_graph;
use super::model::{Head, LineStyle, NodeShape, Prim};
use super::sequence::{parse_seq_message, parse_sequence};
use super::*;

/// 假度量：每字符宽 `0.5em`（与 `math` 的测试同口径）。
fn m() -> impl Fn(&str, f32) -> f32 {
    |s: &str, size: f32| s.chars().count() as f32 * size * 0.5
}

fn lay(src: &str) -> MermaidLayout {
    let f = m();
    layout(src, 14.0, &f).expect("应能布局")
}

fn bad(src: &str) -> MermaidError {
    let f = m();
    layout(src, 14.0, &f).expect_err("应报错")
}

/// 回归（V11-17）：flowchart.js 语法必须能渲染，而不是整卡降级。
///
/// 源样例取自 `docs/logs/示例文档.md` 的「FLowchart流程图」一节——**逐字**，
/// 含首行那处著名的 `flowchat` 笔误与全角 `？`。此前 `detect_kind` 把
/// `flowchat` 当图种关键字 → `Unsupported("flowchat")` → 源码卡，即使改对
/// 笔误也会在 `=>` 处报「期望边」：语法家族不同才是根因。
#[test]
fn flowchart_js_sample_from_demo_doc_renders() {
    let src = "flowchat\n\
               st=>start: 开始\n\
               e=>end: 结束\n\
               op=>operation: 我的操作\n\
               cond=>condition: 确认？\n\
               \n\
               st->op->cond\n\
               cond(yes)->e\n\
               cond(no)->op\n";
    let g = super::flow_js::parse(src).expect("解析");
    // st / e / op / cond 各一个，不得因重复引用而多出。
    assert_eq!(g.nodes.len(), 4, "{:?}", g.nodes);
    // st→op、op→cond、cond→e(yes)、cond→op(no)。
    assert_eq!(g.edges.len(), 4);
    // 定义决定外形：start/end 胶囊、condition 菱形、operation 矩形。
    let shape_of = |id: &str| {
        let pos = g
            .node_ids
            .iter()
            .position(|n| n == id)
            .unwrap_or_else(|| panic!("缺少节点 {id}"));
        (g.nodes[pos].0.clone(), g.nodes[pos].1)
    };
    assert_eq!(shape_of("st"), ("开始".to_string(), NodeShape::Stadium));
    assert_eq!(shape_of("e"), ("结束".to_string(), NodeShape::Stadium));
    assert_eq!(shape_of("op"), ("我的操作".to_string(), NodeShape::Rect));
    assert_eq!(shape_of("cond"), ("确认？".to_string(), NodeShape::Rhombus));
    // 括号标签写在来源引用上，落到它引出的边上。
    let labels: Vec<_> = g.edges.iter().map(|e| e.label.clone()).collect();
    assert_eq!(
        labels,
        vec![None, None, Some("yes".to_string()), Some("no".to_string())],
        "cond(yes)->e 与 cond(no)->op 的标签应各就各位"
    );
    // flowchart.js 恒为自上而下。
    assert!(!g.horizontal);
    // 版面完整可画（宽度高度非零、节点框恰好 4 个）。
    let l = lay(src);
    assert!(l.width > 0.0 && l.height > 0.0);
    assert_eq!(l.node_count(), 4);
}

/// Mermaid 流程图里 `=>` 出现在节点文本中时不得误判为 flowchart.js。
#[test]
fn mermaid_flowchart_with_arrow_in_text_is_not_flowchart_js() {
    assert!(!super::flow_js::is_flowchart_js(
        "flowchart TD\nA[条件 => 分支] --> B\n"
    ));
    assert!(!super::flow_js::is_flowchart_js("graph LR\nA-->B\n"));
    assert!(super::flow_js::is_flowchart_js("st=>start: 开始\nst->e\n"));
}

/// 只有标题行（无任何 `=>` 定义）不算 flowchart.js 语法：回退到 Mermaid
/// 路径报 `Unsupported("flowchat")`——没有可渲染的内容，这样比假装支持更诚实。
#[test]
fn flowchat_header_without_defs_is_unsupported() {
    let e = bad("flowchat\n");
    assert!(matches!(e, MermaidError::Unsupported(_)), "实际 {e:?}");
}

/// 未定义就引用的 id 按宽容约定建成裸节点（与 Mermaid 路径一致）。
#[test]
fn flowchart_js_edge_reference_creates_bare_node() {
    let g = super::flow_js::parse("st=>start: 开始\nst->e\n").expect("解析");
    assert_eq!(g.nodes.len(), 2);
    assert_eq!(g.edges.len(), 1);
    assert_eq!(g.nodes[1].0, "e", "未定义的 e 应以 id 为文本");
}

#[test]
fn simple_graph_has_two_nodes_and_one_edge() {
    let g = parse_graph("graph LR\nA --> B\n").expect("解析");
    assert_eq!(g.nodes.len(), 2);
    assert_eq!(g.edges.len(), 1);
    assert!(g.horizontal, "LR 应为水平流向");
}

/// 回归：「先定义带标签的节点、再按 id 串联」不得重复建节点。
///
/// 这正是 `docs/logs/示例文档.md` 里那段流程图，也是流程图最常见的写法。
/// 缺陷时期望 4 个节点却得到 8 个——多出来的四个是以 id 为标签的空壳，
/// 图面看上去就是「画错了 / 画重了」。
#[test]
fn repeated_id_reference_does_not_duplicate_nodes() {
    let g = parse_graph(
        "graph LR\n\
         A[长方形] -- 链接 --> B((圆))\n\
         A --> C(圆角长方形)\n\
         B --> D{菱形}\n\
         C --> D\n",
    )
    .expect("解析");

    assert_eq!(g.nodes.len(), 4, "节点应只有 A/B/C/D：{:?}", g.nodes);
    assert_eq!(g.edges.len(), 4);
    // id 与显示文本各归其位：标签不因后续的裸 id 引用而丢失。
    let by_id = |id: &str| {
        let pos = g
            .node_ids
            .iter()
            .position(|n| n == id)
            .unwrap_or_else(|| panic!("缺少节点 {id}"));
        g.nodes[pos].0.clone()
    };
    assert_eq!(by_id("A"), "长方形");
    assert_eq!(by_id("B"), "圆");
    assert_eq!(by_id("C"), "圆角长方形");
    assert_eq!(by_id("D"), "菱形");
    // 边必须落在**唯一**的那一对节点上（重复建节点会把边引到空壳上）。
    assert_eq!(g.edges[1].from, g.edges[0].from, "A --> C 应出自 A");
}

/// 链式流程（`A[开始] --> B[中间]` 后 `B --> C[结束]`）同样不得重复。
#[test]
fn bare_id_reference_keeps_existing_label() {
    let g = parse_graph("flowchart TD\n  A[开始] --> B[中间]\n  B --> C[结束]\n").expect("解析");
    assert_eq!(g.nodes.len(), 3, "节点应为 A/B/C：{:?}", g.nodes);
    let texts: Vec<&str> = g.nodes.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(texts, ["开始", "中间", "结束"]);
}

/// 同一 id 后写的形状与文本覆盖先写的（与 Mermaid 行为一致）。
#[test]
fn later_definition_overwrites_earlier() {
    let g = parse_graph("graph LR\nA[旧]\nA[新] --> B\n").expect("解析");
    assert_eq!(g.nodes.len(), 2);
    assert_eq!(g.nodes[0].0, "新");
}

#[test]
fn all_directions_are_recognized() {
    for (src, horiz, rev) in [
        ("graph LR\nA-->B", true, false),
        ("graph RL\nA-->B", true, true),
        ("graph TB\nA-->B", false, false),
        ("graph TD\nA-->B", false, false),
        ("graph BT\nA-->B", false, true),
        ("flowchart LR\nA-->B", true, false),
        // 只有 `graph` 没写方向时按 Mermaid 的默认 TB 处理。
        ("graph\nA-->B", false, false),
    ] {
        let g = parse_graph(src).unwrap_or_else(|e| panic!("{src} 应能解析：{e}"));
        assert_eq!(g.horizontal, horiz, "{src}");
        assert_eq!(g.reversed, rev, "{src}");
    }
}

#[test]
fn node_shapes_are_recognized() {
    let src = "graph LR\nA[a]-->B(b)-->C([c])-->D[[d]]-->E((e))-->F{f}";
    let g = parse_graph(src).expect("解析");
    let shapes: Vec<NodeShape> = g.nodes.iter().map(|(_, s)| *s).collect();
    assert_eq!(
        shapes,
        vec![
            NodeShape::Rect,
            NodeShape::Rounded,
            NodeShape::Stadium,
            NodeShape::Subroutine,
            NodeShape::Circle,
            NodeShape::Rhombus,
        ]
    );
}

#[test]
fn cylinder_and_hexagon_shapes_are_recognized() {
    let g = parse_graph("graph LR\nA[(库)]-->B{{六}}\n").expect("解析");
    assert_eq!(g.nodes[0].1, NodeShape::Cylinder);
    assert_eq!(g.nodes[1].1, NodeShape::Hexagon);
    assert_eq!(g.nodes[0].0, "库");
}

#[test]
fn node_text_overrides_the_id() {
    let g = parse_graph("graph LR\nA[启动流程]-->B\n").expect("解析");
    assert_eq!(g.nodes[0].0, "启动流程");
    // 裸 id 的节点用 id 当文本。
    assert_eq!(g.nodes[1].0, "B");
}

#[test]
fn quoted_node_text_is_unwrapped() {
    let g = parse_graph("graph LR\nA[\"带 空格 的文本\"]-->B\n").expect("解析");
    assert_eq!(g.nodes[0].0, "带 空格 的文本");
}

#[test]
fn edge_styles_and_heads_are_recognized() {
    let g = parse_graph("graph LR\nA-->B\nB-.->C\nC==>D\nD---E\nE--xF\nF--oG\n").expect("解析");
    let styles: Vec<(LineStyle, Head)> = g.edges.iter().map(|e| (e.line, e.head)).collect();
    assert_eq!(
        styles,
        vec![
            (LineStyle::Solid, Head::Arrow),
            (LineStyle::Dotted, Head::Arrow),
            (LineStyle::Thick, Head::Arrow),
            (LineStyle::Solid, Head::None),
            (LineStyle::Solid, Head::Cross),
            (LineStyle::Solid, Head::Circle),
        ]
    );
}

#[test]
fn bidirectional_edge_marks_both_ends() {
    let g = parse_graph("graph LR\nA<-->B\n").expect("解析");
    assert!(g.edges[0].bidirectional, "`<-->` 应记下起点侧的箭头");
}

#[test]
fn inline_edge_label_between_dashes_is_read() {
    let g = parse_graph("graph LR\nA -- 是 --> B\n").expect("解析");
    assert_eq!(g.edges[0].label.as_deref(), Some("是"));
}

#[test]
fn piped_edge_label_is_read() {
    let g = parse_graph("graph LR\nA -->|否| B\n").expect("解析");
    assert_eq!(g.edges[0].label.as_deref(), Some("否"));
}

#[test]
fn chained_edges_produce_one_edge_per_hop() {
    let g = parse_graph("graph LR\nA --> B --> C\n").expect("解析");
    assert_eq!(g.edges.len(), 2);
    assert_eq!(g.nodes.len(), 3);
}

#[test]
fn semicolons_and_comments_are_handled() {
    let g = parse_graph("graph LR; A-->B; B-->C;\n").expect("解析");
    assert_eq!(g.edges.len(), 2);
    // 语句写在头部同一行时也要读进来。
    let g = parse_graph("graph LR; A-->B\nB-->C\n").expect("解析");
    assert_eq!(g.edges.len(), 2);
}

#[test]
fn comment_lines_are_skipped() {
    let g = parse_graph("graph LR\n%% 这是一行注释\nA-->B\n%% 又一行\n").expect("解析");
    assert_eq!(g.edges.len(), 1);
}

#[test]
fn cosmetic_directives_are_ignored_not_fatal() {
    // 只改配色 / 交互的指令不该让整张图打回源码。
    let g = parse_graph(
        "graph LR\nA-->B\nstyle A fill:#f9f\nclassDef x fill:#fff\nlinkStyle 0 stroke:red\nclick A href \"x\"\n",
    )
    .expect("装饰性指令应被忽略");
    assert_eq!(g.nodes.len(), 2);
}

#[test]
fn other_diagram_kinds_are_unsupported() {
    // 这三种现已支持，见下方各自的布局测试；这里只保留仍未支持的图种。
    for src in [
        "pie title x\n\"a\" : 1",
        "stateDiagram-v2\n[*] --> A",
        "erDiagram\nA ||--o{ B : has",
        "journey\ntitle x",
    ] {
        assert!(
            matches!(bad(src), MermaidError::Unsupported(_)),
            "{src} 应报 Unsupported"
        );
    }
}

#[test]
fn sequence_messages_and_participants_parse() {
    let m =
        parse_sequence("sequenceDiagram\nparticipant A as Alice\nA->>B: hi\nNote over A,B: note\n")
            .expect("解析时序图");
    assert_eq!(m.parts.len(), 2, "Alice 与隐式参与者 B");
    assert_eq!(m.parts[0].1, "Alice");
    assert_eq!(m.events.len(), 2);
}

#[test]
fn sequence_layout_has_positive_size_and_prims() {
    let l = lay("sequenceDiagram\nA->>B: hi\nB-->>A: ok\nA--x C\n");
    assert!(l.width > 0.0);
    assert!(l.height > 0.0);
    assert!(prim_count(&l) > 0);
}

/// 回归：含**中文**参与者名的时序图曾直接崩进程。
///
/// 触发链：`parse_seq_arrow` 返回**字符**下标，`parse_seq_message` 却拿它
/// 当**字节**下标切 `&line[..idx]`——`李四` 之后的 `idx=2` 落在 `李`
/// （3 字节）内部，`&str` 切片 panic（`panic = "abort"` 崩进程）。
/// 见崩溃报告 `crash-20261010152913`（消息：end byte index 2 is not a
/// char boundary; it is inside '李'）。
#[test]
fn sequence_with_chinese_actor_names_does_not_panic() {
    let msg = parse_seq_message("李四->>张三: 你好").expect("应解析出消息");
    assert_eq!(msg.0, "李四", "发送方应是中文名 `李四`");
    assert_eq!(msg.1, "张三", "接收方应是中文名 `张三`");
    assert_eq!(msg.2, "你好");
    // 整条布局链路同样不得 panic（崩溃正是走 layout → parse_sequence）。
    let l = lay("sequenceDiagram\n李四->>张三: 你好吗\n张三-->>李四: 我很好\n");
    assert!(l.width > 0.0 && l.height > 0.0);
}

/// 回归：含中文的**类图关系**行曾同样崩在切片上（`parse_rel` 同源缺陷）。
#[test]
fn class_relation_with_chinese_names_does_not_panic() {
    let l = lay("classDiagram\n用户 --> 订单\n订单 <|-- 退款单\n");
    assert!(l.width > 0.0 && l.height > 0.0);
    // 关系两端应被完整切出（未被错位截断）。
    let m = parse_class("classDiagram\n用户 --> 订单\n").expect("解析类图");
    assert_eq!(m.rels.len(), 1);
    assert_eq!(m.rels[0].a, "用户");
    assert_eq!(m.rels[0].b, "订单");
}

#[test]
fn class_relationships_parse() {
    let m = parse_class("classDiagram\nclass A\nclass B\nA <|-- B\nA *-- B : label\n")
        .expect("解析类图");
    assert_eq!(m.classes.len(), 2);
    assert_eq!(m.rels.len(), 2);
    assert_eq!(m.rels[0].kind, RelKind::Inheritance);
}

#[test]
fn class_layout_has_positive_size_and_prims() {
    let l = lay("classDiagram\nclass A {\n+field: T\n-method()\n}\nclass B\nA <|-- B\n");
    assert!(l.width > 0.0);
    assert!(l.height > 0.0);
    assert!(prim_count(&l) > 0);
}

#[test]
fn gantt_tasks_parse() {
    let m = parse_gantt("gantt\ntitle T\nsection S\na : a1, 5d\nb : after a1, crit, 6d\n")
        .expect("解析甘特图");
    assert_eq!(m.tasks.len(), 2);
    assert_eq!(m.tasks[1].status, GanttStatus::Crit);
    assert_eq!(m.tasks[1].dep.as_deref(), Some("a1"));
}

#[test]
fn gantt_layout_has_positive_size_and_prims() {
    let l = lay("gantt\ntitle Plan\nsection Dev\na : a1, 5d\nb : after a1, crit, 6d\n");
    assert!(l.width > 0.0);
    assert!(l.height > 0.0);
    assert!(prim_count(&l) > 0);
}

#[test]
fn gantt_date_tasks_parse_with_real_dates() {
    let m =
        parse_gantt("gantt\ntitle T\nsection S\na : 2024-01-01, 5d\nb : 2024-01-06, 2024-01-09\n")
            .expect("解析甘特图");
    assert_eq!(m.tasks.len(), 2);
    assert!(m.tasks[0].start_date.is_some(), "显式开始日期应被解析");
    assert!(m.tasks[1].end_date.is_some(), "显式结束日期应被解析");
}

#[test]
fn gantt_applies_status_colors_per_bar() {
    let l = lay("gantt\ntitle P\nsection A\na : done, 5d\nb : active, 3d\nc : crit, 2d\nd : 4d\n");
    let colored = l
        .prims
        .iter()
        .filter(|p| {
            matches!(
                p,
                Prim::Poly {
                    fill_color: Some(_),
                    ..
                }
            )
        })
        .count();
    // 四个任务条都应带状态填充色覆盖（done/active/crit/默认）。
    assert!(
        colored >= 4,
        "每个任务条都应有状态填充色覆盖，实际 {colored}"
    );
}

#[test]
fn gantt_title_is_drawn_as_text() {
    let l = lay("gantt\ntitle My Plan\nsection A\na : 5d\n");
    let has_title = l
        .prims
        .iter()
        .any(|p| matches!(p, Prim::Text { text, .. } if text == "My Plan"));
    assert!(has_title, "标题应作为文本图元出现");
}

#[test]
fn gantt_has_date_axis_grid() {
    let l = lay("gantt\ntitle P\nsection A\na : 2024-01-01, 10d\n");
    let grid = l
        .prims
        .iter()
        .filter(|p| matches!(p, Prim::Line { dotted: true, .. }))
        .count();
    assert!(grid >= 2, "应有竖向网格线 + 刻度线，实际 {grid}");
}

#[test]
fn click_directive_creates_hotspot() {
    let l = lay("graph LR\nA-->B\nclick A href \"https://x\"\n");
    assert!(!l.click_targets().is_empty(), "click 应生成可点击热点");
}

#[test]
fn click_on_class_creates_hotspot() {
    let l = lay("classDiagram\nclass A\nclass B\nA --> B\nclick A href \"https://x\"\n");
    assert!(!l.click_targets().is_empty());
}

#[test]
fn markdown_link_in_label_creates_hotspot() {
    let l = lay("sequenceDiagram\nA->>B: 见 [文档](https://y)\n");
    assert!(
        !l.click_targets().is_empty(),
        "标签里的 markdown 链接应生成热点"
    );
}

#[test]
fn subgraph_is_unsupported_rather_than_half_drawn() {
    // 半张图比不画更难排查：用户会先怀疑自己的语法。
    let e = bad("graph LR\nsubgraph 一组\nA-->B\nend\n");
    assert!(matches!(e, MermaidError::Unsupported(_)), "实际 {e:?}");
}

#[test]
fn malformed_input_is_reported_not_panicked() {
    // `panic = "abort"` 下绝不能崩。
    for src in [
        "graph LR\nA -->",
        "graph LR\nA --> )",
        "graph LR\nA[未闭合 --> B",
        "graph LR\n",
        "graph LR\n-->B",
        "graph ZZ\nA-->B",
    ] {
        let _ = bad(src);
    }
}

#[test]
fn ranks_follow_edge_direction() {
    let g = parse_graph("graph LR\nA-->B-->C\n").expect("解析");
    let r = rank_nodes(&g);
    assert!(r[0] < r[1], "B 应排在 A 之后");
    assert!(r[1] < r[2], "C 应排在 B 之后");
}

#[test]
fn parallel_nodes_share_a_rank() {
    let g = parse_graph("graph LR\nA-->B\nA-->C\n").expect("解析");
    let r = rank_nodes(&g);
    assert_eq!(r[1], r[2], "B 与 C 同层");
}

#[test]
fn cycles_do_not_hang_or_panic() {
    let g = parse_graph("graph LR\nA-->B\nB-->C\nC-->A\n").expect("解析");
    let r = rank_nodes(&g);
    assert!(r.iter().any(|v| *v > 0), "有环也要排出层次");
    assert!(r.iter().all(|v| *v <= g.nodes.len()));
    // 布局本身也必须能结束（这条才是「不卡死」的真断言）。
    assert!(lay("graph LR\nA-->B\nB-->C\nC-->A\n").width > 0.0);
}

#[test]
fn self_loop_does_not_hang() {
    let g = parse_graph("graph LR\nA-->A\n").expect("解析");
    let r = rank_nodes(&g);
    assert_eq!(r[0], 0, "自环不应把自身推到第 1 层");
    assert!(lay("graph LR\nA-->A\n").height > 0.0);
}

#[test]
fn layout_produces_positive_bounds_and_prims() {
    let l = lay("graph LR\nA[开始]-->B{判断}-->C[结束]\n");
    assert!(l.width > 0.0);
    assert!(l.height > 0.0);
    assert!(prim_count(&l) > 0);
}

#[test]
fn horizontal_layout_grows_along_x() {
    let l = lay("graph LR\nA[第一步]-->B[第二步]-->C[第三步]\n");
    assert!(
        l.width > l.height,
        "LR 三列应比一列宽：{}x{}",
        l.width,
        l.height
    );
}

#[test]
fn vertical_layout_grows_along_y() {
    let l = lay("graph TD\nA[第一步]-->B[第二步]-->C[第三步]\n");
    assert!(
        l.height > l.width,
        "TD 三层应比一列高：{}x{}",
        l.width,
        l.height
    );
}

#[test]
fn isolated_node_is_still_drawn() {
    let g = parse_graph("graph LR\nA-->B\nC\n").expect("解析");
    assert_eq!(g.nodes.len(), 3);
    assert!(prim_count(&lay("graph LR\nA-->B\nC\n")) > 0);
}

#[test]
fn fitting_scales_down_when_too_wide() {
    let l = lay("graph LR\nA[很长的节点名称甲]-->B[很长的节点名称乙]-->C[很长的节点名称丙]\n");
    let narrow = l.width * 0.5;
    assert!(
        (fitted_height(&l, narrow) - l.height * 0.5).abs() < 0.01,
        "宽度不足时应等比缩小"
    );
    assert!(
        (fitted_height(&l, l.width * 2.0) - l.height).abs() < 0.01,
        "放得下就不该缩放"
    );
}

#[test]
fn wider_node_text_makes_wider_graph() {
    let narrow = lay("graph LR\nA-->B\n");
    let wide = lay("graph LR\nA[一段明显更长的节点文字]-->B\n");
    assert!(wide.width > narrow.width);
}

#[test]
fn every_hostile_input_is_an_error_not_a_panic() {
    // 一批「看起来像但又不对」的输入，全部只允许返回 Err。
    for src in [
        "",
        "\n\n",
        "graph",
        "%%只有注释",
        "graph LR\nA[[x]-->B",
        "graph LR\nA{{x}-->B",
        "graph LR\nA((x)-->B",
    ] {
        let f = m();
        let _ = layout(src, 14.0, &f);
    }
}
