use super::*;
use crate::test_support::TestDir;
const FIXTURE: &str = "LYCHEEDEV_DD_TASK_TEST";
const REQUESTER: &str = "LYCHEEDEV_DD_TASK_REQUESTER_TEST";
const STARTER: &str = "LYCHEEDEV_DD_TASK_STARTER_TEST";
fn start(dir: &Path, mode: &str, control: &OperationControl) -> Result<Value> {
    let executable = std::env::current_exe().unwrap();
    Client::start_with(dir, &executable, control, |record, home| {
        lycheedev_windows::detached::spawn_hidden_no_inherit(
            &executable,
            &[
                "--ignored".into(),
                "--exact".into(),
                "dd::task::tests::owner_process".into(),
                "--nocapture".into(),
            ],
            home,
            &[
                (START_ENV.into(), record.id.clone().into()),
                (
                    FIXTURE.into(),
                    json!({"home":home,"id":record.id,"mode":mode})
                        .to_string()
                        .into(),
                ),
            ],
        )
        .map_err(platform)
    })
}
fn client(dir: &Path, id: &str) -> Client {
    Client::attach(dir, &std::env::current_exe().unwrap(), id).unwrap()
}
fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !path.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn released(dir: &Path, record: &Record) {
    let store = Store::open(dir, &OperationControl::default(), false).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !store.released(record).unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}
struct FixtureBackend {
    home: PathBuf,
    count: u64,
}
impl Backend for FixtureBackend {
    fn identity(&self) -> Value {
        json!({"fixture":true,"calls":self.count})
    }
    fn execute(&mut self, call: Call, control: &OperationControl) -> Result<Value> {
        control.check()?;
        if matches!(call, Call::Status) {
            return Ok(self.identity());
        }
        self.count += 1;
        fs::write(self.home.join("business-calls"), self.count.to_string())?;
        if matches!(&call,Call::Post{path,..}if path=="/hang") {
            loop {
                std::thread::park();
            }
        }
        Ok(json!({"calls":self.count,"owner":std::process::id()}))
    }
    fn retired(&self) -> bool {
        false
    }
    fn close(self, _: &OperationControl) -> Result<()> {
        fs::write(self.home.join("closed"), b"closed")?;
        Ok(())
    }
}
#[test]
#[ignore = "private DD task owner; parent tests supply its complete role and binding"]
fn owner_process() {
    let config: Value =
        serde_json::from_str(&std::env::var(FIXTURE).expect("owner fixture role is required"))
            .unwrap();
    let home = PathBuf::from(config["home"].as_str().unwrap());
    let id = config["id"].as_str().unwrap();
    assert_eq!(std::env::var(START_ENV).unwrap(), id);
    let mode = config["mode"].as_str().unwrap();
    let idle = if mode == "idle" {
        Duration::from_millis(350)
    } else {
        IDLE
    };
    let result = server::serve(&home, id, idle, |_, _, control| {
        let count = fs::read_to_string(home.join("opens"))
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
            + 1;
        fs::write(home.join("opens"), count.to_string())?;
        fs::write(home.join("factory-entered"), b"entered")?;
        if mode == "cancel-start" {
            loop {
                control.check()?;
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if mode == "reject" {
            return Err(fault("synthetic task activation rejection"));
        }
        Ok(FixtureBackend {
            home: home.clone(),
            count: 0,
        })
    });
    if mode != "cancel-start" && mode != "reject" {
        result.unwrap();
    }
}
#[test]
#[ignore = "private DD task starter; parent tests supply its complete role"]
fn starter_process() {
    let home = PathBuf::from(std::env::var_os(STARTER).expect("starter role is required"));
    let result = start(&home, "normal", &OperationControl::default()).unwrap();
    fs::write(
        home.join("starter-result"),
        serde_json::to_vec(&result).unwrap(),
    )
    .unwrap();
}
#[test]
#[ignore = "private DD task requester; parent tests supply its complete role and binding"]
fn requester_process() {
    let value: Value =
        serde_json::from_str(&std::env::var(REQUESTER).expect("requester role is required"))
            .unwrap();
    let home = PathBuf::from(value["home"].as_str().unwrap());
    let output = client(&home, value["session"].as_str().unwrap())
        .get("/read", &[], &OperationControl::default())
        .unwrap();
    fs::write(
        home.join("requester-result"),
        serde_json::to_vec(&output).unwrap(),
    )
    .unwrap();
}
#[test]
fn unpublished_owner_reports_pending_and_launcher_failure_is_recorded() {
    let dir = TestDir::new();
    let executable = std::env::current_exe().unwrap();
    let control = OperationControl::default();
    let error = Client::start_with(&dir.0, &executable, &control, |record, home| {
        let status = Client::status(home, &executable, Some(&record.id), &control)?;
        assert_eq!(status["phase"], "starting");
        assert_eq!(status["startup_pending"], true);
        assert_eq!(status["cleanup_complete"], false);
        assert_eq!(
            client(home, &record.id)
                .stop(&control)
                .unwrap_err()
                .exit_code,
            6
        );
        Err(fault("synthetic launcher rejection"))
    })
    .unwrap_err();
    assert_eq!(error.message, "synthetic launcher rejection");
    let store = Store::open(&dir.0, &control, false).unwrap();
    let record = store.active(&control).unwrap().unwrap();
    assert!(matches!(record.phase, Phase::Failed));
    assert!(record.owner.is_none());
    assert_eq!(
        Client::status(&dir.0, &executable, Some(&record.id), &control).unwrap()["cleanup_complete"],
        true
    );
}
#[test]
fn rejected_activation_is_not_retried_and_keeps_its_original_failure() {
    let dir = TestDir::new();
    let error = start(&dir.0, "reject", &OperationControl::default()).unwrap_err();
    assert_eq!(error.message, "synthetic task activation rejection");
    let store = Store::open(&dir.0, &OperationControl::default(), false).unwrap();
    let record = store.active(&OperationControl::default()).unwrap().unwrap();
    released(&dir.0, &record);
    assert!(matches!(record.phase, Phase::Failed));
    assert_eq!(fs::read(dir.0.join("opens")).unwrap(), b"1");
    assert!(!dir.0.join("business-calls").exists());
}
#[test]
fn separate_command_processes_reuse_one_owner_and_explicit_stop_confirms_resources() {
    let dir = TestDir::new();
    let starter = lycheedev_windows::detached::spawn_hidden_no_inherit(
        &std::env::current_exe().unwrap(),
        &[
            "--ignored".into(),
            "--exact".into(),
            "dd::task::tests::starter_process".into(),
            "--nocapture".into(),
        ],
        &dir.0,
        &[(STARTER.into(), dir.0.as_os_str().to_owned())],
    )
    .unwrap();
    let control = OperationControl::default();
    let exit = loop {
        if let Some(exit) = starter.try_wait().unwrap() {
            break exit;
        }
        control.check().unwrap();
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        starter.terminate_and_close(Instant::now() + REAP).unwrap(),
        exit
    );
    assert_eq!(exit, 0);
    let started: Value =
        serde_json::from_slice(&fs::read(dir.0.join("starter-result")).unwrap()).unwrap();
    let id = started["session_id"].as_str().unwrap();
    assert_eq!(started["phase"], "ready");
    assert_eq!(started["reused"], false);
    let cancelled = OperationControl::default();
    cancelled.tracked_cancel_handle().unwrap().cancel();
    let not_sent = client(&dir.0, id)
        .post(
            "/write",
            &[],
            &json!({"body":"not submitted"}),
            true,
            &cancelled,
        )
        .unwrap_err();
    assert_eq!(not_sent.exit_code, 7);
    assert_eq!(not_sent.details["submission"], "not_sent");
    assert!(!dir.0.join("business-calls").exists());
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "dd::task::tests::requester_process",
            "--nocapture",
        ])
        .env(REQUESTER, json!({"home":dir.0,"session":id}).to_string());
    let requester =
        lycheedev_core::process::run(command, None, 8192, 8192, &OperationControl::default())
            .unwrap();
    assert_eq!(requester.status, 0);
    let output: Value =
        serde_json::from_slice(&fs::read(dir.0.join("requester-result")).unwrap()).unwrap();
    assert_eq!(output["calls"], 1);
    assert_eq!(output["owner"], started["owner"]["pid"]);
    assert_eq!(
        client(&dir.0, id)
            .get("/read", &[], &OperationControl::default())
            .unwrap()["calls"],
        2
    );
    let reused = start(&dir.0, "normal", &OperationControl::default()).unwrap();
    assert_eq!(reused["session_id"], id);
    assert_eq!(reused["reused"], true);
    assert_eq!(fs::read(dir.0.join("opens")).unwrap(), b"1");
    let stopped = client(&dir.0, id)
        .stop(&OperationControl::default())
        .unwrap();
    assert_eq!(stopped["cleanup_complete"], true);
    assert_eq!(
        Client::status(
            &dir.0,
            &std::env::current_exe().unwrap(),
            Some(id),
            &OperationControl::default()
        )
        .unwrap()["running"],
        false
    );
}
#[test]
fn selected_object_upload_records_mutation_intent_on_the_original_owner_without_another_start() {
    let dir = TestDir::new();
    let control = OperationControl::default();
    let started = start(&dir.0, "normal", &control).unwrap();
    let id = started["session_id"].as_str().unwrap();
    let selected = dir.0.join("selected.zip");
    fs::write(&selected, b"fixture object bytes").unwrap();
    let upload = super::super::upload::ObjectUpload::select(
        &selected,
        super::super::upload::UploadKind::PluginArchive,
        &control,
    )
    .unwrap();
    let result = upload.execute(&client(&dir.0, id), &control).unwrap();
    assert_eq!(result["calls"], 1);
    let status = Client::status(
        &dir.0,
        &std::env::current_exe().unwrap(),
        Some(id),
        &control,
    )
    .unwrap();
    assert_eq!(status["last_operation"]["mutation"], true);
    assert_eq!(status["last_operation"]["outcome"], "completed");
    assert_eq!(fs::read(dir.0.join("opens")).unwrap(), b"1");
    assert!(
        !serde_json::to_string(&status)
            .unwrap()
            .contains("fixture object bytes")
    );
    assert_eq!(
        client(&dir.0, id).stop(&control).unwrap()["cleanup_complete"],
        true
    );
}
#[test]
fn cancelled_activation_retires_its_original_owner_and_keeps_the_start_record() {
    let dir = TestDir::new();
    let home = dir.0.clone();
    let control = OperationControl::default();
    let caller = control.clone();
    let work = std::thread::spawn(move || start(&home, "cancel-start", &caller));
    let deadline = Instant::now() + Duration::from_secs(95);
    while !dir.0.join("factory-entered").exists()
        && !work.is_finished()
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let entered = dir.0.join("factory-entered").exists();
    control.tracked_cancel_handle().unwrap().cancel();
    let result = work.join().unwrap();
    assert!(entered, "activation was not reached; {result:?}");
    let error = result.unwrap_err();
    assert_eq!(error.exit_code, 7);
    let store = Store::open(&dir.0, &OperationControl::default(), false).unwrap();
    let record = store.active(&OperationControl::default()).unwrap().unwrap();
    released(&dir.0, &record);
    assert_eq!(fs::read(dir.0.join("opens")).unwrap(), b"1");
    assert!(record.owner.is_some());
}
#[test]
fn interrupted_mutation_keeps_its_original_digest_and_never_replays_into_a_new_owner() {
    let dir = TestDir::new();
    let started = start(&dir.0, "normal", &OperationControl::default()).unwrap();
    let id = started["session_id"].as_str().unwrap();
    let control = OperationControl::default();
    let caller = control.clone();
    let connection = client(&dir.0, id);
    let work = std::thread::spawn(move || {
        connection.post(
            "/hang",
            &[],
            &json!({"body":"private original config"}),
            true,
            &caller,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while !dir.0.join("business-calls").exists() && !work.is_finished() && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let entered = dir.0.join("business-calls").exists();
    control.tracked_cancel_handle().unwrap().cancel();
    let result = work.join().unwrap();
    if !entered {
        client(&dir.0, id)
            .stop(&OperationControl::default())
            .unwrap();
    }
    assert!(entered, "mutation was not reached: {result:?}");
    let error = result.unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert_eq!(
        error.details["schema"],
        lycheedev_core::storage::FILE_CLEANUP_SCHEMA
    );
    assert_eq!(
        error.details["primary"]["details"]["submission"],
        "uncertain"
    );
    assert_eq!(error.details["control"]["code"], "command.cancelled");
    let store = Store::open(&dir.0, &OperationControl::default(), false).unwrap();
    let record = store.record(id, &OperationControl::default()).unwrap();
    released(&dir.0, &record);
    let status = Client::status(
        &dir.0,
        &std::env::current_exe().unwrap(),
        Some(id),
        &OperationControl::default(),
    )
    .unwrap();
    assert_eq!(status["last_operation"]["outcome"], "uncertain");
    assert_eq!(status["replay_allowed"], false);
    assert!(
        !serde_json::to_string(&record)
            .unwrap()
            .contains("private original config")
    );
    assert_eq!(fs::read(dir.0.join("business-calls")).unwrap(), b"1");
    let next = start(&dir.0, "normal", &OperationControl::default()).unwrap();
    assert_ne!(next["session_id"], id);
    assert_eq!(fs::read(dir.0.join("business-calls")).unwrap(), b"1");
    client(&dir.0, next["session_id"].as_str().unwrap())
        .stop(&OperationControl::default())
        .unwrap();
    assert!(
        store
            .record(id, &OperationControl::default())
            .unwrap()
            .last
            .unwrap()
            .mutation
    );
}
#[test]
fn stale_session_selector_cannot_execute_or_close_a_replacement_task() {
    let dir = TestDir::new();
    let first = start(&dir.0, "normal", &OperationControl::default()).unwrap();
    let old = first["session_id"].as_str().unwrap();
    client(&dir.0, old)
        .stop(&OperationControl::default())
        .unwrap();
    let next = start(&dir.0, "normal", &OperationControl::default()).unwrap();
    let id = next["session_id"].as_str().unwrap();
    assert_eq!(
        client(&dir.0, old)
            .get("/read", &[], &OperationControl::default())
            .unwrap_err()
            .exit_code,
        4
    );
    assert_eq!(
        client(&dir.0, id)
            .get("/read", &[], &OperationControl::default())
            .unwrap()["calls"],
        1
    );
    client(&dir.0, id)
        .stop(&OperationControl::default())
        .unwrap();
}
#[test]
fn idle_owner_closes_its_session_without_another_command() {
    let dir = TestDir::new();
    let started = start(&dir.0, "idle", &OperationControl::default()).unwrap();
    let id = started["session_id"].as_str().unwrap();
    let store = Store::open(&dir.0, &OperationControl::default(), false).unwrap();
    let record = store.record(id, &OperationControl::default()).unwrap();
    wait_for(&dir.0.join("closed"));
    released(&dir.0, &record);
    let status = Client::status(
        &dir.0,
        &std::env::current_exe().unwrap(),
        Some(id),
        &OperationControl::default(),
    )
    .unwrap();
    assert_eq!(status["cleanup_complete"], true);
    assert_eq!(status["running"], false);
}
