//! 侧栏行内多段文本的**垂直对齐**回归（用户截图反馈：大纲行里
//! 「# + 标题 + 行号」不在一条水平线上，标题比其他两段低）。
//!
//! # 钉住的不变量
//!
//! 大纲行 / 检查行都由三段以上文本拼成（`#` 与标题、规则名与说明、
//! 右端行号）。它们必须落在同一条水平线上——任何一段整体下沉都会让
//! 用户觉得「这一行是斜的/错的」，即使功能完全正常。
//!
//! # 缺陷根因（为何值得一条专用回归）
//!
//! 行中段用 `allocate_ui_with_layout(vec2(avail, 0.0), …)` 约束宽度
//! （为了截断不溢出侧栏）。0 高的 `desired_size` 会被外层行的 Center
//! 交叉对齐**填满成行高**（`Layout::next_frame_ignore_wrap`），0 高子
//! 矩形于是落在**行垂直中点**；其中的文字再按 egui「横向布局不向上
//! 越界」规则（`frame_rect.top() < cursor.top()` 时整体下推）被压到
//! 中点之下——中段比行首段低半个行高（探针实测 8.5pt），右端行号也被
//! 0 高子矩形带偏 3.5pt。修法：子 Ui 高度传**行高**（取行首段标签的
//! 实测高度），子矩形与行重合，三段自然同线。
//!
//! # 检查行还钉住「行要排在列表里」（问题 065）
//!
//! `lint_row` 曾用 `with_layout` 排内容——它**不限定子 Ui 高度**，行内
//! 控件被 Center 交叉对齐放到滚动区剩余全部可用高的垂直中点（整行浮在
//! 面板中部），且每行游标暴涨约 512pt，只有第一行落在视口内。改用
//! `ui.horizontal`（子 Ui 初始高 = `interact_size.y`）后行与行正常排列。
//! 用例以「三条问题行都渲染且都可见」钉住这一条。
//!
//! 定位手段：真帧收集 `Shape::Text`，按 `#` 段（大纲）/ 规则名（检查）
//! 锚定行，断言同带内三段 top 差 ≤ 0.5pt；另断言长标题仍被截断在行号
//! 左侧（宽度约束没有随对齐修复失效）。

use std::sync::Arc;

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::ReplaceRangeCommand;
use rustmark::presentation::state::app_ui_state::SidebarTab;
use rustmark::presentation::views::main_view;
use rustmark::shared::geometry::{ByteOffset, TextRange};

// ============================================================================
// 装置
// ============================================================================

fn app_with(content: &str) -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = app.ensure_document().expect("应能开出空白文档");
    app.services
        .documents
        .replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
            text: content.to_string(),
        })
        .expect("写入内容应成功");
    app.refresh_text();
    (dir, app)
}

/// 侧栏内容面板内的全部文本矩形（活动栏以右、`sidebar_width` 以左）。
///
/// 两帧：首帧是 egui 的测量 pass，几何与稳定帧有 1~2pt 差（见
/// [`crate::support::settle`] 的说明），取第二帧的画面。
fn sidebar_texts(app: &mut EditorApp, tab: SidebarTab) -> Vec<(String, egui::Rect)> {
    app.ui.sidebar_tab = tab;
    app.ui.sidebar_visible = true;
    let ctx = crate::support::ctx_with_app_fonts();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1000.0, 700.0),
        )),
        ..Default::default()
    };
    let sidebar_w = app.theme().metrics.sidebar_width;
    let mut out = None;
    for _ in 0..2 {
        let full = crate::support::run_ui(&ctx, input.clone(), |ui| {
            rustmark::presentation::theme::mapper::apply(ui.ctx(), &app.ui.theme);
            main_view::show(app, ui);
        });
        out = Some(full);
    }
    let mut texts = Vec::new();
    fn walk(shapes: &[egui::epaint::ClippedShape], out: &mut Vec<(String, egui::Rect)>) {
        fn one(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(t) => {
                    out.push((
                        t.galley.text().to_string(),
                        t.galley.rect.translate(t.pos.to_vec2()),
                    ));
                }
                egui::epaint::Shape::Vec(v) => {
                    for s in v {
                        one(s, out);
                    }
                }
                _ => {}
            }
        }
        for clipped in shapes {
            one(&clipped.shape, out);
        }
    }
    walk(&out.expect("应有输出").shapes, &mut texts);
    // 活动栏 48pt（`sidebar_view::ACTIVITY_BAR_W`，私有故此处复述数值）。
    // 下界取「屏高 − 状态栏高」：状态栏文本（x 与侧栏重叠）不属于侧栏
    // 面板，混进来会把行锚（列表区最左文本）带到状态栏上去。
    let screen_h = 700.0;
    let panel_bottom = screen_h - app.theme().metrics.statusbar_height;
    texts.retain(|(_, r)| r.min.x >= 48.0 && r.max.x <= sidebar_w && r.min.y < panel_bottom);
    texts.sort_by(|a, b| (a.1.min.y.total_cmp(&b.1.min.y)).then(a.1.min.x.total_cmp(&b.1.min.x)));
    texts
}

/// 同一水平带（top 差 ≤ 0.5pt）内的文本。
fn band_around<'a>(
    texts: &'a [(String, egui::Rect)],
    anchor: &egui::Rect,
) -> Vec<&'a (String, egui::Rect)> {
    texts
        .iter()
        .filter(|(_, r)| (r.min.y - anchor.min.y).abs() <= 0.5)
        .collect()
}

/// 带内文本 top 的极差（同一水平带内应 ≤ 0.5pt）。
fn spread(band: &[&(String, egui::Rect)]) -> f32 {
    let tops: Vec<f32> = band.iter().map(|(_, r)| r.min.y).collect();
    tops.iter().cloned().fold(f32::MIN, f32::max) - tops.iter().cloned().fold(f32::MAX, f32::min)
}

/// 同一**行邻域**（top 差 ≤ 半行高）内的文本——比 [`band_around`] 宽松，
/// 用于「只验宽度约束、不关心是否对齐」的场景（对齐由
/// [`outline_row_segments_share_one_baseline`] 专门钉住）。
fn near_row<'a>(
    texts: &'a [(String, egui::Rect)],
    anchor: &egui::Rect,
) -> Vec<&'a (String, egui::Rect)> {
    texts
        .iter()
        .filter(|(_, r)| (r.min.y - anchor.min.y).abs() <= 12.0)
        .collect()
}

/// 大纲文档：一条短标题 + 一条会被截断的超长标题。
const OUTLINE_DOC: &str = "# 标题甲\n\n正文一段。\n\n## 这是一个特别特别特别特别特别特别特别特别特别特别特别特别特别特别特别特别长的标题肯定会被截断\n";

/// 一份含多类问题的文档（与 `lint_tab_test` 同源，稳定产出 3 条问题）。
const BAD_DOC: &str = "# 一级标题\n\n### 跳到三级 \n\n- 甲\n* 乙\n\n```rust\nfn main() {}\n```\n";

// ============================================================================
// 用例
// ============================================================================

/// 大纲行内 `#`、标题、行号三段必须同线（用户截图缺陷的正面回归）。
#[test]
fn outline_row_segments_share_one_baseline() {
    let (_dir, mut app) = app_with(OUTLINE_DOC);
    let texts = sidebar_texts(&mut app, SidebarTab::Outline);

    // 以 `#` 段为行锚：大纲行的行首段就是纯 `#` 串（标题文字本身不以
    // 纯 # 串出现，故锚不会误命中标题）。
    let hashes: Vec<_> = texts
        .iter()
        .filter(|(t, _)| !t.is_empty() && t.chars().all(|c| c == '#'))
        .collect();
    assert_eq!(hashes.len(), 2, "应渲染出两条大纲行，实际 {:?}", hashes);

    for (_, hash) in &hashes {
        let band = band_around(&texts, hash);
        assert!(
            band.len() >= 3,
            "大纲行的三段（# / 标题 / 行号）应落在同一水平带，实际带内只有 \
             {:?}（top={:.2}）——某段整体下沉了",
            band.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
            hash.min.y,
        );
        assert!(
            spread(&band) <= 0.5,
            "同带内文本 top 应一致（≤0.5pt），实际离散 {:.2}pt：{:?}",
            spread(&band),
            band.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
        );
        // 左右次序：`#` < 标题 < 行号（防止「对齐了但顺序乱了」）。
        let title = band
            .iter()
            .find(|(t, r)| !t.chars().all(|c| c == '#') && r.min.x > hash.max.x)
            .expect("带内应有标题段");
        let line_no = band
            .iter()
            .max_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x))
            .expect("带内应有行号段");
        assert!(
            title.1.max.x <= line_no.1.min.x + 0.5,
            "标题右缘({:.1})不应越过行号左缘({:.1})：截断宽度约束必须仍在",
            title.1.max.x,
            line_no.1.min.x,
        );
    }
}

/// 超长标题仍被截断在侧栏内（对齐修复不能弄丢宽度约束，问题 024 的
/// 「长文本撑出侧栏」缺陷类不许回归）。
#[test]
fn long_outline_title_stays_truncated_inside_sidebar() {
    let (_dir, mut app) = app_with(OUTLINE_DOC);
    let sidebar_w = app.theme().metrics.sidebar_width;
    let texts = sidebar_texts(&mut app, SidebarTab::Outline);
    for (text, rect) in &texts {
        assert!(
            rect.max.x <= sidebar_w + 0.5,
            "侧栏文本不应越界（SidePanel 是 exact_width，溢出只会被裁掉）：\
             {:?} 右缘 {:.1} > {sidebar_w}",
            text,
            rect.max.x,
        );
    }
    // 超长标题那行：标题段右缘必须收在行号左侧（截断真的发生了）。
    let long_hash = texts
        .iter()
        .find(|(t, _)| t == "##")
        .expect("应有二级标题行");
    let band = near_row(&texts, &long_hash.1);
    let title = band
        .iter()
        .find(|(t, r)| !t.chars().all(|c| c == '#') && r.min.x > long_hash.1.max.x)
        .expect("行邻域内应有标题段");
    let line_no = band
        .iter()
        .max_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x))
        .expect("行邻域内应有行号段");
    assert!(
        title.1.max.x <= line_no.1.min.x + 0.5,
        "超长标题应被截断（右缘 {:.1} ≤ 行号左缘 {:.1}）",
        title.1.max.x,
        line_no.1.min.x,
    );
}

/// 检查行（规则名 / 说明 / 行号）同线，且三行都渲染在列表里——与大纲行
/// 同款嵌套 Ui 模式、同款缺陷，一并钉住。
#[test]
fn lint_row_segments_share_one_baseline() {
    let (_dir, mut app) = app_with(BAD_DOC);
    let texts = sidebar_texts(&mut app, SidebarTab::Lint);

    // 列表区 = 计数提示（「N 个问题…」）以下的全部文本；行锚是行首段
    // （规则名），即列表区内最左的文本列。
    let hint = texts
        .iter()
        .find(|(t, _)| t.contains("个问题"))
        .expect("应有问题计数提示");
    let list: Vec<&(String, egui::Rect)> = texts
        .iter()
        .filter(|(_, r)| r.min.y > hint.1.max.y)
        .collect();
    let rule_left = list.iter().map(|(_, r)| r.min.x).fold(f32::MAX, f32::min);
    let anchors: Vec<_> = list
        .iter()
        .filter(|(t, r)| (r.min.x - rule_left).abs() <= 0.5 && !t.is_empty())
        .collect();
    assert_eq!(
        anchors.len(),
        3,
        "三条问题行都应渲染且可见（`with_layout` 不限高时只有第一行落在视口内），\
         实际 {:?}",
        anchors.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
    );

    for (_, rule_rect) in &anchors {
        let band = band_around(&texts, rule_rect);
        assert!(
            band.len() >= 3,
            "检查行的三段（规则名 / 说明 / 行号）应落在同一水平带，实际带内只有 \
             {:?}（top={:.2}）——某段整体下沉了",
            band.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
            rule_rect.min.y,
        );
        assert!(
            spread(&band) <= 0.5,
            "同带内文本 top 应一致（≤0.5pt），实际离散 {:.2}pt：{:?}",
            spread(&band),
            band.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
        );
    }
}
