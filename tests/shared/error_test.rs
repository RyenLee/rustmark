//! `shared::error` 单元测试（S2-03）。

use rustmark::shared::error::AppError;
use rustmark::shared::result::AppResult;

/// 编译期断言：`AppError` 必须可跨线程传递（跨层 trait 要求 `Send + Sync`）。
fn assert_send_sync_static<T: Send + Sync + 'static>() {}

#[test]
fn app_error_is_send_sync_static() {
    assert_send_sync_static::<AppError>();
}

#[test]
fn io_error_conversion_keeps_message() {
    let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing.md");
    let converted: AppError = io.into();
    match converted {
        AppError::Io { message } => assert!(message.contains("missing.md")),
        other => panic!("应转换为 Io 变体，实际：{other}"),
    }
}

#[test]
fn constructors_produce_expected_variants() {
    assert!(matches!(AppError::io("boom"), AppError::Io { .. }));
    assert!(matches!(AppError::domain("rule"), AppError::Domain { .. }));
}

#[test]
fn cancelled_is_identifiable() {
    let err = AppError::Cancelled;
    assert!(err.is_cancelled());
    assert!(!AppError::domain("x").is_cancelled());
}

#[test]
fn display_message_is_non_empty_for_every_variant() {
    let all = [
        AppError::io("m"),
        AppError::domain("m"),
        AppError::Parse {
            message: "m".into(),
        },
        AppError::Highlight {
            message: "m".into(),
        },
        AppError::Export {
            message: "m".into(),
        },
        AppError::Dialog {
            message: "m".into(),
        },
        AppError::Config {
            message: "m".into(),
        },
        AppError::Cancelled,
    ];
    for e in &all {
        assert!(!e.to_string().is_empty(), "错误文案不应为空：{e:?}");
    }
}

#[test]
fn app_result_alias_carries_app_error() {
    let ok: AppResult<u8> = Ok(1);
    match ok {
        Ok(v) => assert_eq!(v, 1),
        Err(e) => panic!("应为成功结果，实际：{e}"),
    }

    let err: AppResult<u8> = Err(AppError::Cancelled);
    match err {
        Ok(v) => panic!("应为失败结果，实际：{v}"),
        Err(e) => assert!(e.is_cancelled()),
    }
}
