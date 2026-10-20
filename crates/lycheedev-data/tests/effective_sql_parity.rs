//! Fixed Go 3.2.0 3057a966 records/effective.go, hotfix_snapshots.go,
//! navigatetest/effective_test.go and command/query_hotfix_test.go contracts.
//! Real authenticated synthetic CASC/Encoding/Root/DBD/WDC and archived XFTH;
//! never actual-client or remote-provider acceptance.
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell::U, Fixture, Table, table};
use lycheedev_core::{Invocation, Outcome, control::OperationControl, evidence, selection};
use serde_json::{Value, json};
use std::{
    fs,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

fn sample() -> Table {
    table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![U(1), U(42)], vec![U(2), U(55)], vec![U(4), U(66)]],
    )
}
fn cache(layout: u32, changes: &[(u32, i32, u32, u8, Vec<u8>)]) -> Vec<u8> {
    let mut raw = b"XFTH".to_vec();
    raw.extend(layout.to_le_bytes());
    raw.extend(69933u32.to_le_bytes());
    if layout >= 5 {
        raw.resize(44, 0);
    }
    for (hash, push, id, status, payload) in changes {
        let width = match layout {
            1 | 7 => 24,
            9 => 32,
            _ => 28,
        };
        let mut header = vec![0; width];
        header[..4].copy_from_slice(b"XFTH");
        let (po, ta, ro, si, st) = match layout {
            1 => (4, 12, 16, 8, 20),
            2..=6 => (8, 16, 20, 12, 24),
            7 => (4, 8, 12, 16, 20),
            8 => (4, 12, 16, 20, 24),
            _ => (8, 16, 20, 24, 28),
        };
        for (offset, value) in [
            (po, *push as u32),
            (ta, *hash),
            (ro, *id),
            (si, payload.len() as u32),
        ] {
            header[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        header[st] = *status;
        raw.extend(header);
        raw.extend(payload);
    }
    raw
}
fn payload(id: u32, value: u32) -> Vec<u8> {
    [id.to_le_bytes(), value.to_le_bytes()].concat()
}
fn archive(
    fx: &Fixture,
    raw: &[u8],
    kind: &str,
    pin: &str,
    complete: bool,
    truncated: bool,
) -> String {
    evidence::commit_capture(&fx.home,raw,"application/octet-stream",json!({"kind":kind,"locator":"fixture:effective","snapshot":pin,"dataBuild":catalog_fixture::BUILD}),complete,truncated).unwrap()["id"].as_str().unwrap().into()
}
fn source(fx: &Fixture, raw: &[u8]) -> String {
    archive(fx, raw, "hotfix-cache", &fx.pin, true, false)
}
fn invocation(fx: &Fixture, sql: &str, ids: &[String]) -> Invocation {
    let file = fx.root.join("effective-query.json");
    let mut request = json!({"sql":sql});
    if !ids.is_empty() {
        request["hotfix"] = json!(ids);
    }
    fs::write(&file, serde_json::to_vec(&request).unwrap()).unwrap();
    fx.invocation("data sql", &[("--file", file.to_str().unwrap())])
}
fn query(fx: &Fixture, sql: &str, ids: &[String]) -> Outcome {
    lycheedev_data::migration_oracle::dispatch(&invocation(fx, sql, ids)).unwrap()
}
fn rows(out: &Outcome) -> &Value {
    &out.data["result"]["rows"]
}
fn verify_capture(fx: &Fixture, out: &Outcome) {
    let mut public = Vec::new();
    lycheedev_core::public_json::write_line(&mut public, &out.data, &out.public_numbers).unwrap();
    serde_json::from_slice::<Value>(&public).unwrap();
    let manifest = &out.provenance[0];
    assert_eq!(
        evidence::read_capture(&fx.home, manifest["id"].as_str().unwrap()).unwrap(),
        *manifest
    );
    assert_eq!(
        serde_json::from_slice::<Value>(
            &evidence::read_blob(&fx.home, &manifest["blob"], 16 << 20).unwrap()
        )
        .unwrap(),
        out.data
    );
    assert_eq!(manifest["complete"], true);
    assert_eq!(manifest["truncated"], false);
}
#[test]
fn go_explicit_effective_sql_preserves_base_overlays_join_and_provenance() {
    let sample = sample();
    let hash = sample.hash;
    let fx = Fixture::new(&[sample]);
    let first = source(
        &fx,
        &cache(
            9,
            &[
                (hash, 1, 1, 1, payload(1, 99)),
                (hash, 2, 2, 2, vec![]),
                (hash, 3, 3, 1, payload(3, 77)),
                (hash, -1, 1, 1, payload(1, 10)),
            ],
        ),
    );
    let second = source(
        &fx,
        &cache(
            9,
            &[
                (hash, 1, 1, 1, payload(1, 100)),
                (hash, 1, 1, 1, payload(1, 101)),
            ],
        ),
    );
    let cases = [
        (
            "SELECT ID,Value FROM Sample ORDER BY ID",
            vec![first.clone()],
            json!([[1, 42], [2, 55], [4, 66]]),
        ),
        (
            "SELECT ID,Value FROM effective.Sample ORDER BY ID",
            vec![first.clone()],
            json!([[1, 99], [3, 77], [4, 66]]),
        ),
        (
            "SELECT ID,Value FROM effective.Sample WHERE ID=1.0",
            vec![first.clone(), second.clone()],
            json!([[1, 101]]),
        ),
        (
            "SELECT s.ID,s.Value,e.Value FROM Sample s LEFT JOIN effective.Sample e ON s.ID=e.ID ORDER BY s.ID",
            vec![first.clone()],
            json!([[1, 42, 99], [2, 55, null], [4, 66, 66]]),
        ),
    ];
    for (sql, ids, want) in cases {
        let out = query(&fx, sql, &ids);
        assert_eq!(*rows(&out), want, "{sql}");
        verify_capture(&fx, &out);
        if sql.contains("effective.") {
            let report = &out.data["effectiveSources"][0];
            assert_eq!(report["deleted"], 1);
            assert_eq!(report["replacedOrAdded"], 2);
            assert_eq!(report["captures"].as_array().unwrap().len(), ids.len());
            assert_eq!(report["table"], "Sample");
        } else {
            assert!(out.data.get("effectiveSources").is_none());
        }
    }
    let out = query(
        &fx,
        "SELECT ID,__hotfix_source,__hotfix_push,__hotfix_index FROM effective.Sample ORDER BY ID",
        &[first.clone()],
    );
    assert_eq!(
        *rows(&out),
        json!([[1, first, 1, 0], [3, first, 3, 2], [4, null, null, null]])
    );
    let static_only = query(
        &fx,
        "SELECT ID FROM Sample ORDER BY ID",
        &["CAP-absent".into(), "CAP-absent".into()],
    );
    assert_eq!(*rows(&static_only), json!([[1], [2], [4]]));
}
#[test]
fn go_effective_capture_order_is_secondary_to_push_and_physical_index() {
    let t = sample();
    let hash = t.hash;
    let fx = Fixture::new(&[t]);
    let a = source(
        &fx,
        &cache(
            9,
            &[
                (hash, 4, 1, 1, payload(1, 40)),
                (hash, 1, 3, 1, payload(3, 30)),
            ],
        ),
    );
    let b = source(
        &fx,
        &cache(
            9,
            &[
                (hash, 3, 1, 1, payload(1, 99)),
                (hash, 1, 3, 1, payload(3, 31)),
                (hash, 1, 3, 1, payload(3, 32)),
            ],
        ),
    );
    assert_eq!(
        *rows(&query(
            &fx,
            "SELECT ID,Value FROM effective.Sample ORDER BY ID",
            &[a.clone(), b.clone()]
        )),
        json!([[1, 40], [2, 55], [3, 32], [4, 66]])
    );
    assert_eq!(
        *rows(&query(
            &fx,
            "SELECT ID,Value FROM effective.Sample ORDER BY ID",
            &[b, a]
        )),
        json!([[1, 40], [2, 55], [3, 30], [4, 66]])
    );
}
#[test]
fn go_effective_cached_key_exception_keeps_invalid_deletions_from_overwriting() {
    for hash in [3744420815u32, 35137211] {
        let mut t = sample();
        t.hash = hash;
        t.raw[20..24].copy_from_slice(&hash.to_le_bytes());
        let fx = Fixture::new(&[t]);
        let id = source(
            &fx,
            &cache(
                9,
                &[
                    (hash, -1, 1, 1, payload(1, 10)),
                    (hash, 5, 1, 2, vec![]),
                    (hash, 6, 2, 0, vec![]),
                    (hash, 7, 9, 4, vec![]),
                ],
            ),
        );
        let out = query(
            &fx,
            "SELECT ID,Value FROM effective.Sample ORDER BY ID",
            &[id],
        );
        assert_eq!(*rows(&out), json!([[1, 10], [2, 55], [4, 66]]));
        assert_eq!(out.data["effectiveSources"][0]["deleted"], 0);
        let id = source(&fx, &cache(9, &[(hash, 5, 1, 2, vec![])]));
        assert_eq!(
            *rows(&query(
                &fx,
                "SELECT ID FROM effective.Sample ORDER BY ID",
                &[id]
            )),
            json!([[2], [4]])
        );
    }
}
#[test]
fn go_effective_layout_validity_and_nonmatching_status_isolation() {
    for layout in 1..=9 {
        let t = sample();
        let hash = t.hash;
        let fx = Fixture::new(&[t]);
        let status = if layout < 7 { 2 } else { 1 };
        let id = source(
            &fx,
            &cache(
                layout,
                &[
                    (hash, 1, 1, status, payload(1, 88)),
                    (hash, 2, 2, 0, vec![]),
                    (hash + 1, 3, 7, 255, vec![]),
                ],
            ),
        );
        assert_eq!(
            *rows(&query(
                &fx,
                "SELECT ID,Value FROM effective.Sample ORDER BY ID",
                &[id]
            )),
            json!([[1, 88], [4, 66]])
        );
    }
}
#[test]
fn go_effective_rejects_missing_duplicate_incomplete_wrong_kind_and_pin() {
    let t = sample();
    let hash = t.hash;
    let fx = Fixture::new(&[t]);
    let raw = cache(9, &[(hash, 1, 1, 1, payload(1, 88))]);
    let valid = source(&fx, &raw);
    let mut foreign = selection::load_pinned(&fx.home, &fx.pin)
        .unwrap()
        .data
        .clone()
        .unwrap();
    foreign.region = "eu".into();
    let foreign = selection::pin_selection(
        &fx.home,
        selection::SelectionSpec {
            data: Some(foreign),
            ..Default::default()
        },
    )
    .unwrap();
    let ids = vec![
        vec![],
        vec![valid.clone(), valid.clone()],
        vec![archive(&fx, &raw, "data-hotfix", &fx.pin, true, false)],
        vec![archive(&fx, &raw, "hotfix-cache", &fx.pin, false, false)],
        vec![archive(&fx, &raw, "hotfix-cache", &fx.pin, false, true)],
        vec![archive(&fx, &raw, "hotfix-cache", &foreign.id, true, false)],
    ];
    for ids in ids {
        let error =
            lycheedev_data::migration_oracle::dispatch(&invocation(&fx, "SELECT ID FROM effective.Sample", &ids))
                .err()
                .unwrap();
        assert_eq!(error.code, "records.hotfix_identity", "{ids:?}: {error:?}");
    }
    assert!(
        lycheedev_data::migration_oracle::dispatch(&invocation(
            &fx,
            "SELECT ID FROM effective.Sample",
            &["CAP-absent".into()]
        ))
        .is_err()
    );
}
#[test]
fn go_effective_rejects_bad_status_payload_build_framing_and_blob_integrity() {
    let t = sample();
    let hash = t.hash;
    let fx = Fixture::new(&[t]);
    let mut wrong_build = cache(9, &[]);
    wrong_build[8..12].copy_from_slice(&69934u32.to_le_bytes());
    for (raw, code) in [
        (
            cache(9, &[(hash, 1, 1, 5, vec![])]),
            "records.hotfix_format",
        ),
        (
            cache(9, &[(hash, 1, 1, 1, payload(2, 88))]),
            "records.hotfix_fields",
        ),
        (
            cache(9, &[(hash, 1, 1, 1, vec![1])]),
            "records.hotfix_fields",
        ),
        (wrong_build, "records.hotfix_build_mismatch"),
        (b"XFTH".to_vec(), "records.hotfix_format"),
    ] {
        let id = source(&fx, &raw);
        let error =
            lycheedev_data::migration_oracle::dispatch(&invocation(&fx, "SELECT ID FROM effective.Sample", &[id]))
                .err()
                .unwrap();
        assert_eq!(error.code, code);
    }
    let raw = cache(9, &[(hash, 1, 1, 1, payload(1, 88))]);
    let id = source(&fx, &raw);
    let capture = evidence::read_capture(&fx.home, &id).unwrap();
    let sha = capture["blob"]["sha256"].as_str().unwrap();
    fs::write(
        fx.home.join("blobs").join(&sha[..2]).join(&sha[2..]),
        b"corrupt",
    )
    .unwrap();
    assert!(
        lycheedev_data::migration_oracle::dispatch(&invocation(&fx, "SELECT ID FROM effective.Sample", &[id]))
            .is_err()
    );
}
#[test]
fn go_effective_multiple_tables_share_the_exact_ordered_capture_set() {
    let a = sample();
    let b = table(
        "Other",
        202,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![U(1), U(5)]],
    );
    let ah = a.hash;
    let bh = b.hash;
    let fx = Fixture::new(&[a, b]);
    let id = source(
        &fx,
        &cache(
            9,
            &[(ah, 1, 1, 1, payload(1, 99)), (bh, 1, 1, 1, payload(1, 44))],
        ),
    );
    let out = query(
        &fx,
        "SELECT a.ID,a.Value,b.Value FROM effective.Sample a JOIN effective.Other b ON a.ID=b.ID",
        &[id.clone()],
    );
    assert_eq!(*rows(&out), json!([[1, 99, 44]]));
    let reports = out.data["effectiveSources"].as_array().unwrap();
    assert_eq!(reports.len(), 2);
    for report in reports {
        assert_eq!(report["captures"][0]["id"], id);
    }
}
#[test]
fn go_sql_request_admission_and_cancel_precede_workspace_io() {
    let t = sample();
    let fx = Fixture::new(&[t]);
    for (sql, params) in [
        ("SELECT ID FROM Sample WHERE ID=:id", None),
        ("SELECT ID FROM Sample", Some("extra=1")),
    ] {
        let mut inv = fx.invocation("data sql", &[("--sql", sql)]);
        inv.home = fx.root.join("must-not-create");
        if let Some(param) = params {
            inv.flags.insert("--param".into(), param.into());
        }
        assert_eq!(
            lycheedev_data::migration_oracle::dispatch(&inv).err().unwrap().code,
            "query.unresolved_binding"
        );
        assert!(!inv.home.exists());
    }
    let mut inv = invocation(
        &fx,
        "SELECT ID FROM effective.Sample",
        &["CAP-absent".into()],
    );
    inv.control = OperationControl::new(
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(true)),
    );
    assert_eq!(
        lycheedev_data::migration_oracle::dispatch(&inv).err().unwrap().code,
        "command.cancelled"
    );
    inv.control = OperationControl::uncancelable(Instant::now() - Duration::from_millis(1));
    assert_eq!(
        lycheedev_data::migration_oracle::dispatch(&inv).err().unwrap().code,
        "command.deadline"
    );
}
#[test]
fn go_sql_json_document_parameter_tokens_and_param_conflict() {
    let fx = Fixture::new(&[sample()]);
    let file = fx.root.join("parameters.JSON");
    fs::write(&file,br#"{"sql":"SELECT :big,:max,:exponent,ID FROM Sample WHERE ID=:id","parameters":{"id":1,"big":9007199254740993,"max":18446744073709551615,"exponent":1e2}}"#).unwrap();
    let mut inv = fx.invocation("data sql", &[("--file", file.to_str().unwrap())]);
    let out = lycheedev_data::migration_oracle::dispatch(&inv).unwrap();
    assert_eq!(
        *rows(&out),
        json!([[9007199254740993u64, u64::MAX, 100.0, 1]])
    );
    // Value normalizes number spelling; the public encoder and archived
    // query must retain the request's original JSON-number tokens.
    let parameters =
        r#""parameters":{"big":9007199254740993,"exponent":1e2,"id":1,"max":18446744073709551615}"#;
    let mut encoded = Vec::new();
    lycheedev_core::public_json::write_line(&mut encoded, &out.data, &out.public_numbers).unwrap();
    let encoded = String::from_utf8(encoded).unwrap();
    assert!(encoded.contains(parameters), "{encoded}");
    let archived = String::from_utf8(
        evidence::read_blob(&fx.home, &out.provenance[0]["blob"], 16 << 20).unwrap(),
    )
    .unwrap();
    assert!(archived.contains(parameters), "{archived}");
    verify_capture(&fx, &out);
    inv.flags.insert("--param".into(), "id=1".into());
    assert_eq!(
        lycheedev_data::migration_oracle::dispatch(&inv).err().unwrap().code,
        "command.invalid_arguments"
    );
}

#[test]
fn go_sql_public_numbers_keep_original_parameters_and_typed_result_floats() {
    let fx = Fixture::new(&[sample()]);
    let file = fx.root.join("original-numbers.json");
    fs::write(&file, br#"{"sql":"SELECT :zero,:small,Value/Value FROM Sample WHERE ID=1","parameters":{"zero":-0,"small":1E-07}}"#).unwrap();
    let out =
        lycheedev_data::migration_oracle::dispatch(&fx.invocation("data sql", &[("--file", file.to_str().unwrap())]))
            .unwrap();
    let mut encoded = Vec::new();
    lycheedev_core::public_json::write_line(&mut encoded, &out.data, &out.public_numbers).unwrap();
    let encoded = String::from_utf8(encoded).unwrap();
    assert!(
        encoded.contains(r#""parameters":{"small":1E-07,"zero":-0}"#),
        "{encoded}"
    );
    assert!(encoded.contains(r#""rows":[[0,1e-7,1]]"#), "{encoded}");
    let raw = evidence::read_blob(&fx.home, &out.provenance[0]["blob"], 16 << 20).unwrap();
    assert!(
        String::from_utf8(raw)
            .unwrap()
            .contains(r#""parameters":{"small":1E-07,"zero":-0}"#)
    );
    assert!(out.provenance[0].get("dependencies").is_none());
}

#[test]
fn go_effective_failure_publishes_no_query_capture_and_next_query_recovers() {
    let t = sample();
    let hash = t.hash;
    let fx = Fixture::new(&[t]);
    let bad = source(&fx, &cache(9, &[(hash, 1, 1, 1, vec![1])]));
    let before = lycheedev_core::metadata::Metadata::open(&fx.home, false)
        .unwrap()
        .list::<Value>("capture/", 100)
        .unwrap()
        .len();
    assert_eq!(
        lycheedev_data::migration_oracle::dispatch(&invocation(&fx, "SELECT ID FROM effective.Sample", &[bad]))
            .err()
            .unwrap()
            .code,
        "records.hotfix_fields"
    );
    assert_eq!(
        lycheedev_core::metadata::Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len(),
        before
    );
    let good = source(&fx, &cache(9, &[(hash, 1, 1, 1, payload(1, 88))]));
    let out = query(
        &fx,
        "SELECT ID,Value FROM effective.Sample ORDER BY ID",
        &[good.clone()],
    );
    assert_eq!(*rows(&out), json!([[1, 88], [2, 55], [4, 66]]));
    verify_capture(&fx, &out);
    assert!(out.provenance[0].get("dependencies").is_none());
    assert_eq!(
        evidence::remove_capture(&fx.home, &good).unwrap()["manifestRemoved"],
        true,
        "Go SQL references are checked at publication, not lifetime dependencies"
    );
}

#[test]
fn go_effective_large_capture_metadata_admits_shared_sources_before_projection() {
    let fx = Fixture::new(&[
        table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]]),
        table("Other", 202, &[("ID", "uint")], &[vec![U(2)]]),
    ]);
    let raw = cache(9, &[]);
    let ids = (0..16)
        .map(|index| {
            let locator = format!("fixture:{index}:{}", "a".repeat(750_000));
            let source = evidence::commit_capture(
                &fx.home,
                &raw,
                "application/octet-stream",
                json!({"kind":"hotfix-cache","locator":locator,"snapshot":fx.pin,"dataBuild":catalog_fixture::BUILD}),
                true,
                false,
            ).unwrap();
            let id = source["id"].as_str().unwrap().to_owned();
            assert_eq!(evidence::read_capture(&fx.home, &id).unwrap(), source);
            id
        })
        .collect::<Vec<_>>();
    let count = || {
        lycheedev_core::metadata::Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len()
    };
    let before = count();
    let inv = invocation(
        &fx,
        "SELECT ID FROM effective.Sample UNION ALL SELECT ID FROM effective.Other",
        &ids,
    );
    assert_eq!(
        lycheedev_data::migration_oracle::dispatch(&inv).unwrap_err().code,
        "query.budget_exceeded"
    );
    assert_eq!(
        count(),
        before,
        "An over-16-MiB report publishes no query capture"
    );
    let out = query(&fx, "SELECT ID FROM effective.Sample", &ids);
    assert_eq!(rows(&out), &json!([[1]]));
    assert_eq!(
        out.data["effectiveSources"][0]["captures"]
            .as_array()
            .unwrap()
            .len(),
        16
    );
    assert_eq!(
        count(),
        before + 1,
        "The same legal large sources fit one table without a new input restriction"
    );
    let used = out.data["resourceAccounting"]["used"]["retainedBytes"]
        .as_u64()
        .unwrap();
    assert!(
        used >= 12_000_000 && used < 128 << 20,
        "Unique source metadata is admitted once: {used}"
    );
    verify_capture(&fx, &out);
}
