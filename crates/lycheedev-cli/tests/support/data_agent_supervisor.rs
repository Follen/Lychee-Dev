//! Shared real Windows supervisor witness. Control and reaping match the original test.
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
#[path = "../../../lycheedev-data/tests/support/catalog_fixture.rs"]
mod fixture;
pub(crate) use fixture::{Cell, Fixture};
pub(crate) struct Running {
    pub(crate) child: Option<Child>,
    pub(crate) job: Option<JobGuard>,
    pub(crate) input: Option<ChildStdin>,
    pub(crate) output: Option<BufReader<ChildStdout>>,
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
pub(crate) fn start(fx: &Fixture) -> Running {
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
        .arg(&fx.home)
        .current_dir(&fx.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
        .arg("--snapshot")
        .arg(&fx.pin)
        .arg("--installation")
        .arg(&fx.client)
        .arg("--offline");
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
        output: Some(BufReader::new(child.stdout.take().unwrap())),
        child: Some(child),
        job: Some(job),
    }
}
pub(crate) fn send(run: &mut Running, value: Value) {
    let input = run.input.as_mut().unwrap();
    serde_json::to_writer(&mut *input, &value).unwrap();
    input.write_all(b"\n").unwrap();
    input.flush().unwrap();
}
pub(crate) fn line(run: &mut Running) -> Value {
    let mut text = String::new();
    run.output.as_mut().unwrap().read_line(&mut text).unwrap();
    serde_json::from_str(&text).unwrap()
}
pub(crate) fn query(id: u64) -> Value {
    json!({"protocol":"lycheedev.data-agent.v1","id":id,"timeout_ms":20000,"task":"query","query":{"kind":"plan","plan":{"op":"scan","table":{"name":"AgentWitness","mode":"static"}}}})
}
pub(crate) fn fixture(rows: usize, text: &str) -> Fixture {
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
pub(crate) fn finish(run: Running) -> (i32, String, String) {
    finish_until(run, Instant::now() + Duration::from_secs(15))
}
pub(crate) fn finish_until(mut run: Running, deadline: Instant) -> (i32, String, String) {
    run.input.take();
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
    if let Some(mut output) = run.output.take() {
        output.read_to_string(&mut out).unwrap();
    }
    let mut err = String::new();
    run.child
        .as_mut()
        .unwrap()
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    guarded_spawn::close_child(run.child.take().unwrap()).unwrap();
    // Root exit and pipe EOF precede Job accounting retirement. Release our
    // exited root handle, then require the whole Job to retire by the same
    // deadline; termination in Drop must never turn this assertion into a pass.
    while run
        .job
        .as_ref()
        .unwrap()
        .memory_accounting()
        .unwrap()
        .active_processes
        != 0
    {
        if Instant::now() >= deadline {
            let membership = run.job.as_ref().unwrap().membership();
            let members = membership.as_ref().map(|snapshot| {
                snapshot
                    .member_pids
                    .iter()
                    .map(|pid| {
                        lycheedev_windows::process::Process::open(*pid)
                            .map(|process| (process.identity().clone(), process.image_path()))
                    })
                    .collect::<Vec<_>>()
            });
            panic!(
                "descendant workers did not retire by the original deadline: {membership:?}; identities={members:?}; stderr={err}"
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    run.job.take().unwrap().close().unwrap();
    (status.code().unwrap(), out, err)
}
pub(crate) fn fixture_at(root: std::path::PathBuf, rows: usize, text: &str) -> Fixture {
    let rows = (1..=rows)
        .map(|id| vec![Cell::U(id as u32), Cell::Text(text.into())])
        .collect::<Vec<_>>();
    Fixture::at(
        root,
        &[fixture::table(
            "AgentWitness",
            955,
            &[("ID", "uint"), ("label", "text")],
            &rows,
        )],
        "zhCN",
        0x40,
    )
}
