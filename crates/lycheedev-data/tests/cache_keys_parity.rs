use lycheedev_core::{Invocation, evidence, metadata::Metadata, storage, workspace};
use lycheedev_data::{
    casc,
    decoded_cache::{self, Stats},
    keys::{self, KeySet},
    metadata_format::EncodingRecord,
};
use md5::{Digest, Md5};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
#[test]
fn failed_nested_decode_and_key_loading_release_only_temporary_ownership() {
    use lycheedev_data::resource::{Budget, Cost};
    let home = home();
    let inv = invocation(home.clone());
    let budget = Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(10),
        None,
    ));
    let mut keys =
        keys::Context::prepare_budgeted(&inv, budget.deadline, Some(budget.clone())).unwrap();
    let mut encrypted = vec![b'E', 8];
    encrypted.extend(42u64.to_le_bytes());
    encrypted.push(4);
    encrypted.extend([0; 4]);
    encrypted.push(b'S');
    encrypted.extend([0; 6]);
    let frame = |payload: &[u8], decoded: u32| {
        let mut raw = b"BLTE".to_vec();
        raw.extend(36u32.to_be_bytes());
        raw.extend([15, 0, 0, 1]);
        raw.extend((payload.len() as u32).to_be_bytes());
        raw.extend(decoded.to_be_bytes());
        raw.extend(Md5::digest(payload));
        raw.extend(payload);
        raw
    };
    let mut nested = vec![b'F'];
    nested.extend(frame(&encrypted, 5));
    let encoded = frame(&nested, 5);
    let object = casc::LocalObject {
        index: "owned fixture".into(),
        encoding_key: casc::hex(&Md5::digest(&encoded)),
        span: casc::Span {
            archive: 0,
            offset: 0,
            bytes: encoded.len() as u64 + 30,
        },
        encoded,
    };
    let record = EncodingRecord {
        content_key: casc::hex(&Md5::digest(b"hello")),
        decoded_bytes: 5,
        encoding_keys: vec![],
        page: 0,
    };
    let mut stats = Stats::default();
    let error = decoded_cache::decode_budgeted(
        &home,
        &object,
        &record,
        Default::default(),
        &mut keys,
        &mut stats,
        Some(&budget),
    )
    .unwrap_err();
    assert_eq!(error.code, "container.key_unavailable");
    assert_eq!(keys.source.state, "unavailable_offline");
    // Missing public document's conservative metadata admission remains; the
    // failed nested output/scratch reservation was actually released.
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        (keys::PUBLIC_BYTES * 8 + 65536) as u64
    );
    assert!(budget.snapshot().unwrap().decode_work > 0);
    assert_eq!(stats.content_decode_calls, 1);
    let mut corrupted = object;
    *corrupted.encoded.last_mut().unwrap() ^= 1;
    let private = home.join("empty.json");
    std::fs::write(
        &private,
        br#"{"000000000000002b":"000102030405060708090a0b0c0d0e0f"}"#,
    )
    .unwrap();
    let mut private_inv = inv;
    private_inv
        .flags
        .insert("--key-file".into(), private.to_string_lossy().into());
    let mut keys = keys::Context::prepare(&private_inv, budget.deadline).unwrap();
    let retained = budget.snapshot().unwrap().retained_bytes;
    assert_eq!(
        decoded_cache::decode_budgeted(
            &home,
            &corrupted,
            &record,
            Default::default(),
            &mut keys,
            &mut stats,
            Some(&budget)
        )
        .unwrap_err()
        .code,
        "container.integrity_mismatch"
    );
    assert_eq!(budget.snapshot().unwrap().retained_bytes, retained);
    let cancelled = Budget::new(
        Cost::defaults(),
        budget.deadline,
        Some(Arc::new(AtomicBool::new(true))),
    );
    assert_eq!(
        decoded_cache::decode_budgeted(
            &home,
            &corrupted,
            &record,
            Default::default(),
            &mut keys,
            &mut stats,
            Some(&cancelled)
        )
        .unwrap_err()
        .exit_code,
        7
    );
    std::fs::remove_dir_all(home).unwrap();
}
fn home() -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "lychee-cache-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    workspace::initialize(&home).unwrap();
    home
}
fn invocation(home: PathBuf) -> Invocation {
    Invocation {
        command: "asset inspect".into(),
        control: Default::default(),
        positional: None,
        flags: BTreeMap::from([("--offline".into(), "true".into())]),
        home: home.clone(),
        cwd: home,
    }
}
#[test]
fn key_document_duplicate_hash_bounds_and_redaction() {
    for raw in [
        b"000000000000002A 000102030405060708090A0B0C0D0E0F\n".as_slice(),
        br#"{"000000000000002a":"000102030405060708090a0b0c0d0e0f"}"#.as_slice(),
    ] {
        let format = if raw[0] == b'{' { "json" } else { "text" };
        let set = KeySet::parse(raw, format, &storage::sha256(raw)).unwrap();
        assert_eq!(set.keys()[&42].len(), 16);
        assert_eq!(format!("{set:?}"), "records.KeySet(redacted)");
        assert_eq!(
            KeySet::parse(raw, format, &"a".repeat(64))
                .unwrap_err()
                .code,
            "container.integrity"
        );
    }
    for raw in [
        r#"{"000000000000002a":"000102030405060708090a0b0c0d0e0f","000000000000002A":"000102030405060708090a0b0c0d0e0f"}"#,
        r#"{"000000000000002a":123}"#,
        r#"{"000000000000002a":"secret-not-a-key"}"#,
        r#"{} trailing"#,
    ] {
        let error =
            KeySet::parse(raw.as_bytes(), "json", &storage::sha256(raw.as_bytes())).unwrap_err();
        assert!(!error.message.contains("secret-not-a-key"));
    }
}
#[test]
fn explicit_private_key_identity_is_request_only_and_public_missing_is_precise() {
    let home = home();
    let path = home.join("private.json");
    std::fs::write(
        &path,
        br#"{"000000000000002a":"000102030405060708090a0b0c0d0e0f"}"#,
    )
    .unwrap();
    let mut inv = invocation(home.clone());
    inv.flags
        .insert("--key-file".into(), path.to_string_lossy().into_owned());
    let context = keys::Context::prepare(&inv, Instant::now() + Duration::from_secs(5)).unwrap();
    assert_eq!(context.source.kind, "file");
    assert_eq!(context.keys()[&42].len(), 16);
    assert_eq!(std::fs::read_dir(home.join("blobs")).unwrap().count(), 0);
    assert_eq!(
        context.source.sha256,
        storage::sha256(&std::fs::read(&path).unwrap())
    );
    inv.flags.remove("--key-file");
    let mut context =
        keys::Context::prepare(&inv, Instant::now() + Duration::from_secs(5)).unwrap();
    assert_eq!(context.source.state, "not_needed");
    assert_eq!(
        context.ensure_public().unwrap_err().code,
        "container.key_unavailable"
    );
    assert_eq!(context.source.state, "unavailable_offline");
    std::fs::remove_dir_all(home).unwrap();
}
#[test]
fn decoded_reuse_rehashes_selected_encoded_and_cached_content() {
    let home = home();
    let inv = invocation(home.clone());
    let mut keys = keys::Context::prepare(&inv, Instant::now() + Duration::from_secs(10)).unwrap();
    let data = b"authenticated fixture content";
    let mut encoded = b"BLTE\0\0\0\0N".to_vec();
    encoded.extend_from_slice(data);
    let record = EncodingRecord {
        content_key: casc::hex(&Md5::digest(data)),
        decoded_bytes: data.len() as u64,
        encoding_keys: vec![],
        page: 0,
    };
    let mut object = casc::LocalObject {
        encoding_key: casc::hex(&Md5::digest(&encoded)),
        span: casc::Span {
            archive: 0,
            offset: 0,
            bytes: encoded.len() as u64 + 30,
        },
        index: "fixture.idx".into(),
        encoded,
    };
    let mut stats = Stats::default();
    assert_eq!(
        decoded_cache::decode(
            &home,
            &object,
            &record,
            Default::default(),
            &mut keys,
            &mut stats
        )
        .unwrap()
        .bytes,
        data
    );
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.content_decode_calls, 1);
    let mut stats = Stats::default();
    decoded_cache::decode(
        &home,
        &object,
        &record,
        Default::default(),
        &mut keys,
        &mut stats,
    )
    .unwrap();
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.content_decode_calls, 0);
    assert_eq!(stats.content_hash_bytes, object.encoded.len() as u64);
    assert_eq!(stats.blob_read_bytes, data.len() as u64);
    object.encoded[9] ^= 1;
    assert_eq!(
        decoded_cache::decode(
            &home,
            &object,
            &record,
            Default::default(),
            &mut keys,
            &mut stats
        )
        .unwrap_err()
        .code,
        "records.content_integrity"
    );
    object.encoded[9] ^= 1;
    let digest = storage::sha256(data);
    let blob = evidence::blob_path(&home, &digest).unwrap();
    std::fs::write(&blob, vec![b'x'; data.len()]).unwrap();
    assert_eq!(
        decoded_cache::decode(
            &home,
            &object,
            &record,
            Default::default(),
            &mut keys,
            &mut stats
        )
        .unwrap_err()
        .code,
        "vault.blob_integrity"
    );
    std::fs::remove_file(&blob).unwrap();
    assert_eq!(
        decoded_cache::decode(
            &home,
            &object,
            &record,
            Default::default(),
            &mut keys,
            &mut stats
        )
        .unwrap()
        .bytes,
        data
    );
    let mut material = format!(
        "decoded-v1\0{}\0public\0{}\0",
        record.content_key,
        keys::PUBLIC_SHA256
    )
    .into_bytes();
    material.extend_from_slice(&object.encoded);
    let key = format!("decoded-v1/{}", storage::sha256(&material));
    let mut metadata = Metadata::open(&home, true).unwrap();
    let mut value: serde_json::Value = metadata.read(&key).unwrap();
    value["contentKey"] = json!("a".repeat(32));
    let generation = metadata.generation(&key).unwrap();
    metadata.put(&key, &value, generation).unwrap();
    assert_eq!(
        decoded_cache::decode(
            &home,
            &object,
            &record,
            Default::default(),
            &mut keys,
            &mut stats
        )
        .unwrap_err()
        .code,
        "vault.blob_integrity"
    );
    drop(metadata);
    std::fs::remove_dir_all(home).unwrap();
}
