use super::*;
use std::fs;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lychee-managed-stage-{}",
            crate::random::token().unwrap()
        ));
        fs::create_dir_all(path.join("shared-blobs-v1")).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn bytes(kind: ManagedStageKind, slot: u8) -> [u8; 128] {
    use sha2::{Digest, Sha256};
    let mut bytes = [0; 128];
    bytes[..8].copy_from_slice(b"LDST0002");
    bytes[8] = match kind {
        ManagedStageKind::Spool => 1,
        ManagedStageKind::Shard => 2,
        ManagedStageKind::ExternalExport => 3,
    };
    bytes[9] = slot;
    let hash = Sha256::digest(&bytes[..96]);
    bytes[96..].copy_from_slice(&hash);
    bytes
}
#[test]
fn declaration_is_durable_before_named_payload_and_drop_does_not_erase_it() {
    let f = Fixture::new();
    let records = Directory::open(&f.0.join("shared-blobs-v1")).unwrap();
    let payloads = Directory::open(&f.0).unwrap();
    let mut declaration =
        ManagedStageDeclaration::create(&records, ManagedStageKind::Spool, 0, &mut Uncontrolled)
            .unwrap();
    let token = declaration
        .persist(&bytes(ManagedStageKind::Spool, 0), &mut Uncontrolled)
        .unwrap();
    drop(declaration);
    let record =
        f.0.join("shared-blobs-v1")
            .join(ManagedStageDeclaration::record_name(ManagedStageKind::Spool, 0).unwrap());
    assert_eq!(
        fs::read(&record).unwrap(),
        bytes(ManagedStageKind::Spool, 0)
    );
    let spool = PrivateSpool::create_predeclared(&payloads, &token, &mut Uncontrolled).unwrap();
    assert_eq!(spool.name(), token.as_string());
    assert!(spool.close().is_empty());
    payloads.close().unwrap();
    records.close().unwrap();
    assert!(record.exists());
}
#[test]
fn finite_namespace_refuses_slot_64_and_occupied_record_without_adoption() {
    let f = Fixture::new();
    let records = Directory::open(&f.0.join("shared-blobs-v1")).unwrap();
    assert!(
        ManagedStageDeclaration::create(&records, ManagedStageKind::Spool, 64, &mut Uncontrolled)
            .is_err()
    );
    let name = ManagedStageDeclaration::record_name(ManagedStageKind::Spool, 0).unwrap();
    fs::write(records.path().join(&name), b"foreign").unwrap();
    assert!(
        ManagedStageDeclaration::create(&records, ManagedStageKind::Spool, 0, &mut Uncontrolled)
            .is_err()
    );
    assert_eq!(fs::read(records.path().join(name)).unwrap(), b"foreign");
    records.close().unwrap();
}
#[test]
fn logical_spool_contains_only_successfully_written_prefix_never_zero_tail() {
    let f = Fixture::new();
    let records = Directory::open(&f.0.join("shared-blobs-v1")).unwrap();
    let payloads = Directory::open(&f.0).unwrap();
    let mut declaration =
        ManagedStageDeclaration::create(&records, ManagedStageKind::Spool, 0, &mut Uncontrolled)
            .unwrap();
    let token = declaration
        .persist(&bytes(ManagedStageKind::Spool, 0), &mut Uncontrolled)
        .unwrap();
    let mut spool = PrivateSpool::create_predeclared(&payloads, &token, &mut Uncontrolled).unwrap();
    spool.limit_exact_len(4).unwrap();
    assert_eq!(fs::metadata(f.0.join(spool.name())).unwrap().len(), 0);
    spool.write_all(b"ab").unwrap();
    assert_eq!(fs::metadata(f.0.join(spool.name())).unwrap().len(), 2);
    assert!(spool.finish_writes().is_err());
    assert!(spool.write_all(b"cde").is_err());
    spool.write_all(b"cd").unwrap();
    spool.finish_writes().unwrap();
    let mut all = Vec::new();
    spool.read_to_end(&mut all).unwrap();
    assert_eq!(all, b"abcd");
    spool.rewind_read_ready().unwrap();
    assert_eq!(spool.read(&mut [0; 1]).unwrap(), 1);
    assert!(spool.close().is_empty());
    assert!(declaration.withdraw().is_empty());
    payloads.close().unwrap();
    records.close().unwrap();
}
#[test]
fn stopped_persistence_retains_bounded_record_without_payload_authorization() {
    struct Stop(usize);
    impl FileControl for Stop {
        type Error = io::Error;
        fn charge(&mut self, _: FileCost) -> io::Result<()> {
            Ok(())
        }
        fn check(&mut self) -> io::Result<()> {
            self.0 += 1;
            if self.0 == 2 {
                Err(io::Error::other("first persistence stop"))
            } else {
                Ok(())
            }
        }
    }
    let f = Fixture::new();
    let records = Directory::open(&f.0.join("shared-blobs-v1")).unwrap();
    let mut declaration =
        ManagedStageDeclaration::create(&records, ManagedStageKind::Spool, 0, &mut Uncontrolled)
            .unwrap();
    assert!(
        declaration
            .persist(&bytes(ManagedStageKind::Spool, 0), &mut Stop(0))
            .is_err()
    );
    drop(declaration);
    let name = ManagedStageDeclaration::record_name(ManagedStageKind::Spool, 0).unwrap();
    assert!(records.path().join(name).exists());
    assert!(
        !f.0.join(ManagedStageDeclaration::payload_name(ManagedStageKind::Spool, 0).unwrap())
            .exists()
    );
    records.close().unwrap();
}
#[test]
fn restored_name_checks_checksum_kind_and_bound_and_never_overwrites_payload() {
    let good = bytes(ManagedStageKind::Shard, 63);
    assert!(ManagedStageName::from_declaration(&good).is_ok());
    for index in [0, 8, 9, 16, 24, 56, 96, 127] {
        let mut bad = good;
        bad[index] ^= 0xff;
        assert!(ManagedStageName::from_declaration(&bad).is_err());
    }
    let f = Fixture::new();
    let payloads = Directory::open(&f.0).unwrap();
    let name = ManagedStageName::from_declaration(&good).unwrap();
    fs::write(f.0.join(name.as_string()), b"protected").unwrap();
    assert!(StagedFile::create_predeclared(&payloads, &name, &mut Uncontrolled).is_err());
    assert_eq!(fs::read(f.0.join(name.as_string())).unwrap(), b"protected");
    payloads.close().unwrap();
}
#[test]
fn withdrawal_deletes_only_original_retained_declaration() {
    let f = Fixture::new();
    let records = Directory::open(&f.0.join("shared-blobs-v1")).unwrap();
    let mut declaration =
        ManagedStageDeclaration::create(&records, ManagedStageKind::Shard, 0, &mut Uncontrolled)
            .unwrap();
    declaration
        .persist(&bytes(ManagedStageKind::Shard, 0), &mut Uncontrolled)
        .unwrap();
    fs::write(records.path().join("foreign"), b"keep").unwrap();
    assert!(declaration.withdraw().is_empty());
    assert!(
        !records
            .path()
            .join(ManagedStageDeclaration::record_name(ManagedStageKind::Shard, 0).unwrap())
            .exists()
    );
    assert_eq!(fs::read(records.path().join("foreign")).unwrap(), b"keep");
    records.close().unwrap();
}

#[test]
fn external_export_declaration_never_authorizes_a_payload() {
    let f = Fixture::new();
    let records = Directory::open(&f.0.join("shared-blobs-v1")).unwrap();
    let payloads = Directory::open(&f.0).unwrap();
    let kind = ManagedStageKind::ExternalExport;
    let mut declaration =
        ManagedStageDeclaration::create(&records, kind, 0, &mut Uncontrolled).unwrap();
    let raw = bytes(kind, 0);
    let name = declaration.persist(&raw, &mut Uncontrolled).unwrap();
    assert!(ManagedStageDeclaration::payload_name(kind, 0).is_err());
    assert!(StagedFile::create_predeclared(&payloads, &name, &mut Uncontrolled).is_err());
    assert!(PrivateSpool::create_predeclared(&payloads, &name, &mut Uncontrolled).is_err());
    assert_eq!(fs::read_dir(payloads.path()).unwrap().count(), 1);
    assert!(declaration.withdraw().is_empty());
    assert_eq!(fs::read_dir(records.path()).unwrap().count(), 0);
    payloads.close().unwrap();
    records.close().unwrap();
}
