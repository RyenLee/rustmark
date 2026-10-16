//! 顶部菜单结构用例（回归：「应用级菜单」与「编辑区工具栏」两层必须真的分开，
//! 且下拉菜单里的文案与快捷键不得互相遮挡）。
//!
//! # 为什么必须在**渲染层**断言，而不是在 `UiAction` 表上断言
//!
//! `UiAction` / `MenuGroup` 表全对，界面仍可能错——这两个缺陷都是这样：
//!
//! 1. **分层**：动作表里 `MenuGroup::Format` 一直都在，但渲染时把它当成
//!    第 5 个下拉菜单、又跟在后面把 10 个按钮铺在同一行里。表看不出来，
//!    只有看「谁画在第几行」才知道。
//! 2. **遮挡**：菜单项的文案与快捷键来自同一个 `UiAction`，一个字节都没错，
//!    错的只是**排版**——快捷键是用 `Painter::text` 画的，不占位，
//!    于是被画到文案身上。表上更看不出来。
//!
//! 因此本文件跑真帧、从 `FullOutput::shapes` 里取回每个文字指令的**矩形**，
//! 用几何断言把两条不变量钉住。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::shortcuts::registry::{MenuGroup, UiAction};
use rustmark::presentation::views::toolbar_view::{
    MENU_LABEL_SHORTCUT_GAP, editor_toolbar_groups, menu_row_width, toolbar_control_labels,
};
use std::sync::Arc;

/// `ImeEvent::Enabled` 在 egui 0.37 已废弃（egui 自管 IME 开关），但用例仍要
/// 表达「用户开启输入法」的意图，故用一个带 `#[allow(deprecated)]` 的助手包住。
#[allow(deprecated)]
fn ime_enabled_event() -> egui::Event {
    egui::Event::Ime(egui::ImeEvent::Enabled)
}

/// 验收窗口尺寸（与 `theme_mapper_test` 的两档一致）。
const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// 「两行」判定用：两段文字中心 y 相差多少才算不在同一行。
///
/// 取 8：同一行内的文字基线差不会超过几像素，而两行之间隔着一个
/// `topbar_height`（40），不存在灰区。
const SAME_ROW_EPS: f32 = 8.0;

// ============================================================================
// 基础设施
// ============================================================================

fn app_fixture() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    (dir, EditorApp::new(services, false))
}

/// 装好配置、并**打开一份文档**的应用。
///
/// # 为什么要真的开一份文档
///
/// 否则主区域显示的是**欢迎页**，而欢迎页上列着一串快捷键
/// （「查找 / Ctrl+F」这种）。那些文字与下拉菜单项**逐字相同**，
/// 会污染「下拉内容」的提取，断言出来的重叠根本不是下拉里的重叠
/// （实测踩到过：取到了 y=467 的一对文字，而下拉在 y≈40–190）。
/// 打开文档后欢迎页消失，剩下的只有顶栏、标签条、面包屑、状态栏。
fn app_with_open_document() -> (tempfile::TempDir, EditorApp) {
    let (dir, mut app) = app_fixture();
    let file = dir.path().join("note.md");
    std::fs::write(&file, "# 标题\n\n正文一行。\n").expect("写文件应成功");
    app.open_path(&file.to_string_lossy()).expect("打开应成功");
    // 侧栏的文件树会显示文件名，与菜单项无关却增加噪声，关掉。
    app.ui.sidebar_visible = false;
    (dir, app)
}

/// 干净的上下文（高亮缓存是线程局部的，用例之间会互相污染）。
fn fresh_ctx() -> egui::Context {
    rustmark::presentation::theme::mapper::clear_highlight_caches();
    let ctx = egui::Context::default();
    crate::support::run_ui(&ctx, egui::RawInput::default(), |_| {}); // 装载字库
    ctx
}

fn raw_input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
        events,
        ..Default::default()
    }
}

/// 从一帧的绘制指令里取回所有文字及其**占据的矩形**。
///
/// `TextShape.pos` 是文字左上角，`galley.size()` 是排版尺寸，
/// 两者合成真实占位——这正是判断「有没有互相遮挡」需要的量。
fn painted_texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(egui::Rect, String)> {
    fn walk(shape: &egui::epaint::Shape, out: &mut Vec<(egui::Rect, String)>) {
        match shape {
            egui::epaint::Shape::Text(t) => {
                out.push((
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                    t.galley.text().to_owned(),
                ));
            }
            egui::epaint::Shape::Vec(inner) => {
                for s in inner {
                    walk(s, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// 跑一帧（可带输入事件），返回本帧画出的全部文字与矩形。
fn run_frame(
    ctx: &egui::Context,
    app: &mut EditorApp,
    events: Vec<egui::Event>,
) -> Vec<(egui::Rect, String)> {
    let out = crate::support::run_ui(ctx, raw_input(events), |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(app, ui);
    });
    painted_texts(&out.shapes)
}

fn press_at(pos: egui::Pos2) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    }
}

fn release_at(pos: egui::Pos2) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    }
}

/// 在 `pos` 上完成一次「按下 → 抬起」，返回抬起后那一帧的画面。
fn click_at(
    ctx: &egui::Context,
    app: &mut EditorApp,
    pos: egui::Pos2,
) -> Vec<(egui::Rect, String)> {
    run_frame(
        ctx,
        app,
        vec![egui::Event::PointerMoved(pos), press_at(pos)],
    );
    run_frame(
        ctx,
        app,
        vec![egui::Event::PointerMoved(pos), release_at(pos)],
    )
}

/// 某一帧里某段文字的矩形（取第一个完全匹配的）。
fn rect_of(painted: &[(egui::Rect, String)], text: &str) -> Option<egui::Rect> {
    painted.iter().find(|(_, t)| t == text).map(|(r, _)| *r)
}

/// 某段文字的中心 y（用于判断画在第几行）。
fn y_of(painted: &[(egui::Rect, String)], text: &str) -> Option<f32> {
    rect_of(painted, text).map(|r| r.center().y)
}

/// 顶部面板的总高度（应用级菜单栏 + 标签条 + 面包屑）。
///
/// 用来把断言限定在**顶栏**里：侧边栏的标签也叫「文件」，
/// 状态栏、面包屑也会出现各种字，不限定范围就会把它们的字误判成
/// 「菜单跑到别的行上去了」。编辑区工具栏已移到编辑区顶部，不在此区域内。
fn top_region_height(app: &EditorApp) -> f32 {
    let m = app.theme().metrics;
    m.topbar_height + m.tabstrip_height + m.tabbar_height
}

/// 全部格式类动作（工具理应包含的那一批）。
fn format_actions() -> Vec<UiAction> {
    UiAction::ALL
        .iter()
        .copied()
        .filter(|a| a.group() == MenuGroup::Format)
        .collect()
}

// ============================================================================
// 问题 2：应用级菜单与编辑区工具栏必须分两层
// ============================================================================

/// 应用级菜单栏与编辑区工具栏必须画在**两行**，且编辑区工具栏在「编辑区顶部」
/// （顶栏之外），而不是混进应用级菜单栏那一行。
///
/// 验收描述：「有下拉菜单的项（如文件、视图等）才属于应用级别菜单，
/// 其后『加粗』等功能按钮应归属于编辑区域工具栏，并放到编辑区域顶部」。
#[test]
fn editor_toolbar_lives_in_editor_pane_not_top_region() {
    let (_dir, mut app) = app_with_open_document();
    let ctx = fresh_ctx();
    let painted = run_frame(&ctx, &mut app, Vec::new());

    let top_limit = top_region_height(&app);

    // ① 四个应用级菜单都在「顶栏之内」（y < top_limit）。
    let mut menu_ys = Vec::new();
    for group in MenuGroup::APP_LEVEL {
        let y = y_of(&painted, group.label())
            .unwrap_or_else(|| panic!("应用级菜单栏缺少「{}」", group.label()));
        assert!(
            y < top_limit,
            "应用级菜单「{}」跑到顶栏之外了（y={y}，顶栏高={top_limit}）",
            group.label()
        );
        menu_ys.push((group.label(), y));
    }

    // ② 编辑区工具栏上的**每一个控件**都在「顶栏下方」（y >= top_limit）。
    //
    // 控件文案取自 `toolbar_control_labels()`：平铺分组给出的是每个按钮的
    // 文案（「B 加粗」），下拉分组给出的是分组标题（「插入」）。此前这里
    // 假设「所有分组标题都可见」，但「格式」组已改为平铺——分组标题根本
    // 不存在，于是断言去找一个画不出来的字而必然失败。
    let mut bar_ys: Vec<(String, f32)> = Vec::new();
    for label in toolbar_control_labels() {
        let y = y_of(&painted, &label)
            .unwrap_or_else(|| panic!("编辑区工具栏缺少控件「{label}」——它必须画在编辑区顶部"));
        assert!(
            y >= top_limit,
            "编辑区工具栏控件「{label}」还在顶栏里（y={y}，顶栏高={top_limit}）\
             ——它必须移到编辑区顶部"
        );
        bar_ys.push((label, y));
    }
    assert!(
        bar_ys.len() >= 4,
        "编辑区工具栏控件太少（{}），本用例将失去意义",
        bar_ys.len()
    );

    // ③ 各自的**行内**对齐：同一层内部的 y 必须一致。
    let menu_y0 = menu_ys[0].1;
    for (label, y) in &menu_ys {
        assert!(
            (y - menu_y0).abs() < SAME_ROW_EPS,
            "应用级菜单「{label}」与「{}」不在同一行（y={y} vs {menu_y0}）",
            menu_ys[0].0
        );
    }
    let bar_y0 = bar_ys[0].1;
    for (label, y) in &bar_ys {
        assert!(
            (y - bar_y0).abs() < SAME_ROW_EPS,
            "编辑区工具栏控件「{label}」与「{}」不在同一行（y={y} vs {bar_y0}）",
            bar_ys[0].0
        );
    }

    // ④ 两层之间必须真的分开：工具栏整行在菜单栏整行**下方**，
    //    且相距大于 20pt（肉眼可辨为两行）。
    assert!(
        bar_y0 > menu_y0,
        "编辑区工具栏必须画在应用级菜单栏下方（工具栏 y={bar_y0}，菜单 y={menu_y0}）"
    );
    assert!(
        bar_y0 - menu_y0 > 20.0,
        "两层贴得太近（相距 {}），看起来仍是同一层",
        bar_y0 - menu_y0
    );
}

/// 两层的**内容**不得混：应用级菜单栏里不能有编辑区工具栏的分组标题，反之亦然。
///
/// 只断言「都在」不够——把 4 个分组标题继续铺在菜单栏那一行，
/// 上面那条用例照样绿（它只看「有没有」和「工具栏行是否在下方」）。
#[test]
fn the_two_layers_do_not_mix_contents() {
    let (_dir, mut app) = app_with_open_document();
    let ctx = fresh_ctx();
    let painted = run_frame(&ctx, &mut app, Vec::new());
    let top_limit = top_region_height(&app);

    // 编辑区工具栏的控件文案不得出现在「顶栏」那一行。
    for label in toolbar_control_labels() {
        for (rect, text) in &painted {
            if *text == label {
                assert!(
                    rect.center().y >= top_limit,
                    "编辑区工具栏控件「{label}」出现在应用级菜单栏那一行\
                     （y={}）——两层内容混了",
                    rect.center().y
                );
            }
        }
    }

    // 反向：应用级菜单的名称不得出现在「顶栏下方」编辑区工具栏那一行。
    for group in MenuGroup::APP_LEVEL {
        for (rect, text) in &painted {
            if text == group.label() {
                assert!(
                    rect.center().y < top_limit,
                    "应用级菜单「{}」出现在编辑区工具栏那一行（y={}）",
                    group.label(),
                    rect.center().y
                );
            }
        }
    }
}

/// 编辑区工具栏的四个下拉分组**合起来**必须覆盖全部 `MenuGroup::Format` 动作。
///
/// 否则有动作在界面上无从触发。这条不依赖渲染、纯查表，但它是
/// 「分组是否齐全」的最后防线（见 `toolbar_view::editor_toolbar_groups` 的契约）。
#[test]
fn editor_toolbar_groups_cover_all_format_actions() {
    use std::collections::HashSet;
    let grouped: HashSet<UiAction> = editor_toolbar_groups()
        .iter()
        .flat_map(|(_, actions)| actions.iter().copied())
        .collect();
    let format: Vec<UiAction> = format_actions();

    for action in &format {
        assert!(
            grouped.contains(action),
            "编辑区工具栏的四个分组漏掉了动作 {action:?}——它将在界面上无从触发"
        );
    }
    assert_eq!(
        grouped.len(),
        format.len(),
        "编辑区工具栏分组里混入了非格式类动作，或存在重复"
    );
}

/// 「格式」不再是一个应用级下拉菜单——它整组属于编辑区工具栏。
#[test]
fn format_group_is_not_an_app_level_dropdown() {
    assert!(
        !MenuGroup::APP_LEVEL.contains(&MenuGroup::Format),
        "「格式」是编辑区工具栏的内容，不该列在应用级菜单里"
    );
    // 应用级菜单必须都是「一组命令」而不是单个命令：
    // 每个分组都得有成员，否则会出现一个空的下拉菜单。
    for group in MenuGroup::APP_LEVEL {
        let n = UiAction::ALL.iter().filter(|a| a.group() == *group).count();
        assert!(n > 0, "应用级菜单「{}」里没有任何动作", group.label());
    }
    // 命令面板 / 快捷键一览仍需要「格式」分组，故 `ALL` 必须保留它。
    assert!(MenuGroup::ALL.contains(&MenuGroup::Format));
    // 顶部两层合起来正好覆盖全部动作，不漏不重。
    let covered: usize = MenuGroup::APP_LEVEL
        .iter()
        .chain(std::iter::once(&MenuGroup::Format))
        .map(|g| UiAction::ALL.iter().filter(|a| a.group() == *g).count())
        .sum();
    assert_eq!(
        covered,
        UiAction::ALL.len(),
        "应用级菜单 + 编辑区工具栏应当覆盖全部动作，否则有动作无从触发"
    );
}

// ============================================================================
// 问题 3：下拉菜单里文案与快捷键不得互相遮挡
// ============================================================================

/// 从一帧的画面里切出**下拉菜单的内容**。
///
/// # 为什么不能只按文字匹配
///
/// 侧边栏有一个也叫「文件」的标签（与菜单同名），欢迎页上也会出现
/// 「保存」这类词。只按文字找，会取到那些**不相干**的矩形，
/// 于是断言出来的「重叠」根本不是下拉里的重叠（实测踩到过：
/// 取到了 y=467 的一对文字，而下拉在 y≈40–190）。
///
/// # 为什么「末尾连续块」是可靠的判据
///
/// 下拉是 `PopupKind::Menu` → `Order::Foreground`，而主界面（面板、侧边栏、
/// 编辑区、欢迎页）都在更低图层。egui 按图层顺序拼接绘制指令，
/// 因此下拉的文字必然是整串文字指令**末尾**的一段连续块。
fn popup_items(
    painted: &[(egui::Rect, String)],
    app: &EditorApp,
    actions: &[UiAction],
) -> Vec<(egui::Rect, String)> {
    let registry = app.dispatcher().registry();
    let belongs = |text: &str| {
        actions.iter().any(|a| {
            if a.label() == text {
                return true;
            }
            let combo = registry.display_for(*a);
            !combo.is_empty() && combo == text
        })
    };
    let mut tail: Vec<(egui::Rect, String)> = Vec::new();
    for (rect, text) in painted.iter().rev() {
        if belongs(text) {
            tail.push((*rect, text.clone()));
        } else {
            break;
        }
    }
    tail.reverse();
    tail
}

/// 打开某个应用级菜单的下拉，返回**下拉内部**的文字与矩形。
///
/// # 为什么最后要多跑一帧
///
/// 弹出层是 `Area`，它**首帧做的是「量尺寸」的 sizing pass**
/// （`Area::begin` 里 `state.size` 为空时 `sizing_pass = true`，
/// 而 `Area::content_ui` 会给这种帧加上 `.sizing_pass().invisible()`）。
/// 那一帧只测量、不下发绘制指令，因此点开当帧的画面里**没有**菜单项。
/// 要看到真正的下拉，必须再跑一帧。
fn open_app_menu(
    ctx: &egui::Context,
    app: &mut EditorApp,
    title: &str,
) -> Vec<(egui::Rect, String)> {
    let first = run_frame(ctx, app, Vec::new());
    let center = rect_of(&first, title)
        .unwrap_or_else(|| panic!("应用级菜单栏应有「{title}」，找不到就无法点开它"))
        .center();
    click_at(ctx, app, center);
    // 点开当帧是 sizing pass（不下发绘制指令），再跑一帧才看得到下拉。
    run_frame(ctx, app, Vec::new())
}

/// 打开编辑区工具栏的某个**分组下拉**（如「插入」），返回下拉内部的文字与矩形。
///
/// 编辑区工具栏里只有**下拉分组**平时只显示分组标题（「插入」「列表/结构」
/// 「行操作」），真正的动作（「代码块」…）要打开对应分组才画出来。
/// 「格式」组已改为**平铺按钮**（「B 加粗」…），没有可点开的下拉，
/// 见 `toolbar_view::FLAT_GROUP`。
///
/// 逻辑与 `open_app_menu` 一致：先定位分组标题、点开、再补一帧越过 sizing pass。
fn open_editor_toolbar_group(
    ctx: &egui::Context,
    app: &mut EditorApp,
    title: &str,
) -> Vec<(egui::Rect, String)> {
    let first = run_frame(ctx, app, Vec::new());
    let center = rect_of(&first, title)
        .unwrap_or_else(|| panic!("编辑区工具栏应有分组「{title}」，找不到就无法点开它"))
        .center();
    click_at(ctx, app, center);
    // 点开当帧是 sizing pass（不下发绘制指令），再跑一帧才看得到下拉。
    run_frame(ctx, app, Vec::new())
}

/// 每个应用级菜单的下拉里，**每一项**的文案与快捷键都不得重叠。
///
/// 逐项断言「文案矩形右缘 + 最小间隙 ≤ 快捷键矩形左缘」。
///
/// # 「视图」组只断言看得见的那部分
///
/// 「视图」有 22 项，超过弹出层的最大高度（`Spacing::combo_height`，默认 200pt），
/// 于是 `ScrollArea` 只会**画出可见的那些**——其余项没有绘制指令，
/// 无从度量。这不是缺陷（菜单本来就可滚动），故这里改为：
/// 「画出来的每一项都不得重叠」，并要求至少画出来 [`MIN_VISIBLE_ITEMS`] 项，
/// 以免用例退化成空转。看不见的那部分由
/// [`menu_row_width_fits_the_widest_item_of_every_group`] 从宽度不变量上兜住。
#[test]
fn dropdown_label_never_overlaps_its_shortcut() {
    /// 少于这么多项就说明下拉没开或提取失败，用例会变成空转。
    ///
    /// 分组本身可能比它小（「设置」只有 3 项），故取 `min(组内项数, 这个数)`。
    const MIN_VISIBLE_ITEMS: usize = 5;

    let (_dir, mut app) = app_with_open_document();
    let ctx = fresh_ctx();

    for group in MenuGroup::APP_LEVEL {
        let frame = open_app_menu(&ctx, &mut app, group.label());
        let actions: Vec<UiAction> = UiAction::ALL
            .iter()
            .copied()
            .filter(|a| a.group() == *group)
            .collect();
        let popup = popup_items(&frame, &app, &actions);

        // 用例前提：下拉真的开了，且画出来了足够多的项。
        let wanted = actions.len().min(MIN_VISIBLE_ITEMS);
        let visible = actions
            .iter()
            .filter(|a| popup.iter().any(|(_, t)| t == a.label()))
            .count();
        assert!(
            visible >= wanted,
            "「{}」的下拉只画出 {visible} 项（少于 {wanted}），\
             本用例将失去意义。本帧下拉内容：{:?}",
            group.label(),
            popup.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>()
        );

        for action in &actions {
            let Some(label_rect) = rect_of(&popup, action.label()) else {
                continue; // 超出弹出层可视高度，未画出（见上方注释）。
            };
            let combo = app.dispatcher().registry().display_for(*action);
            if combo.is_empty() {
                continue;
            }
            let combo_rect = rect_of(&popup, &combo).unwrap_or_else(|| {
                panic!("菜单项「{}」的快捷键「{combo}」没画出来", action.label())
            });

            let gap = combo_rect.left() - label_rect.right();
            assert!(
                gap >= MENU_LABEL_SHORTCUT_GAP - 0.5,
                "「{}」下拉的「{}」与快捷键「{combo}」挨得太近（间隙 {gap:.1} < {}）\
                 ——文案与快捷键重叠；文案矩形 {label_rect:?}，快捷键矩形 {combo_rect:?}",
                group.label(),
                action.label(),
                MENU_LABEL_SHORTCUT_GAP
            );
        }
    }
}

/// 菜单项所在行的宽度必须容得下「最宽的一项」。
///
/// 这条是上一条的**成因级**护栏：菜单项的宽度是我们算出来的（见
/// `toolbar_view::menu_row_width`），算小了就会遮挡。断言的是纯几何关系，
/// 因此不依赖下拉是否成功打开——与上一条互为补充。
#[test]
fn menu_row_width_fits_the_widest_item_of_every_group() {
    let (_dir, app) = app_fixture();
    let ctx = fresh_ctx();
    let font = egui::FontId::proportional(app.theme().metrics.ui_font_size);

    crate::support::run_ui(&ctx, raw_input(Vec::new()), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            for group in MenuGroup::ALL {
                let actions: Vec<UiAction> = UiAction::ALL
                    .iter()
                    .copied()
                    .filter(|a| a.group() == *group)
                    .collect();
                let row_w = menu_row_width(ui, &app, &actions);
                assert!(row_w.is_finite() && row_w > 0.0, "行宽必须是一个有限正数");

                for action in &actions {
                    let label_w = ui.fonts_mut(|f| {
                        f.layout_no_wrap(
                            action.label().to_owned(),
                            font.clone(),
                            egui::Color32::WHITE,
                        )
                        .size()
                        .x
                    });
                    let combo = app.dispatcher().registry().display_for(*action);
                    let combo_w = if combo.is_empty() {
                        0.0
                    } else {
                        ui.fonts_mut(|f| {
                            f.layout_no_wrap(combo.clone(), font.clone(), egui::Color32::WHITE)
                                .size()
                                .x
                        })
                    };
                    assert!(
                        row_w >= label_w + combo_w + MENU_LABEL_SHORTCUT_GAP - 0.5,
                        "「{}」组的行宽（{row_w:.1}）容不下「{}」+「{combo}」\
                         （需 {:.1}）——菜单项会被挤到重叠",
                        group.label(),
                        action.label(),
                        label_w + combo_w + MENU_LABEL_SHORTCUT_GAP
                    );
                }
            }
        });
    });
}

/// 悬停某一行时应用级菜单栏与编辑区工具栏都不得消失（下拉是**叠加**不是替换）。
#[test]
fn opening_a_menu_keeps_both_layers_visible() {
    let (_dir, mut app) = app_with_open_document();
    let ctx = fresh_ctx();
    let frame = open_app_menu(&ctx, &mut app, "文件");

    assert!(
        rect_of(&frame, "文件").is_some(),
        "下拉打开后菜单栏本身不应消失"
    );
    // 编辑区工具栏的常驻控件（平铺按钮「B 加粗」与下拉标题「插入」）不得因为
    // 应用级下拉被打开而消失——下拉是**叠加**，不是替换。
    for label in ["B 加粗", "插入"] {
        assert!(
            rect_of(&frame, label).is_some(),
            "下拉打开后编辑区工具栏的「{label}」不见了——下拉把工具栏顶掉了"
        );
    }
}

/// 重写后的菜单项**仍然可点击**，且点击触发的正是同一个 `UiAction`。
///
/// # 为什么必须单独测这一条
///
/// 修「遮挡」时把菜单项从 `SelectableLabel` 换成了
/// 「自己 `allocate_exact_size` + 自己作画」。这类改写最容易出的副作用是
/// **整排菜单变成死按钮**：文字照画，`Sense` 却没挂上（或挂在别的 rect 上），
/// 界面看起来完全正常。上面那些几何用例一条都抓不到它。
///
/// 这里走完整链路：点开「视图」→ 点「显示 / 隐藏状态栏」→
/// 断言状态栏真的被切掉。
#[test]
fn clicking_a_dropdown_item_runs_its_action() {
    let (_dir, mut app) = app_with_open_document();
    let ctx = fresh_ctx();

    let before = app.ui.status_bar_visible;
    let item = UiAction::ToggleStatusbar;
    assert_eq!(
        item.group(),
        MenuGroup::View,
        "用例前提：该动作属于「视图」菜单"
    );

    // ① 点开「视图」的下拉，并在其中找到目标项的位置。
    let frame = open_app_menu(&ctx, &mut app, MenuGroup::View.label());
    let actions: Vec<UiAction> = UiAction::ALL
        .iter()
        .copied()
        .filter(|a| a.group() == MenuGroup::View)
        .collect();
    let popup = popup_items(&frame, &app, &actions);
    let item_rect = rect_of(&popup, item.label())
        .unwrap_or_else(|| panic!("「视图」下拉里应有「{}」且当前可见", item.label()));

    // ② 点它。（下拉本身就在 Foreground，点击会落在菜单项上。）
    click_at(&ctx, &mut app, item_rect.center());

    assert_ne!(
        app.ui.status_bar_visible,
        before,
        "点击菜单项「{}」没有触发任何动作——菜单被改成了死按钮",
        item.label()
    );
    assert!(
        !app.ui.status_bar_visible,
        "该动作是切换：原本可见，点后应隐藏"
    );
}

/// 编辑区工具栏里的动作同样可点击，且触发的是同一个 `UiAction`。
///
/// # 为什么分两路验证
///
/// 编辑区工具栏有两类控件：**平铺按钮**（「格式」组，文案是
/// 「B 加粗」这种）与**分组下拉**（「插入」…，动作要展开才出现）。
/// 两者的控件写法不同（前者是裸 `Button`，后者是 `Button` + `MenuButton`），
/// 各自的「可点」必须分别验证，否则一类被改成死按钮而另一类仍然绿。
///
/// 这里先点平铺按钮「B 加粗」，再点下拉项「代码块」，两条路径都要真的改文档。
#[test]
fn clicking_a_toolbar_button_runs_its_action() {
    // ① 平铺按钮：「B 加粗」直接点。
    let (_dir, mut app) = app_with_open_document();
    let ctx = fresh_ctx();
    let before = app.current_content().unwrap_or_default();

    let frame = run_frame(&ctx, &mut app, Vec::new());
    let bold_button = format!(
        "{} {}",
        UiAction::Bold.icon().expect("加粗应有图标"),
        UiAction::Bold.toolbar_label()
    );
    let rect = rect_of(&frame, &bold_button)
        .unwrap_or_else(|| panic!("编辑区工具栏应有平铺按钮「{bold_button}」"));
    click_at(&ctx, &mut app, rect.center());

    let after = app.current_content().unwrap_or_default();
    assert_ne!(after, before, "点击工具栏「{bold_button}」没有改变文档内容");
    assert!(
        after.contains("**"),
        "点击「{bold_button}」应插入加粗标记，实际内容：{after:?}"
    );

    // ② 分组下拉项：「插入」→「代码块」。
    let (_dir2, mut app2) = app_with_open_document();
    let ctx2 = fresh_ctx();
    let before2 = app2.current_content().unwrap_or_default();

    let opened = open_editor_toolbar_group(&ctx2, &mut app2, "插入");
    let item = UiAction::CodeBlock.label();
    let item_rect =
        rect_of(&opened, item).unwrap_or_else(|| panic!("「插入」下拉里应有「{item}」且当前可见"));

    click_at(&ctx2, &mut app2, item_rect.center());

    let after2 = app2.current_content().unwrap_or_default();
    assert_ne!(
        after2, before2,
        "点击工具栏下拉项「{item}」没有改变文档内容"
    );
    assert!(
        after2.contains("```"),
        "点击「{item}」应插入代码块围栏，实际内容：{after2:?}"
    );
}

// ============================================================================
// 回归：菜单按钮必须等高同排（第一个菜单项不得偏高）
// ============================================================================

/// 顶栏四个下拉按钮的**文字顶边必须一致**。
///
/// 回归：`ui.horizontal` 的初始行高只有 `interact_size.y`（18pt），而
/// `ComboBox` 按钮实际高约 23pt——行高比按钮矮时，egui 把每个按钮在
/// 「被前一个按钮撑高的行」里重新居中，逐个「下楼梯」，第一个菜单项
/// 明显偏高（实测顶边差约 5pt，与后续各项样式不一致）。
/// 修法：整行先占满 `topbar_height`，按钮在同一高度内居中（见
/// `draw_app_menu_bar`）。此用例把「等高同排」钉死。
#[test]
fn app_menu_buttons_share_one_row() {
    let (_dir, mut app) = app_with_open_document();
    let ctx = fresh_ctx();
    let painted = run_frame(&ctx, &mut app, vec![]);

    let labels = ["文件", "编辑", "视图", "设置"];
    let mut tops = Vec::new();
    for label in labels {
        let r = rect_of(&painted, label).unwrap_or_else(|| panic!("顶栏应有「{label}」菜单项"));
        // 只统计顶栏区域内的命中（侧栏标签也叫「文件」，但本用例已关侧栏）。
        if r.top() < top_region_height(&app) {
            tops.push(r.top());
        }
    }
    assert_eq!(tops.len(), 4, "四个下拉都应可见：{tops:?}");
    let lo = tops.iter().cloned().fold(f32::MAX, f32::min);
    let hi = tops.iter().cloned().fold(f32::MIN, f32::max);
    assert!(
        hi - lo < 1.0,
        "菜单按钮顶部不齐（第一个菜单项偏高）：tops={tops:?}"
    );
}

/// 编辑区工具栏的控件同样必须同排齐平（同一缺陷的第二个现场）。
///
/// 控件清单取自 `toolbar_control_labels()`——它同时包含平铺按钮
/// （「B 加粗」…）与下拉标题（「插入」…），因此两类控件会被一起检查。
/// 「一排同类按钮」这个视觉目标要求它们**顶边一致**，否则整行看上去
/// 仍是「两种控件混排」。
#[test]
fn editor_toolbar_buttons_share_one_row() {
    let (_dir, mut app) = app_with_open_document();
    let ctx = fresh_ctx();
    let painted = run_frame(&ctx, &mut app, vec![]);
    let top_limit = top_region_height(&app);

    let labels = toolbar_control_labels();
    assert!(labels.len() >= 4, "工具栏控件太少：{labels:?}");

    let mut tops = Vec::new();
    for label in &labels {
        // 只认编辑区工具栏那一行（在顶栏区域之下）的命中。
        let r = painted
            .iter()
            .filter(|(r, t)| t == label && r.top() > top_limit)
            .map(|(r, _)| *r)
            .next()
            .unwrap_or_else(|| panic!("工具栏应有控件「{label}」（且在顶栏之下）"));
        tops.push(r.top());
    }
    let lo = tops.iter().cloned().fold(f32::MAX, f32::min);
    let hi = tops.iter().cloned().fold(f32::MIN, f32::max);
    assert!(
        hi - lo < 1.0,
        "工具栏控件顶边不齐（两种控件混排的下楼梯）：tops={tops:?}，labels={labels:?}"
    );
}

// ==== 三条顶栏对齐（回归：侧栏标签行 / 编辑区工具栏 / 预览标题栏）====
//
// 三个窗格都是「顶栏 + 内容」的上下布局，顶栏必须像**一对**：
// 同高、内容垂直居中、栏下同一根分隔线、内容区从同一条水平线开始。
//
// # 缺陷现场（实测）
//
// 1. 预览区宿主是 `CentralPanel::default()`，自带 6pt 内边距，而编辑区 /
//    侧栏是 `SidePanel`（竖向 2pt）——预览整体低 4pt、矮 8pt，三个顶栏
//    的文字中心分别落在 y=113.5 / 119 / 125（11.5pt 的下楼梯）。
// 2. 侧栏标签行只取控件默认高（23），另两条栏是 34。
//
// # 修复后的不变量
//
// 三个窗格的**内矩形**同起同止；三条顶栏的文字中心相差 <1pt；
// 三条分隔线段在同一根水平线上。
#[test]
fn three_pane_top_bars_align() {
    let (dir, mut app) = app_fixture();
    let file = dir.path().join("note.md");
    std::fs::write(&file, "# 标题\n\n正文一行。\n").expect("写文件应成功");
    app.open_path(&file.to_string_lossy()).expect("打开应成功");
    let ctx = fresh_ctx();
    // egui 首帧是测量 pass：控件尺寸与稳定帧差 1pt（菜单栏 43 vs 44），
    // 而本用例断言「三条顶栏文字中心相差 <1pt」——直接取首帧会稳定误报。
    crate::support::settle(&ctx, &mut app, SCREEN);

    let out = crate::support::run_ui(&ctx, raw_input(vec![]), |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(&mut app, ui);
    });
    let mut texts: Vec<(egui::Rect, String)> = Vec::new();
    let mut fills: Vec<(egui::Rect, egui::Color32)> = Vec::new();
    let mut segs: Vec<egui::Rect> = Vec::new();
    fn walk(
        s: &egui::epaint::Shape,
        t: &mut Vec<(egui::Rect, String)>,
        f: &mut Vec<(egui::Rect, egui::Color32)>,
        l: &mut Vec<egui::Rect>,
    ) {
        match s {
            egui::epaint::Shape::Text(x) => t.push((
                egui::Rect::from_min_size(x.pos, x.galley.size()),
                x.galley.text().to_owned(),
            )),
            egui::epaint::Shape::Rect(x) => f.push((x.rect, x.fill)),
            egui::epaint::Shape::LineSegment { points: p, .. } => {
                l.push(egui::Rect::from_two_pos(p[0], p[1]));
            }
            egui::epaint::Shape::Vec(v) => v.iter().for_each(|y| walk(y, t, f, l)),
            _ => {}
        }
    }
    for c in &out.shapes {
        walk(&c.shape, &mut texts, &mut fills, &mut segs);
    }

    // ---- ① 三条顶栏的文字中心同一水平线 ----
    // （文字矩形中心比控件矩形更稳：按钮 / 标签 / 纯文字三种控件的外框
    // 各不相同，但同一行里**文字**必须居中在同一高度。）
    //
    // ⚠ 必须限定 `left() > 60`：「文件」在侧栏出现**两次**——活动栏的导航项
    // （035 起新增，x 落在 48pt 宽的活动栏内）与内容面板的作用域标题。
    // 本用例要的三条**顶栏**是「侧栏内容面板标题行 / 编辑区工具栏 / 预览标题栏」；
    // 活动栏是纵向导航列、不是顶栏，它的首项与工具栏并不要求同高。不加这个
    // 限定，`find` 会取到活动栏那个（绘制序在前），量到的是另一个控件。
    let center_of = |needle: &str| -> f32 {
        texts
            .iter()
            .find(|(r, t)| t == needle && r.top() > 95.0 && r.top() < 160.0 && r.left() > 60.0)
            .map(|(r, _)| r.center().y)
            .unwrap_or_else(|| panic!("顶栏应有「{needle}」且在顶栏行内"))
    };
    let sidebar_c = center_of("文件");
    let toolbar_c = center_of("B 加粗");
    let preview_c = center_of("预览");
    for (name, c) in [("工具栏按钮", toolbar_c), ("预览标题", preview_c)] {
        assert!(
            (c - sidebar_c).abs() < 1.0,
            "{name}的文字中心 y={c}，与侧栏标签（{sidebar_c}）差超过 1pt——三条顶栏没对齐"
        );
    }

    // ---- ② 三个窗格的内矩形同起同止 ----
    // 面板 fill 铺满整个 SidePanel/CentralPanel（含边距区），而窗格自己
    // `rect_filled` 画的底色正好是**内矩形**——用它比对。
    //
    // 预览宿主现有**两个**主色矩形：frame 填充（外，覆盖 2pt 边距条，
    // 防清屏黑边回归——见 `preview_border_test`）与内容底色（内）。
    // 内矩形（编辑区 + 预览内容区）仍必须同起同止；外矩形必须恰好
    // 外扩 2pt 包住预览内矩形。
    let panes: Vec<egui::Rect> = fills
        .iter()
        .filter(|(r, c)| {
            *c == app.ui.theme.bg_primary && r.min.y > 95.0 && r.min.y < 130.0 && r.width() > 100.0
        })
        .map(|(r, _)| *r)
        .collect();
    assert_eq!(
        panes.len(),
        3,
        "应有编辑区内矩形 + 预览内矩形 + 预览 frame 外矩形三个主色矩形：{panes:?}"
    );
    let outer = panes
        .iter()
        .copied()
        .find(|r| (r.top() - 100.0).abs() < 1.0)
        .expect("预览宿主 frame 填充矩形（外）应存在");
    let inner: Vec<egui::Rect> = panes
        .iter()
        .copied()
        .filter(|r| r.top() > outer.top() + 1.0)
        .collect();
    assert_eq!(
        inner.len(),
        2,
        "编辑区 / 预览区应各有一个内容底色内矩形：{inner:?}"
    );
    let tops: Vec<f32> = inner.iter().map(|r| r.top()).collect();
    let bottoms: Vec<f32> = inner.iter().map(|r| r.bottom()).collect();
    let same = |xs: &[f32]| {
        xs.iter().cloned().fold(f32::MAX, f32::min) == xs.iter().cloned().fold(f32::MIN, f32::max)
    };
    assert!(
        same(&tops) && same(&bottoms),
        "编辑区与预览区的内容区没有从同一条水平线开始、到同一条线结束：\
         tops={tops:?} bottoms={bottoms:?}（通常是预览宿主面板边距与 SidePanel 不一致）"
    );
    // frame 外矩形必须恰好外扩 2pt 包住预览内矩形：少 1pt 是边距没盖满
    // （清屏黑边回归），多 1pt 是边距与 SidePanel 不一致（顶栏错位回归）。
    let preview_inner = inner
        .iter()
        .copied()
        .find(|r| outer.contains_rect(*r))
        .expect("预览 frame 外矩形应包住预览内容内矩形");
    assert!(
        (outer.top() + 2.0 - preview_inner.top()).abs() < 0.5
            && (outer.bottom() - 2.0 - preview_inner.bottom()).abs() < 0.5,
        "预览 frame 填充应恰好覆盖上下各 2pt 边距条：outer={outer:?} inner={preview_inner:?}"
    );

    // ---- ③ 三条顶栏下的分隔线在同一根水平线 ----
    let bar_seps: Vec<egui::Rect> = segs
        .iter()
        .filter(|r| r.min.y > 135.0 && r.min.y < 155.0 && r.width() > 100.0)
        .cloned()
        .collect();
    assert_eq!(
        bar_seps.len(),
        3,
        "三个窗格顶栏下应各有一根分隔线：{bar_seps:?}"
    );
    let sep_ys: Vec<f32> = bar_seps.iter().map(|r| r.min.y).collect();
    assert!(
        same(&sep_ys),
        "三条顶栏分隔线不在同一根水平线上：{sep_ys:?}"
    );
}

// ==== 回车键：换行与列表序号自增（回归：flush_edit 钳制基准错位）====
//
// # 缺陷现场（实测）
//
// 「回车退出列表」的变换会把文本**变短**（拉起点删掉整个 `4. ` 标记、
// 只插入一个 `\n`），编辑区先把 `editor_text` 拼成变换后的新串再调
// `flush_edit`；而 `flush_edit` 把「相对**旧**文本的字节区间」按**新**文本
// 的长度钳制——`byte_end`（旧插入点）超出新长度被砍短，领域层只删掉
// 标记的前半个字符：空列表项 `4. ` 上回车后残留成 `. `，此后每次回车
// 都只是把残渣往下推，再也续不出正常序号。表象即「按回车不换行、
// 序列号错乱」。
fn pressed_key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
        physical_key: None,
    }
}

/// 把焦点给编辑区 `TextEdit` 并跑一帧。
///
/// 编辑区的文字被高亮 layouter 切成多段 Text 指令，按整段文字定位会
/// 命中预览区那份同文，故直接对已知 `Id` 请求焦点。
fn focus_editor(ctx: &egui::Context, app: &mut EditorApp) {
    ctx.memory_mut(|m| m.request_focus(EditorApp::editor_id()));
    run_frame(ctx, app, vec![]);
}

#[test]
fn enter_key_creates_lines_and_keeps_list_numbers_intact() {
    let (dir, mut app) = app_fixture();
    let file = dir.path().join("note.md");
    std::fs::write(&file, "1. first item").expect("写文件应成功");
    app.open_path(&file.to_string_lossy()).expect("打开应成功");
    let ctx = fresh_ctx();

    let _ = run_frame(&ctx, &mut app, vec![]);
    focus_editor(&ctx, &mut app);

    // ① 行尾回车 → 正确换行，序号自增。
    run_frame(&ctx, &mut app, vec![pressed_key(egui::Key::End)]);
    run_frame(&ctx, &mut app, vec![]);
    run_frame(&ctx, &mut app, vec![pressed_key(egui::Key::Enter)]);
    assert_eq!(
        app.current_content().as_deref(),
        Some("1. first item\n2. "),
        "行尾回车应换行并续出下一序号"
    );

    // ② 空列表项上回车 → 退出列表：**整段**标记删除、只留空行。
    //    残留 `. ` / `2. ` 被下移都是钳制基准错位的表现。
    run_frame(&ctx, &mut app, vec![pressed_key(egui::Key::Enter)]);
    assert_eq!(
        app.current_content().as_deref(),
        Some("1. first item\n\n"),
        "空列表项上回车应删除整个标记退出列表"
    );

    // ③ 再回车 → 普通空行（不再受残留标记干扰）。
    run_frame(&ctx, &mut app, vec![pressed_key(egui::Key::Enter)]);
    assert_eq!(
        app.current_content().as_deref(),
        Some("1. first item\n\n\n"),
        "退出列表后的回车应是普通换行"
    );
}

#[test]
fn enter_key_continues_multi_digit_list_numbers() {
    let (dir, mut app) = app_fixture();
    let file = dir.path().join("note.md");
    std::fs::write(&file, "10. 第十条").expect("写文件应成功");
    app.open_path(&file.to_string_lossy()).expect("打开应成功");
    let ctx = fresh_ctx();

    let _ = run_frame(&ctx, &mut app, vec![]);
    focus_editor(&ctx, &mut app);

    run_frame(&ctx, &mut app, vec![pressed_key(egui::Key::End)]);
    run_frame(&ctx, &mut app, vec![]);
    run_frame(&ctx, &mut app, vec![pressed_key(egui::Key::Enter)]);
    assert_eq!(
        app.current_content().as_deref(),
        Some("10. 第十条\n11. "),
        "两位数序号也应自增续行（数字段必须整段剥离）"
    );
}

// ==== 回车键 × 中文输入法：预编辑逐帧变化 + 候选确认 + 回车换行 ====
//
// # 为什么必须模拟 Preedit（此前所有回车用例都漏了这一层）
//
// egui 0.33 的 `Preedit` 处理会把预编辑串**真实写进文本缓冲区**，并靠
// `state.cursor` 保持「(组合锚点, 预编辑串尾)」选区，下一次 `Preedit` /
// `Commit` 靠它删旧换新；`Commit` 还会校验锚点，不在组合起点则**整个
// 提交被静默忽略**。应用若在组合期的帧间回设折叠光标，预编辑串就会
// 逐帧累积（实测 "s"→"ssa"→"ssasadsadaf"）、中文永远进不来——表象
// 正是用户报告的「回车不换行、行号一直涨」。真实 winit 每敲一个字母
// 来一次 `Preedit`，且逐帧渲染，帧间隔必然存在。
#[test]
fn ime_preedit_commit_and_enter_keeps_text_intact() {
    let (dir, mut app) = app_fixture();
    let file = dir.path().join("note.md");
    std::fs::write(&file, "").expect("写文件应成功");
    app.open_path(&file.to_string_lossy()).expect("打开应成功");
    let ctx = fresh_ctx();

    let _ = run_frame(&ctx, &mut app, vec![]);
    focus_editor(&ctx, &mut app);

    // ① 组合：预编辑逐帧演进（真实节奏：每个字母一帧，帧间留空帧），
    //    候选确认后中文必须**替换**预编辑串，拼音不得残留。
    run_frame(&ctx, &mut app, vec![ime_enabled_event()]);
    run_frame(&ctx, &mut app, vec![]);
    for pre in ["s", "sa", "sad", "sadaf"] {
        run_frame(
            &ctx,
            &mut app,
            vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: pre.to_owned(),
                active_range_chars: None,
            })],
        );
        run_frame(&ctx, &mut app, vec![]);
    }
    run_frame(
        &ctx,
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Commit(
            "撒打发第三方".to_owned(),
        ))],
    );
    run_frame(&ctx, &mut app, vec![]);
    assert_eq!(
        app.current_content().as_deref(),
        Some("撒打发第三方"),
        "候选确认后应只留下中文，预编辑串必须被替换而非累积"
    );

    // ② 行尾回车：正常换行。
    run_frame(&ctx, &mut app, vec![pressed_key(egui::Key::End)]);
    run_frame(&ctx, &mut app, vec![]);
    run_frame(&ctx, &mut app, vec![pressed_key(egui::Key::Enter)]);
    run_frame(&ctx, &mut app, vec![]);
    assert_eq!(
        app.current_content().as_deref(),
        Some("撒打发第三方\n"),
        "组合结束后回车应正常换行"
    );

    // ③ 第二轮组合 + 提交 + 回车：连续中英混输不得互相污染。
    for pre in ["a", "ai", "aidi"] {
        run_frame(
            &ctx,
            &mut app,
            vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: pre.to_owned(),
                active_range_chars: None,
            })],
        );
        run_frame(&ctx, &mut app, vec![]);
    }
    run_frame(
        &ctx,
        &mut app,
        vec![egui::Event::Ime(egui::ImeEvent::Commit(
            "爱迪生".to_owned(),
        ))],
    );
    run_frame(&ctx, &mut app, vec![]);
    run_frame(&ctx, &mut app, vec![pressed_key(egui::Key::Enter)]);
    run_frame(&ctx, &mut app, vec![]);
    assert_eq!(
        app.current_content().as_deref(),
        Some("撒打发第三方\n爱迪生\n"),
        "第二轮组合提交后回车应换行，两轮之间不得有残留"
    );
}

// ==== 行号栏：光标字节偏移落进多字节字符中间不得 panic ====
//
// 程序化设值（跳转、撤销回读）只按**长度**夹紧光标字节偏移，完全可能
// 落在汉字中间。行号栏取「当前行」时按字节切片，非字符边界会 panic——
// `panic = "abort"` 下就是整窗口消失（实测崩溃日志
// crash-20261011005255：3887 落在「右」的 3885..3888 中间）。
// 状态栏 `cursor_line_col` 早有同款防护，行号栏补齐。
#[test]
fn gutter_survives_cursor_inside_multibyte_char() {
    let (dir, mut app) = app_fixture();
    let file = dir.path().join("cn.md");
    std::fs::write(&file, "第一行中文\n第二行").expect("写文件应成功");
    app.open_path(&file.to_string_lossy()).expect("打开应成功");
    let ctx = fresh_ctx();
    let _ = run_frame(&ctx, &mut app, vec![]);

    // 「一」占字节 0..3；偏移 2 落在它中间。
    app.editor.cursor = 2;
    let _ = run_frame(&ctx, &mut app, vec![]);
    // 下取整到字符边界后应高亮第 1 行（0-based）。
    assert_eq!(app.cursor_line_col().0, 1, "光标钳到「一」所在行");
}
