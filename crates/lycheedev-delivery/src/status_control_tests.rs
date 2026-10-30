//! Public status consumers must preserve their invocation control and bounded,
//! checked file reads. Observers only stop the caller or inject a finish result;
//! the production selection, reads, validation and close still execute.
use crate::{AddonSelector, Request, dispatch, installation};
use lycheedev_core::{ToolkitError, control::OperationControl, request::RequestContext, storage};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadPoint {
    Main,
    Slots,
}
type Before = Box<dyn FnMut(ReadPoint, &OperationControl)>;
type Finish = Box<dyn FnMut(&OperationControl) -> Option<ToolkitError>>;
thread_local! {
    static BEFORE: RefCell<Option<Before>> = RefCell::new(None);
    static FINISH: RefCell<Option<Finish>> = RefCell::new(None);
}
pub(crate) fn before_inspect(point: ReadPoint, control: &OperationControl) {
    BEFORE.with_borrow_mut(|observer| {
        if let Some(observer) = observer {
            observer(point, control);
        }
    });
}
pub(crate) fn after_inventory_finish(
    result: lycheedev_core::Result<()>,
    control: &OperationControl,
) -> lycheedev_core::Result<()> {
    result?;
    match FINISH.with_borrow_mut(|observer| observer.as_mut().and_then(|run| run(control))) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
struct Observation;
impl Observation {
    fn before(run: impl FnMut(ReadPoint, &OperationControl) + 'static) -> Self {
        BEFORE.with_borrow_mut(|observer| {
            assert!(observer.is_none());
            *observer = Some(Box::new(run));
        });
        Self
    }
    fn finish(run: impl FnMut(&OperationControl) -> Option<ToolkitError> + 'static) -> Self {
        FINISH.with_borrow_mut(|observer| {
            assert!(observer.is_none());
            *observer = Some(Box::new(run));
        });
        Self
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        BEFORE.with_borrow_mut(|observer| *observer = None);
        FINISH.with_borrow_mut(|observer| *observer = None);
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    client: PathBuf,
    addon: PathBuf,
    skill: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-status-control-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        let client = root.join("_classic_titan_");
        let addon = client.join("Interface/AddOns/Lychee Dev");
        let skill = root.join("skills/lycheedev");
        fs::create_dir_all(&addon).unwrap();
        fs::create_dir_all(&skill).unwrap();
        fs::write(client.join(".flavor.info"), "wow_classic_titan").unwrap();
        fs::write(client.join("version.txt"), "3.80.2.69874").unwrap();
        for (path, component) in [(&addon, "addon"), (&skill, "skill")] {
            fs::write(path.join("main.lua"), b"return 1\n").unwrap();
            storage::atomic_json(
                &path.join(installation::MARKER),
                &json!({
                    "schema":"lycheedev.installation.v1", "component":component,
                    "version":lycheedev_core::VERSION, "commit":"a".repeat(40),
                    "resources":[{"path":format!("{component}/main.lua"), "bytes":9,
                        "sha256":storage::sha256(b"return 1\n")}],
                }),
            )
            .unwrap();
        }
        Self {
            root,
            client,
            addon,
            skill,
        }
    }
    fn context(&self) -> RequestContext {
        RequestContext {
            home: self.root.join("absent-home"),
            cwd: self.root.clone(),
            control: OperationControl::default(),
        }
    }
    fn requests(&self) -> [(Request, &Path); 3] {
        [
            (
                Request::AddonStatus(AddonSelector::Installation(self.client.clone())),
                &self.addon,
            ),
            (
                Request::AddonStatus(AddonSelector::Path(self.addon.clone())),
                &self.addon,
            ),
            (
                Request::SkillStatus {
                    path: self.skill.clone(),
                },
                &self.skill,
            ),
        ]
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn installation_result(request: &Request, data: serde_json::Value) -> serde_json::Value {
    if matches!(
        request,
        Request::AddonStatus(AddonSelector::Installation(_))
    ) {
        data["installation"].clone()
    } else {
        data
    }
}

#[test]
fn status_main_reads_preserve_original_cancel_and_deadline() {
    for deadline in [false, true] {
        let f = Fixture::new();
        for (request, path) in f.requests() {
            let mut ctx = f.context();
            if deadline {
                ctx.control =
                    OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
            }
            let original = ctx.control.clone();
            let cancel = original.tracked_cancel_handle().unwrap();
            let seen = Rc::new(Cell::new(0));
            let observed = seen.clone();
            let observer = Observation::before(move |point, control| {
                assert_eq!(point, ReadPoint::Main);
                assert!(control.same_scope(&original));
                assert_eq!(control.deadline(), original.deadline());
                observed.set(observed.get() + 1);
                if deadline {
                    std::thread::sleep(
                        control.deadline().saturating_duration_since(Instant::now())
                            + Duration::from_millis(1),
                    );
                } else {
                    cancel.cancel();
                }
            });
            let receipt = fs::read(path.join(installation::MARKER)).unwrap();
            let error = dispatch(&ctx, &request).unwrap_err();
            drop(observer);
            assert_eq!(seen.get(), 1);
            let expected = ctx.control.check().unwrap_err();
            assert_eq!(error.code, expected.code, "{request:?}");
            assert_eq!(error.exit_code, expected.exit_code);
            assert_eq!(fs::read(path.join(installation::MARKER)).unwrap(), receipt);
            assert_eq!(fs::read(path.join("main.lua")).unwrap(), b"return 1\n");
            assert!(!ctx.home.exists());
        }
    }
}

#[test]
fn status_slot_read_preserves_original_stop_instead_of_incomplete_success() {
    for deadline in [false, true] {
        let f = Fixture::new();
        let marker = f.addon.parent().unwrap().join(crate::slots::MARKER);
        // Stop must win before interpreting an otherwise invalid inventory.
        fs::write(&marker, b"{}").unwrap();
        let mut ctx = f.context();
        if deadline {
            ctx.control =
                OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
        }
        let original = ctx.control.clone();
        let cancel = original.tracked_cancel_handle().unwrap();
        let seen = Rc::new(Cell::new(0));
        let observed = seen.clone();
        let observer = Observation::before(move |point, control| {
            assert!(control.same_scope(&original));
            assert_eq!(control.deadline(), original.deadline());
            if point == ReadPoint::Slots {
                observed.set(observed.get() + 1);
                if deadline {
                    std::thread::sleep(
                        control.deadline().saturating_duration_since(Instant::now())
                            + Duration::from_millis(1),
                    );
                } else {
                    cancel.cancel();
                }
            }
        });
        let error = dispatch(
            &ctx,
            &Request::AddonStatus(AddonSelector::Installation(f.client.clone())),
        )
        .unwrap_err();
        drop(observer);
        assert_eq!(seen.get(), 1);
        assert_eq!(error.code, ctx.control.check().unwrap_err().code);
        assert_eq!(fs::read(marker).unwrap(), b"{}");
        assert!(!ctx.home.exists());
    }
}

#[test]
fn status_does_not_downgrade_checked_inventory_close_failure() {
    let f = Fixture::new();
    for (request, path) in f.requests() {
        let ctx = f.context();
        let failure = ToolkitError::new(
            "delivery.invalid_payload",
            "injected checked finish failure",
            5,
        )
        .with_details(
            json!({"schema":storage::FILE_CLEANUP_SCHEMA,"close":{"code":"fixture.close"}}),
        );
        let expected = serde_json::to_value(&failure).unwrap();
        let finishes = Rc::new(Cell::new(0));
        let seen = finishes.clone();
        let observer = Observation::finish(move |_| {
            seen.set(seen.get() + 1);
            Some(failure.clone())
        });
        let error = dispatch(&ctx, &request).unwrap_err();
        drop(observer);
        assert_eq!(finishes.get(), 1);
        assert_eq!(serde_json::to_value(error).unwrap(), expected);
        // The fault is injected after the real held file was closed.
        fs::write(path.join("main.lua"), b"return 1\n").unwrap();
        assert!(!ctx.home.exists());
    }
}

#[cfg(windows)]
fn deny_data_reads(path: &Path) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_SHARE_WRITE | FILE_SHARE_DELETE: attributes remain observable while
    // attempts to read file data fail. No large payload is allocated in RAM.
    fs::OpenOptions::new()
        .write(true)
        .share_mode(2 | 4)
        .open(path)
        .unwrap()
}

#[cfg(windows)]
#[test]
fn status_rejects_oversize_receipt_before_opening_its_data() {
    let f = Fixture::new();
    for (request, path) in f.requests() {
        let ctx = f.context();
        let marker = path.join(installation::MARKER);
        let original = fs::read(&marker).unwrap();
        let denied = deny_data_reads(&marker);
        denied.set_len((1 << 20) + 1).unwrap();
        let error = dispatch(&ctx, &request).unwrap_err();
        assert_eq!(
            error.code, "delivery.invalid_installation",
            "must reject metadata size without a data read: {request:?}"
        );
        drop(denied);
        fs::write(marker, original).unwrap();
        assert!(!ctx.home.exists());
    }
}

#[cfg(windows)]
#[test]
fn status_reports_payload_size_drift_without_reading_oversize_data() {
    let f = Fixture::new();
    for (request, path) in f.requests() {
        let ctx = f.context();
        let payload = path.join("main.lua");
        let denied = deny_data_reads(&payload);
        denied.set_len((256 << 20) + 1).unwrap();
        let result = dispatch(&ctx, &request).unwrap().data;
        assert_eq!(installation_result(&request, result)["state"], "modified");
        drop(denied);
        fs::write(payload, b"return 1\n").unwrap();
        assert!(!ctx.home.exists());
    }
}

#[cfg(windows)]
#[test]
fn status_does_not_downgrade_slot_manifest_io_failure() {
    let f = Fixture::new();
    let marker = f.addon.parent().unwrap().join(crate::slots::MARKER);
    fs::write(&marker, b"{}").unwrap();
    let denied = deny_data_reads(&marker);
    let ctx = f.context();
    let error = dispatch(
        &ctx,
        &Request::AddonStatus(AddonSelector::Installation(f.client.clone())),
    )
    .unwrap_err();
    assert_eq!(error.code, "external.io");
    drop(denied);
    assert_eq!(fs::read(marker).unwrap(), b"{}");
    assert!(!ctx.home.exists());
}
