//! Actual authenticated local/CDN consumers; injected failures run only after
//! the real metadata close. These tests are not installed-client evidence.
use crate::{db2, fault, navigate};
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError,
    control::{OperationControl, TrackedCancellation},
    evidence,
    metadata::Metadata,
    storage,
    workspace::data_resources::{self, Policy},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    time::{Duration, Instant},
};
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
pub(crate) mod fixture;
use fixture::{Cell::U, Fixture, Table, table};

pub(crate) fn tables() -> Vec<Table> {
    vec![
        table(
            "Sample",
            201,
            &[("ID", "uint"), ("Value", "uint")],
            &[vec![U(1), U(42)]],
        ),
        table(
            "SpellEffect",
            303,
            &[
                ("ID", "uint"),
                ("SpellID", "uint"),
                ("Effect", "uint"),
                ("EffectMiscValue", "uint"),
            ],
            &[
                vec![U(1), U(7), U(6), U(0)],
                vec![U(2), U(7), U(28), U(5000)],
            ],
        ),
        fixture::decors(&fixture::decor_rows()),
        // No spell references: encounter has two real captures and no extra tables.
        table(
            "JournalEncounterSection",
            331,
            &[
                ("ID", "uint"),
                ("JournalEncounterID", "uint"),
                ("ParentSectionID", "uint"),
            ],
            &[vec![U(1), U(132), U(0)]],
        ),
    ]
}

pub(crate) fn invocations(base: &Invocation) -> Vec<(Invocation, usize)> {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("data db2", &[("--table", "Sample"), ("--id", "1")]),
        ("data db2", &[("--table", "Sample"), ("--limit", "50")]),
        ("data db2 schema", &[]),
        (
            "data db2 search",
            &[("--field", "Value"), ("--query", "42"), ("--limit", "50")],
        ),
        (
            "data db2 foreign-key",
            &[("--field", "Value"), ("--value", "42"), ("--limit", "50")],
        ),
        ("data spell auras", &[("--spell-id", "7")]),
        ("data spell summons", &[("--spell-id", "7")]),
        ("data decor list", &[("--limit", "50")]),
        ("data decor get", &[("--id", "30")]),
        ("data encounter get", &[("--journal-encounter-id", "132")]),
    ];
    cases
        .iter()
        .map(|(command, flags)| {
            let mut inv = base.clone();
            inv.command = (*command).into();
            inv.positional = command.starts_with("data db2 ").then(|| "Sample".into());
            inv.flags
                .extend(flags.iter().map(|(k, v)| ((*k).into(), (*v).into())));
            (
                inv,
                if *command == "data encounter get" {
                    2
                } else {
                    1
                },
            )
        })
        .collect()
}

fn capture_ids(home: &Path) -> BTreeSet<String> {
    let metadata = Metadata::open(home, false).unwrap();
    let captures = metadata.list::<Value>("capture/", 10000).unwrap();
    metadata.close().unwrap();
    captures
        .into_iter()
        .map(|value| value["id"].as_str().unwrap().to_owned())
        .collect()
}
fn released(home: &Path) {
    data_resources::acquire(home, Policy::Measurement, &OperationControl::default())
        .unwrap()
        .close()
        .unwrap();
}
fn dispatch_finishing(
    inv: &Invocation,
    finish: impl for<'a> FnOnce(crate::remote::provider::Scope<'a>, Result<Outcome>) -> Result<Outcome>,
) -> Result<Outcome> {
    if inv.command.starts_with("data db2") {
        db2::dispatch_finishing(inv, &mut || Ok(()), finish)
    } else {
        navigate::dispatch_finishing(inv, &mut || Ok(()), finish)
    }
}

pub(crate) fn finalization_cases(base: &Invocation, expected_captures: usize) {
    for (cancel, fail_close) in [(true, false), (false, true), (true, true)] {
        let before = capture_ids(&base.home);
        let mut inv = base.clone();
        let stop = TrackedCancellation::new();
        inv.control =
            OperationControl::tracked(Instant::now() + Duration::from_secs(60), stop.clone());
        let close_error = fault("vault.sqlite", 4, "fixture checked metadata close failure")
            .with_details(json!({"schema":"fixture.close.v1","afterRealClose":true}));
        let mut observed = Vec::new();
        let mut owner_error = None;
        let mut finishes = 0;
        let mut closes = 0;
        let error = dispatch_finishing(&inv, |scope, result| {
            finishes += 1;
            let out = result
                .as_ref()
                .expect("capture succeeded before consuming finish");
            assert_eq!(out.provenance.len(), expected_captures);
            observed = out.provenance.clone();
            let raw = evidence::read_blob(&inv.home, &observed[0]["blob"], 16 << 20).unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), out.data);
            let result = scope.finish_after_metadata_close(result, &mut || {
                closes += 1;
                // The actual SQLite close and admission release have happened.
                released(&inv.home);
                if cancel {
                    stop.cancel();
                }
                if fail_close {
                    Err(close_error.clone())
                } else {
                    Ok(())
                }
            });
            owner_error = Some(result.as_ref().unwrap_err().clone());
            result
        })
        .unwrap_err();
        assert_eq!(
            (finishes, closes),
            (1, 1),
            "must never replay a committed query"
        );
        let primary = owner_error.unwrap();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            (primary.code.as_str(), primary.exit_code)
        );
        assert_eq!(
            error.details["schema"],
            "lycheedev.query-committed-error.v1"
        );
        assert_eq!(
            error.details["primary"],
            serde_json::to_value(&primary).unwrap()
        );
        assert_eq!(error.details["outputPublished"], false);
        if cancel && !fail_close {
            assert_eq!(
                serde_json::to_value(&primary).unwrap(),
                serde_json::to_value(inv.control.check().unwrap_err()).unwrap()
            );
        } else if !cancel {
            assert_eq!(
                serde_json::to_value(&primary).unwrap(),
                serde_json::to_value(&close_error).unwrap()
            );
        } else {
            assert_eq!(
                primary.details["schema"],
                "lycheedev.admitted-workspace-errors.v1"
            );
            assert_eq!(
                primary.details["causes"][0],
                serde_json::to_value(&close_error).unwrap()
            );
            assert_eq!(
                primary.details["causes"][1],
                serde_json::to_value(inv.control.check().unwrap_err()).unwrap()
            );
            assert_eq!(primary.details["causes"].as_array().unwrap().len(), 2);
        }
        let ids: Vec<_> = observed
            .iter()
            .map(|capture| capture["id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(error.details["completedCaptures"], json!(ids));
        assert_eq!(
            error.message,
            format!(
                "{}\nquery captures already committed: {}",
                primary.message,
                ids.join(", ")
            )
        );
        let mut expected = before;
        expected.extend(ids.iter().cloned());
        assert_eq!(
            capture_ids(&inv.home),
            expected,
            "no unreported or replayed capture"
        );
        for (id, capture) in ids.iter().zip(&observed) {
            assert_eq!(evidence::read_capture(&inv.home, id).unwrap(), *capture);
            let raw = evidence::read_blob(&inv.home, &capture["blob"], 16 << 20).unwrap();
            assert_eq!(storage::sha256(&raw), capture["blob"]["sha256"]);
            assert_eq!(raw.len() as u64, capture["blob"]["bytes"]);
            assert_eq!(
                serde_json::from_slice::<Value>(&raw).unwrap()["complete"],
                capture["complete"]
            );
        }
        released(&inv.home);
        inv.control = OperationControl::default();
        let recovered = crate::migration_oracle::dispatch(&inv).unwrap();
        assert_eq!(recovered.provenance.len(), expected_captures);
        released(&inv.home);
    }
}

#[test]
fn local_db2_navigation_and_encounter_keep_committed_captures_after_final_control_and_close() {
    let fx = Fixture::new(&tables());
    for (inv, count) in invocations(&fx.invocation("", &[])) {
        finalization_cases(&inv, count);
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn partial_db2_keeps_exact_incomplete_capture_after_final_control_and_close() {
    let table = fixture::encrypted_sections(201);
    let (_, encoded) = fixture::encrypted_frame(&table.raw);
    let fx = Fixture::with_encoded_table(table, encoded);
    let keys = fx.root.join("no-private-keys.txt");
    fs::write(
        &keys,
        b"0000000000000001 000102030405060708090a0b0c0d0e0f\n",
    )
    .unwrap();
    let mut inv = fx.invocation("data db2 schema", &[]);
    inv.positional = Some("Sample".into());
    inv.flags
        .insert("--key-file".into(), keys.to_string_lossy().into_owned());
    finalization_cases(&inv, 1);
    for id in capture_ids(&fx.home) {
        let capture = evidence::read_capture(&fx.home, &id).unwrap();
        assert_eq!(capture["complete"], false);
        let raw = evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
        let body: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(body["partial"], true);
        assert_eq!(body["tables"][0]["contentVerified"], false);
        assert_eq!(body["tables"][0]["missing"][0]["bytes"], 8);
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn query_committed_error_keeps_uncommitted_primary_unchanged() {
    let primary: ToolkitError = fault("records.pinned_build_changed", 4, "fixture primary")
        .with_details(json!({"cause":"source changed before capture"}));
    assert_eq!(
        serde_json::to_value(super::query_committed_error(primary.clone(), &[], false)).unwrap(),
        serde_json::to_value(primary).unwrap()
    );
}

#[test]
fn committed_identity_admission_uses_only_safety_and_rejects_before_changing_either_ledger() {
    use crate::resource::{Budget, Cost};
    let budget = Budget::new(
        Cost {
            retained_bytes: 512,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(30),
        None,
    );
    super::admit_committed_identity(&budget).unwrap();
    assert_eq!(budget.snapshot().unwrap(), Cost::retained(512));
    assert_eq!(budget.logical_snapshot().unwrap(), Cost::default());
    let error = super::admit_committed_identity(&budget).unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(budget.snapshot().unwrap(), Cost::retained(512));
    assert_eq!(budget.logical_snapshot().unwrap(), Cost::default());
    let cancel = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), cancel.clone());
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    cancel.cancel();
    let error = super::admit_committed_identity(&budget).unwrap_err();
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        serde_json::to_value(control.check().unwrap_err()).unwrap()
    );
    assert_eq!(budget.snapshot().unwrap(), Cost::default());
    assert_eq!(budget.logical_snapshot().unwrap(), Cost::default());
}
