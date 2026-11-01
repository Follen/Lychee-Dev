use super::*;
use crate::test_support::TestDir;
use std::cell::{Cell, RefCell};
struct Fixture {
    dir: TestDir,
    root: PathBuf,
    profile: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = TestDir::new();
        let root = dir.0.join("roaming/CCVoiceHub/web_cache");
        let profile = root.join("current account/EBWebView");
        std::fs::create_dir_all(&profile).unwrap();
        Self { dir, root, profile }
    }
    fn rows(&self) -> Vec<Row> {
        [
            (10, 1, GUI),
            (11, 10, WEBVIEW),
            (12, 11, WEBVIEW),
            (99, 1, WEBVIEW),
        ]
        .into_iter()
        .map(|(pid, parent, name)| Row {
            pid,
            parent,
            name: name.into(),
        })
        .collect()
    }
    fn metadata(&self, pid: u32, with_arguments: bool) -> Result<Metadata> {
        assert_ne!(pid, 99, "an unrelated browser must never be inspected");
        let image = if pid == 10 {
            self.dir.0.join("program/netease_dd.exe")
        } else {
            self.dir.0.join("runtime/msedgewebview2.exe")
        };
        let arguments = with_arguments.then(|| {
            if pid == 11 {
                vec!["--type=renderer".into()]
            } else {
                let mut profile = OsString::from("--user-data-dir=");
                profile.push(&self.profile);
                vec![NETWORK.into(), profile]
            }
        });
        Ok(Metadata {
            identity: ProcessIdentity {
                pid,
                created: 10000 + pid as u64,
            },
            image,
            arguments,
        })
    }
    fn verified(path: &Path, publishers: &[&str]) -> Result<VerifiedImage> {
        Ok(VerifiedImage {
            path: path.into(),
            publisher: publishers[0].into(),
            version: "synthetic-version".into(),
        })
    }
    fn select(&self) -> Result<Option<Desktop>> {
        select_with(
            &self.rows(),
            &self.root,
            &OperationControl::default(),
            |p, a| self.metadata(p, a),
            Self::verified,
        )
    }
}
#[test]
fn only_the_verified_desktop_descendant_network_role_selects_its_own_profile() {
    let fixture = Fixture::new();
    let checked = RefCell::new(Vec::new());
    let desktop = select_with(
        &fixture.rows(),
        &fixture.root,
        &OperationControl::default(),
        |p, a| fixture.metadata(p, a),
        |path, publishers| {
            checked.borrow_mut().push(publishers[0].to_string());
            Fixture::verified(path, publishers)
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(desktop.gui.pid, 10);
    assert_eq!(desktop.network.pid, 12);
    assert_eq!(desktop.profile, fixture.profile.canonicalize().unwrap());
    assert_eq!(
        &*checked.borrow(),
        &[NETEASE[0].to_string(), MICROSOFT[0].to_string()]
    );
    assert_eq!(
        desktop.installation.program_dir,
        fixture.dir.0.join("program")
    );
}
#[test]
fn channel_anchor_rejects_replacement_browser_profile_and_gui_lifetime_without_selecting_a_new_account()
 {
    let fixture = Fixture::new();
    let original = fixture.select().unwrap().unwrap().channel_anchor();
    assert!(original.ccid().is_err());
    let mut original = original;
    original.profile = fixture.root.join("1234/EBWebView");
    assert_eq!(original.ccid().unwrap(), "1234");
    for change in 0..3 {
        let mut actual = original.clone();
        match change {
            0 => actual.gui.created += 1,
            1 => actual.network.created += 1,
            2 => actual.profile = fixture.root.join("5678/EBWebView"),
            _ => unreachable!(),
        }
        assert!(original.matches(&actual).is_err());
    }
    original.matches(&original.clone()).unwrap();
}
#[test]
fn selection_failures_never_become_native_fallback_or_a_second_login() {
    let fixture = Fixture::new();
    let native = Cell::new(0);
    let borrowed = Cell::new(0);
    let error = super::super::session::route(
        fixture.select(),
        || {
            native.set(native.get() + 1);
            Ok(())
        },
        |_| {
            borrowed.set(borrowed.get() + 1);
            Err(fault("synthetic borrow failure"))
        },
    )
    .unwrap_err();
    assert_eq!(error.message, "synthetic borrow failure");
    assert_eq!(native.get(), 0);
    assert_eq!(borrowed.get(), 1);
    super::super::session::route::<()>(
        Err(fault("synthetic inventory failure")),
        || {
            native.set(native.get() + 1);
            Ok(())
        },
        |_| {
            borrowed.set(borrowed.get() + 1);
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(native.get(), 0);
    assert_eq!(borrowed.get(), 1);
    let absent = select_with(
        &[],
        Path::new("unavailable-root"),
        &OperationControl::default(),
        |_, _| panic!("no process inspection without a GUI"),
        |_, _| panic!("no signature request without a GUI"),
    )
    .unwrap();
    super::super::session::route(
        Ok(absent),
        || {
            native.set(native.get() + 1);
            Ok(())
        },
        |_| panic!("absent GUI cannot select borrowed mode"),
    )
    .unwrap();
    assert_eq!(native.get(), 1);
}
#[test]
fn ambiguous_instances_profiles_signatures_and_restarted_lifetimes_are_rejected() {
    let fixture = Fixture::new();
    let control = OperationControl::default();
    let mut rows = fixture.rows();
    rows.push(Row {
        pid: 20,
        parent: 1,
        name: GUI.into(),
    });
    assert!(
        select_with(
            &rows,
            &fixture.root,
            &control,
            |p, a| fixture.metadata(p, a),
            Fixture::verified
        )
        .is_err()
    );
    for failed in [NETEASE[0], MICROSOFT[0]] {
        assert!(
            select_with(
                &fixture.rows(),
                &fixture.root,
                &control,
                |p, a| fixture.metadata(p, a),
                |path, publishers| {
                    if publishers[0] == failed {
                        Err(fault("synthetic rejected signature"))
                    } else {
                        Fixture::verified(path, publishers)
                    }
                }
            )
            .is_err()
        );
    }
    let calls = Cell::new(0);
    assert!(
        select_with(
            &fixture.rows(),
            &fixture.root,
            &control,
            |p, a| {
                let mut metadata = fixture.metadata(p, a)?;
                if p == 10 {
                    calls.set(calls.get() + 1);
                    if calls.get() == 2 {
                        metadata.identity.created += 1;
                    }
                }
                Ok(metadata)
            },
            Fixture::verified
        )
        .is_err()
    );
    let mut rows = fixture.rows();
    rows.push(Row {
        pid: 13,
        parent: 10,
        name: WEBVIEW.into(),
    });
    assert!(
        select_with(
            &rows,
            &fixture.root,
            &control,
            |p, a| fixture.metadata(p, a),
            Fixture::verified
        )
        .is_err()
    );
}
#[test]
fn outside_duplicate_or_missing_profiles_and_browser_lifetimes_cannot_read_credentials() {
    let fixture = Fixture::new();
    let control = OperationControl::default();
    let outside = fixture.dir.0.join("outside/EBWebView");
    std::fs::create_dir_all(&outside).unwrap();
    for mode in ["outside", "duplicate", "missing", "old-browser"] {
        assert!(
            select_with(
                &fixture.rows(),
                &fixture.root,
                &control,
                |p, a| {
                    let mut metadata = fixture.metadata(p, a)?;
                    if p == 12 {
                        let args = metadata.arguments.as_mut().unwrap();
                        match mode {
                            "outside" => {
                                let mut arg = OsString::from("--user-data-dir=");
                                arg.push(&outside);
                                args[1] = arg;
                            }
                            "duplicate" => args.push(args[1].clone()),
                            "missing" => {
                                args.pop();
                            }
                            "old-browser" => metadata.identity.created = 1,
                            _ => unreachable!(),
                        }
                    }
                    Ok(metadata)
                },
                Fixture::verified
            )
            .is_err()
        );
    }
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        select_with(
            &fixture.rows(),
            &fixture.root,
            &control,
            |_, _| panic!("cancel before metadata"),
            Fixture::verified
        )
        .err()
        .unwrap()
        .exit_code,
        7
    );
}
