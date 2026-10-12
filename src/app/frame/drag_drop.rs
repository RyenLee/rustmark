//! 拖放分类的纯函数（不 IO、不依赖 EditorApp）。

/// 拖放打开（P2）：markdown 编辑器只接受文本类文件。
///
/// 允许 `.md` / `.markdown`（markdown 家族）与 `.txt`（纯文本，渲染为近乎原文）。
/// 其余扩展名（图片、二进制、PDF 等）一律拒绝——避免把非文本塞进编辑缓冲。
pub(crate) fn is_openable_text(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown") || lower.ends_with(".txt")
}

/// 把一批拖入的路径分成「可打开」与「应拒绝」两组（纯函数，便于单测）。
///
/// 不在此处做任何 IO：是否真的能打开由 [`EditorApp::open_path`] 决定，
/// 这里只按扩展名做**意图分类**。
pub(crate) fn classify_dropped(paths: &[String]) -> (Vec<String>, Vec<String>) {
    let mut open = Vec::new();
    let mut reject = Vec::new();
    for p in paths {
        if is_openable_text(p) {
            open.push(p.clone());
        } else {
            reject.push(p.clone());
        }
    }
    (open, reject)
}
