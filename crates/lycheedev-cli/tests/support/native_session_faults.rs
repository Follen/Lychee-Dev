use lycheedev_windows::broker::{BrokerScope, BrokerStream};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn write_frame(stream: &mut BrokerStream, value: &Value) {
    let bytes = serde_json::to_vec(value).unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
    stream.flush().unwrap();
}
fn read_frame(stream: &mut BrokerStream) -> Value {
    let mut header = [0; 4];
    stream.read_exact(&mut header).unwrap();
    let count = u32::from_be_bytes(header) as usize;
    assert!(count <= 32 << 20);
    let mut bytes = vec![0; count];
    stream.read_exact(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// Use the same real owner as the public CLI calls. Many individually small
/// source files make deadline checkpoints observable without a blocking giant
/// parse, executing source or introducing a production delay hook.
pub fn exercise(home: &Path, mut work: Value) -> Vec<Value> {
    let normal_work = work.clone();
    let files = work["snapshot"]["files"].as_object_mut().unwrap();
    for index in 0..1500 {
        let text = (0..24)
            .map(|function| format!("local function f{function}(x) return x+{function} end\n"))
            .collect::<String>();
        files.insert(format!("stress/file{index}.lua"), json!(text));
    }
    let scope = BrokerScope::for_native_source(home).unwrap();
    let mut observations = Vec::new();
    // Rejected envelopes must close only their connection. They cannot clear a
    // valid retained snapshot or replace the process that authenticated Hello.
    let status = |scope: &BrokerScope| {
        let mut stream = BrokerStream::connect(scope, Instant::now() + Duration::from_secs(2), Arc::new(AtomicBool::new(false))).unwrap();
        let hello = read_frame(&mut stream);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        write_frame(&mut stream, &json!({"wire":"lycheedev.native-source-session.v1","deadline_unix_ms":now+2_000,"action":"status","work":null}));
        (hello, read_frame(&mut stream))
    };
    let (retained_hello, retained_status) = status(&scope);
    assert_eq!(retained_status["value"]["state"], "ready");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
    let invalid_envelopes = [
        ("zero_deadline", json!({"wire":"lycheedev.native-source-session.v1","deadline_unix_ms":0,"action":"status","work":null})),
        ("oversized_deadline", json!({"wire":"lycheedev.native-source-session.v1","deadline_unix_ms":u64::MAX,"action":"status","work":null})),
        ("unknown_field", json!({"wire":"lycheedev.native-source-session.v1","deadline_unix_ms":now+30_000,"action":"status","work":null,"unknown_field":true})),
    ];
    for (case, envelope) in invalid_envelopes {
        let mut rejected = BrokerStream::connect(&scope, Instant::now() + Duration::from_secs(2), Arc::new(AtomicBool::new(false))).unwrap();
        let hello = read_frame(&mut rejected);
        assert_eq!(hello["pid"], retained_hello["pid"]);
        assert_eq!(hello["created"], retained_hello["created"]);
        write_frame(&mut rejected, &envelope);
        assert_eq!(rejected.read(&mut [0u8; 1]).unwrap(), 0, "invalid envelope must be rejected before admission: {case}");
        drop(rejected);
        let (after_hello, after_status) = status(&scope);
        assert_eq!(after_hello["pid"], retained_hello["pid"]);
        assert_eq!(after_hello["created"], retained_hello["created"]);
        assert_eq!(after_status, retained_status);
        observations.push(json!({"event":"native_session_rejected_envelope","case":case,"request":envelope,"status":after_status}));
    }
    // Keep the original cancel then cold-deadline cases. A third independent
    // deadline case also proves retirement of a child held BEFORE the request;
    // preparing it does not renew either original stress request's budget.
    for (cancel, code, exit, warm) in [
        (true, "command.cancelled", 7, false),
        (false, "command.deadline", 5, false),
        (false, "command.deadline", 5, true),
    ] {
        if warm {
            let mut prepare = BrokerStream::connect(&scope, Instant::now()+Duration::from_secs(90), Arc::new(AtomicBool::new(false))).unwrap();
            let hello = read_frame(&mut prepare);
            assert_eq!(hello["pid"],retained_hello["pid"]);
            assert_eq!(hello["created"],retained_hello["created"]);
            let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
            write_frame(&mut prepare,&json!({"wire":"lycheedev.native-source-session.v1","deadline_unix_ms":now+90_000,"action":"query","work":normal_work}));
            let prepared=read_frame(&mut prepare);
            assert!(prepared["error"].is_null(),"{prepared}");
        }
        let held_child = (cancel || warm).then(||super::analysis_child(home,retained_hello["pid"].as_u64().unwrap() as u32));
        let begun = Instant::now();
        let total = if cancel {
            Duration::from_secs(8)
        } else {
            Duration::from_millis(1000)
        };
        println!(
            "{}",
            json!({"event":"native_session_fault_started","cancel":cancel,"warm":warm,"budgetMs":total.as_millis()})
        );
        let mut stream = BrokerStream::connect(
            &scope,
            begun + total + Duration::from_secs(1),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let hello = read_frame(&mut stream);
        assert_eq!(hello["wire"], "lycheedev.native-source-session.v1");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let request_deadline = now+total.as_millis() as u64;
        write_frame(
            &mut stream,
            &json!({"wire":"lycheedev.native-source-session.v1","deadline_unix_ms":request_deadline,"action":"query","work":work}),
        );
        if cancel {
            stream.write_all(&[1]).unwrap();
            stream.flush().unwrap();
        }
        let response = read_frame(&mut stream);
        println!(
            "{}",
            json!({"event":"native_session_fault_response","cancel":cancel,"warm":warm,"elapsedMs":begun.elapsed().as_millis(),"response":response})
        );
        assert_eq!(response["error"]["code"], code, "{response}");
        assert_eq!(response["error"]["exit_code"], exit, "{response}");
        assert!(response["value"].is_null());
        assert!(
            begun.elapsed() < total,
            "typed error must return inside original request budget"
        );
        let response_elapsed = begun.elapsed();
        assert!(response["error"]["details"]["analysisRetirementError"].is_null(),"retirement must be confirmed within the original deadline: {response}");
        if let Some(child)=&held_child {
            // The response is sent only after retirement. This zero-wait OS
            // observation cannot turn a later exit into in-budget completion.
            assert_ne!(lycheedev_windows::process::observe_lifetime(child.identity(),||Ok(())).unwrap(),lycheedev_windows::process::ProcessLifetime::Running);
        }
        drop(stream);
        // Failed processing releases the cached snapshot, but retains its owner
        // so a later public CLI request can prepare a fresh analysis.
        let status_started=Instant::now();
        let mut status = BrokerStream::connect(
            &scope,
            status_started + Duration::from_secs(1),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let status_hello = read_frame(&mut status);
        assert_eq!(status_hello["pid"], hello["pid"]);
        assert_eq!(status_hello["created"], hello["created"]);
        let status_now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        write_frame(
            &mut status,
            &json!({"wire":"lycheedev.native-source-session.v1","deadline_unix_ms":status_now+1_000,"action":"status","work":null}),
        );
        let status = read_frame(&mut status);
        assert_eq!(status["value"]["state"], "idle");
        assert_eq!(status["value"]["workerPid"], 0);
        assert_eq!(status["value"]["ownerPid"], hello["pid"]);
        assert!(status["value"]["cachedSnapshot"].is_null());
        assert!(status_started.elapsed()<Duration::from_secs(1),"previous response diagnostics must not block a subsequent short request");
        // Phase files are optional, never completion receipts. Read only after
        // authenticated Status synchronization; bind any observation to this
        // exact request and owner rather than accepting a stale sidecar.
        let diagnostic=std::fs::read(home.join("tmp/source-native-last-request.json")).ok()
            .and_then(|bytes|serde_json::from_slice::<Value>(&bytes).ok())
            .filter(|value|value["workerPid"]==hello["pid"] && value["workerCreated"]==hello["created"] && value["requestDeadlineUnixMs"]==request_deadline);
        let phase_lifetime=diagnostic.as_ref().map(|value| {
            let identity=lycheedev_windows::ProcessIdentity{pid:value["analysisPid"].as_u64().unwrap() as u32,created:value["analysisCreated"].as_u64().unwrap()};
            assert_ne!(identity.pid,hello["pid"].as_u64().unwrap() as u32);
            let observed=lycheedev_windows::process::observe_lifetime(&identity,||Ok(())).unwrap();
            assert_ne!(observed,lycheedev_windows::process::ProcessLifetime::Running);
            json!({"identity":identity,"lifetime":format!("{observed:?}")})
        });
        observations.push(json!({"response":response,"workerPid":hello["pid"],"warm":warm,"elapsedMs":response_elapsed.as_millis(),
            "heldAnalysisIdentity":held_child.as_ref().map(|child|child.identity()),"heldChildExitedAtReply":held_child.is_some(),
            "phaseObservation":phase_lifetime,"phaseDiagnosticState":if diagnostic.is_some(){"matching_optional_phase"}else{"unavailable"},"status":status}));
    }
    observations
}
