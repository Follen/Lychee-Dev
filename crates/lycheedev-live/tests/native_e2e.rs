//! Formal native host, file publication, durable journal and actual Lua 5.1 private peer.
//! Memory/optical publication is an explicit fixture; real-client WGC acceptance is not implied.
#[path = "../../lycheedev-windows/tests/support/owned_target.rs"]
mod owned_target;
#[test]
fn owned_live_target_entry() {
    owned_target::child_entry();
}
use lycheedev_core::{Invocation, Outcome, Result, storage, workspace};
use lycheedev_live::{
    host::{self, Services, Target},
    lifecycle::{Action, ActionReceipt, Session, Transport},
    mailbox::{Record, Selector, decode},
    managed::{ManagedIdentity, ManagedInstallation},
    transport::{Channel, NativeTransport, Snapshot},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    rc::Rc,
};
use windows::{
    Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
    core::w,
};
#[derive(Clone)]
struct Managed;
impl ManagedInstallation for Managed {
    fn verify_clean(&self, p: &Path, r: &str, t: &str, n: u16) -> Result<ManagedIdentity> {
        let v = lycheedev_delivery::verify_clean(p, r, t, n)?;
        Ok(ManagedIdentity {
            canonical_path: v.canonical_path,
            manifest_sha256: v.manifest_sha256,
            generation: v.generation,
        })
    }
    fn publish_slot(&self, i: &ManagedIdentity, n: u16, h: Option<&str>, b: &[u8]) -> Result<()> {
        lycheedev_delivery::slots::publish(
            &lycheedev_delivery::VerifiedInstallation {
                canonical_path: i.canonical_path.clone(),
                manifest_sha256: i.manifest_sha256.clone(),
                generation: i.generation,
            },
            n,
            h,
            b,
        )
    }
}
struct Peer {
    child: Child,
    _job: lycheedev_windows::JobGuard,
    input: ChildStdin,
    out: BufReader<ChildStdout>,
    publication: Value,
    sends: usize,
    publish_fault: bool,
    partial_input: bool,
    partial_activation: bool,
    reload_waiting: bool,
    adapter: bool,
    carrier: Option<Rc<RefCell<owned_target::Owned>>>,
    control: lycheedev_core::control::OperationControl,
}
struct OwnWindow(HWND);
impl OwnWindow {
    fn create() -> Self {
        Self(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Native E2E own HWND"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .unwrap()
        })
    }
    fn drain(&self) {
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(
                &mut message,
                Some(self.0),
                WM_KEYFIRST,
                WM_KEYLAST,
                PM_REMOVE,
            )
            .as_bool()
            {}
        }
    }
}
impl Drop for OwnWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
impl Peer {
    fn start(root: &Path) -> Self {
        Self::start_kind(root, None)
    }
    fn start_kind(root: &Path, addon: Option<&Path>) -> Self {
        let lua = std::env::var("LYCHEEDEV_LUA51")
            .unwrap_or("C:/Program Files (x86)/Lua/5.1/lua.exe".into());
        let mut command = Command::new(lua);
        command
            .arg(root.join(if addon.is_some() {
                "protocol/native-v4-adapter-host-fixture.lua"
            } else {
                "protocol/native-v4-host-fixture.lua"
            }))
            .arg(root);
        if let Some(addon) = addon {
            command
                .arg(addon)
                .arg(lycheedev_windows::random::token().unwrap());
        }
        let mut child = command
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let job = lycheedev_windows::JobGuard::attach(&child).unwrap();
        let input = child.stdin.take().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut peer = Self {
            child,
            _job: job,
            input,
            out,
            publication: Value::Null,
            sends: 0,
            publish_fault: false,
            partial_input: false,
            partial_activation: false,
            reload_waiting: false,
            adapter: addon.is_some(),
            carrier: None,
            control: Default::default(),
        };
        peer.read().unwrap();
        peer
    }
    fn read(&mut self) -> std::result::Result<(), String> {
        let mut b = Vec::new();
        Read::by_ref(&mut self.out)
            .take(4 << 20)
            .read_until(b'\n', &mut b)
            .map_err(|e| e.to_string())?;
        if b.last() != Some(&b'\n') {
            return Err("fixture.incomplete_or_excessive_frame".into());
        }
        self.publication = serde_json::from_slice(&b).map_err(|e| e.to_string())?;
        Ok(())
    }
    fn send(&mut self, line: &str) -> std::result::Result<(), String> {
        self.input
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        self.input.write_all(b"\n").map_err(|e| e.to_string())?;
        self.input.flush().map_err(|e| e.to_string())?;
        self.sends += 1;
        self.read()
    }
    fn record(&self, path: &[&str]) -> Option<Record> {
        let mut v = &self.publication;
        for key in path {
            v = &v[*key]
        }
        v.as_str().filter(|s| !s.is_empty()).map(|s| {
            let bytes = hex::decode(s).unwrap();
            if let Some(carrier) = &self.carrier {
                let (address, observed) = carrier.borrow_mut().stage_wire(&bytes);
                let mut record = decode(&observed).unwrap();
                record.address = address;
                record
            } else {
                let process = lycheedev_windows::Process::open(std::process::id()).unwrap();
                let mut observed = vec![0; bytes.len()];
                process
                    .read_private(bytes.as_ptr() as u64, &mut observed)
                    .unwrap();
                assert_eq!(observed, bytes);
                decode(&observed).unwrap()
            }
        })
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct FilesChannel {
    peer: Rc<RefCell<Peer>>,
    managed: Managed,
    identity: ManagedIdentity,
    window: lycheedev_windows::WindowIdentity,
    session: Option<Session>,
    replacement: lycheedev_live::replacement::Tracker,
}
// Fixture-only evidence, restricted to the exact chord's four-or-fewer allowed keys.
fn chord_admission(
    stage: &str,
    window: &lycheedev_windows::WindowIdentity,
    keys: &[u16],
    queued: Option<u32>,
) {
    let states=keys.iter().map(|key|json!({"vk":key,"highBitHeld":unsafe{windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(*key as i32)}<0})).collect::<Vec<_>>();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".tmp/live-static/native-chord-admission.jsonl");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(file,"{}",json!({"scope":"fixture exact allowed chord only; no global keyboard collection","stage":stage,"window":window,"keys":states,"queued":queued,"time":chrono::Utc::now().to_rfc3339()})).unwrap();
}
fn wait_chord_release(
    control: &lycheedev_core::control::OperationControl,
    keys: &[u16],
) -> std::result::Result<(), String> {
    let deadline = control
        .deadline()
        .min(std::time::Instant::now() + std::time::Duration::from_secs(30));
    loop {
        control.check().map_err(|e| e.to_string())?;
        if keys.iter().all(|key|unsafe{windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(*key as i32)}>=0){return Ok(());}
        if std::time::Instant::now() >= deadline {
            return Err("fixture.allowed_chord_release_deadline".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
impl Channel for FilesChannel {
    fn reserve_request(
        &mut self,
        session: &Session,
        sample: &Snapshot,
    ) -> std::result::Result<(), String> {
        if self.peer.borrow().adapter {
            lycheedev_live::routes::reserve_request(
                &self.identity.canonical_path,
                self.identity.generation,
                session,
                sample,
            )?;
        }
        Ok(())
    }
    fn settle_request(
        &mut self,
        session: &Session,
        sample: &Snapshot,
    ) -> std::result::Result<(), String> {
        if self.peer.borrow().adapter {
            lycheedev_live::routes::settle_request(
                &self.identity.canonical_path,
                self.identity.generation,
                session,
                sample,
            )?;
        }
        Ok(())
    }
    fn claim_route(&mut self, session: &Session) -> std::result::Result<u16, String> {
        self.session = Some(session.clone());
        if self.peer.borrow().adapter {
            lycheedev_live::routes::claim(
                &self.identity.canonical_path,
                self.identity.generation,
                session,
            )
        } else {
            Ok(0)
        }
    }
    fn reload(&mut self, _: &Session, _: &Snapshot) -> std::result::Result<ActionReceipt, String> {
        self.verify()?;
        let receipt = lycheedev_windows::input::fixed_reload(
            &self.window,
            |_| self.window.verify(),
            |_| Ok(()),
        )
        .map_err(|e| e.reason)?;
        self.peer.borrow_mut().send("RELOAD")?;
        self.peer.borrow_mut().reload_waiting = true;
        Ok(ActionReceipt {
            queued: receipt.messages_queued,
            complete: receipt.submission_complete && receipt.release_complete,
            facts: json!({"fixedReload":true}),
        })
    }
    fn replacement_samples(
        &mut self,
        session: &Session,
        current: &Snapshot,
    ) -> std::result::Result<Option<lycheedev_live::replacement::Proof>, String> {
        self.replacement.observe(session, current)
    }
    fn retire_route(
        &mut self,
        session: &Session,
        proof: &lycheedev_live::replacement::Proof,
    ) -> std::result::Result<(), String> {
        lycheedev_live::routes::retire_qualified(
            &self.identity.canonical_path,
            self.identity.generation,
            session,
            proof,
        )
    }
    fn wait_event(&mut self, _: std::time::Duration) -> std::result::Result<bool, String> {
        if !self.peer.borrow().reload_waiting {
            return Ok(false);
        }
        let mut peer = self.peer.borrow_mut();
        peer.send("SAMPLE")?;
        peer.sends -= 1;
        Ok(true)
    }
    fn activation_only(&self) -> bool {
        self.peer.borrow().record(&["identity"]).is_some_and(|r| {
            serde_json::from_slice::<Value>(&r.payload).unwrap()["activationOnly"] == true
        })
    }
    fn activate(&mut self, _: &Snapshot) -> std::result::Result<ActionReceipt, String> {
        self.verify()?;
        if self.peer.borrow().partial_activation {
            self.peer.borrow_mut().partial_activation = false;
            let mut checks = 0;
            let failure =
                lycheedev_windows::input::chord(&self.window, &[0x11, 0x12, 0x7b], || {
                    checks += 1;
                    if checks == 5 {
                        Err("fixture.partial_activation".into())
                    } else {
                        Ok(())
                    }
                })
                .unwrap_err();
            self.peer.borrow_mut().sends += 1;
            return Ok(ActionReceipt {
                queued: failure.receipt.messages_queued,
                complete: false,
                facts: json!({"activationInput":true,"businessInput":false}),
            });
        }
        let receipt = lycheedev_windows::input::chord(&self.window, &[0x11, 0x12, 0x7b], || {
            self.window.verify()
        })
        .map_err(|e| e.reason)?;
        self.peer.borrow_mut().send("ACTIVATE")?;
        assert_eq!(self.peer.borrow().publication["loads"], 0);
        Ok(ActionReceipt {
            queued: receipt.messages_queued,
            complete: receipt.submission_complete && receipt.release_complete,
            facts: json!({"activationInput":true,"businessInput":false}),
        })
    }
    fn verify(&mut self) -> std::result::Result<(), String> {
        self.managed
            .verify_clean(
                &self.identity.canonical_path,
                "3.1.2",
                "memory-slot-v4",
                500,
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn snapshot(&mut self) -> std::result::Result<Snapshot, String> {
        let peer = self.peer.borrow();
        let identity = peer
            .record(&["input"])
            .or_else(|| peer.record(&["identity"]))
            .ok_or("fixture.identity_missing")?;
        let v: Value = serde_json::from_slice(&identity.payload).map_err(|e| e.to_string())?;
        Ok(Snapshot {
            runtime: identity.runtime,
            guid: v["guid"].as_str().unwrap().into(),
            build: v["build"].as_str().unwrap().into(),
            owner: v["owner"].as_str().unwrap().into(),
            fence: v["fence"].as_u64().unwrap(),
            epoch: v["sessionEpoch"].as_str().unwrap().into(),
            next_slot: v["nextSlot"].as_u64().unwrap_or(0) as u16,
            challenge: v["challenge"].as_str().unwrap().into(),
            game_millis: v["sampleMillis"].as_u64().unwrap_or(1000),
            bindings: json!({"wake":"ALT-CTRL-F12","submit":"ALT-CTRL-SHIFT-F12","close":"ALT-CTRL-["}),
            input_blocked: v["inputBlocked"].as_bool().unwrap_or(false),
            sample: identity.sequence,
            address: if self.peer.borrow().carrier.is_some() {
                identity.address
            } else {
                0x1000
            },
        })
    }
    fn record(&mut self, s: &Selector) -> std::result::Result<Option<Record>, String> {
        let p = self.peer.borrow();
        Ok(match s {
            Selector::Identity => p.record(&["identity"]),
            Selector::Receipt { nonce } => p.record(&["receipts", nonce]),
            Selector::Body { head } => {
                let body = p.record(&["bodies", &head.ticket]);
                if let Some(body) = &body {
                    let v: Value = serde_json::from_slice(&head.payload).unwrap();
                    if body.nonce != head.nonce
                        || body.runtime != head.runtime
                        || body.ticket != head.ticket
                        || v["reportBytes"].as_u64() != Some(body.payload.len() as u64)
                        || v["reportChecksum"].as_u64() != Some(body.checksum as u64)
                    {
                        return Err("fixture.body_head_mismatch".into());
                    }
                }
                body
            }
            Selector::Input => None,
        })
    }
    fn publish(&mut self, slot: u16, b: &[u8]) -> std::result::Result<(), String> {
        if self.peer.borrow().adapter {
            lycheedev_live::routes::verify(
                &self.identity.canonical_path,
                self.identity.generation,
                self.session.as_ref().ok_or("fixture.route_unclaimed")?,
                slot,
            )?
        }
        if self.peer.borrow().publish_fault {
            self.peer.borrow_mut().publish_fault = false;
            let parent = self.identity.canonical_path.parent().unwrap();
            let path = parent.join(".lycheedev-slots.json");
            let mut pool: Value = storage::read_json(&path).map_err(|e| e.to_string())?;
            pool["modules"][slot as usize - 1]["pendingHash"] = json!(storage::sha256(b));
            storage::atomic_json(&path, &pool).map_err(|e| e.to_string())?;
            return Err("fixture.publication_interrupt".into());
        }
        self.managed
            .publish_slot(&self.identity, slot, None, b)
            .map_err(|e| e.to_string())
    }
    fn send(
        &mut self,
        action: &Action,
        qualified: &Snapshot,
        slot: u16,
        digest: &str,
    ) -> std::result::Result<ActionReceipt, String> {
        self.verify()?;
        let now = self.snapshot()?;
        if now.runtime != qualified.runtime
            || now.guid != qualified.guid
            || now.sample != qualified.sample
            || now.address != qualified.address
        {
            return Err("fixture.qualified_sample_changed".into());
        }
        let parent = self.identity.canonical_path.parent().unwrap();
        let pool: Value =
            storage::read_json(&parent.join(".lycheedev-slots.json")).map_err(|e| e.to_string())?;
        if pool["modules"][slot as usize - 1]["pendingHash"].is_string() {
            return Err("fixture.publication_pending".into());
        }
        let path = lycheedev_delivery::slots::directory(parent, slot).join("Payload.lua");
        if storage::sha256(&fs::read(&path).map_err(|e| e.to_string())?) != digest {
            return Err("fixture.exact_publication_changed".into());
        }
        let kind = match action {
            Action::Open => "OPEN",
            Action::InputExecute => "EXECUTE",
            Action::InputSettle => "SETTLE",
            Action::Close => "CLOSE",
            _ => return Err("fixture.input_action".into()),
        };
        if *action == Action::InputExecute && self.peer.borrow().partial_input {
            self.peer.borrow_mut().partial_input = false;
            let mut n = 0;
            let failure =
                lycheedev_windows::input::chord(&self.window, &[0x11, 0x12, 0x7b], || {
                    n += 1;
                    if n == 5 {
                        Err("fixture.partial_guard".into())
                    } else {
                        Ok(())
                    }
                })
                .unwrap_err();
            assert_eq!(failure.receipt.messages_queued, 2);
            return Ok(ActionReceipt {
                queued: 2,
                complete: false,
                facts: json!({"inputError":"fixture.partial_guard"}),
            });
        }
        let keys = match action {
            Action::InputSettle => vec![0x11, 0x12, 0x10, 0x7b],
            Action::Close => vec![0x11, 0x12, 0xdb],
            _ => vec![0x11, 0x12, 0x7b],
        };
        chord_admission(&format!("{action:?}.before"), &self.window, &keys, None);
        let control = self.peer.borrow().control.clone();
        wait_chord_release(&control, &keys)?;
        self.verify()?;
        let fresh = self.snapshot()?;
        if fresh.runtime != qualified.runtime
            || fresh.guid != qualified.guid
            || fresh.sample != qualified.sample
            || fresh.address != qualified.address
        {
            return Err("fixture.qualified_sample_changed_after_admission".into());
        }
        let posted = lycheedev_windows::input::chord(&self.window, &keys, || {
            self.window.verify()?;
            if storage::sha256(&fs::read(&path).map_err(|e| e.to_string())?) != digest {
                return Err("fixture.exact_publication_changed".into());
            }
            Ok(())
        })
        .map_err(|e| {
            chord_admission(
                &format!("{action:?}.failed:{}", e.reason),
                &self.window,
                &keys,
                Some(e.receipt.messages_queued),
            );
            e.reason
        })?;
        chord_admission(
            &format!("{action:?}.after"),
            &self.window,
            &keys,
            Some(posted.messages_queued),
        );
        self.peer
            .borrow_mut()
            .send(&format!("{kind}|{slot}|{}", path.display()))?;
        Ok(ActionReceipt {
            queued: posted.messages_queued,
            complete: posted.submission_complete && posted.release_complete,
            facts: Value::Null,
        })
    }
}
struct FixtureServices {
    target: Target,
    peer: Rc<RefCell<Peer>>,
    managed: Managed,
}
impl Services for FixtureServices {
    fn targets(&self, _: &Invocation) -> Result<Vec<Target>> {
        Ok(vec![self.target.clone()])
    }
    fn managed(&self) -> &dyn ManagedInstallation {
        &self.managed
    }
    fn open(&self, _: &Target, _: Option<&Session>) -> Result<Box<dyn Transport>> {
        let identity =
            self.managed
                .verify_clean(&self.target.installation, "3.1.2", "memory-slot-v4", 500)?;
        Ok(Box::new(
            NativeTransport::new(
                FilesChannel {
                    peer: self.peer.clone(),
                    managed: self.managed.clone(),
                    identity,
                    window: self.target.window.clone(),
                    session: None,
                    replacement: Default::default(),
                },
                0,
            )
            .unwrap(),
        ))
    }
}
fn inv(home: &Path, command: &str, argument: Option<&str>, flags: &[(&str, &str)]) -> Invocation {
    Invocation {
        command: command.into(),
        positional: argument.map(str::to_owned),
        flags: flags
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
        home: home.into(),
        cwd: home.into(),
        control: Default::default(),
    }
}
fn call(
    home: &Path,
    command: &str,
    argument: Option<&str>,
    flags: &[(&str, &str)],
    s: &FixtureServices,
) -> Outcome {
    let invocation = inv(home, command, argument, flags);
    s.peer.borrow_mut().control = invocation.control.clone();
    host::dispatch_with(&invocation, s)
        .unwrap_or_else(|error| panic!("{command} {argument:?}: {error:?}"))
}
fn setup_installation() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let root = std::env::temp_dir().join(format!(
        "lycheedev-native-e2e-{}",
        lycheedev_windows::random::token().unwrap()
    ));
    let addon = root.join("AddOns/Lychee Dev");
    fs::create_dir_all(&addon).unwrap();
    let home = root.join("home");
    workspace::initialize(&home).unwrap();
    let mut resources = Vec::new();
    let candidate_manifest = fs::read_to_string(repo.join("addon/Lychee Dev Rust.toc")).unwrap();
    let mut paths = candidate_manifest
        .lines()
        .filter(|line| line.ends_with(".lua"))
        .map(|line| line.replace('\\', "/"))
        .collect::<Vec<_>>();
    paths.push("Lychee Dev.toc".into());
    for p in paths {
        let b = if p == "Lychee Dev.toc" {
            candidate_manifest.as_bytes().to_vec()
        } else {
            fs::read(repo.join("addon").join(&p)).unwrap()
        };
        fs::create_dir_all(addon.join(&p).parent().unwrap()).unwrap();
        fs::write(addon.join(&p), &b).unwrap();
        resources.push(lycheedev_delivery::installation::Resource {
            path: format!("addon/{p}"),
            bytes: b.len() as u64,
            sha256: storage::sha256(&b),
        });
    }
    let receipt = lycheedev_delivery::installation::Receipt {
        schema: "lycheedev.installation.v1".into(),
        component: "addon".into(),
        version: "3.1.2".into(),
        commit: "9d3d3598b8bac7efabf0b937df30508075eaa841".into(),
        resources,
    };
    storage::atomic_json(
        &addon.join(lycheedev_delivery::installation::MARKER),
        &receipt,
    )
    .unwrap();
    lycheedev_delivery::slots::install(addon.parent().unwrap(), "3.1.2").unwrap();
    (repo, root, home, addon)
}
#[test]
fn formal_native_files_lua_result_durable_settle_closed() {
    let (repo, root, home, addon) = setup_installation();
    let peer = Rc::new(RefCell::new(Peer::start(&repo)));
    let process = lycheedev_windows::Process::open(std::process::id()).unwrap();
    let own_window = OwnWindow::create();
    let services = FixtureServices {
        target: Target {
            window: lycheedev_windows::WindowIdentity {
                process: process.identity().clone(),
                hwnd: own_window.0.0 as usize,
            },
            installation: addon,
            actor: "actor".into(),
            build: "fixture".into(),
            module_sha256: "f".repeat(64),
            runtime: "1".repeat(32),
        },
        peer: peer.clone(),
        managed: Managed,
    };
    let connected = call(&home, "live connect", None, &[], &services);
    assert!(connected.complete);
    let connection = connected.data["connectionId"].as_str().unwrap().to_owned();
    let code = root.join("probe.lua");
    fs::write(
        &code,
        "return {answer=7,label='荔枝',body=string.rep('z',90000)}",
    )
    .unwrap();
    let output = call(
        &home,
        "live execute",
        None,
        &[
            ("--session", &connection),
            ("--file", code.to_str().unwrap()),
            ("--request", "frozen-request"),
            ("--budget-seconds", "30"),
        ],
        &services,
    );
    assert!(output.complete, "{output:?}");
    assert_eq!(output.data["report"]["state"], "verified");
    assert_eq!(output.data["cleanup"], "complete");
    assert_eq!(output.data["report"]["result"]["result"]["answer"], 7);
    assert_eq!(output.data["report"]["result"]["result"]["label"], "荔枝");
    assert_eq!(
        output.data["report"]["result"]["result"]["body"]
            .as_str()
            .unwrap()
            .len(),
        90000
    );
    let mut sent = peer.borrow().sends;
    let retry = call(
        &home,
        "live execute",
        None,
        &[
            ("--session", &connection),
            ("--file", code.to_str().unwrap()),
            ("--request", "frozen-request"),
            ("--budget-seconds", "30"),
        ],
        &services,
    );
    assert_eq!(retry.data, output.data);
    assert_eq!(peer.borrow().sends, sent);
    let status = call(
        &home,
        "live status",
        Some(output.data["operationId"].as_str().unwrap()),
        &[],
        &services,
    );
    assert_eq!(status.data["report"]["state"], "verified");
    call(
        &home,
        "live probe put",
        None,
        &[
            ("--name", "staged-probe"),
            ("--file", code.to_str().unwrap()),
        ],
        &services,
    );
    let loaded = call(
        &home,
        "live probe load",
        None,
        &[
            ("--session", &connection),
            ("--probe", "staged-probe"),
            ("--request", "staged-request"),
            ("--budget-seconds", "30"),
        ],
        &services,
    );
    let loaded_id = loaded.data["operationId"].as_str().unwrap();
    assert_eq!(loaded.data["phase"], "Prepared");
    assert_eq!(
        peer.borrow().sends,
        sent,
        "host load must not publish or execute a slot"
    );
    let denied_ack =
        host::dispatch_with(&inv(&home, "live ack", Some(loaded_id), &[]), &services).unwrap_err();
    assert_eq!(denied_ack.code, "live.verified_report_required");
    assert_eq!(peer.borrow().sends, sent);
    // Removing the mutable name cannot change already-frozen code or its budget.
    call(
        &home,
        "live probe remove",
        Some("staged-probe"),
        &[],
        &services,
    );
    let reported = call(&home, "live run", Some(loaded_id), &[], &services);
    assert!(!reported.complete);
    assert_eq!(reported.data["phase"], "ResultDurable");
    assert_eq!(reported.data["report"]["state"], "verified");
    assert_eq!(reported.data["cleanup"], "pending");
    assert_eq!(
        reported.data["report"]["result"]["result"]["body"]
            .as_str()
            .unwrap()
            .len(),
        90000
    );
    assert_eq!(
        peer.borrow().sends,
        sent + 1,
        "run must not spend the SETTLE slot"
    );
    assert_eq!(
        call(&home, "live run", Some(loaded_id), &[], &services).data,
        reported.data
    );
    assert_eq!(peer.borrow().sends, sent + 1);
    let acknowledged = call(&home, "live ack", Some(loaded_id), &[], &services);
    assert!(acknowledged.complete);
    assert_eq!(acknowledged.data["cleanup"], "complete");
    assert_eq!(peer.borrow().sends, sent + 2);
    assert_eq!(
        call(&home, "live finish", Some(loaded_id), &[], &services).data,
        acknowledged.data
    );
    assert_eq!(peer.borrow().sends, sent + 2);
    sent = peer.borrow().sends;
    let closed = call(&home, "live disconnect", Some(&connection), &[], &services);
    assert!(closed.complete);
    assert_eq!(closed.data["phase"], "Closed");
    let repeat = call(&home, "live disconnect", Some(&connection), &[], &services);
    assert!(repeat.complete);
    assert_eq!(peer.borrow().sends, sent + 1);
    // Actual candidate singleton starts disabled, exposes one passive record,
    // and first Wake only Enables. Business needs the subsequent exact OPEN.
    let activation_peer = Rc::new(RefCell::new(Peer::start_kind(
        &repo,
        Some(&services.target.installation),
    )));
    let mut activation_target = services.target.clone();
    activation_target.runtime = activation_peer
        .borrow()
        .record(&["identity"])
        .unwrap()
        .runtime;
    let activation_services = FixtureServices {
        target: activation_target,
        peer: activation_peer.clone(),
        managed: Managed,
    };
    let activated = call(&home, "live connect", None, &[], &activation_services);
    assert!(activated.complete);
    assert_eq!(activation_peer.borrow().sends, 2);
    assert_eq!(activation_peer.borrow().publication["loads"], 1);
    let activated_connection = activated.data["connectionId"].as_str().unwrap();
    let done = call(
        &home,
        "live execute",
        None,
        &[
            ("--session", activated_connection),
            ("--file", code.to_str().unwrap()),
            ("--request", "activated-probe"),
            ("--budget-seconds", "30"),
        ],
        &activation_services,
    );
    assert!(done.complete);
    assert_eq!(done.data["report"]["state"], "verified");
    assert_eq!(activation_peer.borrow().publication["loads"], 3);
    assert!(
        call(
            &home,
            "live disconnect",
            Some(activated_connection),
            &[],
            &activation_services
        )
        .complete
    );
    let before_reload = activation_peer.borrow().sends;
    let reloaded = call(
        &home,
        "live reload",
        None,
        &[
            ("--session", activated_connection),
            ("--request", "closed-qualified-reload"),
        ],
        &activation_services,
    );
    assert!(reloaded.complete);
    assert_eq!(reloaded.data["reload"]["complete"], true);
    assert_eq!(activation_peer.borrow().sends, before_reload + 1);
    let repeated = call(
        &home,
        "live reload",
        None,
        &[
            ("--session", activated_connection),
            ("--request", "closed-qualified-reload"),
        ],
        &activation_services,
    );
    assert!(repeated.complete);
    assert_eq!(activation_peer.borrow().sends, before_reload + 1);
    let mut reconnected_target = activation_services.target.clone();
    reconnected_target.runtime = activation_peer
        .borrow()
        .record(&["identity"])
        .unwrap()
        .runtime;
    let reconnected_services = FixtureServices {
        target: reconnected_target,
        peer: activation_peer.clone(),
        managed: Managed,
    };
    let reconnected = call(&home, "live connect", None, &[], &reconnected_services);
    assert!(reconnected.complete);
    let idle_reloaded = call(
        &home,
        "live reload",
        None,
        &[
            (
                "--session",
                reconnected.data["connectionId"].as_str().unwrap(),
            ),
            ("--request", "idle-qualified-reload"),
        ],
        &reconnected_services,
    );
    assert!(idle_reloaded.complete);
    assert!(!idle_reloaded.data["executionUnknown"].as_bool().unwrap());
    let partial_peer = Rc::new(RefCell::new(Peer::start_kind(
        &repo,
        Some(&services.target.installation),
    )));
    partial_peer.borrow_mut().partial_activation = true;
    let mut partial_target = services.target.clone();
    partial_target.runtime = partial_peer.borrow().record(&["identity"]).unwrap().runtime;
    let partial_services = FixtureServices {
        target: partial_target,
        peer: partial_peer.clone(),
        managed: Managed,
    };
    let pending = call(&home, "live connect", None, &[], &partial_services);
    assert!(!pending.complete);
    let pending_id = pending.data["connectionId"].as_str().unwrap();
    let before = partial_peer.borrow().sends;
    for _ in 0..2 {
        assert!(
            !call(
                &home,
                "live resume",
                Some(pending_id),
                &[],
                &partial_services
            )
            .complete
        )
    }
    assert_eq!(partial_peer.borrow().sends, before);
    assert_eq!(partial_peer.borrow().publication["loads"], 0);
    // A later external explicit Enable can prove the original uncertain input's
    // effect. Resume then OPENs; it still does not resend activation or code.
    partial_peer.borrow_mut().send("ACTIVATE").unwrap();
    let resumed = call(
        &home,
        "live resume",
        Some(pending_id),
        &[],
        &partial_services,
    );
    assert!(resumed.complete);
    let syntax = root.join("syntax-error.lua");
    fs::write(&syntax, "local = broken").unwrap();
    let failed_report = call(
        &home,
        "live execute",
        None,
        &[
            ("--session", pending_id),
            ("--file", syntax.to_str().unwrap()),
            ("--request", "compile-error-report"),
            ("--budget-seconds", "30"),
        ],
        &partial_services,
    );
    assert!(failed_report.complete);
    assert_eq!(failed_report.data["report"]["state"], "verified");
    assert_eq!(failed_report.data["report"]["result"]["ok"], false);
    assert_eq!(
        failed_report.data["report"]["result"]["error"]["kind"],
        "compile_error"
    );
    partial_peer.borrow_mut().partial_input = true;
    let unfinished = call(
        &home,
        "live execute",
        None,
        &[
            ("--session", pending_id),
            ("--file", code.to_str().unwrap()),
            ("--request", "force-reload-unknown"),
            ("--budget-seconds", "30"),
        ],
        &partial_services,
    );
    assert!(!unfinished.complete);
    let loads_before = partial_peer.borrow().publication["loads"].clone();
    let forced = call(
        &home,
        "live reload",
        None,
        &[
            ("--session", pending_id),
            ("--request", "replace-unknown-runtime"),
        ],
        &partial_services,
    );
    assert!(forced.complete);
    assert_eq!(forced.data["executionUnknown"], true);
    assert_eq!(forced.data["report"]["state"], "unavailable");
    assert_eq!(forced.data["reload"]["original_action"], "InputExecute");
    assert_eq!(forced.data["reload"]["original_messages"], 2);
    assert_eq!(partial_peer.borrow().publication["loads"], loads_before);
    let retained = call(
        &home,
        "live status",
        Some(pending_id),
        &[],
        &partial_services,
    );
    assert!(!retained.complete);
    assert_eq!(retained.data["executionUnknown"], true);
    // A fresh offline peer reuses only the isolated installation; retained claims remain exact.
    let next_peer = Rc::new(RefCell::new(Peer::start(&repo)));
    let mut next_target = services.target.clone();
    next_target.runtime = "1".repeat(32);
    let next_services = FixtureServices {
        target: next_target,
        peer: next_peer.clone(),
        managed: Managed,
    };
    let next_connection =
        call(&home, "live connect", None, &[], &next_services).data["connectionId"]
            .as_str()
            .unwrap()
            .to_owned();
    next_peer.borrow_mut().publish_fault = true;
    let request = inv(
        &home,
        "live execute",
        None,
        &[
            ("--session", &next_connection),
            ("--file", code.to_str().unwrap()),
            ("--request", "publish-interrupt"),
            ("--budget-seconds", "30"),
        ],
    );
    let error = host::dispatch_with(&request, &next_services).unwrap_err();
    assert!(error.message.contains("publication_interrupt"));
    let before = next_peer.borrow().sends;
    let resumed = call(
        &home,
        "live resume",
        Some(&next_connection),
        &[],
        &next_services,
    );
    assert!(resumed.complete);
    assert_eq!(resumed.data["report"]["state"], "verified");
    assert_eq!(next_peer.borrow().sends, before + 2);
    next_peer.borrow_mut().partial_input = true;
    let partial = call(
        &home,
        "live execute",
        None,
        &[
            ("--session", &next_connection),
            ("--file", code.to_str().unwrap()),
            ("--request", "partial-input"),
            ("--budget-seconds", "30"),
        ],
        &next_services,
    );
    assert!(!partial.complete);
    let sent = next_peer.borrow().sends;
    for _ in 0..2 {
        let pending = call(
            &home,
            "live resume",
            Some(&next_connection),
            &[],
            &next_services,
        );
        assert!(!pending.complete)
    }
    assert_eq!(
        next_peer.borrow().sends,
        sent,
        "unknown partial input was replayed"
    );
    let another_home = root.join("other-home");
    workspace::initialize(&another_home).unwrap();
    let occupied = host::dispatch_with(
        &inv(&another_home, "live connect", None, &[]),
        &next_services,
    )
    .unwrap_err();
    assert!(occupied.message.contains("original_lifecycle_required"));
    assert_eq!(next_peer.borrow().sends, sent);
    next_peer.borrow_mut().send("ACTOR").unwrap();
    let sent = next_peer.borrow().sends;
    let changed = host::dispatch_with(
        &inv(&home, "live resume", Some(&next_connection), &[]),
        &next_services,
    )
    .unwrap_err();
    assert!(
        changed.message.contains("target_changed") || changed.message.contains("actor_changed")
    );
    assert_eq!(next_peer.borrow().sends, sent);
    drop(next_services);
    drop(next_peer);
    drop(services);
    drop(peer);
    own_window.drain();
}
/// Actual distinct OS targets; memory is a test-only wire carrier and optics remain a fixture.
#[test]
fn formal_bugs_preserves_provider_coverage_and_uses_durable_settle() {
    let (repo, _root, home, addon) = setup_installation();
    let owned = Rc::new(RefCell::new(owned_target::Owned::spawn(
        "owned_live_target_entry",
    )));
    let peer = Rc::new(RefCell::new(Peer::start_kind(&repo, Some(&addon))));
    peer.borrow_mut().carrier = Some(owned.clone());
    let (window, module_sha256) = {
        let target = owned.borrow();
        (target.target.clone(), target.hash.clone())
    };
    let runtime = peer.borrow().record(&["identity"]).unwrap().runtime;
    let services = FixtureServices {
        target: Target {
            window,
            installation: addon,
            actor: "actor".into(),
            build: "fixture".into(),
            module_sha256,
            runtime,
        },
        peer: peer.clone(),
        managed: Managed,
    };
    let connected = call(&home, "live connect", None, &[], &services);
    assert!(connected.complete);
    let con = connected.data["connectionId"].as_str().unwrap();
    peer.borrow_mut().send("BUGS_PRESENT").unwrap();
    let flags = [
        ("--session", con),
        ("--request", "provider-snapshot"),
        ("--count", "2"),
    ];
    let observed = call(&home, "live bugs", None, &flags, &services);
    assert!(observed.complete);
    assert_eq!(observed.data["cleanup"], "complete");
    assert_eq!(observed.data["report"]["state"], "verified");
    let body = &observed.data["report"]["result"]["result"];
    assert_eq!(body["schema"], "lycheedev.bugs.v1");
    assert_eq!(body["status"], "completed");
    assert_eq!(
        body["complete"], false,
        "missing provider fields must stay visible"
    );
    assert_eq!(body["snapshot"]["availableCount"], 3);
    assert_eq!(body["snapshot"]["returnedCount"], 2);
    assert_eq!(body["snapshot"]["scope"], "provider_storage");
    assert_eq!(body["snapshot"]["errors"][0]["message"], "newest");
    assert_eq!(
        body["snapshot"]["errors"][0]["stack"]
            .as_str()
            .unwrap()
            .len(),
        90000
    );
    assert_eq!(body["snapshot"]["errors"][1]["message"], "older 荔枝");
    let sends = peer.borrow().sends;
    let repeated = call(&home, "live bugs", None, &flags, &services);
    assert_eq!(repeated.data, observed.data);
    assert_eq!(peer.borrow().sends, sends);
    let conflict = host::dispatch_with(
        &inv(
            &home,
            "live bugs",
            None,
            &[
                ("--session", con),
                ("--request", "provider-snapshot"),
                ("--count", "3"),
            ],
        ),
        &services,
    )
    .unwrap_err();
    assert_eq!(conflict.code, "live.request_conflict");
    assert_eq!(peer.borrow().sends, sends);
    for (fixture, key, reason) in [
        ("BUGS_ABSENT", "provider-missing", "provider_unavailable"),
        ("BUGS_ERROR", "provider-failed", "provider_error"),
    ] {
        peer.borrow_mut().send(fixture).unwrap();
        let result = call(
            &home,
            "live bugs",
            None,
            &[("--session", con), ("--request", key)],
            &services,
        );
        assert!(result.complete);
        assert_eq!(result.data["cleanup"], "complete");
        assert_eq!(result.data["report"]["state"], "verified");
        let body = &result.data["report"]["result"]["result"];
        assert_eq!(body["status"], "unavailable");
        assert_eq!(body["error"], reason);
        assert_eq!(body["complete"], false);
    }
    let sends = peer.borrow().sends;
    assert_eq!(
        call(&home, "live bugs", None, &flags, &services).data,
        observed.data
    );
    assert_eq!(
        peer.borrow().sends,
        sends,
        "historical snapshot is read-only after later bugs tasks"
    );
    assert!(call(&home, "live disconnect", Some(con), &[], &services).complete);
}

#[test]
fn formal_host_two_actual_targets_share_installation_and_isolate_lua_loaded_routes() {
    let (repo, root, home, addon) = setup_installation();
    let a = Rc::new(RefCell::new(owned_target::Owned::spawn(
        "owned_live_target_entry",
    )));
    let b = Rc::new(RefCell::new(owned_target::Owned::spawn(
        "owned_live_target_entry",
    )));
    assert_ne!(a.borrow().target.process.pid, b.borrow().target.process.pid);
    assert_eq!(a.borrow().hash, b.borrow().hash);
    assert_eq!(
        a.borrow().bytes(),
        format!("owned-instance:{}", a.borrow().marker).as_bytes()
    );
    assert_eq!(
        b.borrow().bytes(),
        format!("owned-instance:{}", b.borrow().marker).as_bytes()
    );
    let peer_a = Rc::new(RefCell::new(Peer::start_kind(&repo, Some(&addon))));
    peer_a.borrow_mut().carrier = Some(a.clone());
    let peer_b = Rc::new(RefCell::new(Peer::start_kind(&repo, Some(&addon))));
    peer_b.borrow_mut().carrier = Some(b.clone());
    let service = |owned: &Rc<RefCell<owned_target::Owned>>, peer: &Rc<RefCell<Peer>>| {
        let (window, hash) = {
            let owner = owned.borrow();
            (owner.target.clone(), owner.hash.clone())
        };
        FixtureServices {
            target: Target {
                window,
                installation: addon.clone(),
                actor: "actor".into(),
                build: "fixture".into(),
                module_sha256: hash,
                runtime: peer.borrow().record(&["identity"]).unwrap().runtime,
            },
            peer: peer.clone(),
            managed: Managed,
        }
    };
    let sa = service(&a, &peer_a);
    let sb = service(&b, &peer_b);
    let ca = call(&home, "live connect", None, &[], &sa);
    let cb = call(&home, "live connect", None, &[], &sb);
    assert!(ca.complete && cb.complete);
    let ida = ca.data["connectionId"].as_str().unwrap();
    let idb = cb.data["connectionId"].as_str().unwrap();
    assert_eq!(peer_a.borrow().publication["loads"], 1);
    assert_eq!(
        peer_b.borrow().publication["loads"],
        2,
        "B physically loads foreign bind1 then own bind3"
    );
    let code = root.join("actual-target.lua");
    std::fs::write(
        &code,
        "return {actualTarget=true,body=string.rep('x',90000)}",
    )
    .unwrap();
    let b_messages = b.borrow_mut().state();
    let b_loads = peer_b.borrow().publication["loads"].clone();
    let done = call(
        &home,
        "live execute",
        None,
        &[
            ("--session", ida),
            ("--file", code.to_str().unwrap()),
            ("--request", "actual-a-task"),
            ("--budget-seconds", "30"),
        ],
        &sa,
    );
    assert!(done.complete);
    assert_eq!(
        done.data["report"]["result"]["result"]["body"]
            .as_str()
            .unwrap()
            .len(),
        90000
    );
    assert_eq!(b.borrow_mut().state(), b_messages);
    assert_eq!(peer_b.borrow().publication["loads"], b_loads);
    let a_messages = a.borrow_mut().state();
    let a_loads = peer_a.borrow().publication["loads"].clone();
    let done_b = call(
        &home,
        "live execute",
        None,
        &[
            ("--session", idb),
            ("--file", code.to_str().unwrap()),
            ("--request", "actual-b-task"),
            ("--budget-seconds", "30"),
        ],
        &sb,
    );
    assert!(done_b.complete);
    assert_eq!(a.borrow_mut().state(), a_messages);
    assert_eq!(peer_a.borrow().publication["loads"], a_loads);
    let book = addon
        .parent()
        .unwrap()
        .join(".lycheedev-native-routes.json");
    let before: Value = storage::read_json(&book).unwrap();
    assert_eq!(before["ledger"]["lanes"]["0"]["cursor"], 2);
    assert_eq!(before["ledger"]["lanes"]["1"]["cursor"], 2);
    let foreign = before["ledger"]["lanes"]["1"].clone();
    assert!(call(&home, "live disconnect", Some(ida), &[], &sa).complete);
    let reloaded = call(
        &home,
        "live reload",
        None,
        &[("--session", ida), ("--request", "actual-a-replacement")],
        &sa,
    );
    assert!(reloaded.complete);
    let after: Value = storage::read_json(&book).unwrap();
    assert_eq!(after["ledger"]["lanes"]["1"], foreign);
    assert_eq!(
        b.borrow_mut().state(),
        b_messages + 14,
        "B only its own EXECUTE+SETTLE chords"
    );
    let mut target = sa.target.clone();
    target.runtime = peer_a.borrow().record(&["identity"]).unwrap().runtime;
    let reconnected = FixtureServices {
        target,
        peer: peer_a.clone(),
        managed: Managed,
    };
    assert!(call(&home, "live connect", None, &[], &reconnected).complete);
    let a_session: Value =
        storage::read_json(&home.join("live/v2").join(format!("{ida}.json"))).unwrap();
    let b_session: Value =
        storage::read_json(&home.join("live/v2").join(format!("{idb}.json"))).unwrap();
    let proof = serde_json::json!({"scope":"formal Host/NativeTransport/actual managed 500 files/Lua NativeAdapter + Loaded model; actual OS target HWND/MEM_PRIVATE wire carriers; optics fixture; not actual Retail ABI/WGC",
        "aTarget":sa.target,"bTarget":sb.target,"aOutput":done.data,"bOutput":done_b.data,"before":before,"after":after,"foreignLaneUnchanged":after["ledger"]["lanes"]["1"]==foreign,"bPhysicalMessages":b.borrow_mut().state(),"aSessionEvidence":a_session,"bSessionEvidence":b_session});
    storage::atomic_json(
        &repo.join(".tmp/live-static/native-actual-dual-target-proof.json"),
        &proof,
    )
    .unwrap();
}
#[test]
fn formal_47_tasks_automatic_capacity_reload_keeps_actual_foreign_target_unchanged() {
    let (repo, root, home, addon) = setup_installation();
    let a = Rc::new(RefCell::new(owned_target::Owned::spawn(
        "owned_live_target_entry",
    )));
    let b = Rc::new(RefCell::new(owned_target::Owned::spawn(
        "owned_live_target_entry",
    )));
    let pa = Rc::new(RefCell::new(Peer::start_kind(&repo, Some(&addon))));
    pa.borrow_mut().carrier = Some(a.clone());
    let pb = Rc::new(RefCell::new(Peer::start_kind(&repo, Some(&addon))));
    pb.borrow_mut().carrier = Some(b.clone());
    let make = |target: &Rc<RefCell<owned_target::Owned>>, peer: &Rc<RefCell<Peer>>| {
        let (window, hash) = {
            let target = target.borrow();
            (target.target.clone(), target.hash.clone())
        };
        FixtureServices {
            target: Target {
                window,
                installation: addon.clone(),
                actor: "actor".into(),
                build: "fixture".into(),
                module_sha256: hash,
                runtime: peer.borrow().record(&["identity"]).unwrap().runtime,
            },
            peer: peer.clone(),
            managed: Managed,
        }
    };
    let sa = make(&a, &pa);
    let sb = make(&b, &pb);
    let ca = call(&home, "live connect", None, &[], &sa);
    let cb = call(&home, "live connect", None, &[], &sb);
    let con = ca.data["connectionId"].as_str().unwrap();
    let foreign_con = cb.data["connectionId"].as_str().unwrap();
    let original_runtime = pa.borrow().record(&["identity"]).unwrap().runtime;
    let book = addon
        .parent()
        .unwrap()
        .join(".lycheedev-native-routes.json");
    let before: Value = storage::read_json(&book).unwrap();
    let foreign = before["ledger"]["lanes"]["1"].clone();
    let b_messages = b.borrow_mut().state();
    let b_loads = pb.borrow().publication["loads"].clone();
    let code = root.join("capacity.lua");
    fs::write(
        &code,
        "_G.capacityRuns=(_G.capacityRuns or 0)+1;return{count=_G.capacityRuns}",
    )
    .unwrap();
    let mut operations = Vec::new();
    for n in 0..47 {
        println!("capacity actual task {n} starting");
        let key = format!("capacity-{n}");
        let out = call(
            &home,
            "live execute",
            None,
            &[
                ("--session", con),
                ("--file", code.to_str().unwrap()),
                ("--request", &key),
                ("--budget-seconds", "30"),
            ],
            &sa,
        );
        assert!(out.complete, "task {n}: {:?}", out.data);
        assert_eq!(out.data["report"]["state"], "verified");
        assert_eq!(out.data["cleanup"], "complete");
        assert_eq!(
            out.data["capacity"]["completedRotations"],
            if n < 28 { 0 } else { 1 }
        );
        let current: Value = storage::read_json(&book).unwrap();
        assert_eq!(
            current["ledger"]["lanes"]["1"], foreign,
            "foreign lane task {n}"
        );
        assert_eq!(b.borrow_mut().state(), b_messages);
        assert_eq!(pb.borrow().publication["loads"], b_loads);
        operations.push(out.data);
    }
    let new_runtime = pa.borrow().record(&["identity"]).unwrap().runtime;
    assert_ne!(original_runtime, new_runtime);
    let saved: Value =
        storage::read_json(&home.join("live/v2").join(format!("{con}.json"))).unwrap();
    assert_eq!(saved["sequence"], 19);
    assert_eq!(saved["capacity_history"].as_array().unwrap().len(), 1);
    assert_eq!(
        saved["capacity_history"][0]["from_runtime"],
        original_runtime
    );
    assert_eq!(saved["capacity_history"][0]["to_runtime"], new_runtime);
    assert!(
        saved["capacity_history"][0]["samples"][1].as_u64().unwrap()
            > saved["capacity_history"][0]["samples"][0].as_u64().unwrap()
    );
    let sends = pa.borrow().sends;
    let repeat = call(
        &home,
        "live execute",
        None,
        &[
            ("--session", con),
            ("--file", code.to_str().unwrap()),
            ("--request", "capacity-46"),
            ("--budget-seconds", "30"),
        ],
        &sa,
    );
    assert!(repeat.complete);
    assert_eq!(pa.borrow().sends, sends);
    let old = call(
        &home,
        "live status",
        Some(operations[0]["operationId"].as_str().unwrap()),
        &[],
        &sa,
    );
    assert_eq!(old.data, operations[0]);
    assert!(call(&home, "live status", Some(foreign_con), &[], &sb).complete);
    let after: Value = storage::read_json(&book).unwrap();
    storage::atomic_json(&repo.join(".tmp/live-static/native-actual-capacity47-proof.json"),&json!({"scope":"production automatic capacity handler + actual owned targets/readonly wirecarriers/managed 500files/Lua permanent Loaded model; optics fixture; no game/WGC ABI claim","aTarget":sa.target,"bTarget":sb.target,"before":before,"after":after,"session":saved,"operations":operations,"bMessages":b_messages,"bLoads":b_loads,"aTotalLoads":pa.borrow().publication["loads"]})).unwrap();
}
