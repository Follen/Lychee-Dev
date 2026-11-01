use super::*;
use crate::test_support::TestDir;
use lycheedev_windows::{JobGuard, guarded_spawn};
use serde_json::json;
use std::{
    cell::RefCell,
    process::{Child as StdChild, Command, Stdio},
    rc::Rc,
};
const ROLE: &str = "LYCHEEDEV_DD_CHANNEL_TEST_ROLE";
struct Controlled {
    child: Option<StdChild>,
    job: Option<JobGuard>,
    id: ProcessIdentity,
}
impl Child for Controlled {
    fn identity(&self) -> &ProcessIdentity {
        &self.id
    }
    fn verify(&self, executable: &Path) -> Result<()> {
        let peer = SameUserPeer::open(&self.id, executable).map_err(platform)?;
        finish_owned(
            peer.verify().map_err(platform),
            peer.close().map_err(platform),
        )
    }
    fn wait(&self, control: &OperationControl) -> Result<()> {
        let process = Process::open(self.id.pid).map_err(platform)?;
        let result = process
            .wait_exit_with_control(control.deadline(), || {
                control.check().map_err(|_| "fixture stopped".into())
            })
            .map_err(platform)
            .map(|_| ());
        finish_owned(result, process.close().map_err(platform))
    }
    fn retire(&mut self, deadline: Instant) -> Result<()> {
        self.job.as_ref().unwrap().terminate().map_err(platform)?;
        loop {
            if self.child.as_mut().unwrap().try_wait()?.is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(invalid());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn close(mut self) -> Result<()> {
        let process = guarded_spawn::close_child(self.child.take().unwrap()).map_err(platform);
        finish_owned(process, self.job.take().unwrap().close().map_err(platform))
    }
}
impl Drop for Controlled {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            let _ = job.terminate();
        }
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn execute(
    dir: &TestDir,
    mode: &str,
    duration: Duration,
) -> (Result<Choices>, Option<ProcessIdentity>) {
    let id = own().unwrap();
    let desktop = super::super::super::desktop::Desktop::synthetic(id.clone(), id);
    let child_id = Rc::new(RefCell::new(None));
    let recorded = child_id.clone();
    let control = OperationControl::uncancelable(Instant::now() + duration);
    let result = call_with(
        &dir.0,
        &desktop.channel_anchor(),
        &control,
        |home, parent, nonce, deadline| {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args(["--ignored", "--exact", "dd::channel::elevated::tests::fixture_process", "--nocapture"])
            .env(ROLE, json!({"home":home,"parent":parent,"nonce":nonce,"deadline":deadline,"mode":mode}).to_string())
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            let (child, job) = guarded_spawn::spawn_suspended_job_guarded_until(
                command,
                control.deadline(),
                control.deadline() + Duration::from_secs(5),
                || control.check().is_err(),
            )
            .map_err(|_| invalid())?;
            let id = guarded_spawn::child_identity(&child).map_err(platform)?;
            *recorded.borrow_mut() = Some(id.clone());
            Ok(Controlled {
                child: Some(child),
                job: Some(job),
                id,
            })
        },
    );
    let observed = child_id.borrow().clone();
    (result, observed)
}
#[test]
#[ignore = "one-shot channel fixture; parent tests supply its required process role and binding"]
fn fixture_process() {
    let value: serde_json::Value = serde_json::from_str(
        &std::env::var(ROLE).expect("channel fixture parent role is required"),
    )
    .unwrap();
    let home = PathBuf::from(value["home"].as_str().unwrap());
    let parent: ProcessIdentity = serde_json::from_value(value["parent"].clone()).unwrap();
    let nonce = value["nonce"].as_str().unwrap().to_owned();
    let deadline_ms = value["deadline"].as_u64().unwrap();
    let mode = value["mode"].as_str().unwrap();
    if mode == "wrong-hello" {
        let control = operation(deadline_ms, MAXIMUM.as_millis() as u64).unwrap();
        let scope = BrokerScope::for_dd_channel(&home, &nonce).unwrap();
        let mut stream =
            BrokerStream::connect_with_control(&scope, control.deadline(), check(&control))
                .unwrap();
        let executable = std::env::current_exe().unwrap();
        let (file, digest) = storage::locked_sha256(&executable, 512 << 20, &control).unwrap();
        send(
            &mut stream,
            &Hello {
                wire: WIRE.into(),
                nonce: "0".repeat(32),
                process: own().unwrap(),
                digest,
            },
        )
        .unwrap();
        drop(file);
        loop {
            std::thread::park();
        }
    }
    let binding = Binding {
        home: home.clone(),
        parent,
        nonce,
        deadline_ms,
    };
    let result = serve(binding, |_, control, parent| {
        parent()?;
        std::fs::write(home.join("channel-read"), b"synthetic-only")?;
        if mode == "hang" {
            loop {
                std::thread::park();
            }
        }
        if mode == "reject" {
            return Err(fault("synthetic original GUI unavailable"));
        }
        control.check()?;
        super::super::projection::choices(
            &json!({"data":[{"teamId":"exact-room","teamName":"中文","channels":[{"channelId":"07","channelType":2,"name":"频道"}]}]}),
        )
    });
    if result.is_ok() {
        std::fs::write(home.join("channel-finished"), b"finished").unwrap();
    }
    std::process::exit(result.err().map_or(0, |e| e.exit_code));
}
fn reaped(id: Option<ProcessIdentity>) {
    let id = id.expect("one controlled helper was launched");
    assert_ne!(
        observe_lifetime(&id, || Ok(())).unwrap(),
        ProcessLifetime::Running
    );
}
#[test]
fn same_account_nonce_pipe_returns_only_choices_and_reaps_the_original_controlled_helper() {
    let dir = TestDir::new();
    let (result, id) = execute(&dir, "normal", Duration::from_secs(8));
    let choices = result.unwrap().value().unwrap();
    assert_eq!(choices["total"], 2);
    assert_eq!(choices["items"][1]["channel_id"], "07");
    assert_eq!(
        std::fs::read(dir.0.join("channel-finished")).unwrap(),
        b"finished"
    );
    reaped(id);
}
#[test]
fn invalid_peer_hello_and_expired_helper_never_reach_the_channel_reader() {
    let dir = TestDir::new();
    let (result, id) = execute(&dir, "wrong-hello", Duration::from_secs(8));
    assert!(result.is_err());
    assert!(!dir.0.join("channel-read").exists());
    reaped(id);
    let before = std::cell::Cell::new(false);
    let args = vec![
        "--publish-dd-channel-helper".into(),
        dir.0.to_string_lossy().into_owned(),
        std::process::id().to_string(),
        "1".into(),
        "a".repeat(32),
        "1".into(),
    ];
    let result = serve(Binding::parse(&args).unwrap(), |_, _, _| {
        before.set(true);
        Err(invalid())
    });
    assert!(result.is_err());
    assert!(!before.get());
    let mut extra = args.clone();
    extra.push("--url=http://127.0.0.1/".into());
    assert!(Binding::parse(&extra).is_err());
}
#[test]
fn helper_rejection_and_stalled_reader_have_one_launch_no_replay_and_verified_retirement() {
    for mode in ["reject", "hang"] {
        let dir = TestDir::new();
        let (result, id) = execute(&dir, mode, Duration::from_millis(1800));
        let error = result.err().unwrap();
        assert!(!error.to_string().contains("private-token"));
        assert!(
            dir.0.join("channel-read").exists(),
            "the original controlled reader actually ran"
        );
        reaped(id);
    }
}
