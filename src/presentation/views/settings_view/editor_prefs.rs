//! 「编辑器」与「语法检查」分区：缩进、折行、行号、自动保存与 lint 开关。

use egui::{RichText, Ui};

use crate::presentation::theme::builtin::Theme;
use crate::presentation::view_context::SettingsViewAccess;
use rustmark_core::application::dto::EditorSettingsView;

use super::*;

/// 「编辑器」分区：缩进 / 自动换行 / 自动保存间隔。
pub(super) fn editor(vctx: &mut dyn SettingsViewAccess, ui: &mut Ui, theme: &Theme) {
    let view = vctx.settings().editor;
    field(ui, theme, "Tab 缩进", |ui| {
        ui.horizontal(|ui| {
            for size in TAB_SIZES {
                let selected = view.tab_size == *size;
                if ui
                    .selectable_label(selected, format!("{size} 空格"))
                    .clicked()
                    && !selected
                {
                    let size = *size;
                    vctx.apply_editor_field(&mut |e: &mut EditorSettingsView| {
                        e.tab_size = size;
                    });
                }
            }
        });
    });
    field(ui, theme, "自动换行", |ui| {
        // 读**会话**值：它是编辑区实际生效的那份。启动时由领域播种、改动经
        // `set_word_wrap` 双写（会话 + 领域），故与设置页其它项显示一致。
        let mut on = vctx.editor_state().word_wrap;
        if ui.checkbox(&mut on, "按窗口宽度折行").changed() {
            vctx.set_word_wrap(on);
        }
    });
    // 行号栏：与自动换行同一手法——`set_line_numbers` 让会话立即生效，
    // `apply_editor_field` 负责落盘；持久化值在启动时播种进会话状态
    // （`EditorApp::new`），故复选框读会话值即可与设置页其它项一致。
    field(ui, theme, "行号栏", |ui| {
        let mut on = vctx.editor_state().line_numbers;
        if ui.checkbox(&mut on, "在编辑区左侧显示行号").changed() {
            vctx.set_line_numbers(on);
            vctx.apply_editor_field(&mut |e: &mut EditorSettingsView| {
                e.show_line_numbers = on;
            });
        }
    });
    // 远程图片（PRIV-01）：与行号栏同一手法读**会话**值——预览区逐图读它
    // 决定是否外联，读 `settings()` 会每图克隆一次设置视图。写入走
    // `set_load_remote_images`（会话 + 领域一起写），故不必再调
    // `apply_editor_field`。
    field(ui, theme, "远程图片", |ui| {
        let mut on = vctx.editor_state().load_remote_images;
        if ui.checkbox(&mut on, "加载文档中的远程图片").changed() {
            vctx.set_load_remote_images(on);
        }
    });
    ui.colored_label(
        theme.text_secondary,
        "关闭后预览区不再向图片主机发起任何请求（离线 / 隐私场景）；\
         文档里的远程图显示为占位，本地图片与 data URI 不受影响。",
    );
    field(ui, theme, "自动保存", |ui| {
        let mut interval = view.auto_save_interval.unwrap_or(0);
        if ui
            .add(egui::Slider::new(&mut interval, 0..=60).suffix(" 秒"))
            .changed()
        {
            let value = (interval > 0).then_some(interval);
            vctx.apply_editor_field(&mut |e: &mut EditorSettingsView| {
                e.auto_save_interval = value;
            });
        }
    });
    if view.auto_save_interval.is_none() {
        ui.colored_label(theme.text_secondary, "自动保存已关闭。");
    }
    // 打开文档数量上限（问题 038）：标签条限额，达上限后拒绝**新的**打开。
    field(ui, theme, "打开文档上限", |ui| {
        let mut limit = view.max_open_documents;
        if ui
            .add(egui::Slider::new(&mut limit, MAX_OPEN_DOCUMENTS_RANGE).suffix(" 个"))
            .changed()
        {
            vctx.apply_editor_field(&mut |e: &mut EditorSettingsView| {
                e.max_open_documents = limit;
            });
        }
    });
    ui.colored_label(
        theme.text_secondary,
        "达到上限后，新的打开请求会被拒绝并提示；已打开的文档与切换不受影响。",
    );
    // 写作目标（R-05）：此前设置页没有入口，只能沿用当前值。
    // 即时落盘（与设置页其它项一致）；空串与 0 都归一为「未设」——
    // 领域的 `WritingGoal::is_set` 以 0 表示未设，存 0 即语义正确，
    // 不必额外搬一份「未设」标记（表现层也不得依赖 `domain` 去读那个常量）。
    field(ui, theme, "写作目标（字数）", |ui| {
        let current = vctx.settings().editor.writing_goal;
        // D-3 阶段 3：clone → 就地编辑 → 变化才值写回。局部缓冲不持
        // `app` 的借用，写回后 `apply_editor_field` 可直接接管。
        let mut writing_goal_text = vctx.settings_ui_state().writing_goal_text.clone();
        let changed = ui.text_edit_singleline(&mut writing_goal_text).changed();
        if changed {
            let next = crate::presentation::state::settings_ui_state::normalize_writing_goal(
                &writing_goal_text,
                current,
            );
            vctx.settings_ui_state()
                .set_writing_goal_text(writing_goal_text);
            if next != current {
                vctx.apply_editor_field(&mut |e: &mut EditorSettingsView| {
                    e.writing_goal = next;
                });
            }
        }
    });
    ui.colored_label(
        theme.text_secondary,
        "达到目标后，底部状态栏显示完成进度；留空表示不设写作目标。",
    );
}

/// 「语法检查」分区。
pub(super) fn spellcheck(vctx: &mut dyn SettingsViewAccess, ui: &mut Ui, theme: &Theme) {
    // 读会话值（启动时由领域播种、改动经 `set_spell_check` 双写），
    // 与状态栏显示及编辑区实际生效值一致。
    let mut on = vctx.editor_state().spell_check;
    if ui.checkbox(&mut on, "在编辑区标记拼写错误").changed() {
        vctx.set_spell_check(on);
    }
    ui.colored_label(
        theme.text_secondary,
        "语法检查状态同时显示在底部状态栏，可点击切换。",
    );
    // 右键菜单是不可见的交互，不写出来的话大多数人永远不会发现
    // 「波浪线下面还能点出替换建议」。
    ui.colored_label(
        theme.text_secondary,
        "在带波浪线的词上点右键，可看到「你可能想输入」的建议并一键替换。",
    );
    ui.add_space(theme.metrics.space_sm);
    lint_rules(vctx, ui, theme);
}

/// 格式检查（lint）规则开关（V11-08）。
///
/// # 为什么与拼写检查同处「语法检查」标签
///
/// 两者对用户是**同一件事**：「帮我看正文有没有毛病」。分到两个标签会让
/// 用户来回找；而侧栏的「检查」标签正是这两类结果共同的去处。
fn lint_rules(vctx: &mut dyn SettingsViewAccess, ui: &mut Ui, theme: &Theme) {
    ui.label(RichText::new("格式检查（Markdown）").color(theme.text_primary));
    let lint = vctx.settings().editor.lint;
    let toggles: [(&str, bool); 4] = [
        ("标题层级跳跃（如 H1 直接到 H3）", lint.heading_jump),
        ("列表符号混用（- / * / +）", lint.list_marker_mix),
        ("行尾多余空格", lint.trailing_whitespace),
        ("未闭合的代码围栏 ```", lint.unclosed_fence),
    ];
    let mut updated = lint;
    for (i, (label, value)) in toggles.iter().enumerate() {
        let mut on = *value;
        if ui.checkbox(&mut on, *label).changed() {
            match i {
                0 => updated.heading_jump = on,
                1 => updated.list_marker_mix = on,
                2 => updated.trailing_whitespace = on,
                _ => updated.unclosed_fence = on,
            }
        }
    }
    if updated != lint {
        vctx.apply_editor_field(&mut |e: &mut EditorSettingsView| {
            e.lint = updated;
        });
    }
    ui.colored_label(
        theme.text_secondary,
        "检查结果显示在侧栏的「检查」标签里，点击问题可跳转到对应行。",
    );
}
