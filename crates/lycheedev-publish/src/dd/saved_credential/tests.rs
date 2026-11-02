use super::*;
use crate::test_support::TestDir;
use serde_json::json;
use std::fs;
fn database(
    dir: &TestDir,
    selected: &str,
    method: i64,
    modifier: i64,
    credential_type: i64,
    name: &str,
    cipher: &[u8],
) -> std::path::PathBuf {
    let path = dir.0.join("synthetic-cred.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE account(type INTEGER NOT NULL,name TEXT NOT NULL,flag INTEGER NOT NULL,cred_type INTEGER,cred BLOB,PRIMARY KEY(name,type));CREATE TABLE config(key TEXT PRIMARY KEY,value);").unwrap();
    db.execute("INSERT INTO config VALUES ('auto_account',?1)", [selected])
        .unwrap();
    db.execute(
        "INSERT INTO account VALUES (?1,?2,?3,?4,?5)",
        (method, name, modifier, credential_type, cipher),
    )
    .unwrap();
    db.close().unwrap();
    path
}
fn encrypted(value: &[u8]) -> Vec<u8> {
    credentials::protect_current_user(value, Some(ENTROPY)).unwrap()
}
#[test]
fn typed_account_selection_preserves_each_supported_credential_mode_without_name_heuristics() {
    let control = OperationControl::default();
    for (method, modifier, ctype, name) in [
        (1, 0, 1, "12345678900"),
        (2, 0, 2, "name@synthetic.invalid"),
        (2, 1, 2, "mobile-password"),
        (2, 4, 2, "mobile-uplink"),
    ] {
        let dir = TestDir::new();
        let path = database(
            &dir,
            &json!([method, name]).to_string(),
            method,
            modifier,
            ctype,
            name,
            &encrypted(b"synthetic-original-token"),
        );
        let before = fs::read(&path).unwrap();
        let saved = SavedCredential::from_database(&path, &control).unwrap();
        assert_eq!(saved.token, "synthetic-original-token");
        assert_eq!(saved.account, name);
        assert_eq!(
            saved.cc_username(),
            if method == 1 {
                name.to_owned()
            } else {
                format!("{name}@mobile.163.com")
            }
        );
        assert_eq!(
            saved.login_type(),
            if method == 1 {
                "ursMailToken"
            } else {
                "ursPhoneTokenNew"
            }
        );
        assert_eq!(saved.kind() == CredentialKind::Email, method == 1);
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
#[test]
fn unsupported_selected_account_never_adopts_another_valid_account_or_modifies_vendor_state() {
    let control = OperationControl::default();
    for (method, modifier, ctype) in [(1, 1, 1), (1, 0, 2), (2, 2, 2), (2, 0, 1), (3, 0, 3)] {
        let dir = TestDir::new();
        let path = database(
            &dir,
            &json!([method, "selected-account"]).to_string(),
            method,
            modifier,
            ctype,
            "selected-account",
            &encrypted(b"synthetic-selected-token"),
        );
        let db = Connection::open(&path).unwrap();
        db.execute(
            "INSERT INTO account VALUES (1,'other@synthetic.invalid',0,1,?1)",
            [encrypted(b"synthetic-other-token")],
        )
        .unwrap();
        db.close().unwrap();
        let before = fs::read(&path).unwrap();
        let error = SavedCredential::from_database(&path, &control)
            .err()
            .unwrap();
        assert_eq!(error.exit_code, 3);
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("synthetic-selected-token")
        );
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
#[test]
fn absent_corrupt_or_wrong_entropy_credentials_do_not_repair_or_leak_the_saved_state() {
    let control = OperationControl::default();
    let dir = TestDir::new();
    assert!(SavedCredential::from_database(&dir.0.join("absent.db"), &control).is_err());
    assert!(!dir.0.join("absent.db").exists());
    for (selected, cipher) in [
        ("null".to_owned(), encrypted(b"private-payload")),
        (
            r#"[1,"different-selected-account"]"#.into(),
            encrypted(b"private-payload"),
        ),
        (
            r#"[1,"selected-account",3]"#.into(),
            encrypted(b"private-payload"),
        ),
        (
            r#"[1,"selected-account"]"#.into(),
            credentials::protect_current_user(b"private-payload", Some(b"another-application"))
                .unwrap(),
        ),
        (r#"[1,"selected-account"]"#.into(), encrypted(b"")),
        (r#"[1,"selected-account"]"#.into(), encrypted(&[255, 254])),
    ] {
        let dir = TestDir::new();
        let path = database(&dir, &selected, 1, 0, 1, "selected-account", &cipher);
        let before = fs::read(&path).unwrap();
        let error = SavedCredential::from_database(&path, &control)
            .err()
            .unwrap();
        assert_eq!(error.exit_code, 3);
        let public = serde_json::to_string(&error).unwrap();
        assert!(!public.contains("private-payload"));
        assert!(!public.contains("selected-account"));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
#[test]
fn selected_name_is_bound_as_sql_data_and_cancellation_stops_before_opening_a_database() {
    let control = OperationControl::default();
    let dir = TestDir::new();
    let name = "account' OR 1=1 --";
    let path = database(
        &dir,
        &json!([1, name]).to_string(),
        1,
        0,
        1,
        name,
        &encrypted(b"synthetic-selected-token"),
    );
    assert_eq!(
        SavedCredential::from_database(&path, &control)
            .unwrap()
            .account,
        name
    );
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        SavedCredential::from_database(&dir.0.join("absent.db"), &control)
            .err()
            .unwrap()
            .exit_code,
        7
    );
    assert!(!dir.0.join("absent.db").exists());
}
