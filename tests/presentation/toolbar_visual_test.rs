//! 编辑区工具栏的**视觉规范**回归（问题 1：工具栏样式与整体设计不一致；
//! 图标纯图形化：字形 + 文字 → 向量图标）。
//!
//! # 三条不变量
//!
//! 工具栏从「一排文字按钮 + 三个固定 100pt 宽的 `ComboBox`」统一到
//! 「同一套尺寸 / 圆角 / 配色 / 间距」，再到「平铺按钮纯图形化」之后，
//! 靠肉眼是验不住的——差异只有 1–2pt。本文件跑**真帧**、从
//! `FullOutput::shapes` 里取回控件背景矩形与全部图元，把三条肉眼难辨的
//! 约束钉住：
//!
//! 1. **每个平铺按钮都真的画出了向量图标**——不是拿背景矩形充数，
//!    且图标居中、尺寸足够。废止字形图标后（豆腐块风险，见
//!    [`rustmark::presentation::icons`] 的模块文档），「有没有图」从
//!    「字体有没有覆盖」变成了「几何有没有画出来」，因此可以精确断言。
//! 2. **所有控件等高、顶边齐平**——两类控件各自算高（按钮取文案高、
//!    下拉取文案与图标取大）时实测差 1–2pt 且顶边错位，整行看起来
//!    「下楼梯」。统一取 `control_height` 后必须**逐控件**验齐平。
//! 3. **控件间距只来自主题刻度**——`Layout` 会在每个控件之间自动叠加一个
//!    `Spacing::item_spacing.x`，与显式 `add_space` 相加后间距翻倍，
//!    「同组紧、异组松」的层级差被冲淡。故渲染层把 `item_spacing.x`
//!    归零，间距因此可以被**精确断言**（组内 `space_xs`、组间 `space_md`）。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::icons::{Icon, spec};
use rustmark::presentation::shortcuts::registry::UiAction;
use rustmark::presentation::views::toolbar_view::{FLAT_GROUP, ToolbarControl, toolbar_controls};
use std::sync::Arc;

/// 验收窗口尺寸（与 `top_menu_test` / `theme_mapper_test` 一致）。
const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

// ============================================================================
// 基础设施
// ============================================================================

fn app_with_open_document() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let file = dir.path().join("note.md");
    std::fs::write(&file, "# 标题\n\n正文一行。\n").expect("写文件应成功");
    app.open_path(&file.to_string_lossy()).expect("打开应成功");
    // 侧栏文件树会显示文件名，增加噪声；关掉，让「工具栏那一行」更干净。
    app.ui.sidebar_visible = false;
    (dir, app)
}

/// 装上与真实应用**同一套**字体（含系统 CJK 兜底）的上下文。
///
/// 直接 `Context::default()` 只有 egui 自带的西文字体，图标/emoji 的覆盖
/// 情况与本机真实运行不一致——拿它测字形会得到假阳性。
fn ctx_with_app_fonts() -> egui::Context {
    let ctx = egui::Context::default();
    let setup = rustmark::infrastructure::fonts::font_definitions(None);
    ctx.set_fonts(rustmark::app::frame::to_egui_font_definitions(
        setup.definitions,
    ));
    // 首个 `run` 会解析字体（`panic = "abort"` 下字体解析失败会直接终止，
    // 故这一步同时守护了「字体文件不被支持」这个风险）。
    crate::support::run_ui(&ctx, raw_input(Vec::new()), |_| {});
    ctx
}

fn raw_input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
        events,
        ..Default::default()
    }
}

/// 跑一帧主界面，取回全部**矩形**、**文字**与**所有图元的外包矩形**。
///
/// 多跑几帧：`ScrollArea` / 弹出层首帧会走 sizing pass，那一帧不下发绘制指令。
///
/// 第三项（全部图元的外包矩形）是图标纯图形化后新增的采集项：向量图标
/// 不再有「文字」可循，只能靠几何确认它被画了出来。
fn render(
    ctx: &egui::Context,
    app: &mut EditorApp,
) -> (
    Vec<egui::epaint::RectShape>,
    Vec<(egui::Rect, String)>,
    Vec<egui::Rect>,
) {
    let mut rects = Vec::new();
    let mut texts = Vec::new();
    let mut shape_rects = Vec::new();
    for _ in 0..3 {
        let out = crate::support::run_ui(ctx, raw_input(Vec::new()), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ui);
        });
        rects.clear();
        texts.clear();
        shape_rects.clear();
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut rects, &mut texts, &mut shape_rects);
        }
    }
    (rects, texts, shape_rects)
}

fn walk(
    shape: &egui::epaint::Shape,
    rects: &mut Vec<egui::epaint::RectShape>,
    texts: &mut Vec<(egui::Rect, String)>,
    shape_rects: &mut Vec<egui::Rect>,
) {
    match shape {
        egui::epaint::Shape::Rect(x) => rects.push(x.clone()),
        egui::epaint::Shape::Text(x) => {
            texts.push((x.visual_bounding_rect(), x.galley.text().to_string()));
        }
        egui::epaint::Shape::Vec(v) => {
            for s in v {
                walk(s, rects, texts, shape_rects);
            }
        }
        // `Noop` 之外的一切图形（线段、路径、圆、贝塞尔……）都记外包矩形：
        // 向量图标的图元正是这些。`visual_bounding_rect` 已含描边宽度。
        other => shape_rects.push(other.visual_bounding_rect()),
    }
}

/// 顶部面板（应用级菜单栏 + 标签条 + 面包屑）的高度。
///
/// 编辑区工具栏画在这个高度**之下**（编辑区顶部），故用它把工具栏那一行
/// 从顶栏的矩形里切出来。
fn top_region_height(app: &EditorApp) -> f32 {
    let m = app.theme().metrics;
    m.topbar_height + m.tabstrip_height + m.tabbar_height
}

/// 从一帧里挑出「编辑区工具栏的控件背景」。
///
/// 判据有两条，都来自**视觉规范本身**：
/// - 高度等于 [`control_height`]（`editor_toolbar_height − 2·space_xxs`）——这一条
///   同时把顶栏的 `ComboBox`（约 23pt）与各种大背景板排除在外；
/// - 落在工具栏那一行的 y 带里（顶栏之下、工具栏高度之内）。
///
/// 返回按左缘排序的矩形。
///
/// [`control_height`]: rustmark::presentation::views::toolbar_view
fn toolbar_control_rects(rects: &[egui::epaint::RectShape], app: &EditorApp) -> Vec<egui::Rect> {
    let m = app.theme().metrics;
    let expected_h = m.editor_toolbar_height - 2.0 * m.space_xxs;
    let top_limit = top_region_height(app);
    let band_end = top_limit + m.editor_toolbar_height + 40.0;

    let mut out: Vec<egui::Rect> = rects
        .iter()
        .map(|r| r.rect)
        .filter(|r| {
            (r.height() - expected_h).abs() < 1.5
                && r.center().y > top_limit
                && r.center().y < band_end
                // 宽度上限：把「恰好等高的整行背景」之类排除掉。
                && r.width() < 260.0
        })
        .collect();
    out.sort_by(|a, b| a.min.x.partial_cmp(&b.min.x).unwrap());
    out
}

// ============================================================================
// 不变量 1：每个平铺按钮都画出了向量图标
// ============================================================================

/// 每个**平铺控件**都必须有非空的向量图标规格。
///
/// 「纯图形按钮」最怕的不是图形难看，而是根本没有图形——按钮退化成一个
/// 空方块，用户完全看不出它能干什么（新增平铺动作却忘了画图标时尤其如此）。
/// 这条把「有图」钉死在数据层：渲染前就能发现，不必等截图。
#[test]
fn every_flat_control_has_a_vector_icon_spec() {
    let mut flat = 0;
    for control in toolbar_controls() {
        if let ToolbarControl::Flat(action) = control {
            flat += 1;
            assert!(
                !spec(Icon::Action(action)).is_empty(),
                "{action:?} 是平铺图标按钮，但没有向量图标规格——按钮上会是一个空方块"
            );
        }
    }
    assert!(flat >= 5, "平铺按钮至少要有 5 个（格式组），实际 {flat}");
}

/// 每个平铺按钮都必须**真的画出**向量图标：图元数量、居中度、尺寸都达标。
///
/// # 为什么三条一起断言
///
/// 只数「按钮里有没有图形」不够——画一笔歪线甚至一个点都能通过。
/// 三条合起来才对应「肉眼看得出的图标」：
///
/// 1. **数量**：规格表里最简的图标也有 3 个图元；少于 2 个说明画秃了；
/// 2. **居中**：图标中心与按钮中心相差 > 2.5pt 就会被看成「画歪了」；
/// 3. **尺寸**：长边 < 9pt 的图标在 16pt 按钮里只是个小点，丧失辨识度；
///    上界 18pt 则保证没有图元溢出到按钮边框之外。
#[test]
fn every_flat_button_paints_a_centered_vector_icon() {
    let (_dir, mut app) = app_with_open_document();
    let ctx = ctx_with_app_fonts();
    let (rects, _texts, shape_rects) = render(&ctx, &mut app);
    let controls = toolbar_control_rects(&rects, &app);
    let flat_count = toolbar_controls()
        .iter()
        .filter(|c| matches!(c, ToolbarControl::Flat(_)))
        .count();
    assert!(
        controls.len() >= flat_count,
        "控件矩形（{}）比平铺按钮数（{flat_count}）还少——工具栏没画全",
        controls.len()
    );

    // 平铺分组在最前，故前 flat_count 个控件就是图标按钮。
    for (i, r) in controls.iter().take(flat_count).enumerate() {
        // 图标图元：中心落在按钮内、且面积明显小于按钮背景的形状
        // （按钮背景自身面积 == r.area()，被 0.9 系数排除）。
        let inner: Vec<egui::Rect> = shape_rects
            .iter()
            .copied()
            .filter(|s| r.contains(s.center()) && s.area() < r.area() * 0.9)
            .collect();
        assert!(
            inner.len() >= 2,
            "第 {i} 个平铺按钮只画出 {} 个图形元素——纯图形按钮不能没有图标",
            inner.len()
        );
        let union = inner
            .iter()
            .copied()
            .fold(egui::Rect::NOTHING, |a, b| a.union(b));
        assert!(
            (union.center() - r.center()).length() < 2.5,
            "第 {i} 个按钮的图标偏离中心 {:.2}pt（图标中心 {:?}，按钮中心 {:?}）",
            (union.center() - r.center()).length(),
            union.center(),
            r.center()
        );
        let longer = union.width().max(union.height());
        assert!(
            (9.0..=18.0).contains(&longer),
            "第 {i} 个按钮的图标长边 {longer:.2}pt 不在 9..=18 内——\
             要么小得看不见，要么溢出了按钮（按钮宽 {:.1}）",
            r.width()
        );
    }
}

/// 下拉触发器右侧的三角同样必须有字形。
#[test]
fn the_dropdown_arrow_has_a_glyph() {
    // 与渲染层同一个字号档位（`ui_font_size − 3.0`）。
    let ctx = ctx_with_app_fonts();
    let font = egui::FontId::proportional(10.0);
    let has = ctx.fonts_mut(|f| f.has_glyphs(&font, "⏷"));
    assert!(has, "下拉三角「⏷」无字形——下拉按钮上会出现一个豆腐块方框");
}

// ============================================================================
// 不变量 2：所有控件等高、顶边齐平
// ============================================================================

/// 工具栏上**全部**控件的背景矩形必须等高、顶边齐平、数量与预期一致。
///
/// 数量取自 [`toolbar_controls`]：平铺按钮 + 下拉标题。若渲染层少画了
/// 一个控件（例如某个分组改为平铺后忘了改渲染分支），这里会立刻发现。
#[test]
fn toolbar_controls_are_equal_height_and_aligned() {
    let (_dir, mut app) = app_with_open_document();
    let ctx = ctx_with_app_fonts();
    let (rects, _texts, _shapes) = render(&ctx, &mut app);
    let controls = toolbar_control_rects(&rects, &app);

    let expected = toolbar_controls().len();
    assert_eq!(
        controls.len(),
        expected,
        "工具栏控件数量应为 {expected}（见 `toolbar_controls`），实际 {}。\
         控件矩形：{controls:?}",
        controls.len()
    );

    let m = app.theme().metrics;
    let expected_h = m.editor_toolbar_height - 2.0 * m.space_xxs;

    // ① 等高。
    for (i, r) in controls.iter().enumerate() {
        assert!(
            (r.height() - expected_h).abs() < 0.5,
            "第 {i} 个控件高度 {}，应为 {expected_h}（工具栏行高 − 上下各一个咬合间距）",
            r.height()
        );
    }

    // ② 顶边齐平。
    let top0 = controls[0].top();
    for (i, r) in controls.iter().enumerate() {
        assert!(
            (r.top() - top0).abs() < 0.5,
            "第 {i} 个控件顶边 {} 与第一个 {top0} 不齐——整行会「下楼梯」",
            r.top()
        );
    }

    // ③ 圆角来自主题（不是某处写死的数字）。
    let expected_radius = egui::CornerRadius::same(m.radius_button.round().clamp(0.0, 255.0) as u8);
    let radius_counts = rects
        .iter()
        .filter(|r| controls.contains(&r.rect))
        .filter(|r| r.corner_radius == expected_radius)
        .count();
    assert_eq!(
        radius_counts,
        controls.len(),
        "有控件的圆角不等于主题的 `radius_button`——「统一圆角」被某处写死的数字破坏了。\
         期望 {expected_radius:?}"
    );

    // ④ 平铺按钮是**等宽正方形**（纯图形化后不再有文案宽度）。
    //    下拉触发器仍是文字按钮（宽随标题），故只检查平铺那一段。
    let flat_count = toolbar_controls()
        .iter()
        .filter(|c| matches!(c, ToolbarControl::Flat(_)))
        .count();
    let w0 = controls[0].width();
    for (i, r) in controls.iter().take(flat_count).enumerate() {
        assert!(
            (r.width() - w0).abs() < 0.5,
            "第 {i} 个平铺按钮宽 {:.2}，与第一个 {w0:.2} 不等——\
             纯图形按钮应为等宽正方形栅格",
            r.width()
        );
    }
}

// ============================================================================
// 不变量 3：间距只来自主题刻度
// ============================================================================

/// 控件之间的水平间距必须**精确**等于主题刻度：组内 `space_xs`、组间 `space_md`。
///
/// # 为什么必须精确
///
/// `Layout` 会在每个控件之间自动叠加 `Spacing::item_spacing.x`（默认 8pt），
/// 于是「组内 4 + 自动 8 = 12」与「组间 8 + 自动 8×2 = 16」——两处都被自动间距
/// 抬到同一档，「同组紧、异组松」的层级差被冲淡。渲染层把 `item_spacing.x`
/// 归零之后，间距就只剩显式给的值，因此可以逐段断言。
#[test]
fn toolbar_control_gaps_come_from_theme_scale() {
    let (_dir, mut app) = app_with_open_document();
    let ctx = ctx_with_app_fonts();
    let (rects, _texts, _shapes) = render(&ctx, &mut app);
    let controls = toolbar_control_rects(&rects, &app);
    assert!(controls.len() >= 2, "至少要有两个控件才能验间距");

    let m = app.theme().metrics;

    // 每个分组贡献几个控件：平铺分组 = 动作数，其余 = 1（一个下拉）。
    let per_group: Vec<usize> =
        rustmark::presentation::views::toolbar_view::editor_toolbar_groups()
            .iter()
            .map(|(title, actions)| {
                if *title == FLAT_GROUP {
                    actions.len()
                } else {
                    1
                }
            })
            .collect();
    assert_eq!(
        per_group.iter().sum::<usize>(),
        controls.len(),
        "分组贡献的控件数与实际画出的控件数不一致——渲染层的分组处理漏了某组"
    );

    // 「某控件之后有一条分隔线」的下标集合（每组最后一个控件之后，末组除外）。
    let mut divider_after = Vec::new();
    let mut idx = 0usize;
    for (gi, &count) in per_group.iter().enumerate() {
        idx += count;
        if gi + 1 < per_group.len() {
            divider_after.push(idx - 1);
        }
    }

    for i in 0..controls.len() - 1 {
        let gap = controls[i + 1].min.x - controls[i].max.x;
        let want = if divider_after.contains(&i) {
            m.space_md // 组间：分隔线的占位宽度
        } else {
            m.space_xs // 组内
        };
        assert!(
            (gap - want).abs() < 0.75,
            "第 {i} 与第 {} 个控件之间的间距是 {gap:.2}，应为 {want:.2}\
             （{}）——说明 `item_spacing.x` 又被自动叠加进来了，\
             「同组紧、异组松」的层级差会被冲淡",
            i + 1,
            if divider_after.contains(&i) {
                "组间 · space_md"
            } else {
                "组内 · space_xs"
            }
        );
    }
}

// ============================================================================
// 控件清单：渲染与用例同源
// ============================================================================

/// `toolbar_controls()` 必须与「分组展开后的全部格式动作」一一对应。
///
/// 平铺分组：一个动作 = 一个图标按钮；下拉分组：整组只算一个控件（标题）。
/// 这条保证用例里「要检查的控件」跟界面上「真的画出来的控件」是同一批。
#[test]
fn toolbar_controls_match_the_groups() {
    use rustmark::presentation::views::toolbar_view::editor_toolbar_groups;

    let mut expected: Vec<ToolbarControl> = Vec::new();
    for (title, actions) in editor_toolbar_groups() {
        if title == FLAT_GROUP {
            expected.extend(actions.into_iter().map(ToolbarControl::Flat));
        } else {
            expected.push(ToolbarControl::Dropdown(title));
        }
    }

    assert_eq!(
        toolbar_controls(),
        expected,
        "`toolbar_controls` 与分组表对不上——渲染与用例会看到不同的控件集合"
    );

    // 平铺按钮的动作必须**互不相同**：图标按钮没有文字可循，用例与用户
    // 都按「第几个按钮」定位，重复动作会让两者串位。
    let flat: Vec<UiAction> = editor_toolbar_groups()
        .iter()
        .filter(|(t, _)| *t == FLAT_GROUP)
        .flat_map(|(_, a)| a.iter().copied())
        .collect();
    let unique: std::collections::HashSet<UiAction> = flat.iter().copied().collect();
    assert_eq!(unique.len(), flat.len(), "平铺按钮动作出现重复：{flat:?}");
}
