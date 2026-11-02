use lycheedev_core::{storage, workspace};
use lycheedev_data::keys::{self, KeySet};
use std::{path::PathBuf, time::{Duration, Instant}};

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
fn context(home: PathBuf) -> lycheedev_data::request::DataContext {
    lycheedev_data::request::DataContext {
        control: Default::default(),
        source: lycheedev_data::request::SourceOptions { offline: true, ..Default::default() },
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
    let mut inv = context(home.clone());
    inv.source.key_file = Some(path.to_string_lossy().into_owned());
    let context = keys::Context::prepare_scoped(&inv.home, inv.source.key_file.as_deref(), inv.source.offline, &inv.control, Instant::now() + Duration::from_secs(5), None).unwrap();
    assert_eq!(context.source.kind, "file");
    assert_eq!(context.keys()[&42].len(), 16);
    assert_eq!(std::fs::read_dir(home.join("blobs")).unwrap().count(), 0);
    assert_eq!(
        context.source.sha256,
        storage::sha256(&std::fs::read(&path).unwrap())
    );
    inv.source.key_file = None;
    let mut context =
        keys::Context::prepare_scoped(&inv.home, inv.source.key_file.as_deref(), inv.source.offline, &inv.control, Instant::now() + Duration::from_secs(5), None).unwrap();
    assert_eq!(context.source.state, "not_needed");
    assert_eq!(
        context.ensure_public().unwrap_err().code,
        "container.key_unavailable"
    );
    assert_eq!(context.source.state, "unavailable_offline");
    std::fs::remove_dir_all(home).unwrap();
}
