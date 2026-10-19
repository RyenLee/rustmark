//! 侧栏「整行可点」容器。

use egui::{Response, Sense, Ui};

/// 排一行内容，并在**整行矩形**上挂 `Sense::click()` 命中区。
///
/// 返回 `(整行 Response, 闭包返回值)`。
///
/// # 为什么必须封装：`clicked()` 恒为 false 且不报错
///
/// `ui.horizontal` 的子 Ui 默认 sense 是 **hover**（egui 0.37
/// `UiBuilder::sense` 文档原话："Default is `Sense::hover`"）。侧栏的行由
/// 箭头、图标、文件名、行号等多段拼成，每一段都是 hover——于是：
///
/// - 拿 `ui.horizontal(...).response` 当行响应，`clicked()` **永远是 false**；
/// - 而各段自己的矩形又盖在行交互之上，把点击**吞掉**。
///
/// 症状就是「文件树点击与右键均无响应」「大纲点击不跳转」「搜索命中点不动」
/// 「检查结果点不动」——四个标签各撞过一次（问题 1）。它不报错、不 panic，
/// 只在真机上表现为「点了没反应」，单测也很难发现（要造真帧才测得到命中）。
///
/// # 正确写法只有一种顺序
///
/// 先 `ui.horizontal` 把内容排出来、取回整行矩形，再在同一矩形上用
/// `ui.interact` 挂 click-sense 命中区——命中区在绘制顺序上位于内容之上，
/// 点击稳稳落到行上。反过来（先挂命中区再排内容）会让后续内容盖住命中区，
/// 症状一模一样。
///
/// 这个顺序封装在本函数里，调用方拿到的直接就是可点的 `Response`，
/// 没有「忘了挂 interact」这个中间状态。
///
/// # 为什么是 `horizontal` 而不是 `horizontal_top`
///
/// 列表行要用 `horizontal`：它把子 Ui 初始高定为 `spacing().interact_size.y`
/// （约 18pt），行内控件在行高内居中、行与行正常堆叠。`horizontal_top` 是
/// **顶栏横条**的纪律（问题 060：顶栏被挤压到 16pt 时必须贴顶，否则文字
/// 底部越界被裁），用在列表行上会改变行高语义。
///
/// 两者的取舍不是「哪个更好」，而是「这块区域的高度由谁决定」：
/// 顶栏高度被预算钉死、内容只能贴顶塞进去；列表行高度由内容决定、必须让
/// 每行自成一体。
///
/// # 为什么也不能用 `with_layout`（问题 065）
///
/// `with_layout` **不限定子 Ui 的高度**：子 Ui 拿到的是外层剩余的**全部**
/// 可用高（滚动区里探针实测 509pt），行内控件被 Center 交叉对齐放到这块
/// 高度的垂直中点——整行浮在面板中部；子 Ui 的 `min_rect` 也被撑成 509pt，
/// 每行游标暴涨 512pt，**只有第一行落在视口内**，其余行画在面板外被裁掉；
/// `row_rect`（顶部 34pt）与文字（面板中部）相差 248pt，点文字根本不命中。
///
/// 三个候选（`with_layout` / `horizontal` / `horizontal_top`）里只有
/// `horizontal` 同时满足「行高由内容决定」与「行内控件居中对齐」，本函数
/// 把选择固定下来，调用点不再有选错的余地。
///
/// # 关于 `id_salt`
///
/// 用 `ui.scope_id().with(id_salt)` 派生命中区 id，故同一列表内每行的 salt
/// 必须互不相同（通常是行标识：路径 / 行号 / 规则名 + 文案）。重复 salt 会
/// 让多行共享同一个交互 id——表现为只有第一行可点。
pub fn clickable_row<R>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> (Response, R) {
    let inner = ui.horizontal(add_contents);
    let row_id = ui.scope_id().with(id_salt);
    let response = ui.interact(inner.response.rect, row_id, Sense::click());
    (response, inner.inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 跑两帧真实 egui：首帧让字库完成一次布局，第二帧量到的几何才稳定
    /// （egui 0.37 的首帧是测量 pass，见 `tests/support/mod.rs::settle`）。
    ///
    /// 无头环境没有渲染后端来 apply 纹理增量，`FullOutput` 必须显式清空——
    /// egui 的 `Drop` 对未应用的 delta 有 `debug_assert`。
    fn run_two_frames(mut f: impl FnMut(&mut Ui)) {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 200.0));
        for _ in 0..2 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| f(ui),
            );
            out.textures_delta.clear();
        }
    }

    /// 行内容排完后，命中区必须覆盖整行、且有非零高度。
    ///
    /// 这条钉的是「命中区挂在**内容矩形**上」而不是「挂在某个固定尺寸上」：
    /// 曾经用 `allocate_exact_size` 给行占位，结果行高与实际内容不一致，
    /// 命中区与文字错位（点文字不中）。
    ///
    /// 需要一个真实帧：`horizontal` 的子 Ui 尺寸在布局 pass 之后才确定，
    /// 纯逻辑调用拿不到 rect。
    #[test]
    fn clickable_row_hit_area_covers_the_row() {
        let mut hit = egui::Rect::NOTHING;
        let mut inner_height = 0.0;
        run_two_frames(|ui| {
            let (response, content_rect) =
                clickable_row(ui, "row", |ui| ui.add(egui::Label::new("第一段")).rect);
            hit = response.rect;
            inner_height = content_rect.height();
        });
        assert!(hit.height() > 0.0, "命中区高度应大于 0：{hit:?}");
        assert!(
            hit.height() + 0.5 >= inner_height,
            "命中区须覆盖整行内容：命中区 {hit:?} vs 内容高 {inner_height}"
        );
    }

    /// 相同 salt 的两行不能共享交互 id（否则只有一行可点）。
    ///
    /// 用「点击第二行能否命中」不好在无头环境构造，这里退一步钉住
    /// id 派生本身：不同 salt → 不同 id。它是上面那条注释的可执行版本。
    #[test]
    fn distinct_salts_produce_distinct_row_ids() {
        let mut ids = Vec::new();
        run_two_frames(|ui| {
            ids.clear();
            for salt in ["a", "b"] {
                let (response, _) = clickable_row(ui, salt, |ui| {
                    ui.add(egui::Label::new("x"));
                });
                ids.push(response.id);
            }
        });
        assert_eq!(ids.len(), 2, "两帧都应有两次取样");
        assert_ne!(ids[0], ids[1], "不同 salt 必须派生出不同的行交互 id");
    }
}
