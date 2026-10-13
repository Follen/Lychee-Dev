//! Bounded, owned external process execution. The Windows job guards descendants
//! after attachment; spawn-to-attach race remains documented until suspended spawn.
use lycheedev_core::{Result, ToolkitError};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
#[derive(Debug)]
pub struct Output {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
enum Event {
    Output(usize, Vec<u8>, bool),
    Exited(std::io::Result<std::process::ExitStatus>),
    Input(std::io::Result<()>),
}
#[cfg(test)]
pub fn run(cmd: Command, cap: usize, timeout: Duration) -> Result<Output> {
    run_with_input(cmd, cap, timeout, None)
}
#[cfg(test)]
pub fn run_with_input(
    cmd: Command,
    cap: usize,
    timeout: Duration,
    input: Option<Vec<u8>>,
) -> Result<Output> {
    run_controlled(cmd, cap, timeout, input, None)
}
pub fn run_controlled(
    cmd: Command,
    cap: usize,
    timeout: Duration,
    input: Option<Vec<u8>>,
    cancel: Option<&AtomicBool>,
) -> Result<Output> {
    run_with_limits(cmd, cap, cap, timeout, input, cancel)
}
pub(crate) fn run_with_limits(
    mut cmd: Command,
    stdout_cap: usize,
    stderr_cap: usize,
    timeout: Duration,
    input: Option<Vec<u8>>,
    cancel: Option<&AtomicBool>,
) -> Result<Output> {
    let deadline = Instant::now() + timeout;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .spawn()
        .map_err(|e| ToolkitError::new("luals.unavailable", e.to_string(), 3))?;
    let job = match lycheedev_windows::JobGuard::attach(&child) {
        Ok(j) => j,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolkitError::new("luals.unavailable", e, 3));
        }
    };
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let mut input_complete = input.is_none();
    if let Some(input) = input {
        let mut pipe = child.stdin.take().unwrap();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let result = pipe.write_all(&input);
            drop(pipe);
            let _ = tx.send(Event::Input(result));
        });
    }
    for (i, mut pipe) in [
        (0, Box::new(out) as Box<dyn Read + Send>),
        (1, Box::new(err) as Box<dyn Read + Send>),
    ] {
        let cap = if i == 0 { stdout_cap } else { stderr_cap };
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe.by_ref().take(cap as u64 + 1).read_to_end(&mut bytes);
            let overflow = bytes.len() > cap || result.is_err();
            let _ = tx.send(Event::Output(i, bytes, overflow));
        });
    }
    std::thread::spawn(move || {
        let _ = tx.send(Event::Exited(child.wait()));
    });
    let mut outputs = [Vec::new(), Vec::new()];
    let mut received = 0;
    let mut status = None;
    let mut input_error = None;
    while received < 2 || status.is_none() || !input_complete {
        if cancel.is_some_and(|c| c.load(Ordering::Acquire)) {
            let mut error = ToolkitError::new("process.cancelled", "External process cancelled", 7);
            error.details =
                terminate_and_reap(&job, &rx, received, status.is_some(), input_complete);
            return Err(error);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let mut error = ToolkitError::new(
                "luals.check_failed",
                "External process execution deadline exceeded",
                5,
            );
            error.details =
                terminate_and_reap(&job, &rx, received, status.is_some(), input_complete);
            return Err(error);
        }
        match rx.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(Event::Output(i, b, overflow)) => {
                if overflow {
                    let mut error = ToolkitError::new(
                        "luals.input_budget",
                        "External process output cap exceeded",
                        3,
                    );
                    error.details = terminate_and_reap(
                        &job,
                        &rx,
                        received + 1,
                        status.is_some(),
                        input_complete,
                    );
                    return Err(error);
                }
                received += 1;
                outputs[i] = b;
            }
            Ok(Event::Exited(result)) => {
                status = Some(result?.code().unwrap_or(-1));
            }
            Ok(Event::Input(result)) => {
                input_complete = true;
                input_error = result.err();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => {
                let _ = job.terminate();
                return Err(ToolkitError::new(
                    "luals.check_failed",
                    "External process execution deadline exceeded",
                    5,
                ));
            }
        }
    }
    if status == Some(0)
        && let Some(e) = input_error
    {
        return Err(ToolkitError::new(
            "luals.check_failed",
            format!("Process stdin failed: {e}"),
            5,
        ));
    }
    Ok(Output {
        status: status.unwrap(),
        stdout: std::mem::take(&mut outputs[0]),
        stderr: std::mem::take(&mut outputs[1]),
    })
}
fn terminate_and_reap(
    job: &lycheedev_windows::JobGuard,
    rx: &mpsc::Receiver<Event>,
    mut outputs: usize,
    mut reaped: bool,
    mut input_complete: bool,
) -> serde_json::Value {
    let terminated = job.terminate().is_ok();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !reaped || !input_complete || outputs < 2 {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Event::Exited(_)) => reaped = true,
            Ok(Event::Input(_)) => input_complete = true,
            Ok(Event::Output(_, _, _)) => outputs += 1,
            Err(_) => break,
        }
    }
    serde_json::json!({"terminated":terminated,"reaped":reaped,"stdinWriterComplete":input_complete,"outputReadersComplete":outputs>=2})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_process() {
        let Ok(mode) = std::env::var("LYCHEEDEV_SOURCE_TEST_PROCESS") else {
            return;
        };
        match mode.as_str() {
            "output" => {
                std::io::stdout().write_all(&vec![b'x'; 65536]).unwrap();
            }
            "block" => std::thread::sleep(Duration::from_secs(20)),
            _ => panic!("unknown helper mode"),
        }
    }
    fn helper(mode: &str) -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "process::tests::helper_process", "--nocapture"])
            .env("LYCHEEDEV_SOURCE_TEST_PROCESS", mode);
        cmd
    }
    #[test]
    fn timeout_output_budget_and_cancel_reap_blocked_writer() {
        let timed = run_with_input(
            helper("block"),
            1024,
            Duration::from_millis(100),
            Some(vec![b'x'; 4 << 20]),
        )
        .unwrap_err();
        assert_eq!(timed.details["reaped"], true, "{:?}", timed.details);
        assert_eq!(timed.details["stdinWriterComplete"], true);
        assert_eq!(timed.details["outputReadersComplete"], true);
        let over = run(helper("output"), 1024, Duration::from_secs(10)).unwrap_err();
        assert_eq!(over.code, "luals.input_budget");
        assert_eq!(over.details["reaped"], true);
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            signal.store(true, Ordering::Release);
        });
        let aborted = run_controlled(
            helper("block"),
            1024,
            Duration::from_secs(10),
            Some(vec![b'x'; 4 << 20]),
            Some(&cancel),
        )
        .unwrap_err();
        thread.join().unwrap();
        assert_eq!(aborted.exit_code, 7);
        assert_eq!(aborted.details["reaped"], true);
        assert_eq!(aborted.details["stdinWriterComplete"], true);
        assert_eq!(aborted.details["outputReadersComplete"], true);
    }
}
