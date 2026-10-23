//! Markdown 片段静态表（一键插入结构）。

use super::catalog::SymbolItem;

/// Markdown 片段（一键插入结构）。
///
/// 刻意不含原型的 `{{< highlight rust >}}`：那是 Hugo 短代码，
/// 本项目的导出器不消费它，插进去就是一段永不渲染的文本。
pub(super) const SNIPPETS: &[SymbolItem] = &[
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
