//! 拼写检查右键菜单：错词命中、建议组装与浮层绘制（V11-07）。

use egui::Context;

use crate::app::frame::EditorApp;
use crate::application::markdown::queries::{SpellCheckQuery, SpellSuggestQuery};
use crate::presentation::state::editor_ui_state::SpellSuggestMenu;
use crate::presentation::theme::builtin::Theme;
use crate::presentation::views::editor_ops;
use crate::shared::text::word_bounds_at;

/// 拼写建议菜单一次最多列几条（V11-07）。
///
/// 超过 5 条时用户基本只会看前两三条，再长反而把「替换」这个动作
/// 变成一次阅读任务；且菜单越高越容易盖住正文。
const SPELL_SUGGEST_LIMIT: usize = 5;

/// 右键落在**错词**上时组装建议菜单；否则返回 `None`（关闭菜单）。
///
/// 公开是为了让无头用例直接驱动它：坐标 → 字节的换算是这条链路上最容易
/// 静默错位的一环（编译不报、运行不崩，只是菜单弹在别的词上），
/// 没有自动化断言就只能靠肉眼看。
///
/// # 为什么必须自己把屏幕坐标换算成字节偏移
///
/// egui 的 `TextEdit` 只告诉「指针在哪个像素」，不给「哪个字符」。
/// 换算靠 `Galley::cursor_from_pos`：它把坐标换成本帧排版里的**字符**下标，
/// 再经 `editor_ops::char_to_byte` 变成字节偏移——与编辑区其余部分
/// （光标同步、查找高亮）同一个坐标系。
///
/// 用本帧的 `galley` 而不是「按行号推算」，是因为自动换行时一个逻辑行
/// 折成多行，行号推算必然错位——那正是行号栏踩过的坑。
pub fn spell_menu_for(
    app: &mut EditorApp,
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    pointer: egui::Pos2,
) -> Option<SpellSuggestMenu> {
    let relative = pointer - galley_pos;
    let char_index = galley.cursor_from_pos(relative).index;
    let byte = editor_ops::char_to_byte(&app.editor_text, char_index.0);
    let (start, end) = word_bounds_at(&app.editor_text, byte)?;
    // 只给**已标红**的词提建议：没有被判错的词弹出替换菜单，
    // 会让用户以为程序认为他写错了。
    //
    // 判错结果在这里现取而不是复用帧首算好的那份：右键一帧才发生一次，
    // 为此多调一次服务的代价可以忽略，换来的是本函数**自给自足**——
    // 无头测试可以直接驱动它，不必先手工拼出标记集。
    let flagged = app
        .active_doc
        .map(|doc_id| {
            app.services
                .markdown
                .spell_check(SpellCheckQuery {
                    doc_id,
                    enabled: app.editor.spell_check,
                })
                .into_iter()
                .any(|(s, e)| s == start && e == end)
        })
        .unwrap_or(false);
    if !flagged {
        return None;
    }
    let word = app.editor_text.get(start..end)?.to_string();
    let doc_id = app.active_doc?;
    let suggestions = app.services.markdown.spell_suggestions(SpellSuggestQuery {
        doc_id,
        offset: start,
        limit: SPELL_SUGGEST_LIMIT,
    });
    // 没有候选就不弹：一个只有标题的空菜单只会让人以为坏了。
    if suggestions.is_empty() {
        return None;
    }
    Some(SpellSuggestMenu::new(
        start,
        end,
        word,
        suggestions,
        pointer,
    ))
}

/// 描画拼写建议菜单（V11-07）。
///
/// # 为什么用 `Area` 而不是 `Window` / `Popup`
///
/// 菜单位置锚在**右键的那个像素**上，`Window` 是浮动窗口（带标题栏、
/// 可拖动、会被 `Esc` 当浮层关掉），语义不对；`Area` 恰恰是「钉在屏幕某点
/// 的即时模式浮层」，且不需要进 `Overlay` 栈——它不是对话框，
/// 不该抢「关闭浮层」那条快捷键。
pub(super) fn draw_spell_menu(app: &mut EditorApp, ctx: &Context, theme: &Theme) {
    let Some(menu) = app.editor.spell_menu.clone() else {
        return;
    };
    // 陈旧检测：菜单里存的是**上一帧**的字节区间。用户可能在菜单开着的
    // 时候改了文本，此时按旧区间替换会改到别处去——宁可关掉重来。
    let stale = app.editor_text.get(menu.byte_start..menu.byte_end) != Some(menu.word.as_str());
    let escaped = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if stale || escaped {
        app.editor.spell_menu = None;
        return;
    }

    let mut picked: Option<String> = None;
    egui::Area::new(egui::Id::unique("rustmark.spell.menu"))
        .fixed_pos(menu.screen_pos())
        .order(egui::Order::Foreground)
        .movable(false)
        .interactable(true)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme.bg_tertiary)
                .stroke(egui::Stroke::new(1.0_f32, theme.border))
                .corner_radius(theme.metrics.radius_card)
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    ui.set_min_width(170.0);
                    ui.weak(format!("你可能想输入：{}", menu.word));
                    ui.separator();
                    for suggestion in &menu.suggestions {
                        if ui.button(suggestion.as_str()).clicked() {
                            picked = Some(suggestion.clone());
                        }
                    }
                });
        });

    if let Some(replacement) = picked {
        let (start, end) = (menu.byte_start, menu.byte_end);
        let _ = app.replace_spelling(start, end, &replacement);
        app.editor.spell_menu = None;
    }
}
