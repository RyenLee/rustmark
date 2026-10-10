//! 符号 / Emoji 面板状态（S6-11f，对齐原型 A-26 `SymbolPicker.vue`）。
//!
//! # 静态表照搬原型，但**刻意不加**原型里的一个分组
//!
//! 原型的 `SNIPPETS` 里有 `{{< highlight rust >}}`——那是 **Hugo 短代码**，
//! 本项目导出的是 HTML / Markdown / PDF，产物里不会出现任何 Hugo 语法，
//! 插进去只会得到一段永远不会被渲染的文本。
//! **插入一个本应用永远不消费的东西，比不提供更糟**：用户看不出它无效。
//!
//! # 搜索用子序列匹配
//!
//! 与快速打开同一套理由（见 `quick_open_ui_state`），且这里更需要：
//! 符号的中文名常有 3–4 个字（「实心星」「无序列表」），
//! 子串匹配下用户得记得词序。

use crate::presentation::state::quick_open_ui_state::fuzzy_match;

/// 符号分组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymbolGroup {
    /// 常用表情。
    Emoji,
    /// 标点与特殊符号。
    Symbol,
    /// Markdown 片段。
    Snippet,
}

impl SymbolGroup {
    /// 全部分组（按显示顺序）。
    pub const ALL: &'static [SymbolGroup] = &[Self::Emoji, Self::Symbol, Self::Snippet];

    /// 分组显示名。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Emoji => "表情",
            Self::Symbol => "符号",
            Self::Snippet => "片段",
        }
    }
}

/// 一个可插入项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymbolItem {
    /// 插入到光标处的文本。
    pub text: &'static str,
    /// 面板中展示的字形。
    pub glyph: &'static str,
    /// 中文名 / 别名（搜索用，空格分隔多个关键词）。
    pub label: &'static str,
}

/// 常用表情（写作中高频，覆盖情绪 / 状态 / 记号）。
const EMOJI: &[SymbolItem] = &[
    SymbolItem {
        text: "✅",
        glyph: "✅",
        label: "对勾 完成 检查",
    },
    SymbolItem {
        text: "❌",
        glyph: "❌",
        label: "叉 错误 失败",
    },
    SymbolItem {
        // 不带 VS16（U+FE0F）：变体选择符本身无字形，带上它整串会渲染异常。
        text: "⚠",
        glyph: "⚠",
        label: "警告 注意",
    },
    SymbolItem {
        text: "💡",
        glyph: "💡",
        label: "想法 灵感 提示",
    },
    SymbolItem {
        text: "🔥",
        glyph: "🔥",
        label: "火 热门 重点",
    },
    SymbolItem {
        text: "⭐",
        glyph: "⭐",
        label: "星 收藏 推荐",
    },
    SymbolItem {
        text: "📌",
        glyph: "📌",
        label: "图钉 标记 置顶",
    },
    SymbolItem {
        text: "📝",
        glyph: "📝",
        label: "笔记 待办 编辑",
    },
    SymbolItem {
        text: "📚",
        glyph: "📚",
        label: "书 资料 参考",
    },
    SymbolItem {
        text: "🔖",
        glyph: "🔖",
        label: "书签 标签",
    },
    SymbolItem {
        text: "🎯",
        glyph: "🎯",
        label: "目标 靶心",
    },
    SymbolItem {
        text: "🚀",
        glyph: "🚀",
        label: "火箭 发布 上线",
    },
    SymbolItem {
        text: "💬",
        glyph: "💬",
        label: "对话 评论",
    },
    SymbolItem {
        // 同「警告」：去掉 VS16。
        text: "❤",
        glyph: "❤",
        label: "心 喜欢 红心",
    },
    SymbolItem {
        text: "👍",
        glyph: "👍",
        label: "赞 好",
    },
    SymbolItem {
        // 🤔(U+1F914) 不在字体覆盖内；💭(U+1F4AD) 有字形且表意一致。
        text: "💭",
        glyph: "💭",
        label: "思考 疑问",
    },
    SymbolItem {
        text: "😀",
        glyph: "😀",
        label: "笑 开心",
    },
    SymbolItem {
        text: "🎉",
        glyph: "🎉",
        label: "庆祝 撒花",
    },
    SymbolItem {
        text: "🔧",
        glyph: "🔧",
        label: "工具 修复 配置",
    },
    SymbolItem {
        text: "🔍",
        glyph: "🔍",
        label: "搜索 放大镜",
    },
    SymbolItem {
        text: "🐛",
        glyph: "🐛",
        label: "虫子 bug",
    },
    SymbolItem {
        text: "☕",
        glyph: "☕",
        label: "咖啡 休息",
    },
];

/// 标点 / 特殊符号（排版与公式常用）。
const SYMBOLS: &[SymbolItem] = &[
    SymbolItem {
        text: "—",
        glyph: "—",
        label: "长破折号 em dash",
    },
    SymbolItem {
        text: "–",
        glyph: "–",
        label: "短破折号 en dash",
    },
    SymbolItem {
        text: "…",
        glyph: "…",
        label: "省略号 ellipsis",
    },
    SymbolItem {
        text: "“",
        glyph: "“",
        label: "左双引号",
    },
    SymbolItem {
        text: "”",
        glyph: "”",
        label: "右双引号",
    },
    SymbolItem {
        text: "‘",
        glyph: "‘",
        label: "左单引号",
    },
    SymbolItem {
        text: "’",
        glyph: "’",
        label: "右单引号",
    },
    SymbolItem {
        text: "§",
        glyph: "§",
        label: "章节符 section",
    },
    SymbolItem {
        text: "¶",
        glyph: "¶",
        label: "段落符 paragraph",
    },
    SymbolItem {
        text: "•",
        glyph: "•",
        label: "项目符号 bullet",
    },
    SymbolItem {
        text: "†",
        glyph: "†",
        label: "脚注符 dagger",
    },
    SymbolItem {
        text: "×",
        glyph: "×",
        label: "乘号 multiply",
    },
    SymbolItem {
        text: "÷",
        glyph: "÷",
        label: "除号 divide",
    },
    SymbolItem {
        text: "±",
        glyph: "±",
        label: "正负 plus minus",
    },
    SymbolItem {
        text: "≤",
        glyph: "≤",
        label: "小于等于",
    },
    SymbolItem {
        text: "≥",
        glyph: "≥",
        label: "大于等于",
    },
    SymbolItem {
        text: "≠",
        glyph: "≠",
        label: "不等于",
    },
    SymbolItem {
        text: "≈",
        glyph: "≈",
        label: "约等于",
    },
    SymbolItem {
        text: "∞",
        glyph: "∞",
        label: "无穷 infinity",
    },
    SymbolItem {
        text: "→",
        glyph: "→",
        label: "右箭头 arrow",
    },
    SymbolItem {
        text: "←",
        glyph: "←",
        label: "左箭头",
    },
    SymbolItem {
        text: "↑",
        glyph: "↑",
        label: "上箭头",
    },
    SymbolItem {
        text: "↓",
        glyph: "↓",
        label: "下箭头",
    },
    SymbolItem {
        // ⇒ / ⇔ / ⇐（U+21D0–21D4 区）在本项目字体下均无字形；
        // ↔(U+2194) 有字形，与已有的 → ← ↑ ↓ 组成完整四向箭头。
        text: "↔",
        glyph: "↔",
        label: "双向箭头 arrow",
    },
    SymbolItem {
        text: "©",
        glyph: "©",
        label: "版权 copyright",
    },
    SymbolItem {
        text: "®",
        glyph: "®",
        label: "注册商标 registered",
    },
    SymbolItem {
        text: "™",
        glyph: "™",
        label: "商标 trademark",
    },
    SymbolItem {
        text: "°",
        glyph: "°",
        label: "度 degree",
    },
    SymbolItem {
        text: "€",
        glyph: "€",
        label: "欧元 euro",
    },
    SymbolItem {
        text: "¥",
        glyph: "¥",
        label: "人民币 日元 yen",
    },
    SymbolItem {
        text: "★",
        glyph: "★",
        label: "实心星 star",
    },
    SymbolItem {
        text: "☆",
        glyph: "☆",
        label: "空心星",
    },
    SymbolItem {
        // 用 √ / × 而非 ✓ / ✗：后者（U+2713 / U+2717）无字形覆盖，
        // 插入文档后与面板里都会显示为方框。
        text: "√",
        glyph: "√",
        label: "对勾",
    },
    SymbolItem {
        text: "×",
        glyph: "×",
        label: "叉",
    },
    SymbolItem {
        text: "♥",
        glyph: "♥",
        label: "心 heart",
    },
    SymbolItem {
        text: "◆",
        glyph: "◆",
        label: "实心菱形",
    },
    SymbolItem {
        text: "◇",
        glyph: "◇",
        label: "空心菱形",
    },
];

/// Markdown 片段（一键插入结构）。
///
/// 刻意不含原型的 `{{< highlight rust >}}`：那是 Hugo 短代码，
/// 本项目的导出器不消费它，插进去就是一段永不渲染的文本。
const SNIPPETS: &[SymbolItem] = &[
    SymbolItem {
        text: "# 标题",
        glyph: "H",
        label: "标题 heading h1",
    },
    SymbolItem {
        text: "> 引用",
        // ❝(U+275D) 无字形；改用弯引号，表意一致。
        glyph: "“",
        label: "引用 quote blockquote",
    },
    SymbolItem {
        text: "- 列表项",
        glyph: "•",
        label: "无序列表 bullet list",
    },
    SymbolItem {
        text: "1. 列表项",
        glyph: "1.",
        label: "有序列表 ordered list",
    },
    SymbolItem {
        text: "- [ ] 待办",
        glyph: "☐",
        label: "任务列表 task todo",
    },
    SymbolItem {
        text: "- [x] 已完成",
        glyph: "☑",
        label: "已办任务 done",
    },
    SymbolItem {
        text: "**粗体**",
        glyph: "B",
        label: "粗体 bold",
    },
    SymbolItem {
        text: "*斜体*",
        glyph: "I",
        label: "斜体 italic",
    },
    SymbolItem {
        text: "`代码`",
        glyph: "</>",
        label: "行内代码 code",
    },
    SymbolItem {
        text: "```\n代码块\n```",
        // ▤(U+25A4) 无字形；≡(U+2261) 的三横线正好示意多行代码。
        glyph: "≡",
        label: "代码块 codeblock fenced",
    },
    SymbolItem {
        text: "[链接](https://)",
        glyph: "🔗",
        label: "链接 link",
    },
    SymbolItem {
        text: "![图片](image.png)",
        glyph: "🖼",
        label: "图片 image",
    },
    SymbolItem {
        text: "| 列 A | 列 B |\n| --- | --- |\n| 内容 | 内容 |",
        // ▦(U+25A6) 无字形；▣(U+25A3) 的方格示意表格。
        glyph: "▣",
        label: "表格 table",
    },
    SymbolItem {
        text: "---",
        glyph: "─",
        label: "分隔线 hr divider",
    },
    SymbolItem {
        text: "> [!NOTE]\n注意事项",
        // ⓘ(U+24D8) 无字形；ℹ(U+2139) 同为 info 语义且有字形。
        glyph: "ℹ",
        label: "提示块 note callout",
    },
    SymbolItem {
        text: "[^1]: 脚注定义",
        glyph: "†",
        label: "脚注 footnote",
    },
    SymbolItem {
        text: "<br>",
        // ⏎(U+23CE) / ↵(U+21B5) 均无字形；↩(U+21A9) 的回车箭头含义相同。
        glyph: "↩",
        label: "换行 line break",
    },
];

/// 某分组的全部条目。
pub const fn items_of(group: SymbolGroup) -> &'static [SymbolItem] {
    match group {
        SymbolGroup::Emoji => EMOJI,
        SymbolGroup::Symbol => SYMBOLS,
        SymbolGroup::Snippet => SNIPPETS,
    }
}

/// 符号面板状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolUiState {
    /// 当前分组。
    pub group: SymbolGroup,
    /// 搜索词。
    pub query: String,
    /// 选中项在该分组**可见列表**中的下标。
    pub selected: usize,
    /// 打开时的搜索词（判据：搜索是否改过）。
    opened_query: Option<String>,
}

impl Default for SymbolUiState {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolUiState {
    /// 构造默认状态（第一个分组、无搜索）。
    pub fn new() -> Self {
        Self {
            group: SymbolGroup::Emoji,
            query: String::new(),
            selected: 0,
            opened_query: Some(String::new()),
        }
    }

    /// 打开面板：重置到第一个分组、清搜索、夹紧选中项。
    pub fn open(&mut self) {
        self.group = SymbolGroup::Emoji;
        self.query.clear();
        self.selected = 0;
        self.opened_query = Some(String::new());
    }

    /// 切换分组并把选中项夹回 0。
    ///
    /// 不夹回 0 会让用户从「符号」第 30 项切到「片段」第 2 项——
    /// 两个分组长度差 10 倍，视觉上像是随机的。
    pub fn set_group(&mut self, group: SymbolGroup) {
        if self.group != group {
            self.group = group;
            self.selected = 0;
        }
    }

    /// 设置搜索词。
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
    }

    /// 搜索是否改过。
    pub fn query_changed(&self) -> bool {
        self.opened_query.as_deref() != Some(self.query.as_str())
    }

    /// 当前分组在搜索下的可见条目。
    pub fn visible(&self) -> Vec<SymbolItem> {
        search(items_of(self.group), &self.query)
    }

    /// 移动选中项（`delta = ±1`，越界环绕）。
    pub fn move_selection(&mut self, delta: isize, total: usize) {
        if total == 0 {
            self.selected = 0;
            return;
        }
        let n = total as isize;
        let next = (self.selected as isize + delta) % n;
        self.selected = next.rem_euclid(n) as usize;
    }

    /// 把选中项夹进候选范围。
    pub fn clamp(&mut self, total: usize) {
        if total == 0 {
            self.selected = 0;
        } else if self.selected >= total {
            self.selected = total - 1;
        }
    }
}

/// 在分组的条目里按搜索词过滤。
///
/// 匹配 `label` 的**任一关键词**（按空格拆），而非整串。
/// 原型的 `label` 就是空格分隔的多关键词（「实心星 star」），
/// 整串匹配会让 `star` 命中不了「实心星 star」这一条——因为
/// 中间夹了别的词。
pub fn search(items: &[SymbolItem], query: &str) -> Vec<SymbolItem> {
    let q = query.trim();
    if q.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .filter(|item| {
            item.label
                .split_whitespace()
                .any(|keyword| fuzzy_match(keyword, q).is_some())
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_has_items_and_a_label() {
        for group in SymbolGroup::ALL {
            assert!(!items_of(*group).is_empty(), "{:?} 不应是空分组", group);
            assert!(!group.label().is_empty());
        }
    }

    #[test]
    fn every_item_has_distinct_display_data() {
        for group in SymbolGroup::ALL {
            for item in items_of(*group) {
                assert!(!item.text.is_empty(), "{:?} 有空插入文本", group);
                assert!(!item.glyph.is_empty(), "{:?} 有空字形", group);
                assert!(!item.label.is_empty(), "{:?} 有空名称", group);
            }
        }
    }

    #[test]
    fn no_snippet_is_a_foreign_template_shortcode() {
        // 本项目不消费 Hugo 短代码，插进去只会得到永不渲染的文本。
        for item in items_of(SymbolGroup::Snippet) {
            assert!(
                !item.text.contains("{{<"),
                "片段不应含本应用不消费的短代码：{}",
                item.text
            );
        }
    }

    #[test]
    fn empty_query_returns_the_whole_group() {
        let out = search(items_of(SymbolGroup::Snippet), "");
        assert_eq!(out.len(), items_of(SymbolGroup::Snippet).len());
    }

    #[test]
    fn any_keyword_can_match() {
        // 「实心星 star」：搜中文名与搜英文名都应命中。
        let out = search(items_of(SymbolGroup::Symbol), "实心星");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "★");
        let out = search(items_of(SymbolGroup::Symbol), "star");
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn no_match_returns_empty() {
        assert!(search(items_of(SymbolGroup::Emoji), "zzzz").is_empty());
    }

    #[test]
    fn search_does_not_leak_across_groups() {
        // 「表格」只在片段组；符号组与表情组都不该出现它。
        assert!(!search(items_of(SymbolGroup::Snippet), "表格").is_empty());
        assert!(search(items_of(SymbolGroup::Symbol), "表格").is_empty());
        assert!(search(items_of(SymbolGroup::Emoji), "表格").is_empty());
    }

    #[test]
    fn switching_group_resets_selection() {
        let mut state = SymbolUiState::new();
        state.selected = 15;
        state.set_group(SymbolGroup::Symbol);
        assert_eq!(state.selected, 0);
        // 同组重复点击不应重置（点「表情」时不该跳回第一项）。
        state.selected = 3;
        state.set_group(SymbolGroup::Symbol);
        assert_eq!(state.selected, 3);
    }

    #[test]
    fn opening_resets_everything() {
        let mut state = SymbolUiState::new();
        state.set_group(SymbolGroup::Snippet);
        state.set_query("表格");
        state.selected = 2;
        state.open();
        assert_eq!(state.group, SymbolGroup::Emoji);
        assert!(state.query.is_empty());
        assert_eq!(state.selected, 0);
        assert!(!state.query_changed());
    }

    #[test]
    fn selection_wraps_and_clamps() {
        let mut state = SymbolUiState::new();
        state.move_selection(-1, 5);
        assert_eq!(state.selected, 4);
        state.move_selection(1, 5);
        assert_eq!(state.selected, 0);
        state.selected = 9;
        state.clamp(5);
        assert_eq!(state.selected, 4);
        state.move_selection(1, 0);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn narrowing_the_search_pulls_selection_back() {
        let mut state = SymbolUiState::new();
        state.set_group(SymbolGroup::Snippet);
        state.selected = 15;
        // 切到「片段」组（片段在默认的「表情」组里搜不到），
        // 再用能命中片段的词收窄。
        state.set_query("代码块");
        let total = state.visible().len();
        assert!(total > 0, "前提：这一组应有命中");
        state.clamp(total);
        assert!(state.selected < total);
    }
}
