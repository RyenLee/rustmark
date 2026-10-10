//! Document 用例服务测试（S4-04）。
//!
//! 覆盖：打开 / 新建 / 保存（成功与失败语义）/ 编辑 / 撤销重做 / 关闭决策 / 统计。
//! 每个服务均包含主路径 + ≥2 错误路径。

use std::sync::{Arc, RwLock};

use rustmark::application::document::commands::*;
use rustmark::application::document::queries::*;
use rustmark::application::document::registry::DocumentRegistry;
use rustmark::application::document::service::{DocumentAppService, compute_stats};
use rustmark::application::dto::{CloseDecisionView, StatsView};
use rustmark::domain::document::repository::DocumentRepository;
use rustmark::shared::path::FilePath;

use super::fakes::*;

fn fp(s: &str) -> FilePath {
    FilePath::new(s).unwrap()
}

/// 装配一个指向共享仓储与事件总线的文档服务。
fn build(
    repo: Arc<dyn DocumentRepository>,
) -> (
    DocumentAppService,
    Arc<dyn rustmark::shared::event::EventBus>,
) {
    let registry = Arc::new(RwLock::new(DocumentRegistry::new()));
    let bus: Arc<dyn rustmark::shared::event::EventBus> = Arc::new(InMemoryEventBus::new());
    let buffer_factory: Arc<dyn rustmark::domain::document::services::BufferFactory> =
        Arc::new(FakeBufferFactory);
    let clock: Arc<dyn rustmark::shared::time::Clock> = Arc::new(FakeClock::new(0));
    let service = DocumentAppService::new(registry, repo, bus.clone(), buffer_factory, clock);
    (service, bus.clone())
}

#[test]
fn open_existing_file_succeeds() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    repo.seed(fp("/docs/notes.md"), "# Title\nbody", None);
    let (mut svc, _bus) = build(repo);

    let id = svc
        .open(OpenFileCommand {
            path: fp("/docs/notes.md"),
        })
        .unwrap();
    let view = svc.get_document(GetDocumentQuery { id }).unwrap();
    assert_eq!(view.path.as_ref(), Some(&fp("/docs/notes.md")));
    assert!(!view.is_dirty, "刚打开的文件应为干净态");
    assert_eq!(view.byte_len, "# Title\nbody".len());
}

#[test]
fn open_missing_file_fails() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    let (mut svc, _bus) = build(repo);

    let res = svc.open(OpenFileCommand {
        path: fp("/docs/missing.md"),
    });
    assert!(res.is_err(), "打开不存在的文件必须返回 Err");
}

#[test]
fn new_document_is_untitled_and_clean() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    let (mut svc, _bus) = build(repo);

    let id = svc.new_document(NewDocumentCommand).unwrap();
    let view = svc.get_document(GetDocumentQuery { id }).unwrap();
    assert!(view.path.is_none(), "未命名文档路径应为 None");
    assert!(!view.is_dirty);
}

#[test]
fn save_persists_and_clears_dirty_flag() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    repo.seed(fp("/docs/notes.md"), "orig", None);
    let (mut svc, _bus) = build(repo.clone());

    let id = svc
        .open(OpenFileCommand {
            path: fp("/docs/notes.md"),
        })
        .unwrap();
    svc.insert_text(InsertTextCommand {
        id,
        offset: 0,
        text: " more".to_string(),
    })
    .unwrap();
    assert!(svc.get_document(GetDocumentQuery { id }).unwrap().is_dirty);

    svc.save(SaveCommand { id, save_as: false }).unwrap();
    let view = svc.get_document(GetDocumentQuery { id }).unwrap();
    assert!(!view.is_dirty, "保存成功必须清除脏标记");
    assert_eq!(repo.len(), 1, "保存必须写入仓储");
    assert_eq!(
        svc.get_content(GetContentQuery { id, range: None })
            .unwrap(),
        //offset 0 为前插：内容变为 " more" + "orig"。
        " moreorig"
    );
}

#[test]
fn save_failure_keeps_dirty_flag() {
    let repo = Arc::new(FailingDocumentRepository::new());
    repo.seed(fp("/docs/notes.md"), "orig", None);
    let (mut svc, _bus) = build(repo);

    let id = svc
        .open(OpenFileCommand {
            path: fp("/docs/notes.md"),
        })
        .unwrap();
    svc.insert_text(InsertTextCommand {
        id,
        offset: 0,
        text: "x".to_string(),
    })
    .unwrap();
    assert!(svc.get_document(GetDocumentQuery { id }).unwrap().is_dirty);

    let res = svc.save(SaveCommand { id, save_as: false });
    assert!(res.is_err(), "保存失败应返回 Err");
    assert!(
        svc.get_document(GetDocumentQuery { id }).unwrap().is_dirty,
        "保存失败时脏标记必须保留（防止误判为已保存）"
    );
}

#[test]
fn save_untitled_without_path_fails() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    let (mut svc, _bus) = build(repo);

    let id = svc.new_document(NewDocumentCommand).unwrap();
    let res = svc.save(SaveCommand { id, save_as: false });
    assert!(res.is_err(), "未命名文档未经另存为必须返回 Err");
}

#[test]
fn insert_then_undo_then_redo() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    repo.seed(fp("/docs/a.md"), "", None);
    let (mut svc, _bus) = build(repo);

    let id = svc
        .open(OpenFileCommand {
            path: fp("/docs/a.md"),
        })
        .unwrap();
    svc.insert_text(InsertTextCommand {
        id,
        offset: 0,
        text: "abc".to_string(),
    })
    .unwrap();
    assert_eq!(
        svc.get_content(GetContentQuery { id, range: None })
            .unwrap(),
        "abc"
    );

    svc.undo(UndoCommand { id }).unwrap();
    assert_eq!(
        svc.get_content(GetContentQuery { id, range: None })
            .unwrap(),
        ""
    );

    svc.redo(RedoCommand { id }).unwrap();
    assert_eq!(
        svc.get_content(GetContentQuery { id, range: None })
            .unwrap(),
        "abc"
    );
}

#[test]
fn undo_empty_stack_fails() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    repo.seed(fp("/docs/a.md"), "", None);
    let (mut svc, _bus) = build(repo);

    let id = svc
        .open(OpenFileCommand {
            path: fp("/docs/a.md"),
        })
        .unwrap();
    let res = svc.undo(UndoCommand { id });
    assert!(res.is_err(), "空撤销栈必须返回 Err");
}

#[test]
fn replace_range_overwrites_selection() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    repo.seed(fp("/docs/a.md"), "hello world", None);
    let (mut svc, _bus) = build(repo);

    let id = svc
        .open(OpenFileCommand {
            path: fp("/docs/a.md"),
        })
        .unwrap();
    svc.replace_range(ReplaceRangeCommand {
        id,
        range: rustmark::shared::geometry::TextRange::new(
            rustmark::shared::geometry::ByteOffset::new(0),
            rustmark::shared::geometry::ByteOffset::new(5),
        ),
        text: "HI".to_string(),
    })
    .unwrap();
    assert_eq!(
        svc.get_content(GetContentQuery { id, range: None })
            .unwrap(),
        "HI world"
    );
}

#[test]
fn close_dirty_asks_for_save() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    repo.seed(fp("/docs/a.md"), "x", None);
    let (mut svc, _bus) = build(repo);

    let id = svc
        .open(OpenFileCommand {
            path: fp("/docs/a.md"),
        })
        .unwrap();
    svc.insert_text(InsertTextCommand {
        id,
        offset: 0,
        text: "y".to_string(),
    })
    .unwrap();

    let decision = svc.close(CloseCommand { id, force: false }).unwrap();
    assert_eq!(decision, CloseDecisionView::NeedsSave);
    assert_eq!(svc.list_open(ListOpenDocumentsQuery).unwrap().len(), 1);
}

#[test]
fn close_force_discards_and_removes() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    repo.seed(fp("/docs/a.md"), "x", None);
    let (mut svc, _bus) = build(repo);

    let id = svc
        .open(OpenFileCommand {
            path: fp("/docs/a.md"),
        })
        .unwrap();
    let decision = svc.close(CloseCommand { id, force: true }).unwrap();
    assert_eq!(decision, CloseDecisionView::Closed);
    assert!(svc.list_open(ListOpenDocumentsQuery).unwrap().is_empty());
}

#[test]
fn close_unopened_document_fails() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    let (mut svc, _bus) = build(repo);

    let res = svc.close(CloseCommand {
        id: rustmark::shared::ids::DocumentId::new(),
        force: true,
    });
    assert!(res.is_err());
}

#[test]
fn get_stats_counts_words_and_tasks() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    repo.seed(
        fp("/docs/tasks.md"),
        "# Plan\n- [ ] todo\n- [x] done\ntext here",
        None,
    );
    let (mut svc, _bus) = build(repo);

    let id = svc
        .open(OpenFileCommand {
            path: fp("/docs/tasks.md"),
        })
        .unwrap();
    let stats: StatsView = svc.get_stats(GetStatsQuery { id }).unwrap();
    // Plan / todo / x / done / text / here = 6 个西文词。
    // 列表标记 `-` 不计（它没有字母数字），但 `[x]` 里的 `x` 是字母，算一个。
    assert_eq!(stats.words, 6, "西文按词计，孤立连字符不计");
    assert_eq!(stats.tasks_total, 2);
    assert_eq!(stats.tasks_done, 1);
    assert_eq!(stats.reading_minutes, 1, "下限为 1 分钟");
}

// ============================================================================
// 统计口径（逐项对齐原型 `computeStats`）
// ============================================================================

/// 中日韩按**字**、西文按**词**。
///
/// 这条是口径里最容易被改坏的一条：改回 `split_whitespace().count()`
/// 后中文文档的字数会少一个数量级，而测试要能第一时间发现。
#[test]
fn words_count_cjk_per_character_and_latin_per_word() {
    let cn = compute_stats("这是一段中文", "");
    assert_eq!(cn.words, 6, "6 个汉字 = 6 字（按空白切分只会得 1）");

    let mixed = compute_stats("中文 mixed with English", "");
    assert_eq!(mixed.words, 2 + 3, "2 个汉字 + 3 个西文词");

    let latin = compute_stats("well-known state_of_the_art", "");
    assert_eq!(latin.words, 2, "连字符与下划线不拆词");

    let empty = compute_stats("", "");
    assert_eq!(empty.words, 0, "空文档 0 字");
}

/// 字符数与行数。
#[test]
fn characters_and_lines_match_the_prototype() {
    let s = compute_stats("第一行\n第二行\n", "");
    assert_eq!(s.characters, 8, "含换行的 Unicode 标量数");
    assert_eq!(s.characters_no_space, 6, "不含换行");
    // 末尾换行之后还有一行空的，split 出来是 3 行（对齐原型的 split('\n')）。
    assert_eq!(s.lines, 3);
    assert_eq!(
        compute_stats("", "").lines,
        1,
        "空文档算 1 行（不显示 0 行）"
    );
}

/// 阅读时长：原型口径 `max(1, round(words / 300))`。
#[test]
fn reading_minutes_follows_the_prototype_formula() {
    assert_eq!(compute_stats("a", "").reading_minutes, 1, "下限 1");
    // 600 个西文词 → 2 分钟。
    let words = (0..600)
        .map(|i| format!("w{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(compute_stats(&words, "").reading_minutes, 2);
    // 299 词仍不足 1 分钟 → 取整后为 0，但下限抬到 1。
    let few = (0..299)
        .map(|i| format!("w{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(compute_stats(&few, "").reading_minutes, 1);
}

/// 段落数：被空行分隔的连续非空行各算一段。
#[test]
fn paragraphs_are_blank_separated_blocks() {
    let s = compute_stats("第一段\n第二行\n\n第二段\n\n\n第三段", "");
    assert_eq!(s.paragraphs, 3, "连续多个空行只分隔一次");
    assert_eq!(compute_stats("", "").paragraphs, 0);
    assert_eq!(compute_stats("\n\n\n", "").paragraphs, 0, "全是空行则 0 段");
}

/// 标题数：`#` ~ `######`，且围栏代码块内的不计。
#[test]
fn headings_ignore_fenced_code() {
    let s = compute_stats("# 一\n## 二\n###### 六\n####### 七\n#NoSpace", "");
    assert_eq!(
        s.headings, 3,
        "6 级以内 + 必须跟空格（`#NoSpace` 不是标题）"
    );
    assert_eq!(s.code_blocks, 0);

    let fenced = compute_stats("```\n## 不是标题\n# 也不是\n```\n# 真标题", "");
    assert_eq!(fenced.headings, 1, "围栏内的 # 不该被计成标题");
    assert_eq!(fenced.code_blocks, 1);
}

/// 围栏：` ``` ` 与 `~~~ ` 都能开启/闭合，缩进超过 3 空格不算围栏。
#[test]
fn fences_close_only_on_the_same_marker() {
    let s = compute_stats("~~~\n# 不是标题\n~~~\n# 真标题", "");
    assert_eq!(s.code_blocks, 1);
    assert_eq!(s.headings, 1, "~~~ 围栏内的 # 不计");

    let indented = compute_stats("    ```\n    # 缩进代码\n", "");
    assert_eq!(
        indented.code_blocks, 0,
        "4 空格缩进的是缩进代码块而非围栏（CommonMark 规则）"
    );
}

/// 任务项：`- [ ]` / `* [x]` / `+ [X]`，`-` 与 `[` 之间允许空白。
#[test]
fn tasks_accept_all_three_markers_with_optional_space() {
    let s = compute_stats("- [ ] a\n* [x] b\n+ [X] c\n-  [ ] d\n-[ ] e", "");
    assert_eq!(s.tasks_total, 5, "三种标记都认，`-` 与 `[` 间可有空白");
    assert_eq!(s.tasks_done, 2, "x 与 X 都是已完成");

    let not_task = compute_stats("- [] 空括号\n- [y] 非法状态\n-[普通链接]", "");
    assert_eq!(
        not_task.tasks_total, 0,
        "`[]` / `[y]` / 紧随的 `[]` 都不是任务项"
    );
}

/// 链接与图片：图片不计入链接数，括号可嵌套。
#[test]
fn links_exclude_images_and_allow_nested_parens() {
    let s = compute_stats("[a](b) ![图](c.png) [带括号](d(e)f) [裸括号] [^1]", "");
    assert_eq!(s.links, 2, "裸括号与脚注引用不是链接");
    assert_eq!(s.images, 1, "图片单独计数");
}

/// 围栏内的任务项不计数。
#[test]
fn tasks_inside_fences_are_ignored() {
    let s = compute_stats("```\n- [ ] 代码里的\n```\n- [ ] 真的", "");
    assert_eq!(s.tasks_total, 1, "代码示例里的任务项不该计数");
}

#[test]
fn set_then_get_selection() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    repo.seed(fp("/docs/a.md"), "hello", None);
    let (mut svc, _bus) = build(repo);

    let id = svc
        .open(OpenFileCommand {
            path: fp("/docs/a.md"),
        })
        .unwrap();
    svc.set_selection(SetSelectionCommand {
        id,
        selection: rustmark::shared::geometry::TextRange::new(
            rustmark::shared::geometry::ByteOffset::new(0),
            rustmark::shared::geometry::ByteOffset::new(3),
        ),
    })
    .unwrap();
    let sel = svc.get_selection(GetSelectionQuery { id }).unwrap();
    assert!(sel.is_some());
    assert_eq!(sel.unwrap().end, 3);
}

/// 打开顺序即 `list_open` 的返回顺序（标签条顺序 / `Alt+N` 下标依据）。
///
/// 这条锁的是 S6-08 修掉的一个缺陷：注册表曾用 `HashMap` 直接迭代，
/// 而 `HashMap` 的迭代顺序**不确定**——于是
///
/// - 标签条顺序可能与打开顺序不一致；
/// - `Alt+2`（`GotoTab2`）按下后可能「切到自己」，表现为**快捷键毫无反应**；
/// - 「下一个标签」在两个文档之间来回跳。
///
/// 这些都不报错，只是行为随机，因此必须由测试钉住顺序本身，
/// 而不是「恰好这次能对上」。
#[test]
fn list_open_returns_documents_in_the_order_they_were_opened() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    for name in ["a", "b", "c", "d", "e"] {
        repo.seed(fp(&format!("/docs/{name}.md")), name, None);
    }
    let (mut svc, _bus) = build(repo);

    let mut opened = Vec::new();
    for name in ["a", "b", "c", "d", "e"] {
        opened.push(
            svc.open(OpenFileCommand {
                path: fp(&format!("/docs/{name}.md")),
            })
            .unwrap(),
        );
    }

    let listed: Vec<_> = svc
        .list_open(ListOpenDocumentsQuery)
        .unwrap()
        .into_iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(listed, opened, "文档顺序必须与打开顺序逐一对应");
}

/// 重复插入同一文档**不**改变它的位置。
///
/// 「内容更新」与「新开标签」是两件事：后者才改变标签条顺序。
/// 若重复插入把文档挪到末尾，用户正在编辑的标签会突然跳到最右，
/// 而他并没有做任何切换操作。
#[test]
fn reinserting_a_document_keeps_its_position() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    repo.seed(fp("/docs/a.md"), "a", None);
    repo.seed(fp("/docs/b.md"), "b", None);
    let (mut svc, _bus) = build(repo);

    let a = svc
        .open(OpenFileCommand {
            path: fp("/docs/a.md"),
        })
        .unwrap();
    svc.open(OpenFileCommand {
        path: fp("/docs/b.md"),
    })
    .unwrap();

    // 改 `a` 的内容（内部会重新插入同一 ID）。
    svc.insert_text(InsertTextCommand {
        id: a,
        offset: 0,
        text: "z".to_string(),
    })
    .unwrap();

    let listed: Vec<_> = svc
        .list_open(ListOpenDocumentsQuery)
        .unwrap()
        .into_iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(listed[0], a, "重新插入同一文档不应把它挪到末尾");
}

/// 关闭文档后，剩余文档的相对顺序不变。
#[test]
fn closing_a_document_preserves_the_order_of_the_rest() {
    let repo = Arc::new(InMemoryDocumentRepository::new());
    for name in ["a", "b", "c"] {
        repo.seed(fp(&format!("/docs/{name}.md")), name, None);
    }
    let (mut svc, _bus) = build(repo);

    let mut opened = Vec::new();
    for name in ["a", "b", "c"] {
        opened.push(
            svc.open(OpenFileCommand {
                path: fp(&format!("/docs/{name}.md")),
            })
            .unwrap(),
        );
    }
    svc.close(CloseCommand {
        id: opened[1],
        force: true,
    })
    .unwrap();

    let listed: Vec<_> = svc
        .list_open(ListOpenDocumentsQuery)
        .unwrap()
        .into_iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(
        listed,
        vec![opened[0], opened[2]],
        "关掉中间一个后，其余顺序应保持"
    );
}
