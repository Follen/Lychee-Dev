//! Default offline CDN dispatch with real framed/encrypted WDC3 input.
use super::*;
use lycheedev_core::{
    Invocation, evidence,
    metadata::Metadata,
    selection::{self, SelectionSpec},
};
use serde_json::json;
#[path = "../../../../tests/support/partial_contract.rs"]
mod contract;
#[allow(dead_code)]
#[path = "../../../../tests/support/catalog_fixture.rs"]
mod fixture;
fn prepare() -> (Fixture, Invocation) {
    let table = fixture::encrypted_sections(7);
    let source = Fixture::with_framing(false, None, table.raw.clone(), fixture::encrypted_frame);
    let key = source.home.join("keys.txt");
    std::fs::write(&key, "0000000000000001 000102030405060708090a0b0c0d0e0f\n").unwrap();
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
    let manifest = serde_json::to_vec(&json!([{"tableName":"Sample", "tableHash":format!("{:08X}",table.hash), "db2FileDataID":7}])).unwrap();
    let dbd = table
        .dbd
        .replace(fixture::BUILD, &source.pin.full_build)
        .into_bytes();
    let mut metadata = Metadata::open(&source.home, true).unwrap();
    for (suffix, bytes) in [
        ("manifest.json", manifest),
        ("definitions/Sample.dbd", dbd),
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
    let mut admitted = source.admission();
    let mut options = source.options();
    options.key_file = Some(key.to_string_lossy().into_owned());
    let mut reader = Session::borrowed_with(
        &mut admitted,
        &source.pin,
        options,
        &source.budget,
        &source.backend,
        &source.backend,
    )
    .unwrap();
    reader
        .read_table_batches(&[7], &mut |files| {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].reading["contentVerified"], false);
            assert_eq!(files[0].reading["missing"][0]["bytes"], 8);
            Ok(Vec::new())
        })
        .unwrap();
    reader.close().unwrap();
    admitted.finish(Ok(())).unwrap();
    let inv = Invocation {
        command: "data db2 schema".into(),
        positional: Some("Sample".into()),
        flags: BTreeMap::from([
            ("--snapshot".into(), pin.id),
            ("--cdn".into(), "true".into()),
            ("--offline".into(), "true".into()),
            ("--key-file".into(), key.to_string_lossy().into_owned()),
        ]),
        home: source.home.clone(),
        cwd: source.home.clone(),
        control: Default::default(),
    };
    (source, inv)
}
fn raw(inv: &Invocation, pin: &DataPin) -> Result<()> {
    let budget = Arc::new(Budget::query_controlled(
        inv.control.deadline(),
        inv.control.clone(),
    ));
    let mut scope = crate::remote::provider::OpenedScope::open(inv, &budget)?.admit()?;
    let result = scope.read_batches(pin, &[7], &mut |files| {
        assert_eq!(files.len(), 1);
        Ok(Vec::new())
    });
    scope.finish(result)
}
#[test]
fn default_remote_missing_key_preserves_db2_sql_csv_stream_and_key_identity() {
    let (source, inv) = prepare();
    contract::exercise(&inv);
    let mut public = inv.clone();
    public.flags.remove("--key-file");
    let partial = crate::db2::dispatch(&public).unwrap();
    assert!(!partial.complete);
    assert_eq!(
        partial.data["tables"][0]["keySource"]["state"],
        "unavailable_offline"
    );
    let key_blob = evidence::blob_path(&source.home, keys::PUBLIC_SHA256).unwrap();
    std::fs::create_dir_all(key_blob.parent().unwrap()).unwrap();
    std::fs::write(&key_blob, b"corrupt public provider").unwrap();
    assert!(crate::db2::dispatch(&public).is_err());
    let error = raw(&inv, &source.pin).unwrap_err();
    assert_eq!(error.code, "container.key_unavailable");
    assert_eq!(error.details["missingKey"], true);
    let key = inv.flag("--key-file").unwrap();
    std::fs::write(key, "1234567890abcdef 000102030405060708090a0b0c0d0e0f\n").unwrap();
    contract::complete_page(&inv);
    contract::complete_page(&inv);
    raw(&inv, &source.pin).unwrap();
    std::fs::write(key, "1234567890abcdef ffffffffffffffffffffffffffffffff\n").unwrap();
    assert!(
        crate::db2::dispatch(&inv).is_err(),
        "changed key identity reused complete bytes"
    );
    std::fs::remove_dir_all(source.home).unwrap();
}
