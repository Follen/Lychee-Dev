use super::*;
use lycheedev_core::{storage, workspace::data_disk::DataCachePaths};
use std::fs;
#[allow(dead_code)]
#[path = "../../../tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, Fixture};
fn sample() -> Fixture {
    Fixture::new(&[fixture::table(
        "Sample",
        201,
        &[("ID", "uint"), ("Name", "text")],
        &[vec![Cell::U(7), Cell::Text("retirement".into())]],
    )])
}
fn prepare(f: &Fixture) -> lycheedev_core::Outcome {
    let mut inv = f.invocation("data cache prepare", &[]);
    inv.positional = Some("Sample".into());
    crate::dispatch(&inv).unwrap()
}
fn command(f: &Fixture) -> Invocation {
    let mut inv = f.invocation("cache prune", &[]);
    inv.flags.clear();
    inv.flags.insert("--data-derived".into(), "true".into());
    inv
}
fn path(f: &Fixture, result: &lycheedev_core::Outcome) -> std::path::PathBuf {
    DataCachePaths::new(&f.home).generations.join(format!(
        "{}.json",
        result.data["generation"].as_str().unwrap()
    ))
}
#[test]
fn public_data_scope_retires_real_prepared_generation_then_prepare_rebuilds_exact_identity() {
    let f = sample();
    let first = prepare(&f);
    let paths = DataCachePaths::new(&f.home);
    let object = paths
        .object(first.data["object"]["sha256"].as_str().unwrap())
        .unwrap();
    let raw = fs::read(&object).unwrap();
    let manifest = fs::read(path(&f, &first)).unwrap();
    let result = crate::prune_derived_cache(&command(&f)).unwrap();
    assert!(result.complete);
    assert_eq!(result.data["removedObjects"], 1);
    assert_eq!(result.data["removedGenerations"], 1);
    assert_eq!(
        result.data["reclaimedBytes"],
        (raw.len() + manifest.len()) as u64
    );
    assert_eq!(result.data["coverage"]["automaticEvictionEnabled"], false);
    assert!(result.data["retainedOtherDataBytes"].as_u64().unwrap() > 0);
    assert!(!object.exists());
    assert!(!path(&f, &first).exists());
    let second = prepare(&f);
    assert_eq!(second.data["generation"], first.data["generation"]);
    assert_eq!(second.data["object"], first.data["object"]);
    assert_eq!(fs::read(&object).unwrap(), raw);
    assert_eq!(fs::read(path(&f, &second)).unwrap(), manifest);
    fs::remove_dir_all(f.root).unwrap();
}
#[test]
fn partial_generation_retirement_accepts_real_missing_key_metadata_without_promoting_it() {
    let spec = fixture::encrypted_sections(201);
    let (_, encoded) = fixture::encrypted_frame(&spec.raw);
    let f = Fixture::with_encoded_table(spec, encoded);
    let result = prepare(&f);
    assert!(!result.complete);
    let manifest = fs::read(path(&f, &result)).unwrap();
    let budget = Budget::controlled(
        Cost::defaults(),
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        Default::default(),
    );
    let object = reference(
        result.data["generation"].as_str().unwrap(),
        &manifest,
        &budget,
    )
    .unwrap();
    assert_eq!(object.sha256, result.data["object"]["sha256"]);
    assert!(crate::prune_derived_cache(&command(&f)).unwrap().complete);
    let rebuilt = prepare(&f);
    assert!(!rebuilt.complete);
    assert_eq!(rebuilt.data["generation"], result.data["generation"]);
    fs::remove_dir_all(f.root).unwrap();
}
#[test]
fn malformed_unknown_duplicate_or_inconsistent_typed_manifest_blocks_all_deletion() {
    let f = sample();
    let result = prepare(&f);
    let manifest_path = path(&f, &result);
    let raw = fs::read_to_string(&manifest_path).unwrap();
    let key = result.data["generation"].as_str().unwrap();
    let replacements = [
        raw.replace(
            "lycheedev.data-static-generation.v1",
            "lycheedev.data-static-generation.v9",
        ),
        raw.replacen(
            &format!("\"identitySha256\":\"{key}\""),
            &format!("\"identitySha256\":\"{}\"", "f".repeat(64)),
            1,
        ),
        raw.replacen(
            "\"recordIdOrder\":\"ascending\"",
            "\"recordIdOrder\":\"unknown\"",
            1,
        ),
        format!("{{\"unknown\":true,{}", &raw[1..]),
        format!("{{\"identitySha256\":\"{key}\",{}", &raw[1..]),
        raw.replacen("\"recordIdColumn\":2", "\"recordIdColumn\":0", 1),
    ];
    for changed in replacements {
        assert_ne!(changed, raw);
        fs::write(&manifest_path, &changed).unwrap();
        let error = crate::prune_derived_cache(&command(&f)).unwrap_err();
        assert_eq!(error.code, "records.generation_invalid");
        assert_eq!(error.details["result"]["removedObjects"], 0);
        assert_eq!(error.details["result"]["removedGenerations"], 0);
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), changed);
        let object = DataCachePaths::new(&f.home)
            .object(result.data["object"]["sha256"].as_str().unwrap())
            .unwrap();
        assert_eq!(
            storage::sha256(&fs::read(object).unwrap()),
            result.data["object"]["sha256"]
        );
    }
    fs::remove_dir_all(f.root).unwrap();
}
#[test]
fn retirement_projection_accepts_historical_uppercase_source_blobrefs_only() {
    let f = sample();
    let result = prepare(&f);
    let raw = fs::read(path(&f, &result)).unwrap();
    let mut changed = String::from_utf8(raw.clone()).unwrap();
    // Start from the real prepare producer and preserve all canonical nested
    // structures. This checks source format, not generation source re-auth.
    let receipt: Receipt<'_> = serde_json::from_slice(&raw).unwrap();
    for (field, original) in [
        ("sourceContent", receipt.source_content),
        ("definitionObject", receipt.definition_object),
        ("definitionManifest", receipt.definition_manifest),
    ] {
        let value: BlobRef = serde_json::from_str(original.get()).unwrap();
        let uppercase = value.sha256.to_ascii_uppercase();
        assert_ne!(uppercase, value.sha256);
        let replacement = original.get().replace(&value.sha256, &uppercase);
        changed = changed.replacen(
            &format!("\"{field}\":{}", original.get()),
            &format!("\"{field}\":{replacement}"),
            1,
        );
    }
    let budget = Budget::controlled(
        Cost::defaults(),
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        Default::default(),
    );
    let key = result.data["generation"].as_str().unwrap();
    let projected = reference(key, changed.as_bytes(), &budget).unwrap();
    assert_eq!(projected.sha256, result.data["object"]["sha256"]);
    let invalid = changed.replacen(
        &format!("\"object\":{{\"sha256\":\"{}\"", projected.sha256),
        &format!(
            "\"object\":{{\"sha256\":\"{}\"",
            projected.sha256.to_ascii_uppercase()
        ),
        1,
    );
    assert_ne!(invalid, changed);
    assert!(reference(key, invalid.as_bytes(), &budget).is_err());
    for (field, value) in [
        ("identitySha256", receipt.identity_sha256),
        ("definitionSha256", receipt.definition_sha256),
        ("typedSchemaSha256", receipt.typed_schema_sha256),
    ] {
        let invalid = changed.replacen(
            &format!("\"{field}\":\"{value}\""),
            &format!("\"{field}\":\"{}\"", value.to_ascii_uppercase()),
            1,
        );
        assert_ne!(invalid, changed);
        assert_eq!(
            reference(key, invalid.as_bytes(), &budget)
                .unwrap_err()
                .code,
            "records.generation_invalid"
        );
    }
    let wrong_definition = format!(r#"{{"sha256":"{}","bytes":1}}"#, "A".repeat(64));
    assert!(
        !"A".repeat(64)
            .eq_ignore_ascii_case(receipt.definition_sha256)
    );
    let definition: BlobRef = serde_json::from_str(receipt.definition_object.get()).unwrap();
    let uppercase_definition = receipt
        .definition_object
        .get()
        .replace(&definition.sha256, &definition.sha256.to_ascii_uppercase());
    let invalid = changed.replacen(
        &format!("\"definitionObject\":{uppercase_definition}"),
        &format!("\"definitionObject\":{wrong_definition}"),
        1,
    );
    assert_ne!(invalid, changed);
    assert_eq!(
        reference(key, invalid.as_bytes(), &budget)
            .unwrap_err()
            .code,
        "records.generation_invalid"
    );
    assert_eq!(fs::read(path(&f, &result)).unwrap(), raw);
    fs::remove_dir_all(f.root).unwrap();
}
#[test]
fn retirement_source_blobrefs_keep_strict_shape_length_and_optional_contract() {
    for hash in ["ab".repeat(32), "AB".repeat(32), "aB".repeat(32)] {
        let raw = RawValue::from_string(format!(r#"{{"sha256":"{hash}","bytes":1}}"#)).unwrap();
        for required in [false, true] {
            let value = blob(&raw, required).unwrap().unwrap();
            assert_eq!(value.sha256, hash);
            assert_eq!(value.bytes, 1);
        }
    }
    let hash = "AB".repeat(32);
    for malformed in [
        format!(r#"{{"sha256":"{}","bytes":1}}"#, "A".repeat(63)),
        format!(r#"{{"sha256":"{}","bytes":1}}"#, "A".repeat(65)),
        format!(r#"{{"sha256":"{}G","bytes":1}}"#, "A".repeat(63)),
        format!(r#"{{"sha256":"{}é","bytes":1}}"#, "A".repeat(62)),
        format!(r#"{{"sha256":"{hash}","bytes":0}}"#),
        format!(r#"{{"sha256":"{hash}","bytes":-1}}"#),
        format!(r#"{{"sha256":"{hash}","bytes":1,"unknown":true}}"#),
        format!(r#"{{"sha256":"{hash}","sha256":"{hash}","bytes":1}}"#),
    ] {
        let raw = RawValue::from_string(malformed).unwrap();
        for required in [false, true] {
            assert_eq!(
                blob(&raw, required).unwrap_err().code,
                "records.generation_invalid"
            );
        }
    }
    for empty in ["null", r#"{"sha256":"","bytes":0}"#] {
        let raw = RawValue::from_string(empty.into()).unwrap();
        assert!(blob(&raw, false).unwrap().is_none());
        assert_eq!(
            blob(&raw, true).unwrap_err().code,
            "records.generation_invalid"
        );
    }
}
#[test]
fn typed_dry_run_preserves_exact_files_and_incompatible_staging_flag_is_rejected() {
    let f = sample();
    let result = prepare(&f);
    let path = path(&f, &result);
    let bytes = fs::read(&path).unwrap();
    let mut inv = command(&f);
    inv.flags.insert("--dry-run".into(), "true".into());
    let planned = crate::prune_derived_cache(&inv).unwrap();
    assert!(planned.complete);
    assert_eq!(planned.data["dryRun"], true);
    assert_eq!(planned.data["deletions"][0]["state"], "planned");
    assert_eq!(fs::read(&path).unwrap(), bytes);
    inv.flags.insert("--uncommitted".into(), "true".into());
    let error = crate::prune_derived_cache(&inv).unwrap_err();
    assert_eq!(error.code, "command.invalid_arguments");
    assert_eq!(fs::read(path).unwrap(), bytes);
    fs::remove_dir_all(f.root).unwrap();
}
