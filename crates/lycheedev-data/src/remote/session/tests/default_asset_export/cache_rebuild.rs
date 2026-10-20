//! Production offline CDN dispatch: the only removed file is a decoded cache
//! body. Its source, alias, original ownership and retained evidence stay exact.
use super::*;
use lycheedev_core::workspace::data_disk::DataCachePaths;
#[allow(dead_code)]
#[path = "../../../../../tests/support/catalog_fixture.rs"]
mod tables;

#[test]
fn default_export_rebuild_preserves_existing_capture_keep_and_alias_documents() {
    let f = AssetFixture::new(b"retained original source body".to_vec());
    let first = crate::dispatch(&f.invocation("first.bin")).unwrap();
    f.verify(&first, "first.bin");
    let capture = &first.provenance[0];
    let mut keep = f.invocation("unused");
    keep.command = "evidence keep".into();
    keep.positional = Some(capture["id"].as_str().unwrap().to_owned());
    keep.flags.clear();
    evidence::dispatch(&keep).unwrap();
    let source_keys = keys::Context::prepare_scoped(
        &f.source.home,
        None,
        true,
        &f.source.control,
        f.source.budget.deadline,
        Some(f.source.budget.clone()),
    )
    .unwrap();
    let encoded = framed(&f.source.payload).1;
    let mut identity = format!(
        "decoded-v1\0{}\0{}\0{}\0",
        hash(&f.source.payload),
        source_keys.source.kind,
        source_keys.source.sha256
    )
    .into_bytes();
    identity.extend(encoded);
    let alias_key = format!("decoded-v1/{}", storage::sha256(&identity));
    // Use the actual production Store, so the snapshot stays non-vacuous when
    // composed with private metadata rather than querying its legacy database.
    let store = crate::remote::cache::Store {
        home: &f.source.home,
        control: &f.source.control,
        budget: Some(&f.source.budget),
    };
    let alias = store
        .read::<Value>(&alias_key)
        .unwrap()
        .expect("real decoded alias");
    assert_eq!(alias.0["contentKey"], hash(&f.source.payload));
    let metadata = Metadata::open(&f.source.home, false).unwrap();
    let retained = metadata
        .read::<Value>(&format!(
            "evidence/keep/{}",
            capture["id"].as_str().unwrap()
        ))
        .unwrap();
    metadata.close().unwrap();
    let paths = DataCachePaths::new(&f.source.home);
    let digest = storage::sha256(&f.source.payload);
    let intent_path = paths.shared_blobs().join(format!("{digest}.intent"));
    let complete_path = paths.shared_blobs().join(format!("{digest}.complete"));
    let pair = (
        std::fs::read(&intent_path).unwrap(),
        std::fs::read(&complete_path).unwrap(),
    );
    std::fs::remove_file(evidence::blob_path(&f.source.home, &digest).unwrap()).unwrap();
    let rebuilt = crate::dispatch(&f.invocation("rebuilt.bin")).unwrap();
    f.verify(&rebuilt, "rebuilt.bin");
    assert_eq!(
        rebuilt.data["source"]["decodedCache"]["contentDecodeCalls"],
        1
    );
    assert_eq!(
        std::fs::read(f.output.join("rebuilt.bin")).unwrap(),
        f.source.payload
    );
    assert_eq!(
        (
            std::fs::read(intent_path).unwrap(),
            std::fs::read(complete_path).unwrap()
        ),
        pair
    );
    assert_eq!(
        evidence::read_capture(&f.source.home, capture["id"].as_str().unwrap()).unwrap(),
        *capture
    );
    let metadata = Metadata::open(&f.source.home, false).unwrap();
    assert_eq!(
        store
            .read::<Value>(&alias_key)
            .unwrap()
            .expect("retained decoded alias"),
        alias
    );
    assert_eq!(
        metadata
            .read::<Value>(&format!(
                "evidence/keep/{}",
                capture["id"].as_str().unwrap()
            ))
            .unwrap(),
        retained
    );
    metadata.close().unwrap();
    assert!(
        !paths
            .shared_blobs()
            .join(format!("{digest}.repair.intent"))
            .exists()
    );
    assert!(f.source.backend.calls.lock().unwrap().is_empty());
    f.close();
}

#[test]
fn default_cache_prepare_rebuilds_missing_decoded_source_and_preserves_exact_rows() {
    let table = tables::table(
        "Sample",
        7,
        &[("ID", "uint"), ("Value", "signed64")],
        &[vec![
            tables::Cell::U(1),
            tables::Cell::I64(9_007_199_254_740_993),
        ]],
    );
    let f = AssetFixture::new(table.raw.clone());
    let base = format!(
        "https://raw.githubusercontent.com/wowdev/WoWDBDefs/{}/",
        f.source.pin.definition_commit
    );
    let manifest = serde_json::to_vec(
        &json!([{"tableName":"Sample","tableHash":format!("{:08X}",table.hash),"db2FileDataID":7}]),
    )
    .unwrap();
    let dbd = table
        .dbd
        .replace(tables::BUILD, &f.source.pin.full_build)
        .into_bytes();
    let metadata = crate::cache_fixture::Cache::new(&f.source.home);
    for (suffix, bytes) in [
        ("manifest.json", manifest),
        ("definitions/Sample.dbd", dbd),
        ("meta/mapping.dbdm", Vec::new()),
    ] {
        let url = format!("{base}{suffix}");
        let blob = evidence::publish_blob(&f.source.home, &bytes, 4 << 20).unwrap();
        metadata
            .put(
                &format!("definition/{}", storage::sha256(url.as_bytes())),
                &json!({"url":url,"blob":blob}),
                None,
            )
            .unwrap();
    }

    let digest = storage::sha256(&f.source.payload);
    let body = evidence::blob_path(&f.source.home, &digest).unwrap();
    std::fs::remove_file(&body).unwrap();
    let mut inv = f.invocation("unused");
    inv.command = "data cache prepare".into();
    inv.positional = Some("Sample".into());
    inv.flags.remove("--file-id");
    inv.flags.remove("--output");
    let first = crate::dispatch(&inv).unwrap();
    assert!(first.complete);
    assert_eq!(first.data["availableRows"], 1);
    assert_eq!(std::fs::read(&body).unwrap(), table.raw);
    let paths = DataCachePaths::new(&f.source.home);
    let object = paths
        .object(first.data["object"]["sha256"].as_str().unwrap())
        .unwrap();
    let raw = std::fs::read(object).unwrap();
    assert_eq!(
        storage::sha256(&raw),
        first.data["object"]["sha256"].as_str().unwrap()
    );
    let mut reader =
        arrow_ipc::reader::FileReader::try_new(std::io::Cursor::new(raw), None).unwrap();
    let batch = reader.next().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 1);
    assert_eq!(
        batch
            .column(1)
            .as_any()
            .downcast_ref::<arrow_array::Int64Array>()
            .unwrap()
            .value(0),
        9_007_199_254_740_993
    );
    assert!(reader.next().is_none());
    drop(reader);
    let second = crate::dispatch(&inv).unwrap();
    assert_eq!(first.data["generation"], second.data["generation"]);
    assert_eq!(first.data["object"], second.data["object"]);
    assert_eq!(std::fs::read_dir(&paths.staging).unwrap().count(), 0);
    assert!(
        !paths
            .shared_blobs()
            .join(format!("{digest}.repair.intent"))
            .exists()
    );
    assert!(f.source.backend.calls.lock().unwrap().is_empty());
    f.close();
}
