//! 纯图形图标系统：工具栏按钮的「字形 + 文字」组合 → **向量手绘**的纯图形图标。
//!
//! # 为什么废止「字形图标」
//!
//! 原实现（`UiAction::icon`）用**字体字形**当图标（`B` / `I` / `<>` / `🔗`…）。
//! 它在两个方向上都不及格：
//!
//! 1. **依赖字体覆盖**：字形一旦落在已装载字体之外，界面上就是一个豆腐块
//!    方框——比没有图标更糟（实测 `▾ ◂ ▦ ⌫ ⇥ ⤒ ↹ ✓` 等一批符号无字形）。
//!    覆盖率还随平台变化，只能逐台实测，无法静态保证。
//! 2. **表达力贫乏**：单字形只能是「一个字符」，谈不上配色、层次与风格。
//!
//! 现在图标全部改为 [`Painter`] 向量绘制：几何在代码里，渲染只依赖 egui 本身，
//! 跨平台一致，且**每一个像素都可被测试断言**（图元数量、外包矩形、居中度）。
//!
//! # 图标规格 = 数据，不是代码分支
//!
//! 每个图标是一张 [`IconOp`] 表（[`spec`] 取值）。表里每个图元自带两维自由度：
//!
//! - **绘制方式**（[`Paint`]）：描边（线稿）或实心（色块）——线稿图标与
//!   实心图标可以出现在同一套 UI 里而风格不散；
//! - **配色角色**（[`Tone`]）：主色（随控件文字色，悬停/按下自动换档）或
//!   强调色（主题强调色，用来点出图标里「最该被看到」的那一笔）。
//!
//! 于是「多样式组合」不是口头承诺：新图标 = 往表里追加几行图元，
//! 描边/实心、主色/强调色自由搭配，无需改渲染器一行代码。
//! 渲染器（[`paint_icon`]）只做一件事：把 16×16 设计栅格等距缩放进目标矩形。
//!
//! 现有规格以**描边**为主是刻意的：16pt 量级上实心色块会糊成一片，
//! 线稿的呼吸感更接近专业编辑器的工具栏。[`Paint::Fill`] / [`IconOp::Circle`]
//! 等实心图元为更大尺寸的场景（欢迎页主操作、关于页）预留，随场景扩展。
//!
//! # 设计栅格
//!
//! 全部坐标写在一个 **16×16** 的设计栅格里（[`GRID`]），y 向下，
//! 与屏幕像素无关；缩放由渲染器统一完成，改图标尺寸只改调用方的矩形。
//! 栅格纪律由单元测试把守：任何图元不得越界（[`icon_ops_stay_inside_the_grid`]）。

use egui::epaint::{
    CubicBezierShape, PathShape, PathStroke, QuadraticBezierShape, Stroke, StrokeKind,
};
use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Vec2};

use crate::presentation::shortcuts::registry::UiAction;

// ============================================================================
// 图元 DSL
// ============================================================================

/// 图元的绘制方式：描边（线稿）或实心（色块）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Paint {
    /// 只画轮廓。
    Stroke,
    /// 填满区域。
    Fill,
}

/// 图元的配色角色。
///
/// 分离「画什么」与「用什么颜色画」：颜色由调用方按场景给（见 [`IconColors`]），
/// 图标规格因此与主题完全解耦——同一张规格表在深色/浅色/自定义主题下都成立。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    /// 主色：控件的前景色（悬停/按下时随 egui 的 `fg_stroke` 换档）。
    Primary,
    /// 强调色：主题强调色，点出图标里语义最重的那一笔。
    Accent,
}

/// 设计栅格上的点（16×16，y 向下）。
pub(crate) type GridPt = (f32, f32);

/// 一枚图元（设计栅格坐标）。
///
/// 每种图元都可以选 [`Paint`] 与 [`Tone`]（贝塞尔曲线天然只能描边）——
/// 这就是「融合多种视觉风格、配色与图形元素」的落地形式。
pub enum IconOp {
    /// 直线段。
    Line {
        /// 起点（设计栅格坐标）。
        from: GridPt,
        /// 终点（设计栅格坐标）。
        to: GridPt,
        /// 设计栅格单位下的线宽。
        w: f32,
        /// 配色角色。
        tone: Tone,
    },
    /// 折线（`closed` 时首尾自动闭合）。
    Polyline {
        /// 依次连接的折线点（设计栅格坐标）。
        pts: Vec<GridPt>,
        /// 是否把首尾相连闭合成环。
        closed: bool,
        /// 设计栅格单位下的线宽（仅描边时生效）。
        w: f32,
        /// 描边或实心填充（实心要求点集构成凸多边形）。
        paint: Paint,
        /// 配色角色。
        tone: Tone,
    },
    /// 二次贝塞尔弧（曲线字形，如有序列表的 `2` / `3`）。
    Quad {
        /// 起点（设计栅格坐标）。
        from: GridPt,
        /// 控制点：曲线被其「吸引」但不经过它。
        ctrl: GridPt,
        /// 终点（设计栅格坐标）。
        to: GridPt,
        /// 设计栅格单位下的线宽。
        w: f32,
        /// 配色角色。
        tone: Tone,
    },
    /// 三次贝塞尔弧（S / B 这类有两个反向弯曲的字形）。
    Cubic {
        /// 依次为起点、两枚控制点、终点（曲线只经过首尾两点）。
        pts: [GridPt; 4],
        /// 设计栅格单位下的线宽。
        w: f32,
        /// 配色角色。
        tone: Tone,
    },
    /// 圆。
    Circle {
        /// 圆心（设计栅格坐标）。
        c: GridPt,
        /// 半径（设计栅格单位）。
        r: f32,
        /// 描边或实心填充。
        paint: Paint,
        /// 仅描边时使用。
        w: f32,
        /// 配色角色。
        tone: Tone,
    },
    /// 圆角矩形。
    RoundRect {
        /// 左上角 x（设计栅格坐标）。
        x: f32,
        /// 左上角 y（设计栅格坐标）。
        y: f32,
        /// 宽（设计栅格单位）。
        w: f32,
        /// 高（设计栅格单位）。
        h: f32,
        /// 圆角半径（设计栅格单位）。
        r: f32,
        /// 描边或实心填充。
        paint: Paint,
        /// 仅描边时使用。
        stroke_w: f32,
        /// 配色角色。
        tone: Tone,
    },
}

/// 图标配色（由调用方按场景从主题派生）。
#[derive(Clone, Copy, Debug)]
pub struct IconColors {
    /// 主色（对应 [`Tone::Primary`]）。
    pub primary: Color32,
    /// 强调色（对应 [`Tone::Accent`]）。
    pub accent: Color32,
}

// ============================================================================
// 图标标识与规格表
// ============================================================================

/// 设计栅格边长。
pub(crate) const GRID: f32 = 16.0;

/// 图标标识：按**场景**扩展（动作图标、文档图标……）。
///
/// 新增场景 = 加一个变体 + 在 [`spec`] 里给一张规格表，渲染器与调用方都不动。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Icon {
    /// 与某个 [`UiAction`] 绑定的图标（语义必须与该动作一致）。
    Action(UiAction),
    /// 文档 / 文件（面包屑等「这是一个文件」的场景）。
    Document,
    /// 侧栏（收起/展开切换）：「主内容 + 左栏」的布局语言。
    Sidebar,
}

/// 规格表缓存：首帧建表，之后每帧零分配。
///
/// 工具栏每帧都要取 18 张规格表，若每次现算（`Vec` 图元 + 弧线采样点），
/// 每帧会白扔一堆小分配——与项目「每帧热路径不造新分配」的纪律同源。
static SPECS: std::sync::OnceLock<Vec<(Icon, Vec<IconOp>)>> = std::sync::OnceLock::new();

/// 取一枚图标的规格表（没有规格时返回空表——调用方据此判定「该动作不配图标」）。
pub fn spec(icon: Icon) -> &'static [IconOp] {
    SPECS
        .get_or_init(build_specs)
        .iter()
        .find(|(key, _)| *key == icon)
        .map(|(_, ops)| ops.as_slice())
        .unwrap_or(&[])
}

/// 把角度（度，0 = +x，正方向顺时针——栅格 y 向下）离散成折线点。
///
/// 弧线（链环的缺口）没有对应的 egui 图元，按 12 段折线逼近：16pt 尺寸下
/// 肉眼与真弧无差异，且形状可被单元测试逐点断言。
fn arc(cx: f32, cy: f32, r: f32, from_deg: f32, to_deg: f32) -> Vec<GridPt> {
    const SEGMENTS: usize = 12;
    let mut pts = Vec::with_capacity(SEGMENTS + 1);
    for i in 0..=SEGMENTS {
        let t = i as f32 / SEGMENTS as f32;
        let a = (from_deg + (to_deg - from_deg) * t).to_radians();
        pts.push((cx + r * a.cos(), cy + r * a.sin()));
    }
    pts
}

/// 建全部规格表（只跑一次，见 [`SPECS`]）。
fn build_specs() -> Vec<(Icon, Vec<IconOp>)> {
    let line = |from: GridPt, to: GridPt, w: f32, tone: Tone| IconOp::Line { from, to, w, tone };
    let poly =
        |pts: Vec<GridPt>, closed: bool, w: f32, paint: Paint, tone: Tone| IconOp::Polyline {
            pts,
            closed,
            w,
            paint,
            tone,
        };
    let cubic = |pts: [GridPt; 4], w: f32, tone: Tone| IconOp::Cubic { pts, w, tone };
    let round_rect =
        |x: f32, y: f32, w: f32, h: f32, r: f32, paint: Paint, stroke_w: f32, tone: Tone| {
            IconOp::RoundRect {
                x,
                y,
                w,
                h,
                r,
                paint,
                stroke_w,
                tone,
            }
        };

    vec![
        // ── 格式组（平铺图标按钮）────────────────────────────────────────
        //
        // **加粗**：字母 B 的**加粗**字身。用粗字身而不是细字身，图形自己
        // 就在说「粗」——语义不靠配色传达，缩小到 16pt 仍可辨。
        (
            Icon::Action(UiAction::Bold),
            vec![
                line((4.6, 2.6), (4.6, 13.4), 2.3, Tone::Primary),
                cubic(
                    [(4.6, 2.6), (10.8, 2.6), (11.6, 6.8), (4.6, 7.3)],
                    2.0,
                    Tone::Primary,
                ),
                cubic(
                    [(4.6, 7.3), (12.4, 7.7), (12.8, 12.8), (4.6, 13.4)],
                    2.0,
                    Tone::Primary,
                ),
            ],
        ),
        // **斜体**：右斜的 I，带上下衬线。倾斜本身就是「斜」的字形语言。
        (
            Icon::Action(UiAction::Italic),
            vec![
                line((5.8, 3.9), (12.6, 2.5), 1.5, Tone::Primary),
                line((9.2, 3.2), (6.8, 12.8), 1.6, Tone::Primary),
                line((3.4, 13.5), (10.2, 12.1), 1.5, Tone::Primary),
            ],
        ),
        // **删除线**：字母 S 拦腰一道横线——「文字被划掉」的直译。
        (
            Icon::Action(UiAction::Strikethrough),
            vec![
                cubic(
                    [(11.8, 4.4), (8.8, 2.8), (5.6, 4.2), (6.2, 7.4)],
                    1.5,
                    Tone::Primary,
                ),
                cubic(
                    [(6.2, 7.4), (10.2, 8.4), (10.4, 10.6), (5.6, 11.8)],
                    1.5,
                    Tone::Primary,
                ),
                line((2.6, 7.9), (13.4, 7.9), 1.6, Tone::Primary),
            ],
        ),
        // **行内代码**：一对尖括号 + 一道斜杠。斜杠取强调色——同一枚图标里
        // 主色/强调色两种角色，示范「配色组合」的用法。
        (
            Icon::Action(UiAction::InlineCode),
            vec![
                poly(
                    vec![(5.4, 3.8), (2.4, 8.0), (5.4, 12.2)],
                    false,
                    1.5,
                    Paint::Stroke,
                    Tone::Primary,
                ),
                poly(
                    vec![(10.6, 3.8), (13.6, 8.0), (10.6, 12.2)],
                    false,
                    1.5,
                    Paint::Stroke,
                    Tone::Primary,
                ),
                line((9.4, 3.4), (6.6, 12.6), 1.4, Tone::Accent),
            ],
        ),
        // **插入链接**：两枚相扣的链环。左环主色、右环强调色——「链接」由
        // 两个部件咬合而成，双色让咬合处一眼可辨。
        (
            Icon::Action(UiAction::InsertLink),
            vec![
                poly(
                    arc(5.6, 8.0, 3.1, 30.0, 330.0),
                    false,
                    1.5,
                    Paint::Stroke,
                    Tone::Primary,
                ),
                poly(
                    arc(10.4, 8.0, 3.1, 210.0, 510.0),
                    false,
                    1.5,
                    Paint::Stroke,
                    Tone::Accent,
                ),
            ],
        ),
        // ── 文档场景 ────────────────────────────────────────────────────
        //
        // **文档**：一页纸 + 两行文字。用于面包屑前缀（替代原 📄 字形——
        // emoji 依赖字体覆盖，且与文件树里手绘的页面图标不同源）。
        (
            Icon::Document,
            vec![
                round_rect(3.6, 2.4, 8.8, 11.2, 1.2, Paint::Stroke, 1.4, Tone::Primary),
                line((6.0, 5.8), (10.0, 5.8), 1.2, Tone::Primary),
                line((6.0, 8.4), (10.0, 8.4), 1.2, Tone::Primary),
            ],
        ),
        // **侧栏**：圆角外框 + 左 1/3 的竖向分割线——「主内容 + 左栏」的
        // 布局语言（VS Code 的 layout-sidebar-left 同款拓扑）。收起 / 展开
        // 共用这一枚：图形表意「侧栏」，状态由悬停提示文案区分——
        // 方向性箭头（« / »）在两种状态间跳动会让人找不到按钮。
        (
            Icon::Sidebar,
            vec![
                round_rect(2.4, 3.0, 11.2, 10.0, 1.5, Paint::Stroke, 1.4, Tone::Primary),
                line((6.2, 3.0), (6.2, 13.0), 1.4, Tone::Primary),
            ],
        ),
    ]
}

// ============================================================================
// 渲染器
// ============================================================================

/// 把图标画进 `rect`：等比缩放到 [`GRID`] 设计栅格并居中。
///
/// 颜色来自 [`IconColors`]——调用方决定「这个场景下主色/强调色是什么」，
/// 渲染器只认角色不认色值，因此同一张规格表可直接复用于工具栏按钮、
/// 面包屑等不同场景（[`Icon`] 按场景扩展）。
pub fn paint_icon(painter: &Painter, icon: Icon, rect: Rect, colors: IconColors) {
    let ops = spec(icon);
    if ops.is_empty() {
        return;
    }
    let scale = rect.width().min(rect.height()) / GRID;
    if scale <= 0.0 {
        return;
    }
    // 栅格原点：让 16×16 的设计栅格中心落在 rect 中心。
    let origin = rect.center() - Vec2::splat(GRID * scale / 2.0);
    let p = |g: GridPt| -> Pos2 { origin + Vec2::new(g.0 * scale, g.1 * scale) };
    let tone = |t: Tone| -> Color32 {
        match t {
            Tone::Primary => colors.primary,
            Tone::Accent => colors.accent,
        }
    };
    let path_stroke = |w: f32, t: Tone| PathStroke::new(w * scale, tone(t));

    for op in ops {
        match op {
            IconOp::Line {
                from,
                to,
                w,
                tone: t,
            } => {
                painter.line_segment([p(*from), p(*to)], Stroke::new(w * scale, tone(*t)));
            }
            IconOp::Polyline {
                pts,
                closed,
                w,
                paint: pa,
                tone: t,
            } => {
                let points: Vec<Pos2> = pts.iter().map(|g| p(*g)).collect();
                let shape = match pa {
                    Paint::Stroke => Shape::Path(if *closed {
                        PathShape::closed_line(points, path_stroke(*w, *t))
                    } else {
                        PathShape::line(points, path_stroke(*w, *t))
                    }),
                    // 实心折线 = 凸多边形（图元表里的封闭折线都保证凸性）。
                    Paint::Fill => Shape::convex_polygon(points, tone(*t), PathStroke::NONE),
                };
                painter.add(shape);
            }
            IconOp::Quad {
                from,
                ctrl,
                to,
                w,
                tone: t,
            } => {
                painter.add(Shape::QuadraticBezier(
                    QuadraticBezierShape::from_points_stroke(
                        [p(*from), p(*ctrl), p(*to)],
                        false,
                        Color32::TRANSPARENT,
                        path_stroke(*w, *t),
                    ),
                ));
            }
            IconOp::Cubic { pts, w, tone: t } => {
                painter.add(Shape::CubicBezier(CubicBezierShape::from_points_stroke(
                    [p(pts[0]), p(pts[1]), p(pts[2]), p(pts[3])],
                    false,
                    Color32::TRANSPARENT,
                    path_stroke(*w, *t),
                )));
            }
            IconOp::Circle {
                c,
                r,
                paint: pa,
                w,
                tone: t,
            } => match pa {
                Paint::Fill => {
                    painter.circle_filled(p(*c), r * scale, tone(*t));
                }
                Paint::Stroke => {
                    painter.circle_stroke(p(*c), r * scale, Stroke::new(w * scale, tone(*t)));
                }
            },
            IconOp::RoundRect {
                x,
                y,
                w: rw,
                h,
                r,
                paint: pa,
                stroke_w,
                tone: t,
            } => {
                let rect = Rect::from_min_size(p((*x, *y)), Vec2::new(rw * scale, h * scale));
                let radius = CornerRadius::same((r * scale).round().clamp(0.0, 255.0) as u8);
                match pa {
                    Paint::Fill => {
                        painter.rect_filled(rect, radius, tone(*t));
                    }
                    Paint::Stroke => {
                        painter.rect(
                            rect,
                            radius,
                            Color32::TRANSPARENT,
                            Stroke::new(stroke_w * scale, tone(*t)),
                            StrokeKind::Middle,
                        );
                    }
                }
            }
        }
    }
}

// ============================================================================
// 单元测试：规格表的数据纪律
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个有规格的图标都必须**非空**——空表画出来等于没有图标，
    /// 而「按钮上看不出任何图形」正是本次改造要消灭的缺陷。
    #[test]
    fn every_icon_with_a_spec_is_nonempty() {
        for icon in [
            Icon::Action(UiAction::Bold),
            Icon::Action(UiAction::Italic),
            Icon::Action(UiAction::Strikethrough),
            Icon::Action(UiAction::InlineCode),
            Icon::Action(UiAction::InsertLink),
            Icon::Document,
            Icon::Sidebar,
        ] {
            assert!(!spec(icon).is_empty(), "{icon:?} 的规格表是空的");
        }
    }

    /// 规格表必须**稳定且零分配**：同一图标两次取到的是同一块内存。
    ///
    /// 这条把守的是每帧热路径——若 `spec` 每次现算，工具栏每帧会白扔
    /// 若干 `Vec` 分配（弧线采样点尤其明显）。
    #[test]
    fn spec_lookup_is_stable_and_allocation_free() {
        let a = spec(Icon::Action(UiAction::Bold));
        let b = spec(Icon::Action(UiAction::Bold));
        assert!(
            std::ptr::eq(a, b),
            "同一图标的两次取值不是同一块内存——缓存没生效"
        );
    }

    /// 所有图元必须落在 16×16 设计栅格内。
    ///
    /// 越界的图元会被渲染器画到图标框外：轻则与相邻控件粘连，重则超出
    /// 按钮热区——而这类偏移在 16pt 尺寸上肉眼极难定位。
    #[test]
    fn icon_ops_stay_inside_the_grid() {
        let eps = 0.01;
        let check = |x: f32, y: f32, what: &str| {
            assert!(
                (-eps..=GRID + eps).contains(&x) && (-eps..=GRID + eps).contains(&y),
                "{what} 越出设计栅格：({x}, {y})，栅格是 0..={GRID}"
            );
        };
        // 直接遍历内部建表结果（`spec` 只给已注册的图标）。
        for (icon, ops) in build_specs() {
            for op in &ops {
                match op {
                    IconOp::Line { from, to, .. } => {
                        check(from.0, from.1, &format!("{icon:?} 线段起点"));
                        check(to.0, to.1, &format!("{icon:?} 线段终点"));
                    }
                    IconOp::Polyline { pts, .. } => {
                        for g in pts {
                            check(g.0, g.1, &format!("{icon:?} 折线点"));
                        }
                    }
                    IconOp::Quad { from, ctrl, to, .. } => {
                        check(from.0, from.1, &format!("{icon:?} 二次弧起点"));
                        check(ctrl.0, ctrl.1, &format!("{icon:?} 二次弧控制点"));
                        check(to.0, to.1, &format!("{icon:?} 二次弧终点"));
                    }
                    IconOp::Cubic { pts, .. } => {
                        for g in pts {
                            check(g.0, g.1, &format!("{icon:?} 三次弧点"));
                        }
                    }
                    IconOp::Circle { c, r, .. } => {
                        check(c.0 - r, c.1 - r, &format!("{icon:?} 圆左上"));
                        check(c.0 + r, c.1 + r, &format!("{icon:?} 圆右下"));
                    }
                    IconOp::RoundRect { x, y, w, h, .. } => {
                        check(*x, *y, &format!("{icon:?} 圆角矩形左上"));
                        check(x + w, y + h, &format!("{icon:?} 圆角矩形右下"));
                    }
                }
            }
        }
    }

    /// 弧线采样必须真的跨过指定的角度范围（链环缺口朝向的几何依据）。
    #[test]
    fn arc_sampling_covers_the_requested_range() {
        let pts = arc(0.0, 0.0, 3.0, 30.0, 330.0);
        assert_eq!(pts.len(), 13, "12 段应有 13 个点");
        // 首尾应在半径上、且角度正确（0° = +x，顺时针）。
        let (x0, y0) = pts[0];
        assert!((x0 - 3.0 * 30f32.to_radians().cos()).abs() < 0.01);
        assert!((y0 - 3.0 * 30f32.to_radians().sin()).abs() < 0.01);
        let (x1, y1) = *pts.last().unwrap();
        assert!((x1 - 3.0 * 330f32.to_radians().cos()).abs() < 0.01);
        assert!((y1 - 3.0 * 330f32.to_radians().sin()).abs() < 0.01);
    }
}
