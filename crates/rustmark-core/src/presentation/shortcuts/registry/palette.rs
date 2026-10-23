//! 动作表视图：菜单与命令面板渲染的唯一依据，含模糊检索。

use super::*;

// ============================================================================
// 动作表（菜单 / 命令面板渲染用）
// ============================================================================

/// 动作表视图：菜单与命令面板渲染的**唯一**依据。
///
/// 刻意做成借用注册表的轻量包装（而非快照）：改键后无需重建表，
/// 下一次渲染自然看到新键位——这正是「三处同步」想要的语义。
pub struct ActionTable<'a> {
    pub(super) registry: &'a ShortcutRegistry,
}

/// 命令面板 / 菜单的一行。
#[derive(Clone, Debug, PartialEq)]
pub struct ActionEntry {
    /// 动作。
    pub action: UiAction,
    /// 中文显示名。
    pub label: &'static str,
    /// 分组。
    pub(crate) group: MenuGroup,
    /// 显示用键位（可能为空串）。
    pub combo: String,
}

impl<'a> ActionTable<'a> {
    /// 某分组下的全部动作（按枚举序）。
    pub fn group(&self, group: MenuGroup) -> Vec<ActionEntry> {
        UiAction::ALL
            .iter()
            .filter(|action| action.group() == group)
            .map(|action| self.entry(*action))
            .collect()
    }

    /// 全部动作（命令面板用）。
    pub fn all(&self) -> Vec<ActionEntry> {
        UiAction::ALL.iter().map(|a| self.entry(*a)).collect()
    }

    /// 单个动作的条目。
    pub fn entry(&self, action: UiAction) -> ActionEntry {
        ActionEntry {
            action,
            label: action.label(),
            group: action.group(),
            combo: self.registry.display_for(action),
        }
    }

    /// 命令面板的模糊检索。
    ///
    /// 子序列匹配（对齐原型 `fuzzyScore`）：`bt` 能命中「加粗bold」。
    /// 检索范围含中文显示名与英文关键词，故中英文都能搜到。
    pub fn search(&self, query: &str) -> Vec<ScoredAction> {
        let mut out = Vec::new();
        for action in UiAction::ALL {
            let entry = self.entry(*action);
            // 标签命中权重高于关键词命中：搜「保存」应先出「保存」而非「保存全部」。
            //
            // 关键词命中时 `hits` 必须是**空**而非关键词内的坐标：
            // [`ScoredAction::hits`] 的语义是「在 `label` 中的字符下标」，
            // 而关键词（`bold` / `insert_link`）与中文标签（「加粗」）
            // 长度与内容都对不上。把关键词坐标塞进来，UI 按它切标签就会
            // 越界——「加粗」只有 2 个字却被告知命中下标 0..4。
            let best = fuzzy_match(action.label(), query)
                .map(|hits| (100, hits))
                .or_else(|| fuzzy_match(action.keywords(), query).map(|_| (50, Vec::new())));
            if let Some((score, hits)) = best {
                out.push(ScoredAction { entry, score, hits });
            }
        }
        // 分数降序；同分保持动作枚举序（`sort_by` 稳定），结果因此可预测。
        out.sort_by_key(|a| std::cmp::Reverse(a.score));
        out
    }
}

/// 一条检索命中。
#[derive(Clone, Debug, PartialEq)]
pub struct ScoredAction {
    /// 动作条目。
    pub entry: ActionEntry,
    /// 匹配得分（越高越靠前）。
    pub(crate) score: i32,
    /// 命中字符在 `label` 中的**字符**下标（供 UI 高亮）。
    ///
    /// **空 = 未命中 `label`**：只命中英文关键词时也是空的。
    /// 关键词里的坐标对 `label` 无效（`bold` 的 `[0..4]` 落在
    /// 「加粗」之外），UI 必须按空处理——不画高亮，而不是越界切片。
    pub hits: Vec<usize>,
}

/// 子序列模糊匹配；命中时返回**字符**索引序列。
///
/// 索引以 `char` 为单位而非字节——中文标签按字节切会把字符劈开，
/// 后续高亮就会切在字符中间。
fn fuzzy_match(text: &str, query: &str) -> Option<Vec<usize>> {
    if query.trim().is_empty() {
        return Some(Vec::new());
    }
    let haystack: Vec<char> = text.to_lowercase().chars().collect();
    let needle: Vec<char> = query.to_lowercase().chars().collect();
    let mut cursor = 0usize;
    let mut hits = Vec::with_capacity(needle.len());
    for ch in needle {
        let found = haystack.iter().skip(cursor).position(|c| *c == ch)?;
        hits.push(cursor + found);
        cursor = hits.last().copied().unwrap_or(0) + 1;
    }
    Some(hits)
}
