//! NOT_RUN. Go3057a966 internal/vault/workspace.go:42–73,158–174.
use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "lycheedev-workspace-open-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn write(&self, schema: &str, id: &str, time: &str) {
        fs::write(
            self.0.join("workspace.json"),
            serde_json::to_vec(&json!({"schema":schema,"workspaceId":id,"createdAt":time}))
                .unwrap(),
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn workspace_open_accepts_uppercase_hex_without_normalizing_identity() {
    let f = Fixture::new();
    let id = "ABCDEF0123456789ABCDEF0123456789";
    assert_eq!(id.len(), 32);
    f.write("lycheedev.workspace.v1", id, "2026-10-15T00:00:00Z");
    assert_eq!(open(&f.0).unwrap()["workspaceId"], id);
    let oversized = format!("{id}AB");
    assert_eq!(oversized.len(), 34);
    f.write("lycheedev.workspace.v1", &oversized, "2026-10-15T00:00:00Z");
    assert!(open(&f.0).is_err());
}
#[test]
fn workspace_open_rejects_zero_instant_including_offset_forms() {
    let f = Fixture::new();
    for time in [
        "0001-01-01T00:00:00Z",
        "0001-01-01T01:00:00+01:00",
        "0001-01-01T00:00:00.0000000000Z",
    ] {
        f.write("lycheedev.workspace.v9", &"a".repeat(32), time);
        let e = open(&f.0).unwrap_err();
        assert_eq!(e.code, "vault.unsupported_workspace");
        assert_eq!(e.message, "vault.unsupported_workspace");
    }
}
#[test]
fn workspace_open_missing_manifest_distinguishes_nonempty_legacy() {
    let f = Fixture::new();
    let empty = open(&f.0).unwrap_err();
    assert_ne!(empty.code, "vault.legacy_detected");
    fs::write(f.0.join("old-data"), b"retained").unwrap();
    let legacy = open(&f.0).unwrap_err();
    assert_eq!(legacy.code, "vault.legacy_detected");
    assert_eq!(legacy.message, "vault.legacy_detected");
    assert_eq!(legacy.exit_code, 3);
    assert_eq!(fs::read(f.0.join("old-data")).unwrap(), b"retained");
    assert!(!f.0.join("workspace.json").exists());
}
#[test]
fn workspace_open_schema_family_and_validation_priority_match_go() {
    let f = Fixture::new();
    for (schema, newer) in [
        ("foreign", false),
        ("lycheedev.workspace.v0", false),
        ("lycheedev.workspace.v", false),
        ("lycheedev.workspace.v-2", false),
        ("lycheedev.workspace.v2x", false),
        ("lycheedev.workspace.v02", true),
        ("lycheedev.workspace.v9223372036854775808", false),
    ] {
        f.write(schema, &"a".repeat(32), "2026-10-15T00:00:00Z");
        let e = open(&f.0).unwrap_err();
        assert_eq!(
            e.code,
            if newer {
                "vault.workspace_schema_newer"
            } else {
                "vault.unsupported_workspace"
            }
        );
        assert_eq!(
            e.message,
            if newer {
                "vault.workspace_schema_newer\nvault.unsupported_workspace"
            } else {
                "vault.unsupported_workspace"
            }
        );
        assert_eq!(e.exit_code, 4);
    }
    f.write("lycheedev.workspace.v2", "short", "2026-10-15T00:00:00Z");
    assert_eq!(open(&f.0).unwrap_err().code, "vault.unsupported_workspace");
    f.write(
        "lycheedev.workspace.v2",
        &"x".repeat(32),
        "2026-10-15T00:00:00Z",
    );
    assert_eq!(open(&f.0).unwrap_err().code, "vault.workspace_schema_newer");
}
#[test]
fn workspace_open_go_case_duplicate_null_and_time_escape_rules() {
    let f = Fixture::new();
    let raw = format!(
        r#"{{"SCHEMA":"lycheedev.workspace.v1","workspaceId":"{}","WORKSPACEID":null,"createdAt":"2026-10-15T0:00:00,1234567899Z","CreatedAt":null,"unknown":{{"field":true}}}}"#,
        "a".repeat(32)
    );
    fs::write(f.0.join("workspace.json"), raw).unwrap();
    let v = open(&f.0).unwrap();
    assert_eq!(v["createdAt"], "2026-10-15T00:00:00.123456789Z");
    let raw = format!(
        r#"{{"schema":"lycheedev.workspace.v1","workspaceId":"{}","createdAt":"2026-10-15T00:00:00\u005a"}}"#,
        "a".repeat(32)
    );
    fs::write(f.0.join("workspace.json"), raw).unwrap();
    assert_eq!(
        open(&f.0).unwrap_err().message,
        "vault.unsupported_workspace"
    );
}
#[test]
fn workspace_open_malformed_field_rejects_even_when_later_duplicate_is_valid() {
    let f = Fixture::new();
    let raw = format!(
        r#"{{"schema":"lycheedev.workspace.v1","workspaceId":7,"workspaceId":"{}","createdAt":"2026-10-15T00:00:00Z"}}"#,
        "a".repeat(32)
    );
    fs::write(f.0.join("workspace.json"), raw).unwrap();
    assert_eq!(open(&f.0).unwrap_err().code, "vault.unsupported_workspace");
}
