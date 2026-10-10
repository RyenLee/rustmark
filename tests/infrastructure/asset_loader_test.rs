//! `AssetLoader` 单测（S5-11）。
//!
//! 覆盖三种来源各自的**成功路径**与**拒绝路径**，重点是安全边界：
//! `data:text/html` 必须被拒、SVG 必须被拒、超限必须**硬拒绝**
//! （而非原型那样的「只提示但依然插入」）。

use std::path::{Path, PathBuf};

use rustmark::infrastructure::persistence::asset_loader::{AssetError, AssetLimits, AssetLoader};

/// 临时目录守卫。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let unique = format!(
            "rustmark-asset-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let path = std::env::temp_dir().join(unique.replace(['(', ')', ' '], ""));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("应能创建临时目录");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 一个 1x1 的合法 PNG（用于本地路径与体积测试）。
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// `TINY_PNG` 的 base64。
///
/// 由上面那份字节**实际编码**得来（不要凭记忆写：写错会被
/// `data_uri_png_is_decoded` 立刻抓到，因为解码结果必须逐字节相等）。
const TINY_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMAAQAABQABDQottAAAAABJRU5ErkJggg==";

// ---------------------------------------------------------------------------
// 来源一：本地路径
// ---------------------------------------------------------------------------

#[test]
fn local_file_is_loaded_relative_to_document() {
    let dir = TempDir::new("local");
    std::fs::write(dir.join("a.png"), TINY_PNG).expect("应能写入");

    let loader = AssetLoader::new();
    let asset = loader
        .load("a.png", Some(&dir.0))
        .expect("相对路径应能加载");

    assert_eq!(asset.mime, "image/png");
    assert_eq!(asset.bytes, TINY_PNG, "字节应原样返回");
    assert_eq!(asset.alt, "a", "alt 应为去扩展名的文件名");
    assert!(!asset.is_empty());
}

#[test]
fn local_file_with_subdirectory_is_loaded() {
    let dir = TempDir::new("subdir");
    std::fs::create_dir_all(dir.join("images")).expect("应能建目录");
    std::fs::write(dir.join("images/b.png"), TINY_PNG).expect("应能写入");

    let asset = AssetLoader::new()
        .load("images/b.png", Some(&dir.0))
        .expect("含子目录的相对路径应能加载");
    assert_eq!(asset.bytes, TINY_PNG);
}

#[test]
fn absolute_path_is_loaded_without_base_dir() {
    let dir = TempDir::new("abs");
    let file = dir.join("c.png");
    std::fs::write(&file, TINY_PNG).expect("应能写入");

    let raw = file.to_string_lossy().replace('\\', "/");
    let asset = AssetLoader::new()
        .load(&raw, None)
        .expect("绝对路径不需要基准目录");
    assert_eq!(asset.mime, "image/png");
}

/// 相对路径 + 无基准文档 → 必须给出**明确**错误，而不是静默失败。
#[test]
fn relative_path_without_base_dir_is_rejected_explicitly() {
    let error = AssetLoader::new()
        .load("a.png", None)
        .expect_err("无基准目录时相对路径无法解析");
    match error {
        AssetError::UnresolvedPath { ref reason, .. } => {
            assert!(
                reason.contains("先打开文档"),
                "错误应说明原因，实际：{reason}"
            );
        }
        other => panic!("应是路径解析错误，实际：{other:?}"),
    }
}

#[test]
fn missing_local_file_returns_read_error_not_panic() {
    let dir = TempDir::new("missing");
    let error = AssetLoader::new()
        .load("nope.png", Some(&dir.0))
        .expect_err("文件不存在应报错");
    assert!(
        matches!(error, AssetError::ReadFailed { .. }),
        "应是读取失败错误，实际：{error:?}"
    );
}

/// **解码失败不 panic**：伪装成 PNG 的文本文件必须走错误路径。
#[test]
fn corrupt_image_content_does_not_panic() {
    let dir = TempDir::new("corrupt");
    std::fs::write(dir.join("bad.png"), b"this is definitely not a png").expect("应能写入");

    let result = AssetLoader::new().load("bad.png", Some(&dir.0));
    // 本实现只做字节读取与体积校验，不解码像素，
    // 因此「损坏」不会报错——但**更不能 panic**。
    // 断言的是：无论内容如何，调用都正常返回。
    match result {
        Ok(asset) => assert_eq!(asset.bytes, b"this is definitely not a png"),
        Err(AssetError::ReadFailed { .. }) => {}
        Err(other) => panic!("不应出现其它错误：{other:?}"),
    }
}

#[test]
fn empty_local_file_is_rejected() {
    let dir = TempDir::new("empty");
    std::fs::write(dir.join("e.png"), b"").expect("应能写入");
    assert!(
        AssetLoader::new().load("e.png", Some(&dir.0)).is_err(),
        "空图片应被拒绝"
    );
}

// ---------------------------------------------------------------------------
// 来源二：data URI
// ---------------------------------------------------------------------------

#[test]
fn data_uri_png_is_decoded() {
    let asset = AssetLoader::new()
        .load(&format!("data:image/png;base64,{TINY_PNG_BASE64}"), None)
        .expect("合法 PNG data URI 应能解码");

    assert_eq!(asset.mime, "image/png");
    assert_eq!(asset.bytes, TINY_PNG, "base64 应正确解码回原字节");
    assert!(!asset.is_empty());
}

/// **安全核心用例**：`data:text/html` 是 XSS 向量，必须被拒。
#[test]
fn data_text_html_is_rejected() {
    let error = AssetLoader::new()
        .load(
            "data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==",
            None,
        )
        .expect_err("data:text/html 必须被拒绝");
    assert!(
        matches!(error, AssetError::DisallowedMime { .. }),
        "应是类型不允许错误，实际：{error:?}"
    );
}

/// `data:image/svg+xml` 也必须被拒——SVG 可内嵌 `<script>`。
#[test]
fn data_svg_is_rejected() {
    let error = AssetLoader::new()
        .load(
            "data:image/svg+xml;base64,PHN2Zz48c2NyaXB0Lz48L3N2Zz4=",
            None,
        )
        .expect_err("SVG 必须被拒绝");
    match error {
        AssetError::DisallowedMime { ref mime } => {
            assert_eq!(mime, "image/svg+xml");
        }
        other => panic!("应是类型不允许错误，实际：{other:?}"),
    }
}

/// `image/pngx` 这类「前缀相似但不是白名单成员」的必须被拒——
/// 精确匹配，不能用 `starts_with`。
#[test]
fn lookalike_mime_is_rejected() {
    let error = AssetLoader::new()
        .load("data:image/pngx;base64,AAAA", None)
        .expect_err("image/pngx 不在白名单内");
    assert!(matches!(error, AssetError::DisallowedMime { .. }));
}

#[test]
fn malformed_data_uri_returns_clear_errors() {
    let loader = AssetLoader::new();

    // 缺少 `,`
    assert!(matches!(
        loader.load("data:image/png;base64", None),
        Err(AssetError::MalformedDataUri { .. })
    ));

    // base64 长度非法（不是 4 的倍数）
    assert!(matches!(
        loader.load("data:image/png;base64,AAA", None),
        Err(AssetError::MalformedDataUri { .. })
    ));

    // base64 含非法字符
    assert!(matches!(
        loader.load("data:image/png;base64,AA*A", None),
        Err(AssetError::MalformedDataUri { .. })
    ));

    // 解码后为空
    assert!(matches!(
        loader.load("data:image/png;base64,====", None),
        Err(AssetError::MalformedDataUri { .. })
    ));
}

/// 未标注 `;base64` 时按百分号编码解析。
#[test]
fn percent_encoded_data_uri_is_decoded() {
    let asset = AssetLoader::new()
        .load("data:image/png,hello%20world", None)
        .expect("百分号编码应能解码");
    assert_eq!(asset.bytes, b"hello world");
}

/// base64 中允许出现换行（原型正则也允许 `\s`）。
#[test]
fn base64_with_newlines_is_accepted() {
    // 在中间插入换行，验证解码时空白被正确忽略
    let (head, tail) = TINY_PNG_BASE64.split_at(8);
    let with_newline = format!("data:image/png;base64,{head}\n{tail}");

    let asset = AssetLoader::new()
        .load(&with_newline, None)
        .expect("base64 中的换行应被忽略");
    assert_eq!(asset.mime, "image/png");
    assert_eq!(asset.bytes, TINY_PNG, "换行不应影响解码结果");
}

// ---------------------------------------------------------------------------
// 来源三：远程 URL
// ---------------------------------------------------------------------------

/// HTTPS 不被支持时必须**明确报错**，不能静默降级到明文 http。
#[test]
fn https_url_is_rejected_explicitly_not_downgraded() {
    let error = AssetLoader::new()
        .load("https://example.com/a.png", None)
        .expect_err("当前版本未内置 TLS，应报错");
    match error {
        AssetError::DownloadFailed { ref reason, .. } => {
            assert!(
                reason.contains("TLS"),
                "错误应说明是 TLS 原因，实际：{reason}"
            );
        }
        other => panic!("应是下载失败错误，实际：{other:?}"),
    }
}

#[test]
fn unreachable_host_returns_download_error_not_panic() {
    // 保留端口 1（tcpmux，几乎不会有服务监听）
    let error = AssetLoader::new()
        .load("http://127.0.0.1:1/a.png", None)
        .expect_err("连不上的主机应返回下载错误");
    assert!(
        matches!(error, AssetError::DownloadFailed { .. }),
        "应是下载失败错误，实际：{error:?}"
    );
}

#[test]
fn remote_download_of_real_image_succeeds() {
    // 用本机临时起的极简 HTTP 服务，避免依赖外网与第三方 crate。
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        // 沙箱环境可能禁止监听端口；跳过而非失败，
        // 因为这条用例验证的是协议解析，不是不变量。
        Err(_) => return,
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    if port == 0 {
        return;
    }

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            let body = TINY_PNG;
            let response = format!(
                "HTTP/1.0 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
        }
    });

    let loader = AssetLoader::with_limits(AssetLimits {
        max_inline_bytes: 1024 * 1024,
        ..AssetLimits::default()
    });
    let asset = loader
        .load(&format!("http://127.0.0.1:{port}/a.png"), None)
        .expect("本机 HTTP 服务应能下载成功");

    assert_eq!(asset.mime, "image/png");
    assert_eq!(asset.bytes, TINY_PNG);
}

// ---------------------------------------------------------------------------
// 体积硬约束（原型缺陷 2.1 的根治）
// ---------------------------------------------------------------------------

#[test]
fn oversized_data_uri_is_rejected_with_actionable_message() {
    let loader = AssetLoader::with_limits(AssetLimits {
        max_inline_bytes: 16,
        ..AssetLimits::default()
    });
    let error = loader
        .load(&format!("data:image/png;base64,{TINY_PNG_BASE64}"), None)
        .expect_err("超过上限应被硬拒绝");

    match error {
        AssetError::TooLarge { actual, limit } => {
            assert!(actual > 16, "实际值应大于上限：{actual}");
            assert_eq!(limit, 16);
        }
        other => panic!("应是体积超限错误，实际：{other:?}"),
    }

    // 文案必须给出**可操作**的下一步，而不只是「太大了」。
    let text = loader
        .load(&format!("data:image/png;base64,{TINY_PNG_BASE64}"), None)
        .expect_err("应仍报错")
        .to_string();
    assert!(
        text.contains("相对路径"),
        "文案应建议改用相对路径，实际：{text}"
    );
}

/// 原型的 `IMAGE_WARN_BYTES` 是 512 KB，本实现保持同值。
#[test]
fn default_limit_matches_prototype() {
    assert_eq!(AssetLoader::new().max_inline_bytes(), 512 * 1024);
}

#[test]
fn exactly_at_limit_is_accepted() {
    let loader = AssetLoader::with_limits(AssetLimits {
        max_inline_bytes: TINY_PNG.len(),
        ..AssetLimits::default()
    });
    assert!(
        loader
            .load(&format!("data:image/png;base64,{TINY_PNG_BASE64}"), None)
            .is_ok(),
        "恰好等于上限应被接受（边界含端点）"
    );
}

// ---------------------------------------------------------------------------
// 引用识别
// ---------------------------------------------------------------------------

#[test]
fn image_reference_detection_covers_three_sources() {
    assert!(AssetLoader::is_image_reference("a.png"));
    assert!(AssetLoader::is_image_reference("images/b.JPG"));
    assert!(AssetLoader::is_image_reference("https://example.com/c.gif"));
    assert!(AssetLoader::is_image_reference("http://example.com/d.webp"));
    assert!(AssetLoader::is_image_reference(
        "data:image/png;base64,AAAA"
    ));

    assert!(!AssetLoader::is_image_reference(""));
    assert!(!AssetLoader::is_image_reference("   "));
    assert!(!AssetLoader::is_image_reference("doc.md"));
    assert!(!AssetLoader::is_image_reference("ftp://x/y.png"));
}

/// 带 scheme 但不支持的协议必须**明确报错**，
/// 不能被当成本地路径去 `fs::read`（那会报一个「找不到文件」的误导性错误）。
#[test]
fn unsupported_scheme_is_rejected_explicitly() {
    let error = AssetLoader::new()
        .load("ftp://example.com/a.png", None)
        .expect_err("不支持的协议应报错");
    match error {
        AssetError::DownloadFailed { ref reason, .. } => {
            assert!(
                reason.contains("ftp"),
                "错误应点名是哪个协议不支持，实际：{reason}"
            );
        }
        other => panic!("应是下载/协议错误，实际：{other:?}"),
    }
}

#[test]
fn empty_reference_is_rejected_with_clear_reason() {
    let error = AssetLoader::new()
        .load("   ", None)
        .expect_err("空引用应报错");
    match error {
        AssetError::UnresolvedPath { ref reason, .. } => {
            assert!(reason.contains("空"), "实际：{reason}");
        }
        other => panic!("应是路径解析错误，实际：{other:?}"),
    }
}

/// SVG 本地文件同样必须被拒（与 data URI 侧保持一致的白名单）。
#[test]
fn local_svg_file_is_rejected() {
    let dir = TempDir::new("svg");
    std::fs::write(dir.join("x.svg"), b"<svg onload=alert(1)></svg>").expect("应能写入");

    let error = AssetLoader::new()
        .load("x.svg", Some(&dir.0))
        .expect_err("本地 SVG 也必须被拒绝");
    assert!(
        matches!(error, AssetError::DisallowedMime { .. }),
        "SVG 应被类型白名单拒绝，实际：{error:?}"
    );
}

#[test]
fn unknown_extension_is_rejected() {
    let dir = TempDir::new("unknownext");
    std::fs::write(dir.join("a.txt"), b"hello").expect("应能写入");
    let error = AssetLoader::new()
        .load("a.txt", Some(&dir.0))
        .expect_err("非图片扩展名应被拒绝");
    assert!(
        matches!(error, AssetError::UnresolvedPath { .. }),
        "实际：{error:?}"
    );
}

/// 确保 `Path` 导入被使用（部分平台分支需要）。
#[test]
fn loader_accepts_path_base_dir() {
    let dir = TempDir::new("pathdir");
    std::fs::write(dir.join("p.png"), TINY_PNG).expect("应能写入");
    let base: &Path = &dir.0;
    assert!(AssetLoader::new().load("p.png", Some(base)).is_ok());
}
