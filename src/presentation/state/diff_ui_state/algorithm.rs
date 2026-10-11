//! LCS 差异算法：行级对齐、增删统计、字符级差异与渲染行合并。

use super::model::{CharSeg, CharSegKind, DiffOp, DiffRow, DiffStat, DisplayRow, MAX_CELLS};

/// 两侧是否超出降级阈值。
///
/// 单独暴露而不是让 `diff_lines` 顺带返回：界面要**先**知道会降级，
/// 才能在渲染前就显示提示，而 [`diff_lines`] 只返回行。
pub fn exceeds_budget(left: &str, right: &str) -> bool {
    let n = if left.is_empty() {
        0
    } else {
        left.split('\n').count()
    };
    let m = if right.is_empty() {
        0
    } else {
        right.split('\n').count()
    };
    n * m > MAX_CELLS
}

/// 行级文本差异（基于最长公共子序列）。
///
/// 空文档对空文档返回**空**而不是一行空串：`"".split('\n')` 产出 `[""]`，
/// 不特判就会显示一行「两侧都有的空行」，用户会以为文件里真有一行。
pub fn diff_lines(a: &str, b: &str) -> Vec<DiffRow> {
    if a == b {
        // 两侧相同：逐行标 `Equal`。仍走一遍而非直接返回空——
        // 用户需要看到「未改动的行」才能确认比较确实覆盖了全文。
        if a.is_empty() {
            return Vec::new();
        }
        let mut rows = Vec::new();
        for (i, line) in a.split('\n').enumerate() {
            rows.push(DiffRow::equal(line, i + 1, i + 1));
        }
        return rows;
    }
    if a.is_empty() || b.is_empty() {
        let mut rows = Vec::new();
        if !a.is_empty() {
            for (i, line) in a.split('\n').enumerate() {
                rows.push(DiffRow::remove(line, i + 1));
            }
        }
        if !b.is_empty() {
            for (j, line) in b.split('\n').enumerate() {
                rows.push(DiffRow::add(line, j + 1));
            }
        }
        return rows;
    }

    let left: Vec<&str> = a.split('\n').collect();
    let right: Vec<&str> = b.split('\n').collect();
    let (n, m) = (left.len(), right.len());

    if n * m > MAX_CELLS {
        // 降级：整段替换。至少能看出「变了」，但看不出变了哪里——
        // 这一点必须由界面**明说**，否则用户会以为没有差异。
        let mut rows = Vec::with_capacity(n + m);
        for (i, line) in left.iter().enumerate() {
            rows.push(DiffRow::remove(line, i + 1));
        }
        for (j, line) in right.iter().enumerate() {
            rows.push(DiffRow::add(line, j + 1));
        }
        return rows;
    }

    // dp[i * width + j] = left[i..] 与 right[j..] 的 LCS 长度。
    // 展平成一维而非二维：二维 `Vec<Vec<u32>>` 有 n+1 次额外分配，
    // 而这正是要在大文档上跑的路径。
    let width = m + 1;
    let mut dp = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * width + j] = if left[i] == right[j] {
                dp[(i + 1) * width + j + 1] + 1
            } else {
                dp[(i + 1) * width + j].max(dp[i * width + j + 1])
            };
        }
    }

    let mut rows = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    let (mut old_no, mut new_no) = (1usize, 1usize);
    while i < n && j < m {
        if left[i] == right[j] {
            rows.push(DiffRow::equal(left[i], old_no, new_no));
            i += 1;
            j += 1;
            old_no += 1;
            new_no += 1;
        } else if dp[(i + 1) * width + j] >= dp[i * width + j + 1] {
            rows.push(DiffRow::remove(left[i], old_no));
            i += 1;
            old_no += 1;
        } else {
            rows.push(DiffRow::add(right[j], new_no));
            j += 1;
            new_no += 1;
        }
    }
    while i < n {
        rows.push(DiffRow::remove(left[i], old_no));
        i += 1;
        old_no += 1;
    }
    while j < m {
        rows.push(DiffRow::add(right[j], new_no));
        j += 1;
        new_no += 1;
    }
    rows
}

/// 增删行数统计（**基于行级原始结果**，不受渲染合并影响）。
pub fn diff_stat(rows: &[DiffRow]) -> DiffStat {
    rows.iter().fold(DiffStat::default(), |mut acc, row| {
        match row.op {
            DiffOp::Add => acc.added += 1,
            DiffOp::Remove => acc.removed += 1,
            DiffOp::Equal => {}
        }
        acc
    })
}

/// 字符级差异（按 `char` 而非字节）。
///
/// 按字节会在多字节字符上算错：「你」是 3 字节，删掉中间那个字节
/// 会得到两个无法解码的片段，且后续按段绘制时排版宽度也对不上。
pub fn diff_chars(a: &str, b: &str) -> (Vec<CharSeg>, Vec<CharSeg>) {
    let left: Vec<char> = a.chars().collect();
    let right: Vec<char> = b.chars().collect();
    let (n, m) = (left.len(), right.len());
    if n == 0 || m == 0 {
        let old = if n == 0 {
            Vec::new()
        } else {
            vec![CharSeg::new(CharSegKind::Remove, a)]
        };
        let new = if m == 0 {
            Vec::new()
        } else {
            vec![CharSeg::new(CharSegKind::Add, b)]
        };
        return (old, new);
    }

    // 单行字符级 diff 的规模远小于行级，但仍设上限：
    // 两个 5000 字符的单行（压缩后的 JSON）会算出 2500 万个格子。
    if n * m > MAX_CELLS {
        return (
            vec![CharSeg::new(CharSegKind::Remove, a)],
            vec![CharSeg::new(CharSegKind::Add, b)],
        );
    }

    let width = m + 1;
    let mut dp = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * width + j] = if left[i] == right[j] {
                dp[(i + 1) * width + j + 1] + 1
            } else {
                dp[(i + 1) * width + j].max(dp[i * width + j + 1])
            };
        }
    }

    let mut old_segs: Vec<CharSeg> = Vec::new();
    let mut new_segs: Vec<CharSeg> = Vec::new();
    let push = |segs: &mut Vec<CharSeg>, kind: CharSegKind, ch: char| match segs.last_mut() {
        Some(last) if last.kind == kind => last.text.push(ch),
        _ => segs.push(CharSeg {
            kind,
            text: ch.to_string(),
        }),
    };
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if left[i] == right[j] {
            push(&mut old_segs, CharSegKind::Equal, left[i]);
            push(&mut new_segs, CharSegKind::Equal, right[j]);
            i += 1;
            j += 1;
        } else if dp[(i + 1) * width + j] >= dp[i * width + j + 1] {
            push(&mut old_segs, CharSegKind::Remove, left[i]);
            i += 1;
        } else {
            push(&mut new_segs, CharSegKind::Add, right[j]);
            j += 1;
        }
    }
    while i < n {
        push(&mut old_segs, CharSegKind::Remove, left[i]);
        i += 1;
    }
    while j < m {
        push(&mut new_segs, CharSegKind::Add, right[j]);
        j += 1;
    }
    (old_segs, new_segs)
}

/// 把行级结果整理成渲染用的行：紧邻的删除 + 新增合并成「替换」。
///
/// # 为什么只在**紧邻**时合并
///
/// 行级 LCS 的输出天然把一段删除与一段新增排在一起，但它们**不一定**
/// 对应同一处修改。跨过相等行去配对会把两处无关的改动说成一处，
/// 用户会照着「这一行从 A 改成了 B」去找，实际上 A 变成了别的东西。
///
/// 行数不等时按位置配对，多出来的一侧退化为纯删除 / 纯新增——
/// 宁可少合并，也不要错合并。
pub fn to_display_rows(rows: &[DiffRow]) -> Vec<DisplayRow> {
    let mut out: Vec<DisplayRow> = Vec::with_capacity(rows.len());
    let mut i = 0usize;
    while i < rows.len() {
        if rows[i].op != DiffOp::Remove {
            out.push(match rows[i].op {
                DiffOp::Equal => DisplayRow::Equal {
                    text: rows[i].text.clone(),
                    old_no: rows[i].old_no.unwrap_or_default(),
                    new_no: rows[i].new_no.unwrap_or_default(),
                },
                _ => DisplayRow::Add {
                    text: rows[i].text.clone(),
                    new_no: rows[i].new_no.unwrap_or_default(),
                },
            });
            i += 1;
            continue;
        }
        // 从这里收集连续的删除，再看紧随其后连续的**新增**。
        let remove_start = i;
        while i < rows.len() && rows[i].op == DiffOp::Remove {
            i += 1;
        }
        let removes = &rows[remove_start..i];
        let add_start = i;
        while i < rows.len() && rows[i].op == DiffOp::Add {
            i += 1;
        }
        let adds = &rows[add_start..i];
        // 没有对应的新增：纯删除。
        if adds.is_empty() {
            for row in removes {
                out.push(DisplayRow::Remove {
                    text: row.text.clone(),
                    old_no: row.old_no.unwrap_or_default(),
                });
            }
            continue;
        }
        // 两侧行数不等时，尾部多出来的一侧按纯增 / 纯删出。
        // `get(..)` 而非 `[..]`：当删除侧比新增侧**少**时
        // `adds[removes.len()..]` 的起点越界，直接切会 panic
        // （`panic = "abort"` 下等于终止进程）。
        for row in removes.get(adds.len()..).unwrap_or_default() {
            out.push(DisplayRow::Remove {
                text: row.text.clone(),
                old_no: row.old_no.unwrap_or_default(),
            });
        }
        for row in adds.get(removes.len()..).unwrap_or_default() {
            out.push(DisplayRow::Add {
                text: row.text.clone(),
                new_no: row.new_no.unwrap_or_default(),
            });
        }
        for (old, new) in removes.iter().zip(adds.iter()) {
            let (old_segs, new_segs) = diff_chars(&old.text, &new.text);
            out.push(DisplayRow::Replace {
                old_text: old.text.clone(),
                new_text: new.text.clone(),
                old_no: old.old_no.unwrap_or_default(),
                new_no: new.new_no.unwrap_or_default(),
                old_segs,
                new_segs,
            });
        }
    }
    out
}
