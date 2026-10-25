//! 拼写错词波浪线：区间几何（纯函数）与绘制。

/// 编辑区查找命中高亮区间（**文档字节**坐标）。
///
/// 由表现层在绘制编辑区前从 `rustmark_ui::presentation::state::find_replace_ui_state::FindReplaceUiState`
/// （V11-62 批次 2 起住在 `rustmark-ui`；core 不反向依赖 ui，文字指路不成链接）
/// 的命中列表换算而来，传给 [`crate::presentation::theme::mapper::highlight_layout_with_find`]，由其落到逐行 `Galley`
/// 的背景上。`current` 标记「当前命中」，使用更醒目的背景色
/// （[`Theme::find_current_bg`](crate::presentation::theme::builtin::Theme::find_current_bg)），其余命中用 [`Theme::find_bg`](crate::presentation::theme::builtin::Theme::find_bg)——
/// 与原型 `tk-find` / `tk-find-current` 的观感一致。
#[derive(Clone, Copy, Debug)]
pub struct FindMark {
    /// 命中起点（字节，含）。
    pub start: usize,
    /// 命中终点（字节，不含）。
    pub end: usize,
    /// 是否为当前命中（更醒目的背景）。
    pub current: bool,
}

/// 单个波浪线片段的几何（**纯函数产物**，供无头测试断言）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Squiggle {
    /// 片段左上角。
    pub rect: egui::Rect,
    /// 锯齿相位。相邻片段必须错开相位，否则连成一条实线而不是波浪线。
    pub phase: f32,
}

/// 拼写错词标记（字节区间 + 原词）。
///
/// 与 [`FindMark`] 同构：都只带区间不带内容，供排版层每帧比对。
#[derive(Clone, Copy, Debug)]
pub struct SpellMark {
    /// 错词起点（字节，含）。
    pub start: usize,
    /// 错词终点（字节，不含）。
    pub end: usize,
}

/// 波浪线的振幅与波长（像素）。
///
/// 取值参考主流编辑器的观感：振幅 1.5px 在 14–16px 字号下清晰可辨又不至于
/// 抢过正文；波长 4px 约等于「一个字符宽的三分之一」，密集但不糊成实线。
const SQUIGGLE_AMPLITUDE: f32 = 1.5;
const SQUIGGLE_WAVELENGTH: f32 = 4.0;

/// 由错词区间生成波浪线片段几何（**纯函数**）。
///
/// # 为什么是纯函数
///
/// 波浪线是「错词区间 × 排版结果」的乘积，天然只能在有 `Galley` 的地方算。
/// 但「区间 → 锯齿片段序列」这段逻辑本身与主题、字体无关，抽出来才能在
/// 无头环境断言：跨行错词是否被拆成两段、零宽/越界区间是否被丢弃、
/// 相邻片段的相位是否错开——这些恰恰是肉眼很难发现的缺陷。
///
/// `galley` 须由与 `text` 同源的排版结果而来，否则字节↔像素映射会错位。
pub fn squiggles_for(
    galley: &egui::Galley,
    text: &str,
    marks: &[SpellMark],
    origin: egui::Vec2,
) -> Vec<Squiggle> {
    let mut out = Vec::new();
    for (i, m) in marks.iter().enumerate() {
        // 越界区间直接跳过：调用方（应用层）已按文档内容给界，
        // 但纯函数不该假设输入一定合法——`panic="abort"` 下越界切片会直接崩进程。
        if m.start >= m.end || m.end > text.len() || !text.is_char_boundary(m.start) {
            continue;
        }
        // `CCursor` 用**字符索引**而非字节偏移，故先换算。
        // 区间两端已验证落在字符边界上，`chars().count()` 的 O(n) 代价可接受——
        // 错词数量通常个位数。
        let start_char = text[..m.start].chars().count();
        let end_char = text[..m.end].chars().count();
        let start_rect = galley.pos_from_cursor(egui::text::CCursor::new(start_char));
        let end_rect = galley.pos_from_cursor(egui::text::CCursor::new(end_char));

        for (seg, rect) in row_segments(galley, start_rect, end_rect)
            .into_iter()
            .enumerate()
        {
            let rect = rect.translate(origin);
            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                continue;
            }
            out.push(Squiggle {
                rect,
                // 跨行片段之间也要错开相位：按片段序号推相位，
                // 否则第二行的波浪会与第一行垂直对齐，看着像下划线。
                phase: (i + seg) as f32 * SQUIGGLE_WAVELENGTH,
            });
        }
    }
    out
}

/// 把错词区间拆成「每行一段」的矩形列表（处理软换行导致的跨行）。
///
/// # 为什么需要拆
///
/// `pos_from_cursor` 只给出**光标所在那一行**的矩形。一个错词若跨了软换行，
/// 直接拿首尾矩形会得到一个横跨两行的巨大矩形——波浪线会横穿整段正文。
///
/// 本版本 egui 的 [`egui::text::Row`] 不暴露字符区间，故按**行的几何**切分：
/// 用首尾光标矩形的纵向位置定位起止行，中间行取整行宽度。
fn row_segments(
    galley: &egui::Galley,
    start_rect: egui::Rect,
    end_rect: egui::Rect,
) -> Vec<egui::Rect> {
    // 定位起止行在 `rows` 中的下标（按纵向位置匹配）。
    //
    // # 为什么必须借`rows` 而不能直接用光标矩形
    //
    // `pos_from_cursor` 返回的是**零宽的光标矩形**（`[[x, top] - [x, bottom]]`，
    // 宽恒为 0）。直接用它俩拼`from_min_max(start.left_center(), end.right_center())`
    // 得到的矩形**高度也是 0**，会被后续的零面积过滤整条丢掉——
    // 这正是「一个片段都没产出」的原因。波浪线需要的是**行的纵向范围**，
    // 故必须回到 `galley.rows` 取真实行高。
    let row_top = |y: f32| -> Option<usize> {
        galley
            .rows
            .iter()
            .position(|r| y >= r.pos.y - 0.5 && y < r.pos.y + r.row.size.y + 0.5)
    };
    let single = |sr: egui::Rect, er: egui::Rect| {
        vec![egui::Rect::from_min_max(
            egui::pos2(sr.left(), sr.top()),
            egui::pos2(er.left(), er.bottom()),
        )]
    };

    let Some(first) = row_top(start_rect.top()) else {
        return single(start_rect, end_rect);
    };
    let Some(last) = row_top(end_rect.top()) else {
        return single(start_rect, end_rect);
    };

    let mut out = Vec::new();
    for idx in first..=last.min(galley.rows.len().saturating_sub(1)) {
        let placed = &galley.rows[idx];
        let row_rect = placed.rect();
        let x0 = if idx == first {
            start_rect.left_center().x
        } else {
            row_rect.left()
        };
        let x1 = if idx == last {
            end_rect.right_center().x
        } else {
            row_rect.right()
        };
        let seg = egui::Rect::from_min_max(
            egui::pos2(x0, row_rect.top()),
            egui::pos2(x1, row_rect.bottom()),
        );
        // 退化段（零宽 / 越界）跳过，避免画出无意义的点。
        if seg.width() > 0.0 {
            out.push(seg);
        }
    }
    // 兜底：行定位失败时退回单段——宁可位置不够准，也不要整条错词没有波浪线。
    // 注意纵向范围仍取自光标矩形的 top/bottom（光标矩形是零宽但**高度非零**，
    // 正是行高；只有横向是退化的）。
    if out.is_empty() {
        out.push(egui::Rect::from_min_max(
            egui::pos2(start_rect.left(), start_rect.top()),
            egui::pos2(end_rect.left(), end_rect.bottom()),
        ));
    }
    out
}

/// 绘制锯齿波浪线（给定片段几何）。
///
/// 用 `Shape::closed_line` 逐段连出锯齿而非 `hline`：实线波浪线在深色主题下
/// 视觉噪声远低于锯齿线，这也是原生编辑器的通行做法。
pub fn paint_squiggles(painter: &egui::Painter, squiggles: &[Squiggle], color: egui::Color32) {
    let stroke = egui::Stroke::new(1.0_f32, color);
    for sq in squiggles {
        let rect = sq.rect;
        // 贴在文字基线下方一点，避免与字形下缘重叠。
        let y = rect.bottom() - 1.0;
        let segments = ((rect.width() / SQUIGGLE_WAVELENGTH).ceil() as usize).max(1);
        let step = rect.width() / segments as f32;
        let mut pts: Vec<egui::Pos2> = Vec::with_capacity(segments + 1);
        for k in 0..=segments {
            let x = rect.left() + step * k as f32;
            // 用全局 x 参与相位计算，保证相邻片段的波形连续（不会在边界处错相）。
            let phase_x = x + sq.phase;
            let t = (phase_x / SQUIGGLE_WAVELENGTH) * std::f32::consts::TAU;
            pts.push(egui::pos2(x, y + t.sin() * SQUIGGLE_AMPLITUDE));
        }
        painter.add(egui::Shape::line(pts, stroke));
    }
}
