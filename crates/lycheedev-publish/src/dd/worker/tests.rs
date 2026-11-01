use super::*;
use crate::test_support::TestDir;

const FIXTURE: &str = "LYCHEEDEV_DD_WORKER_TEST_BINDING";
fn spawn(dir: &TestDir, control: &OperationControl, mode: &str) -> Result<Worker> {
    Worker::spawn_with(&dir.0, control, |nonce, parent| {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "dd::worker::tests::fixture_process",
                "--nocapture",
            ])
            .env(START_ENV, nonce)
            .env(
                FIXTURE,
                json!({"home":dir.0,"nonce":nonce,"parent":parent,"mode":mode}).to_string(),
            );
        command
    })
}
struct FixtureSession {
    home: PathBuf,
    count: u64,
}
impl Session for FixtureSession {
    fn identity(&self) -> Value {
        json!({"fixture":true,"calls":self.count})
    }
    fn execute(&mut self, home: &Path, call: &Call, control: &OperationControl) -> Result<Value> {
        control.check()?;
        self.count += 1;
        std::fs::write(self.home.join("fixture-calls"), self.count.to_string())?;
        if matches!(call,Call::Get{path,..}|Call::Post{path,..}if path=="/hang") {
            loop {
                std::thread::park();
            }
        }
        match call {
            Call::Get { query, .. } => Ok(json!({"calls":self.count,"query":query})),
            Call::Post { body, .. } => Ok(json!({"calls":self.count,"body":body})),
            Call::Status => Ok(self.identity()),
            Call::Upload { request } => {
                assert_eq!(home, self.home);
                let wire = serde_json::to_value(request)?;
                Ok(json!({"fixture_upload":true,"sha256":wire["sha256"],"bytes":wire["size"]}))
            }
            Call::Channels => Ok(
                json!({"total":0,"items":[],"dependencies":[{"parent":"room_id","children":["channel_id","channel_type"]}]}),
            ),
            _ => Err(protocol()),
        }
    }
    fn close(self) -> Result<()> {
        std::fs::write(self.home.join("fixture-closed"), b"closed")?;
        Ok(())
    }
}
#[test]
#[ignore = "private worker fixture; exact parent tests supply its required role and binding"]
fn fixture_process() {
    let value: Value =
        serde_json::from_str(&std::env::var(FIXTURE).expect("parent fixture role is required"))
            .unwrap();
    let home = PathBuf::from(value["home"].as_str().unwrap());
    let nonce = value["nonce"].as_str().unwrap().to_owned();
    assert_eq!(std::env::var(START_ENV).unwrap(), nonce);
    let parent: ProcessIdentity = serde_json::from_value(value["parent"].clone()).unwrap();
    let mode = value["mode"].as_str().unwrap();
    let result = serve(
        Binding {
            home,
            nonce,
            parent,
        },
        |home, _| {
            let own = Process::open(std::process::id()).map_err(platform)?;
            let identity = own.identity().clone();
            own.close().map_err(platform)?;
            std::fs::write(home.join("fixture-started"), serde_json::to_vec(&identity)?)?;
            if mode == "hang-start" {
                loop {
                    std::thread::park();
                }
            }
            if mode == "reject-start" {
                return Err(fault("synthetic startup rejection"));
            }
            Ok(FixtureSession {
                home: home.to_owned(),
                count: 0,
            })
        },
    );
    if mode == "bad-frame" {
        assert_eq!(result.unwrap_err().exit_code, 4);
    } else if mode == "reject-start" {
        assert_eq!(result.unwrap_err().message, "synthetic startup rejection");
    } else {
        result.unwrap();
    }
}
#[test]
fn one_owned_process_survives_startup_deadline_and_serves_serial_exact_values_then_confirms_cleanup()
 {
    let dir = TestDir::new();
    let startup = OperationControl::uncancelable(Instant::now() + Duration::from_secs(2));
    let mut worker = spawn(&dir, &startup, "normal").unwrap();
    let identity = worker.process().unwrap().clone();
    assert_eq!(worker.identity()["fixture"], true);
    std::thread::sleep(Duration::from_millis(2100));
    assert!(startup.check().is_err());
    let first = OperationControl::default();
    assert_eq!(
        worker
            .get("/read", &[("sn", "中文&+?".into())], &first)
            .unwrap(),
        json!({"calls":1,"query":[["sn","中文&+?"]]})
    );
    let body = json!({"integer":9007199254740993u64,"clear":null,"false":false,"text":"private opaque WA content"});
    assert_eq!(
        worker
            .post("/write", &[], &body, true, &OperationControl::default())
            .unwrap(),
        json!({"calls":2,"body":body})
    );
    assert_eq!(worker.process(), Some(&identity));
    assert_eq!(
        worker
            .call(Call::Channels, &OperationControl::default())
            .unwrap(),
        json!({"total":0,"items":[],"dependencies":[{"parent":"room_id","children":["channel_id","channel_type"]}]})
    );
    let selected = dir.0.join("selected.zip");
    std::fs::write(&selected, b"original fixture bytes").unwrap();
    let request=serde_json::from_value(json!({"path":selected,"kind":"plugin_archive","sha256":lycheedev_core::storage::sha256(b"original fixture bytes"),"size":22})).unwrap();
    assert_eq!(
        worker
            .call(Call::Upload { request }, &OperationControl::default())
            .unwrap(),
        json!({"fixture_upload":true,"sha256":lycheedev_core::storage::sha256(b"original fixture bytes"),"bytes":22})
    );
    assert_eq!(
        worker.close(&OperationControl::default()).unwrap()["cleanup_complete"],
        true
    );
    assert_eq!(
        std::fs::read(dir.0.join("fixture-closed")).unwrap(),
        b"closed"
    );
}
#[test]
fn uninterruptible_native_work_is_retired_by_its_job_at_the_original_rpc_deadline_without_replay() {
    let dir = TestDir::new();
    let mut worker = spawn(&dir, &OperationControl::default(), "normal").unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(150));
    let started = Instant::now();
    let error = worker
        .post(
            "/hang",
            &[],
            &json!({"private":"do-not-echo"}),
            true,
            &control,
        )
        .unwrap_err();
    assert_eq!(error.exit_code, 5);
    assert_eq!(error.details["submission"], "uncertain");
    assert_eq!(error.details["replay_allowed"], false);
    assert!(!error.to_string().contains("do-not-echo"));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(worker.child.is_none() && worker.job.is_none() && worker.stream.is_none());
    assert_eq!(std::fs::read(dir.0.join("fixture-calls")).unwrap(), b"1");
    assert!(
        worker
            .get("/read", &[], &OperationControl::default())
            .is_err()
    );
    assert!(!dir.0.join("fixture-closed").exists());
}
#[test]
fn cancelled_request_is_not_sent_and_does_not_retire_a_healthy_worker() {
    let dir = TestDir::new();
    let mut worker = spawn(&dir, &OperationControl::default(), "normal").unwrap();
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    let error = worker
        .post("/write", &[], &json!({}), true, &control)
        .unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert_eq!(error.details["submission"], "not_sent");
    assert!(!dir.0.join("fixture-calls").exists());
    assert_eq!(
        worker
            .get("/read", &[], &OperationControl::default())
            .unwrap()["calls"],
        1
    );
    worker.close(&OperationControl::default()).unwrap();
}
#[test]
fn startup_rejection_and_uninterruptible_initialization_both_release_the_exact_child() {
    for mode in ["reject-start", "hang-start"] {
        let dir = TestDir::new();
        let control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(700));
        let started = Instant::now();
        let error = spawn(&dir, &control, mode).err().unwrap();
        assert_eq!(error.exit_code, if mode == "reject-start" { 3 } else { 5 });
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(dir.0.join("fixture-started").exists());
        let identity: ProcessIdentity =
            serde_json::from_slice(&std::fs::read(dir.0.join("fixture-started")).unwrap()).unwrap();
        assert_ne!(
            lycheedev_windows::process::observe_lifetime(&identity, || Ok(())).unwrap(),
            lycheedev_windows::process::ProcessLifetime::Running
        );
    }
}
#[test]
fn a_stale_worker_identity_is_rejected_before_any_business_effect() {
    let dir = TestDir::new();
    let control = OperationControl::default();
    let mut worker = spawn(&dir, &control, "bad-frame").unwrap();
    let stream = worker.stream.as_mut().unwrap();
    send(
        stream,
        &Request {
            wire: WIRE.into(),
            nonce: "00000000000000000000000000000000".into(),
            id: 1,
            deadline_ms: wire_deadline(&control).unwrap(),
            call: Call::Post {
                path: "/write".into(),
                query: vec![],
                body: json!({"private":"opaque"}),
                mutation: true,
            },
        },
    )
    .unwrap();
    assert!(read_frame::<Response>(stream).is_err());
    worker.retire(Instant::now() + REAP, false).unwrap();
    assert!(!dir.0.join("fixture-calls").exists());
    assert!(dir.0.join("fixture-closed").exists());
}
