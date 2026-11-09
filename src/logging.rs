use flexi_logger::{DeferredNow, style};
use log::Record;
use std::io::Write;

/// Helper to extract formatted filename and line number from logs.
fn format_log_location(record: &Record) -> String {
    match (record.file(), record.line()) {
        (Some(file), Some(line)) => format!(" {}:{}", file, line),
        _ => String::new(),
    }
}

/// Logger formatter that produces plain text for files.
pub fn detailed_plain_format(
    w: &mut dyn Write,
    now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    write!(
        w,
        "[{}] {:<5} [{}] {}{}",
        now.format("%Y-%m-%d %H:%M:%S%.6f %:z"),
        record.level(),
        record.target(),
        record.args(),
        format_log_location(record)
    )
}

/// Logger formatter that produces ANSI color output for terminal displays.
pub fn detailed_colored_format(
    w: &mut dyn Write,
    now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    write!(
        w,
        "[{}] {} [{}] {}{}",
        now.format("%Y-%m-%d %H:%M:%S%.6f %:z"),
        style(record.level()).paint(format!("{:<5}", record.level())),
        record.target(),
        record.args(),
        format_log_location(record)
    )
}

/// The running logger, kept so Settings can switch detailed logs on and off.
static HANDLE: std::sync::OnceLock<flexi_logger::LoggerHandle> = std::sync::OnceLock::new();

/// Mimick's own messages at debug level, and everything else at info.
const DETAILED_SPEC: &str = "mimick=debug,info";

/// Remember the logger started in `main`, so `set_detailed` can change its level.
pub fn keep_handle(handle: &flexi_logger::LoggerHandle) {
    HANDLE.set(handle.clone()).ok();
}

/// Detailed logs record Mimick's debug messages too, to help diagnose a problem.
/// `RUST_LOG` wins when set, and dev profiles are always detailed.
pub fn set_detailed(detailed: bool) {
    if std::env::var_os("RUST_LOG").is_some() {
        return;
    }
    let spec = if detailed || crate::profile::name().is_some() {
        DETAILED_SPEC
    } else {
        "info"
    };
    if let Some(handle) = HANDLE.get()
        && let Err(err) = handle.parse_new_spec(spec)
    {
        log::warn!("Could not change the log level to {spec}: {err}");
    }
}
