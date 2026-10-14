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
    for (cancel, code, exit) in [
        (true, "command.cancelled", 7),
        (false, "command.deadline", 5),
    ] {
        let begun = Instant::now();
        let total = if cancel {
            Duration::from_secs(8)
        } else {
            Duration::from_millis(1000)
        };
        println!(
            "{}",
            json!({"event":"native_session_fault_started","cancel":cancel,"budgetMs":total.as_millis()})
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
        write_frame(
            &mut stream,
            &json!({"wire":"lycheedev.native-source-session.v1","deadline_unix_ms":now+total.as_millis() as u64,"action":"query","work":work}),
        );
        if cancel {
            stream.write_all(&[1]).unwrap();
            stream.flush().unwrap();
        }
        let response = read_frame(&mut stream);
        println!(
            "{}",
            json!({"event":"native_session_fault_response","cancel":cancel,"elapsedMs":begun.elapsed().as_millis(),"response":response})
        );
        assert_eq!(response["error"]["code"], code, "{response}");
        assert_eq!(response["error"]["exit_code"], exit, "{response}");
        assert!(response["value"].is_null());
        assert!(
            begun.elapsed() < total,
            "typed error must return inside original request budget"
        );
        observations.push(json!({"response":response,"workerPid":hello["pid"],"elapsedMs":begun.elapsed().as_millis()}));
        drop(stream);
        // Failed processing releases the cached snapshot, but retains its owner
        // so a later public CLI request can prepare a fresh analysis.
        let mut status = BrokerStream::connect(
            &scope,
            Instant::now() + Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let status_hello = read_frame(&mut status);
        assert_eq!(status_hello["pid"], hello["pid"]);
        write_frame(
            &mut status,
            &json!({"wire":"lycheedev.native-source-session.v1","deadline_unix_ms":now+10_000,"action":"status","work":null}),
        );
        let status = read_frame(&mut status);
        assert_eq!(status["value"]["state"], "idle");
        assert_eq!(status["value"]["workerPid"], 0);
        assert_eq!(status["value"]["ownerPid"], hello["pid"]);
        assert!(status["value"]["cachedSnapshot"].is_null());
    }
    observations
}
