# GFM 全特性语料（golden）

> 本文件是 `tests/integration/golden_test.rs` 的固定输入。
> **修改前必须先跑一次测试看diff**，确认是有意变更再更新期望值
> （流程见 `CONTRIBUTING.md`）。

## 标题

# 一级标题

## 二级标题

### 三级标题

Setext 风格标题
==============

另一个 Setext
-------------

## 段落与换行

第一段，包含 `行内代码`、**粗体**、*斜体*、~~删除线~~、
[链接](https://example.com) 与裸链接 https://example.com/auto 。

第二段。
同一段落后跟两个空格的硬换行。

## 列表

- 无序项1
- 无序项2
  - 嵌套项 2.1
  - 嵌套项 2.2
- 无序项3

1. 有序项 1
2. 有序项 2
   1. 嵌套有序 2.1
   2. 嵌套有序 2.2
3. 有序项 3

## 任务列表

- [x] 已完成的任务
- [ ] 未完成的任务
- [X] 大写 X 也应视为完成
- [ ] 带 **粗体** 的未完成任务

## 引用

> 这是引用。
> 引用有多行。

> 嵌套引用：
> > 里面的引用。

## 代码

行内 `code with <html> & "quotes"`。

```rust
fn main() {
    let s = "字符串里的 // 不是注释";
    // 真正的注释
    println!("{s}");
}
```

```
无语言标记的围栏
```

## GFM 表格

| 左对齐 | 居中 | 右对齐 |
|:-------|:----:|------:|
| a      | b    | c     |
| 1      | 2    | 3     |
| 包含 `code` | **粗** | ![](missing.png) |

## 分割线

---

***

___

## 图片

![替代文本](images/photo.png)

![图片带标题](images/photo.png "这是 title")

## 链接引用

[引用式链接][ref]

[ref]: https://example.com/ref "引用标题"

## 脚注

这是带脚注的句子[^1]，还有另一个[^long]。

[^1]: 脚注一的内容。

[^long]: 脚注二的内容，
    跨多行的脚注。

## HTML 内联

<div align="center">
  <strong>原始 HTML 块</strong>
</div>

段落中的 <abbr title="缩写">内联 HTML</abbr>。

<!-- 这是一条注释，应被解析器跳过 -->

## 特殊字符

* 星号 _ 下划线 ` 反引号 # 井号 [ 方括号 ] ( 圆括号 )

> 引用里的 \\ 反斜杠

<https://autolink.example.com>

&copy; &amp; &lt;实体&gt;