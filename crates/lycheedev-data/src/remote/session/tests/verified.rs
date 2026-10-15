//! Actual Session chain fixtures with synthetic CDN objects, not public keys
//! or live network. Core staged publication must be integrated to run these.
use super::*;

fn framed_chunks(raw: &[u8]) -> (String, Vec<u8>) {
    let chunks: Vec<_> = raw
        .chunks(65536)
        .map(|chunk| [b"N".as_slice(), chunk].concat())
        .collect();
    let mut encoded = b"BLTE".to_vec();
    encoded.extend_from_slice(&(12u32 + 24 * chunks.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&[
        15,
        ((chunks.len() >> 16) & 255) as u8,
        ((chunks.len() >> 8) & 255) as u8,
        (chunks.len() & 255) as u8,
    ]);
    for chunk in &chunks {
        encoded.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
        encoded.extend_from_slice(&((chunk.len() - 1) as u32).to_be_bytes());
        encoded.extend_from_slice(&Md5::digest(chunk));
    }
    let key = hash(&encoded);
    for chunk in chunks {
        encoded.extend(chunk);
    }
    (key, encoded)
}
fn consume(file: &mut VerifiedFile) -> Result<(u64, String)> {
    let mut digest = Md5::new();
    let mut bytes = 0u64;
    let mut block = [0u8; 4096];
    loop {
        let n = file.blob.read_checked(&mut block)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        digest.update(&block[..n]);
    }
    Ok((bytes, casc::hex(&digest.finalize())))
}
#[test]
fn independent_verified_cold_warm_sessions_keep_digest_identity_and_go_counters() {
    let f = Fixture::new();
    let mut statistics = Vec::new();
    // Independent query owners match Go NewSession/Close. Reusing one Rust
    // Session still reconstructs Root/Encoding, an inherited separate gap.
    for _ in 0..2 {
        let mut admission = f.admission();
        let mut session = Session::borrowed_with(
            &mut admission,
            &f.pin,
            f.options(),
            &f.budget,
            &f.backend,
            &f.backend,
        )
        .unwrap();
        let (bytes, ckey) = session
            .with_verified(7, &mut |file| {
                assert_eq!(file.entry.file_data_id, 7);
                assert_eq!(file.reading["contentVerified"], true);
                assert_eq!(file.blob.reference().sha256, storage::sha256(&f.payload));
                statistics.push(file.reading["decodedCache"].clone());
                let first = consume(file)?;
                file.blob.rewind()?;
                assert_eq!(consume(file)?, first);
                Ok(first)
            })
            .unwrap();
        assert_eq!(bytes, f.payload.len() as u64);
        assert_eq!(ckey, hash(&f.payload));
        session.close().unwrap();
        admission.finish(Ok(())).unwrap();
    }
    // Each independent query prepares its own Root. Cold publication is not a
    // Go ReadVerifiedBlob event merely because Core scanned its new disk body.
    assert_eq!(statistics[0]["contentDecodeCalls"], 2);
    assert_eq!(statistics[0]["blobReads"], 1);
    assert_eq!(statistics[0]["hits"], 0);
    assert_eq!(statistics[1]["contentDecodeCalls"], 0);
    assert_eq!(statistics[1]["blobReads"], 2);
    assert_eq!(statistics[1]["hits"], 2);
    assert!(f.budget.snapshot().unwrap().network_requests > 0);
    f.control
        .ordered_child(f.control.deadline())
        .unwrap()
        .check()
        .unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn cold_multichunk_verified_body_succeeds_without_payload_sized_retained_allocation() {
    let mut f = Fixture::with_framing(false, None, vec![b'x'; 8 << 20], framed_chunks);
    let mut limits = Cost::defaults();
    limits.retained_bytes = 4 << 20;
    f.budget = Arc::new(Budget::controlled(
        limits,
        f.control.deadline(),
        f.control.clone(),
    ));
    let mut admission = f.admission();
    let mut options = f.options();
    options.content_bytes = 16 << 20;
    let mut session = Session::borrowed_with(
        &mut admission,
        &f.pin,
        options,
        &f.budget,
        &f.backend,
        &f.backend,
    )
    .unwrap();
    let expected = hash(&f.payload);
    session
        .with_verified(7, &mut |file| {
            assert_eq!(consume(file)?, (8 << 20, expected.clone()));
            assert!(f.budget.snapshot()?.retained_bytes < f.payload.len() as u64);
            Ok(())
        })
        .unwrap();
    // This is the internal cold body mode. Asset capture's additional Go
    // logical retained charge is deliberately outside this fixture.
    assert!(f.budget.logical_snapshot().unwrap().retained_bytes < f.payload.len() as u64);
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    assert!(f.budget.snapshot().unwrap().retained_bytes < 4 << 20);
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn verified_ckey_failure_never_publishes_selected_body_or_calls_consumer() {
    let f = Fixture::with_corrupt_ckey(true);
    let mut actual = f.payload.clone();
    actual[0] ^= 1;
    let body = evidence::blob_path(&f.home, &storage::sha256(&actual)).unwrap();
    let mut admission = f.admission();
    let mut session = Session::borrowed_with(
        &mut admission,
        &f.pin,
        f.options(),
        &f.budget,
        &f.backend,
        &f.backend,
    )
    .unwrap();
    let error = session
        .with_verified(7, &mut |_| -> Result<()> { panic!("unverified callback") })
        .unwrap_err();
    assert_eq!(error.code, "records.content_integrity");
    assert_eq!(error.details["publication"]["state"], "not_published");
    assert!(!body.exists());
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn verified_callback_missing_key_error_is_terminal_with_known_published_identity() {
    let f = Fixture::new();
    let mut admission = f.admission();
    let mut session = Session::borrowed_with(
        &mut admission,
        &f.pin,
        f.options(),
        &f.budget,
        &f.backend,
        &f.backend,
    )
    .unwrap();
    let mut calls = 0;
    let error = session
        .with_verified(7, &mut |_| -> Result<()> {
            calls += 1;
            Err(missing_page_key())
        })
        .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(error.code, "container.key_unavailable");
    assert_eq!(error.details["missingKey"], true);
    assert_eq!(error.details["publication"]["state"], "published");
    assert_eq!(
        error.details["publication"]["reference"]["sha256"],
        storage::sha256(&f.payload)
    );
    assert!(!recoverable_missing_key(&error));
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn verified_consumer_cancellation_stops_next_read_and_checks_close_without_replay() {
    let f = Fixture::with_payload(false, None, vec![b'z'; 3 * 65536]);
    let mut admission = f.admission();
    let mut session = Session::borrowed_with(
        &mut admission,
        &f.pin,
        f.options(),
        &f.budget,
        &f.backend,
        &f.backend,
    )
    .unwrap();
    let mut calls = 0;
    let error = session
        .with_verified(7, &mut |file| -> Result<()> {
            calls += 1;
            let mut block = [0; 4096];
            assert_eq!(file.blob.read_checked(&mut block)?, block.len());
            f.control.tracked_cancel_handle()?.cancel();
            file.blob.read_checked(&mut block).map(|_| ())
        })
        .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(
        error.details["publication"]["reference"]["sha256"],
        storage::sha256(&f.payload)
    );
    assert_eq!(session.close().unwrap_err().code, "command.cancelled");
    assert_eq!(
        admission.finish(Ok(())).unwrap_err().code,
        "command.cancelled"
    );
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn staged_key_recovery_rejects_unknown_published_reused_and_composite_failures() {
    for (state, reused, allowed) in [
        ("not_published", false, true),
        ("not_published", true, false),
        ("published", false, false),
        ("unknown", false, false),
    ] {
        let mut error = missing_page_key();
        error.details["publication"] = serde_json::json!({"state":state,"reused":reused});
        assert_eq!(recoverable_missing_key(&error), allowed, "{state}/{reused}");
    }
    let mut nested = missing_page_key();
    nested.details["publication"] = serde_json::json!({
        "state":"not_published", "reused":false,
        "prior":{"state":"not_published", "reused":true},
    });
    assert!(!recoverable_missing_key(&nested));
    let mut error = missing_page_key();
    error.details["cleanup"] = serde_json::json!({"code":"test.close"});
    assert!(!recoverable_missing_key(&error));
}

fn selected_alias(f: &Fixture, session: &Session<'_, '_>) -> String {
    let (_, encoded) = framed(&f.payload);
    let source = &session.keys.as_ref().unwrap().source;
    let mut identity = format!(
        "decoded-v1\0{}\0{}\0{}\0",
        hash(&f.payload),
        source.kind,
        source.sha256
    )
    .into_bytes();
    identity.extend_from_slice(&encoded);
    format!("decoded-v1/{}", storage::sha256(&identity))
}
#[test]
fn verified_cache_native_missing_body_decodes_once_but_wrong_ckey_is_terminal() {
    let f = Fixture::new();
    let mut admission = f.admission();
    let mut session = Session::borrowed_with(
        &mut admission,
        &f.pin,
        f.options(),
        &f.budget,
        &f.backend,
        &f.backend,
    )
    .unwrap();
    let reference = session
        .with_verified(7, &mut |file| Ok(file.blob.reference().clone()))
        .unwrap();
    let body = evidence::blob_path(&f.home, &reference.sha256).unwrap();
    std::fs::remove_file(&body).unwrap();
    session
        .with_verified(7, &mut |file| {
            assert_eq!(file.blob.reference().sha256, reference.sha256);
            assert_eq!(file.reading["decodedCache"]["contentDecodeCalls"], 1);
            assert_eq!(file.reading["decodedCache"]["hits"], 1); // Root only.
            Ok(())
        })
        .unwrap();
    assert!(body.exists());
    let alias = selected_alias(&f, &session);
    {
        let store = &session.files.as_ref().unwrap().store;
        let (mut entry, generation) = store.read::<serde_json::Value>(&alias).unwrap().unwrap();
        let wrong = vec![b'w'; f.payload.len()];
        entry["blob"] =
            serde_json::to_value(store.publish_blob(&wrong, wrong.len()).unwrap()).unwrap();
        store.put(&alias, &entry, Some(generation)).unwrap();
    }
    let error = session
        .with_verified(7, &mut |_| -> Result<()> {
            panic!("corrupt cache must not yield")
        })
        .unwrap_err();
    assert_eq!(error.code, "vault.blob_integrity");
    session.close().unwrap();
    admission.finish(Ok(())).unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
