//! 冷启动耗时埋点（S8-03）。
//!
//! # 为什么默认完全不工作
//!
//! `NFR-PERF-02` 要求「冷启动 ≤ 2s」。测量它的最大陷阱是**测量本身污染被测对象**：
//! 若埋点无条件执行，每次启动都要多几次字符串格式化与 IO，量到的就不是真实启动。
//! 因此本模块只有设置环境变量 `RUSTMARK_STARTUP_TIMING=1` 时才记录，
//! 未开启时每个函数都是一次环境变量查询后立即返回，开销可忽略。
//!
//! # 里程碑口径（对齐 `docs/metrics.md` §3）
//!
//! - `进程启动`：进程入口第一行（本模块在 `main` 中被调用的第一句）
//! - `装配完成`：`Services::assemble` 返回（配置读盘 + 语法集加载 + 最近文件恢复）
//! - `应用构造完成`：`EditorApp::new` 返回（主题解析 + 快捷键注册 + 草稿扫描）
//! - `首帧提交`：第一次 `update` 结束（窗口骨架可见）
//! - `内容就绪`：文档解析 + 高亮完成（有文档打开时才有意义）

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// 进程启动时刻。只在开启埋点时设置。
static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// 里程碑序列 `(标签, 距进程启动的毫秒数)`。
static MARKS: Mutex<Vec<(&'static str, f64)>> = Mutex::new(Vec::new());

/// 首帧是否已记录（保证「首帧提交」只记一次）。
static FIRST_FRAME_RECORDED: OnceLock<()> = OnceLock::new();

/// 是否开启埋点。结果缓存，避免每次启动都重复查询环境变量。
fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("RUSTMARK_STARTUP_TIMING").is_ok_and(|v| v == "1" || v == "true")
    })
}

/// 记录进程启动时刻。**必须**是 `main` 的第一句，晚一行就会漏掉那部分的耗时。
pub fn mark_process_start() {
    if !enabled() {
        return;
    }
    let _ = PROCESS_START.set(Instant::now());
}

/// 记录一个里程碑。
pub fn mark(label: &'static str) {
    if !enabled() {
        return;
    }
    let Some(start) = PROCESS_START.get() else {
        return;
    };
    if let Ok(mut marks) = MARKS.lock() {
        marks.push((label, start.elapsed().as_secs_f64() * 1000.0));
    }
}

/// 记录首帧并打印全部里程碑。重复调用只有第一次生效。
///
/// 放在 `update` 末尾：此时窗口骨架已经画完，对应用户「看到窗口了」的时刻。
pub fn mark_first_frame_and_report() {
    if !enabled() {
        return;
    }
    if FIRST_FRAME_RECORDED.set(()).is_err() {
        return; // 已记录过，不是首帧
    }
    mark("首帧提交");

    let Ok(marks) = MARKS.lock() else {
        return;
    };
    if marks.is_empty() {
        return;
    }
    let mut out = String::from("[startup-timing] 冷启动里程碑：");
    for (i, (label, ms)) in marks.iter().enumerate() {
        if i > 0 {
            out.push_str(" → ");
        }
        out.push_str(&format!("{label}={ms:.1}ms"));
    }
    if let Some((last_label, last_ms)) = marks.last() {
        out.push_str(&format!(" ｜ 合计（到 {last_label}）={last_ms:.1}ms"));
    }
    eprintln!("{out}");
}
