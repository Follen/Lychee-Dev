//! Actual default SQL path over authenticated offline CDN cache fixtures.
use super::*;
use crate::local_query;
use lycheedev_core::{
    Invocation, evidence,
    metadata::Metadata,
    selection::{self, SelectionSpec},
};
use serde_json::{Value, json};
#[allow(dead_code)]
#[path = "../../../../tests/support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell, table};

struct SqlFixture {
    source: Fixture,
    snapshot: String,
}
impl SqlFixture {
    fn new(bad_string: bool, prime: bool) -> Self {
        let table = table(
            "RemoteSample",
            7,
            &[("ID", "uint"), ("Name", "text")],
            &[vec![
                Cell::U(7),
                if bad_string {
                    Cell::BadString
                } else {
                    Cell::Text("verified remote row".into())
                },
            ]],
        );
        let source = Fixture::with_payload(false, None, table.raw.clone());
        let pin = selection::pin_selection(
            &source.home,
            SelectionSpec {
                data: Some(source.pin.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let base = format!(
            "https://raw.githubusercontent.com/wowdev/WoWDBDefs/{}/",
            source.pin.definition_commit
        );
        let manifest = serde_json::to_vec(&json!([{"tableName":table.name,"tableHash":format!("{:08X}",table.hash),"db2FileDataID":7}])).unwrap();
        let dbd = table
            .dbd
            .replace(catalog_fixture::BUILD, &source.pin.full_build)
            .into_bytes();
        let mut metadata = Metadata::open(&source.home, true).unwrap();
        for (suffix, bytes) in [
            ("manifest.json", manifest),
            ("definitions/RemoteSample.dbd", dbd),
            ("meta/mapping.dbdm", Vec::new()),
        ] {
            let url = format!("{base}{suffix}");
            let blob = evidence::publish_blob(&source.home, &bytes, 4 << 20).unwrap();
            metadata
                .put(
                    &format!("definition/{}", storage::sha256(url.as_bytes())),
                    &json!({"url":url,"blob":blob}),
                    None,
                )
                .unwrap();
        }
        metadata.close().unwrap();
        if prime {
            let mut admitted = source.admission();
            let mut reader = Session::borrowed_with(
                &mut admitted,
                &source.pin,
                source.options(),
                &source.budget,
                &source.backend,
                &source.backend,
            )
            .unwrap();
            reader
                .read_batches(&[7], &mut |files| {
                    assert_eq!(files[0].bytes(), source.payload);
                    Ok(Vec::new())
                })
                .unwrap();
            reader.close().unwrap();
            admitted.finish(Ok(())).unwrap();
        }
        Self {
            source,
            snapshot: pin.id,
        }
    }
    fn invocation(&self, sql: &str) -> Invocation {
        Invocation {
            command: "data sql".into(),
            positional: None,
            flags: BTreeMap::from([
                ("--snapshot".into(), self.snapshot.clone()),
                ("--cdn".into(), "true".into()),
                ("--offline".into(), "true".into()),
                ("--sql".into(), sql.into()),
            ]),
            home: self.source.home.clone(),
            cwd: self.source.home.clone(),
            control: OperationControl::default(),
        }
    }
    fn captures(&self) -> Vec<Value> {
        let metadata = Metadata::open(&self.source.home, false).unwrap();
        let values = metadata.list::<Value>("capture/", 100).unwrap();
        metadata.close().unwrap();
        values
    }
    fn verify(&self, outcome: &lycheedev_core::Outcome) {
        assert!(outcome.complete);
        assert_eq!(outcome.provenance.len(), 1);
        let capture = &outcome.provenance[0];
        assert_eq!(capture["provenance"]["snapshot"], self.snapshot);
        let raw = evidence::read_blob(&self.source.home, &capture["blob"], 16 << 20).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), outcome.data);
        assert_eq!(storage::sha256(&raw), capture["blob"]["sha256"]);
    }
    fn close(self) {
        std::fs::remove_dir_all(self.source.home).unwrap();
    }
}

#[test]
fn default_remote_sql_static_and_effective_use_cached_authenticated_files_and_capture() {
    let f = SqlFixture::new(false, true);
    for sql in [
        "SELECT ID, Name FROM static.RemoteSample",
        "SELECT ID, Name FROM effective.RemoteSample",
    ] {
        let out = crate::dispatch(&f.invocation(sql)).unwrap();
        assert_eq!(
            out.data["result"]["rows"],
            json!([[7, "verified remote row"]])
        );
        assert_eq!(out.data["sources"][0]["file"]["source"], "cdn");
        assert_eq!(out.data["sources"][0]["file"]["contentVerified"], true);
        assert_eq!(
            out.data["sources"][0]["file"]["verification"]["encodingWholeDecodedCKey"],
            false
        );
        assert_eq!(out.data["resourceAccounting"]["used"]["networkRequests"], 0);
        assert_eq!(
            out.data["sources"][0]["file"]["decodedCache"]["contentDecodeCalls"],
            0
        );
        f.verify(&out);
    }
    f.close();
}

#[test]
fn default_remote_sql_meta_is_admitted_but_does_not_prepare_cdn_or_keys() {
    let f = SqlFixture::new(false, false);
    let mut inv = f.invocation("SELECT COUNT(*) FROM meta.RemoteSample");
    inv.flags
        .insert("--installation".into(), "not-a-source-for-meta".into());
    inv.flags
        .insert("--key-file".into(), "missing-private-keys".into());
    inv.flags
        .insert("--content-variant".into(), "invalid-unread-variant".into());
    let out = crate::dispatch(&inv).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[0]]));
    assert_eq!(out.data["sources"], json!([]));
    assert_eq!(out.data["resourceAccounting"]["used"]["networkRequests"], 0);
    assert_eq!(out.data["semanticSources"].as_array().unwrap().len(), 1);
    assert!(f.source.backend.calls.lock().unwrap().is_empty());
    f.verify(&out);
    f.close();
}

#[test]
fn default_remote_sql_explain_does_not_scan_invalid_base_or_effective_string_cells() {
    let f = SqlFixture::new(true, true);
    for catalog in ["static", "effective"] {
        let out = crate::dispatch(
            &f.invocation(&format!("EXPLAIN SELECT Name FROM {catalog}.RemoteSample")),
        )
        .unwrap();
        f.verify(&out);
        let before = f.captures().len();
        let error =
            crate::dispatch(&f.invocation(&format!("SELECT Name FROM {catalog}.RemoteSample")))
                .unwrap_err();
        assert_eq!(error.code, "table.invalid_format");
        assert_eq!(f.captures().len(), before);
    }
    f.close();
}

#[test]
fn default_remote_sql_holds_workspace_admission_through_execution_and_stops_before_capture() {
    let f = SqlFixture::new(false, true);
    let inv = f.invocation("SELECT ID FROM RemoteSample");
    let before = f.captures().len();
    let mut called = false;
    let error = local_query::sql_inner_after_execute(&inv, &mut || {
        called = true;
        let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
        assert!(
            lycheedev_core::workspace::data_resources::acquire(
                &f.source.home,
                Policy::Measurement,
                &short
            )
            .is_err()
        );
        inv.control.cancel_handle().store(true, Ordering::Release);
        Ok(())
    })
    .unwrap_err();
    assert!(called);
    assert_eq!(error.exit_code, 7);
    assert_eq!(f.captures().len(), before);
    let fresh = OperationControl::default();
    lycheedev_core::workspace::data_resources::acquire(&f.source.home, Policy::Measurement, &fresh)
        .unwrap()
        .close()
        .unwrap();
    let out = crate::dispatch(&f.invocation("SELECT ID FROM RemoteSample")).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[7]]));
    f.verify(&out);
    f.close();
}

#[test]
fn default_remote_sql_missing_cache_and_conflicting_physical_source_never_fallback_or_capture() {
    let f = SqlFixture::new(false, false);
    let before = f.captures().len();
    let mut inv = f.invocation("SELECT ID FROM RemoteSample");
    assert_eq!(
        crate::dispatch(&inv).unwrap_err().code,
        "records.remote_unavailable_offline"
    );
    inv.flags
        .insert("--installation".into(), "missing-client".into());
    assert_eq!(
        crate::dispatch(&inv).unwrap_err().code,
        "records.metadata_limit"
    );
    assert_eq!(f.captures().len(), before);
    f.close();
}

#[test]
fn default_remote_sql_original_deadline_after_execution_prevents_success_capture() {
    let f = SqlFixture::new(false, true);
    let mut inv = f.invocation("SELECT ID FROM RemoteSample");
    let before = f.captures().len();
    // The original immutable deadline starts only after fixture/cache inspection.
    // Ten seconds is a readiness bound, not a timer refreshed by the hook.
    inv.control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(10));
    let mut executed = false;
    let error = local_query::sql_inner_after_execute(&inv, &mut || {
        inv.control
            .check()
            .expect("SQL must reach the execution hook while the original deadline is still live");
        executed = true;
        std::thread::sleep(
            inv.control
                .deadline()
                .saturating_duration_since(Instant::now())
                + Duration::from_millis(5),
        );
        Ok(())
    })
    .unwrap_err();
    assert!(
        executed,
        "fixture must reach execution before exercising the original deadline"
    );
    assert_eq!(error.code, inv.control.check().unwrap_err().code);
    assert_eq!(f.captures().len(), before);
    let fresh = OperationControl::default();
    lycheedev_core::workspace::data_resources::acquire(&f.source.home, Policy::Measurement, &fresh)
        .unwrap()
        .close()
        .unwrap();
    f.close();
}

mod default_db2;
