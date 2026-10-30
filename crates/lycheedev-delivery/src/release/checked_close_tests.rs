use super::*;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-release-close-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn resource(&self, path: &str, bytes: &[u8]) -> Resource {
        let file = self.0.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
        Resource {
            path: path.into(),
            bytes: bytes.len() as u64,
            sha256: storage::sha256(bytes),
        }
    }
    fn manifest(&self) -> PathBuf {
        let path = self.0.join("release.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "schema": "lycheedev.release.v1",
                "version": "3.2.0",
                "commit": "a".repeat(40),
                "binaries": {"windows-amd64": {
                    "path": "native/windows-amd64/lycheedev.exe", "bytes": 1,
                    "sha256": "b".repeat(64)
                }},
                "resources": [
                    {"path":"skill/SKILL.md", "bytes": 1, "sha256": "c".repeat(64)},
                    {"path":"addon/Lychee Dev.toc", "bytes": 1, "sha256": "d".repeat(64)}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn assert_held(path: &Path) {
    #[cfg(windows)]
    {
        assert!(fs::write(path, b"must stay locked").is_err());
        assert!(fs::remove_file(path).is_err());
    }
    #[cfg(not(windows))]
    let _ = path;
}
fn assert_released(path: &Path) {
    fs::write(path, b"released").unwrap();
    fs::remove_file(path).unwrap();
}

#[test]
fn manifest_success_and_validation_failures_keep_owner_until_checked_finish() {
    for failure in ["none", "version", "json", "size", "channel"] {
        let fixture = Fixture::new();
        let path = fixture.manifest();
        match failure {
            "json" => fs::write(&path, b"{").unwrap(),
            "size" => fs::write(&path, vec![b' '; (1 << 20) + 1]).unwrap(),
            "channel" => {
                let mut raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                raw["runtime"] = json!("rust");
                fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
            }
            _ => {}
        }
        let control = OperationControl::default();
        let mut finishes = 0;
        let result = read_manifest_with_finish(
            &fixture.0,
            if failure == "version" {
                "wrong"
            } else {
                "3.2.0"
            },
            &control,
            |file, result| {
                finishes += 1;
                assert_eq!(result.is_ok(), failure == "none");
                assert!(file.metadata().is_ok());
                assert_held(&path);
                storage::finish_file(file, result, &control)
            },
        );
        assert_eq!(finishes, 1);
        assert_eq!(result.is_ok(), failure == "none");
        assert_released(&path);
    }
}

#[test]
fn manifest_cancellation_at_finish_closes_and_rejects_late_success() {
    let fixture = Fixture::new();
    let path = fixture.manifest();
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let mut finishes = 0;
    let error = read_manifest_with_finish(&fixture.0, "3.2.0", &control, |file, result| {
        finishes += 1;
        assert!(result.is_ok());
        assert_held(&path);
        cancel.cancel();
        storage::finish_file(file, result, &control)
    })
    .unwrap_err();
    assert_eq!(finishes, 1);
    assert_eq!(error.code, "command.cancelled");
    assert!(error.details.is_null());
    assert_released(&path);
}

#[test]
fn manifest_checked_finish_failure_is_not_downgraded_or_swallowed() {
    let fixture = Fixture::new();
    let path = fixture.manifest();
    let control = OperationControl::default();
    let injected = ToolkitError::new("fixture.close", "injected finish failure", 5)
        .with_details(json!({"retained": true}));
    let expected = serde_json::to_value(&injected).unwrap();
    let error = read_manifest_with_finish(&fixture.0, "3.2.0", &control, |file, result| {
        storage::finish_file(file, result, &control)?;
        // Injection at the finish boundary; not a kernel CloseHandle failure.
        Err(injected)
    })
    .unwrap_err();
    assert_eq!(serde_json::to_value(error).unwrap(), expected);
    assert_released(&path);
}

#[test]
fn inventory_success_checks_each_owned_file_close_once_after_identity_validation() {
    let fixture = Fixture::new();
    let resources = [
        fixture.resource("addon/a", b"first"),
        fixture.resource("addon/b", b"second"),
    ];
    let control = OperationControl::default();
    let mut finishes = 0;
    verify_inventory_with_finish(&fixture.0, &resources, None, &control, |file, result| {
        let resource = &resources[finishes];
        finishes += 1;
        assert!(result.is_ok());
        assert_eq!(file.metadata().unwrap().len(), resource.bytes);
        assert_held(&fixture.0.join(&resource.path));
        storage::finish_file(file, result, &control)
    })
    .unwrap();
    assert_eq!(finishes, resources.len());
    for resource in resources {
        assert_released(&fixture.0.join(resource.path));
    }
}

#[test]
fn inventory_shape_and_digest_rejections_still_finish_the_held_file() {
    for wrong_shape in [true, false] {
        let fixture = Fixture::new();
        let mut first = fixture.resource("addon/a", b"first");
        if wrong_shape {
            first.bytes += 1;
        } else {
            first.sha256 = "0".repeat(64);
        }
        let resources = [first, fixture.resource("addon/b", b"second")];
        let control = OperationControl::default();
        let mut finishes = 0;
        let error =
            verify_inventory_with_finish(&fixture.0, &resources, None, &control, |file, result| {
                finishes += 1;
                assert_eq!(
                    result.as_ref().unwrap_err().code,
                    "delivery.invalid_payload"
                );
                assert_held(&fixture.0.join("addon/a"));
                storage::finish_file(file, result, &control)
            })
            .unwrap_err();
        assert_eq!(
            finishes, 1,
            "later resources must not be opened after rejection"
        );
        assert_eq!(error.code, "delivery.invalid_payload");
        assert!(error.message.starts_with(if wrong_shape {
            "file shape differs:"
        } else {
            "digest differs:"
        }));
        assert_released(&fixture.0.join("addon/a"));
    }
}

#[test]
fn inventory_failed_hash_constructor_closes_before_returning_without_second_finish() {
    let fixture = Fixture::new();
    let mut resource = fixture.resource("addon/a", b"too long");
    resource.bytes = 1;
    let control = OperationControl::default();
    let mut finishes = 0;
    let error =
        verify_inventory_with_finish(&fixture.0, &[resource], None, &control, |file, result| {
            finishes += 1;
            storage::finish_file(file, result, &control)
        })
        .unwrap_err();
    assert_eq!(finishes, 0);
    assert_eq!(error.code, "input.invalid");
    assert_released(&fixture.0.join("addon/a"));
}

#[test]
fn inventory_finish_failure_or_cancellation_stops_before_the_next_resource() {
    for cancelled in [true, false] {
        let fixture = Fixture::new();
        let resources = [
            fixture.resource("addon/a", b"first"),
            fixture.resource("addon/b", b"second"),
        ];
        let control = OperationControl::default();
        let cancel = control.tracked_cancel_handle().unwrap();
        let mut finishes = 0;
        let error =
            verify_inventory_with_finish(&fixture.0, &resources, None, &control, |file, result| {
                finishes += 1;
                assert!(result.is_ok());
                assert_held(&fixture.0.join("addon/a"));
                if cancelled {
                    cancel.cancel();
                }
                storage::finish_file(file, result, &control)?;
                // Successful real close followed by a failing finish outcome.
                Err(ToolkitError::new(
                    "fixture.close",
                    "injected finish failure",
                    5,
                ))
            })
            .unwrap_err();
        assert_eq!(finishes, 1);
        assert_eq!(
            error.code,
            if cancelled {
                "command.cancelled"
            } else {
                "fixture.close"
            }
        );
        assert_released(&fixture.0.join("addon/a"));
    }
}

#[test]
fn release_pre_cancelled_entry_does_not_acquire_file_ownership() {
    let fixture = Fixture::new();
    let path = fixture.manifest();
    let resource = fixture.resource("addon/a", b"first");
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    let mut finishes = 0;
    let error = read_manifest_with_finish(&fixture.0, "3.2.0", &control, |file, result| {
        finishes += 1;
        storage::finish_file(file, result, &control)
    })
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    let error =
        verify_inventory_with_finish(&fixture.0, &[resource], None, &control, |file, result| {
            finishes += 1;
            storage::finish_file(file, result, &control)
        })
        .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(finishes, 0);
    assert_released(&path);
    assert_released(&fixture.0.join("addon/a"));
}

#[test]
fn inventory_directory_scan_still_rejects_unlisted_files_after_checked_closes() {
    let fixture = Fixture::new();
    let resource = fixture.resource("addon/a", b"listed");
    fixture.resource("addon/extra", b"unlisted");
    let control = OperationControl::default();
    let mut finishes = 0;
    let error =
        verify_inventory_with_finish(&fixture.0, &[resource], None, &control, |file, result| {
            finishes += 1;
            storage::finish_file(file, result, &control)
        })
        .unwrap_err();
    assert_eq!(finishes, 1);
    assert_eq!(error.code, "delivery.invalid_payload");
    assert_eq!(error.message, "unlisted file: addon/extra");
    assert_released(&fixture.0.join("addon/a"));
}

#[test]
fn manifest_runtime_channel_uses_the_same_version_binding_as_update() {
    for (version, tag, accepted) in [
        ("3.5.0", "beta", true),
        ("3.5.0", "next", false),
        ("3.5.0", "latest", false),
        ("3.1.2", "next", true),
        ("3.1.2", "beta", false),
    ] {
        let fixture = Fixture::new();
        let path = fixture.manifest();
        let mut raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        raw["version"] = json!(version);
        raw["runtime"] = json!("rust");
        raw["npmChannel"] = json!({
            "schema": "lycheedev.npm-channel.v1", "version": version,
            "distTag": tag, "preserveLatest": "2.5.1"
        });
        fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        let result = read_manifest_with_control(&fixture.0, version, &OperationControl::default());
        assert_eq!(result.is_ok(), accepted, "{version}/{tag}: {result:?}");
        assert_eq!(crate::npm_update::binding(&fixture.0, version).is_ok(), accepted);
        if let Err(error) = result {
            assert_eq!(error.code, "delivery.invalid_release");
            assert_eq!(error.exit_code, 4);
            assert_eq!(error.message, "runtime/channel binding differs from the owner release contract");
        }
        assert_released(&path);
    }
}

const CURRENT_CHANNEL: &[u8] = include_bytes!("../../../../release/npm-channel.json");

fn write_channel_manifest(fixture: &Fixture, version: &str, channel: &[u8]) -> PathBuf {
    let path = fixture.manifest();
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["version"] = json!(version);
    manifest["runtime"] = json!("rust");
    // Embed the repository channel's original bytes, so the actual shipped
    // release policy is exercised rather than a hand-written historic value.
    let mut raw = serde_json::to_vec(&manifest).unwrap();
    assert_eq!(raw.pop(), Some(b'}'));
    raw.extend_from_slice(b",\"npmChannel\":");
    raw.extend_from_slice(channel);
    raw.push(b'}');
    fs::write(&path, raw).unwrap();
    path
}

fn assert_channel_readers(fixture: &Fixture, version: &str, accepted: bool, case: &str) {
    let manifest = read_manifest_with_control(&fixture.0, version, &OperationControl::default());
    let update = crate::npm_update::binding(&fixture.0, version);
    assert_eq!(manifest.is_ok(), accepted, "manifest {case}: {manifest:?}");
    assert_eq!(update.is_ok(), accepted, "update {case}: {update:?}");
    if !accepted {
        assert_eq!(manifest.unwrap_err().code, "delivery.invalid_release");
        let error = update.unwrap_err();
        assert_eq!(error.code, "update.runtime_incompatible");
        assert_eq!(error.exit_code, 3);
    }
}

#[test]
fn current_npm_channel_bytes_are_accepted_by_release_and_update() {
    let channel: Value = serde_json::from_slice(CURRENT_CHANNEL).unwrap();
    let version = channel["version"].as_str().unwrap();
    let fixture = Fixture::new();
    let path = write_channel_manifest(&fixture, version, CURRENT_CHANNEL);
    assert_channel_readers(&fixture, version, true, "current release/npm-channel.json");
    assert_released(&path);
}

#[test]
fn npm_channel_preserved_latest_is_a_distinct_stable_version_not_a_fixed_release() {
    let current: Value = serde_json::from_slice(CURRENT_CHANNEL).unwrap();
    let version = current["version"].as_str().unwrap();
    for preserved in ["0.0.0", "3.2.5", "4.100.999", "01.02.003"] {
        let fixture = Fixture::new();
        let mut channel = current.clone();
        channel["preserveLatest"] = json!(preserved);
        let path = write_channel_manifest(&fixture, version, &serde_json::to_vec(&channel).unwrap());
        assert_channel_readers(&fixture, version, true, preserved);
        assert_released(&path);
    }
    for preserved in [
        "", "3.2", "3.2.6-rc.1", "v3.2.6", "3.2.6+build", " 3.2.6", "3.2.6\n", "3.2.6x",
        ".2.6", version,
    ] {
        let fixture = Fixture::new();
        let mut channel = current.clone();
        channel["preserveLatest"] = json!(preserved);
        let path = write_channel_manifest(&fixture, version, &serde_json::to_vec(&channel).unwrap());
        assert_channel_readers(&fixture, version, false, preserved);
        assert_released(&path);
    }
}

#[test]
fn npm_channel_binding_rejects_incomplete_fields_wrong_types_and_original_identity_conflicts() {
    let current: Value = serde_json::from_slice(CURRENT_CHANNEL).unwrap();
    let version = current["version"].as_str().unwrap();
    let mut cases = vec![
        ("array", json!([])),
        ("string", json!("channel")),
        ("null", Value::Null),
    ];
    for field in ["schema", "version", "distTag", "preserveLatest"] {
        let mut missing = current.clone();
        missing.as_object_mut().unwrap().remove(field);
        cases.push((field, missing));
        let mut wrong_type = current.clone();
        wrong_type[field] = json!(326);
        cases.push((field, wrong_type));
    }
    for (field, value) in [
        ("schema", json!("lycheedev.npm-channel.v2")),
        ("version", json!(if version == "0.0.0" { "0.0.1" } else { "0.0.0" })),
        ("version", json!("3.5.0-dev")),
        ("distTag", json!(if current["distTag"] == "beta" { "next" } else { "beta" })),
        ("distTag", json!("latest")),
        ("extra", json!(true)),
    ] {
        let mut channel = current.clone();
        channel[field] = value;
        cases.push((field, channel));
    }
    for (case, channel) in cases {
        let fixture = Fixture::new();
        let path = write_channel_manifest(&fixture, version, &serde_json::to_vec(&channel).unwrap());
        assert_channel_readers(&fixture, version, false, case);
        assert_released(&path);
    }
    for runtime in [Value::Null, json!("go")] {
        let fixture = Fixture::new();
        let path = write_channel_manifest(&fixture, version, CURRENT_CHANNEL);
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["runtime"] = runtime;
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert_channel_readers(&fixture, version, false, "runtime");
        assert_released(&path);
    }
    for invalid_version in ["3.5.0-dev", "3.5", "3.5.0+build", "3.5.0\n"] {
        let fixture = Fixture::new();
        let mut channel = current.clone();
        channel["version"] = json!(invalid_version);
        channel["distTag"] = json!(crate::npm_update::release_channel(invalid_version));
        let path = write_channel_manifest(
            &fixture,
            invalid_version,
            &serde_json::to_vec(&channel).unwrap(),
        );
        assert_channel_readers(&fixture, invalid_version, false, "nonstable version");
        assert_released(&path);
    }
}
