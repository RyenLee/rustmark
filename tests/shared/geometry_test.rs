//! `shared::geometry` 单元测试（S2-05）。

use rustmark::shared::geometry::{ByteOffset, LineCol, TextRange};

// ---------- ByteOffset ----------

#[test]
fn byte_offset_add_and_sub() {
    let o = ByteOffset::new(10);
    assert_eq!((o + 5).value(), 15);
    assert_eq!((o - 4).value(), 6);
}

#[test]
fn byte_offset_sub_saturates_at_zero() {
    let o = ByteOffset::new(3);
    assert_eq!((o - 10).value(), 0, "减法越界应饱和到 0 而非 panic");
    assert_eq!(o - ByteOffset::new(9), 0);
}

#[test]
fn byte_offset_difference_is_length() {
    let a = ByteOffset::new(4);
    let b = ByteOffset::new(10);
    assert_eq!(b - a, 6);
}

// ---------- TextRange ----------

#[test]
fn reversed_range_is_normalized() {
    let r = TextRange::new(ByteOffset::new(9), ByteOffset::new(3));
    assert_eq!(r.start().value(), 3);
    assert_eq!(r.end().value(), 9);
    assert_eq!(r.len(), 6);
}

#[test]
fn empty_range_has_zero_len() {
    let r = TextRange::empty(ByteOffset::new(7));
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
}

#[test]
fn contains_is_half_open() {
    let r = TextRange::new(ByteOffset::new(2), ByteOffset::new(6));
    assert!(r.contains(ByteOffset::new(2)), "起点应包含");
    assert!(r.contains(ByteOffset::new(5)));
    assert!(!r.contains(ByteOffset::new(6)), "终点应排除");
    assert!(!r.contains(ByteOffset::new(1)));
}

#[test]
fn intersects_truth_table() {
    let base = TextRange::new(ByteOffset::new(4), ByteOffset::new(8));

    let overlapping = TextRange::new(ByteOffset::new(6), ByteOffset::new(10));
    let containing = TextRange::new(ByteOffset::new(0), ByteOffset::new(100));
    let contained = TextRange::new(ByteOffset::new(5), ByteOffset::new(6));
    let adjacent_before = TextRange::new(ByteOffset::new(0), ByteOffset::new(4));
    let adjacent_after = TextRange::new(ByteOffset::new(8), ByteOffset::new(12));
    let disjoint = TextRange::new(ByteOffset::new(20), ByteOffset::new(30));

    assert!(base.intersects(overlapping));
    assert!(base.intersects(containing));
    assert!(base.intersects(contained));
    assert!(!base.intersects(adjacent_before), "端点相接不算相交");
    assert!(!base.intersects(adjacent_after), "端点相接不算相交");
    assert!(!base.intersects(disjoint));
}

#[test]
fn clamp_trims_to_limit() {
    let r = TextRange::new(ByteOffset::new(0), ByteOffset::new(100));
    let limit = TextRange::new(ByteOffset::new(10), ByteOffset::new(20));
    let clamped = r.clamp(limit);
    assert_eq!(clamped.start().value(), 10);
    assert_eq!(clamped.end().value(), 20);
}

#[test]
fn clamp_without_intersection_collapses_to_empty() {
    let r = TextRange::new(ByteOffset::new(50), ByteOffset::new(60));
    let limit = TextRange::new(ByteOffset::new(0), ByteOffset::new(10));
    let clamped = r.clamp(limit);
    assert!(clamped.is_empty());
    assert_eq!(clamped.start().value(), 0);
}

#[test]
fn as_usize_range_matches_bounds() {
    let r = TextRange::new(ByteOffset::new(3), ByteOffset::new(7));
    assert_eq!(r.as_usize_range(), 3..7);
}

// ---------- LineCol ----------

#[test]
fn line_col_is_zero_based_internally_and_one_based_for_display() {
    let lc = LineCol::new(0, 0);
    assert_eq!((lc.line(), lc.col()), (0, 0));
    assert_eq!((lc.display_line(), lc.display_col()), (1, 1));

    let lc = LineCol::new(4, 11);
    assert_eq!((lc.display_line(), lc.display_col()), (5, 12));
}
