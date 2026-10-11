//! 回归：预览区四周不得出现「黑边」——面板之间与面板边距条必须有填充覆盖。
//!
//! # 缺陷机理（2026-10-11 用户实拍取证）
//!
//! 用户在真实窗口看到：预览区左侧一条约 14px 的纯黑竖条（纵贯内容区）、
//! 顶部一条 2px 黑线。无头取证发现这两处**没有任何绘制指令**——不是画了
//! 黑色，而是**谁都不画**，真实 GPU 渲染下露出交换链清屏色（黑）：
//!
//! 1. 分割条 `SidePanel`（4pt 宽）此前用 egui 默认面板 frame
//!    （`Frame::side_top_panel`，自带 `Margin::symmetric(8, 2)`）。4pt 装不下
//!    16pt 边距，egui 把 frame 的响应矩形撑到约 16pt 宽，`set_cursor` 取
//!    `rect.max.x` 排布下一面板，把预览 CentralPanel 推后约 12pt——这 12pt
//!    落在分割条与预览之间，无面板认领。分割条自己的 `paint_divider` 也因
//!    `ui.max_rect()` 被边距挤成负宽而退化成零宽矩形。
//! 2. 预览宿主 `CentralPanel` 的 frame 边距取 `Margin::symmetric(0, 2)`，
//!    但 `Frame::new()` 无填充：预览自己的 `rect_filled` 只画内容区，
//!    上下两条 2pt 边距条裸露清屏色。
//!
//! 之所以长期没被无头测试抓住：无头断言看的是「画了什么形状」，而本缺陷
//! 是「**该画的地方没画**」——只有把形状的**涂色覆盖范围**（rect ∩ clip）
//! 拼起来检查是否留洞才能发现。本用例正是这么做的。
//!
//! 回归触发史：`3a90371` 把预览宿主从 `CentralPanel::default()`（自带
//! `panel_fill` 填充，恰好盖住了这些洞）换成 `Frame::new()`（无填充）后
//! 暴露。

use crate::support::fake_dialog::FakeDialog;
use rustmark::app::bootstrap::{ConfigPaths, Services};
use rustmark::app::frame::EditorApp;
use rustmark::presentation::theme::builtin::Theme;
use std::sync::Arc;

use eframe::App as _;

/// 验收窗口（与用户实拍一致：ppi=1，1908×1191）。
fn screen_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1908.0, 1191.0))
}

/// 收集一帧里所有「实心填充」的**实际涂色区域**（形状矩形 ∣ 裁剪矩形）。
///
/// 只取不透明填充：半透明色叠在清屏色（黑）上仍然是黑的，不能算「盖住」。
fn opaque_painted_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::Rect> {
    fn walk(shape: &egui::epaint::Shape, clip: egui::Rect, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::epaint::Shape::Rect(r) => {
                let opaque = match r.fill {
                    egui::Color32::TRANSPARENT => false,
                    c => c.a() >= 250,
                };
                if opaque {
                    let painted = r.rect.intersect(clip);
                    if painted.is_positive() {
                        out.push(painted);
                    }
                }
            }
            egui::epaint::Shape::Vec(inner) => {
                for s in inner {
                    walk(s, clip, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        walk(&clipped.shape, clipped.clip_rect, &mut out);
    }
    out
}

/// 给定一行 y，返回该行上 [x0, x1] 内被实心填充覆盖的 x 区间（合并后）。
fn covered_runs_at(rects: &[egui::Rect], y: f32, x0: f32, x1: f32) -> Vec<(f32, f32)> {
    let mut runs: Vec<(f32, f32)> = rects
        .iter()
        .filter(|r| r.y_range().contains(y))
        .map(|r| (r.left().max(x0), r.right().min(x1)))
        .filter(|(a, b)| b > a)
        .collect();
    runs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f32, f32)> = Vec::new();
    for (a, b) in runs {
        match merged.last_mut() {
            Some(last) if a <= last.1 + 0.5 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

fn assert_no_gap(runs: &[(f32, f32)], x0: f32, x1: f32, what: &str) {
    // 覆盖必须从 x0 连到 x1（允许 0.5pt 的抗锯齿余量）。
    let mut cursor = x0;
    for (a, b) in runs {
        if *a > cursor + 0.5 {
            panic!(
                "{what}: [{cursor:.1}..{a:.1}] 无人绘制（清屏色会露出黑边）；\
                 实际覆盖段 {runs:?}"
            );
        }
        cursor = cursor.max(*b);
    }
    assert!(
        cursor + 0.5 >= x1,
        "{what}: [{cursor:.1}..{x1:.1}] 无人绘制；实际覆盖段 {runs:?}"
    );
}

#[test]
fn preview_region_has_no_unpainted_black_border() {
    let dir = tempfile::tempdir().expect("应可创建临时目录");
    let services = Services::assemble_with_dialogs(
        ConfigPaths::in_dir(dir.path()),
        Arc::new(FakeDialog::new()),
    )
    .expect("装配应成功");
    let mut app = EditorApp::new(services, false);
    let file = dir.path().join("note.md");
    std::fs::write(&file, "# 标题\n\n```mermaid\ngraph LR\nA-->B\n```\n").expect("写文件应成功");
    app.open_path(&file.to_string_lossy()).expect("打开应成功");
    app.ui.sidebar_visible = true;
    // 用户现场：深色偏好启动 → 状态栏切浅色（切换不得残留空隙）。
    app.ui.set_theme(Theme::dark());
    app.ui.set_theme(Theme::light());

    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |_| {}); // 装载字库
    let mut frame = eframe::Frame::_new_kittest();
    let screen = screen_rect();
    let mut out_full = None;
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            app.update(ctx, &mut frame);
        });
        out_full = Some(out);
    }
    let rects = opaque_painted_rects(&out_full.expect("至少跑了一帧").shapes);

    // ① 内容区中段一行（y=600）：从侧栏右缘到窗口右缘必须被连续填充——
    //    分割条与预览之间的空隙（黑竖条）会在这里断开。
    let runs_mid = covered_runs_at(&rects, 600.0, 0.0, screen.right());
    assert_no_gap(&runs_mid, 0.0, screen.right(), "y=600 行");

    // ② 预览面板顶边距条：frame 无填充时这里裸露清屏色。egui 内容区顶 =
    //    topbar(40) + tabstrip(32) + tabbar(28)，边距条 2pt 从这里开始
    //    （预览自己的底色 rect_filled 只画内容区，从 +2pt 起）。逐行断言。
    let m = app.theme().metrics;
    let content_top = m.topbar_height + m.tabstrip_height + m.tabbar_height;
    for y in [
        content_top + 0.5,
        content_top + 1.5,
        content_top + 2.5,
        content_top - 2.5,
        content_top - 0.5,
    ] {
        let runs_top = covered_runs_at(&rects, y, 1200.0, screen.right());
        assert_no_gap(
            &runs_top,
            1200.0,
            screen.right(),
            &format!("y={y} 行（预览顶部）"),
        );
    }
}
