//! Real Windows snapshot/CREATE/replace regressions. No timing-count shims.
use super::*;
use std::{
    process::{Child, Command, Stdio},
    thread,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-atomic-facts-{}-{}",
            std::process::id(),
            lycheedev_windows::random::token().unwrap()
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let result = fs::remove_dir_all(&self.0);
        if !thread::panicking() {
            result.unwrap();
        }
    }
}
fn cache(fixture: &Fixture, control: &OperationControl) -> SourceCache {
    SourceCache::open(&fixture.0, Kind::Facts, control).unwrap()
}
fn lock_names(home: &Path) -> Vec<std::ffi::OsString> {
    let mut names: Vec<_> = fs::read_dir(home.join("locks"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    names
}

#[test]
fn facts_profile_reports_only_real_stage_phases_and_never_opens_lock_directory() {
    for fail_after_write in [false, true] {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = cache(&f, &control);
        let before = lock_names(&f.0);
        let key = storage::sha256(b"facts profile");
        let mut phases = Vec::new();
        let result = cache.publish_profiled(
            &key,
            b"complete",
            &control,
            &mut |at| {
                if fail_after_write && at == "written" {
                    Err(ToolkitError::new("fixture.write", "first cause", 5))
                } else {
                    Ok(())
                }
            },
            Some(&mut |phase, _| phases.push(phase)),
        );
        use SourceCachePublicationPhase::*;
        if fail_after_write {
            let error = result.unwrap_err();
            assert_eq!(error.code, "fixture.write");
            assert_eq!(error.details["state"], "NotPublished");
            assert_eq!(phases, [Stage, Write, StageClose]);
            assert!(cache.read(&key, &control).unwrap().is_none());
        } else {
            result.unwrap();
            assert_eq!(phases, [Stage, Write, Seal, Rename, StageClose]);
            assert_eq!(cache.read(&key, &control).unwrap().unwrap(), b"complete");
        }
        assert!(cache.lock_directory.get().is_none());
        assert_eq!(lock_names(&f.0), before);
        cache.finish(Ok(()), &control).unwrap();
        assert_eq!(lock_names(&f.0), before);
    }
}

#[test]
fn facts_publication_ignores_a_held_legacy_facts_lock_without_releasing_it() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cache = cache(&f, &control);
    let key = storage::sha256(b"held legacy facts lock");
    let resource = format!("source-cache:{}:{key}", Kind::Facts.directory());
    let lock =
        storage::ExclusiveLock::resource_controlled(&cache.locks, &resource, &control).unwrap();
    let before = lock_names(&f.0);
    cache.publish(&key, b"complete", &control).unwrap();
    assert_eq!(cache.read(&key, &control).unwrap().unwrap(), b"complete");
    assert!(cache.lock_directory.get().is_none());
    assert_eq!(lock_names(&f.0), before);
    assert_eq!(
        storage::ExclusiveLock::resource_controlled(&cache.locks, &resource, &control)
            .err()
            .unwrap()
            .code,
        "workspace.busy"
    );
    lock.close().unwrap();
    cache.finish(Ok(()), &control).unwrap();
}

#[test]
fn facts_cancel_at_each_owned_stage_boundary_preserves_old_snapshot_and_foreign_slot() {
    for stopped_at in ["acquired", "written", "sealed", "renamed", "closed"] {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = cache(&f, &control);
        let key = storage::sha256(b"phase cancellation");
        cache.publish(&key, b"old complete", &control).unwrap();
        let foreign = cache
            .directory
            .path()
            .join(format!(".lycheedev-cache-{key}-0"));
        fs::write(&foreign, b"foreign original").unwrap();
        let cancelled = OperationControl::default();
        let token = cancelled.tracked_cancel_handle().unwrap();
        let body = vec![51; 131073];
        let error = cache
            .publish_profiled(
                &key,
                &body,
                &cancelled,
                &mut |at| {
                    if at == stopped_at {
                        token.cancel();
                    }
                    Ok(())
                },
                Some(&mut |phase, _| {
                    if matches!(
                        (stopped_at, phase),
                        ("acquired", SourceCachePublicationPhase::Stage)
                            | ("renamed", SourceCachePublicationPhase::Rename)
                            | ("closed", SourceCachePublicationPhase::StageClose)
                    ) {
                        token.cancel();
                    }
                }),
            )
            .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        let published = matches!(stopped_at, "renamed" | "closed");
        assert_eq!(
            error.details["state"],
            if published {
                "Published"
            } else {
                "NotPublished"
            }
        );
        assert_eq!(error.details["durable"], false);
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign original");
        let read = cache.read(&key, &control).unwrap().unwrap();
        assert_eq!(
            read.as_slice(),
            if published {
                body.as_slice()
            } else {
                b"old complete"
            }
        );
        assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 2);
        cache.finish(Ok(()), &control).unwrap();
    }
}

fn hold_published_writer<'a>(
    cache: &'a SourceCache,
    key: &str,
    control: &OperationControl,
) -> StagedFile<'a> {
    let mut first =
        StagedFile::create_rebuildable_cache(&cache.directory, key, &mut Controlled(control))
            .unwrap()
            .unwrap();
    first.write_all(b"first complete").unwrap();
    first.seal_rebuildable_cache().unwrap();
    assert_eq!(
        first
            .replace_name(
                OsStr::new(&cache.name(key).unwrap()),
                &mut Controlled(control)
            )
            .unwrap(),
        StagedPublication::Published
    );
    first
}

#[test]
fn facts_waits_on_real_published_writer_then_completes_after_its_checked_close() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cache = cache(&f, &control);
    let key = storage::sha256(b"real writer close wait");
    let first = hold_published_writer(&cache, &key, &control);
    let mut old_reader = cache
        .directory
        .open_optional_project_file_controlled(
            OsStr::new(&cache.name(&key).unwrap()),
            &mut Controlled(&control),
        )
        .unwrap()
        .unwrap();
    let (blocked, received) = std::sync::mpsc::channel();
    thread::scope(|scope| {
        let pending = scope.spawn(|| {
            let mut phases = Vec::new();
            let result = cache.publish_profiled(
                &key,
                b"second complete",
                &control,
                &mut |at| {
                    if at == "rename_blocked" {
                        blocked.send(()).unwrap();
                    }
                    Ok(())
                },
                Some(&mut |phase, _| phases.push(phase)),
            );
            (result, phases)
        });
        // Two real native rejections prove the production wait/retry path ran
        // while first still owned its writable deny-delete handle.
        received.recv_timeout(Duration::from_secs(10)).unwrap();
        let temporary: Vec<_> = fs::read_dir(cache.directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with(".lycheedev-cache-"))
            .collect();
        assert_eq!(temporary.len(), 1);
        received.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(!pending.is_finished());
        assert!(cache.directory.path().join(&temporary[0]).exists());
        assert_eq!(
            cache.read(&key, &control).unwrap().unwrap(),
            b"first complete"
        );
        assert!(first.close().is_empty());
        let (result, phases) = pending.join().unwrap();
        result.unwrap();
        use SourceCachePublicationPhase::*;
        assert_eq!(phases, [Stage, Write, Seal, Rename, StageClose]);
        assert!(!cache.directory.path().join(&temporary[0]).exists());
    });
    assert_eq!(
        cache.read(&key, &control).unwrap().unwrap(),
        b"second complete"
    );
    let mut old = Vec::new();
    old_reader.read_to_end(&mut old).unwrap();
    assert_eq!(old, b"first complete");
    lycheedev_windows::filesystem::close_file(old_reader).unwrap();
    assert!(cache.lock_directory.get().is_none());
    assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 1);
    cache.finish(Ok(()), &control).unwrap();
}

#[test]
fn facts_real_sharing_wait_cancel_discards_only_the_private_candidate() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cache = cache(&f, &control);
    let key = storage::sha256(b"cancel sharing wait");
    let first = hold_published_writer(&cache, &key, &control);
    let foreign = cache
        .directory
        .path()
        .join(format!(".lycheedev-cache-{key}-63"));
    fs::write(&foreign, b"foreign retained").unwrap();
    let bounded = OperationControl::default();
    let cancel = bounded.tracked_cancel_handle().unwrap();
    let mut blocked = 0;
    let error = cache
        .publish_observed(&key, b"second complete", &bounded, |at| {
            if at == "rename_blocked" {
                blocked += 1;
                cancel.cancel();
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(blocked, 1);
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.details["state"], "NotPublished");
    assert_eq!(error.details["durable"], false);
    assert_eq!(first.publication(), StagedPublication::Published);
    assert_eq!(
        cache.read(&key, &control).unwrap().unwrap(),
        b"first complete"
    );
    assert_eq!(fs::read(&foreign).unwrap(), b"foreign retained");
    assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 2);
    assert!(first.close().is_empty());
    cache.finish(Ok(()), &control).unwrap();
}

#[test]
fn facts_real_sharing_wait_uses_original_deadline_and_preserves_published_owner() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cache = cache(&f, &control);
    let key = storage::sha256(b"deadline sharing wait");
    let first = hold_published_writer(&cache, &key, &control);
    let deadline = Instant::now() + Duration::from_secs(5);
    let bounded = OperationControl::uncancelable(deadline);
    let mut blocked = 0;
    let error = cache
        .publish_observed(&key, b"second complete", &bounded, |at| {
            if at == "rename_blocked" {
                blocked += 1;
                // Keep preparation and real contention separate from the bounded
                // wait: reach its last 10 ms under the original immutable deadline.
                if blocked == 1 {
                    thread::sleep(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .saturating_sub(Duration::from_millis(10)),
                    );
                }
            }
            Ok(())
        })
        .unwrap_err();
    assert!(
        blocked > 0,
        "fixture never reached real sharing contention: {error}"
    );
    assert_eq!(bounded.deadline(), deadline);
    assert_eq!(error.code, "command.deadline");
    assert_eq!(error.details["state"], "NotPublished");
    assert_eq!(first.publication(), StagedPublication::Published);
    assert_eq!(
        cache.read(&key, &control).unwrap().unwrap(),
        b"first complete"
    );
    assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 1);
    assert!(first.close().is_empty());
    cache.finish(Ok(()), &control).unwrap();
}

#[test]
fn facts_primary_failure_keeps_details_and_late_cancel_after_checked_stage_close() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cache = cache(&f, &control);
    let stopped = OperationControl::default();
    let cancel = stopped.tracked_cancel_handle().unwrap();
    let key = storage::sha256(b"compound causes");
    let error = cache
        .publish_profiled(
            &key,
            b"private candidate",
            &stopped,
            &mut |_| {
                Err(ToolkitError::new("fixture.first", "first write failure", 5)
                    .with_details(serde_json::json!({"originalMarker":"kept"})))
            },
            Some(&mut |phase, _| {
                if phase == SourceCachePublicationPhase::StageClose {
                    cancel.cancel();
                }
            }),
        )
        .unwrap_err();
    assert_eq!(error.code, "fixture.first");
    assert_eq!(error.details["state"], "NotPublished");
    let causes = error.details.to_string();
    for expected in [
        "fixture.first",
        "originalMarker",
        "kept",
        "command.cancelled",
    ] {
        assert!(causes.contains(expected), "lost original cause: {expected}");
    }
    assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 0);
    cache.finish(Ok(()), &control).unwrap();
}

#[test]
fn facts_deadline_remains_absolute_at_seal_and_discards_only_its_stage() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cache = cache(&f, &control);
    let key = storage::sha256(b"absolute deadline");
    cache.publish(&key, b"old", &control).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let bounded = OperationControl::uncancelable(deadline);
    let mut reached_seal = false;
    let error = cache
        .publish_observed(&key, b"new", &bounded, |at| {
            if at == "sealed" {
                reached_seal = true;
                thread::sleep(
                    deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
                );
            }
            Ok(())
        })
        .unwrap_err();
    assert!(
        reached_seal,
        "fixture never reached seal within its original preparation budget: {error}"
    );
    assert_eq!(error.code, "command.deadline");
    assert_eq!(error.details["state"], "NotPublished");
    assert_eq!(cache.read(&key, &control).unwrap().unwrap(), b"old");
    assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 1);
    cache.finish(Ok(()), &control).unwrap();
}

#[test]
fn facts_finish_closes_only_its_directory_and_preserves_all_causes() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cache = cache(&f, &control);
    cache
        .publish(&storage::sha256(b"close"), b"complete", &control)
        .unwrap();
    let cancel = control.tracked_cancel_handle().unwrap();
    let mut closes = 0;
    let primary: Result<()> = Err(ToolkitError::new("fixture.primary", "first cause", 5));
    let error = cache
        .finish_with(primary, &control, |directory| {
            closes += 1;
            let mut errors = directory.close().err().unwrap_or_default();
            errors.push(std::io::Error::other("fixture.directory_close"));
            cancel.cancel();
            Err(errors)
        })
        .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.code, "fixture.primary");
    let causes = error.details.to_string();
    for cause in [
        "fixture.primary",
        "fixture.directory_close",
        "command.cancelled",
    ] {
        assert!(causes.contains(cause), "lost cause: {cause}");
    }
}

struct ChildOwner(Option<Child>);
impl ChildOwner {
    fn start(home: &Path, key: &str, id: &str, fill: u8, repeats: usize) -> Self {
        Self(Some(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cache::source_facts::atomic_publication_tests::facts_publication_process_helper", "--nocapture"])
            .env("LYCHEEDEV_FACTS_PUBLICATION_HOME", home)
            .env("LYCHEEDEV_FACTS_PUBLICATION_KEY", key)
            .env("LYCHEEDEV_FACTS_PUBLICATION_ID", id)
            .env("LYCHEEDEV_FACTS_PUBLICATION_FILL", fill.to_string())
            .env("LYCHEEDEV_FACTS_PUBLICATION_REPEATS", repeats.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn().unwrap()))
    }
    fn wait_ready(&mut self, home: &Path, id: &str, deadline: Instant) {
        while !home.join(format!("{id}.ready")).try_exists().unwrap() {
            assert!(
                self.0.as_mut().unwrap().try_wait().unwrap().is_none(),
                "helper exited before stage readiness"
            );
            assert!(Instant::now() < deadline, "helper readiness deadline");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn finished(&mut self) -> bool {
        self.0.as_mut().unwrap().try_wait().unwrap().is_some()
    }
    fn finish(&mut self, deadline: Instant) {
        while !self.finished() {
            assert!(Instant::now() < deadline, "helper publication deadline");
            thread::sleep(Duration::from_millis(5));
        }
        let status = self.0.as_mut().unwrap().wait().unwrap();
        self.0.take();
        assert!(status.success());
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            if !matches!(child.try_wait(), Ok(Some(_))) {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}
#[test]
fn facts_publication_process_helper() {
    let Some(home) = std::env::var_os("LYCHEEDEV_FACTS_PUBLICATION_HOME") else {
        return;
    };
    let home = PathBuf::from(home);
    let key = std::env::var("LYCHEEDEV_FACTS_PUBLICATION_KEY").unwrap();
    let id = std::env::var("LYCHEEDEV_FACTS_PUBLICATION_ID").unwrap();
    let fill: u8 = std::env::var("LYCHEEDEV_FACTS_PUBLICATION_FILL")
        .unwrap()
        .parse()
        .unwrap();
    let repeats: usize = std::env::var("LYCHEEDEV_FACTS_PUBLICATION_REPEATS")
        .unwrap()
        .parse()
        .unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(30));
    let cache = SourceCache::open(&home, Kind::Facts, &control).unwrap();
    let body = vec![fill; 131073];
    let mut ready = false;
    let result = (|| {
        for _ in 0..repeats {
            cache.publish_observed(&key, &body, &control, |at| {
                if at == "sealed" && !ready {
                    fs::write(home.join(format!("{id}.ready")), b"sealed")?;
                    ready = true;
                    while !home.join(format!("{id}.go")).try_exists()? {
                        control.check()?;
                        thread::sleep(Duration::from_millis(5));
                    }
                }
                Ok(())
            })?;
        }
        Ok(())
    })();
    cache.finish(result, &control).unwrap();
}

#[test]
fn two_processes_hold_distinct_same_key_stages_then_last_replace_wins() {
    for second_fill in [17, 29] {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cache = cache(&f, &control);
        let key = storage::sha256(b"process shared identity");
        let old_body = vec![3; 131073];
        cache.publish(&key, &old_body, &control).unwrap();
        let mut old_reader = cache
            .directory
            .open_optional_project_file_controlled(
                OsStr::new(&cache.name(&key).unwrap()),
                &mut Controlled(&control),
            )
            .unwrap()
            .unwrap();
        let before_locks = lock_names(&f.0);
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut first = ChildOwner::start(&f.0, &key, "first", 17, 1);
        let mut second = ChildOwner::start(&f.0, &key, "second", second_fill, 1);
        first.wait_ready(&f.0, "first", deadline);
        second.wait_ready(&f.0, "second", deadline);
        // Both are sealed before either may publish: the test cannot pass by
        // serializing their private-stage lifetime under an old resource lock.
        assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 3);
        assert_eq!(cache.read(&key, &control).unwrap().unwrap(), old_body);
        fs::write(f.0.join("first.go"), b"publish").unwrap();
        first.finish(deadline);
        assert_eq!(
            cache.read(&key, &control).unwrap().unwrap(),
            vec![17; 131073]
        );
        fs::write(f.0.join("second.go"), b"publish").unwrap();
        second.finish(deadline);
        assert_eq!(
            cache.read(&key, &control).unwrap().unwrap(),
            vec![second_fill; 131073]
        );
        let mut old = Vec::new();
        old_reader.read_to_end(&mut old).unwrap();
        assert_eq!(old, old_body);
        storage::finish_file(old_reader, Ok(()), &control).unwrap();
        assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 1);
        assert_eq!(lock_names(&f.0), before_locks);
        cache.finish(Ok(()), &control).unwrap();
    }
}

#[test]
fn concurrent_process_replacements_repair_corrupt_bytes_and_read_only_complete_snapshots() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cache = cache(&f, &control);
    let key = storage::sha256(b"process corrupt repair");
    fs::write(
        cache.directory.path().join(cache.name(&key).unwrap()),
        b"corrupt",
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut first = ChildOwner::start(&f.0, &key, "first", 17, 16);
    let mut second = ChildOwner::start(&f.0, &key, "second", 29, 16);
    first.wait_ready(&f.0, "first", deadline);
    second.wait_ready(&f.0, "second", deadline);
    fs::write(f.0.join("first.go"), b"publish").unwrap();
    fs::write(f.0.join("second.go"), b"publish").unwrap();
    loop {
        assert!(Instant::now() < deadline, "concurrent publishers stalled");
        let body = cache.read(&key, &control).unwrap().unwrap();
        assert!(body == b"corrupt" || body == vec![17; 131073] || body == vec![29; 131073]);
        if first.finished() && second.finished() {
            break;
        }
    }
    first.finish(deadline);
    second.finish(deadline);
    let body = cache.read(&key, &control).unwrap().unwrap();
    assert!(body == vec![17; 131073] || body == vec![29; 131073]);
    assert_eq!(fs::read_dir(cache.directory.path()).unwrap().count(), 1);
    cache.finish(Ok(()), &control).unwrap();
}
