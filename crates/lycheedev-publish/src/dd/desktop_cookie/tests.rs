use super::*;
use lycheedev_core::storage;
use serde_json::{Value, json};

fn database(version: u32) -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    schema(&connection, version);
    connection
}
fn schema(connection: &Connection, version: u32) {
    connection.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE cookies(host_key TEXT,name TEXT,value TEXT,encrypted_value BLOB,expires_utc INTEGER); CREATE TABLE padding(id INTEGER PRIMARY KEY,body BLOB);").unwrap();
    connection
        .execute(
            "INSERT INTO meta VALUES('version',?1)",
            [version.to_string()],
        )
        .unwrap();
}
fn insert(connection: &Connection, plain: &str, encrypted: &[u8], expiry: i64) {
    connection
        .execute(
            "INSERT INTO cookies VALUES('.163.com','NTES_YD_SESS',?1,?2,?3)",
            (plain, encrypted, expiry),
        )
        .unwrap();
}
fn bytes(connection: &Connection) -> Vec<u8> {
    connection.serialize(MAIN_DB).unwrap().to_vec()
}
fn state(key: &[u8]) -> Vec<u8> {
    let mut encrypted = b"DPAPI".to_vec();
    encrypted.extend(credentials::protect_current_user(key, None).unwrap());
    serde_json::to_vec(&json!({"os_crypt":{"encrypted_key":STANDARD.encode(encrypted)}})).unwrap()
}
fn fixtures() -> Vec<Value> {
    serde_json::from_slice::<Value>(include_bytes!("fixtures.json")).unwrap()["cases"]
        .as_array()
        .unwrap()
        .clone()
}
fn decode_value(connection: &Connection, key: &[u8]) -> Result<Cookie> {
    decode(
        &bytes(connection),
        &[],
        &state(key),
        100,
        &OperationControl::default(),
    )
}

#[test]
fn independently_encrypted_v10_values_honor_key_sizes_database_version_and_host_binding() {
    for case in fixtures()
        .into_iter()
        .filter(|case| case["name"] != "wrongHost")
    {
        let connection = database(case["version"].as_u64().unwrap() as u32);
        let cipher = hex::decode(case["encrypted"].as_str().unwrap()).unwrap();
        let key = hex::decode(case["key"].as_str().unwrap()).unwrap();
        insert(&connection, "ignored-plain-value", &cipher, 0);
        let cookie = decode_value(&connection, &key).unwrap();
        assert_eq!(cookie.into_value(), case["cookie"].as_str().unwrap());
    }
}
#[test]
fn authentication_domain_and_application_bound_failures_never_fall_back_or_echo_a_cookie() {
    for case in fixtures()
        .into_iter()
        .filter(|case| case["name"] == "wrongHost" || case["name"] == "bound256")
    {
        let connection = database(24);
        let mut cipher = hex::decode(case["encrypted"].as_str().unwrap()).unwrap();
        if case["name"] == "bound256" {
            *cipher.last_mut().unwrap() ^= 1;
        }
        let key = hex::decode(case["key"].as_str().unwrap()).unwrap();
        insert(
            &connection,
            "synthetic-plaintext-must-not-be-a-fallback",
            &cipher,
            0,
        );
        let error = decode_value(&connection, &key).err().unwrap();
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("synthetic-")
        );
    }
    let connection = database(24);
    insert(
        &connection,
        "synthetic-never-use-this",
        b"v20-application-bound",
        0,
    );
    let error = decode_value(&connection, &[32; 32]).err().unwrap();
    assert!(error.message.contains("application-bound"));
    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .contains("synthetic-never-use-this")
    );
}
#[test]
fn live_cookie_selection_supports_plain_and_current_user_dpapi_without_guessing_ambiguous_rows() {
    for encrypted in [false, true] {
        let connection = database(23);
        insert(&connection, "expired-cookie", &[], 99);
        let cipher = if encrypted {
            credentials::protect_current_user(b"synthetic-live-cookie", None).unwrap()
        } else {
            vec![]
        };
        insert(&connection, "synthetic-live-cookie", &cipher, 101);
        assert_eq!(
            decode_value(&connection, &[16; 16]).unwrap().into_value(),
            "synthetic-live-cookie"
        );
        insert(&connection, "another-live-cookie", &[], 0);
        assert!(decode_value(&connection, &[16; 16]).is_err());
    }
    for value in ["", "unsafe;delimiter", "unsafe\r\nheader", "unsafe\0value"] {
        let connection = database(24);
        insert(&connection, value, &[], 0);
        assert!(decode_value(&connection, &[16; 16]).is_err());
    }
}
#[test]
fn sqlite_wal_snapshots_use_only_complete_verified_commits_and_leave_the_original_files_unchanged()
{
    let dir = crate::test_support::TestDir::new();
    let path = dir.0.join("Cookies");
    let mut connection = Connection::open(&path).unwrap();
    connection.pragma_update(None, "page_size", 1024).unwrap();
    schema(&connection, 24);
    insert(&connection, "older-cookie", &[], 0);
    let mode: String = connection
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    connection
        .pragma_update(None, "wal_autocheckpoint", 0)
        .unwrap();
    connection
        .execute("UPDATE cookies SET value='committed-first'", [])
        .unwrap();
    let database = std::fs::read(&path).unwrap();
    let wal_path = dir.0.join("Cookies-wal");
    let first = std::fs::read(&wal_path).unwrap();
    connection
        .execute("UPDATE cookies SET value='committed-second'", [])
        .unwrap();
    let second = std::fs::read(&wal_path).unwrap();
    assert!(second.len() > first.len());
    let local_state = state(&[32; 32]);
    let control = OperationControl::default();
    let decode_wal = |wal: &[u8]| {
        decode(&database, wal, &local_state, 100, &control)
            .unwrap()
            .into_value()
    };
    assert_eq!(decode_wal(&second), "committed-second");
    assert_eq!(decode_wal(&second[..second.len() - 1]), "committed-first");
    let mut bad_tail = second.clone();
    *bad_tail.last_mut().unwrap() ^= 1;
    assert_eq!(decode_wal(&bad_tail), "committed-first");
    let mut bad_salt = second.clone();
    bad_salt[first.len() + 8] ^= 1;
    assert_eq!(decode_wal(&bad_salt), "committed-first");
    let mut bad_header = second.clone();
    bad_header[24] ^= 1;
    assert!(image::compose(&database, &bad_header, &control).is_err());
    assert_eq!(
        storage::sha256(&std::fs::read(&path).unwrap()),
        storage::sha256(&database)
    );
    assert_eq!(std::fs::read(&wal_path).unwrap(), second);
    // SQLite itself emits valid uncommitted frames when a small page cache spills.
    connection.pragma_update(None, "cache_size", 1).unwrap();
    connection.pragma_update(None, "cache_spill", true).unwrap();
    let transaction = connection.transaction().unwrap();
    transaction
        .execute("UPDATE cookies SET value='uncommitted-never-use'", [])
        .unwrap();
    for index in 0..64 {
        transaction
            .execute("INSERT INTO padding VALUES(?1,zeroblob(1024))", [index])
            .unwrap();
    }
    let uncommitted = std::fs::read(&wal_path).unwrap();
    assert!(
        uncommitted.len() > second.len(),
        "the real SQLite writer must emit the intended uncommitted frames"
    );
    assert_eq!(decode_wal(&uncommitted), "committed-second");
    assert_eq!(std::fs::read(&wal_path).unwrap(), uncommitted);
    transaction.rollback().unwrap();
    connection.close().unwrap();
}
#[test]
fn malformed_snapshots_state_and_original_cancellation_stop_before_a_cookie_is_returned() {
    let connection = database(24);
    insert(&connection, "synthetic-live-cookie", &[], 0);
    let image = bytes(&connection);
    let control = OperationControl::default();
    for invalid in [
        b"{}".as_slice(),
        b"{\"os_crypt\":{},\"os_crypt\":{}}",
        b"{\"os_crypt\":{\"encrypted_key\":\"AAAA\"}}",
    ] {
        let error = decode(&image, &[], invalid, 100, &control).err().unwrap();
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("synthetic-live-cookie")
        );
    }
    assert!(image::compose(b"not a database", &[], &control).is_err());
    assert!(image::compose(&image, b"incomplete WAL", &control).is_err());
    let mut invalid = image.clone();
    invalid[16..18].copy_from_slice(&[0, 3]);
    assert!(image::compose(&invalid, &[], &control).is_err());
    assert!(image::compose(&vec![0; MAX_IMAGE + 1], &[], &control).is_err());
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        decode(&image, &[], &state(&[32; 32]), 100, &control)
            .err()
            .unwrap()
            .exit_code,
        7
    );
}
