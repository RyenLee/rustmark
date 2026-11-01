//! `asset_loader` 的内联单测（评审 P2 #6 / #7）。
//!
//! 这里只覆盖本次加固的两条**内容校验**：
//! - `ensure_within_base_real`：符号链接越界（#6）；
//! - `bytes_match_declared_mime`：远程字节头与声明 MIME 同族（#7）。
//!
//! 三种来源的常规成功 / 拒绝路径由 `tests/infrastructure/asset_loader_test.rs`
//! 从外部（`rustmark::…` 路径）覆盖，两边刻意不重复。

use super::policy::{MAGIC_WINDOW, bytes_match_declared_mime};
use super::{AssetError, AssetLoader};

/// 一个 1x1 的合法 PNG（本地路径用例用）。
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

// ---------------------------------------------------------------------------
// #7 魔数族匹配（纯函数，与 feature 无关）
// ---------------------------------------------------------------------------

/// PNG：`89 50 4E 47 0D 0A 1A 0A` + 首个 chunk 长度。
const PNG_HEAD: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
];
/// JPEG：`FF D8 FF` + APP0 段起始。
const JPEG_HEAD: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01,
];
/// GIF89a + 逻辑屏幕描述符前几字节。
const GIF_HEAD: &[u8] = b"GIF89a\x01\x00\x01\x00\x00\x00";
/// WebP：`RIFF` + 长度 + `WEBP`。
const WEBP_HEAD: &[u8] = b"RIFF\x24\x00\x00\x00WEBP";
/// BMP：`BM` + 文件头字段（总长 ≥ 14）。
const BMP_HEAD: &[u8] = b"BM\x36\x00\x00\x00\x00\x00\x00\x00\x36\x00\x00\x00";

#[test]
fn all_five_signatures_are_recognized() {
    let cases: [(&str, &[u8]); 5] = [
        ("image/png", PNG_HEAD),
        ("image/jpeg", JPEG_HEAD),
        ("image/gif", GIF_HEAD),
        ("image/webp", WEBP_HEAD),
        ("image/bmp", BMP_HEAD),
    ];
    for (mime, head) in cases {
        assert!(
            head.len() >= MAGIC_WINDOW,
            "{mime} 的样本至少要覆盖魔数窗口，否则用例本身无效"
        );
        assert!(
            bytes_match_declared_mime(mime, head),
            "{mime} 的签名应被认出"
        );
    }
}

/// `image/jpg` 与 `image/jpeg` 同族（前者是非标准但常见的写法）。
#[test]
fn jpg_alias_shares_the_jpeg_family() {
    assert!(bytes_match_declared_mime("image/jpg", JPEG_HEAD));
    assert!(bytes_match_declared_mime("image/jpeg", JPEG_HEAD));
}

/// 声明 png、字节是 jpeg：必须拒（这正是伪装响应 / 错误页的形态）。
#[test]
fn mismatched_family_is_rejected() {
    assert!(!bytes_match_declared_mime("image/png", JPEG_HEAD));
    assert!(!bytes_match_declared_mime("image/jpeg", PNG_HEAD));
    assert!(!bytes_match_declared_mime("image/webp", GIF_HEAD));
    assert!(!bytes_match_declared_mime("image/bmp", WEBP_HEAD));
    // HTML 错误页伪装成 png（最常见的一种）。
    assert!(!bytes_match_declared_mime(
        "image/png",
        b"<!DOCTYPE html><html>"
    ));
}

/// 白名单之外的类型一律不认——尤其是 `image/svg+xml`（它可携带脚本）。
#[test]
fn unknown_or_forbidden_mime_never_matches() {
    assert!(!bytes_match_declared_mime("image/svg+xml", PNG_HEAD));
    assert!(!bytes_match_declared_mime("text/html", PNG_HEAD));
    assert!(!bytes_match_declared_mime("", PNG_HEAD));
}

/// 截断响应不得靠「前缀凑巧对上」蒙混：不足窗口宽度即判否，且不 panic。
#[test]
fn truncated_head_never_matches() {
    for head in [PNG_HEAD, JPEG_HEAD, GIF_HEAD, WEBP_HEAD, BMP_HEAD] {
        let truncated = &head[..MAGIC_WINDOW - 1];
        for mime in [
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/webp",
            "image/bmp",
        ] {
            assert!(
                !bytes_match_declared_mime(mime, truncated),
                "{mime} 不应因截断前缀而通过"
            );
        }
    }
    assert!(!bytes_match_declared_mime("image/png", b""));
    assert!(!bytes_match_declared_mime("image/png", &PNG_HEAD[..3]));
}

// ---------------------------------------------------------------------------
// #6 真实路径校验（符号链接越界）
// ---------------------------------------------------------------------------

/// 正常路径：文档目录内的文件经 canonicalize 后仍在子树内，必须能读。
#[test]
fn file_inside_base_still_loads_after_real_path_check() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    std::fs::write(dir.path().join("a.png"), TINY_PNG).expect("应能写入");

    let asset = AssetLoader::new()
        .load("a.png", Some(dir.path()))
        .expect("目录内文件必须仍可加载");
    assert_eq!(asset.bytes, TINY_PNG);
}

/// 子目录内的文件同样可读（真实路径校验不能把合法嵌套路径误伤）。
#[test]
fn nested_file_inside_base_still_loads() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let sub = dir.path().join("images");
    std::fs::create_dir_all(&sub).expect("应能建子目录");
    std::fs::write(sub.join("b.png"), TINY_PNG).expect("应能写入");

    let asset = AssetLoader::new()
        .load("images/b.png", Some(dir.path()))
        .expect("子目录内的图片应可加载");
    assert_eq!(asset.bytes, TINY_PNG);
}

/// 文件不存在时仍报 `ReadFailed`（而不是含糊的「路径非法」）：
/// 真实路径校验对 `NotFound` 只退一步校验父目录，不能改变既有的错误语义。
#[test]
fn missing_file_still_reports_read_failure() {
    let dir = tempfile::tempdir().expect("应能建临时目录");
    let error = AssetLoader::new()
        .load("nope.png", Some(dir.path()))
        .expect_err("不存在的文件应报错");
    assert!(
        matches!(error, AssetError::ReadFailed { .. }),
        "应保持读取失败语义，实际：{error:?}"
    );
}

/// **本项加固的核心用例**：文档目录内的符号链接指向目录外文件时必须被拒。
///
/// 仅 `#[cfg(unix)]`：Windows 创建符号链接需要管理员 / 开发者模式权限，
/// 在 CI 上不可靠。Windows 侧的 canonicalize 正常路径由上面两条用例覆盖。
#[cfg(unix)]
#[test]
fn symlink_escaping_base_is_rejected() {
    let root = tempfile::tempdir().expect("应能建临时目录");
    let outside = root.path().join("outside.png");
    std::fs::write(&outside, TINY_PNG).expect("应能写入目录外文件");

    let doc_dir = root.path().join("docs");
    std::fs::create_dir_all(&doc_dir).expect("应能建文档目录");
    std::os::unix::fs::symlink(&outside, doc_dir.join("evil.png")).expect("应能建符号链接");

    let error = AssetLoader::new()
        .load("evil.png", Some(&doc_dir))
        .expect_err("指向目录外的符号链接必须被拒");
    assert!(
        matches!(error, AssetError::UnresolvedPath { .. }),
        "应为路径越界错误，实际：{error:?}"
    );
}

/// 指向目录外的**目录**符号链接同样要拦（`docs/linkdir/x.png` 这种形态）。
#[cfg(unix)]
#[test]
fn symlinked_directory_escaping_base_is_rejected() {
    let root = tempfile::tempdir().expect("应能建临时目录");
    let outside_dir = root.path().join("secrets");
    std::fs::create_dir_all(&outside_dir).expect("应能建目录");
    std::fs::write(outside_dir.join("x.png"), TINY_PNG).expect("应能写入");

    let doc_dir = root.path().join("docs");
    std::fs::create_dir_all(&doc_dir).expect("应能建文档目录");
    std::os::unix::fs::symlink(&outside_dir, doc_dir.join("linkdir")).expect("应能建符号链接");

    let error = AssetLoader::new()
        .load("linkdir/x.png", Some(&doc_dir))
        .expect_err("经目录符号链接越界必须被拒");
    assert!(
        matches!(error, AssetError::UnresolvedPath { .. }),
        "应为路径越界错误，实际：{error:?}"
    );
}

/// 链接指向 base **内部**时不得误伤（否则用户正常的 `ln -s` 布局会被打断）。
#[cfg(unix)]
#[test]
fn symlink_staying_inside_base_is_allowed() {
    let doc_dir = tempfile::tempdir().expect("应能建临时目录");
    std::fs::write(doc_dir.path().join("real.png"), TINY_PNG).expect("应能写入");
    std::os::unix::fs::symlink(
        doc_dir.path().join("real.png"),
        doc_dir.path().join("alias.png"),
    )
    .expect("应能建符号链接");

    let asset = AssetLoader::new()
        .load("alias.png", Some(doc_dir.path()))
        .expect("指向目录内的链接应可加载");
    assert_eq!(asset.bytes, TINY_PNG);
}

// ---------------------------------------------------------------------------
// #7 远程路径的行为级用例（需 remote-images；本地起 HTTP 服务，不依赖外网）
// ---------------------------------------------------------------------------

/// 起一个只服务一次的极简 HTTP 服务，返回端口（无法监听时返回 `None`）。
#[cfg(feature = "remote-images")]
fn serve_once(content_type: &'static str, body: Vec<u8>) -> Option<u16> {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            let header = format!(
                "HTTP/1.0 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    Some(port)
}

/// 声明 `image/png`、实际返回 JPEG 字节 → 必须拒（伪装响应）。
#[cfg(feature = "remote-images")]
#[test]
fn remote_body_of_wrong_family_is_rejected() {
    let Some(port) = serve_once("image/png", JPEG_HEAD.to_vec()) else {
        return; // 沙箱禁止监听端口：跳过而非失败
    };
    let error = AssetLoader::new()
        .load(&format!("http://127.0.0.1:{port}/a.png"), None)
        .expect_err("字节头与声明不符必须拒绝");
    assert!(
        matches!(error, AssetError::DisallowedMime { .. }),
        "应为类型不符错误，实际：{error:?}"
    );
}

/// 声明与字节同族 → 正常放行（避免把正常图片一起拦掉）。
#[cfg(feature = "remote-images")]
#[test]
fn remote_body_of_matching_family_is_accepted() {
    let Some(port) = serve_once("image/png", TINY_PNG.to_vec()) else {
        return;
    };
    let asset = AssetLoader::new()
        .load(&format!("http://127.0.0.1:{port}/a.png"), None)
        .expect("字节头匹配时应正常加载");
    assert_eq!(asset.mime, "image/png");
    assert_eq!(asset.bytes, TINY_PNG);
}

/// 截断响应（不足魔数窗口）→ 拒绝且不 panic。
#[cfg(feature = "remote-images")]
#[test]
fn truncated_remote_body_is_rejected_without_panic() {
    let Some(port) = serve_once("image/png", PNG_HEAD[..3].to_vec()) else {
        return;
    };
    let error = AssetLoader::new()
        .load(&format!("http://127.0.0.1:{port}/a.png"), None)
        .expect_err("截断响应必须拒绝");
    assert!(
        matches!(error, AssetError::DownloadFailed { .. }),
        "截断应按下载失败处理，实际：{error:?}"
    );
}

/// 空响应体 → 仍是「响应体为空」，不 panic。
#[cfg(feature = "remote-images")]
#[test]
fn empty_remote_body_is_rejected_without_panic() {
    let Some(port) = serve_once("image/png", Vec::new()) else {
        return;
    };
    let result = AssetLoader::new().load(&format!("http://127.0.0.1:{port}/a.png"), None);
    assert!(
        matches!(result, Err(AssetError::DownloadFailed { .. })),
        "空响应应按下载失败处理，实际：{result:?}"
    );
}
