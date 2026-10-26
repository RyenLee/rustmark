//! 标签条几何用例（问题 037 续 / 038）。
//!
//! # 为什么必须在渲染层断言
//!
//! 「单行横向滚动」看起来是 `ScrollArea::horizontal()` 的属性，其实**不是**：
//! 滚动区只决定「超出视口时能不能滚」，而内层 Ui 的**布局方向**决定标签
//! 沿哪个轴排布。egui 的 `ScrollArea` 内层 Ui 继承父级 `top_down` 布局，
//! `allocate_exact_size` 于是**纵向**推进光标——标签逐行堆叠，
//! 第二行越出 `TopBottomPanel::exact_height` 后无人绘制（黑框）。
//! 截图里的症状正是「标签竖着排」。只有从真实帧里取回每个标题的矩形，
//! 才能钉住「同一行」这条不变量。
//!
//! 第二条钉住的是「全部关闭」按钮：它必须常驻条带右端、不随标签滚动
//! ——标签多到要滚动时，恰恰是用户最需要它的时刻。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use std::sync::Arc;

/// 验收窗口尺寸（与 `top_menu_test` 一致）。
const SCREEN: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// 「同一行」判定：同一行内文字中心 y 相差不超过 1pt。
const SAME_ROW_EPS: f32 = 1.0;

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

/// 打开 `n` 份文档（标题即 `doc{i}`——文件标题是去扩展名的词干，
/// 与标签条上显示的一致，短到不会被截断）。
fn open_n(app: &mut EditorApp, dir: &std::path::Path, n: usize) {
    for i in 0..n {
        let path = dir.join(format!("doc{i}.md"));
        std::fs::write(&path, format!("body {i}\n")).expect("写文件应成功");
        app.open_path(&path.to_string_lossy()).expect("打开应成功");
    }
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

/// 从一帧的绘制指令里取回所有**可见**文字及其占据的矩形。
///
/// 必须按 `ClippedShape.clip_rect` 过滤：滚动区把内容剪掉时，文字形状
/// 仍然会出现在 `shapes` 里，只是 clip 到视口——不过滤就会把「已经滚走
/// 的按钮」误判为可见，而这正是本文件要钉住的区别。
fn painted_texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(egui::Rect, String)> {
    let mut out = Vec::new();
    for clipped in shapes {
        let clip = clipped.clip_rect;
        for (rect, text) in walk_collect(&clipped.shape) {
            if clip.contains(rect.center()) {
                out.push((rect, text));
            }
        }
    }
    out
}

/// 收集一个形状里的全部文字（不做可见性过滤）。
fn walk_collect(shape: &egui::epaint::Shape) -> Vec<(egui::Rect, String)> {
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
    walk(shape, &mut out);
    out
}

/// 空跑一帧，让布局**落定**（见 `crate::support::settle` 的说明）。
///
/// 本文件必须用它：`run_frame`（取文字画面）与 `out_shapes`（取图标锚点）
/// 各自跑一帧，若其一落在 egui 的首帧测量 pass 上，两者的行中线就差 2pt，
/// 超过 `SAME_ROW_EPS`（1pt）而误报「排成竖向」。
fn settle(ctx: &egui::Context, app: &mut EditorApp) {
    crate::support::settle(ctx, app, SCREEN);
}

fn run_frame(
    ctx: &egui::Context,
    app: &mut EditorApp,
    events: Vec<egui::Event>,
) -> Vec<(egui::Rect, String)> {
    settle(ctx, app);
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
fn click_at(ctx: &egui::Context, app: &mut EditorApp, pos: egui::Pos2) {
    run_frame(
        ctx,
        app,
        vec![egui::Event::PointerMoved(pos), press_at(pos)],
    );
    run_frame(
        ctx,
        app,
        vec![egui::Event::PointerMoved(pos), release_at(pos)],
    );
}

/// 某段文字的全部矩形（面包屑里也有文件名，故不能只取第一个）。
fn rects_of(painted: &[(egui::Rect, String)], text: &str) -> Vec<egui::Rect> {
    painted
        .iter()
        .filter(|(_, t)| t == text)
        .map(|(r, _)| *r)
        .collect()
}

/// 顶栏总高（菜单栏 + 标签条 + 面包屑）——把断言关在顶栏里。
fn top_region_height(app: &EditorApp) -> f32 {
    let m = app.theme().metrics;
    m.topbar_height + m.tabstrip_height + m.tabbar_height
}

/// 标签条带的 y 区间：顶栏 = 菜单栏(40) + 分隔线 + 标签条(32) + 面包屑(24)，
/// 条带大致 [top_h−56, top_h−24]——菜单栏行（y≈20）的下拉三角被排除在外。
fn strip_rect(app: &EditorApp) -> egui::Rect {
    let top_h = top_region_height(app);
    egui::Rect::from_min_max(
        egui::pos2(0.0, top_h - 56.0),
        egui::pos2(SCREEN.x, top_h - 24.0),
    )
}

/// 从一帧绘制指令里找「全部关闭」按钮的矩形。
///
/// 041 起按钮本体是**描边方框 + × 线段**（没有文字了）：条带内的矩形形状里，
/// 标签底色都是填充（stroke 无宽）、悬停底色只在悬停帧出现，分隔线是线段——
/// 唯一带描边的矩形就是这枚图标的外框，取最右者做锚点。
fn close_all_icon_rect(shapes: &[egui::epaint::ClippedShape], strip: egui::Rect) -> egui::Rect {
    fn walk(shape: &egui::epaint::Shape, strip: egui::Rect, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::epaint::Shape::Rect(r) => {
                if r.fill.a() == 0 && r.stroke.width > 0.0 && strip.contains(r.rect.center()) {
                    out.push(r.rect);
                }
            }
            egui::epaint::Shape::Vec(inner) => {
                for s in inner {
                    walk(s, strip, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        walk(&clipped.shape, strip, &mut out);
    }
    out.sort_by(|a, b| a.left().total_cmp(&b.left()));
    out.pop().expect("条带内应画出「全部关闭」图标（描边方框）")
}

// ============================================================================
// 用例
// ============================================================================

/// 标签必须排成**一行**（问题 038 的核心回归）。
///
/// 坏形态（修复前实测）：`ScrollArea` 内层 Ui 是 `top_down` 布局，
/// 每个标签占 168×24 并**向下**推进光标——八个标签占八行，
/// 只有前一两行落在 32pt 的条带里，其余画到面板外。
/// 判据取「每个标题都有一段文字落在同一 y 上」：面包屑里也有当前
/// 文件名，但它在另一行，不影响「存在同一行」的判定。
#[test]
fn tabs_lay_out_in_a_single_horizontal_row() {
    let (dir, mut app) = app_fixture();
    // 6 个 × 170pt ≈ 1020pt，稳稳落在 1280pt 视口内——每个标题都完整
    // 可见，断言不依赖「第几个恰好没被裁掉」。
    open_n(&mut app, dir.path(), 6);
    let ctx = fresh_ctx();
    let painted = run_frame(&ctx, &mut app, Vec::new());

    // 以「全部关闭」图标为行锚：它和标签同条带，位置确定。
    let anchor = close_all_icon_rect(&out_shapes(&ctx, &mut app), strip_rect(&app));
    let row_y = anchor.center().y;

    let mut xs = Vec::new();
    for i in 0..6 {
        let title = format!("doc{i}");
        let on_row = rects_of(&painted, &title)
            .into_iter()
            .find(|r| (r.center().y - row_y).abs() <= SAME_ROW_EPS);
        let rect = on_row.unwrap_or_else(|| {
            panic!("标签「{title}」不在标签行上（行锚 y={row_y}）——多半又排成竖向了")
        });
        xs.push((title, rect.center().x));
    }
    // 同一行还不够：必须从左到右按打开顺序递增。
    for pair in xs.windows(2) {
        assert!(
            pair[1].1 > pair[0].1,
            "标签顺序应为打开顺序且沿 x 递增：{:?}",
            xs
        );
    }
}

/// 标签多到超出视口时，「全部关闭」仍常驻右端可见。
///
/// 12 个标签 × 170pt ≈ 2040pt，远超 1280pt 视口——滚动区此时必然在
/// 滚动状态。按钮若放在滚动区里会跟着滚走，那正是它最该在场的时刻。
#[test]
fn close_all_stays_visible_when_tabs_overflow() {
    let (dir, mut app) = app_fixture();
    open_n(&mut app, dir.path(), 12);
    let ctx = fresh_ctx();
    let painted = run_frame(&ctx, &mut app, Vec::new());

    let button = close_all_icon_rect(&out_shapes(&ctx, &mut app), strip_rect(&app));
    // 右端：落在屏幕右侧 1/4 内（按钮在条带最右，前面是 12 个标签）。
    assert!(
        button.center().x > SCREEN.x * 0.75,
        "「全部关闭」应常驻条带右端，实际 x={:.0}",
        button.center().x
    );
    // 顶栏之内：不越出顶部面板（越出即无人绘制的黑框家族）。
    assert!(
        button.bottom() <= top_region_height(&app),
        "「全部关闭」越出顶栏（bottom={:.0}，顶栏高={:.0}）",
        button.bottom(),
        top_region_height(&app)
    );
    // 至少有一个标签与按钮**同行**可见，且都在按钮左侧。
    //
    // ⚠ 不能钉 `doc0`：活动标签（最后打开的 `doc11`）变化时会自动滚入视野
    // （见 `draw_tab_strip` 的 reveal 逻辑），溢出场景下最左的 `doc0` 本来就
    // 会被滚出视口——那是**有意**行为，不是缺陷。旧写法能过只是因为被测帧
    // 恰好是首帧（reveal 的写回要到下一帧才生效、首帧仍画在 offset 0），
    // 属于「碰巧通过」（041 教训：时间 / 跨帧行为必须以稳定帧取证）。
    let visible_on_row: Vec<egui::Rect> = (0..12)
        .flat_map(|i| rects_of(&painted, &format!("doc{i}")))
        .filter(|r| (r.center().y - button.center().y).abs() <= SAME_ROW_EPS)
        .collect();
    assert!(
        !visible_on_row.is_empty(),
        "溢出时仍应有标签与「全部关闭」同行可见"
    );
    for r in &visible_on_row {
        assert!(
            r.center().x < button.center().x,
            "「全部关闭」应画在所有可见标签右侧（标签 x={:.0}，按钮 x={:.0}）",
            r.center().x,
            button.center().x
        );
    }
}

/// 点「全部关闭」→ 一个不剩，回到欢迎页状态。
#[test]
fn clicking_close_all_closes_every_document() {
    let (dir, mut app) = app_fixture();
    open_n(&mut app, dir.path(), 3);
    let ctx = fresh_ctx();
    // 先跑一帧让控件就位，再按「全部关闭」图标的中心点击。
    let center = close_all_icon_rect(&out_shapes(&ctx, &mut app), strip_rect(&app)).center();

    click_at(&ctx, &mut app, center);

    assert!(
        app.open_document_list().is_empty(),
        "点「全部关闭」后应一个文档都不剩"
    );
    assert!(app.should_show_welcome(), "应回到欢迎页状态");
}

/// 条带高度纪律：所有标签与按钮都在顶栏预算内（不撑高面板）。
///
/// 这条与 `preview_border_test` 互补：那里断言面板总高与中央区顶边，
/// 这里断言**条带内容自身**不越界——换行 / 竖向堆叠会同时违反两条。
#[test]
fn the_strip_stays_within_the_top_budget() {
    let (dir, mut app) = app_fixture();
    open_n(&mut app, dir.path(), 6);
    let ctx = fresh_ctx();
    let painted = run_frame(&ctx, &mut app, Vec::new());

    let limit = top_region_height(&app);
    let button = close_all_icon_rect(&out_shapes(&ctx, &mut app), strip_rect(&app));
    for i in 0..6 {
        let title = format!("doc{i}");
        let rect = rects_of(&painted, &title)
            .into_iter()
            .find(|r| (r.center().y - button.center().y).abs() <= SAME_ROW_EPS)
            .unwrap_or_else(|| panic!("标签「{title}」不在标签行上"));
        assert!(
            rect.bottom() <= limit,
            "标签「{title}」越出顶栏（bottom={:.0}，预算={limit:.0}）",
            rect.bottom()
        );
    }
    assert!(
        button.bottom() <= limit,
        "「全部关闭」越出顶栏（bottom={:.0}，预算={limit:.0}）",
        button.bottom()
    );
}

/// 从一帧画面里收集**标签条带内**的三角形（导航箭头是 3 顶点闭合路径；
/// 菜单栏的下拉箭头也是三角形，但它在菜单栏行——按条带 y 区间过滤掉）。
fn nav_arrow_rects(shapes: &[egui::epaint::ClippedShape], strip: egui::Rect) -> Vec<egui::Rect> {
    fn walk(shape: &egui::epaint::Shape, strip: egui::Rect, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::epaint::Shape::Path(p) if p.closed && p.points.len() == 3 => {
                let rect = p.visual_bounding_rect();
                if strip.contains(rect.center()) {
                    out.push(rect);
                }
            }
            egui::epaint::Shape::Vec(inner) => {
                for s in inner {
                    walk(s, strip, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        walk(&clipped.shape, strip, &mut out);
    }
    out.sort_by(|a, b| a.center().x.total_cmp(&b.center().x));
    out
}

/// 溢出时必须有左右导航箭头，点击能把标签滚出/滚回视口（问题 040 / 041）。
///
/// 修复前只有 `AlwaysHidden` 的横向滚动区：滚轮不动横向、滚动条不画，
/// 纯鼠标用户没有任何滚动手段——12 个标签的后 5 个永远看不到。
/// 041 起滚入是**即时**的：打开序最后一个 doc11 是活动标签，首帧即把
/// 滚动区定位到右端（旧实现的动画在真实时间里也会到同一位置），
/// 故步进断言从右端状态出发：◀ 把 doc11 滚出右缘、▶ 把它滚回来。
#[test]
fn nav_arrows_scroll_the_strip_when_tabs_overflow() {
    let (dir, mut app) = app_fixture();
    open_n(&mut app, dir.path(), 12);
    let ctx = fresh_ctx();
    let strip = strip_rect(&app);
    let close_all = close_all_icon_rect(&out_shapes(&ctx, &mut app), strip);
    let arrows = nav_arrow_rects(&out_shapes(&ctx, &mut app), strip);
    assert!(
        arrows.len() >= 2,
        "12 个标签必然溢出，应画出左右两个导航箭头，实际：{:?}",
        arrows
    );
    // 箭头都在「全部关闭」左侧（中间隔着导航位）。
    for rect in &arrows {
        assert!(
            rect.right() < close_all.left(),
            "导航箭头应在「全部关闭」左侧：箭头 {:?} vs 按钮 {close_all:?}",
            rect
        );
    }
    // 活动标签滚入语义：doc11（最后打开）可见，doc0 已在视口左外。
    let painted = run_frame(&ctx, &mut app, Vec::new());
    let doc11 = rects_of(&painted, "doc11")
        .into_iter()
        .next()
        .expect("活动标签 doc11 滚入后应可见");
    assert!(doc11.center().x < close_all.center().x);
    assert!(
        rects_of(&painted, "doc0").is_empty(),
        "滚到右端后 doc0 不应可见"
    );

    // 点最左的箭头（◀）一次：滚动一个标签宽，doc11 应滚出右缘、doc10 进入。
    let prev = arrows[0].center();
    click_at(&ctx, &mut app, prev);
    let painted = run_frame(&ctx, &mut app, Vec::new());
    assert!(
        rects_of(&painted, "doc11").is_empty(),
        "点 ◀ 一次后 doc11 应滚出视口（约一个标签宽）"
    );
    let close_all = close_all_icon_rect(&out_shapes(&ctx, &mut app), strip);
    let doc10 = rects_of(&painted, "doc10")
        .into_iter()
        .find(|r| (r.center().y - close_all.center().y).abs() <= SAME_ROW_EPS)
        .expect("点 ◀ 后 doc10 应在标签行上可见");
    assert!(doc10.center().x < close_all.center().x);

    // 点最右的箭头（▶）：回到右端，doc11 重新可见——两箭头互为逆操作。
    let next = arrows[arrows.len() - 1].center();
    click_at(&ctx, &mut app, next);
    let painted = run_frame(&ctx, &mut app, Vec::new());
    assert!(
        !rects_of(&painted, "doc11").is_empty(),
        "点 ▶ 后 doc11 应回到视口"
    );
}

/// 跑一帧并返回原始绘制指令（导航箭头是向量形状，`painted_texts` 不收）。
fn out_shapes(ctx: &egui::Context, app: &mut EditorApp) -> Vec<egui::epaint::ClippedShape> {
    settle(ctx, app);
    let out = crate::support::run_ui(ctx, raw_input(Vec::new()), |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(app, ui);
    });
    out.shapes
}

/// 带**显式时间**跑一帧（问题 041 回归需要）。
///
/// 测试帧的墙钟间隔是微秒级，egui 的滚动动画在这种节奏下永远走不完，
/// 「动画目标把手写 offset 拉回去」的缺陷根本来不及发作——显式给
/// `RawInput.time` 才能确定性地驱动动画时间轴。
fn run_frame_at(
    ctx: &egui::Context,
    app: &mut EditorApp,
    events: Vec<egui::Event>,
    time: f64,
) -> Vec<(egui::Rect, String)> {
    let mut input = raw_input(events);
    input.time = Some(time);
    let out = crate::support::run_ui(ctx, input, |ui| {
        rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
        rustmark::presentation::views::main_view::show(app, ui);
    });
    painted_texts(&out.shapes)
}

/// 在显式时间轴上完成一次 ◀ 点击（按下 t_press / 抬起 t_release）。
fn click_prev_at_time(
    ctx: &egui::Context,
    app: &mut EditorApp,
    pos: egui::Pos2,
    t_press: f64,
    t_release: f64,
) {
    run_frame_at(
        ctx,
        app,
        vec![egui::Event::PointerMoved(pos), press_at(pos)],
        t_press,
    );
    run_frame_at(
        ctx,
        app,
        vec![egui::Event::PointerMoved(pos), release_at(pos)],
        t_release,
    );
}

// ============================================================================
// 问题 041：◀ 失效（动画目标覆盖手动 offset）与「全部关闭」图标化
// ============================================================================

/// 点 ◀ 要能把标签滚回、▶ 再滚回去，且步进可累积（问题 041 回归）。
///
/// 修复前 ◀ 在「滚入目标残留」时整体失效（位移被动画逐帧拉回）；
/// 从右端状态连点两步再两步滚回，把「步进可累积」一并钉住。
#[test]
fn the_left_arrow_steps_back_and_the_right_arrow_undoes_it() {
    let (dir, mut app) = app_fixture();
    open_n(&mut app, dir.path(), 12);
    let ctx = fresh_ctx();
    let strip = strip_rect(&app);
    let arrows = nav_arrow_rects(&out_shapes(&ctx, &mut app), strip);
    assert!(arrows.len() >= 2, "12 个标签必然溢出，应画出左右箭头");
    let prev = arrows[0].center();
    let next = arrows[arrows.len() - 1].center();

    // 初始滚入右端：doc11（活动标签）可见。
    let painted = run_frame(&ctx, &mut app, Vec::new());
    assert!(
        !rects_of(&painted, "doc11").is_empty(),
        "初始 doc11 应在视口内"
    );

    // ◀ 两步：doc11、doc10 依次滚出右缘，doc9 进入视口。
    click_at(&ctx, &mut app, prev);
    click_at(&ctx, &mut app, prev);
    let painted = run_frame(&ctx, &mut app, Vec::new());
    assert!(
        rects_of(&painted, "doc11").is_empty() && rects_of(&painted, "doc10").is_empty(),
        "◀ 两步后 doc10/doc11 应滚出右缘——◀ 失效时此断言红"
    );
    assert!(
        !rects_of(&painted, "doc9").is_empty(),
        "◀ 两步后 doc9 应进入视口（步进要可累积）"
    );

    // ▶ 两步：滚回右端，doc11 重新可见——两箭头互为逆操作。
    click_at(&ctx, &mut app, next);
    click_at(&ctx, &mut app, next);
    let painted = run_frame(&ctx, &mut app, Vec::new());
    assert!(
        !rects_of(&painted, "doc11").is_empty(),
        "▶ 两步后 doc11 应滚回视口"
    );
}

/// 活动标签自动滚入之后，◀ 仍要生效（问题 041 的用户实测场景）。
///
/// 切到左边的标签触发滚入；旧实现用 `ui.scroll_to_rect`，动画目标存在
/// `State` 私有字段里、每帧把 offset 拉向旧目标——◀ 的手动位移被抹掉，
/// 表现即「点击失效」。新实现滚入直接写 offset（无动画目标可残留）。
#[test]
fn the_left_arrow_wins_after_the_active_tab_reveal() {
    use rustmark::presentation::shortcuts::registry::UiAction;

    let (dir, mut app) = app_fixture();
    open_n(&mut app, dir.path(), 12); // 打开序最后一个 doc11 是活动标签
    let ctx = fresh_ctx();
    let strip = strip_rect(&app);
    let prev = nav_arrow_rects(&out_shapes(&ctx, &mut app), strip)[0].center();

    // 时间轴（秒）：t=0.1 `PrevTab` 切到 doc10（滚入定位——旧实现在此留下
    // 一个 0.3s 的动画目标）；t=0.2 按下、t=0.3 抬起点 ◀；t=0.5 复核。
    // ◀ 是往回滚：内容**右移**，doc10 的文字 x 应增大约一个标签宽。
    app.execute(UiAction::PrevTab);
    let at_press = run_frame_at(&ctx, &mut app, Vec::new(), 0.1);
    let before = rects_of(&at_press, "doc10")
        .into_iter()
        .next()
        .expect("切到 doc10 后（滚入完成）它应在视口内")
        .center()
        .x;

    click_prev_at_time(&ctx, &mut app, prev, 0.2, 0.3);
    let after = run_frame_at(&ctx, &mut app, Vec::new(), 0.5);
    let doc10_x = rects_of(&after, "doc10")
        .into_iter()
        .next()
        .map(|r| r.center().x)
        .unwrap_or(f32::INFINITY); // 完全滚出视口也算「生效」

    assert!(
        doc10_x > before + 100.0,
        "点 ◀ 后 doc10 应右移约一个标签宽（{before:.0} → {doc10_x:.0}）；\
         若位置不动即动画目标把手写 offset 拉了回去（问题 041）"
    );
}

/// 导航箭头必须**双向都能步进、步进可累积、到头到底后停住**（问题 042 回归）。
///
/// 042 之前标签条的滚动偏移会被「别的滚动区泄漏过来的增量」污染：
/// egui 的 `scroll_delta` 是全局的、不分滚动区，而标签条在顶栏总是第一个
/// begin——它把编辑区 / 预览区的滚动请求整个抢走，offset 随机漂移
/// （实测出现过 584 → 481.5 → 595 这种既不按步长也不按方向的跳变），
/// 表现就是「箭头时灵时不灵」。修法是把偏移提升为应用状态、每帧末尾
/// 无条件写回，这里用「连续步进」把确定性钉住。
#[test]
fn nav_arrows_step_in_both_directions_and_stop_at_the_ends() {
    let (dir, mut app) = app_fixture();
    open_n(&mut app, dir.path(), 12);
    let ctx = fresh_ctx();
    let strip = strip_rect(&app);
    let arrows = nav_arrow_rects(&out_shapes(&ctx, &mut app), strip);
    assert!(arrows.len() >= 2, "12 个标签必然溢出，应画出左右箭头");
    let prev = arrows[0].center();
    let next = arrows[arrows.len() - 1].center();
    // 连点 ◀：一路滚到最左，doc0 必须出现。
    //
    // ⚠ 「doc0 可见」≠「已在最左」（问题 061 修正几何后暴露）：doc0 占
    // 内容坐标 [行左, 行左+槽宽]，偏移小于一个槽宽时它就已部分可见——
    // 旧步进（166 < 槽宽 170）恰好让「首次可见」与「偏移归零」同帧到达，
    // 掩盖了这个前提漏洞。故 doc0 出现后继续点 ◀ 直到位置不再变化。
    let mut seen_doc0 = false;
    for _ in 0..12 {
        click_at(&ctx, &mut app, prev);
        let painted = run_frame(&ctx, &mut app, Vec::new());
        if !rects_of(&painted, "doc0").is_empty() {
            seen_doc0 = true;
            break;
        }
    }
    assert!(seen_doc0, "连点 ◀ 应能把 doc0 滚进视口");
    let mut at_left = run_frame(&ctx, &mut app, Vec::new());
    for _ in 0..12 {
        click_at(&ctx, &mut app, prev);
        let next = run_frame(&ctx, &mut app, Vec::new());
        let settled = first_x(&at_left, "doc0") == first_x(&next, "doc0");
        at_left = next;
        if settled {
            break;
        }
    }

    // 到底后继续点 ◀ 不再变化（幂等，不抖动）。
    let at_left = run_frame(&ctx, &mut app, Vec::new());
    click_at(&ctx, &mut app, prev);
    let still_left = run_frame(&ctx, &mut app, Vec::new());
    assert_eq!(
        first_x(&at_left, "doc0"),
        first_x(&still_left, "doc0"),
        "已在最左时继续点 ◀ 不应改变视图"
    );

    // 连点 ▶：一路滚回最右，doc11 必须出现。
    let mut seen_doc11 = false;
    for _ in 0..12 {
        click_at(&ctx, &mut app, next);
        let painted = run_frame(&ctx, &mut app, Vec::new());
        if !rects_of(&painted, "doc11").is_empty() {
            seen_doc11 = true;
            break;
        }
    }
    assert!(seen_doc11, "连点 ▶ 应能把 doc11 滚回视口");
}

/// 取某个文字第一次出现处的中心 x（无该文字时给 `f32::INFINITY`）。
fn first_x(painted: &[(egui::Rect, String)], text: &str) -> f32 {
    rects_of(painted, text)
        .into_iter()
        .next()
        .map(|r| r.center().x)
        .unwrap_or(f32::INFINITY)
}

/// 分隔线不得压进导航箭头的热区（问题 043 截图实证）。
/// 「全部关闭」按钮与分隔线是**手写绝对坐标**：曾漏加面板内边距基准
/// `strip_left`，分隔线正好画在 ▶ 的矩形里、按钮与箭头贴死——headless
/// 测试用「箭头 vs 图标」比较测不出（图标在按钮内缩 14pt，恰好躲过）。
/// 这条测试直接收集条带内所有**竖直线段**，要求每一条都落在箭头矩形之外。
#[test]
fn the_close_all_divider_never_crosses_a_nav_arrow() {
    let (dir, mut app) = app_fixture();
    open_n(&mut app, dir.path(), 12);
    let ctx = fresh_ctx();
    let strip = strip_rect(&app);
    let shapes = out_shapes(&ctx, &mut app);
    let arrows = nav_arrow_rects(&shapes, strip);
    assert!(arrows.len() >= 2, "12 个标签必然溢出，应画出左右箭头");

    fn collect_verticals(shape: &egui::epaint::Shape, strip: egui::Rect, out: &mut Vec<f32>) {
        match shape {
            // 竖直线段：两端 x 相同、整体落在条带 y 区间内。
            egui::epaint::Shape::LineSegment { points: l, .. }
                if (l[0].x - l[1].x).abs() < 0.01
                    && strip.contains(l[0])
                    && strip.contains(l[1]) =>
            {
                out.push(l[0].x)
            }
            egui::epaint::Shape::Vec(inner) => {
                for s in inner {
                    collect_verticals(s, strip, out);
                }
            }
            _ => {}
        }
    }
    let mut vertical_xs: Vec<f32> = Vec::new();
    for clipped in &shapes {
        collect_verticals(&clipped.shape, strip, &mut vertical_xs);
    }
    assert!(!vertical_xs.is_empty(), "条带内应有一条「全部关闭」分隔线");
    for x in vertical_xs {
        for rect in &arrows {
            assert!(
                x < rect.left() - 1.0 || x > rect.right() + 1.0,
                "条带内的竖线 x={x:.1} 压进了箭头矩形 {rect:?}——\
                 「全部关闭」的坐标基准又漏了面板内边距（问题 043）"
            );
        }
    }
}

// ============================================================================
// 问题 061：页签重叠（槽位步进被 put 污染 + 标题中置右溢）
// ============================================================================

/// CJK 字形来自应用字库：默认 egui 字库无 CJK，标签标题会画成豆腐块。
fn fresh_ctx_with_app_fonts() -> egui::Context {
    rustmark::presentation::theme::mapper::clear_highlight_caches();
    let ctx = egui::Context::default();
    let setup = rustmark::infrastructure::fonts::font_definitions(None);
    ctx.set_fonts(rustmark::app::frame::to_egui_font_definitions(
        setup.definitions,
    ));
    crate::support::run_ui(&ctx, raw_input(Vec::new()), |_| {});
    ctx
}

/// 相邻页签不得重叠、标题不得右溢出自己的标签（问题 061 截图实证）。
///
/// 两层根因（探针实测，slot 8..178 → 174..344）：
///
/// 1. **槽位步进被 put 污染**：循环体内的 `ui.put`（标题 / × 按钮）会把
///    **父 Ui 的光标**推到 put 矩形右缘——最后落笔的 × 按钮右缘在
///    「标签右缘 − 2pt」，下一槽位若从光标出发，步进 = 槽宽 − 4 而非
///    槽宽，相邻标签底色重叠 2pt、× 距右邻标签仅 6pt。
/// 2. **标题中置右溢**：`ui.put` 的子布局是 `centered_and_justified`，
///    Label 的 galley 锚点取 `rect.center_top()`——**没被 elide 的标题从
///    put 矩形的水平中点起画**，长标题整体右移半个可用宽（实测 65pt）
///    压进标签右侧的关闭按钮区乃至条带右端。
///
/// 判据：× 按钮的 galley 宽度恒定，其**中心距 = 槽位步进**（修前 166 ≠
/// 176，修后相等）；长标题的 galley 右缘不得越过本标签的右缘（修前
/// 溢出 ~30pt，修后余量 ~35pt）。
#[test]
fn tabs_never_overlap_and_titles_stay_in_their_tab() {
    let (dir, mut app) = app_fixture();
    // 标题含一枚接近截断线的长名（不 elide、但修前必右溢）。
    let names = [
        "缓存改进",
        "问题记录1",
        "上线前体检报告-很长很长的标题测试",
        "第四篇",
    ];
    for name in names {
        let path = dir.path().join(format!("{name}.md"));
        std::fs::write(&path, "body\n").expect("写文件应成功");
        app.open_path(path.to_str().unwrap()).expect("打开应成功");
    }
    let ctx = fresh_ctx_with_app_fonts();
    let painted = run_frame(&ctx, &mut app, Vec::new());

    // 行锚与既有用例一致：「全部关闭」图标定位标签行的 y。
    let anchor = close_all_icon_rect(&out_shapes(&ctx, &mut app), strip_rect(&app));
    let row_y = anchor.center().y;

    // 槽位步进 = TAB_MAX_W(168) + TAB_GAP(8)；常量私有，此处按值钉住，
    // 改常量时这里会红——正是提醒同步本判据。
    const SLOT_PITCH: f32 = 176.0;
    const TAB_W: f32 = 168.0;

    // × 按钮的文字矩形（每标签一枚；关闭按钮 put 居中于其矩形，galley
    // 中心 = 按钮矩形中心 = 标签右缘 − 12pt）。
    let mut crosses: Vec<egui::Rect> = rects_of(&painted, "×")
        .into_iter()
        .filter(|r| (r.center().y - row_y).abs() <= SAME_ROW_EPS)
        .collect();
    crosses.sort_by(|a, b| a.center().x.total_cmp(&b.center().x));
    assert_eq!(
        crosses.len(),
        names.len(),
        "每个标签应恰有一枚 × 关闭按钮，实际：{crosses:?}"
    );
    for pair in crosses.windows(2) {
        let pitch = pair[1].center().x - pair[0].center().x;
        assert!(
            (pitch - SLOT_PITCH).abs() <= 0.5,
            "× 按钮步进 {pitch:.1} ≠ 槽位步进 {SLOT_PITCH:.0}——put 把父光标推到 \
             × 右缘、下一槽位随之左移，页签底色重叠（问题 061）"
        );
    }

    // 行左基准：第一枚 × 中心 = 行左 + TAB_W − 12（见上）。
    let row_left = crosses[0].center().x - (TAB_W - 12.0);
    // 「标题左缘 − 槽位左原点」在各标签间必须恒定 = 左对齐。
    //
    // # 为什么单靠上面「右缘不越标签」不够
    //
    // 居中偏移是 `(name_avail − 文字宽) / 2`：短标题约 60pt，而标题右缘到
    // 标签右缘的余量有 ~90pt——**短标题即使被居中也不会越界**。于是
    // `Label::halign(LEFT)` 在 egui 0.37 变成空操作（见
    // `widgets::text::put_elided` 的对照表）后，整整一个版本没人发现：
    // 用户看到的只是「未命名文档」这类短标题浮在标签中间偏右。
    //
    // 这条判据用**不同长度**的标题互相比对：左对齐时各标签的偏移相同；
    // 居中时偏移随文字宽度漂移（本组样本相差约 30pt）。它不依赖
    // `TAB_PAD` 的具体取值，改内边距不会误报。
    let mut left_offsets: Vec<f32> = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let title_rect = painted
            .iter()
            .filter(|(r, t)| {
                t.starts_with(&name[..name.chars().take(4).map(char::len_utf8).sum()])
                    && (r.center().y - row_y).abs() <= SAME_ROW_EPS
            })
            .map(|(r, _)| *r)
            .next()
            .unwrap_or_else(|| panic!("标题「{name}」不在标签行上"));
        let tab_left = row_left + i as f32 * SLOT_PITCH;
        let tab_right = tab_left + TAB_W;
        assert!(
            title_rect.right() <= tab_right + 0.5,
            "「{name}」标题右缘 {:.1} 越过本标签右缘 {tab_right:.1}——put 居中锚点\
             把标题画出了自己的标签（问题 061）",
            title_rect.right()
        );
        left_offsets.push(title_rect.left() - tab_left);
    }
    let min = left_offsets.iter().copied().fold(f32::INFINITY, f32::min);
    let max = left_offsets
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        max - min <= 1.0,
        "各标签标题左缘偏移不一致（{left_offsets:?}，极差 {:.1}pt）——标题没有从\
         槽位左缘起画，而是按自身宽度居中（问题 061 在 egui 0.37 下的复发形态）",
        max - min
    );
}
