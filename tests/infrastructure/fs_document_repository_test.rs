//! `FsDocumentRepository` 单测（S5-04）。
//!
//! 重点是 **`FR-04` 原子保存**：任何失败路径都不得破坏原文件。
//! 这类用例无法靠「读回来看对不对」验证——必须显式构造失败并检查原文件字节。

use std::fs;
use std::path::{Path, PathBuf};

use rustmark::domain::document::repository::{DocumentRepository, DocumentSnapshot};
use rustmark::infrastructure::persistence::fs_document_repository::{
    FsDocumentRepository, LineEnding,
};
use rustmark::shared::ids::DocumentId;
use rustmark::shared::path::FilePath;

/// 测试用临时目录守卫：Drop 时递归删除。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        // 用「标签 + 进程 ID」命名，避免同机并行测试互相覆盖。
        let unique = format!(
            "rustmark-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let path = std::env::temp_dir().join(unique.replace(['(', ')', ' '], ""));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("应能创建临时目录");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 把磁盘路径包装成领域层的 `FilePath`。
fn fp(disk: &Path) -> FilePath {
    FilePath::new(disk.to_string_lossy().replace('\\', "/")).expect("临时路径应合法")
}

fn snapshot(disk: &Path, content: &str) -> DocumentSnapshot {
    DocumentSnapshot::new(DocumentId::new(), fp(disk), content, None)
}

#[test]
fn save_then_load_roundtrips_content() {
    let dir = TempDir::new("doc-roundtrip");
    let repo = FsDocumentRepository::new();
    let target = dir.join("note.md");

    repo.save(&snapshot(&target, "第一段\n第二段"))
        .expect("保存应成功");
    let loaded = repo.load(&fp(&target)).expect("读取应成功");
    assert_eq!(loaded.content, "第一段\n第二段");
    assert!(loaded.saved_at.is_some(), "保存过的文件应有修改时间");
}

#[test]
fn load_missing_file_returns_io_error_with_path() {
    let dir = TempDir::new("doc-missing");
    let repo = FsDocumentRepository::new();
    let target = dir.join("nope.md");

    let error = repo
        .load(&fp(&target))
        .expect_err("读取不存在的文件必须失败");
    // 错误信息必须带路径，否则用户看不懂是哪个文件出了问题。
    assert!(
        error.to_string().contains("nope.md"),
        "错误信息应包含路径，实际：{error}"
    );
}

#[test]
fn exists_and_delete_are_idempotent() {
    let dir = TempDir::new("doc-exists");
    let repo = FsDocumentRepository::new();
    let target = dir.join("x.md");

    assert!(!repo.exists(&fp(&target)), "尚不存在时应为 false");
    repo.save(&snapshot(&target, "c")).expect("保存应成功");
    assert!(repo.exists(&fp(&target)), "保存后应为 true");

    repo.delete(&fp(&target)).expect("首次删除应成功");
    assert!(!repo.exists(&fp(&target)));
    // 契约明说幂等：删不存在的文件不应报错。
    repo.delete(&fp(&target)).expect("重复删除应按成功处理");
}

#[test]
fn save_leaves_no_temp_file_behind() {
    let dir = TempDir::new("doc-temp");
    let repo = FsDocumentRepository::new();
    let target = dir.join("clean.md");
    repo.save(&snapshot(&target, "内容")).expect("保存应成功");

    let leftovers: Vec<String> = fs::read_dir(dir.path())
        .expect("应能列出目录")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with(".rustmark-tmp-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "保存后不应残留临时文件，实际：{leftovers:?}"
    );
}

#[test]
fn atomic_save_keeps_original_intact_when_target_is_a_directory() {
    let dir = TempDir::new("doc-atomic");
    let repo = FsDocumentRepository::new();
    let target = dir.join("occupied.md");
    // 先写入正常内容，确认它是「好」的。
    repo.save(&snapshot(&target, "原始内容"))
        .expect("首次保存应成功");

    // 制造失败：把目标路径占成一个**目录**，`rename` 到非空目录必然失败。
    let blocked = dir.join("blocked.md");
    fs::create_dir(&blocked).expect("应能创建目录");
    fs::write(blocked.join("child.txt"), b"x").expect("应能写子文件");

    let result = repo.save(&snapshot(&blocked, "新内容"));
    assert!(result.is_err(), "rename 到非空目录必须失败");

    // 关键断言：原文件（target）字节**完全未变**。
    // 内容不含 BOM——本实现读时剥离 BOM、写时不补回，故直接比对纯文本字节。
    assert_eq!(
        fs::read(&target).unwrap_or_default(),
        "原始内容".as_bytes(),
        "失败保存后原文件字节必须保持不变"
    );
}

#[test]
fn crlf_is_normalized_on_load_and_restored_on_save() {
    let dir = TempDir::new("doc-crlf");
    let repo = FsDocumentRepository::new();
    let target = dir.join("win.md");
    // 直接写入 CRLF 字节（绕过本仓储的编码逻辑）。
    fs::write(&target, b"line1\r\nline2\r\nline3").expect("应能写原始字节");

    let loaded = repo.load(&fp(&target)).expect("读取应成功");
    assert_eq!(
        loaded.content, "line1\nline2\nline3",
        "读取时必须把 CRLF 归一为 LF，否则行号计算会错"
    );

    repo.save(&snapshot(&target, &loaded.content))
        .expect("保存应成功");
    let raw = fs::read(&target).expect("应能读回");
    assert!(raw.windows(2).all(|w| w != b"\n\n"), "不应产生空行");
    assert_eq!(
        raw, b"line1\r\nline2\r\nline3",
        "写回应还原为 CRLF，保持原文件风格"
    );
}

#[test]
fn bom_is_stripped_on_load_and_restored_on_save() {
    let dir = TempDir::new("doc-bom");
    let repo = FsDocumentRepository::new();
    let target = dir.join("bom.md");
    // UTF-8 BOM + 正文
    let mut raw = vec![0xEF, 0xBB, 0xBF];
    raw.extend_from_slice("带 BOM 的内容".as_bytes());
    fs::write(&target, &raw).expect("应能写原始字节");

    let loaded = repo.load(&fp(&target)).expect("读取应成功");
    assert_eq!(
        loaded.content, "带 BOM 的内容",
        "读取时必须剥离 BOM，否则正文首部有不可见字符"
    );
    assert!(
        !loaded.content.starts_with('\u{feff}'),
        "内容不应以 BOM 字符开头"
    );

    repo.save(&snapshot(&target, &loaded.content))
        .expect("保存应成功");
    let after = fs::read(&target).expect("应能读回");
    // 「写回原编码」契约的一部分：原本带 BOM 的文件保存后仍带 BOM。
    // （此前的断言是「不补回」——那会让带 BOM 文件在保存一次后静默变味，
    // 依赖 BOM 的工具（如某些 PowerShell 脚本）从此读错编码。）
    assert!(
        after.starts_with(&[0xEF, 0xBB, 0xBF]),
        "原本带 BOM 的文件保存后应保留 BOM"
    );
    assert_eq!(
        &after[3..],
        "带 BOM 的内容".as_bytes(),
        "BOM 之外的正文应原样写回"
    );
}

#[test]
fn invalid_utf8_does_not_panic_and_uses_replacement_chars() {
    let dir = TempDir::new("doc-invalid");
    let repo = FsDocumentRepository::new();
    let target = dir.join("bad.md");
    // 孤立的 0xFF 不是合法 UTF-8。
    fs::write(&target, [0x61u8, 0xFF, 0x62]).expect("应能写原始字节");

    // 绝不能 panic：编辑器必须能打开任何文件。
    //
    // 这里走的是编码探测的**兜底路径**：输入既非合法 UTF-8、
    // 也不含任何合法的 UTF-8 多字节序列（只有孤立的 0xFF），
    // 因此被当作 GBK；0xFF 不是合法的 GBK 首字节，同样产出替换字符。
    // 无论走哪条分支，"不 panic + 坏字节可见"这一契约都不变。
    let loaded = repo.load(&fp(&target)).expect("读取应成功而非报错");
    assert!(
        loaded.content.contains('\u{fffd}'),
        "非法字节应被替换为 U+FFFD，实际 {:?}",
        loaded.content
    );
    assert!(
        loaded.content.starts_with('a') && loaded.content.ends_with('b'),
        "坏字节两旁的 ASCII 不得丢失，实际 {:?}",
        loaded.content
    );
}

/// GBK 编码的文件必须能**正确**打开。
///
/// 这是用户最常见的"乱码"来源：Windows 上历史中文文本多为 GBK，
/// 此前实现用 `from_utf8_lossy` 兜底，会把每个字节变成 `�`，
/// 而且一旦保存就把替换字符写回磁盘，**原文不可逆丢失**。
#[test]
fn gbk_encoded_file_is_decoded_correctly() {
    let dir = TempDir::new("doc-gbk");
    let repo = FsDocumentRepository::new();
    let target = dir.join("gbk.md");
    // "中文测试，编辑器" 的 GBK（CP936）字节。
    let bytes: &[u8] = &[
        0xD6, 0xD0, 0xCE, 0xC4, 0xB2, 0xE2, 0xCA, 0xD4, 0xA3, 0xAC, 0xB1, 0xE0, 0xBC, 0xAD, 0xC6,
        0xF7,
    ];
    fs::write(&target, bytes).expect("应能写原始字节");

    let loaded = repo.load(&fp(&target)).expect("GBK 文件应能打开");
    assert_eq!(
        loaded.content, "中文测试，编辑器",
        "GBK 内容应被正确解码，而不是逐字节变成替换字符"
    );
    assert!(
        !loaded.content.contains('\u{fffd}'),
        "GBK 文件不应产生任何替换字符"
    );
}

/// GBK 文件「打开 → 保存」后**仍是 GBK**，不能被静默转成 UTF-8。
///
/// 此前保存一律写 UTF-8：用户的一次普通 `Ctrl+S` 会改变文件编码，
/// 依赖原编码的外部工具（老系统、某些导入器）从此读不了这个文件。
#[test]
fn gbk_file_stays_gbk_after_save() {
    let dir = TempDir::new("doc-gbk-save");
    let repo = FsDocumentRepository::new();
    let target = dir.join("gbk-save.md");
    // "中文测试" 的 GBK 字节。
    let bytes: &[u8] = &[0xD6, 0xD0, 0xCE, 0xC4, 0xB2, 0xE2, 0xCA, 0xD4];
    fs::write(&target, bytes).expect("应能写原始字节");

    let loaded = repo.load(&fp(&target)).expect("GBK 文件应能打开");
    assert_eq!(loaded.content, "中文测试");

    repo.save(&snapshot(&target, &loaded.content))
        .expect("保存应成功");
    let after = fs::read(&target).expect("应能读回");
    // 保存后的字节必须**不是** UTF-8（GBK 中文的双字节序列不合法于 UTF-8），
    // 且能按 GBK 正确解回原文。
    assert!(
        std::str::from_utf8(&after).is_err(),
        "GBK 文件保存后不应变成合法 UTF-8，实际 {after:?}"
    );
    let (text, enc) = rustmark::infrastructure::persistence::encoding::decode(&after);
    assert_eq!(
        enc,
        rustmark::infrastructure::persistence::encoding::Encoding::Gbk
    );
    assert_eq!(text, "中文测试", "保存后的 GBK 字节应能无损解回原文");
}

/// UTF-16 文件保存后仍是 UTF-16（含 BOM），往返无损。
#[test]
fn utf16_file_stays_utf16_after_save() {
    let dir = TempDir::new("doc-utf16-save");
    let repo = FsDocumentRepository::new();
    let target = dir.join("utf16-save.md");

    let text = "记事本另存为 Unicode";
    let mut raw = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        raw.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(&target, &raw).expect("应能写原始字节");

    let loaded = repo.load(&fp(&target)).expect("UTF-16 文件应能打开");
    assert_eq!(loaded.content, text);

    repo.save(&snapshot(&target, &loaded.content))
        .expect("保存应成功");
    let after = fs::read(&target).expect("应能读回");
    assert_eq!(
        &after[..2],
        &[0xFF, 0xFE],
        "UTF-16 LE 文件保存后应保留小端 BOM"
    );
    let (decoded, enc) = rustmark::infrastructure::persistence::encoding::decode(&after);
    assert_eq!(
        enc,
        rustmark::infrastructure::persistence::encoding::Encoding::Utf16Le
    );
    assert_eq!(decoded, text, "保存后的 UTF-16 字节应能无损解回原文");
}

/// GBK 文件加入 GBK 双字节装不下的字符后，保存落 GB18030 四字节
/// （而非整篇转 UTF-8），重新打开仍完整可读。
#[test]
fn gbk_file_with_out_of_gbk_char_saves_as_gb18030_four_byte() {
    let dir = TempDir::new("doc-gb18030-save");
    let repo = FsDocumentRepository::new();
    let target = dir.join("gb18030-save.md");
    // "中文测试" 的 GBK 字节（同上）。
    let bytes: &[u8] = &[0xD6, 0xD0, 0xCE, 0xC4, 0xB2, 0xE2, 0xCA, 0xD4];
    fs::write(&target, bytes).expect("应能写原始字节");

    let mut loaded = repo.load(&fp(&target)).expect("GBK 文件应能打开");
    // 𠀀（U+20000）不在 GBK 双字节表内，需要 GB18030 四字节编码。
    loaded.content.push_str("\n\u{20000}");
    repo.save(&loaded).expect("保存应成功");

    let after = fs::read(&target).expect("应能读回");
    // 保存后的字节仍应按 GBK/GB18030 识别（双字节中文不是合法 UTF-8），
    // 且重新打开时全文无损。
    let (decoded, enc) = rustmark::infrastructure::persistence::encoding::decode(&after);
    assert_eq!(
        enc,
        rustmark::infrastructure::persistence::encoding::Encoding::Gbk
    );
    assert_eq!(
        decoded, loaded.content,
        "四字节扩展字符应被 GB18030 编码并无损往返"
    );
}

/// GBK 文件的中英混排同样要正确（ASCII 区与 UTF-8 兼容，不该被破坏）。
#[test]
fn gbk_file_with_ascii_mixed_is_decoded_correctly() {
    let dir = TempDir::new("doc-gbk-mixed");
    let repo = FsDocumentRepository::new();
    let target = dir.join("mixed.md");
    // "文档 README 说明" 的 GBK 字节。
    let bytes: &[u8] = &[
        0xCE, 0xC4, 0xB5, 0xB5, 0x20, 0x52, 0x45, 0x41, 0x44, 0x4D, 0x45, 0x20, 0xCB, 0xB5, 0xC3,
        0xF7,
    ];
    fs::write(&target, bytes).expect("应能写原始字节");

    let loaded = repo.load(&fp(&target)).expect("混合编码文件应能打开");
    assert_eq!(loaded.content, "文档 README 说明");
}

#[test]
fn save_creates_missing_parent_directories() {
    let dir = TempDir::new("doc-mkdir");
    let repo = FsDocumentRepository::new();
    let nested = dir.join("a/b/c/nested.md");

    repo.save(&snapshot(&nested, "深目录"))
        .expect("应自动创建父目录");
    assert_eq!(repo.load(&fp(&nested)).unwrap().content, "深目录");
}

#[test]
fn explicit_line_ending_overrides_original_style() {
    let dir = TempDir::new("doc-force-lf");
    let repo = FsDocumentRepository::with_line_ending(LineEnding::Lf);
    let target = dir.join("forced.md");
    // 原文件是 CRLF。
    fs::write(&target, b"a\r\nb").expect("应能写原始字节");

    repo.save(&snapshot(&target, "x\ny")).expect("保存应成功");
    let raw = fs::read(&target).expect("应能读回");
    assert_eq!(raw, b"x\ny", "显式配置应覆盖原文件风格");
}

#[test]
fn draft_roundtrips_and_clears() {
    let dir = TempDir::new("doc-draft");
    let repo = FsDocumentRepository::new();
    let drafts = dir.join("drafts");
    let doc_id = DocumentId::new();

    assert_eq!(
        repo.load_draft(&drafts, doc_id).expect("读取应成功"),
        None,
        "尚无草稿时应返回 None 而非报错"
    );

    repo.save_draft(&drafts, doc_id, "未保存的草稿内容", None)
        .expect("写草稿应成功");
    assert_eq!(
        repo.load_draft(&drafts, doc_id).expect("读草稿应成功"),
        Some("未保存的草稿内容".to_string())
    );

    repo.clear_draft(&drafts, doc_id).expect("清草稿应成功");
    assert_eq!(repo.load_draft(&drafts, doc_id).unwrap(), None);
    // 幂等：重复清理不应报错。
    repo.clear_draft(&drafts, doc_id)
        .expect("重复清理应按成功处理");
}

/// 草稿旁车文件（问题 069）：路径随草稿一起落盘、读回、清除。
///
/// 只存内容的草稿恢复后一律是「未命名」——标签条丢失原始文件名。
/// 旁车 `<doc_id>.draft.path` 把文档身份一起存下来，恢复时据此还原。
#[test]
fn draft_path_sidecar_roundtrips_and_clears() {
    let dir = TempDir::new("doc-draft-path");
    let repo = FsDocumentRepository::new();
    let drafts = dir.join("drafts");
    let doc_id = DocumentId::new();
    let path = fp(&dir.join("notes").join("readme.md"));

    // 无旁车时读路径返回 None（未命名文档的草稿）。
    assert_eq!(repo.load_draft_path(&drafts, doc_id).unwrap(), None);

    // 带路径写草稿：内容与旁车双双落盘。
    repo.save_draft(&drafts, doc_id, "草稿正文", Some(&path))
        .expect("写草稿应成功");
    assert_eq!(
        repo.load_draft_path(&drafts, doc_id).expect("读旁车应成功"),
        Some(path.clone()),
        "旁车应记下文档路径，恢复后标签条才能显示原始文件名"
    );
    // 内容不受旁车影响，仍原样读回。
    assert_eq!(
        repo.load_draft(&drafts, doc_id).expect("读草稿应成功"),
        Some("草稿正文".to_string())
    );

    // 清草稿应连旁车一起消失（否则磁盘上留下孤儿文件）。
    repo.clear_draft(&drafts, doc_id).expect("清草稿应成功");
    assert_eq!(repo.load_draft_path(&drafts, doc_id).unwrap(), None);
    assert_eq!(repo.load_draft(&drafts, doc_id).unwrap(), None);
}

#[test]
fn overwriting_existing_file_replaces_content_entirely() {
    let dir = TempDir::new("doc-overwrite");
    let repo = FsDocumentRepository::new();
    let target = dir.join("shrink.md");

    repo.save(&snapshot(&target, "很长的原始内容 abcdefghijklmnop"))
        .expect("首次保存应成功");
    // 写入更短的内容：不能残留旧内容的尾部。
    repo.save(&snapshot(&target, "短")).expect("二次保存应成功");

    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "短",
        "覆盖写必须完全替换旧内容"
    );
}
