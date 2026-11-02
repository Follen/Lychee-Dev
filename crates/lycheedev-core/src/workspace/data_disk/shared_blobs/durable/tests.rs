use super::*;
use crate::evidence::CaptureAccounting;
use std::io::{self, Cursor};
struct Meter;
impl CaptureAccounting for Meter {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        Ok(())
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-durable-stage-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        crate::workspace::initialize(&home).unwrap();
        let f = Self(home);
        let control = OperationControl::default();
        let owner = DerivedReservation::acquire_inner(
            None,
            &f.0,
            0,
            AdditionalPeak::default(),
            &control,
            None,
            true,
        )
        .unwrap();
        owner.finish(Ok(())).unwrap();
        f
    }
    fn paths(&self) -> DataCachePaths {
        DataCachePaths::new(&self.0)
    }
    fn limit(&self, bytes: u64) {
        let mut config = crate::workspace::default_config();
        config["dataCache"]["maxBytes"] = serde_json::json!(bytes);
        storage::atomic_json(&self.0.join("config.json"), &config).unwrap();
    }
    fn usage(&self) -> DerivedUsage {
        let c = OperationControl::default();
        let owner = DerivedReservation::acquire_inner(
            None,
            &self.0,
            0,
            AdditionalPeak::default(),
            &c,
            None,
            true,
        )
        .unwrap();
        let value = owner.report.existing;
        owner.finish(Ok(())).unwrap();
        value
    }
    fn seed(
        &self,
        kind: ManagedStageKind,
        slot: u8,
        source: &SourceIdentity,
        body: &[u8],
        prefix: Option<&[u8]>,
    ) -> Declaration {
        let record = Declaration {
            kind,
            slot,
            source: source.clone(),
            expected: body.len() as u64,
            digest: if kind == ManagedStageKind::Shard {
                Sha256::digest(body).into()
            } else {
                [0; 32]
            },
        };
        // Deliberate crash image: bytes are exactly the durable wire protocol,
        // not test-only adoption authority in production.
        fs::write(
            self.paths()
                .shared_blobs()
                .join(record.record_name().unwrap()),
            record.encode().unwrap(),
        )
        .unwrap();
        if let Some(prefix) = prefix {
            let parent = record.payload_parent(&self.0, &self.paths()).unwrap();
            fs::create_dir_all(&parent).unwrap();
            fs::write(parent.join(record.payload_name().unwrap()), prefix).unwrap();
        }
        record
    }
    fn recover(
        &self,
        source: &SourceIdentity,
        reader: &mut impl Read,
        expected: u64,
    ) -> Result<()> {
        recover_spool_once(
            &self.0,
            source,
            reader,
            expected,
            None,
            &OperationControl::default(),
            &|| Ok(()),
            &Meter,
        )
    }
    fn publish(&self, source: &SourceIdentity, body: &[u8]) -> Result<evidence::VerifiedBlob> {
        super::super::stream::publish_shared_blob_stream_identified_with_check(
            &self.0,
            &mut Cursor::new(body),
            body.len() as u64,
            body.len() as u64,
            &OperationControl::default(),
            || Ok(()),
            &Meter,
            source,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn source() -> SourceIdentity {
    SourceIdentity::from_parts(&[b"encoded-sha", b"ckey", b"keys-fingerprint", b"strict"])
}
fn error<T>(value: Result<T>) -> ToolkitError {
    match value {
        Ok(_) => panic!("expected failure"),
        Err(e) => e,
    }
}
#[test]
fn declaration_codec_binds_kind_slot_source_digest_length_and_every_reserved_byte() {
    let f = Fixture::new();
    let record = f.seed(ManagedStageKind::Shard, 3, &source(), b"body", None);
    let raw = record.encode().unwrap();
    assert_eq!(
        Declaration::decode(record.kind, 3, &raw)
            .unwrap()
            .encode()
            .unwrap(),
        raw
    );
    for index in 0..128 {
        let mut altered = raw;
        altered[index] ^= 1;
        assert!(
            Declaration::decode(record.kind, 3, &altered).is_err(),
            "byte {index}"
        );
    }
    assert!(Declaration::decode(ManagedStageKind::Spool, 3, &raw).is_err());
    assert!(Declaration::decode(record.kind, 4, &raw).is_err());
}
#[test]
fn source_identity_is_length_framed_and_separates_policy_and_key_context() {
    assert_ne!(
        SourceIdentity::from_parts(&[b"ab", b"c"]),
        SourceIdentity::from_parts(&[b"a", b"bc"])
    );
    assert_ne!(
        source(),
        SourceIdentity::from_parts(&[
            b"encoded-sha",
            b"ckey",
            b"keys-fingerprint",
            b"table-allow-missing"
        ])
    );
    assert_ne!(
        source(),
        SourceIdentity::from_parts(&[
            b"encoded-sha",
            b"ckey",
            b"other-provided-key-fingerprint",
            b"strict"
        ])
    );
}
#[test]
fn spool_inventory_replaces_actual_sample_and_reserves_missing_payload() {
    let f = Fixture::new();
    let record = f.seed(ManagedStageKind::Spool, 0, &source(), b"body", None);
    assert_eq!(f.usage().temporary_namespace_bytes, 4);
    assert_eq!(f.usage().generation_namespace_bytes, 128);
    let path = f.paths().staging.join(record.payload_name().unwrap());
    for prefix in [b"".as_slice(), b"b", b"bo", b"body"] {
        fs::write(&path, prefix).unwrap();
        assert_eq!(f.usage().temporary_namespace_bytes, 4);
    }
    fs::write(path, b"oversized").unwrap();
    assert_eq!(f.usage().temporary_namespace_bytes, 9);
}
#[test]
fn spool_inventory_refreshes_sparse_case_insensitive_slots_and_retains_torn_records() {
    let f = Fixture::new();
    let high = f.seed(ManagedStageKind::Spool, 63, &source(), b"body", None);
    let records = f.paths().shared_blobs();
    let high_name = high.record_name().unwrap();
    fs::rename(
        records.join(&high_name),
        records.join(high_name.to_ascii_uppercase()),
    )
    .unwrap();
    let usage = f.usage();
    assert_eq!(usage.temporary_namespace_bytes, 4);
    assert_eq!(usage.generation_namespace_bytes, 128);

    f.seed(ManagedStageKind::Spool, 31, &source(), b"new", None);
    let torn =
        records.join(ManagedStageDeclaration::record_name(ManagedStageKind::Spool, 7).unwrap());
    fs::write(&torn, b"torn").unwrap();
    let usage = f.usage();
    assert_eq!(usage.temporary_namespace_bytes, 7);
    assert_eq!(usage.generation_namespace_bytes, 260);
    assert_eq!(fs::read(&torn).unwrap(), b"torn");

    let control = OperationControl::default();
    let owner = DerivedReservation::acquire_inner(
        None,
        &f.0,
        0,
        AdditionalPeak::default(),
        &control,
        None,
        true,
    )
    .unwrap();
    assert_eq!(
        existing_record_slots(
            record_directory(&owner),
            ManagedStageKind::Spool,
            &owner.control
        )
        .unwrap()
        .collect::<Vec<_>>(),
        [7, 31, 63],
    );
    owner.finish(Ok(())).unwrap();
}

#[test]
fn all_spool_slots_at_exact_quota_recover_without_new_spool_or_producer_replay() {
    struct Once {
        bytes: Cursor<Vec<u8>>,
        calls: usize,
        eofs: usize,
    }
    impl Read for Once {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.calls += 1;
            let n = self.bytes.read(b)?;
            if n == 0 {
                self.eofs += 1;
            }
            Ok(n)
        }
    }
    let f = Fixture::new();
    for slot in 0..MANAGED_STAGE_SLOTS {
        f.seed(
            ManagedStageKind::Spool,
            slot,
            &source(),
            b"body",
            Some(b"bo"),
        );
    }
    let before = f.usage().total().unwrap();
    f.limit(before);
    let mut reader = Once {
        bytes: Cursor::new(b"body".to_vec()),
        calls: 0,
        eofs: 0,
    };
    let e = error(f.recover(&source(), &mut reader, 4));
    assert_eq!(e.code, "vault.data_stage_recovered_retry_required");
    assert_eq!(reader.eofs, 1);
    assert_eq!(reader.bytes.position(), 4);
    assert_eq!(f.usage().total().unwrap(), before - 132);
}
#[test]
fn source_only_recovery_rejects_wrong_prefix_without_deleting_record_or_payload() {
    let f = Fixture::new();
    let record = f.seed(ManagedStageKind::Spool, 0, &source(), b"body", Some(b"bad"));
    let before = f.usage();
    assert_eq!(
        error(f.recover(&source(), &mut Cursor::new(b"body"), 4)).code,
        "vault.data_shared_blob_integrity"
    );
    assert_eq!(f.usage(), before);
    assert_eq!(
        fs::read(f.paths().staging.join(record.payload_name().unwrap())).unwrap(),
        b"bad"
    );
}
#[test]
fn producer_business_eof_error_survives_and_never_authorizes_cleanup() {
    struct BadEof(Cursor<&'static [u8]>);
    impl Read for BadEof {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            let n = self.0.read(b)?;
            if n == 0 {
                Err(io::Error::other(ToolkitError::new(
                    "fixture.business_eof",
                    "original business error",
                    4,
                )))
            } else {
                Ok(n)
            }
        }
    }
    let f = Fixture::new();
    f.seed(ManagedStageKind::Spool, 0, &source(), b"body", Some(b"bo"));
    let before = f.usage();
    let e = error(f.recover(&source(), &mut BadEof(Cursor::new(b"body")), 4));
    assert_eq!(e.code, "fixture.business_eof");
    assert_eq!(f.usage(), before);
}
#[test]
fn spool_slot_preserves_foreign_payload_and_keeps_staging_pinned_until_owner_close() {
    let f = Fixture::new();
    let staging = f.paths().staging;
    let foreign =
        staging.join(ManagedStageDeclaration::payload_name(ManagedStageKind::Spool, 0).unwrap());
    fs::write(&foreign, b"foreign").unwrap();
    let control = OperationControl::default();
    let mut owner = DerivedReservation::acquire_inner(
        None,
        &f.0,
        0,
        AdditionalPeak::default(),
        &control,
        None,
        true,
    )
    .unwrap();
    owner.admit_shared(128, 4).unwrap();
    let slot = StageSlot::create_spool(&owner, &source(), 4).unwrap();
    assert_eq!(slot.declaration.slot, 1);
    assert_eq!(fs::read(&foreign).unwrap(), b"foreign");
    assert!(
        !f.paths()
            .shared_blobs()
            .join(ManagedStageDeclaration::record_name(ManagedStageKind::Spool, 0).unwrap())
            .exists()
    );
    slot.finish(&owner, true).unwrap();
    let moved = staging.with_extension("moved");
    assert!(fs::rename(&staging, &moved).is_err());
    owner.finish(Ok(())).unwrap();
    fs::rename(&staging, &moved).unwrap();
    assert_eq!(
        fs::read(moved.join(foreign.file_name().unwrap())).unwrap(),
        b"foreign",
    );
}

#[test]
fn active_slot_is_skipped_without_consuming_source_or_inferring_pid_liveness() {
    let f = Fixture::new();
    let c = OperationControl::default();
    let mut owner =
        DerivedReservation::acquire_inner(None, &f.0, 0, AdditionalPeak::default(), &c, None, true)
            .unwrap();
    owner.admit_shared(128, 4).unwrap();
    let slot = StageSlot::create_spool(&owner, &source(), 4).unwrap();
    owner.finish(Ok(())).unwrap();
    let mut source_bytes = Cursor::new(b"body");
    f.recover(&source(), &mut source_bytes, 4).unwrap();
    assert_eq!(source_bytes.position(), 0);
    slot.release().unwrap();
    assert_eq!(
        error(f.recover(&source(), &mut source_bytes, 4)).code,
        "vault.data_stage_recovered_retry_required"
    );
}
#[test]
fn torn_declaration_remains_occupied_and_physically_counted_without_delete_authority() {
    let f = Fixture::new();
    let name = ManagedStageDeclaration::record_name(ManagedStageKind::Spool, 0).unwrap();
    let path = f.paths().shared_blobs().join(name);
    fs::write(&path, b"LDST").unwrap();
    let mut body = Cursor::new(b"body");
    f.recover(&source(), &mut body, 4).unwrap();
    assert_eq!(body.position(), 0);
    assert_eq!(f.usage().generation_namespace_bytes, 4);
    assert_eq!(fs::read(&path).unwrap(), b"LDST");
}
#[test]
fn shard_only_missing_body_recovery_withdraws_pending_intent_then_new_request_publishes() {
    let f = Fixture::new();
    let record = f.seed(ManagedStageKind::Shard, 0, &source(), b"body", Some(b"bo"));
    let reference = BlobRef {
        sha256: hex::encode(record.digest),
        bytes: 4,
    };
    let raw = intent(&reference).unwrap();
    fs::write(
        f.paths()
            .shared_blobs()
            .join(format!("{}.intent", reference.sha256)),
        raw,
    )
    .unwrap();
    let before = f.usage().total().unwrap();
    f.limit(before);
    assert_eq!(
        error(f.recover(&source(), &mut Cursor::new(b"body"), 4)).code,
        "vault.data_stage_recovered_retry_required"
    );
    assert_eq!(f.usage().total().unwrap(), 0);
    f.limit(3 * 4 + 256 + 80);
    f.publish(&source(), b"body").unwrap().close().unwrap();
    assert_eq!(f.usage().total().unwrap(), 84);
}
#[test]
fn completed_attribution_is_never_removed_by_shard_recovery() {
    let f = Fixture::new();
    let record = f.seed(ManagedStageKind::Shard, 0, &source(), b"body", Some(b"bo"));
    let reference = BlobRef {
        sha256: hex::encode(record.digest),
        bytes: 4,
    };
    let raw = intent(&reference).unwrap();
    let records = f.paths().shared_blobs();
    fs::write(records.join(format!("{}.intent", reference.sha256)), raw).unwrap();
    fs::write(
        records.join(format!("{}.complete", reference.sha256)),
        completion(&raw),
    )
    .unwrap();
    let body = evidence::blob_path(&f.0, &reference.sha256).unwrap();
    fs::write(body, b"body").unwrap();
    assert_eq!(
        error(f.recover(&source(), &mut Cursor::new(b"body"), 4)).code,
        "vault.data_stage_recovered_retry_required"
    );
    assert_eq!(f.usage().total().unwrap(), 84);
    assert_eq!(
        fs::read(evidence::blob_path(&f.0, &reference.sha256).unwrap()).unwrap(),
        b"body"
    );
}
#[test]
fn named_shard_slot_reuses_same_declaration_without_new_record_headroom() {
    let f = Fixture::new();
    let record = f.seed(ManagedStageKind::Shard, 63, &source(), b"body", None);
    let reference = BlobRef {
        sha256: hex::encode(record.digest),
        bytes: 4,
    };
    let c = OperationControl::default();
    let owner =
        DerivedReservation::acquire_inner(None, &f.0, 0, AdditionalPeak::default(), &c, None, true)
            .unwrap();
    let before = owner.report.existing;
    let slot = StageSlot::create_shard(&owner, &source(), &reference).unwrap();
    assert_eq!(slot.name.as_string(), record.payload_name().unwrap());
    slot.release().unwrap();
    owner.finish(Ok(())).unwrap();
    assert_eq!(f.usage(), before);
}
#[test]
fn fresh_identified_stream_admits_declarations_and_peak_before_any_payload_write() {
    let f = Fixture::new();
    f.limit(131);
    let e = error(f.publish(&source(), b"body"));
    assert_eq!(e.code, "vault.data_cache_budget_exceeded");
    assert_eq!(f.usage().total().unwrap(), 0);
    f.limit(3 * 4 + 256 + 80);
    let blob = f.publish(&source(), b"body").unwrap();
    blob.close().unwrap();
    let usage = f.usage();
    assert_eq!(usage.total().unwrap(), 84);
    assert_eq!(usage.retained_shared_peak_bytes, 0);
    assert_eq!(usage.temporary_namespace_bytes, 0);
}

#[test]
fn existing_source_cas_first_identified_data_attribution_completes() {
    let f = Fixture::new();
    let body = b"body";
    evidence::publish_blob_bounded(&f.0, body, body.len(), &OperationControl::default()).unwrap();
    assert_eq!(f.usage().total().unwrap(), 0);
    f.limit(3 * 4 + 256 + 80);
    f.publish(&source(), body).unwrap().close().unwrap();
    let usage = f.usage();
    assert_eq!(usage.total().unwrap(), 84);
    assert_eq!(usage.retained_shared_peak_bytes, 0);
    assert_eq!(usage.temporary_namespace_bytes, 0);
}

#[test]
fn declaration_only_memory_crash_retries_at_original_exact_peak() {
    let f = Fixture::new();
    let body = b"body";
    let reference = BlobRef {
        sha256: storage::sha256(body),
        bytes: body.len() as u64,
    };
    let record = f.seed(
        ManagedStageKind::Shard,
        63,
        &SourceIdentity::memory(&reference),
        body,
        None,
    );
    let records = f.paths().shared_blobs();
    let name = record.record_name().unwrap();
    let retained = records.join(name.to_ascii_uppercase());
    fs::rename(records.join(&name), &retained).unwrap();
    f.limit(128 + 80 + 2 * 4);
    let actual = super::super::publish_shared_blob_with_check(
        &f.0,
        body,
        body.len(),
        &OperationControl::default(),
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(actual.sha256, reference.sha256);
    assert_eq!(f.usage().total().unwrap(), 84);
    assert!(!retained.exists());
}

#[test]
fn declaration_only_verified_enrollment_reuses_original_exact_peak_without_body_copy() {
    let f = Fixture::new();
    let body = b"body";
    let control = OperationControl::default();
    let raw = evidence::publish_blob_bounded(&f.0, body, body.len(), &control).unwrap();
    let blob = evidence::open_verified_blob_bounded(&f.0, &raw, 4, &control, |_| Ok(())).unwrap();
    f.seed(
        ManagedStageKind::Shard,
        0,
        &SourceIdentity::memory(blob.reference()),
        body,
        None,
    );
    f.limit(128 + 80 + 2 * 4);
    let mut owner = DerivedReservation::acquire_inner(
        None,
        &f.0,
        0,
        AdditionalPeak::default(),
        &control,
        None,
        true,
    )
    .unwrap();
    let result = super::super::enroll_verified_with_owner(&mut owner, &blob);
    owner.finish(result).unwrap();
    blob.close().unwrap();
    assert_eq!(f.usage().total().unwrap(), 84);
}

#[test]
fn torn_unrelated_shard_slot_does_not_block_remaining_slots_or_lose_its_charge() {
    let f = Fixture::new();
    let name = ManagedStageDeclaration::record_name(ManagedStageKind::Shard, 0).unwrap();
    let path = f.paths().shared_blobs().join(name);
    fs::write(&path, b"torn").unwrap();
    f.publish(&source(), b"body").unwrap().close().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"torn");
    assert_eq!(f.usage().total().unwrap(), 88);
}
