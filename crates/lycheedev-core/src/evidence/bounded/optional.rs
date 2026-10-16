//! Optional cache lookup is classified at native acquisition, before converting
//! IO errors to ToolkitError. An acquired body can never become a cache miss.
use super::*;
use lycheedev_windows::filesystem::destination::{FileControl, FileCost};

// Share the original invocation and caller budget through every native directory
// walk and ancestry-verification step. FileCost is not a new ledger: these APIs
// add cancellation checks only, leaving existing Data accounting unchanged.
struct LookupControl<'a> {
    control: &'a OperationControl,
    supplemental: &'a mut dyn FnMut() -> Result<()>,
}
impl FileControl for LookupControl<'_> {
    type Error = ToolkitError;
    fn charge(&mut self, _: FileCost) -> Result<()> {
        self.check()
    }
    fn check(&mut self) -> Result<()> {
        self.control.check()?;
        (self.supplemental)()
    }
}

// Terminal checks must observe both independent owners, even if the invocation
// is already stopped. Ordinary work uses check_with and still stops first.
pub(super) fn finish_checks<T>(
    result: Result<T>,
    control: &OperationControl,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<T> {
    let result = finish_check(result, control.check());
    finish_check(result, supplemental())
}
pub(super) fn finish_directory<T>(result: Result<T>, directory: Directory) -> Result<T> {
    finish_directory_close(result, directory.close())
}
fn finish_directory_close<T>(
    mut result: Result<T>,
    closed: std::result::Result<(), Vec<io::Error>>,
) -> Result<T> {
    if let Err(errors) = closed {
        for error in errors {
            // Each close is a separate event, including identical failures.
            result = crate::metadata::historical_write::finish_historical(
                result,
                Err(ToolkitError::from(error)),
            );
        }
    }
    result
}
fn close_optional_opened<T>(
    opened: OpenBlob,
    result: Result<T>,
    control: &OperationControl,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<T> {
    let result = crate::metadata::historical_write::finish_historical(
        result,
        lycheedev_windows::filesystem::close_file(opened.file).map_err(ToolkitError::from),
    );
    finish_checks(
        finish_directory(result, opened._directory),
        control,
        supplemental,
    )
}

fn open_optional_blob(
    home: &Path,
    reference: BlobRef,
    control: &OperationControl,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<Option<OpenBlob>> {
    check_with(Some(control), supplemental)?;
    let path = blob_path(home, &reference.sha256)?;
    storage::guard_path_with_check(home, &path, || check_with(Some(control), supplemental))?;
    // Hold every workspace/blobs ancestor against deletion. Failure to open
    // that required parent is an error. Only the subsequent shard can be absent.
    let parent = Directory::open_shared_controlled(
        &home.join("blobs"),
        &mut LookupControl {
            control,
            supplemental,
        },
    )?;
    let result = (|| {
        check_with(Some(control), supplemental)?;
        let shard_name = path
            .parent()
            .expect("blob shard")
            .file_name()
            .expect("shard name");
        let shard_file = parent.open_optional_file_controlled(
            shard_name,
            &mut LookupControl {
                control,
                supplemental,
            },
        )?;
        let Some(shard_file) = shard_file else {
            return Ok(None);
        };
        #[cfg(test)]
        test_hooks::after_shard();
        // Native Reader accepts a directory entry (no NON_DIRECTORY option).
        // Keep it against delete while requiring the checked directory walk;
        // errors from that walk or metadata are never classified as a miss.
        let acquired = (|| {
            check_with(Some(control), supplemental)?;
            Directory::open_controlled(
                &parent.path().join(shard_name),
                &mut LookupControl {
                    control,
                    supplemental,
                },
            )
        })();
        let closed =
            lycheedev_windows::filesystem::close_file(shard_file).map_err(ToolkitError::from);
        let directory = match acquired {
            Ok(directory) => match finish_checks(closed, control, supplemental) {
                Ok(()) => directory,
                Err(error) => return finish_directory(Err(error), directory),
            },
            Err(error) => {
                return finish_checks(
                    crate::metadata::historical_write::finish_historical(Err(error), closed),
                    control,
                    supplemental,
                );
            }
        };
        let result = (|| {
            check_with(Some(control), supplemental)?;
            let name = path.file_name().expect("blob digest name");
            #[cfg(test)]
            test_hooks::before_metadata();
            let info = directory.optional_metadata_controlled(
                name,
                &mut LookupControl {
                    control,
                    supplemental,
                },
            );
            let Some(info) = finish_checks(info, control, supplemental)? else {
                return Ok(None);
            };
            validate_metadata(&info, reference.bytes)?;
            check_with(Some(control), supplemental)?;
            #[cfg(test)]
            test_hooks::before_body();
            let file = directory.open_optional_file_controlled(
                name,
                &mut LookupControl {
                    control,
                    supplemental,
                },
            )?;
            let Some(file) = file else {
                return Ok(None);
            };
            let result = (|| {
                check_with(Some(control), supplemental)?;
                let info = file.metadata()?;
                validate_metadata(&info, reference.bytes)
            })();
            let result = finish_checks(result, control, supplemental);
            match result {
                Ok(()) => Ok(Some(file)),
                Err(error) => crate::metadata::historical_write::finish_historical(
                    Err(error),
                    lycheedev_windows::filesystem::close_file(file).map_err(ToolkitError::from),
                ),
            }
        })();
        match result {
            Ok(Some(file)) => Ok(Some(OpenBlob {
                file,
                _directory: directory,
                reference,
            })),
            result => finish_checks(finish_directory(result, directory), control, supplemental)
                .map(|_| None),
        }
    })();
    // Close the extra parent before exposing either a miss or a body owner.
    // If cleanup or stop fails after acquisition, close the body as well.
    match result {
        Ok(Some(opened)) => {
            let closed = finish_checks(finish_directory(Ok(()), parent), control, supplemental);
            match closed {
                Ok(()) => Ok(Some(opened)),
                Err(error) => close_optional_opened(opened, Err(error), control, supplemental),
            }
        }
        result => finish_checks(finish_directory(result, parent), control, supplemental),
    }
}

/// Return None only for an absent blob shard or entry at native open. Invalid
/// workspace/ref, integrity, permission, cancellation and cleanup remain errors.
/// This does not add network or retained-byte accounting for cached disk bytes.
pub fn read_blob_bounded_optional_with_check(
    home: &Path,
    blob: &Value,
    maximum: usize,
    control: &OperationControl,
    mut supplemental: impl FnMut() -> Result<()>,
) -> Result<Option<Vec<u8>>> {
    check_with(Some(control), &mut supplemental)?;
    open_workspace(home, Some(control), &mut supplemental)?;
    let reference = admitted_ref(blob, maximum as u64)?;
    let Some(mut opened) = open_optional_blob(home, reference, control, &mut supplemental)? else {
        return Ok(None);
    };
    let result = (|| {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(opened.reference.bytes as usize)
            .map_err(|_| limit("body allocation was not admitted"))?;
        scan_open_blob_with_check(&mut opened, Some(control), &mut supplemental, |block| {
            bytes.extend_from_slice(block);
            Ok(())
        })?;
        Ok(Some(bytes))
    })();
    close_optional_opened(opened, result, control, &mut supplemental)
}

/// Optional counterpart of the held verified stream. The same caller-owned
/// check remains alive through returned Read, rewind and consuming close.
pub fn open_verified_blob_bounded_optional_with_check(
    home: &Path,
    blob: &Value,
    maximum: u64,
    control: &OperationControl,
    accept: impl FnMut(&[u8]) -> Result<()>,
    supplemental: impl FnMut() -> Result<()> + Send + 'static,
) -> Result<Option<VerifiedBlob>> {
    let mut supplemental: Option<BlobCheck> = Some(Box::new(supplemental));
    stream_check(control, &mut supplemental)?;
    open_workspace(home, Some(control), &mut || {
        optional_check(&mut supplemental)
    })?;
    let reference = admitted_ref(blob, maximum)?;
    let Some(opened) = open_optional_blob(home, reference, control, &mut || {
        optional_check(&mut supplemental)
    })?
    else {
        return Ok(None);
    };
    let opened = prepare_opened_verified_blob(opened, control, accept, &mut supplemental)?;
    Ok(Some(VerifiedBlob {
        opened,
        home: home.to_owned(),
        control: control.clone(),
        supplemental,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_directory_close_events_are_retained_separately() {
        let errors = vec![
            io::Error::other("same-close"),
            io::Error::other("same-close"),
        ];
        let error = finish_directory_close::<()>(Ok(()), Err(errors)).unwrap_err();
        assert_eq!(error.message.matches("same-close").count(), 2);
        assert!(error.details.get("primary").is_some());
        assert!(error.details.get("cleanup").is_some());
    }
    #[test]
    fn missing_result_does_not_hide_cleanup_or_either_stop() {
        use crate::control::TrackedCancellation;
        use std::time::{Duration, Instant};
        let token = TrackedCancellation::new();
        let control =
            OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
        token.cancel();
        let cleaned = finish_directory_close::<Option<()>>(
            Ok(None),
            Err(vec![io::Error::other("directory close failure")]),
        );
        let error = finish_checks(cleaned, &control, &mut || {
            Err(ToolkitError::new("fixture.budget", "budget stopped", 7))
        })
        .unwrap_err();
        assert_eq!(error.code, "external.io");
        let rendered = serde_json::to_string(&error.details).unwrap();
        for cause in [
            "directory close failure",
            "command.cancelled",
            "fixture.budget",
        ] {
            assert!(rendered.contains(cause), "missing cause: {cause}");
        }
    }
    struct NativeFixture {
        root: PathBuf,
        leaf: PathBuf,
    }
    impl NativeFixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "lychee-optional-walk-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let leaf = root.join("a/b/c/d/e/f/g/h");
            fs::create_dir_all(&leaf).unwrap();
            Self { root, leaf }
        }
    }
    impl Drop for NativeFixture {
        fn drop(&mut self) {
            let result = fs::remove_dir_all(&self.root);
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
    fn control() -> OperationControl {
        OperationControl::uncancelable(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        )
    }
    fn stop() -> ToolkitError {
        ToolkitError::new("fixture.lookup_stop", "lookup stopped", 7)
    }

    #[test]
    fn controlled_directory_walk_observes_mid_walk_budget_and_releases_native_handles() {
        let f = NativeFixture::new();
        for shared in [false, true] {
            let inv = control();
            let mut total = 0;
            let mut count = || {
                total += 1;
                Ok(())
            };
            let mut bridge = LookupControl {
                control: &inv,
                supplemental: &mut count,
            };
            let directory = if shared {
                Directory::open_shared_controlled(&f.leaf, &mut bridge)
            } else {
                Directory::open_controlled(&f.leaf, &mut bridge)
            }
            .unwrap();
            finish_directory(Ok(()), directory).unwrap();
            assert!(total > 16);
            let mut calls = 0;
            let mut check = || {
                calls += 1;
                if calls >= total / 2 {
                    Err(stop())
                } else {
                    Ok(())
                }
            };
            let mut bridge = LookupControl {
                control: &inv,
                supplemental: &mut check,
            };
            let result = if shared {
                Directory::open_shared_controlled(&f.leaf, &mut bridge)
            } else {
                Directory::open_controlled(&f.leaf, &mut bridge)
            };
            let error = match result {
                Err(error) => error,
                Ok(directory) => {
                    finish_directory(Ok(()), directory).unwrap();
                    panic!("walk did not stop")
                }
            };
            assert_eq!(error.code, "fixture.lookup_stop");
            assert_eq!(calls, total / 2);
            inv.check().unwrap();
            // A failed native construction owns the Drop boundary. Its acquired
            // directory handles have been released, even before Core gets one.
            let moved = f.root.with_extension("moved");
            fs::rename(&f.root, &moved).unwrap();
            fs::rename(&moved, &f.root).unwrap();
        }
    }

    #[test]
    fn optional_native_entry_verification_checks_original_invocation_before_budget() {
        use crate::control::TrackedCancellation;
        use std::time::{Duration, Instant};
        let f = NativeFixture::new();
        let directory = Directory::open(&f.leaf).unwrap();
        let token = TrackedCancellation::new();
        let inv =
            OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
        token.cancel();
        let mut must_not_run = || panic!("invocation must precede independent budget");
        let mut bridge = LookupControl {
            control: &inv,
            supplemental: &mut must_not_run,
        };
        assert_eq!(
            directory
                .open_optional_file_controlled(std::ffi::OsStr::new("missing"), &mut bridge)
                .unwrap_err()
                .code,
            "command.cancelled"
        );
        let inv = control();
        let mut checks = 0;
        let mut check = || {
            checks += 1;
            if checks == 2 { Err(stop()) } else { Ok(()) }
        };
        let error = directory
            .optional_metadata_controlled(
                std::ffi::OsStr::new("missing"),
                &mut LookupControl {
                    control: &inv,
                    supplemental: &mut check,
                },
            )
            .unwrap_err();
        assert_eq!(error.code, "fixture.lookup_stop");
        assert_eq!(checks, 2); // first held-ancestry metadata operation, before entry acquisition
        finish_directory(Ok(()), directory).unwrap();
    }

    #[test]
    fn native_post_acquisition_stop_retains_typed_cause_and_drop_releases_file() {
        let f = NativeFixture::new();
        let path = f.leaf.join("body");
        fs::write(&path, b"native").unwrap();
        let directory = Directory::open(&f.leaf).unwrap();
        let inv = control();
        let mut total = 0;
        let mut count = || {
            total += 1;
            Ok(())
        };
        let file = directory
            .open_optional_file_controlled(
                std::ffi::OsStr::new("body"),
                &mut LookupControl {
                    control: &inv,
                    supplemental: &mut count,
                },
            )
            .unwrap()
            .unwrap();
        lycheedev_windows::filesystem::close_file(file).unwrap();
        let mut calls = 0;
        let mut check = || {
            calls += 1;
            if calls == total { Err(stop()) } else { Ok(()) }
        };
        let error = directory
            .open_optional_file_controlled(
                std::ffi::OsStr::new("body"),
                &mut LookupControl {
                    control: &inv,
                    supplemental: &mut check,
                },
            )
            .unwrap_err();
        assert_eq!(error.code, "fixture.lookup_stop");
        assert_eq!(calls, total);
        // The Windows API checks after acquisition, before returning ownership.
        // Its error uses Drop, not Core's injectable checked-close boundary.
        fs::remove_file(&path).unwrap();
        finish_directory(Ok(()), directory).unwrap();
    }
}

// Test-only one-shot race fixtures. They do not change the public API or add a
// production observer; explicit phases avoid counting platform-dependent native
// control checks to cause a disappearance at a particular acquisition boundary.
#[cfg(test)]
pub(super) mod test_hooks {
    use std::cell::RefCell;
    thread_local! {
        static AFTER_SHARD: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
        static BEFORE_METADATA: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
        static BEFORE_BODY: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    }
    pub(in super::super) fn set_after_shard(run: impl FnOnce() + 'static) {
        AFTER_SHARD.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(Box::new(run));
        });
    }
    pub(super) fn after_shard() {
        let run = AFTER_SHARD.with(|slot| slot.borrow_mut().take());
        if let Some(run) = run {
            run();
        }
    }
    pub(in super::super) fn set_before_metadata(run: impl FnOnce() + 'static) {
        BEFORE_METADATA.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(Box::new(run));
        });
    }
    pub(in super::super) fn set_before_body(run: impl FnOnce() + 'static) {
        BEFORE_BODY.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(Box::new(run));
        });
    }
    pub(super) fn before_metadata() {
        let run = BEFORE_METADATA.with(|slot| slot.borrow_mut().take());
        if let Some(run) = run {
            run();
        }
    }
    pub(super) fn before_body() {
        let run = BEFORE_BODY.with(|slot| slot.borrow_mut().take());
        if let Some(run) = run {
            run();
        }
    }
}
