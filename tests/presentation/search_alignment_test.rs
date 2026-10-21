//! 搜索分组头的**垂直对齐**回归（与大纲/检查同款的「0 高子矩形」反模式，
//! 见 `widgets/text.rs::elided_segment` 长注释与问题 065）。
//!
//! # 钉住的不变量
//!
//! 分组头 = 左侧文件名（限宽截断）+ 右侧命中数。两者必须落在同一条水平线。
//! 旧写法 `allocate_ui_with_layout(vec2(avail, 0.0), …)` 把文件名子矩形压到
//! 行垂直中点之下（约半个行高），命中数（右侧 `weak`）留在正确高度，于是
//! 「这一行的两段不在一条线上」。修法：文件名改用 `elided_segment`，高度取
//! 右侧命中数段的实测高度（`search.rs::group_header`）。
//!
//! # 检查手段
//!
//! 真帧收集侧栏 `Shape::Text`，按「带内最左文本非 needle（即文件名而非片段）
//! 且最右文本为纯整数（即命中数而非行号）」识别分组头，断言带内 top 离散 ≤ 0.5pt。

use std::sync::Arc;

use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::application::document::commands::ReplaceRangeCommand;
use rustmark::presentation::state::app_ui_state::SidebarTab;
use rustmark::presentation::state::sidebar_ui_state::SearchScope;
use rustmark::presentation::views::main_view;
use rustmark::shared::geometry::{ByteOffset, TextRange};

/// 开一个含 `needle` 的文档（未存盘草稿即可，搜索 `OpenDocuments` 范围只读内存）。
fn app_with_needle() -> (tempfile::TempDir, EditorApp) {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(crate::support::fake_dialog::FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let id = app.ensure_document().expect("应能开出空白文档");
    app.services
        .documents
        .replace_range(ReplaceRangeCommand {
            id,
            range: TextRange::new(ByteOffset::new(0), ByteOffset::new(0)),
            text: "alpha needle beta\nsecond needle line\ngamma delta".to_string(),
        })
        .expect("写入内容应成功");
    app.refresh_text();
    app.sidebar.search_query = "needle".to_string();
    app.sidebar.search_scope = SearchScope::OpenDocuments;
    app.ui.sidebar_tab = SidebarTab::Search;
    app.ui.sidebar_visible = true;
    (dir, app)
}

/// 侧栏面板内的全部文本矩形（活动栏以右、sidebar_width 以左）。
fn sidebar_texts(app: &mut EditorApp) -> Vec<(String, egui::Rect)> {
    let ctx = egui::Context::default();
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
    let screen_h = 700.0;
    let panel_bottom = screen_h - app.theme().metrics.statusbar_height;
    texts.retain(|(_, r)| r.min.x >= 48.0 && r.max.x <= sidebar_w && r.min.y < panel_bottom);
    texts.sort_by(|a, b| (a.1.min.y.total_cmp(&b.1.min.y)).then(a.1.min.x.total_cmp(&b.1.min.x)));
    texts
}

/// 同一水平带（top 差 ≤ 0.5pt）内的全部文本。
fn band_around<'a>(
    texts: &'a [(String, egui::Rect)],
    anchor: &egui::Rect,
) -> Vec<&'a (String, egui::Rect)> {
    texts
        .iter()
        .filter(|(_, r)| (r.min.y - anchor.min.y).abs() <= 0.5)
        .collect()
}

/// 分组头：带内最左文本不含 `needle`（是文件名而非命中片段），最右文本为
/// 纯整数（是命中数而非行号）。这种带只可能是分组头。
fn group_headers(texts: &[(String, egui::Rect)]) -> Vec<Vec<(String, egui::Rect)>> {
    let mut headers = Vec::new();
    let mut i = 0;
    while i < texts.len() {
        let band = band_around(texts, &texts[i].1);
        let left = &band[0].0;
        let right = &band[band.len() - 1].0;
        let right_is_count = right.parse::<usize>().is_ok();
        if !left.contains("needle") && right_is_count && band.len() >= 2 {
            headers.push(band.iter().map(|(t, r)| (t.clone(), *r)).collect());
            // 跳到下一带
            let y = texts[i].1.min.y;
            while i < texts.len() && (texts[i].1.min.y - y).abs() <= 0.5 {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    headers
}

#[test]
fn search_group_header_filename_and_count_share_one_baseline() {
    let (_dir, mut app) = app_with_needle();
    let texts = sidebar_texts(&mut app);
    let headers = group_headers(&texts);
    if headers.is_empty() {
        eprintln!(
            "DEBUG texts(y,xL,xR): {:?}",
            texts
                .iter()
                .map(|(t, r)| (t.as_str(), r.min.y as i32, r.min.x as i32, r.max.x as i32))
                .collect::<Vec<_>>()
        );
    }
    assert!(
        !headers.is_empty(),
        "搜索面板应渲染出至少一个分组头（文件名 + 命中数），实际文本：{:?}",
        texts.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>()
    );
    for header in &headers {
        let tops: Vec<f32> = header.iter().map(|(_, r)| r.min.y).collect();
        let spread = tops.iter().cloned().fold(f32::MIN, f32::max)
            - tops.iter().cloned().fold(f32::MAX, f32::min);
        assert!(
            spread <= 0.5,
            "分组头文件名与命中数应同线（≤0.5pt），实际离散 {spread:.2}pt：{:?}",
            header.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>()
        );
    }
}
