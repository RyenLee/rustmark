//! 符号分组模型与静态目录（表情 / 标点符号两张表 + 分组取数）。

use super::snippets::SNIPPETS;

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

/// 某分组的全部条目。
pub const fn items_of(group: SymbolGroup) -> &'static [SymbolItem] {
    match group {
        SymbolGroup::Emoji => EMOJI,
        SymbolGroup::Symbol => SYMBOLS,
        SymbolGroup::Snippet => SNIPPETS,
    }
}
