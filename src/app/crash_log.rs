//! 崩溃日志（S9-08）：panic 时把可提交的诊断报告写到本地磁盘。
//!
//! # 为什么是「写盘」而不是「上报」
//!
//! 需求 §4.3 的崩溃率目标（< 0.1%）靠可观测手段支撑，但本项目**不做遥测**：
//! 网络上报要引入 HTTP 客户端与隐私合规面（ADR-001 的零依赖原则也会被打破）。
//! 折中方案是「本地落盘 + 用户手动提交 Issue」——报告里只有版本、平台、
//! 时间、panic 消息与栈回溯，不含任何文档内容（用户正文绝不入报告）。
//!
//! # `panic = "abort"` 下 hook 仍然生效
//!
//! release profile 用 `panic = "abort"`（体积优化），但 panic hook 的调用点
//! 在**展开/中止之前**（`std::panicking` 的 rust_panic_with_hook），两种策略
//! 都会先走 hook。所以 hook 里写盘是唯一可靠的捕获时机；等 panic 返回再写
//! 就来不及了——abort 下根本没有「返回」。
//!
//! # hook 内的纪律：绝不 panic、绝不锁
//!
//! panic 可能发生在**任何**代码路径上，包括「正持有某把锁」的路径。
//! 因此 hook 内：
//!
//! - 只用 `std::fs::write` 这类不会反向 panic 的操作，全部错误**静默吞掉**
//!   （写不出报告时再 panic = 崩溃套崩溃，比丢报告糟得多）；
//! - 不碰 `log` / 任何可能持锁的全局状态；
//! - 文件名精确到秒并附带进程 ID：同一秒内二次崩溃也不互相覆盖。
//!
//! # 默认零日志
//!
//! 正常运行**不产生任何日志文件**——没有日志级别、没有滚动文件。
//! 唯一会出现在磁盘上的是崩溃报告本身，这也是「默认不产生日志」验收的依据。

use std::path::{Path, PathBuf};

/// 单份崩溃报告要携带的全部信息（可测试：构造与写盘分离）。
#[derive(Clone, Debug)]
pub struct CrashReport {
    /// 应用版本（编译期取自 `Cargo.toml`）。
    pub version: String,
    /// 平台描述（如 `windows x86_64`）。
    pub platform: String,
    /// 触发时刻（本地时间，ISO 风格 `YYYY-MM-DD HH:MM:SS`）。
    pub timestamp: String,
    /// panic 消息。
    pub message: String,
    /// panic 位置（文件:行:列）。
    pub location: String,
    /// 线程名（主线程 panic 与后台线程 panic 的定位价值不同）。
    pub thread: String,
    /// 栈回溯（`std::backtrace` 的 `Display` 输出）。
    pub backtrace: String,
}

impl CrashReport {
    /// 从当前 panic 载荷与位置构造报告（供 hook 调用）。
    ///
    /// panic 载荷可能是 `&str` / `String` / 任意类型；非字符串载荷
    /// 降级为固定占位文案而不是强行 `downcast`。
    pub fn from_panic(
        payload: &(dyn std::any::Any + Send),
        location: String,
        thread: String,
    ) -> Self {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<非字符串 panic 载荷>".to_string());
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            timestamp: utc_timestamp_string(),
            message,
            location,
            thread,
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        }
    }

    /// 渲染为可提交的纯文本报告。
    ///
    /// 首段是固定骨架（版本/平台/时间），正文是消息、位置、线程与回溯。
    /// 渲染是**纯函数**：同样输入永远得到同样文本，测试可直接断言内容。
    pub fn render(&self) -> String {
        format!(
            "rustmark 崩溃报告\n\
             ================================\n\
             版本：{}\n\
             平台：{}\n\
             时间：{}\n\
             线程：{}\n\
             位置：{}\n\
             \n\
             消息：{}\n\
             \n\
             栈回溯：\n\
             {}\n\
             \n\
             --------------------------------\n\
             请将本文件作为附件提交到 Issue：\n\
             报告只含版本 / 平台 / 崩溃位置信息，不含你的文档内容。\n",
            self.version,
            self.platform,
            self.timestamp,
            self.thread,
            self.location,
            self.message,
            self.backtrace,
        )
    }
}

/// 崩溃报告的文件名前缀。
const FILE_PREFIX: &str = "crash-";

/// 当前 UTC 时间的 `YYYY-MM-DD HH:MM:SS` 文本。
///
/// 刻意**不**用本地时间：本地时区依赖平台 API（Windows / *nix 各一套），
/// 而报告是给 Issue 提交用的，UTC 无歧义且跨平台一致。
/// 历法换算是 Howard Hinnant 的 `civil_from_days` 纯整数算法，
/// 不引入 `chrono`（见 `shared/time.rs` 的体积理由）。
fn utc_timestamp_string() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (h, m, s) = ((secs / 3_600) % 24, (secs / 60) % 60, secs % 60);
    // 自 1970-03-01 起的天数 → 公历年月日（proleptic Gregorian）。
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02}")
}

/// 把报告写到 `dir`，返回写入的文件路径。
///
/// 文件名形如 `crash-20261009-113000-1234.txt`（时间 + 进程 PID），
/// 同一秒内的第二次崩溃不会覆盖第一份。
///
/// # 错误处理：静默失败
///
/// 目录创建或写盘失败一律返回 `Err`（由调用方决定是否提示）；
/// hook 路径上会直接忽略——崩溃套崩溃比丢一份报告更糟。
pub fn write_report(dir: &Path, report: &CrashReport) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!(
        "{FILE_PREFIX}{}-{}.txt",
        report.timestamp.replace(['-', ' ', ':'], ""),
        std::process::id()
    ));
    std::fs::write(&path, report.render())?;
    Ok(path)
}

/// 目录内最多保留的崩溃报告份数。
///
/// 崩溃若成风暴（启动即崩 → 用户反复重试），无上限会悄悄吃满磁盘；
/// 10 份足以覆盖「最近一次」与「可对比的历史」，更老的按时间丢弃。
const MAX_REPORTS: usize = 10;

/// 清理旧报告：只保留最新 [`MAX_REPORTS`] 份。
///
/// 按**文件名降序**（文件名内嵌时间戳，字典序即时间序）保留前 10 份，
/// 其余删除。删除失败静默忽略——清理失败不该让 hook 路径出错。
/// 正常调用点是「安装 hook 时」，即每次启动顺手清一次；
/// 崩溃风暴场景下报告会暂时超过 10 份，下次启动收敛。
pub fn prune_reports(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(FILE_PREFIX) && n.ends_with(".txt"))
        })
        .collect();
    if files.len() <= MAX_REPORTS {
        return;
    }
    files.sort_unstable();
    files.reverse();
    for old in files.iter().skip(MAX_REPORTS) {
        let _ = std::fs::remove_file(old);
    }
}

/// 安装全局 panic hook：panic 时向 `crash_dir` 写报告并保留最近 10 份。
///
/// 由 `main()` 在**任何其他逻辑之前**调用（晚了就漏掉启动期 panic）。
/// 原 hook（若存在）会在报告写完后继续调用，保持与既有 hook 的链式兼容。
///
/// 这是生产装配点；测试请直接用 [`write_report`] /
/// [`CrashReport::from_panic`] 的纯路径，或安装到临时目录（见测试）。
pub fn install_hook(crash_dir: PathBuf) {
    let previous = std::panic::take_hook();
    let hook_dir = crash_dir.clone();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<未知位置>".to_string());
        let thread = std::thread::current()
            .name()
            .unwrap_or("<未命名线程>")
            .to_string();
        let report = CrashReport::from_panic(info.payload(), location, thread);
        let _ = write_report(&hook_dir, &report);
        previous(info);
    }));
    // 安装顺手清一次旧报告：崩溃风暴后的磁盘收敛点就在这里。
    prune_reports(&crash_dir);
}
