use super::layout::Item;
use super::*;

/// 假度量：每个字符宽 `0.5em`。用它替代真实字体度量，
/// 让排版规则的断言与字体无关、跨平台稳定。
fn fake_measure() -> impl Fn(&str, f32) -> f32 {
    |s: &str, size: f32| s.chars().count() as f32 * size * 0.5
}

fn lay(src: &str, display: bool) -> MathLayout {
    let m = fake_measure();
    layout(src, 16.0, display, &m).expect("应能排版")
}

fn err(src: &str) -> MathError {
    let m = fake_measure();
    layout(src, 16.0, true, &m).expect_err("应报错")
}

#[test]
fn plain_text_layouts_and_has_positive_size() {
    let l = lay("x", false);
    assert!(l.width > 0.0);
    assert!(l.height() > 0.0);
}

#[test]
fn longer_content_is_wider() {
    assert!(lay("x+y+z", false).width > lay("x", false).width);
}

#[test]
fn greek_letters_and_symbols_are_mapped() {
    // 未支持的命令会整体失败，故「能排出来」本身就是映射表命中的证据。
    for src in [
        "\\alpha+\\beta",
        "a\\leq b",
        "\\infty",
        "\\forall x",
        "\\nabla f",
    ] {
        let l = lay(src, false);
        assert!(l.width > 0.0, "{src} 应能排版");
        // 字形必须真的换掉了源命令，而不是把 `\alpha` 原样画出来。
        let joined: String = l
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            !joined.contains('\\'),
            "{src} 的字形里不应残留反斜杠：{joined}"
        );
    }
}

#[test]
fn fraction_is_taller_than_its_parts() {
    let line = lay("a", false);
    let frac = lay("\\frac{a}{b}", false);
    assert!(
        frac.height() > line.height() * 1.5,
        "分数应明显比单行高：{} vs {}",
        frac.height(),
        line.height()
    );
    // 分数线必须存在——它才是「这是分数而非两个字符」的全部信息。
    assert!(
        frac.items.iter().any(|i| matches!(i, Item::Rect { .. })),
        "分数应画出分数线"
    );
}

#[test]
fn fraction_has_above_and_below_content() {
    let frac = lay("\\frac{a}{b}", false);
    let above = frac
        .items
        .iter()
        .filter(|i| matches!(i, Item::Text { y, .. } if *y < 0.0))
        .count();
    let below = frac
        .items
        .iter()
        .filter(|i| matches!(i, Item::Text { y, .. } if *y > 0.0))
        .count();
    assert!(above >= 1, "分子应在基线上方");
    assert!(below >= 1, "分母应在基线下方");
}

#[test]
fn superscript_is_smaller_than_base() {
    let s = lay("x^{2}", false);
    let sizes: Vec<f32> = s
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Text { size, .. } => Some(*size),
            _ => None,
        })
        .collect();
    assert_eq!(sizes.len(), 2);
    assert!(sizes[0] > sizes[1], "上标字号应小于基准：{sizes:?}");
}

#[test]
fn subscript_sits_below_baseline() {
    let s = lay("x_{i}", false);
    assert!(
        s.items
            .iter()
            .any(|i| matches!(i, Item::Text { y, .. } if *y > 0.0)),
        "下标应在基线下"
    );
}

#[test]
fn combined_sub_and_sup_share_one_column() {
    // `x_1^2`：两个角标的 x 应**相同**（同一个 Script 节点），
    // 否则会出现「上下标一左一右」的错位。
    let s = lay("x_1^2", false);
    let xs: Vec<f32> = s
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Text { x, text, .. } if text == "1" || text == "2" => Some(*x),
            _ => None,
        })
        .collect();
    assert_eq!(xs.len(), 2, "应同时排出一上一下两个角标");
    assert!((xs[0] - xs[1]).abs() < 0.01, "上下标应共用同一列：{xs:?}");
}

#[test]
fn sqrt_draws_radical_and_sits_above_body() {
    let s = lay("\\sqrt{x+1}", false);
    assert!(
        s.items
            .iter()
            .filter(|i| matches!(i, Item::Line { .. }))
            .count()
            >= 3,
        "根号应有勾的两段折线 + 顶部横线"
    );
}

#[test]
fn sum_limits_stack_in_display_but_not_inline() {
    let display = lay("\\sum_{i=1}^{n}", true);
    let inline = lay("\\sum_{i=1}^{n}", false);
    // display：上限在基线上方较远处（叠放）；inline：贴在右侧。
    let display_width = display.width;
    let inline_width = inline.width;
    assert!(
        inline_width > 0.0 && display_width > 0.0,
        "两种模式都应能排版"
    );
    assert!(
        display_width < inline_width,
        "叠放时上下限不占横向宽度，应比右侧摆放更窄：{display_width} vs {inline_width}"
    );
}

/// `\int` 系列在 display 模式下叠放上下限（对齐 KaTeX 官方渲染）。
///
/// 曾经 `\int` 被排除在 `LIMIT_OPS` 之外，display 模式的
/// `\int_0^\infty` 把限画在积分号右侧——与官方（限在上下方）不符，
/// 用户拿预览对比官方渲染时一眼就能看出差别（问题 3）。
/// TeX 的 `\int` 在 display 模式本就等价于 `\intop`，叠放才是正解。
#[test]
fn integral_limits_stack_in_display_only() {
    let display = lay("\\int_0^\\infty t\\,dt", true);
    let inline = lay("\\int_0^\\infty t\\,dt", false);

    // 取上下限文字（`0` 与 `∞`）的基线偏移。
    let offset_of = |l: &MathLayout, want: &str| {
        l.items
            .iter()
            .filter_map(|i| match i {
                Item::Text { text, y, .. } if text == want => Some(*y),
                _ => None,
            })
            .next()
            .unwrap_or_else(|| panic!("{want} 应被排版"))
    };
    // display：下限 `0` 远在基线下方（叠在积分号下），上限 `∞` 远在上方。
    assert!(
        offset_of(&display, "0") > 16.0,
        "display 下限应叠在积分号下方：{}",
        offset_of(&display, "0")
    );
    assert!(
        offset_of(&display, "∞") < -16.0,
        "display 上限应叠在积分号上方：{}",
        offset_of(&display, "∞")
    );
    // inline：限贴在右侧，偏移不超过半字号。
    assert!(
        offset_of(&inline, "0").abs() < 8.0 && offset_of(&inline, "∞").abs() < 8.0,
        "行内模式的限应贴近基线（右侧摆放）：{:?} / {:?}",
        offset_of(&inline, "0"),
        offset_of(&inline, "∞")
    );
    // 叠放不占横向宽度：display 版面应更窄。
    assert!(
        display.width < inline.width,
        "叠放应比右侧摆放窄：{} vs {}",
        display.width,
        inline.width
    );
}

#[test]
fn left_right_delimiters_scale_with_content() {
    let small = lay("\\left(a\\right)", false);
    let big = lay("\\left(\\frac{a}{b}\\right)", false);
    let size_of = |l: &MathLayout| {
        l.items
            .iter()
            .filter_map(|i| match i {
                Item::Text { text, size, .. } if text == "(" => Some(*size),
                _ => None,
            })
            .next()
            .expect("应有左括号")
    };
    assert!(
        size_of(&big) > size_of(&small),
        "内容变高时括号应放大：{} vs {}",
        size_of(&big),
        size_of(&small)
    );
}

#[test]
fn accent_adds_marks_above() {
    let plain = lay("x", false);
    let hat = lay("\\hat{x}", false);
    assert!(hat.items.len() > plain.items.len(), "重音应额外画出符号");
    assert!(hat.height() >= plain.height());
}

#[test]
fn text_command_keeps_its_literal_content() {
    let l = lay("\\text{速度 v}", false);
    let joined: String = l
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        joined.contains("速度 v"),
        "`\\text{{}}` 应原样保留：{joined}"
    );
}

#[test]
fn matrix_rows_and_columns_align() {
    let l = lay("\\begin{pmatrix}a&b\\\\c&d\\end{pmatrix}", true);
    // 4 个格子的文字 + 两个括号。
    let texts = l
        .items
        .iter()
        .filter(|i| matches!(i, Item::Text { .. }))
        .count();
    assert_eq!(texts, 6, "应为 4 格 + 2 个括号");
}

#[test]
fn cases_environment_is_supported() {
    let l = lay("\\begin{cases}x&x>0\\\\-x&x\\leq 0\\end{cases}", true);
    assert!(l.width > 0.0);
    // cases 不画括号。
    assert!(
        !l.items
            .iter()
            .any(|i| matches!(i, Item::Text { text, .. } if text == "(" || text == "[")),
        "cases 不应画出外括号"
    );
}

#[test]
fn malformed_input_is_reported_not_panicked() {
    // `panic = "abort"` 下绝不能崩——这几类坏输入必须变成 `Err`。
    for src in ["{a", "a}", "\\frac{a}", "^2", "x & y", "\\begin{matrix}a"] {
        let _ = err(src);
    }
}

#[test]
fn unsupported_command_is_reported_not_guessed() {
    // 子集外的写法必须**明确失败**，由调用方回退到源码卡，
    // 而不是画出一个缺胳膊少腿的公式让用户以为公式写错了。
    let e = err("\\notarealcommand{x}");
    assert!(
        matches!(e, MathError::Unsupported(_)),
        "应报 Unsupported，实际 {e:?}"
    );
}

#[test]
fn empty_formula_does_not_produce_zero_width() {
    let l = lay("   ", false);
    assert!(l.width > 0.0, "空公式也不该是 0 宽（会变成隐形块）");
}

#[test]
fn spacing_commands_add_room() {
    let tight = lay("ab", false);
    let loose = lay("a\\quad b", false);
    assert!(loose.width > tight.width);
}

#[test]
fn deep_nesting_does_not_shrink_to_nothing() {
    let l = lay("x^{y^{z^{w}}}", false);
    let min_size = l
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Text { size, .. } => Some(*size),
            _ => None,
        })
        .fold(f32::INFINITY, f32::min);
    assert!(
        min_size >= 16.0 * SCRIPT_MIN_SCALE - 0.01,
        "最深层角标不应小于下限：{min_size}"
    );
}
