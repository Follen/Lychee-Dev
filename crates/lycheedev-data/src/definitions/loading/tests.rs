use super::*;
use lycheedev_core::{control::TrackedCancellation, workspace};
use serde_json::json;
use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
};

fn home() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let home = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/rust-definition-control-fixture")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    workspace::initialize(&home).unwrap();
    home
}
fn budget() -> Budget {
    Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(10),
        None,
    )
}
fn serve(run: impl FnOnce(TcpStream, String) + Send + 'static) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/definition", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < until => {
                    thread::sleep(Duration::from_millis(2))
                }
                Err(e) => panic!("loopback accept failed: {e}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            assert!(request.len() < 16384);
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        run(stream, String::from_utf8(request).unwrap());
    });
    (url, server)
}
fn reply(raw: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    serve(move |mut stream, _| {
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            raw.len()
        );
        stream.write_all(header.as_bytes()).unwrap();
        let _ = stream.write_all(&raw);
    })
}
fn cached(home: &Path, url: &str, raw: &[u8]) -> Value {
    let blob = evidence::publish_blob(home, raw, MAXIMUM).unwrap();
    DataCacheMetadata::put_controlled_with_check(
        &home,
        &format!("definition/{}", storage::sha256(url.as_bytes())),
        &json!({"url":url,"blob":blob}),
        None,
        &OperationControl::default(),
        || Ok(()),
    )
    .unwrap();
    blob
}
fn assert_absent(home: &Path, url: &str) {
    let ledger = budget();
    let error = load(
        home,
        url,
        true,
        ledger.deadline,
        Some(&ledger),
        None,
        |_| Ok(()),
    )
    .unwrap_err();
    assert_eq!(error.code, "records.definition_unavailable_offline");
}

#[test]
fn pre_cancelled_invocation_has_no_workspace_side_effects() {
    let missing = home().join("absent");
    let token = TrackedCancellation::new();
    token.cancel();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(10), token);
    let ledger = budget();
    let error = load(
        &missing,
        "https://example.invalid/definition",
        false,
        ledger.deadline,
        Some(&ledger),
        Some(&control),
        |_| panic!("validation after cancellation"),
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert!(!missing.exists());
    assert_eq!(ledger.snapshot().unwrap(), Cost::default());
}
#[test]
fn independent_budget_stop_keeps_live_invocation_and_has_no_side_effects() {
    let missing = home().join("absent");
    let cancel = Arc::new(AtomicBool::new(true));
    let ledger = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(10),
        Some(cancel),
    );
    let control = OperationControl::default();
    let original = control.deadline();
    let error = load(
        &missing,
        "https://example.invalid/definition",
        false,
        ledger.deadline,
        Some(&ledger),
        Some(&control),
        |_| panic!("validation after stop"),
    )
    .unwrap_err();
    assert_eq!(error.code, "operation.cancelled");
    assert!(control.check().is_ok());
    assert_eq!(control.deadline(), original);
    assert!(!missing.exists());
}
#[test]
fn independent_budget_stops_pending_http_headers() {
    let home = home();
    let cancel = Arc::new(AtomicBool::new(false));
    let ledger = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(10),
        Some(cancel.clone()),
    );
    let control = OperationControl::default();
    let (url, server) = serve(move |_stream, _| cancel.store(true, Ordering::Release));
    let error = load(
        &home,
        &url,
        false,
        ledger.deadline,
        Some(&ledger),
        Some(&control),
        |_| Ok(()),
    )
    .unwrap_err();
    server.join().unwrap();
    assert_eq!(error.code, "operation.cancelled");
    assert!(control.check().is_ok());
    assert_eq!(ledger.snapshot().unwrap().network_requests, 1);
    assert_eq!(ledger.snapshot().unwrap().network_bytes, 0);
    assert_absent(&home, &url);
}
#[test]
fn invocation_stops_pending_http_headers_with_unrelated_data_budget() {
    let home = home();
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(10), token.clone());
    let ledger = budget();
    let (url, server) = serve(move |_stream, _| token.cancel());
    let error = load(
        &home,
        &url,
        false,
        ledger.deadline,
        Some(&ledger),
        Some(&control),
        |_| Ok(()),
    )
    .unwrap_err();
    server.join().unwrap();
    assert_eq!(error.code, "command.cancelled");
    assert!(ledger.check().is_ok());
    assert_eq!(ledger.snapshot().unwrap().network_requests, 1);
    assert_absent(&home, &url);
}
#[test]
fn consumed_body_bytes_survive_later_independent_cancellation() {
    let home = home();
    let cancel = Arc::new(AtomicBool::new(false));
    let ledger = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(10),
        Some(cancel.clone()),
    );
    let observed = ledger.safety_only();
    let control = OperationControl::default();
    let (url, server) = serve(move |mut stream, _| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n1234")
            .unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let cost = observed.snapshot().unwrap();
            if cost.network_bytes == 4 && cost.retained_bytes == 32 {
                break;
            }
            assert!(
                Instant::now() < until,
                "body did not reach consumer admission before cancellation: {cost:?}"
            );
            thread::sleep(Duration::from_millis(2));
        }
        cancel.store(true, Ordering::Release);
    });
    let error = load(
        &home,
        &url,
        false,
        ledger.deadline,
        Some(&ledger),
        Some(&control),
        |_| Ok(()),
    )
    .unwrap_err();
    server.join().unwrap();
    assert_eq!(error.code, "operation.cancelled");
    let cost = ledger.snapshot().unwrap();
    assert_eq!((cost.network_requests, cost.network_bytes), (1, 4));
    assert_eq!(cost.retained_bytes, 32); // Previously admitted read work is not refunded.
    assert!(control.check().is_ok());
    assert_absent(&home, &url);
}
#[test]
fn cold_budget_failure_does_not_publish_or_refund_observed_work() {
    for dimension in ["scratch", "metadata", "network"] {
        let home = home();
        let mut limits = Cost::defaults();
        match dimension {
            "scratch" => limits.retained_bytes = (8 << 20) - 1,
            "metadata" => limits.metadata_bytes = 1,
            "network" => limits.network_bytes = 1,
            _ => unreachable!(),
        }
        let ledger = Budget::new(limits, Instant::now() + Duration::from_secs(10), None);
        let (url, server) = reply(b"//x\n".to_vec());
        let error = load(
            &home,
            &url,
            false,
            ledger.deadline,
            Some(&ledger),
            None,
            |_| Ok(()),
        )
        .unwrap_err();
        server.join().unwrap();
        assert_eq!(error.code, "records.query_resource_budget", "{dimension}");
        assert_eq!(
            ledger.snapshot().unwrap().retained_bytes,
            if dimension == "metadata" { 32 } else { 0 },
            "{dimension}"
        );
        if dimension == "metadata" {
            assert_eq!(ledger.snapshot().unwrap().network_bytes, 4);
        }
        assert_absent(&home, &url);
    }
}
#[test]
fn validated_bytes_are_not_published_after_independent_stop() {
    let home = home();
    let cancel = Arc::new(AtomicBool::new(false));
    let ledger = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(10),
        Some(cancel.clone()),
    );
    let control = OperationControl::default();
    let (url, server) = reply(b"//x\n".to_vec());
    let error = load(
        &home,
        &url,
        false,
        ledger.deadline,
        Some(&ledger),
        Some(&control),
        |_| {
            cancel.store(true, Ordering::Release);
            Ok(())
        },
    )
    .unwrap_err();
    server.join().unwrap();
    assert_eq!(error.code, "operation.cancelled");
    assert_eq!(ledger.logical_snapshot().unwrap().metadata_bytes, 16);
    assert_eq!(ledger.logical_snapshot().unwrap().network_bytes, 4);
    assert_absent(&home, &url);
}
#[test]
fn private_quota_wait_observes_original_independent_budget() {
    let home = home();
    let url = "https://example.invalid/definition";
    cached(&home, url, b"//x\n");
    let guard =
        storage::ExclusiveLock::resource(&home.join("locks"), "data-derived-v1:quota").unwrap();
    let ledger = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_millis(100),
        None,
    );
    let control = OperationControl::default();
    let start = Instant::now();
    let error = load(
        &home,
        url,
        true,
        control.deadline(),
        Some(&ledger),
        Some(&control),
        |_| Ok(()),
    )
    .unwrap_err();
    guard.close().unwrap();
    assert_eq!(error.code, "records.query_resource_budget");
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(control.check().is_ok());
    assert_eq!(ledger.snapshot().unwrap().network_bytes, 0);
    assert!(
        load(
            &home,
            url,
            true,
            control.deadline(),
            None,
            Some(&control),
            |_| Ok(())
        )
        .is_ok()
    );
}

#[test]
fn corrupt_cache_is_rehashed_without_authorizing_network_replacement() {
    let home = home();
    let url = "https://example.invalid/definition";
    let blob = cached(&home, url, b"//x\n");
    std::fs::write(
        evidence::blob_path(&home, blob["sha256"].as_str().unwrap()).unwrap(),
        b"//y\n",
    )
    .unwrap();
    let ledger = budget();
    let error = load(
        &home,
        url,
        false,
        ledger.deadline,
        Some(&ledger),
        None,
        |_| panic!("corrupt parse"),
    )
    .unwrap_err();
    assert_eq!(error.code, "vault.blob_integrity");
    assert_eq!(ledger.snapshot().unwrap().network_requests, 0);
    assert_eq!(ledger.logical_snapshot().unwrap().metadata_bytes, 16);
}
#[test]
fn go_cached_object_merges_casefolded_fields_and_validates_size_without_a_budget() {
    let home = home();
    let url = "https://example.invalid/definition";
    let blob = evidence::publish_blob(&home, b"//x\n", MAXIMUM).unwrap();
    let document = format!(
        r#"{{"URL":"{url}","URL":null,"BLOB":{{"SHA256":"{}"}},"blob":{{"bytes":4}},"ignored":true}}"#,
        blob["sha256"].as_str().unwrap()
    );
    let object: CachedObject = serde_json::from_str(&document).unwrap();
    assert_eq!(object.url, url);
    assert_eq!(object.blob.bytes, 4);
    let key = format!("definition/{}", storage::sha256(url.as_bytes()));
    DataCacheMetadata::put_controlled_with_check(
        &home,
        &key,
        &json!({"URL":url,"BLOB":{"SHA256":blob["sha256"],"BYTES":4},"ignored":true}),
        None,
        &OperationControl::default(),
        || Ok(()),
    )
    .unwrap();
    let result = load(
        &home,
        url,
        true,
        Instant::now() + Duration::from_secs(5),
        None,
        None,
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(result.1, b"//x\n");
    DataCacheMetadata::put_controlled_with_check(
        &home,
        &key,
        &json!({"url":url,"blob":{"sha256":blob["sha256"],"bytes":-1}}),
        Some(1),
        &OperationControl::default(),
        || Ok(()),
    )
    .unwrap();
    let error = load(
        &home,
        url,
        true,
        Instant::now() + Duration::from_secs(5),
        None,
        None,
        |_| Ok(()),
    )
    .unwrap_err();
    assert_eq!(error.code, "records.metadata_limit");
}
#[test]
fn redirect_is_terminal_and_automatic_gzip_charges_delivered_definition_bytes() {
    let home = home();
    let (url, server) = serve(|mut stream, request| {
        assert!(
            request
                .to_ascii_lowercase()
                .contains("accept-encoding: gzip")
        );
        stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: https://example.invalid/other\r\nContent-Length: 0\r\n\r\n").unwrap();
    });
    let ledger = budget();
    assert_eq!(
        load(
            &home,
            &url,
            false,
            ledger.deadline,
            Some(&ledger),
            None,
            |_| Ok(())
        )
        .unwrap_err()
        .code,
        "records.definition_http"
    );
    server.join().unwrap();
    assert_eq!(ledger.snapshot().unwrap().network_requests, 1);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(b"//x\n").unwrap();
    let compressed = encoder.finish().unwrap();
    let (url, server) = serve(move |mut stream, _| {
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n",
            compressed.len()
        );
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(&compressed).unwrap();
    });
    let ledger = budget();
    let result = load(
        &home,
        &url,
        false,
        ledger.deadline,
        Some(&ledger),
        None,
        |_| Ok(()),
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(result.1, b"//x\n");
    assert_eq!(
        ledger.logical_snapshot().unwrap(),
        Cost {
            retained_bytes: 16,
            metadata_bytes: 16,
            decode_work: 1,
            network_requests: 1,
            network_bytes: 4
        }
    );
    let warm = budget();
    assert_eq!(
        load(&home, &url, true, warm.deadline, Some(&warm), None, |_| Ok(
            ()
        ))
        .unwrap()
        .1,
        b"//x\n"
    );
    assert_eq!(
        warm.logical_snapshot().unwrap(),
        Cost {
            retained_bytes: 16,
            metadata_bytes: 16,
            decode_work: 1,
            ..Cost::default()
        }
    );
}
#[test]
fn parse_failure_cleanup_and_stop_remain_typed_and_are_not_retry_permission() {
    let control = OperationControl::default();
    let cancel = Arc::new(AtomicBool::new(true));
    let ledger = Budget::new(Cost::defaults(), control.deadline(), Some(cancel));
    let error = finish::<()>(
        Err(fault("vault.generation_conflict", 4, "conflict")),
        Err(fault("fixture.close", 5, "close")),
        control.deadline(),
        Some(&ledger),
        &control,
    )
    .unwrap_err();
    assert_eq!(error.code, "vault.generation_conflict");
    assert_eq!(error.details["cleanup"]["code"], "fixture.close");
    assert_eq!(error.details["budget"]["code"], "operation.cancelled");
    assert!(!pure_expected(&error, "vault.generation_conflict"));
    assert!(control.check().is_ok());
}
#[test]
fn dbd_and_manifest_parsers_check_inside_work_and_preserve_exact_typed_stop() {
    let stop = fault("fixture.parse_stop", 7, "independent parser stop");
    let raw = b"COLUMNS\nint ID\nint Value\n\nBUILD 12.1.0.69933\nLAYOUT 00000001\n$id$ID<u32>\nValue<u32>\n";
    let mut checks = 0;
    let error = schema::Document::parse_with_check(raw, || {
        checks += 1;
        if checks == 5 {
            Err(stop.clone())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(checks, 5);
    assert_eq!(error.code, stop.code);
    let raw = br#"[{"tableName":"Test","tableHash":"00000001","db2FileDataID":1},{"tableName":"Next","tableHash":"00000002","db2FileDataID":2}]"#;
    let mut checks = 0;
    let result = Manifest::parse_with_check(raw, || {
        checks += 1;
        if checks == 5 {
            Err(stop.clone())
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(error) if error.code == stop.code));
    assert_eq!(checks, 5);
}

#[test]
fn raw_and_gzip_retained_admission_never_refunds_other_owner_or_network() {
    for gzip in [false, true] {
        let home = home();
        let ledger = budget();
        let other = Cost::defaults().retained_bytes - 16;
        ledger
            .charge_split(
                Cost::default(),
                Cost {
                    retained_bytes: other,
                    ..Cost::default()
                },
            )
            .unwrap();
        let (url, server) = if gzip {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(b"//x\n").unwrap();
            let body = encoder.finish().unwrap();
            serve(move |mut stream, _| {
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(&body).unwrap();
            })
        } else {
            reply(b"//x\n".to_vec())
        };
        let error = load(
            &home,
            &url,
            false,
            ledger.deadline,
            Some(&ledger),
            None,
            |_| panic!("failed allocation reached parser"),
        )
        .unwrap_err();
        server.join().unwrap();
        assert_eq!(error.code, "records.query_resource_budget");
        assert_eq!(ledger.snapshot().unwrap().network_bytes, 4);
        assert_eq!(ledger.snapshot().unwrap().retained_bytes, other);
        assert_eq!(ledger.logical_snapshot().unwrap().retained_bytes, 0);
        assert_absent(&home, &url);
    }
}
#[test]
fn four_mebibyte_plus_one_prefix_is_rejected_without_extra_read_or_publication() {
    let home = home();
    let ledger = budget();
    let (url, server) = reply(vec![b'x'; MAXIMUM + 2]);
    let error = load(
        &home,
        &url,
        false,
        ledger.deadline,
        Some(&ledger),
        None,
        |_| panic!("oversized definition parsed"),
    )
    .unwrap_err();
    server.join().unwrap();
    assert_eq!(error.code, "records.metadata_limit");
    assert_eq!(
        ledger.snapshot().unwrap().network_bytes,
        (MAXIMUM + 1) as u64
    );
    assert_eq!(
        ledger.snapshot().unwrap().retained_bytes,
        (MAXIMUM + 1) as u64 * 8
    );
    assert_eq!(ledger.logical_snapshot().unwrap().metadata_bytes, 0);
    assert_absent(&home, &url);
}

#[test]
fn malformed_manifest_keeps_parse_primary_and_actual_invocation_stop() {
    let home = home();
    let ledger = budget();
    let token = TrackedCancellation::new();
    let control = OperationControl::tracked(ledger.deadline, token.clone());
    let (url, server) = reply(b"[{invalid json".to_vec());
    let mut checks = 0;
    let error = load(
        &home,
        &url,
        false,
        ledger.deadline,
        Some(&ledger),
        Some(&control),
        |raw| {
            Manifest::parse_with_check(raw, || {
                checks += 1;
                // Arm a stop at the last admitted pre-parse boundary. Its next
                // observation must not replace the malformed parse already returned.
                if checks == 1 {
                    token.cancel();
                    Ok(())
                } else {
                    control.check()
                }
            })
            .map(|_| ())
        },
    )
    .unwrap_err();
    server.join().unwrap();
    assert_eq!(checks, 1);
    assert_eq!(error.code, "schema.invalid_definition");
    assert_eq!(
        error.details["primary"]["code"],
        "schema.invalid_definition"
    );
    assert_eq!(error.details["control"]["code"], "command.cancelled");
    assert!(ledger.check().is_ok());
    assert_eq!(ledger.snapshot().unwrap().network_bytes, 14);
    assert_absent(&home, &url);
}
#[test]
fn malformed_cached_object_keeps_parse_primary_and_actual_invocation_stop() {
    let ledger = budget();
    let token = TrackedCancellation::new();
    let control = OperationControl::tracked(ledger.deadline, token.clone());
    let mut checks = 0;
    let parsed = decode_cached_object(br#"{"url":17}"#, &control, Some(&ledger), || {
        checks += 1;
        // Check 1 follows the bounded JSON grammar pass; check 2 follows
        // normalization, immediately before typed object deserialization.
        if checks == 2 {
            token.cancel();
            Ok(())
        } else {
            control.check()
        }
    });
    let error = finish(
        parsed.map(|_| ()),
        Ok(()),
        ledger.deadline,
        Some(&ledger),
        &control,
    )
    .unwrap_err();
    assert_eq!(checks, 2);
    assert_eq!(error.code, "records.definition_identity");
    assert_eq!(
        error.details["primary"]["code"],
        "records.definition_identity"
    );
    assert_eq!(error.details["control"]["code"], "command.cancelled");
    assert_eq!(ledger.snapshot().unwrap(), Cost::default());
    assert!(ledger.check().is_ok());
}

#[test]
fn malformed_cached_json_keeps_syntax_primary_and_actual_invocation_stop() {
    let ledger = budget();
    let token = TrackedCancellation::new();
    let control = OperationControl::tracked(ledger.deadline, token.clone());
    // Model a cancellation that arrives after the caller's admitted pre-check,
    // while the bounded grammar pass is already running. Its completed syntax
    // failure must survive the next control observation and checked finish.
    token.cancel();
    let mut checks = 0;
    let parsed = decode_cached_object(b"{invalid json", &control, Some(&ledger), || {
        checks += 1;
        control.check()
    });
    let error = finish(
        parsed.map(|_| ()),
        Ok(()),
        ledger.deadline,
        Some(&ledger),
        &control,
    )
    .unwrap_err();
    assert_eq!(
        checks, 0,
        "a completed syntax failure must precede post-parse checks"
    );
    assert_eq!(error.code, "records.definition_identity");
    assert_eq!(
        error.details["primary"]["code"],
        "records.definition_identity"
    );
    assert_eq!(error.details["control"]["code"], "command.cancelled");
    assert_eq!(ledger.snapshot().unwrap(), Cost::default());
    assert!(ledger.check().is_ok());
}

#[test]
fn definition_ignores_historical_shared_alias_and_preserves_evidence() {
    let home = home();
    let url = "https://example.invalid/historical-definition";
    let key = format!("definition/{}", storage::sha256(url.as_bytes()));
    let blob = evidence::publish_blob(&home, b"//x\n", MAXIMUM).unwrap();
    let old = json!({"url":url,"blob":blob});
    let mut shared = lycheedev_core::metadata::Metadata::open(&home, true).unwrap();
    shared.put(&key, &old, None).unwrap();
    shared
        .put("capture/fixture", &json!({"retained":true}), None)
        .unwrap();
    shared.close().unwrap();
    let error = load(
        &home,
        url,
        true,
        Instant::now() + Duration::from_secs(5),
        None,
        None,
        |_| Ok(()),
    )
    .unwrap_err();
    assert_eq!(error.code, "records.definition_unavailable_offline");
    let shared = lycheedev_core::metadata::Metadata::open(&home, false).unwrap();
    assert_eq!(shared.read::<Value>(&key).unwrap(), old);
    assert_eq!(
        shared.read::<Value>("capture/fixture").unwrap(),
        json!({"retained":true})
    );
    shared.close().unwrap();
    cached(&home, url, b"//private\n");
    assert_eq!(
        load(
            &home,
            url,
            true,
            Instant::now() + Duration::from_secs(5),
            None,
            None,
            |_| Ok(())
        )
        .unwrap()
        .1,
        b"//private\n"
    );
}
