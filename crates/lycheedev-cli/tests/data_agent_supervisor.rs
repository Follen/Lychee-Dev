//! Real Windows subprocess witnesses. Declaration is not execution evidence.
#![cfg(windows)]
#[path = "support/data_agent_cpu.rs"]
mod cpu;
#[path = "support/data_agent_supervisor.rs"]
mod supervisor;
use serde_json::{Value, json};
use std::process::Command;
use supervisor::*;
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
    let diagnostic: Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(diagnostic["error"]["code"], "command.cancelled");
    assert_eq!(diagnostic["error"]["details"]["requestId"], 1);
    assert_eq!(diagnostic["error"]["details"]["cancelRequestId"], 2);
    assert_eq!(diagnostic["error"]["details"]["sessionInvalidated"], true);
    assert_eq!(diagnostic["error"]["details"]["replayed"], false);
    let schema = lycheedev_data::agent_session::schema()["responseSchema"].clone();
    let terminal = &schema["$defs"]["TerminalDiagnostic"];
    for required in terminal["required"].as_array().unwrap() {
        assert!(diagnostic.get(required.as_str().unwrap()).is_some());
    }
    assert!(terminal["properties"]["protocol"]["enum"].as_array().unwrap().contains(&diagnostic["protocol"]));
    assert_eq!(diagnostic["ok"], terminal["properties"]["ok"]["const"]);
    assert!(diagnostic.get("id").is_none());
    assert!(diagnostic.get("result").is_none());
    for required in schema["$defs"]["ToolkitError"]["required"].as_array().unwrap() {
        assert!(diagnostic["error"].get(required.as_str().unwrap()).is_some());
    }
}

#[test]
fn supervisor_cancel_after_real_worker_cpu_progress_retires_epoch_without_replay() {
    use std::{
        io::BufRead,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    let fx = fixture(16384, "cpu");
    let mut run = start(&fx);
    send(&mut run, query(1));
    let first = line(&mut run);
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["result"]["result"]["rows"], 16384);
    let old_handle = first["result"]["result"]["handle"]
        .as_str()
        .unwrap()
        .to_owned();

    let supervisor_pid = run.child.as_ref().unwrap().id();
    let image = std::path::Path::new(env!("CARGO_BIN_EXE_lycheedev"))
        .canonicalize()
        .unwrap();
    let workers = run
        .job
        .as_ref()
        .unwrap()
        .membership()
        .unwrap()
        .member_pids
        .into_iter()
        .filter(|pid| *pid != supervisor_pid)
        .filter_map(|pid| {
            let process = lycheedev_windows::process::Process::open(pid).unwrap();
            (process.image_path().unwrap().canonicalize().unwrap() == image
                && process.parent_pid().unwrap() == supervisor_pid)
                .then(|| process.identity().clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(workers.len(), 1, "one exact owned CLI worker: {workers:?}");
    let worker = cpu::WorkerCpu::open(workers[0].clone());
    let baseline = worker.user_ticks();

    // Continuously drain the actual pipe. This count returns one row, so CPU
    // progress cannot be explained by the existing stdout-backpressure witness.
    let mut output = run.output.take().unwrap();
    let (sender, received) = mpsc::channel();
    let drain = thread::spawn(move || {
        let mut all = String::new();
        loop {
            let mut line = String::new();
            if output.read_line(&mut line).unwrap() == 0 {
                break;
            }
            all.push_str(&line);
            if sender.send(line).is_err() {
                break;
            }
        }
        all
    });
    let begun = Instant::now();
    let deadline = begun + Duration::from_secs(15);
    let scan = json!({"op":"scan","table":{"name":"AgentWitness","mode":"static"}});
    send(
        &mut run,
        json!({"protocol":"lycheedev.data-agent.v1","id":2,"timeout_ms":15000,
        "task":"query","query":{"kind":"plan","plan":{"op":"aggregate","groups":[],
        "metrics":[{"name":"pairs","function":"count"}],"input":{"op":"join","kind":"inner",
        "left":scan,"right":scan,"on":[{"left":"AgentWitness.label","right":"AgentWitness.label"}]}}}}),
    );
    let mut prior = baseline;
    let mut progressing_samples = 0;
    let cpu_ticks = loop {
        assert!(
            matches!(received.try_recv(), Err(mpsc::TryRecvError::Empty)),
            "CPU query completed or worker closed before cancellation witness"
        );
        let now = worker.user_ticks();
        if now > prior {
            progressing_samples += 1;
        }
        prior = now;
        if now - baseline >= 1_000_000 && progressing_samples >= 2 {
            break now - baseline;
        }
        assert!(
            Instant::now() < deadline,
            "no actual worker CPU progress before original deadline"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert!(
        matches!(received.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "target request must remain incomplete when cancel is submitted"
    );
    eprintln!(
        "CPU cancellation witness: worker={:?} userTicks100ns={cpu_ticks} progressingSamples={progressing_samples}",
        workers[0]
    );
    send(
        &mut run,
        json!({"protocol":"lycheedev.data-agent.v1","id":3,"task":"cancel","request_id":2}),
    );
    // Do not retain our observation handle while Job retirement is awaited.
    worker.close();
    let (code, trailing, err) = finish_until(run, deadline);
    let output = drain.join().unwrap();
    assert!(
        begun.elapsed() < Duration::from_secs(15),
        "retirement exceeded original deadline"
    );
    assert_eq!(code, 7, "{err}");
    assert!(trailing.is_empty());
    assert!(
        output.is_empty(),
        "CPU query must not complete before cancellation: {output}"
    );
    let diagnostic: Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(diagnostic["error"]["code"], "command.cancelled");
    assert_eq!(diagnostic["error"]["details"]["requestId"], 2);
    assert_eq!(diagnostic["error"]["details"]["cancelRequestId"], 3);
    assert_eq!(diagnostic["error"]["details"]["sessionInvalidated"], true);
    assert_eq!(diagnostic["error"]["details"]["replayed"], false);

    let mut fresh = start(&fx);
    send(
        &mut fresh,
        json!({"protocol":"lycheedev.data-agent.v1","id":1,
        "task":"page","handle":old_handle,"offset":0,"limit":1}),
    );
    let (code, output, err) = finish(fresh);
    assert_eq!(code, 0, "{err}");
    let missing: Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(missing["error"]["code"], "data.session.result_missing");
}

#[test]
fn supervisor_enforces_queued_request_deadline_while_prior_output_is_blocked() {
    let fx = fixture(512, &"x".repeat(4096));
    let mut run = start(&fx);
    send(&mut run, query(1));
    let first = line(&mut run);
    let handle = first["result"]["result"]["handle"].as_str().unwrap();
    send(
        &mut run,
        json!({"protocol":"lycheedev.data-agent.v1","id":2,"timeout_ms":5000,
        "task":"page","handle":handle,"offset":0,"limit":512}),
    );
    send(
        &mut run,
        json!({"protocol":"lycheedev.data-agent.v1","id":3,"timeout_ms":100,
        "task":"page","handle":handle,"offset":0,"limit":1}),
    );
    // The first page fills stdout. The next serial request must retain its own
    // ingress deadline while queued, without waiting for page 2's five seconds.
    let (code, _partial, err) = finish(run);
    assert_eq!(code, 7, "{err}");
    let diagnostic: Value = serde_json::from_str(err.trim()).unwrap();
    assert_eq!(diagnostic["error"]["code"], "command.deadline");
    assert_eq!(diagnostic["error"]["details"]["requestId"], 3);
    assert_eq!(diagnostic["error"]["details"]["sessionInvalidated"], true);
    assert_eq!(diagnostic["error"]["details"]["replayed"], false);
}

#[test]
fn supervisor_preserves_committed_capture_on_real_stdout_broken_pipe() {
    let fx = fixture(2, "exact");
    let mut run = start(&fx);
    send(&mut run, query(1));
    let first = line(&mut run);
    let handle = first["result"]["result"]["handle"].as_str().unwrap();
    // Close the actual pipe read end before requesting CAP. Capture work commits
    // before transport writes its response; no stdout delivery can then succeed.
    drop(run.output.take());
    send(
        &mut run,
        json!({"protocol":"lycheedev.data-agent.v1","id":2,
        "task":"capture","handle":handle}),
    );
    let (code, out, err) = finish(run);
    assert_eq!(code, 5, "{err}");
    assert!(out.is_empty());
    let diagnostics = err
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let worker = diagnostics
        .iter()
        .find(|value| value["protocol"] == "lycheedev.data-agent-worker.v1")
        .unwrap();
    let details = &worker["error"]["details"];
    assert_eq!(details["capturePublication"]["state"], "committed");
    let id = details["capturePublication"]["id"].as_str().unwrap();
    assert_eq!(details["completedCaptures"], json!([id]));
    let supervisor = diagnostics
        .iter()
        .find(|value| value["protocol"] == "lycheedev.data-agent-supervisor.v1")
        .unwrap();
    // The private channel may report native IO failure before the parent
    // observes process exit. Both terminal paths invalidate the same session.
    assert!(
        matches!(
            supervisor["error"]["code"].as_str(),
            Some("data.session.worker_exited" | "external.io")
        ),
        "{err}"
    );
    assert_eq!(
        supervisor["error"]["details"]["sessionInvalidated"], true,
        "{err}"
    );
    assert_eq!(supervisor["error"]["details"]["replayed"], false, "{err}");

    let metadata = lycheedev_core::metadata::Metadata::open(&fx.home, false).unwrap();
    let captures = metadata.list::<Value>("capture/", 10).unwrap();
    metadata.close().unwrap();
    assert_eq!(
        captures.len(),
        1,
        "failed delivery must not replay CAP work"
    );
    let capture = lycheedev_core::evidence::read_capture(&fx.home, id).unwrap();
    assert_eq!(captures[0], capture);
    let body = lycheedev_core::evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["schema"], "lycheedev.data-agent-result.v1");
    assert_eq!(body["rowCount"], 2);
    assert_eq!(body["rows"].as_array().unwrap().len(), 2);
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
