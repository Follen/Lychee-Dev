//! A real Data worker dies while the sibling Source owner and a real persisted
//! Live journal/lease belong to the same controller, home and investigation root.
use super::data_supervisor;
use serde_json::{Value, json};
#[path = "independent_live_recovery.rs"]
mod live_recovery;

pub(crate) fn data_failure(fx: &data_supervisor::Fixture) -> Value {
    let live = live_recovery::RetainedLive::create(&fx.root, &fx.client);
    let mut run = data_supervisor::start(fx);
    data_supervisor::send(&mut run, data_supervisor::query(1));
    let first = data_supervisor::line(&mut run);
    assert_eq!(first["ok"], true, "{first}");
    let handle = first["result"]["result"]["handle"].as_str().unwrap().to_owned();
    drop(run.output.take());
    data_supervisor::send(&mut run, json!({"protocol":"lycheedev.data-agent.v1","id":2,"task":"capture","handle":handle}));
    let (code, output, stderr) = data_supervisor::finish(run);
    assert_eq!(code, 5, "{stderr}");
    assert!(output.is_empty());
    let diagnostics = stderr.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).collect::<Vec<_>>();
    let worker = diagnostics.iter().find(|value| value["protocol"] == "lycheedev.data-agent-worker.v1").unwrap();
    assert_eq!(worker["error"]["details"]["capturePublication"]["state"], "committed");
    let capture_id = worker["error"]["details"]["capturePublication"]["id"].as_str().unwrap();
    assert_eq!(lycheedev_core::evidence::read_capture(&fx.home, capture_id).unwrap()["id"], capture_id);
    let supervisor = diagnostics.iter().find(|value| value["protocol"] == "lycheedev.data-agent-supervisor.v1").unwrap();
    assert!(matches!(supervisor["error"]["code"].as_str(), Some("data.session.worker_exited" | "external.io")), "{stderr}");
    assert_eq!(supervisor["error"]["details"]["sessionInvalidated"], true);
    assert_eq!(supervisor["error"]["details"]["replayed"], false);
    live.assert_preserved();
    let mut fresh = data_supervisor::start(fx);
    data_supervisor::send(&mut fresh, json!({"protocol":"lycheedev.data-agent.v1","id":1,"task":"page","handle":handle,"offset":0,"limit":1}));
    let (code, output, stderr) = data_supervisor::finish(fresh);
    assert_eq!(code, 0, "{stderr}");
    let invalid: Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(invalid["error"]["code"], "data.session.result_missing");
    live.close();
    json!({"workerFailure":diagnostics,"oldHandle":invalid,"scope":"real Data supervisor failure; persisted offline Live journal/lease; Source reuse asserted by caller"})
}
