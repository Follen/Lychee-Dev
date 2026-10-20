//! Real Windows subprocess witnesses. Declaration is not execution evidence.
#![cfg(windows)]
use lycheedev_windows::{JobGuard, guarded_spawn};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[allow(dead_code)]
#[path = "../../lycheedev-data/tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, Fixture};
struct Running {
    child: Option<Child>,
    job: Option<JobGuard>,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            let _ = job.terminate();
        }
        if let Some(child) = &mut self.child {
            let _ = child.kill();
        }
    }
}
fn start(fx: &Fixture) -> Running {
    let inv = fx.invocation("data agent serve", &[]);
    let mut command = Command::new(env!("CARGO_BIN_EXE_lycheedev"));
    command
        .args([
            "data",
            "agent",
            "serve",
            "--format",
            "jsonl",
            "--timeout-seconds",
            "30",
        ])
        .arg("--home")
        .arg(&inv.home)
        .current_dir(&inv.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &inv.flags {
        command.arg(key);
        if value != "true" {
            command.arg(value);
        }
    }
    let now = Instant::now();
    let (mut child, job) = guarded_spawn::spawn_suspended_guarded_until(
        command,
        2usize << 30,
        now + Duration::from_secs(10),
        now + Duration::from_secs(15),
        || false,
    )
    .unwrap();
    Running {
        input: child.stdin.take(),
        output: BufReader::new(child.stdout.take().unwrap()),
        child: Some(child),
        job: Some(job),
    }
}
fn send(run: &mut Running, value: Value) {
    let input = run.input.as_mut().unwrap();
    serde_json::to_writer(&mut *input, &value).unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
}
fn line(run: &mut Running) -> Value {
    let mut text = String::new();
    run.output.read_line(&mut text).unwrap();
    serde_json::from_str(&text).unwrap()
}
fn query(id: u64) -> Value {
    json!({"protocol":"lycheedev.data-agent.v1","id":id,"timeout_ms":20000,"task":"query","query":{"kind":"plan","plan":{"op":"scan","table":{"name":"AgentWitness","mode":"static"}}}})
}
fn fixture(rows: usize, text: &str) -> Fixture {
    let rows = (1..=rows)
        .map(|id| vec![Cell::U(id as u32), Cell::Text(text.into())])
        .collect::<Vec<_>>();
    Fixture::new(&[fixture::table(
        "AgentWitness",
        955,
        &[("ID", "uint"), ("label", "text")],
        &rows,
    )])
}
fn finish(mut run: Running) -> (i32, String, String) {
    run.input.take();
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = run.child.as_mut().unwrap().try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            run.job.as_ref().unwrap().terminate().unwrap();
            panic!("supervisor did not exit before independent test deadline");
        }
        thread::sleep(Duration::from_millis(10));
    };
    let mut out = String::new();
    run.output.read_to_string(&mut out).unwrap();
    let mut err = String::new();
    run.child
        .as_mut()
        .unwrap()
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert_eq!(
        run.job
            .as_ref()
            .unwrap()
            .memory_accounting()
            .unwrap()
            .active_processes,
        0,
        "all descendant workers must be reaped"
    );
    guarded_spawn::close_child(run.child.take().unwrap()).unwrap();
    run.job.take().unwrap().close().unwrap();
    (status.code().unwrap(), out, err)
}
#[test]
fn supervisor_eof_half_close_drains_submitted_query_instead_of_cancelling_it() {
    let fx = fixture(2, "exact");
    let mut run = start(&fx);
    send(&mut run, query(1));
    let (code, out, err) = finish(run);
    assert_eq!(code, 0, "{err}");
    let frame: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(frame["id"], 1);
    assert_eq!(frame["ok"], true);
    assert_eq!(frame["result"]["result"]["rows"], 2);
}
#[test]
fn supervisor_deadline_terminates_stdout_backpressure_and_old_handles_are_invalid() {
    let fx = fixture(512, &"x".repeat(4096));
    let mut run = start(&fx);
    send(&mut run, query(1));
    let first = line(&mut run);
    assert_eq!(first["ok"], true, "{first}");
    let handle = first["result"]["result"]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    send(
        &mut run,
        json!({"protocol":"lycheedev.data-agent.v1","id":2,"timeout_ms":100,"task":"page","handle":handle,"offset":0,"limit":512}),
    );
    // Deliberately do not consume stdout while waiting: the child must not need
    // output progress or a yielding DataFusion poll for the parent to stop it.
    let (code, _partial, err) = finish(run);
    assert_eq!(code, 7, "{err}");
    assert!(err.contains("command.deadline"), "{err}");
    assert!(err.contains("sessionInvalidated"), "{err}");
    let mut fresh = start(&fx);
    send(
        &mut fresh,
        json!({"protocol":"lycheedev.data-agent.v1","id":1,"task":"page","handle":handle,"offset":0,"limit":1}),
    );
    let (code, out, err) = finish(fresh);
    assert_eq!(code, 0, "{err}");
    let frame: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(frame["error"]["code"], "data.session.result_missing");
}
#[test]
fn supervisor_cancel_of_submitted_work_terminates_epoch_without_replay() {
    let fx = fixture(512, &"x".repeat(4096));
    let mut run = start(&fx);
    // Write both frames before waiting: this exercises cancellation of an
    // outstanding request, not a cooperating library call before admission.
    send(&mut run, query(1));
    send(
        &mut run,
        json!({"protocol":"lycheedev.data-agent.v1","id":2,"task":"cancel","request_id":1}),
    );
    let (code, _out, err) = finish(run);
    assert_eq!(code, 7, "{err}");
    assert!(err.contains("command.cancelled"), "{err}");
    assert!(err.contains("sessionInvalidated"), "{err}");
    assert!(err.contains("\"replayed\":false"), "{err}");
}
#[test]
fn private_worker_rejects_direct_entry_without_actual_supervisor() {
    let out = Command::new(env!("CARGO_BIN_EXE_lycheedev"))
        .arg("--data-agent-worker")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("supervisor"));
    assert!(out.stdout.is_empty());
}
