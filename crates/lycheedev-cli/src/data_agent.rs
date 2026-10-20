//! Caller-owned supervisor and isolated Data worker. No game process is opened.
#[cfg(windows)]
mod supervisor;
#[cfg(windows)]
mod worker;
use lycheedev_core::{Invocation, Result, ToolkitError};

#[cfg(windows)]
pub fn serve(inv: Invocation) -> Result<()> {
    supervisor::serve(inv)
}
#[cfg(not(windows))]
pub fn serve(_inv: Invocation) -> Result<()> {
    Err(unavailable())
}
#[cfg(windows)]
pub fn worker(args: &[String]) -> Result<()> {
    worker::run(args)
}
#[cfg(not(windows))]
pub fn worker(_args: &[String]) -> Result<()> {
    Err(unavailable())
}
fn unavailable() -> ToolkitError {
    ToolkitError::new(
        "data.session.process_limit_unavailable",
        "Data Agent production worker requires the supported Windows Job commit and supervised-process boundary",
        3,
    )
}
#[cfg(windows)]
fn fault(message: impl Into<String>) -> ToolkitError {
    let message: String = message.into();
    let bounded: String = message.chars().take(512).collect();
    ToolkitError::new("data.session.worker", bounded, 3)
}
#[cfg(windows)]
const START_ENV: &str = "LYCHEEDEV_DATA_AGENT_START_TOKEN";
#[cfg(windows)]
const RECEIPT_PROTOCOL: &str = "lycheedev.data-agent-private.v1";
#[cfg(windows)]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Startup {
    home: std::path::PathBuf,
    cwd: std::path::PathBuf,
    flags: std::collections::BTreeMap<String, String>,
    lifetime_ms: u64,
}
#[cfg(windows)]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    protocol: String,
    token: String,
    pid: u32,
    event: String,
    id: Option<u64>,
}

/// Terminal diagnostics must not turn a stopped worker into a parent blocked
/// forever by an unread stderr pipe. The exit code remains authoritative when
/// this bounded best-effort diagnostic cannot be fully delivered.
fn diagnostic_bytes(error: &ToolkitError) -> Vec<u8> {
    use std::io::Write;
    struct Limited(Vec<u8>);
    impl Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self
                .0
                .len()
                .checked_add(bytes.len())
                .is_none_or(|size| size > 16384)
            {
                return Err(std::io::Error::other("terminal diagnostic bound"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Limited(Vec::new());
    #[derive(serde::Serialize)]
    struct Diagnostic<'a> {
        protocol: &'static str,
        ok: bool,
        error: &'a ToolkitError,
    }
    let full = Diagnostic {
        protocol: "lycheedev.data-agent-supervisor.v1",
        ok: false,
        error,
    };
    if serde_json::to_writer(&mut buffer, &full).is_err() {
        buffer.0.clear();
        let compact = serde_json::json!({"protocol":"lycheedev.data-agent-supervisor.v1","ok":false,
            "error":{"code":error.code.chars().take(128).collect::<String>(),"message":error.message.chars().take(512).collect::<String>(),"exit_code":error.exit_code,
                "details":{"diagnosticTruncated":true,"sessionInvalidated":true,"replayed":false}}});
        if serde_json::to_writer(&mut buffer, &compact).is_err() {
            return Vec::new();
        }
    }
    buffer.0.push(b'\n');
    buffer.0
}

pub fn report_error(error: &ToolkitError) {
    use std::io::Write;
    let buffer = diagnostic_bytes(error);
    let Ok(writer) = std::thread::Builder::new()
        .name("data-agent-terminal-diagnostic".into())
        .spawn(move || {
            let stderr = std::io::stderr();
            let mut stderr = stderr.lock();
            let _ = stderr.write_all(&buffer);
            let _ = stderr.flush();
        })
    else {
        return;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
    while !writer.is_finished() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    if writer.is_finished() {
        let _ = writer.join();
    }
    // A still-blocked standard-stream writer ends with the immediate process
    // exit in main. Never claim the partial diagnostic was delivered.
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn terminal_diagnostic_is_valid_and_bounded_for_large_errors() {
        let error = ToolkitError::new("large", "x".repeat(1024 * 1024), 7);
        let bytes = diagnostic_bytes(&error);
        assert!(bytes.len() <= 16385);
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["details"]["diagnosticTruncated"], true);
        assert_eq!(value["error"]["exit_code"], 7);
    }
    #[test]
    fn terminal_diagnostic_preserves_small_errors() {
        let error = ToolkitError::new("small", "detail", 3);
        let value: serde_json::Value = serde_json::from_slice(&diagnostic_bytes(&error)).unwrap();
        assert_eq!(value["error"]["code"], "small");
        assert_eq!(value["error"]["message"], "detail");
    }
}
