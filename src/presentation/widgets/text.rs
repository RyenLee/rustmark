//! 限宽文本：截断与左对齐的两条固定配方。
//!
//! 两条配方共同解决同一类缺陷：**固定宽容器里的文本溢出**。侧栏是
//! `exact_width` 面板、顶栏与欢迎页是预算固定的横条——溢出不会显示滚动条，
//! 而是把面板的 response 矩形撑宽，`SidePanel` 据此把编辑区整体推右，
//! 侧栏右缘之外露出交换链清屏色（用户看到的是「一条竖向黑框」）。
//! 详见问题 024 / 033 / 059。

use egui::{Align, Label, Rect, Response, Ui, WidgetText};

/// 在「剩余宽度 − `reserve`」的范围内排一段可截断文本，**高度对齐到 `measured`**。
///
/// `measured` 传行内**已排控件**的矩形（通常是行首标签的 `rect`）：
/// 它既是高度来源，也表达「本段与它同线」的意图。
///
/// # 为什么高度参数是 `Rect` 而不是 `f32`（问题 065：传 0 就错位）
///
/// 为了限宽而用 `allocate_ui_with_layout(vec2(avail, 0.0), …)` 是直觉写法，
/// 也是本缺陷反复出现的原因——它看起来完全合理：反正高度会被外层决定。
/// 但 egui 的实际行为是：
///
/// 1. 0 高的 `desired_size` 会被外层行的 **Center 交叉对齐填满成行高**
///    （`Layout::next_frame_ignore_wrap`）；
/// 2. 于是 0 高子矩形落在**行垂直中点**；
/// 3. 其中的文字再按 egui「横向布局不向上越界」规则（`frame_rect.top() <
///    cursor.top()` 时整体下推）被压到中点之下。
///
/// 结果：中段比行首段低半个行高（探针实测 8.5pt），右端行号也被带偏 3.5pt
/// ——用户看到的是「这一行是斜的」。传**行高**后子矩形与行重合，各段自然同线。
///
/// 把参数类型定成 `Rect` 而不是 `f32`，是为了让「传 0」在**类型层面**说不
/// 出口：你没法凭空构造一个「0 高」的正当理由，只能从已排控件那里取真实矩形。
/// 这是本封装相对「照抄注释」的全部价值。
///
/// # 为什么截断必须发生在**一段**文本上（问题 033）
///
/// 「前段 / 命中段 / 后段」各自 `truncate()` 同样会溢出：水平布局里最后一
/// 段的可用宽可能只剩几个点、比一个字形还窄，而 epaint 的截断实现保证
/// 「行不为空」——装不下任何字形时它仍会排出一个字形（或省略号），该段
/// galley 因此超过可用宽，整行冲出面板。
///
/// 所以 `text` 应传**一个** `WidgetText`（通常是拼好的 `LayoutJob`，用不同
/// 底色的 section 表达高亮），只截断一次。本函数不提供「多段分别截断」的
/// 入口，正是为了让这条约束没有别的写法可选。
pub fn elided_segment(
    ui: &mut Ui,
    measured: Rect,
    reserve: f32,
    text: impl Into<WidgetText>,
) -> Response {
    // 预留宽大于剩余宽时钳到 0：宁可这一格什么都不显示，也不能给出负宽度
    // （负 width 会让子 Ui 的 max_rect 反向，后续控件坐标全乱）。
    let avail = (ui.available_width() - reserve).max(0.0);
    ui.allocate_ui_with_layout(
        egui::vec2(avail, measured.height()),
        egui::Layout::left_to_right(Align::Center),
        |ui| ui.add(Label::new(text).truncate()),
    )
    .inner
}

/// 在给定矩形里放一段**左对齐**的可截断文本。
///
/// # 为什么不能写 `ui.put(rect, Label::new(t).truncate().halign(LEFT))`
///
/// 这是本项目 061 记录的写法，但它**在 egui 0.37 下不生效**——实测
/// （本文件 `put_elided_left_aligns_short_text` 与一次临时探针）：
///
/// | 写法 | 240pt 矩形里的字形左缘 |
/// |---|---|
/// | `put` + `truncate` + `halign(LEFT)` | 121.84（居中） |
/// | `put` + `truncate` | 123.00（居中） |
/// | `show` 内 `left_to_right` 布局 | **10.00（贴左）** |
///
/// 原因是 `Ui::put` 的子 Ui 布局是 `centered_and_justified(TopDown)`：Label 先
/// `allocate_exact_size(galley.size())` 拿到一个**已被父布局居中**的矩形，
/// `halign(LEFT)` 只决定字形相对**这个矩形**的落点（`rect.left_top()`），
/// 对矩形自己被摆在哪没有任何影响。而 `put` 的 `justify` 并不会把 galley
/// 撑满可用宽（`truncate()` 的 `max_rows = 1` 下实测 galley 宽 = 文字宽），
/// 于是「居中」原样保留。
///
/// 所以正确做法是换掉布局本身：`scope_builder` + `left_to_right`，子 Ui 的
/// 光标从矩形左缘起排，Label 自然贴左。061 当年在 egui 0.33 上看到的修复
/// 效果，很可能是长文本被 elide 恰好撑满全宽造成的“看起来对”；一旦换成
/// 短文本（未命名文档 / 短文件名）就会暴露。本函数用 `left_to_right` 而不
/// 依赖 `halign`，与字体、文本长度、elide 与否都无关。
///
/// # 副作用与 `put` 相同
///
/// `scope_builder` 与 `put` 一样会在父 Ui 里分配这块矩形（父光标推进到
/// 矩形右缘），所以 061 的第二个副作用仍在：横向循环里用定位矩形排多个
/// 槽位时，槽位 x 必须自己 `allocate_rect` 显式推进，不能读父光标。
///
/// # 悬浮提示
///
/// `Label::show_tooltip_when_elided` **默认即为 `true`**（egui 0.37
/// `Label::default`），被省略时自动显示全文，本函数不改这一默认——面包屑
/// 路径、标签标题、最近打开与快速打开的文件名都需要它。若某处已有更完整的
/// 行级 tooltip、不希望叠加，调用方直接 `ui.put` 并显式
/// `.show_tooltip_when_elided(false)`，不要用本函数。
///
/// # 返回
///
/// 控件 `Response`，供调用方按需链 `.on_hover_text(…)` 或读 `clicked()`。
pub fn put_elided(ui: &mut Ui, rect: Rect, text: impl Into<WidgetText>) -> Response {
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(Align::Center)),
        |ui| ui.add(Label::new(text).truncate()),
    )
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 跑两帧真实 egui（首帧是测量 pass，几何 1~2pt 不稳），返回第二帧里
    /// 实际下发的文本图元「字形矩形 + 文本内容」。
    ///
    /// 纹理增量必须显式清空，理由见 `widgets::row` 的同名工具。
    ///
    /// # 为什么必须收 `Shape::Text` 而不是读 `Response::rect`
    ///
    /// `put` 返回的 `Response::rect` 是**分配矩形**（整个 `rect` 参数），
    /// 与文字实际落在哪里无关。用探针验证过：去掉 `halign(LEFT)` 后
    /// `response.rect.left()` 一动不动，测试照样通过——这样的断言没有牙齿。
    /// 字形矩形来自 `TextShape::visual_bounding_rect()`，才是「文字画在哪」。
    fn run_two_frames(mut f: impl FnMut(&mut Ui)) -> Vec<(Rect, String)> {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
        let mut texts = Vec::new();
        for _ in 0..2 {
            texts.clear();
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| f(ui),
            );
            for clipped in &out.shapes {
                walk(&clipped.shape, &mut texts);
            }
            out.textures_delta.clear();
        }
        texts
    }

    fn walk(shape: &egui::epaint::Shape, out: &mut Vec<(Rect, String)>) {
        match shape {
            egui::epaint::Shape::Text(t) => {
                out.push((t.visual_bounding_rect(), t.galley.text().to_string()));
            }
            egui::epaint::Shape::Vec(v) => {
                for s in v {
                    walk(s, out);
                }
            }
            _ => {}
        }
    }

    /// 短文本必须**贴左**，不能浮在矩形中间（问题 061）。
    ///
    /// 判定用「字形左缘 − 矩形左缘」：居中布局在 240pt 宽的矩形里会偏移
    /// 约 (240 − 文字宽) / 2 ≈ 110pt，与「贴左」的 0 差了一个数量级，
    /// 不存在阈值敏感性。
    ///
    /// # 这条用例抓出过一个真回归
    ///
    /// 第一版实现照抄 061 记录的 `put + halign(LEFT)`，描边走测试时用
    /// `Response::rect` 断言——**探针（去掉 halign）照样通过**，因为
    /// `Response::rect` 是分配矩形，与文字画在哪无关。改用真帧字形矩形后
    /// 立刻读到 126.75（居中），才查出 `halign` 在 `put` 下是空操作。
    /// 于是实现改为 `left_to_right` 布局，本用例成为它的回归锚点。详见
    /// [`put_elided`] 文档里的三种写法对照表。
    #[test]
    fn put_elided_left_aligns_short_text() {
        let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(240.0, 20.0));
        let texts = run_two_frames(|ui| {
            put_elided(ui, rect, "短");
        });
        let (glyph, _) = texts
            .iter()
            .find(|(_, text)| text == "短")
            .unwrap_or_else(|| panic!("帧里找不到「短」的字形：{texts:?}"));
        assert!(
            glyph.left() - rect.left() < 1.0,
            "短文本应从矩形左缘起画：矩形左缘 {}，字形左缘 {}",
            rect.left(),
            glyph.left()
        );
    }

    /// 限宽段必须与行内其他文本**共一条中心线**。
    ///
    /// 这是问题 065 的**行为**守卫。构造方式刻意复现缺陷现场：行高先由
    /// 一个 30pt 占位撑起来（文件树里是图标列，大纲/检查行里是行高本身），
    /// 之后放一段参考文本与一个限宽段——只有行高大于文本高，居中偏移才会
    /// 显现。
    ///
    /// 若 `elided_segment` 把 `desired_size` 的高度传成 0，0 高子矩形会先被
    /// 行的 Center 交叉对齐拉到行中点，其中的文字再按「横向布局不向上越界」
    /// 规则整体下推——中心线偏离约 7.5pt（与探针实测的 8.5pt 同量级），
    /// 与本用例 2pt 的阈值差一个数量级，不存在阈值敏感性。
    ///
    /// # 两个构造细节（都是 egui 布局语义逼出来的）
    ///
    /// - **比中心线而不是 top**：行内各段按 Center 交叉对齐，字号不同时
    ///   top 天然不同，比 top 会把「字号差异」误判成「错位」。
    /// - **撑高项必须排在最前**：egui 横向布局的交叉轴范围是**增量**增长的，
    ///   先排的控件按初始行高居中、后排的才看到撑高后的行高。撑高项放在
    ///   中间会让参考文本与限宽段各自按不同行高居中，测出来的是偏移而不是
    ///   对齐（实测差 5pt）。
    ///
    /// 必须包在 `clickable_row` 里跑：那是本项目里「行」的唯一构造入口
    /// （`horizontal` 布局），脱离它两段会各自占一行，测的就不是对齐了。
    ///
    /// 更完整的回归（三段同线 + 长标题仍被截断在行号左侧）在
    /// `tests/presentation/outline_alignment_test.rs`，那里跑的是真帧。
    #[test]
    fn elided_segment_aligns_with_row_leading_widget() {
        use super::super::row::clickable_row;

        let mut ref_center = f32::NAN;
        let mut seg_center = f32::NAN;
        run_two_frames(|ui| {
            clickable_row(ui, "align", |ui| {
                // 先把行高撑到 30pt（模拟文件树的图标列 / 侧栏固定行高）。
                ui.allocate_exact_size(egui::vec2(1.0, 30.0), egui::Sense::hover());
                let reference = ui.label("行首");
                let segment = elided_segment(ui, reference.rect, 10.0, "一段很长的说明文字");
                ref_center = reference.rect.center().y;
                seg_center = segment.rect.center().y;
            });
        });
        assert!(
            (seg_center - ref_center).abs() < 2.0,
            "限宽段须与行内文本共中心线：参考 {ref_center}，段 {seg_center}"
        );
    }

    /// 预留宽大于剩余宽时必须钳到 0，不能给出负宽度。
    ///
    /// 负 width 会让子 Ui 的 `max_rect` 反向，后续控件坐标全乱——这类错误
    /// 只在「窄窗口 + 长行号预留」的极端组合下出现，最容易漏测。
    #[test]
    fn elided_segment_clamps_negative_width() {
        let mut width = f32::NAN;
        run_two_frames(|ui| {
            let (lead, _) = ui.allocate_exact_size(egui::vec2(10.0, 20.0), egui::Sense::hover());
            // 预留比整行可用宽还大：`available − reserve` 必然为负。
            let reserve = ui.available_width() + 100.0;
            width = elided_segment(ui, lead, reserve, "abc").rect.width();
        });
        assert!(
            width >= 0.0 && width.is_finite(),
            "负预留必须钳到 0，实际宽 {width}"
        );
    }
}
