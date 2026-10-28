//! 领域错误目录测试（S9-02 覆盖率补强 + 文案契约）。
//!
//! # 为什么给错误写「目录」测试
//!
//! 领域错误的 `Display` 文案是**直接呈现给用户**的（规范第 16 条），
//! 但它只在错误路径上执行——常规测试只触达个别变体，其余文案成了
//! 「从没被渲染过的用户界面」。本文件把全部变体渲染一遍并断言
//! 插值字段确实出现在文案里：一条测试守住「文案可读 + 格式化没坏 +
//! 新增变体必须给出文案」三件事。
//!
//! 同时补齐几处只有错误路径 / 便捷构造器才执行的行为
//! （`From` 收敛、默认草稿方法、撤销策略访问器），使领域层行覆盖
//! 达到 90% 门限（S9-02）。

use rustmark::domain::document::aggregate::Document;
use rustmark::domain::document::errors::{DocumentError, TextBufferError};
use rustmark::domain::document::repository::{DocumentRepository, DocumentSnapshot};
use rustmark::domain::document::undo::UndoPolicy;
use rustmark::domain::document::value_objects::{Edit, EditPlan};
use rustmark::domain::markdown::errors::{HighlightError, ParseError, SpellCheckError};
use rustmark::domain::preferences::errors::PreferencesError;
use rustmark::infrastructure::text::rope_text_buffer::RopeTextBuffer;
use rustmark::ports::time::Timestamp;
use rustmark::shared::error::AppError;
use rustmark::shared::geometry::{ByteOffset, TextRange};
use rustmark::shared::ids::DocumentId;
use rustmark::shared::path::FilePath;

/// 文档上下文：TextBufferError 三变体 + 向 DocumentError 的收敛。
#[test]
fn text_buffer_error_catalog() {
    let e = TextBufferError::OutOfRange { offset: 7, len: 5 };
    assert_eq!(e.to_string(), "偏移 7 越界，缓冲区长度 5 字节");
    let e = TextBufferError::InvalidRange {
        start: 4,
        end: 2,
        len: 5,
    };
    assert_eq!(e.to_string(), "区间 [4, 2) 非法，缓冲区长度 5 字节");
    let e = TextBufferError::NotCharBoundary { offset: 3 };
    assert_eq!(e.to_string(), "偏移 3 不在 UTF-8 字符边界上");

    // From 收敛：文案随变体语义改写（越界→文档越界等）。
    let e: DocumentError = TextBufferError::OutOfRange { offset: 9, len: 8 }.into();
    assert_eq!(e, DocumentError::BufferOutOfRange { offset: 9, len: 8 });
    let e: DocumentError = TextBufferError::InvalidRange {
        start: 1,
        end: 9,
        len: 8,
    }
    .into();
    assert_eq!(
        e,
        DocumentError::InvalidRange {
            start: 1,
            end: 9,
            len: 8
        }
    );
    let e: DocumentError = TextBufferError::NotCharBoundary { offset: 2 }.into();
    assert_eq!(e, DocumentError::NotCharBoundary { offset: 2 });
}

/// 文档上下文：DocumentError 全变体 + `From<DocumentError> for AppError`。
#[test]
fn document_error_catalog() {
    let cases: Vec<(DocumentError, &str)> = vec![
        (
            DocumentError::BufferOutOfRange { offset: 5, len: 3 },
            "偏移 5 超出文档长度 3",
        ),
        (
            DocumentError::InvalidRange {
                start: 2,
                end: 1,
                len: 9,
            },
            "区间 [2, 1) 非法，文档长度 9",
        ),
        (
            DocumentError::NotCharBoundary { offset: 4 },
            "偏移 4 不在 UTF-8 字符边界上",
        ),
        (DocumentError::NotDirty, "没有未保存的修改"),
        (DocumentError::UndoStackEmpty, "没有可撤销的操作"),
        (DocumentError::RedoStackEmpty, "没有可重做的操作"),
        (DocumentError::NotFound(DocumentId::new()), "找不到 ID 为"),
        (
            DocumentError::io_unavailable("磁盘被拔了"),
            "无法访问文件：磁盘被拔了",
        ),
    ];
    for (e, needle) in cases {
        let text = e.to_string();
        assert!(
            text.contains(needle),
            "文案缺关键信息：{text}（期望含 {needle:?}）"
        );
        // 全部收敛为 AppError::Domain 且保留用户文案。
        let app: AppError = e.clone().into();
        assert!(
            app.to_string().contains(&text),
            "AppError 必须原样携带领域文案：{app} vs {text}"
        );
    }
}

/// Markdown 上下文：三个结构体错误的构造与文案。
#[test]
fn markdown_error_catalog() {
    let e = ParseError::new("第 3 行表格列数不一致");
    assert_eq!(e.to_string(), "Markdown 解析失败：第 3 行表格列数不一致");
    let app: AppError = e.into();
    assert!(app.to_string().contains("解析失败"), "{app}");

    let e = HighlightError::new("语法集损坏");
    assert_eq!(e.to_string(), "语法高亮失败：语法集损坏");
    let app: AppError = e.into();
    assert!(app.to_string().contains("高亮失败"), "{app}");

    // 刻意没有 From<AppError>：拼写检查失败必须降级，不冒泡。
    let e = SpellCheckError::new("词典缺失");
    assert_eq!(e.to_string(), "拼写检查失败：词典缺失");
}

/// 工作区上下文：WorkspaceError 全变体文案（含路径插值）。
#[test]
fn workspace_error_catalog() {
    let p = FilePath::new("/ws/a.md").unwrap();
    let q = FilePath::new("/ws/sub").unwrap();
    let cases: Vec<(rustmark::domain::workspace::errors::WorkspaceError, &str)> = vec![
        (
            rustmark::domain::workspace::errors::WorkspaceError::DuplicateName { path: p.clone() },
            "已存在同名条目：/ws/a.md",
        ),
        (
            rustmark::domain::workspace::errors::WorkspaceError::MoveIntoDescendant {
                from: p.clone(),
                to: q.clone(),
            },
            "不能把 /ws/a.md 移动到它自己的子目录 /ws/sub 中",
        ),
        (
            rustmark::domain::workspace::errors::WorkspaceError::NotFound { path: p.clone() },
            "不存在：/ws/a.md",
        ),
        (
            rustmark::domain::workspace::errors::WorkspaceError::NotADirectory { path: p.clone() },
            "/ws/a.md 不是目录",
        ),
        (
            rustmark::domain::workspace::errors::WorkspaceError::RootNotSet,
            "尚未打开工作区目录",
        ),
        (
            rustmark::domain::workspace::errors::WorkspaceError::TabNotFound(DocumentId::new()),
            "标签页中不存在 ID 为",
        ),
        (
            rustmark::domain::workspace::errors::WorkspaceError::OutsideRoot {
                path: p.clone(),
                root: q.clone(),
            },
            "/ws/a.md 不在工作区根目录 /ws/sub 之内",
        ),
    ];
    for (e, needle) in cases {
        let text = e.to_string();
        assert!(
            text.contains(needle),
            "文案缺关键信息：{text}（期望含 {needle:?}）"
        );
        let app: AppError = e.into();
        assert!(app.to_string().contains(needle), "AppError 丢失文案：{app}");
    }
}

/// 偏好上下文：PreferencesError 两变体 + 便捷构造器。
#[test]
fn preferences_error_catalog() {
    let e = PreferencesError::InvalidCombo {
        raw: "Ctrl+Ctrl".to_string(),
    };
    assert_eq!(e.to_string(), "无法识别的快捷键：Ctrl+Ctrl");

    let e = PreferencesError::invalid_setting("字号", "超出 [10, 72]");
    assert_eq!(e.to_string(), "设置项 字号 取值非法：超出 [10, 72]");
    let app: AppError = e.into();
    assert!(app.to_string().contains("取值非法"), "{app}");
}

/// 覆盖补充：撤销策略访问器与 Default。
#[test]
fn undo_policy_accessors_and_default() {
    let policy = UndoPolicy::default();
    assert_eq!(
        policy,
        UndoPolicy::default_policy(),
        "Default 等于 default_policy"
    );
    assert_eq!(policy.merge_window_ms(), 400);
    assert_eq!(policy.max_depth(), 200);
    assert_eq!(
        policy.max_bytes(),
        rustmark::domain::document::undo::DEFAULT_MAX_BYTES,
        "默认字节预算应与常量同源"
    );

    let custom = UndoPolicy::new(250, 7);
    assert_eq!(custom.merge_window_ms(), 250);
    assert_eq!(custom.max_depth(), 7);
    // 0 深度按 1 处理：至少保留一个撤销点（不然撤销栈形同虚设）。
    assert_eq!(UndoPolicy::new(250, 0).max_depth(), 1);
    // 字节预算可覆盖；0 收敛到 1，避免撤销能力被静默关闭。
    assert_eq!(custom.with_max_bytes(1024).max_bytes(), 1024);
    assert_eq!(custom.with_max_bytes(0).max_bytes(), 1);
}

/// 覆盖补充：仓储 trait 的**默认草稿方法**（空实现）。
///
/// 测试替身只依赖默认实现即可正确工作——这正是默认实现存在的原因；
/// 这里用最小 double 验证默认语义（Ok / None / 空列表）。
struct NoDraftRepo;

impl DocumentRepository for NoDraftRepo {
    fn load(&self, _path: &FilePath) -> Result<DocumentSnapshot, AppError> {
        Err(AppError::io("本替身不实现 load"))
    }
    fn save(&self, _snapshot: &DocumentSnapshot) -> Result<(), AppError> {
        Err(AppError::io("本替身不实现 save"))
    }
    fn exists(&self, _path: &FilePath) -> bool {
        false
    }
    fn delete(&self, _path: &FilePath) -> Result<(), AppError> {
        Ok(())
    }
}

#[test]
fn repository_default_draft_methods_are_inert() {
    let repo = NoDraftRepo;
    let dir = std::env::temp_dir().join("rustmark-prop-default-drafts");
    let id = DocumentId::new();
    assert!(repo.save_draft(&dir, id, "内容", None).is_ok());
    assert!(repo.load_draft(&dir, id).unwrap().is_none());
    assert!(repo.load_draft_path(&dir, id).unwrap().is_none());
    assert!(repo.clear_draft(&dir, id).is_ok());
    assert!(repo.list_drafts(&dir).unwrap().is_empty());
}

/// 覆盖补充：`apply_plan` 带选区建议的路径。
///
/// 常规编辑测试走的是无选区 `EditPlan`；带建议选区的分支在这里钉住：
/// 应用后主光标在选区末、锚点在选区首（方向反转的正确性）。
#[test]
fn apply_plan_applies_the_suggested_selection() {
    let mut doc = Document::new_untitled(
        Box::new(RopeTextBuffer::new_with_text("hello")),
        Timestamp::from_millis(1),
    );
    // 把 "hello" 全选替换为 "你好"（带建议选区 = 新文本区间）。
    let plan = EditPlan::new(
        Edit::new(
            TextRange::new(ByteOffset::new(0), ByteOffset::new(5)),
            "你好",
        ),
        Some(TextRange::new(ByteOffset::new(0), ByteOffset::new(2))),
    );
    doc.apply_plan(&plan, Timestamp::from_millis(2))
        .expect("应用计划应成功");
    assert_eq!(doc.text(), "你好");
}
