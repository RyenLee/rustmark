//! 排版主逻辑：`layout_nodes`（公开入口）与 `layout_node`（单节点分发）。
//!
//! 从 `super::layout` 剪切而来，保持零改动。

use super::*;

/// 排版一棵语法树。
///
/// `size` 是**当前层**字号；上下标经 [`Ctx::script_size`] 收缩。
pub(crate) fn layout_nodes(nodes: &[Node], size: f32, ctx: &Ctx<'_>) -> Layout {
    let mut row = Layout::default();
    let mut prev: Option<&Node> = None;
    for node in nodes {
        // 函数名与前面的变量之间留一点空隙（`\sin x` 而不是 `sinx`）。
        if matches!(node, Node::Fn(_)) && prev.is_some() {
            row.w += size * 0.12;
        }
        let box_ = layout_node(node, size, ctx);
        row.push(&box_, row.w, 0.0);
        row.grow(box_.asc, box_.desc);
        prev = Some(node);
    }
    row
}

/// 排版单个节点。
pub(crate) fn layout_node(node: &Node, size: f32, ctx: &Ctx<'_>) -> Layout {
    match node {
        Node::Row(inner) => layout_nodes(inner, size, ctx),
        Node::Text { text, font } => {
            let w = (ctx.measure)(text, size);
            Layout {
                w,
                // 文本的高度直接用字号近似：真实字体的 ascent 因字符而异，
                // 而这里只需要「一个够用的盒子」——用固定比例反而不受字体影响。
                asc: size * 0.78,
                desc: size * 0.22,
                items: vec![Item::Text {
                    x: 0.0,
                    y: 0.0,
                    text: text.clone(),
                    size,
                    font: *font,
                }],
            }
        }
        Node::Fn(name) => {
            let w = (ctx.measure)(name, size);
            Layout {
                w,
                asc: size * 0.78,
                desc: size * 0.22,
                items: vec![Item::Text {
                    x: 0.0,
                    y: 0.0,
                    text: name.clone(),
                    size,
                    font: MathFont::Upright,
                }],
            }
        }
        Node::Space(em) => Layout {
            w: size * em,
            ..Default::default()
        },
        Node::BigOp { glyph, limits } => {
            // 大运算符放大到 1.35 倍：不放大就与普通符号分不出主次。
            let big = size * 1.35;
            let w = (ctx.measure)(glyph, big);
            let asc = big * 0.72;
            let desc = big * 0.28;
            let _ = limits;
            Layout {
                w,
                asc,
                desc,
                items: vec![Item::Text {
                    x: 0.0,
                    y: 0.0,
                    text: glyph.clone(),
                    size: big,
                    font: MathFont::Upright,
                }],
            }
        }
        Node::Script {
            base,
            sup,
            sub,
            stacked,
        } => {
            let b = layout_node(base, size, ctx);
            let small = ctx.script_size(size);
            let sup_box = sup.as_ref().map(|s| layout_nodes(s, small, ctx));
            let sub_box = sub.as_ref().map(|s| layout_nodes(s, small, ctx));

            if *stacked {
                // 上下限叠放（display 模式的 `\sum`）：符号居中，限在上下。
                let lim_w = sup_box
                    .as_ref()
                    .map_or(0.0, |b| b.w)
                    .max(sub_box.as_ref().map_or(0.0, |b| b.w));
                let w = b.w.max(lim_w);
                let mut items = Vec::new();
                let mut asc = b.asc;
                let mut desc = b.desc;
                // 运算符居中。
                let bcx = (w - b.w) * 0.5;
                items.extend(b.offset_items(bcx, 0.0));
                if let Some(s) = &sup_box {
                    let dy = -(b.asc + size * LIMIT_GAP + s.desc);
                    let x = (w - s.w) * 0.5;
                    items.extend(s.offset_items(x, dy));
                    asc = asc.max(-dy + s.asc);
                }
                if let Some(s) = &sub_box {
                    let dy = b.desc + size * LIMIT_GAP + s.asc;
                    let x = (w - s.w) * 0.5;
                    items.extend(s.offset_items(x, dy));
                    desc = desc.max(dy + s.desc);
                }
                return Layout {
                    w,
                    asc,
                    desc,
                    items,
                };
            }

            // 常规角标：贴在上/右下角，**不**撑高基准盒的高度
            // （否则 `x^2` 会把整行行距顶开，与 TeX 的观感不符）。
            let mut w = b.w;
            let mut items = b.offset_items(0.0, 0.0);
            let mut asc = b.asc;
            let mut desc = b.desc;
            if let Some(s) = &sup_box {
                let x = b.w + size * 0.02;
                let dy = -size * SUP_SHIFT;
                items.extend(s.offset_items(x, dy));
                w = w.max(x + s.w);
                asc = asc.max(-dy + s.asc);
            }
            if let Some(s) = &sub_box {
                let x = b.w + size * 0.02;
                let dy = size * SUB_SHIFT;
                items.extend(s.offset_items(x, dy));
                w = w.max(x + s.w);
                desc = desc.max(dy + s.desc);
            }
            Layout {
                w,
                asc,
                desc,
                items,
            }
        }
        Node::Frac { num, den, big } => {
            // `\tfrac` 的分数字号缩一档（行内 `\frac` 视觉上更轻）。
            let size = if *big { size } else { size * 0.85 };
            let n = layout_nodes(num, size, ctx);
            let d = layout_nodes(den, size, ctx);
            let w = n.w.max(d.w) + size * 0.3;
            let gap = size * FRAC_GAP;
            let thick = (size * RULE_THICK).max(1.0);
            // 分数线在基线上方一点：TeX 把分数线放在数学轴（≈ 基线 + 0.25em）处。
            let axis = -size * 0.25;
            let mut items = Vec::new();
            items.extend(n.offset_items((w - n.w) * 0.5, axis - gap - n.desc));
            items.extend(d.offset_items((w - d.w) * 0.5, axis + gap + d.asc));
            items.push(Item::Rect {
                x: 0.0,
                y: axis - thick * 0.5,
                w,
                h: thick,
            });
            let asc = -axis + gap + n.asc + n.desc + gap;
            let desc = axis + gap + d.asc + d.desc + gap;
            Layout {
                w,
                asc,
                desc,
                items,
            }
        }
        Node::Sqrt { index, body } => {
            let b = layout_nodes(body, size, ctx);
            let hook = size * SQRT_HOOK_RATIO;
            let gap = size * SQRT_GAP;
            let thick = (size * RULE_THICK).max(1.0);
            // 被开方内容整体上移「勾的高度」，让勾的尖点落在它的左缘。
            let dy = -hook;
            let mut items = Vec::new();
            let mut x0 = 0.0;
            let mut asc = b.asc + hook;
            if let Some(idx) = index {
                let i = layout_nodes(idx, ctx.script_size(size), ctx);
                items.extend(i.offset_items(0.0, -b.asc - i.desc + hook * 0.2));
                x0 = i.w + size * 0.04;
                asc = asc.max(b.asc + hook + i.asc);
            }
            let top = dy - b.asc;
            let bottom = b.desc;
            // 勾：左下要点 → 折点 → 顶部横线。
            let notch = x0 + hook * 0.5;
            items.push(Item::Line {
                from: (x0, -bottom * 0.35),
                to: (notch * 0.5 + x0 * 0.5, bottom * 0.55),
                width: thick * 1.6,
            });
            items.push(Item::Line {
                from: (notch * 0.5 + x0 * 0.5, bottom * 0.55),
                to: (x0 + hook, top),
                width: thick * 1.6,
            });
            items.push(Item::Line {
                from: (x0 + hook, top),
                to: (x0 + hook + b.w + gap, top),
                width: thick,
            });
            items.extend(b.offset_items(x0 + hook + gap, dy));
            let w = x0 + hook + gap + b.w + gap;
            Layout {
                w,
                asc: asc.max(-top),
                desc: bottom.max(0.0),
                items,
            }
        }
        Node::Delim { left, right, body } => {
            let b = layout_nodes(body, size, ctx);
            // 括号按内容高度**伸缩**：内容比单行高时把括号字号放大，
            // 并上下拉伸（这里用「放大字号」近似拉伸——字形拉伸需要矢量轮廓，
            // 而放大字号在视觉上已经足够，且不会把括号画成两条平行线）。
            let content_h = b.asc + b.desc;
            let scale = (content_h / (size * 1.0)).clamp(1.0, 2.6);
            let dsize = size * scale;
            let mut items = Vec::new();
            let mut x = 0.0;
            let mut asc = b.asc;
            let mut desc = b.desc;
            if !left.is_empty() {
                let lw = (ctx.measure)(left, dsize);
                // 括号基线对齐到内容的中线，而不是内容的基线：
                // 内容叠得越高，括号越要「居中包裹」才像成对的。
                let mid = (b.asc - b.desc) * 0.5;
                items.push(Item::Text {
                    x: 0.0,
                    y: -mid,
                    text: left.clone(),
                    size: dsize,
                    font: MathFont::Upright,
                });
                asc = asc.max(dsize * 0.5 + mid.abs());
                desc = desc.max(dsize * 0.5 - mid);
                x += lw + size * 0.05;
            }
            items.extend(b.offset_items(x, 0.0));
            x += b.w + size * 0.05;
            if !right.is_empty() {
                let mid = (b.asc - b.desc) * 0.5;
                items.push(Item::Text {
                    x,
                    y: -mid,
                    text: right.clone(),
                    size: dsize,
                    font: MathFont::Upright,
                });
                let rw = (ctx.measure)(right, dsize);
                asc = asc.max(dsize * 0.5 + mid.abs());
                desc = desc.max(dsize * 0.5 - mid);
                x += rw;
            }
            Layout {
                w: x,
                asc,
                desc,
                items,
            }
        }
        Node::Accent { kind, body } => {
            let b = layout_nodes(body, size, ctx);
            let mut items = b.offset_items(0.0, 0.0);
            let cx = b.w * 0.5;
            let top = -b.asc - size * 0.08;
            let thick = (size * RULE_THICK * 1.2).max(1.0);
            let mut extra = 0.0;
            match kind {
                Accent::Bar => items.push(Item::Rect {
                    x: 0.0,
                    y: top - thick,
                    w: b.w,
                    h: thick,
                }),
                Accent::Hat => {
                    let half = (b.w * 0.45).max(size * 0.18);
                    items.push(Item::Line {
                        from: (cx - half, top),
                        to: (cx, top - size * 0.22),
                        width: thick,
                    });
                    items.push(Item::Line {
                        from: (cx, top - size * 0.22),
                        to: (cx + half, top),
                        width: thick,
                    });
                    extra = size * 0.22;
                }
                Accent::Vec => {
                    let half = (b.w * 0.45).max(size * 0.2);
                    items.push(Item::Line {
                        from: (cx - half, top),
                        to: (cx + half, top),
                        width: thick,
                    });
                    // 箭头两撇。
                    items.push(Item::Line {
                        from: (cx + half, top),
                        to: (cx + half - size * 0.16, top - size * 0.12),
                        width: thick,
                    });
                    items.push(Item::Line {
                        from: (cx + half, top),
                        to: (cx + half - size * 0.16, top + size * 0.12),
                        width: thick,
                    });
                }
                Accent::Dot => items.push(Item::Rect {
                    x: cx - thick,
                    y: top - thick,
                    w: thick * 2.0,
                    h: thick * 2.0,
                }),
                Accent::Tilde => {
                    let half = (b.w * 0.45).max(size * 0.2);
                    items.push(Item::Line {
                        from: (cx - half, top),
                        to: (cx - half * 0.3, top - size * 0.14),
                        width: thick,
                    });
                    items.push(Item::Line {
                        from: (cx - half * 0.3, top - size * 0.14),
                        to: (cx + half * 0.3, top),
                        width: thick,
                    });
                    items.push(Item::Line {
                        from: (cx + half * 0.3, top),
                        to: (cx + half, top - size * 0.14),
                        width: thick,
                    });
                    extra = size * 0.14;
                }
            }
            Layout {
                w: b.w,
                asc: b.asc + size * 0.12 + extra,
                desc: b.desc,
                items,
            }
        }
        Node::Matrix {
            open,
            close,
            rows,
            left_align_cols,
        } => {
            let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
            let col_gap = size * 0.6;
            let row_gap = size * 0.35;
            // 先排每个格子，再按列取最大宽 / 按行取最大高。
            let cells: Vec<Vec<Layout>> = rows
                .iter()
                .map(|row| row.iter().map(|c| layout_nodes(c, size, ctx)).collect())
                .collect();
            let mut col_w = vec![0.0_f32; cols];
            for row in &cells {
                for (c, cell) in row.iter().enumerate() {
                    col_w[c] = col_w[c].max(cell.w);
                }
            }
            let rows_h: Vec<f32> = cells
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|c| c.asc + c.desc)
                        .fold(0.0_f32, f32::max)
                        .max(size)
                })
                .collect();
            let total_w: f32 = col_w.iter().sum::<f32>() + col_gap * cols.saturating_sub(1) as f32;
            let total_h: f32 =
                rows_h.iter().sum::<f32>() + row_gap * rows_h.len().saturating_sub(1) as f32;

            // 内容整体居中在基线两侧：矩阵通常作为整体出现，居中最好看。
            let content_asc = total_h * 0.55;
            let content_desc = total_h - content_asc;

            let mut items = Vec::new();
            let mut y = -content_asc;
            for (r, row) in cells.iter().enumerate() {
                let h = rows_h[r];
                // 本行内各项的基线：按各自 ascent 顶部对齐后居中。
                let row_top = y;
                let mut x = 0.0;
                for (c, cell) in row.iter().enumerate() {
                    let cell_asc = h * 0.5 + (cell.asc - cell.desc) * 0.5;
                    let dx = if *left_align_cols && c + 1 == cols {
                        // `cases` 的第二列（条件）左对齐贴住第一列。
                        0.0
                    } else {
                        (col_w[c] - cell.w) * 0.5
                    };
                    items.extend(cell.offset_items(x + dx, row_top + cell_asc));
                    x += col_w[c] + col_gap;
                }
                y += h + row_gap;
            }

            // 外括号。
            let mut asc = content_asc;
            let mut desc = content_desc;
            let w = total_w;
            let mut total = 0.0;
            if !open.is_empty() || !close.is_empty() {
                let scale = ((total_h + size * 0.4) / size).clamp(1.0, 3.6);
                let dsize = size * scale;
                let mid = (content_asc - content_desc) * 0.5;
                let mut shifted: Vec<Item> = Vec::new();
                if !open.is_empty() {
                    let lw = (ctx.measure)(open, dsize);
                    shifted.push(Item::Text {
                        x: 0.0,
                        y: -mid,
                        text: open.clone(),
                        size: dsize,
                        font: MathFont::Upright,
                    });
                    shifted.extend(items.iter().map(|i| shift(i, lw + size * 0.06, 0.0)));
                    total += lw + size * 0.06;
                } else {
                    shifted.extend(items.iter().cloned());
                }
                total += w;
                if !close.is_empty() {
                    let rw = (ctx.measure)(close, dsize);
                    shifted.push(Item::Text {
                        x: total,
                        y: -mid,
                        text: close.clone(),
                        size: dsize,
                        font: MathFont::Upright,
                    });
                    total += rw;
                }
                asc = asc.max(dsize * 0.5 + mid.abs());
                desc = desc.max(dsize * 0.5 - mid);
                return Layout {
                    w: total,
                    asc,
                    desc,
                    items: shifted,
                };
            }

            Layout {
                w: total_w,
                asc,
                desc,
                items,
            }
        }
    }
}
