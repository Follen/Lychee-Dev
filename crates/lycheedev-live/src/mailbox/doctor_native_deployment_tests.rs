//! Real deployment inspection and owned Windows files. No window resolution,
//! native reader, process lifetime lookup, connection or input is permitted.
use super::*;
use lycheedev_core::storage;
use lycheedev_delivery::{installation, slots};
use std::{
    os::windows::fs::OpenOptionsExt,
    sync::atomic::{AtomicU64, Ordering},
};

struct NoProcessAccess;
impl ProcessLifetimeObserver for NoProcessAccess {
    fn observe(&mut self, _: &ProcessIdentity, _: &OperationControl) -> ChannelResult<ProcessEnd> {
        panic!("deployment inspection must not observe a process");
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    target: ConnectionTarget,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-doctor-native-deployment-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let client_directory = root.join("fixture-client");
        let addon = client_directory.join("Interface/AddOns/Lychee Dev");
        fs::create_dir_all(addon.join("Bridge")).unwrap();
        let runtime = b"-- owned deployment fixture\n";
        fs::write(addon.join("Bridge/SlotRuntime.lua"), runtime).unwrap();
        let receipt = installation::Receipt {
            schema: "lycheedev.installation.v1".into(),
            component: "addon".into(),
            version: lycheedev_core::VERSION.into(),
            commit: "a".repeat(40),
            resources: vec![installation::Resource {
                path: "addon/Bridge/SlotRuntime.lua".into(),
                sha256: storage::sha256(runtime),
                bytes: runtime.len() as u64,
            }],
        };
        storage::atomic_json(&addon.join(installation::MARKER), &receipt).unwrap();
        slots::install_controlled(
            addon.parent().unwrap(),
            lycheedev_core::VERSION,
            &OperationControl::default(),
        )
        .unwrap();
        let target = ConnectionTarget {
            client: ClientRecord {
                directory: client::display_path(&client_directory),
                product: "retail".into(),
                full_build: "fixture".into(),
                ..Default::default()
            },
            // These values are deliberately not live OS identities. The real
            // inspect_deployment consumer must use only the recorded directory.
            window: WindowRecord {
                process_id: 17,
                process_started_at: 19,
                handle: 23,
                ..Default::default()
            },
        };
        Self { root, target }
    }
    fn environment(&self) -> NativeEnvironment<NoProcessAccess> {
        NativeEnvironment::new(&self.root, lycheedev_core::VERSION, NoProcessAccess).unwrap()
    }
    fn inspect(&self) -> ChannelResult<DoctorDeployment> {
        self.environment()
            .inspect_deployment(&self.target, &OperationControl::default())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn slot_io_errors_remain_typed_failures_without_authorizing_reset() {
    let f = Fixture::new();
    assert_eq!(f.inspect().unwrap().slots.unwrap().state, "managed");
    for file in [
        f.target.addon_parent().join(slots::MARKER),
        slots::directory(&f.target.addon_parent(), 1).join("Payload.lua"),
    ] {
        let original = fs::read(&file).unwrap();
        // Keep attributes observable while denying real data reads. This is
        // an actual Windows sharing failure, not a replacement slot inspector.
        let denied = fs::OpenOptions::new()
            .write(true)
            .share_mode(2 | 4)
            .open(&file)
            .unwrap();
        let result = f.inspect();
        drop(denied);
        assert_eq!(fs::read(&file).unwrap(), original);
        let error =
            result.expect_err("an unavailable read must not become repairable slot corruption");
        assert!(
            matches!(error, ChannelError::Toolkit { ref code, exit_code: 5, .. } if code == "external.io"),
            "{file:?}: {error:?}"
        );
        assert_eq!(f.inspect().unwrap().slots.unwrap().state, "managed");
    }
    assert!(!f.root.join(".lycheedev").exists());
}

#[test]
fn malformed_or_missing_slot_structure_remains_an_incomplete_diagnostic() {
    let f = Fixture::new();
    let marker = f.target.addon_parent().join(slots::MARKER);
    let original = fs::read(&marker).unwrap();
    for missing in [false, true] {
        if missing {
            fs::remove_file(&marker).unwrap();
        } else {
            fs::write(&marker, b"{").unwrap();
        }
        let deployment = f.inspect().unwrap();
        assert_eq!(deployment.state, "managed");
        assert_eq!(deployment.slots.as_ref().unwrap().state, "incomplete");
        assert!(deployment.needs_slot_reset(false));
        fs::write(&marker, &original).unwrap();
    }
    let member = slots::directory(&f.target.addon_parent(), 1).join("Payload.lua");
    let retained = f.root.join("retained-payload.lua");
    fs::rename(&member, &retained).unwrap();
    let deployment = f.inspect().unwrap();
    assert_eq!(deployment.slots.unwrap().state, "incomplete");
    fs::rename(retained, member).unwrap();
    assert_eq!(f.inspect().unwrap().slots.unwrap().state, "managed");
    assert!(!f.root.join(".lycheedev").exists());
}
