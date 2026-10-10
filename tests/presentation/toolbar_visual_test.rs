//! 编辑区工具栏的**视觉规范**回归（问题 1：工具栏样式与整体设计不一致）。
//!
//! # 三条不变量
//!
//! 工具栏从「一排文字按钮 + 三个固定 100pt 宽的 `ComboBox`」统一到
//! 「同一套尺寸 / 圆角 / 配色 / 间距」之后，靠肉眼是验不住的——
//! 差异只有 1–2pt。本文件跑**真帧**、从 `FullOutput::shapes` 里取回控件背景
//! 矩形，把三条肉眼难辨的约束钉住：
//!
//! 1. **图标必须有字形**——egui 自带的西文字体只覆盖一部分符号，
//!    图标一旦落在覆盖之外，界面上就是一个豆腐块方框，比没有图标更糟。
//!    逐个用 `Fonts::has_glyphs` 实测（见 [`UiAction::icon`] 的注释）。
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
use rustmark::presentation::shortcuts::registry::{MenuGroup, UiAction};
use rustmark::presentation::views::toolbar_view::{FLAT_GROUP, toolbar_control_labels};
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
    let setup = rustmark::infrastructure::fonts::font_definitions();
    ctx.set_fonts(setup.definitions);
    // 首个 `run` 会解析字体（`panic = "abort"` 下字体解析失败会直接终止，
    // 故这一步同时守护了「字体文件不被支持」这个风险）。
    let _ = ctx.run(raw_input(Vec::new()), |_| {});
    ctx
}

fn raw_input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
        events,
        ..Default::default()
    }
}

/// 跑一帧主界面，取回全部**矩形**与**文字**绘制指令。
///
/// 多跑几帧：`ScrollArea` / 弹出层首帧会走 sizing pass，那一帧不下发绘制指令。
fn render(
    ctx: &egui::Context,
    app: &mut EditorApp,
) -> (Vec<egui::epaint::RectShape>, Vec<(egui::Rect, String)>) {
    let mut rects = Vec::new();
    let mut texts = Vec::new();
    for _ in 0..3 {
        let out = ctx.run(raw_input(Vec::new()), |ctx| {
            rustmark::presentation::theme::mapper::apply(ctx, &app.ui.theme);
            rustmark::presentation::views::main_view::show(app, ctx);
        });
        rects.clear();
        texts.clear();
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut rects, &mut texts);
        }
    }
    (rects, texts)
}

fn walk(
    shape: &egui::epaint::Shape,
    rects: &mut Vec<egui::epaint::RectShape>,
    texts: &mut Vec<(egui::Rect, String)>,
) {
    match shape {
        egui::epaint::Shape::Rect(x) => rects.push(x.clone()),
        egui::epaint::Shape::Text(x) => {
            texts.push((x.visual_bounding_rect(), x.galley.text().to_string()));
        }
        egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| walk(s, rects, texts)),
        _ => {}
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
// 不变量 1：图标有字形
// ============================================================================

/// 工具栏上**每一个**图标都必须在本机字体里有字形。
///
/// 这条不是「锦上添花」：图标落在字体覆盖之外时，egui 会画一个豆腐块方框——
/// 用户看到的是一个莫名其妙的方块，比不放图标更糟。而覆盖率随平台变化，
/// 所以只能实测，不能「看着像有」。
#[test]
fn every_toolbar_icon_has_a_glyph_in_the_app_fonts() {
    let ctx = ctx_with_app_fonts();
    // 按钮文案用的字号（`control_button` 取 `ui_font_size − 1.0`）。
    // 字形覆盖与字号无关，取整以贴近真实档位。
    let font = egui::FontId::proportional(13.0);

    let missing: Vec<(String, &'static str)> = ctx.fonts_mut(|f| {
        UiAction::ALL
            .iter()
            .filter_map(|a| a.icon().map(|icon| (format!("{a:?}"), icon)))
            .filter(|(_, icon)| !f.has_glyphs(&font, icon))
            .collect()
    });

    assert!(
        missing.is_empty(),
        "以下图标在本机字体里**没有字形**，界面上会显示为豆腐块方框：{missing:?}。\
         换一个字符，或把该动作的 `UiAction::icon` 改回 `None`（宁可没有图标）。"
    );
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
/// 数量取自 [`toolbar_control_labels`]：平铺按钮 + 下拉标题。若渲染层少画了
/// 一个控件（例如某个分组改为平铺后忘了改渲染分支），这里会立刻发现。
#[test]
fn toolbar_controls_are_equal_height_and_aligned() {
    let (_dir, mut app) = app_with_open_document();
    let ctx = ctx_with_app_fonts();
    let (rects, _texts) = render(&ctx, &mut app);
    let controls = toolbar_control_rects(&rects, &app);

    let expected = toolbar_control_labels().len();
    assert_eq!(
        controls.len(),
        expected,
        "工具栏控件数量应为 {expected}（见 `toolbar_control_labels`），实际 {}。\
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
}

// ============================================================================
// 不变量 3：间距只来自主题刻度
// ============================================================================

/// 控件之间的水平间距必须**精确**等于主题刻度：组内 `space_xs`、组间 `space_md`。
///
/// # 为什么必须精确
///
/// `Layout` 会在每个控件之间自动叠加 `Spacing::item_spacing.x`（默认 8pt），
/// 于是「组内 4 + 自动 8 = 12」与「组间 8 + 自动 8 = 16」——两处都被自动间距
/// 抬到同一档，「同组紧、异组松」的层级差被冲淡。渲染层把 `item_spacing.x`
/// 归零之后，间距就只剩显式给的值，因此可以逐段断言。
#[test]
fn toolbar_control_gaps_come_from_theme_scale() {
    let (_dir, mut app) = app_with_open_document();
    let ctx = ctx_with_app_fonts();
    let (rects, _texts) = render(&ctx, &mut app);
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
// 文案规范：短文案 + 图标，且覆盖全部格式动作
// ============================================================================

/// 工具栏按钮文案必须是「图标 + 空格 + 短文案」，且短文案不宜过长。
///
/// 完整文案（`UiAction::label`，如「提升标题级别」）铺到一行 18 个按钮上会
/// 横向溢出，故工具栏走 `toolbar_label()`；完整说法由悬停提示补足。
/// 这里把「短」与「带图标」两条钉住。
#[test]
fn toolbar_button_text_is_short_and_prefixed_with_its_icon() {
    for action in UiAction::ALL
        .iter()
        .copied()
        .filter(|a| a.group() == MenuGroup::Format)
    {
        let short = action.toolbar_label();
        assert!(
            !short.trim().is_empty(),
            "{action:?} 没有工具栏短文案——按钮上会是空白"
        );
        assert!(
            short.chars().count() <= 4,
            "{action:?} 的工具栏短文案「{short}」太长（{} 字）——会撑宽按钮并挤掉别的控件",
            short.chars().count()
        );

        let text = rustmark::presentation::views::toolbar_view::button_text(action);
        match action.icon() {
            Some(icon) => assert!(
                text.starts_with(icon) && text.len() > icon.len(),
                "{action:?} 有图标 {icon:?}，但按钮文案「{text}」没有以它开头"
            ),
            None => assert_eq!(
                text, short,
                "{action:?} 无图标，按钮文案应就是短文案本身（不额外补空格）"
            ),
        }
    }
}

/// `toolbar_control_labels()` 必须与「分组展开后的全部格式动作」一一对应。
///
/// 平铺分组：一个动作 = 一个按钮；下拉分组：整组只算一个控件（标题）。
/// 这条保证用例里「要检查的控件」跟界面上「真的画出来的控件」是同一批。
#[test]
fn toolbar_control_labels_match_the_groups() {
    use rustmark::presentation::views::toolbar_view::{button_text, editor_toolbar_groups};

    let mut expected: Vec<String> = Vec::new();
    for (title, actions) in editor_toolbar_groups() {
        if title == FLAT_GROUP {
            expected.extend(actions.iter().map(|a| button_text(*a)));
        } else {
            expected.push(title.to_string());
        }
    }

    let labels = toolbar_control_labels();
    assert_eq!(
        labels, expected,
        "`toolbar_control_labels` 与分组表对不上——渲染与用例会看到不同的控件集合"
    );

    // 平铺分组的按钮文案必须**互不相同**，否则按文案定位控件（几何用例、
    // 以及用户靠文字辨认）都会串。
    let flat: Vec<String> = editor_toolbar_groups()
        .iter()
        .filter(|(t, _)| *t == FLAT_GROUP)
        .flat_map(|(_, a)| a.iter().map(|x| button_text(*x)))
        .collect();
    let unique: std::collections::HashSet<&String> = flat.iter().collect();
    assert_eq!(unique.len(), flat.len(), "平铺按钮文案出现重复：{flat:?}");
}
