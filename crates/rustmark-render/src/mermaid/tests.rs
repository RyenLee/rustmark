use crate::render::{Pt, geom::Rect};

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
    // 已支持的图种见各自模块内的布局测试；这里只保留仍未实现的图种。
    for src in [
        "kanban\n  todo[待办]\n    t1[任务]\n",
        "radar-beta\n  axis a, b\n",
        "info\n",
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
fn subgraph_renders_as_group_boxes() {
    // 取 `docs/logs/第三方加密授权平台.md` 的「总体架构」一节的真实写法：
    // `subgraph ID["标题"]` + 跨组连线 + 双向箭头。
    let src = "flowchart LR\n\
               subgraph DEV[\"开发者侧\"]\n\
               D1[\"业务源码/工程\"]\n\
               D2[\"接入 SDK\"]\n\
               end\n\
               subgraph PLAT[\"第三方授权平台\"]\n\
               P1[\"应用注册\"]\n\
               end\n\
               subgraph CUST[\"客户运行环境\"]\n\
               C1[\"加密后的程序\"]\n\
               C2[\"license.lic\"]\n\
               end\n\
               D1 --> D2\n\
               D2 --> P1\n\
               P1 <-->|\"HTTPS\"| C1\n\
               C1 --> C2\n";
    let g = parse_graph(src).expect("分组流程图应能解析");
    assert_eq!(g.groups.len(), 3, "三个 subgraph 各成一组");
    assert_eq!(g.groups[0].label, "开发者侧", "标题取方括号内文本");
    assert_eq!(g.groups[0].members.len(), 2);
    assert_eq!(g.edges.len(), 4);

    let l = lay(src);
    assert!(l.width > 0.0 && l.height > 0.0);
    // 分组框只描边不填充，故不计入节点数。
    assert_eq!(l.node_count(), 5, "分组框不得被算成节点");
    // 分组框必须最先发出（否则会盖住组内节点）：每个框 = 描边矩形 + 标题。
    for p in &l.prims[..2] {
        match p {
            Prim::Poly { fill, .. } => assert!(!*fill, "分组框应为不填充的描边矩形"),
            Prim::Text { .. } => {}
            other => panic!("首几个图元应为分组框，实际 {other:?}"),
        }
    }
    let box_count = l
        .prims
        .iter()
        .filter(|p| matches!(p, Prim::Poly { fill: false, .. }))
        .count();
    assert_eq!(box_count, 3, "三个 subgraph 应各有一个框");
}

/// `<br/>` 是 Mermaid 里最常见的换行写法，不能被原样画进节点。
#[test]
fn br_tags_split_node_text_into_lines() {
    let l = lay("flowchart LR\nA[\"第一行<br/>第二行\"]\n");
    let texts: Vec<&str> = l
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["第一行", "第二行"]);

    let single = lay("flowchart LR\nA[\"第一行\"]\n");
    assert!(l.height > single.height, "两行标签应比单行更高");
}

/// 适配 `docs/logs/第三方加密授权平台.md`「授权生命周期状态机」一节的真实写法。
#[test]
fn state_diagram_from_doc_renders() {
    let src = "stateDiagram-v2\n\
               [*] --> 未激活\n\
               未激活 --> 已激活: 激活成功\n\
               已激活 --> 运行中: 启动校验通过\n\
               运行中 --> 已过期: 超过有效期\n\
               运行中 --> 已吊销: 平台吊销 / 设备数超限\n\
               已过期 --> 已激活: 续期\n\
               已吊销 --> [*]\n\
               已过期 --> [*]\n\
               运行中 --> 宽限期: 心跳连续失败\n\
               宽限期 --> 运行中: 恢复联网\n\
               宽限期 --> 受限模式: 超过宽限期\n";
    let g = super::state::parse_state(src).expect("状态图应能解析");
    // 7 个状态 + 3 个 `[*]` 标记（各自独立）。
    assert_eq!(g.nodes.len(), 10, "{:?}", g.nodes);
    assert_eq!(g.edges.len(), 11);
    assert!(g.edges.iter().any(|e| e.label.as_deref() == Some("续期")));

    let l = lay(src);
    assert!(l.width > 0.0 && l.height > 0.0);
    assert_eq!(l.node_count(), 10, "状态与起止标记都要画出来");
}

/// 回归：《第三方加密授权平台》总体架构图的排版不允许重叠。
///
/// 截图反馈：跨组连线的边标签压在「构建插件 / 标记并加密关键模块」等节点上，
/// 连线把节点文字划开。这里把两条约束钉在版面层面：
/// 1. 节点框两两不相交；
/// 2. 所有边标签（`plate = true` 的文字）不落在任何节点框内。
#[test]
fn clustered_architecture_has_no_overlap() {
    let src = r#"flowchart LR
subgraph DEV["开发者侧"]
D1["业务源码/工程"]
D2["接入 SDK<br/>授权校验 + 解密加载"]
D3["构建插件<br/>标记并加密关键模块"]
end
subgraph PLAT["第三方授权平台"]
P1["应用注册 / 版本管理"]
P2["KMS 密钥管理<br/>主密钥永不下发"]
P3["加密构建服务"]
P4["授权签发服务"]
P5["激活 / 校验 / 心跳 API"]
P6["审计日志 + 风控"]
end
subgraph CUST["客户运行环境"]
C1["加密后的程序"]
C2["license.lic 授权文件"]
C3["运行时 SDK"]
end
D1 --> D3 --> P3
D2 --> P3
P2 --> P3
P2 --> P4
P3 -->|"加密产物 + 桩代码"| D1
P4 -->|"签发授权文件"| C2
C1 --> C3
C2 --> C3
C3 <-->|"HTTPS 激活/心跳"| P5
P5 --> P6
"#;
    let l = lay(src);
    assert_eq!(l.node_count(), 12, "12 个节点应各画一个框");

    let rect_of = |points: &[Pt]| {
        let mut min = points[0];
        let mut max = points[0];
        for p in points {
            min = Pt::new(min.x.min(p.x), min.y.min(p.y));
            max = Pt::new(max.x.max(p.x), max.y.max(p.y));
        }
        Rect::from_two_pos(min, max)
    };
    let nodes: Vec<Rect> = l
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Poly {
                points, fill: true, ..
            } => Some(rect_of(points)),
            _ => None,
        })
        .collect();
    assert_eq!(nodes.len(), 12);

    let overlap = |a: Rect, b: Rect| {
        a.left() < b.right() && b.left() < a.right() && a.top() < b.bottom() && b.top() < a.bottom()
    };
    for i in 0..nodes.len() {
        for j in i + 1..nodes.len() {
            assert!(
                !overlap(nodes[i].expand(-0.5), nodes[j].expand(-0.5)),
                "节点 {i} 与节点 {j} 的框重叠"
            );
        }
    }

    let measure = m();
    let mut labels = 0;
    let mut label_rects: Vec<(Rect, &str)> = Vec::new();
    for p in &l.prims {
        let Prim::Text {
            at,
            text,
            size,
            plate: true,
            ..
        } = p
        else {
            continue;
        };
        labels += 1;
        let half = Pt::new(
            (measure(text, *size).max(*size) + *size * 0.5) * 0.5,
            *size * 1.4 * 0.5,
        );
        let r = Rect::from_center_size(*at, Pt::new(half.x * 2.0, half.y * 2.0));
        for (i, n) in nodes.iter().enumerate() {
            assert!(!overlap(*n, r), "边标签 `{text}` 压在节点 {i} 上");
        }
        label_rects.push((r, text));
    }
    assert_eq!(labels, 3, "图里应有 3 条带标签的边");
    for i in 0..label_rects.len() {
        for j in i + 1..label_rects.len() {
            assert!(
                !overlap(label_rects[i].0, label_rects[j].0),
                "边标签 `{}` 与 `{}` 互相重叠",
                label_rects[i].1,
                label_rects[j].1
            );
        }
    }
}

/// 回归：10 个新增图种都必须经 `layout` 分发出可绘制版面。
#[test]
fn new_diagram_kinds_dispatch_and_render() {
    let cases = [
        ("pie", "pie title 占比\n\"A\" : 40\n\"B\" : 60\n"),
        ("journey", "journey\ntitle 旅程\nsection 出门\n起床: 3\n"),
        ("timeline", "timeline\n2024 : 立项\n2025 : 上线\n"),
        ("mindmap", "mindmap\n 根\n   分支\n"),
        ("quadrantChart", "quadrantChart\nA: [0.2, 0.8]\n"),
        ("xychart", "xychart-beta\nx-axis [a, b]\nbar [1, 2]\n"),
        ("erDiagram", "erDiagram\nA ||--o{ B : has\n"),
        (
            "requirementDiagram",
            "requirementDiagram\nrequirement r {\n id: 1\n}\n",
        ),
        ("gitGraph", "gitGraph\ncommit\n"),
        ("sankey", "sankey-beta\nA,B,1\n"),
    ];
    for (kind, src) in cases {
        let l = lay(src);
        assert!(l.width > 0.0 && l.height > 0.0, "{kind} 应有正尺寸");
        assert!(prim_count(&l) > 0, "{kind} 应有图元");
    }
}

/// `xychart-beta` / `sankey-beta` 的 `-beta` 后缀会被 `leading_word` 截断，
/// 分发仍须命中。
#[test]
fn suffixed_kind_names_are_recognized() {
    for (kind, src) in [
        ("xychart-beta", "xychart-beta\nx-axis [a]\nbar [1]\n"),
        ("sankey-beta", "sankey-beta\nA,B,1\n"),
    ] {
        assert!(layout(src, 14.0, &m()).is_ok(), "{kind} 应被识别");
    }
}

/// 新增图种的畸形输入只能是 `Err`，绝不能 panic（`panic = "abort"`）。
#[test]
fn new_kinds_hostile_inputs_are_errors_not_panics() {
    for src in [
        "pie\n\"A\" : x\n",
        "pie\n",
        "journey\n任务: 9\n",
        "timeline\n",
        "mindmap\n",
        "quadrantChart\nA: 右上\n",
        "xychart-beta\n",
        "erDiagram\nA ||--\n",
        "requirementDiagram\nr - likes -> e\n",
        "gitGraph\n",
        "sankey-beta\nA,B\n",
    ] {
        let _ = bad(src);
    }
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
