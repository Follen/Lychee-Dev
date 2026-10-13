//! Bounded external tool transport with one operation control and an owned Windows job.
//! Spawn-to-job attachment is still a distinct race; this is not a suspended-spawn guarantee.
use crate::{Result, ToolkitError, control::OperationControl};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
pub struct Output {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
enum Event {
    Pipe(usize, Vec<u8>, bool),
    Input(std::io::Result<()>),
    Exit(std::io::Result<std::process::ExitStatus>),
}
pub fn run(
    mut command: Command,
    input: Option<Vec<u8>>,
    stdout_cap: usize,
    stderr_cap: usize,
    control: &OperationControl,
) -> Result<Output> {
    control.check()?;
    if stdout_cap > 64 << 20 || stderr_cap > 16 << 20 {
        return Err(ToolkitError::new(
            "command.child_output_limit",
            "external output admission exceeded",
            3,
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let job = match lycheedev_windows::JobGuard::attach(&child) {
        Ok(job) => job,
        Err(message) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolkitError::new("command.child_guard", message, 5));
        }
    };
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (send, receive) = mpsc::channel();
    let mut input_done = input.is_none();
    if let Some(input) = input {
        let mut pipe = child.stdin.take().unwrap();
        let send = send.clone();
        std::thread::spawn(move || {
            let result = pipe.write_all(&input);
            drop(pipe);
            let _ = send.send(Event::Input(result));
        });
    }
    for (index, mut pipe, cap) in [
        (0, Box::new(stdout) as Box<dyn Read + Send>, stdout_cap),
        (1, Box::new(stderr) as Box<dyn Read + Send>, stderr_cap),
    ] {
        let send = send.clone();
        std::thread::spawn(move || {
            let mut raw = Vec::new();
            let failed = pipe
                .by_ref()
                .take(cap as u64 + 1)
                .read_to_end(&mut raw)
                .is_err()
                || raw.len() > cap;
            drop(pipe);
            let _ = send.send(Event::Pipe(index, raw, failed));
        });
    }
    std::thread::spawn(move || {
        let _ = send.send(Event::Exit(child.wait()));
    });
    let mut pipes = 0;
    let mut code = None;
    let mut output = [Vec::new(), Vec::new()];
    let mut input_error = None;
    let cleanup = |error: ToolkitError,
                   pipes: usize,
                   reaped: bool,
                   input_done: bool|
     -> ToolkitError {
        let terminated = job.terminate().is_ok();
        let deadline = Instant::now() + Duration::from_secs(5);
        let (mut pipes, mut reaped, mut input_done) = (pipes, reaped, input_done);
        while pipes < 2 || !reaped || !input_done {
            match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(Event::Pipe(_, _, _)) => pipes += 1,
                Ok(Event::Exit(_)) => reaped = true,
                Ok(Event::Input(_)) => input_done = true,
                Err(_) => break,
            }
        }
        error.with_details(serde_json::json!({"terminated":terminated,"reaped":reaped,"stdinWriterComplete":input_done,"outputReadersComplete":pipes==2}))
    };
    while pipes < 2 || code.is_none() || !input_done {
        let remaining = match control.remaining() {
            Ok(value) => value,
            Err(error) => return Err(cleanup(error, pipes, code.is_some(), input_done)),
        };
        match receive.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(Event::Pipe(index, bytes, overflow)) => {
                pipes += 1;
                if overflow {
                    return Err(cleanup(
                        ToolkitError::new(
                            "command.child_output_limit",
                            "external output exceeded its byte cap",
                            3,
                        ),
                        pipes,
                        code.is_some(),
                        input_done,
                    ));
                }
                output[index] = bytes;
            }
            Ok(Event::Input(result)) => {
                input_done = true;
                input_error = result.err();
            }
            Ok(Event::Exit(result)) => code = Some(result?.code().unwrap_or(-1)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => {
                return Err(cleanup(
                    ToolkitError::new(
                        "command.child_failed",
                        "external process event stream disappeared",
                        5,
                    ),
                    pipes,
                    code.is_some(),
                    input_done,
                ));
            }
        }
    }
    if code == Some(0)
        && let Some(error) = input_error
    {
        return Err(ToolkitError::io(error));
    }
    Ok(Output {
        status: code.unwrap(),
        stdout: std::mem::take(&mut output[0]),
        stderr: std::mem::take(&mut output[1]),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper() {
        match std::env::var("LYCHEEDEV_CORE_PROCESS_FIXTURE").as_deref() {
            Ok("block") => std::thread::sleep(Duration::from_secs(20)),
            Ok("output") => std::io::stdout().write_all(&vec![b'x'; 65536]).unwrap(),
            _ => {}
        }
    }
    fn command(mode: &str) -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "process::tests::helper", "--nocapture"])
            .env("LYCHEEDEV_CORE_PROCESS_FIXTURE", mode);
        cmd
    }
    #[test]
    fn deadline_and_output_overflow_reap_owned_child_and_blocked_stdin() {
        let control = OperationControl::new(
            Instant::now() + Duration::from_millis(100),
            Default::default(),
        );
        let fault = run(
            command("block"),
            Some(vec![b'x'; 4 << 20]),
            1024,
            1024,
            &control,
        )
        .err()
        .unwrap();
        assert_eq!(fault.code, "command.deadline");
        assert_eq!(fault.exit_code, 5);
        assert_eq!(fault.details["terminated"], true);
        assert_eq!(fault.details["reaped"], true);
        assert_eq!(fault.details["stdinWriterComplete"], true);
        assert_eq!(fault.details["outputReadersComplete"], true);
        let fault = run(command("output"), None, 1024, 1024, &Default::default())
            .err()
            .unwrap();
        assert_eq!(fault.code, "command.child_output_limit");
        assert_eq!(fault.details["reaped"], true);
    }
}
