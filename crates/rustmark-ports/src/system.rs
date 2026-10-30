//! 外部打开器端口与链接安全校验（rustmark-ports，R-10 项 A 从 shared 抽出，零外部依赖）。
//!
//! egui 在本项目编译时未启用 `open_url` feature（见 `Cargo.toml` 的
//! `egui = { version = "0.37.0", default-features = false }`），且项目对运行时依赖一贯
//! 严控（数学 / 图表均自研），因此不引入 `webbrowser` crate，而是用
//! 操作系统自带的默认程序打开——零额外依赖，桌面端行为与原生应用一致。
//!
//! # 为什么执行端不在这里
//!
//! `rustmark-ports` 是**零 IO** 的共享内核（R-03 起 `std::process` 亦进
//! [`SHARED_FORBIDDEN`]，见 `docs/spec/architecture-constraints.md`）。早先
//! 「`std::process` 不在禁止清单，故在此落位」是**绕规则落位**——文件头
//! 自认绕过，与「零外部依赖」的自我描述矛盾。现按 `DialogPort` / `Clock`
//! 的既有范式拆开：
//!
//! - **端口**（[`ExternalOpener`] trait）留在这里——表现层（预览区）需要
//!   这个能力，而架构约束禁止 `presentation` 依赖 `crate::infrastructure`；
//!   `rustmark-ports` 可被任意层依赖，端口本身零 IO；
//! - **实现**（三平台 `Command::new(...).spawn()`）移到
//!   `infrastructure::system::SystemExternalOpener`；
//! - **链接安全校验**（[`is_safe_to_open`]，纯逻辑）**必须留在这里**——
//!   若随执行一起搬走，`rustmark-ports` 会重新含 IO，且校验逻辑会被拆到
//!   infrastructure 侧，表现层再想复用就得反向依赖 infrastructure。

/// 用系统默认程序打开一个外部地址 / 本地文件。
///
/// 行为对齐「在浏览器中打开链接」：http(s) / mailto 交浏览器 / 邮件客户端，
/// 本地路径交文件管理器 / 关联程序。实现应 `spawn` 而非 `wait`——不阻塞 UI 帧。
///
/// 失败只返回 `Err`，不 panic——打开器不可用（或平台不支持）不应拖垮编辑器。
///
/// # 为什么这是个 trait 而不是自由函数
///
/// 预览区点击外链是**用户可见**行为，而真起进程的调用在测试里无法断言
/// （`spawn` 一个真 `cmd` 既不幂等也没法观察）。抽成端口后，测试可注入
/// 假实现，断言「点了链接、参数正确、且**只在通过安全校验后**才调」。
///
/// 与 [`crate::dialog::DialogPort`] / [`crate::time::Clock`]
/// 同一范式：端口在 `rustmark-ports`，实现在 `infrastructure`。
pub trait ExternalOpener: Send + Sync {
    /// 打开 `target`（URL 或本地路径）。
    fn open(&self, target: &str) -> std::io::Result<()>;
}

/// 外链打开目标的分类结果（纯逻辑，零 IO，可被任意层依赖）。
///
/// 「能不能打开」与「该怎么打开」是两件事：前者是安全判断，后者是平台行为。
/// 把两者收在一处（本 crate），实现层只做动作分派，避免出现
/// 「校验在表现层、执行在基础设施、两边口径不一致」的分叉。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenTarget {
    /// 文档内锚点，调用方自行跳转（打开器应视为 no-op）。
    Anchor,
    /// http / https / mailto / tel，交系统协议处理器。
    Remote,
    /// 本地文本 / 位图，可交系统关联程序打开（不会执行代码）。
    LocalDocument,
    /// 本地但不该执行（可执行文件 / 脚本 / HTML / PDF 等）：
    /// 仅允许「在文件管理器中定位」。
    RevealOnly,
    /// 明确不安全（XSS scheme、UNC、协议相对、控制字符、编码绕过、空串）。
    Reject,
}

/// 可安全交给关联程序打开的本地扩展名（**不含**可执行 / 脚本 / HTML / PDF）。
///
/// 这些格式只能被「读取」，不会被当作程序执行；HTML / PDF 之所以不在列，
/// 是因为它们的渲染器会执行脚本，与浏览器打开网页同属一个风险面。
const OPENABLE_EXTENSIONS: &[&str] = &[
    "md", "markdown", "txt", "png", "jpg", "jpeg", "gif", "webp", "bmp",
];

/// 识别形如 `scheme:` 的前缀；Windows 盘符 `C:` 不算 scheme。
fn scheme_of(s: &str) -> Option<&str> {
    let colon = s.find(':')?;
    let scheme = &s[..colon];
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
    {
        return None;
    }
    // 单字母：Windows 盘符（`C:\x` / `C:/x` / `c:x`），按本地路径处理。
    if scheme.len() == 1 {
        return None;
    }
    Some(scheme)
}

/// 取本地串的扩展名（`file://` 先剥前缀）。
fn local_extension(target: &str) -> &str {
    let path = target.strip_prefix("file://").unwrap_or(target);
    match path.rsplit_once('.') {
        Some((_, ext)) => ext,
        None => "",
    }
}

/// 该目标应如何打开（纯函数，单测唯一入口）。
///
/// 判定顺序即优先级：空 / 控制字符 → 锚点 → UNC 与协议相对 → XSS scheme →
/// 协议白名单 → 本地路径。
pub fn classify_open_target(raw: &str) -> OpenTarget {
    let t = raw.trim();
    if t.is_empty() {
        return OpenTarget::Reject;
    }
    // 控制字符（含 NUL）：命令行与文件名都不该出现。
    if t.chars().any(|c| c.is_control()) {
        return OpenTarget::Reject;
    }
    if t.starts_with('#') {
        return OpenTarget::Anchor;
    }
    // 协议相对 `//host` 与 UNC `\\host`：Windows 下会触发 SMB 访问
    // （凭据外泄 + 远程内容加载），一律拒绝。
    if t.starts_with("//") || t.starts_with(r"\\") {
        return OpenTarget::Reject;
    }
    // XSS 向量：显式拦截，防 `://` 分支漏过（`javascript:` 通常不含 `://`）。
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("data:")
    {
        return OpenTarget::Reject;
    }

    match scheme_of(t) {
        // 绝对协议：仅白名单（大小写不敏感，修掉 `HTTPS://` 被误拒的旧缺陷）。
        Some(scheme) => match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" | "mailto" | "tel" => OpenTarget::Remote,
            "file" => classify_local(t),
            _ => OpenTarget::Reject,
        },
        // 无 scheme：相对 / 绝对本地路径。
        None => classify_local(t),
    }
}

fn classify_local(target: &str) -> OpenTarget {
    let path = target.strip_prefix("file://").unwrap_or(target);
    // 本地路径里的百分号 / 查询串 / 锚点：`x%2Eexe` 这类编码可绕过扩展名判定。
    if path.contains('%') || path.contains('?') || path.contains('#') {
        return OpenTarget::Reject;
    }
    let ext = local_extension(target).to_ascii_lowercase();
    if OPENABLE_EXTENSIONS.contains(&ext.as_str()) {
        OpenTarget::LocalDocument
    } else {
        // 可执行 / 脚本 / 归档 / HTML / PDF / 无扩展名：只定位，不执行。
        OpenTarget::RevealOnly
    }
}

/// 该链接目标是否可安全打开（拒绝 XSS 向量）。
///
/// 这是 [`classify_open_target`] 的薄门面：`true` 只表示「不是明确危险输入」，
/// 具体动作由实现层按分类决定。因此 `RevealOnly`（本地 exe / html / pdf）
/// 也返回 `true`——它们不会被**执行**，只会被文件管理器定位。
///
/// 与 `export::html_sanitize::is_safe_url` 的取向一致但面向「点击跳转」：
/// 绝对协议只放 `http/https/mailto/tel`；相对路径与 `file://` 视为本地文件；
/// `#锚点`（内部跳转）在调用方单独处理，这里返回 `true` 以免误杀。
pub fn is_safe_to_open(url: &str) -> bool {
    !matches!(classify_open_target(url), OpenTarget::Reject)
}

#[cfg(test)]
mod tests {
    use super::is_safe_to_open;

    /// 端口必须能装进 `Arc<dyn ExternalOpener>` 并跨线程传递（`Services`
    /// 持有它，对话框端口同为 `Send + Sync`）。这条是**编译期**约束：
    /// 若哪天有人给 trait 去掉 `Send + Sync`，实现侧 `Arc<dyn …>` 会编译失败。
    #[test]
    fn opener_port_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn super::ExternalOpener>();
    }

    #[test]
    fn rejects_xss_protocols() {
        // 这些 scheme 不含 `://`，必须有单独拦截，否则会被「相对路径」分支放过。
        assert!(!is_safe_to_open("javascript:alert(1)"));
        assert!(!is_safe_to_open("javascript://evil.com"));
        assert!(!is_safe_to_open("vbscript:msgbox"));
        assert!(!is_safe_to_open("data:text/html,<script>"));
        assert!(!is_safe_to_open("DATA:image/png;base64,xxxx"));
    }

    #[test]
    fn allows_safe_protocols_and_local() {
        assert!(is_safe_to_open("https://example.com"));
        assert!(is_safe_to_open("http://example.com"));
        assert!(is_safe_to_open("mailto:a@b.com"));
        assert!(is_safe_to_open("tel:+123456"));
        assert!(is_safe_to_open("relative/path.md"));
        assert!(is_safe_to_open("./foo.png"));
        assert!(is_safe_to_open("file:///C:/x.png"));
        // 内部锚点：交给调用方跳转，本身无害。
        assert!(is_safe_to_open("#section"));
    }

    #[test]
    fn rejects_empty_or_whitespace() {
        assert!(!is_safe_to_open(""));
        assert!(!is_safe_to_open("   "));
    }

    /// 分类是「怎么打开」的唯一事实来源，逐条钉住输入 → 动作的映射。
    ///
    /// 这张表同时是 C-1 的回归清单：`&calc.exe` 这类 payload 曾因
    /// `cmd /C` 二次解析而真正执行，现在必须原样作为 URL 交给系统。
    #[test]
    fn classifies_targets() {
        use super::OpenTarget::{Anchor, LocalDocument, Reject, Remote, RevealOnly};
        use super::classify_open_target;

        let cases: &[(&str, super::OpenTarget)] = &[
            // 合法远程地址：查询串里的 `&` 是普通字符，不再有 shell 语义。
            ("https://a.com&calc.exe", Remote),
            ("https://a.com?a=1&b=2", Remote),
            ("HTTPS://example.com", Remote),
            ("http://example.com", Remote),
            ("mailto:a@b.com", Remote),
            ("tel:+123456", Remote),
            // 本地可执行 / 脚本：只允许在文件管理器中定位，绝不执行。
            ("C:\\x\\evil.exe", RevealOnly),
            ("./evil.bat", RevealOnly),
            ("//host/share", Reject),
            ("\\\\host\\share", Reject),
            ("javascript:alert(1)", Reject),
            ("DATA:text/html,x", Reject),
            ("x%2Eexe", Reject),
            ("", Reject),
            ("   ", Reject),
            ("a\u{0}b", Reject),
            ("#section", Anchor),
            ("./foo.png", LocalDocument),
            ("relative/path.md", LocalDocument),
            ("file:///C:/x.png", LocalDocument),
        ];
        for (input, want) in cases {
            assert_eq!(
                classify_open_target(input),
                *want,
                "输入 {input:?} 的分类不符"
            );
        }
    }
}
