use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "probe-registry-{}-{}-{}",
            std::process::id(),
            now().replace(':', ""),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        workspace::initialize(&root.join("home")).unwrap();
        Metadata::open(&root.join("home"), true)
            .unwrap()
            .close()
            .unwrap();
        fs::write(root.join("code.lua"), b"return 42").unwrap();
        Self { root }
    }
    fn context(&self) -> LiveContext {
        LiveContext { home: self.root.join("home"), control: Default::default() }
    }
    fn put_request(&self, name: &str) -> ProbeRequest {
        ProbeRequest::Put { name: name.into(), file: self.root.join("code.lua") }
    }
    fn put(&self, name: &str) -> Outcome {
        dispatch(&self.context(), &self.put_request(name)).unwrap()
    }
    fn remove(&self, name: &str) {
        dispatch(&self.context(), &ProbeRequest::Remove { name: name.into() }).unwrap();
    }
    fn corrupt_name(&self, key: &str, name: &str) {
        let mut meta = Metadata::open(&self.root.join("home"), true).unwrap();
        let key = format!("probe/name/{key}");
        let mut n: Name = meta.read(&key).unwrap();
        n.name = name.into();
        let generation = meta.generation(&key).unwrap();
        meta.put(&key, &n, generation).unwrap();
        meta.close().unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn active_limit_pages_past_removed_names_and_include_removed_keeps_order() {
    let f = Fixture::new();
    for n in ["a", "b", "z"] {
        f.put(n);
    }
    f.remove("a");
    f.remove("b");
    let ctx = f.context();
    let request = ProbeRequest::List { limit: 1, include_removed: false };
    let result = dispatch(&ctx, &request).unwrap();
    assert_eq!(result.data["items"].as_array().unwrap().len(), 1);
    assert_eq!(result.data["items"][0]["name"], "z");
    let including_removed = ProbeRequest::List { limit: 1, include_removed: true };
    assert_eq!(dispatch(&ctx, &including_removed).unwrap().data["items"][0]["name"], "a");
    f.remove("z");
    assert_eq!(dispatch(&ctx, &request).unwrap().data["items"], json!([]));
}
#[test]
fn corrupt_selector_rejects_put_before_new_revision_is_committed() {
    let f = Fixture::new();
    f.put("a");
    f.corrupt_name("a", "wrong");
    let bytes = b"return 43";
    fs::write(f.root.join("code.lua"), bytes).unwrap();
    let ctx = f.context();
    let request = f.put_request("a");
    assert_eq!(dispatch(&ctx, &request).unwrap_err().code, "live.probe_name_corrupt");
    let meta = Metadata::open(&ctx.home, false).unwrap();
    assert_eq!(
        meta.read::<Revision>(&format!("probe/revision/PRB-{}", storage::sha256(bytes)))
            .unwrap_err()
            .code,
        "vault.record_missing"
    );
    meta.close().unwrap();
}
#[test]
fn list_and_remove_validate_key_identity_including_removed_names() {
    let f = Fixture::new();
    f.put("a");
    f.remove("a");
    f.corrupt_name("a", "wrong");
    assert_eq!(
        dispatch(&f.context(), &ProbeRequest::List { limit: 100, include_removed: false }).unwrap_err().code,
        "live.probe_name_corrupt"
    );
    let ctx = f.context();
    let request = ProbeRequest::Remove { name: "a".into() };
    assert_eq!(dispatch(&ctx, &request).unwrap_err().code, "live.probe_name_corrupt");
}
#[test]
fn same_revision_is_idempotent_and_removed_revision_remains_readable() {
    let f = Fixture::new();
    let first = f.put("a");
    assert_eq!(first.data["changed"], true);
    assert_eq!(f.put("a").data["changed"], false);
    let id = first.data["revision"]["id"].as_str().unwrap();
    f.remove("a");
    assert_eq!(code(&f.context(), id).unwrap(), b"return 42");
    assert_eq!(f.put("a").data["changed"], true);
}
#[test]
fn bounded_input_rechecks_actual_growth_and_empty_after_same_handle_stat() {
    let f = Fixture::new();
    let path = f.root.join("code.lua");
    let ctx = f.context();
    let error = source_bytes_observed(&ctx, &path, || fs::write(&path, vec![b'x'; MAX_CODE + 1]).unwrap())
        .unwrap_err();
    assert_eq!(error.exit_code, 2);
    fs::write(&path, b"x").unwrap();
    let error = source_bytes_observed(&ctx, &path, || fs::write(&path, []).unwrap()).unwrap_err();
    assert_eq!(error.exit_code, 2);
}
#[test]
fn cancelled_registry_commands_publish_nothing() {
    use lycheedev_core::control::{OperationControl, TrackedCancellation};
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    let mut ctx = f.context();
    let request = f.put_request("a");
    let token = TrackedCancellation::new();
    token.cancel();
    ctx.control = OperationControl::tracked(Instant::now() + Duration::from_secs(10), token);
    assert_eq!(dispatch(&ctx, &request).unwrap_err().exit_code, 7);
    let meta = Metadata::open(&ctx.home, false).unwrap();
    assert_eq!(
        meta.read::<Name>("probe/name/a").unwrap_err().code,
        "vault.record_missing"
    );
    meta.close().unwrap();
}
#[test]
fn failed_selector_cas_rolls_back_new_revision_in_same_transaction() {
    let f = Fixture::new();
    f.put("a");
    let bytes = b"return 999";
    fs::write(f.root.join("code.lua"), bytes).unwrap();
    let ctx = f.context();
    let request = f.put_request("a");
    let error = dispatch_observed(&ctx, &request, || {
        let mut meta = Metadata::open(&ctx.home, true)?;
        let key = "probe/name/a";
        let n: Name = meta.read(key)?;
        let generation = meta.generation(key)?;
        meta.put(key, &n, generation)?;
        meta.close()
    })
    .unwrap_err();
    assert_eq!(error.code, "vault.generation_conflict");
    let meta = Metadata::open(&ctx.home, false).unwrap();
    assert_eq!(
        meta.read::<Revision>(&format!("probe/revision/PRB-{}", storage::sha256(bytes)))
            .unwrap_err()
            .code,
        "vault.record_missing"
    );
    meta.close().unwrap();
}
#[test]
fn cancellation_after_validation_does_not_commit_either_document() {
    use lycheedev_core::control::{OperationControl, TrackedCancellation};
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    let token = TrackedCancellation::new();
    let mut ctx = f.context();
    let request = f.put_request("a");
    ctx.control = OperationControl::tracked(Instant::now() + Duration::from_secs(10), token.clone());
    assert_eq!(
        dispatch_observed(&ctx, &request, || {
            token.cancel();
            Ok(())
        })
        .unwrap_err()
        .exit_code,
        7
    );
    let meta = Metadata::open(&ctx.home, false).unwrap();
    assert!(meta.list::<Name>("probe/name/", 100).unwrap().is_empty());
    assert!(
        meta.list::<Revision>("probe/revision/", 100)
            .unwrap()
            .is_empty()
    );
    meta.close().unwrap();
    assert_eq!(f.put("a").data["changed"], true);
}
#[test]
fn source_read_retains_original_handle_when_path_is_replaced() {
    let f = Fixture::new();
    let path = f.root.join("code.lua");
    let ctx = f.context();
    let bytes = source_bytes_observed(&ctx, &path, || {
        fs::rename(&path, f.root.join("original.lua")).unwrap();
        fs::write(&path, b"replacement").unwrap();
    })
    .unwrap();
    assert_eq!(bytes, b"return 42");
}
#[test]
fn second_mutation_conflict_rolls_back_selector_update() {
    let f = Fixture::new();
    let original = f.put("a");
    let bytes = b"return 999";
    fs::write(f.root.join("code.lua"), bytes).unwrap();
    let ctx = f.context();
    let request = f.put_request("a");
    let digest = storage::sha256(bytes);
    let id = format!("PRB-{digest}");
    let error = dispatch_observed(&ctx, &request, || {
        let mut meta = Metadata::open(&ctx.home, true)?;
        meta.create(
            &format!("probe/revision/{id}"),
            &Revision {
                schema: "lycheedev.probe-revision.v1".into(),
                id: id.clone(),
                sha256: digest,
                bytes: bytes.len(),
                code: encode(bytes),
                created_at: now(),
            },
        )?;
        meta.close()
    })
    .unwrap_err();
    assert_eq!(error.code, "vault.generation_conflict");
    let meta = Metadata::open(&ctx.home, false).unwrap();
    let name: Name = meta.read("probe/name/a").unwrap();
    assert_eq!(
        name.revision,
        original.data["revision"]["id"].as_str().unwrap()
    );
    meta.close().unwrap();
}
#[test]
fn exact_maximum_code_round_trips_and_one_extra_byte_is_not_published() {
    let f = Fixture::new();
    let path = f.root.join("code.lua");
    let bytes = vec![b'x'; MAX_CODE];
    fs::write(&path, &bytes).unwrap();
    let result = f.put("max");
    let id = result.data["revision"]["id"].as_str().unwrap();
    assert_eq!(code(&f.context(), id).unwrap(), bytes);
    fs::write(&path, vec![b'x'; MAX_CODE + 1]).unwrap();
    let ctx = f.context();
    let request = f.put_request("too_large");
    assert_eq!(dispatch(&ctx, &request).unwrap_err().exit_code, 2);
    let meta = Metadata::open(&ctx.home, false).unwrap();
    assert_eq!(
        meta.read::<Name>("probe/name/too_large").unwrap_err().code,
        "vault.record_missing"
    );
    meta.close().unwrap();
}

const ZERO_TIMESTAMPS: &[&str] = &[
    "0001-01-01T00:00:00Z",
    "0001-01-01T00:00:00.000000000Z",
    "0001-01-01T00:00:00+00:00",
    "0001-01-01T01:00:00+01:00",
    "0000-12-31T23:00:00-01:00",
];

fn set_name_timestamp(f: &Fixture, stamp: &str) {
    let mut meta = Metadata::open(&f.root.join("home"), true).unwrap();
    let key = "probe/name/a";
    let mut named: Name = meta.read(key).unwrap();
    named.updated_at = stamp.into();
    let generation = meta.generation(key).unwrap();
    meta.put(key, &named, generation).unwrap();
    meta.close().unwrap();
}
fn change_revision(f: &Fixture, id: &str, change: impl FnOnce(&mut Revision)) {
    let mut meta = Metadata::open(&f.root.join("home"), true).unwrap();
    let key = format!("probe/revision/{id}");
    let mut revision: Revision = meta.read(&key).unwrap();
    change(&mut revision);
    let generation = meta.generation(&key).unwrap();
    meta.put(&key, &revision, generation).unwrap();
    meta.close().unwrap();
}
fn registry_snapshot(f: &Fixture) -> Vec<(String, i64, Vec<u8>)> {
    let ctx = f.context();
    let meta = Metadata::open(&ctx.home, false).unwrap();
    let mut documents = Vec::new();
    let (count, _) = meta.visit_raw_page_controlled_with_check(
        "probe/", "", 100, &ctx.control, || Ok(()),
        |key, generation, bytes| {
            documents.push((key.to_owned(), generation, bytes.to_vec()));
            Ok(())
        },
    ).unwrap();
    assert!(count < 100, "fixture snapshot must contain every probe document");
    meta.close().unwrap();
    documents
}
#[test]
fn zero_selector_timestamps_reject_all_consumers_without_registry_mutation() {
    for stamp in ZERO_TIMESTAMPS {
        for removed in [false, true] {
            let f = Fixture::new();
            let original = f.put("a");
            let id = original.data["revision"]["id"].as_str().unwrap();
            if removed {
                f.remove("a");
            }
            set_name_timestamp(&f, stamp);
            let before = registry_snapshot(&f);
            for include_removed in [false, true] {
                let ctx = f.context();
                let request = ProbeRequest::List { limit: 100, include_removed };
                assert_eq!(dispatch(&ctx, &request).unwrap_err().code, "live.probe_name_corrupt", "{stamp}");
                assert_eq!(registry_snapshot(&f), before);
            }
            for request in [ProbeRequest::Show { selector: "a".into() }, ProbeRequest::Remove { name: "a".into() }] {
                let ctx = f.context();
                assert_eq!(dispatch(&ctx, &request).unwrap_err().code, "live.probe_name_corrupt", "{stamp}");
                assert_eq!(registry_snapshot(&f), before);
            }
            assert_eq!(code(&f.context(), "a").unwrap_err().code, "live.probe_name_corrupt");
            // Both the otherwise-idempotent put and a fresh revision must fail
            // before either stored document or its generation is changed.
            assert_eq!(dispatch(&f.context(), &f.put_request("a")).unwrap_err().code, "live.probe_name_corrupt");
            assert_eq!(registry_snapshot(&f), before);
            fs::write(f.root.join("code.lua"), b"return 43").unwrap();
            assert_eq!(dispatch(&f.context(), &f.put_request("a")).unwrap_err().code, "live.probe_name_corrupt");
            assert_eq!(registry_snapshot(&f), before);
            // Explicit immutable revision lookup does not consume a stored
            // selector. Its synthetic unnamed zero timestamp remains legal.
            let shown = show(&f.context(), id).unwrap();
            assert_eq!(shown.data["name"]["name"], "");
            assert_eq!(shown.data["name"]["updatedAt"], "0001-01-01T00:00:00Z");
            assert_eq!(code(&f.context(), id).unwrap(), b"return 42");
            assert_eq!(registry_snapshot(&f), before);
        }
    }
}
#[test]
fn zero_revision_timestamps_reject_show_code_and_put_without_partial_names() {
    for stamp in ZERO_TIMESTAMPS {
        let f = Fixture::new();
        let original = f.put("a");
        let id = original.data["revision"]["id"].as_str().unwrap();
        change_revision(&f, id, |revision| revision.created_at = (*stamp).into());
        let before = registry_snapshot(&f);
        for selector in ["a", id] {
            let ctx = f.context();
            let request = ProbeRequest::Show { selector: selector.into() };
            assert_eq!(dispatch(&ctx, &request).unwrap_err().code, "live.probe_revision_corrupt", "{stamp}");
            assert_eq!(code(&ctx, selector).unwrap_err().code, "live.probe_revision_corrupt", "{stamp}");
            assert_eq!(registry_snapshot(&f), before);
        }
        for name in ["a", "new_name"] {
            assert_eq!(dispatch(&f.context(), &f.put_request(name)).unwrap_err().code, "live.probe_revision_corrupt", "{stamp}");
            assert_eq!(registry_snapshot(&f), before);
        }
        // Go list/remove consume selectors alone; the timestamp check must not
        // grow into an unrelated revision scan or stop retiring a valid name.
        assert_eq!(dispatch(&f.context(), &ProbeRequest::List { limit: 100, include_removed: false }).unwrap().data["items"][0]["name"], "a");
        assert_eq!(registry_snapshot(&f), before);
        f.remove("a");
        let after = registry_snapshot(&f);
        let key = format!("probe/revision/{id}");
        assert_eq!(before.iter().find(|doc| doc.0 == key), after.iter().find(|doc| doc.0 == key));
        assert_eq!(dispatch(&f.context(), &ProbeRequest::List { limit: 100, include_removed: false }).unwrap().data["items"], json!([]));
    }
}
#[test]
fn nonzero_stored_times_and_unnamed_zero_fallback_keep_registry_behavior() {
    for stamp in [
        "0001-01-01T00:00:00.000000001Z",
        "0001-01-01T00:00:00+01:00",
        "1970-01-01T00:00:00Z",
        "2026-10-18T01:02:03.123456789+01:00",
    ] {
        let f = Fixture::new();
        let original = f.put("a");
        let id = original.data["revision"]["id"].as_str().unwrap();
        set_name_timestamp(&f, stamp);
        change_revision(&f, id, |revision| revision.created_at = stamp.into());
        let before = registry_snapshot(&f);
        let shown = show(&f.context(), "a").unwrap();
        assert_eq!(shown.data["name"]["updatedAt"], stamp);
        assert_eq!(shown.data["revision"]["createdAt"], stamp);
        assert_eq!(code(&f.context(), "a").unwrap(), b"return 42");
        assert_eq!(f.put("a").data["changed"], false);
        assert_eq!(registry_snapshot(&f), before);
        assert_eq!(dispatch(&f.context(), &ProbeRequest::List { limit: 100, include_removed: false }).unwrap().data["items"][0]["updatedAt"], stamp);
        f.remove("a");
        assert_eq!(show(&f.context(), "a").unwrap_err().code, "live.probe_removed");
        let shown = show(&f.context(), id).unwrap();
        assert_eq!(shown.data["name"]["updatedAt"], "0001-01-01T00:00:00Z");
        assert_eq!(shown.data["revision"]["createdAt"], stamp);
        assert_eq!(code(&f.context(), id).unwrap(), b"return 42");
        assert_eq!(f.put("a").data["changed"], true);
        assert_eq!(code(&f.context(), "a").unwrap(), b"return 42");
    }
}
#[test]
fn valid_stored_timestamps_do_not_weaken_revision_identity_or_code_integrity() {
    for field in ["schema", "id", "sha256", "bytes", "code"] {
        let f = Fixture::new();
        let original = f.put("a");
        let id = original.data["revision"]["id"].as_str().unwrap();
        change_revision(&f, id, |revision| match field {
            "schema" => revision.schema = "wrong".into(),
            "id" => revision.id = format!("PRB-{}", "0".repeat(64)),
            "sha256" => revision.sha256 = "0".repeat(64),
            "bytes" => revision.bytes += 1,
            "code" => revision.code = encode(b"return 43"),
            _ => unreachable!(),
        });
        let before = registry_snapshot(&f);
        for selector in ["a", id] {
            let ctx = f.context();
            assert_eq!(show(&ctx, selector).unwrap_err().code, "live.probe_revision_corrupt", "{field}");
            assert_eq!(code(&ctx, selector).unwrap_err().code, "live.probe_revision_corrupt", "{field}");
        }
        assert!(dispatch(&f.context(), &f.put_request("a")).is_err());
        assert!(dispatch(&f.context(), &f.put_request("new_name")).is_err());
        assert_eq!(registry_snapshot(&f), before);
    }
}
