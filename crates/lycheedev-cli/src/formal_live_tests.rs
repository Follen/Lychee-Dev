//! Offline command/output fixtures pinned to Go 3.2.0. Native process/input
//! acceptance is separate; these tests never select or drive a real game.
use super::*;
use lycheedev_core::control::{StopCause, TrackedCancellation};
use lycheedev_live::go_mailbox_v1::{Identity, connection::RawReport, recovery::Continuation};
use std::{
    cell::RefCell,
    fs,
    io::{self, Write},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-formal-cli-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[derive(Debug, PartialEq, Eq)]
enum Call {
    Connect(String, u32, String, String, String, String),
    Status(String),
    Resume(String),
    Disconnect(String),
    Execute(String, String, Vec<u8>, i32, OperationPolicy),
    Activate(String, u32, String),
    Reload(String, String),
    Doctor(String),
    Fix,
}
struct Fake {
    root: PathBuf,
    calls: Rc<RefCell<Vec<Call>>>,
    outcome: Option<ServiceOutcome>,
    report: DoctorResult,
    cancellation: Option<TrackedCancellation>,
}
impl Services for Fake {
    fn project_root(&self) -> &Path {
        &self.root
    }
    fn invoke(&mut self, action: Action<'_>, control: &OperationControl) -> ServiceOutcome {
        assert!(
            control.stop_cause().is_ok(),
            "formal adapter lost ordered control"
        );
        let call = match action {
            Action::Connect(t) => Call::Connect(
                t.installation.clone(),
                t.pid,
                t.filter.character.clone(),
                t.filter.realm.clone(),
                t.filter.build.clone(),
                t.filter.product.clone(),
            ),
            Action::Status(s) => Call::Status(s.into()),
            Action::Resume(s) => Call::Resume(s.into()),
            Action::Disconnect(s) => Call::Disconnect(s.into()),
            Action::Execute(s, r) => Call::Execute(
                s.into(),
                r.request.into(),
                r.code.to_vec(),
                r.budget_seconds,
                r.policy,
            ),
            Action::Activate(t, k) => Call::Activate(t.installation.clone(), t.pid, k.into()),
            Action::Reload(s, k) => Call::Reload(s.into(), k.into()),
        };
        self.calls.borrow_mut().push(call);
        if let Some(c) = &self.cancellation {
            c.cancel();
            assert_eq!(control.stop_cause().unwrap(), Some(StopCause::Cancelled));
        }
        self.outcome
            .take()
            .unwrap_or(ServiceOutcome::Completed(project_result()))
    }
    fn doctor(
        &mut self,
        _: &TargetRequest,
        session: &str,
        control: &OperationControl,
    ) -> DoctorResult {
        assert!(control.stop_cause().is_ok());
        self.calls.borrow_mut().push(Call::Doctor(session.into()));
        self.report.clone()
    }
    fn fix(&mut self, _: &DoctorResult, _: &Invocation, _: &OperationControl) -> ServiceOutcome {
        self.calls.borrow_mut().push(Call::Fix);
        self.outcome
            .take()
            .unwrap_or(ServiceOutcome::Completed(project_result()))
    }
}
fn fake(root: &Path, calls: Rc<RefCell<Vec<Call>>>) -> Fake {
    Fake {
        root: root.into(),
        calls,
        outcome: None,
        report: DoctorResult::default(),
        cancellation: None,
    }
}
fn invocation(words: &[&str], root: &Path) -> Invocation {
    let mut inv = crate::parse(&words.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap();
    inv.cwd = root.into();
    inv.home = root.join("home");
    inv
}
fn assert_clap_error(error: &ToolkitError, kind: &str, token: &str) {
    assert_eq!(error.code, "command.invalid_arguments");
    assert_eq!(error.exit_code, 2);
    assert_eq!(error.details["parser"], "clap");
    assert_eq!(error.details["kind"], kind);
    assert!(error.details["diagnostic"].as_str().unwrap().contains(token), "{error:?}");
}
fn run(inv: &Invocation, fake: Fake) -> Response {
    // Exactly the production main boundary, followed by its injectable formal
    // service factory. A generic Outcome/old host cannot answer this test.
    crate::dispatch_formal_entry_with(inv, |inv| dispatch_with(inv, |_| Ok(fake)))
        .expect("formal default entry")
}
fn project_result() -> ProjectResult {
    ProjectResult {
        continuation:Continuation {kind:"continue".into(),session:"CON-original".into(),request_id:"request-1".into(),goal:"execute".into(),progress_version:3,remaining_budget_ms:Some(42),blocker:None},
        process_end:String::new(),runtime_end:None,input:None,waiting:String::new(),session:"CON-original".into(),operation:"OP-original".into(),operation_state:"result_verified".into(),scans:String::new(),
        identity:Identity::default(),bound:true,closed:false,stage:"result_verified".into(),complete:false,report_state:"verified".into(),cleanup:"pending".into(),cleanup_method:String::new(),
        report:Some(RawReport(br#"{ "ok":false, "same":1, "same":2, "big":9007199254740993123456789, "minus":-0, "scale":1.2300e+04, "html":"<>&" }"#.to_vec())),
        journal:"fixture/con.jsonl".into(),reload:None,report_origin:None,
    }
}
fn rendered(response: &Response, format: &str) -> String {
    let mut out = vec![];
    render(&mut out, format, response).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn main_routes_con_aliases_to_formal_service_and_never_old_home() {
    let f = Fixture::new();
    for (words, expected) in [
        (
            vec!["live", "status", "CON-original"],
            Call::Status("CON-original".into()),
        ),
        (
            vec!["live", "session", "CON-original"],
            Call::Status("CON-original".into()),
        ),
        (
            vec!["live", "resume", "CON-original"],
            Call::Resume("CON-original".into()),
        ),
        (
            vec!["live", "connect", "--session", "CON-original"],
            Call::Resume("CON-original".into()),
        ),
        (
            vec!["live", "disconnect", "CON-original"],
            Call::Disconnect("CON-original".into()),
        ),
        (
            vec![
                "live",
                "reload",
                "--session",
                "CON-original",
                "--request",
                "reload-key",
            ],
            Call::Reload("CON-original".into(), "reload-key".into()),
        ),
    ] {
        let inv = invocation(&words, &f.0);
        let calls = Rc::new(RefCell::new(vec![]));
        let response = run(&inv, fake(&f.0, calls.clone()));
        assert_eq!(*calls.borrow(), [expected]);
        assert_eq!(response.context["transport"], TRANSPORT);
        assert!(!inv.home.exists());
        assert!(!f.0.join(".lycheedev").exists());
    }
}
#[test]
fn route_and_argument_priority_matches_frozen_go() {
    let f = Fixture::new();
    let cases = [
        (
            vec!["live", "execute", "--request", "x"],
            "Native execution requires --session CON-...; historical OP/BTP execution is retired",
        ),
        (
            vec!["live", "bugs", "--session", "OP-old"],
            "Native execution requires --session CON-...; historical OP/BTP execution is retired",
        ),
        (
            vec![
                "live",
                "connect",
                "--session",
                "CON-original",
                "--character",
                "new",
            ],
            "resume the retained CON session without new target selectors",
        ),
        (
            vec![
                "live",
                "execute",
                "--session",
                "CON-original",
                "--request",
                "x",
                "--budget-seconds",
                "1",
            ],
            "live execute requires --session CON-... --request <key> --budget-seconds <1-120> and exactly one of --file or --probe",
        ),
        (
            vec![
                "live",
                "bugs",
                "--session",
                "CON-original",
                "--count",
                "101",
                "--request",
                "x",
            ],
            "live bugs requires --request and --count <1-100>",
        ),
        (
            vec!["live", "reload", "fallback", "--request", "x"],
            "native activation requires --installation --pid --request; resume a pending CON id without new input",
        ),
        (
            vec!["live", "cancel", "CON-original"],
            "CON connections support execute, bugs, status, session, resume, reload and disconnect; cleanup is automatic",
        ),
    ];
    for (words, message) in cases {
        let inv = invocation(&words, &f.0);
        let calls = Rc::new(RefCell::new(vec![]));
        let response = run(&inv, fake(&f.0, calls.clone()));
        assert_eq!(response.error.as_ref().unwrap().message, message);
        assert_eq!(response.exit_code(), 2);
        assert!(response.context.is_empty());
        assert!(calls.borrow().is_empty());
    }
}
#[test]
fn parser_live_typed_options_keep_admitted_value_order_and_exact_errors() {
    for (args, message) in [
        (
            vec!["live", "execute", "--budget-seconds", "0"],
            "--budget-seconds must be between 1 and 120",
        ),
        (
            vec!["live", "connect", "--pid", "0", "--wait-seconds", "0"],
            "invalid --pid",
        ),
        (
            vec!["live", "connect", "--wait-seconds", "0", "--pid", "0"],
            "--wait-seconds must be between 1 and 600",
        ),
        (vec!["live", "bugs", "--count", "2001"], "invalid --count"),
    ] {
        let e = crate::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap_err();
        assert_eq!(e.message, message);
    }
    let error = crate::parse(&["live", "execute", "--policy", "retry"].map(str::to_owned)).unwrap_err();
    assert_clap_error(&error, "InvalidValue", "--policy");
    assert!(error.details["diagnostic"].as_str().unwrap().contains("retry"));
}
#[test]
fn connect_and_activation_preserve_explicit_target() {
    let f = Fixture::new();
    let calls = Rc::new(RefCell::new(vec![]));
    let inv = invocation(
        &[
            "live",
            "connect",
            "--installation",
            "selected",
            "--pid",
            "17",
            "--character",
            "Name",
            "--realm",
            "Realm",
        ],
        &f.0,
    );
    run(&inv, fake(&f.0, calls.clone()));
    assert_eq!(
        *calls.borrow(),
        [Call::Connect(
            "selected".into(),
            17,
            "Name".into(),
            "Realm".into(),
            String::new(),
            String::new()
        )]
    );
    calls.borrow_mut().clear();
    let inv = invocation(
        &[
            "live",
            "reload",
            "fallback",
            "--installation",
            "selected",
            "--pid",
            "17",
            "--request",
            "activation-key",
        ],
        &f.0,
    );
    run(&inv, fake(&f.0, calls.clone()));
    assert_eq!(
        *calls.borrow(),
        [Call::Activate(
            "selected".into(),
            17,
            "activation-key".into()
        )]
    );
}
#[test]
fn bugs_uses_exact_fixed_go_source_budget_and_observation_policy() {
    let f = Fixture::new();
    let calls = Rc::new(RefCell::new(vec![]));
    let inv = invocation(
        &[
            "live",
            "bugs",
            "--session",
            "CON-original",
            "--request",
            "bugs-key",
        ],
        &f.0,
    );
    run(&inv, fake(&f.0, calls.clone()));
    let calls = calls.borrow();
    let Call::Execute(_, key, code, budget, policy) = &calls[0] else {
        panic!("execute required")
    };
    assert_eq!(key, "bugs-key");
    assert_eq!(*budget, 5);
    assert_eq!(*policy, OperationPolicy::Observation);
    assert_eq!(code,b"local snapshot, reason = LycheeDevInternal.Diagnostics.SnapshotRecentErrors(80, \"provider_storage\")\nreturn {schema=\"lycheedev.bugs.v1\",status=snapshot and \"completed\" or \"unavailable\",complete=snapshot and snapshot.complete==true or false,snapshot=snapshot,error=reason}");
}
#[test]
fn execute_preserves_file_bytes_and_does_not_invent_a_request() {
    let f = Fixture::new();
    let calls = Rc::new(RefCell::new(vec![]));
    let bytes = b"return '\xff\xfe'\n";
    fs::write(f.0.join("probe.lua"), bytes).unwrap();
    let inv = invocation(
        &[
            "live",
            "execute",
            "--session",
            "CON-original",
            "--request",
            "same-key",
            "--budget-seconds",
            "7",
            "--file",
            "probe.lua",
        ],
        &f.0,
    );
    run(&inv, fake(&f.0, calls.clone()));
    assert_eq!(
        *calls.borrow(),
        [Call::Execute(
            "CON-original".into(),
            "same-key".into(),
            bytes.to_vec(),
            7,
            OperationPolicy::Opaque
        )]
    );
}
#[test]
fn execute_reuses_exact_stored_probe_revision_without_reput() {
    let f = Fixture::new();
    let bytes = b"return 7\n";
    let path = f.0.join("probe.lua");
    fs::write(&path, bytes).unwrap();
    let mut put = invocation(
        &[
            "live",
            "probe",
            "put",
            "--name",
            "reusable",
            "--file",
            path.to_str().unwrap(),
        ],
        &f.0,
    );
    lycheedev_core::workspace::initialize(&put.home).unwrap();
    lycheedev_live::probes::dispatch(&put).unwrap();
    put.command = "live probe show".into();
    put.positional = Some("reusable".into());
    let before = lycheedev_live::probes::show(&put, "reusable").unwrap().data;
    let inv = invocation(
        &[
            "live",
            "execute",
            "--session",
            "CON-original",
            "--request",
            "same-key",
            "--budget-seconds",
            "7",
            "--probe",
            "reusable",
        ],
        &f.0,
    );
    let calls = Rc::new(RefCell::new(vec![]));
    run(&inv, fake(&f.0, calls.clone()));
    assert_eq!(
        *calls.borrow(),
        [Call::Execute(
            "CON-original".into(),
            "same-key".into(),
            bytes.to_vec(),
            7,
            OperationPolicy::Opaque
        )]
    );
    assert_eq!(
        before,
        lycheedev_live::probes::show(&put, "reusable").unwrap().data
    );
}
#[test]
fn typed_verified_report_survives_pending_error_without_number_or_duplicate_loss() {
    let outcome = ServiceOutcome::ReportedWithFailure {
        result: project_result(),
        error: ChannelError::Pending { causes: vec![] }.into(),
    };
    let response = connection_response(Path::new("fixture"), outcome);
    assert_eq!(response.exit_code(), 6);
    for format in ["json", "jsonl"] {
        let text = rendered(&response, format);
        assert!(text.contains("\"same\":1,\"same\":2"), "{text}");
        assert!(text.contains("9007199254740993123456789"));
        assert!(text.contains("\"minus\":-0"));
        assert!(text.contains("\"scale\":1.2300e+04"));
        assert!(text.contains("\"html\":\"<>&\""));
        assert!(text.contains("\"resumeOperationId\":\"CON-original\""));
        assert!(text.contains("\"retryable\":true"));
        assert!(text.contains("\"stage\":\"result_verified\""));
        assert!(text.contains("\"reportState\":\"verified\""));
        if format == "jsonl" {
            let frames: Vec<Value> = text
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            assert_eq!(frames.len(), 2);
            assert_eq!(frames[1]["type"], "error");
        }
    }
    assert_eq!(
        rendered(&response, "text"),
        "live.channel_pending: live.channel_pending\n"
    );
}
#[test]
fn success_jsonl_ends_once_and_business_false_does_not_fail_transport() {
    let response = connection_response(
        Path::new("fixture"),
        ServiceOutcome::Completed(project_result()),
    );
    assert_eq!(response.exit_code(), 0);
    let text = rendered(&response, "jsonl");
    let frames: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        frames
            .iter()
            .map(|v| v["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["begin", "record", "end"]
    );
    assert_eq!(frames[2]["envelope"]["ok"], true);
    assert_eq!(frames[2]["envelope"]["result"]["report"]["ok"], false);
    let pretty = rendered(&response, "text");
    assert!(pretty.contains("\n  \"continuation\": {"));
    assert!(pretty.contains("\"same\": 1,\n    \"same\": 2"));
}
#[test]
fn trace_flush_precedes_cancel_and_pending_with_original_result() {
    let cause = AdmissionError::Joined(vec![
        ChannelError::Pending { causes: vec![] }.into(),
        ChannelError::Cancelled.into(),
        ChannelError::Contract(lycheedev_live::go_mailbox_v1::ContractError(
            "live.channel_trace_flush",
        ))
        .into(),
    ]);
    let response = connection_response(
        Path::new("fixture"),
        ServiceOutcome::ReportedWithFailure {
            result: project_result(),
            error: cause,
        },
    );
    assert_eq!(response.exit_code(), 5);
    assert_eq!(
        response.error.as_ref().unwrap().code,
        "live.channel_trace_flush"
    );
    assert!(response.result.is_some());
    let raw = rendered(&response, "json");
    assert!(raw.contains("\"retryable\":false"));
    assert!(raw.contains("live.channel_trace_flush"));
}
#[test]
fn ordered_invocation_cancellation_survives_wait_child() {
    let f = Fixture::new();
    let calls = Rc::new(RefCell::new(vec![]));
    let inv = invocation(
        &["live", "resume", "CON-original", "--wait-seconds", "1"],
        &f.0,
    );
    let mut services = fake(&f.0, calls);
    services.cancellation = Some(inv.control.tracked_cancel_handle().unwrap());
    run(&inv, services);
    assert_eq!(
        inv.control.stop_cause().unwrap(),
        Some(StopCause::Cancelled)
    );
}
#[test]
fn cancelled_main_never_opens_service_or_restarts_budget() {
    let f = Fixture::new();
    let inv = invocation(&["live", "resume", "CON-original"], &f.0);
    inv.control.tracked_cancel_handle().unwrap().cancel();
    let response =
        crate::dispatch_formal_entry_with(&inv, |_| panic!("cancelled command opened service"))
            .unwrap();
    assert_eq!(response.exit_code(), 7);
}
#[test]
fn doctor_retains_typed_connection_and_uses_protocol_context() {
    let f = Fixture::new();
    let inv = invocation(&["live", "doctor", "--session", "CON-original"], &f.0);
    let calls = Rc::new(RefCell::new(vec![]));
    let mut services = fake(&f.0, calls.clone());
    services.report.healthy = false;
    services.report.connection = Some(project_result());
    let response = run(&inv, services);
    assert_eq!(*calls.borrow(), [Call::Doctor("CON-original".into())]);
    assert_eq!(response.exit_code(), 3);
    assert_eq!(response.error.as_ref().unwrap().code, "doctor.check_failed");
    assert_eq!(response.context["protocol"], TRANSPORT);
    assert!(!response.context.contains_key("transport"));
    assert!(response.operation_id.is_empty());
    assert!(rendered(&response, "json").contains("\"same\":1,\"same\":2"));
}
#[test]
fn doctor_fix_preserves_result_on_error_and_does_not_reinspect_after_failure() {
    let f = Fixture::new();
    let inv = invocation(
        &["live", "doctor", "--session", "CON-original", "--fix"],
        &f.0,
    );
    let calls = Rc::new(RefCell::new(vec![]));
    let mut services = fake(&f.0, calls.clone());
    services.outcome = Some(ServiceOutcome::ReportedWithFailure {
        result: project_result(),
        error: ChannelError::Pending { causes: vec![] }.into(),
    });
    let response = run(&inv, services);
    assert_eq!(
        *calls.borrow(),
        [Call::Doctor("CON-original".into()), Call::Fix]
    );
    assert_eq!(response.exit_code(), 6);
    let value: Value = serde_json::from_str(&rendered(&response, "json")).unwrap();
    assert_eq!(value["result"]["checks"][0]["id"], "live.repair");
    assert_eq!(value["error"]["retryable"], false);
    assert_eq!(value["error"]["resumeOperationId"], "");
    assert_eq!(value["result"]["connection"]["session"], "CON-original");
}
#[test]
fn doctor_validation_precedes_project_io_and_top_level_unselected_is_quiet() {
    let f = Fixture::new();
    let inv = invocation(
        &["live", "doctor", "--session", "CON-original", "--pid", "17"],
        &f.0,
    );
    let response = dispatch_with::<Fake>(&inv, |_| panic!("invalid options opened project"));
    assert_eq!(
        response.error.unwrap().message,
        "use --session with its original --project, or select --installation/--pid"
    );
    let inv = invocation(&["doctor"], &f.0);
    assert!(doctor_checks(&inv).is_empty());
    assert!(!handles(&inv));
}
#[test]
fn finish_flush_failure_is_not_reported_as_success() {
    #[derive(Default)]
    struct Sink {
        bytes: Vec<u8>,
        flushes: usize,
    }
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Err(io::Error::other("fixture flush failed"))
        }
    }
    let response = connection_response(
        Path::new("fixture"),
        ServiceOutcome::Completed(project_result()),
    );
    let mut sink = Sink::default();
    let e = render(&mut sink, "json", &response).unwrap_err();
    assert_eq!(e.exit_code, 5);
    assert_eq!(sink.flushes, 1);
    assert!(!sink.bytes.is_empty());
}
#[test]
fn default_cli_advertises_only_the_connected_formal_routes() {
    let implemented = crate::contract::implemented();
    for route in [
        "live connect",
        "live status",
        "live session",
        "live resume",
        "live execute",
        "live bugs",
        "live disconnect",
        "live reload",
        "live reload fallback",
        "live doctor",
    ] {
        assert!(implemented.iter().any(|d| d.path == route), "{route}");
    }
    assert!(!implemented.iter().any(|d| d.path == "live report"));
    for route in ["live cancel", "live abandon"] {
        assert!(
            implemented.iter().any(|d| d.path == route),
            "historical disk-only retirement must have a real registered adapter"
        );
    }
}

/// An actual production adapter fixture: persisted history is established in
/// a private synthetic client tree, then the default NativeService answers it.
/// The impossible PID and nonexistent executable can never select a real game.
fn seed_completed_history(root: &Path) -> (String, PathBuf) {
    use lycheedev_live::go_mailbox_v1::{
        SCHEMA,
        admission::{ClientRecord, ConnectionBook, ConnectionTarget, WindowRecord},
        driver::TokenSource,
        journal::{StateJournal, save_state},
        observations::ChannelResult,
        state::{Operation, OperationStage, ResultBytes},
    };
    struct Tokens;
    impl TokenSource for Tokens {
        fn token(&mut self) -> ChannelResult<String> {
            Ok("3".repeat(32))
        }
    }
    let control = OperationControl::default();
    let client = root.join("fake-client");
    fs::create_dir_all(client.join("Interface/AddOns")).unwrap();
    let identity = Identity {
        schema: SCHEMA.into(),
        runtime: "2".repeat(32),
        owner: "0".repeat(32),
        fence: 0,
        next_slot: 1,
        slots: 500,
        input_state: SCHEMA.into(),
        character: "Fixture".into(),
        realm: "Realm".into(),
        guid: "Player-fixture".into(),
        build: "12.1.0.99999".into(),
        product: "retail".into(),
        release: VERSION.into(),
        ..Default::default()
    };
    let target = ConnectionTarget {
        client: ClientRecord {
            directory: client.display().to_string(),
            full_build: identity.build.clone(),
            product: identity.product.clone(),
            ..Default::default()
        },
        window: WindowRecord {
            process_id: u32::MAX,
            process_started_at: 19,
            handle: 23,
            executable: client.join("nonexistent.exe").display().to_string(),
            class: "fixture-not-wow".into(),
            ..Default::default()
        },
    };
    let mut book = ConnectionBook::open(root).unwrap();
    let created = book
        .create(target, identity, &SystemClock, &mut Tokens, &control)
        .unwrap();
    let id = created.state.id;
    let (mut record, mut journal) = book.acquire(&id, &control).unwrap();
    record.state.bound = true;
    let mut op = Operation::prepared(
        format!("LMO-{}", "4".repeat(32)),
        "5".repeat(32),
        "return 1".into(),
        5,
        OperationPolicy::Opaque,
    );
    op.request = "first".into();
    op.stage = OperationStage::Complete;
    op.attempt = 1;
    op.origin = Some(record.state.identity.clone());
    op.prepared_nonce = "6".repeat(32);
    op.challenge = "7".repeat(32);
    op.result_bytes =
        ResultBytes(br#"{ "ok":false,"large":9007199254740993,"x":1,"x":2 }"#.to_vec());
    op.report_bytes = op.result_bytes.0.len() as u32;
    op.report_checksum = lycheedev_live::go_mailbox_v1::checksum(&op.result_bytes.0);
    record.state.operation = Some(op);
    save_state(
        &mut journal,
        &control,
        &mut record.state,
        "fixture_first_complete",
    )
    .unwrap();
    record.state.operation.as_mut().unwrap().request = "second".into();
    record.state.operation.as_mut().unwrap().id = format!("LMO-{}", "8".repeat(32));
    save_state(
        &mut journal,
        &control,
        &mut record.state,
        "fixture_later_complete",
    )
    .unwrap();
    journal.close().unwrap();
    (id, record.paths.log_path().into())
}
#[test]
fn actual_default_dispatch_reads_completed_history_before_any_native_open() {
    let f = Fixture::new();
    let (id, log) = seed_completed_history(&f.0);
    let before = fs::read(&log).unwrap();
    fs::write(f.0.join("same.lua"), b"return 1").unwrap();
    let inv = invocation(
        &[
            "live",
            "execute",
            "--session",
            &id,
            "--request",
            "first",
            "--file",
            "same.lua",
            "--budget-seconds",
            "5",
        ],
        &f.0,
    );
    let response = crate::dispatch_formal_entry_with(&inv, dispatch).unwrap();
    assert!(response.error.is_none(), "{:?}", response.error);
    let Some(LiveResult::Connection(result)) = &response.result else {
        panic!("typed connection result missing")
    };
    assert_eq!(result.operation, format!("LMO-{}", "4".repeat(32)));
    assert_eq!(result.cleanup, "complete");
    assert!(result.complete);
    assert_eq!(before, fs::read(&log).unwrap());
    assert!(rendered(&response, "json").contains("\"x\":1,\"x\":2"));
    assert!(!inv.home.exists());
    let inv = invocation(&["live", "status", &id], &f.0);
    let response = crate::dispatch_formal_entry_with(&inv, dispatch).unwrap();
    let Some(LiveResult::Connection(current)) = response.result else {
        panic!("current result missing")
    };
    assert_eq!(current.operation, format!("LMO-{}", "8".repeat(32)));
    assert_eq!(before, fs::read(log).unwrap());
}
#[test]
fn actual_default_dispatch_rejects_history_conflicts_without_native_or_new_journal() {
    let f = Fixture::new();
    let (id, log) = seed_completed_history(&f.0);
    let before = fs::read(&log).unwrap();
    fs::write(f.0.join("different.lua"), b"return 2").unwrap();
    let inv = invocation(
        &[
            "live",
            "execute",
            "--session",
            &id,
            "--request",
            "first",
            "--file",
            "different.lua",
            "--budget-seconds",
            "5",
        ],
        &f.0,
    );
    let response = crate::dispatch_formal_entry_with(&inv, dispatch).unwrap();
    assert_eq!(response.exit_code(), 2);
    assert_eq!(
        response.error.as_ref().unwrap().code,
        "live.channel_request_conflict"
    );
    assert_eq!(before, fs::read(log).unwrap());
    assert!(!inv.home.exists());
}
#[test]
fn project_open_error_precedes_route_specific_required_arguments() {
    let f = Fixture::new();
    let inv = invocation(&["live", "execute", "--session", "CON-original"], &f.0);
    let response = dispatch_with::<Fake>(&inv, |_| {
        Err(ToolkitError::arguments("fixture project unavailable"))
    });
    assert_eq!(
        response.error.as_ref().unwrap().message,
        "fixture project unavailable"
    );
    assert!(response.context.is_empty());
}
#[test]
fn wait_child_cannot_extend_original_deadline_or_lose_ancestor_cancel() {
    let f = Fixture::new();
    let mut inv = invocation(
        &["live", "resume", "CON-original", "--wait-seconds", "600"],
        &f.0,
    );
    let cancellation = TrackedCancellation::new();
    inv.control = OperationControl::tracked(
        Instant::now() + Duration::from_secs(2),
        cancellation.clone(),
    );
    let child = wait_control(&inv, 120).unwrap();
    assert_eq!(child.deadline(), inv.control.deadline());
    cancellation.cancel();
    assert_eq!(child.stop_cause().unwrap(), Some(StopCause::Cancelled));
    assert_eq!(
        inv.control.stop_cause().unwrap(),
        Some(StopCause::Cancelled)
    );
}
#[test]
fn deadline_without_pending_wrapper_uses_go_live_deadline_classification() {
    let response = connection_response(
        Path::new("fixture"),
        ServiceOutcome::Failed(ChannelError::DeadlineExceeded.into()),
    );
    assert_eq!(response.exit_code(), 6);
    assert_eq!(
        response.error.as_ref().unwrap().code,
        "live.deadline_pending"
    );
    let value: Value = serde_json::from_str(&rendered(&response, "json")).unwrap();
    assert_eq!(value["error"]["stage"], "dispatch");
    assert_eq!(value["error"]["retryable"], true);
}
#[test]
fn no_cache_compatibility_does_not_change_formal_resume_action() {
    let f = Fixture::new();
    let calls = Rc::new(RefCell::new(vec![]));
    for words in [
        vec!["live", "connect", "--session", "CON-original"],
        vec!["live", "connect", "--no-cache", "--session", "CON-original"],
    ] {
        let inv = invocation(&words, &f.0);
        run(&inv, fake(&f.0, calls.clone()));
    }
    assert_eq!(
        *calls.borrow(),
        [
            Call::Resume("CON-original".into()),
            Call::Resume("CON-original".into())
        ]
    );
    for words in [
        vec!["--no-cache", "live", "connect", "--session", "CON-original"],
        vec!["live", "status", "CON-original", "--no-cache"],
        vec!["live", "connect", "--no-cache=false"],
    ] {
        assert!(crate::parse(&words.iter().map(|s| s.to_string()).collect::<Vec<_>>()).is_err());
    }
}
#[test]
fn file_admission_failures_keep_empty_context_and_do_not_invoke_backend() {
    let f = Fixture::new();
    fs::write(f.0.join("empty.lua"), b"").unwrap();
    fs::create_dir(f.0.join("directory.lua")).unwrap();
    for name in ["empty.lua", "directory.lua"] {
        let inv = invocation(
            &[
                "live",
                "execute",
                "--session",
                "CON-original",
                "--request",
                "key",
                "--file",
                name,
                "--budget-seconds",
                "1",
            ],
            &f.0,
        );
        let calls = Rc::new(RefCell::new(vec![]));
        let response = run(&inv, fake(&f.0, calls.clone()));
        assert_eq!(response.exit_code(), 2);
        assert!(response.context.is_empty());
        assert!(calls.borrow().is_empty());
    }
}
#[test]
fn live_parser_structural_errors_precede_admitted_value_checks() {
    for (words, kind, token) in [
        (
            vec!["live", "execute", "--budget-seconds", "0", "--pid", "0"],
            "UnknownArgument", "--pid",
        ),
        (
            vec!["live", "connect", "--pid", "0", "--pid", "1"],
            "ArgumentConflict", "--pid",
        ),
        (
            vec!["live", "connect", "--pid", "1", "--pid", "0"],
            "ArgumentConflict", "--pid",
        ),
        (
            vec!["live", "execute", "--budget-seconds", "1", "--pid", "0"],
            "UnknownArgument", "--pid",
        ),
        (
            vec!["live", "execute", "--budget-seconds", "0", "--unknown"],
            "UnknownArgument", "--unknown",
        ),
    ] {
        let error =
            crate::parse(&words.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap_err();
        assert_clap_error(&error, kind, token);
    }
    let error = crate::parse(&["live", "connect", "--wait-seconds", "0", "--pid", "+1"].map(str::to_owned)).unwrap_err();
    assert_eq!(error.message, "--wait-seconds must be between 1 and 600");
}
#[test]
fn pid_uses_go_parse_uint_lexical_boundary() {
    for pid in ["+1", "0", "4294967296", "1_0", "１"] {
        let error =
            crate::parse(&["live", "connect", "--pid", pid].map(str::to_owned)).unwrap_err();
        assert_eq!(error.message, "invalid --pid", "{pid}");
    }
    // An inline negative is a value; a separate negative is a clap option token.
    assert_eq!(crate::parse(&["live", "connect", "--pid=-1"].map(str::to_owned)).unwrap_err().message, "invalid --pid");
    let error = crate::parse(&["live", "connect", "--pid", "-1"].map(str::to_owned)).unwrap_err();
    assert_clap_error(&error, "UnknownArgument", "-1");
    for pid in ["1", "0001", "4294967295"] {
        assert!(crate::parse(&["live", "connect", "--pid", pid].map(str::to_owned)).is_ok());
    }
    assert!(crate::parse(&["live", "connect", "--wait-seconds", "+1"].map(str::to_owned)).is_ok());
}
#[test]
fn probe_read_mid_operation_cancellation_remains_typed_after_checked_close() {
    let f = Fixture::new();
    let inv = invocation(
        &[
            "live",
            "execute",
            "--session",
            "CON-original",
            "--request",
            "key",
            "--file",
            "probe.lua",
            "--budget-seconds",
            "1",
        ],
        &f.0,
    );
    fs::write(f.0.join("probe.lua"), b"return 1").unwrap();
    let signal = inv.control.tracked_cancel_handle().unwrap();
    let closes = std::cell::Cell::new(0);
    let error = read_probe_file_with(
        &inv,
        Path::new("probe.lua"),
        |file| {
            let mut bytes = vec![];
            file.read_to_end(&mut bytes)?;
            signal.cancel();
            Ok(bytes)
        },
        |file, result| {
            closes.set(closes.get() + 1);
            lycheedev_core::storage::finish_file(file, result, &inv.control)
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.exit_code, 7);
    assert_eq!(closes.get(), 1);
    assert!(error.details.is_null());
}
#[test]
fn probe_checked_close_and_control_causes_survive_public_error_projection() {
    let f = Fixture::new();
    let inv = invocation(
        &[
            "live",
            "execute",
            "--session",
            "CON-original",
            "--request",
            "key",
            "--file",
            "probe.lua",
            "--budget-seconds",
            "1",
        ],
        &f.0,
    );
    fs::write(f.0.join("probe.lua"), b"return 1").unwrap();
    let signal = inv.control.tracked_cancel_handle().unwrap();
    let closes = std::cell::Cell::new(0);
    let error=read_probe_file_with(&inv,Path::new("probe.lua"),|_| {signal.cancel();Ok(b"return 1".to_vec())},|file,result| {
        closes.set(closes.get()+1);let original=lycheedev_core::storage::finish_file(file,result,&inv.control).unwrap_err();
        // Model an independent checked-close failure after consuming the real
        // fixture handle. No raw handle misuse or native failure is induced.
        let mut combined=original.clone();combined.details=json!({"schema":lycheedev_core::storage::FILE_CLEANUP_SCHEMA,"primary":original,"cleanup":ToolkitError::io(io::Error::other("modeled close failure")),"control":null});Err(combined)
    }).unwrap_err();
    let original = error.details.clone();
    let response = Response::failure(crate::command_fault(&inv, error));
    assert_eq!(closes.get(), 1);
    assert_eq!(response.exit_code(), 7);
    let value: Value = serde_json::from_str(&rendered(&response, "json")).unwrap();
    assert_eq!(value["error"]["details"], original);
    assert_eq!(value["error"]["code"], "command.cancelled");
}
#[test]
fn main_file_preparation_does_not_turn_cancellation_into_arguments_or_invoke_service() {
    let f = Fixture::new();
    fs::write(f.0.join("probe.lua"), b"return 1").unwrap();
    let inv = invocation(
        &[
            "live",
            "execute",
            "--session",
            "CON-original",
            "--request",
            "key",
            "--file",
            "probe.lua",
            "--budget-seconds",
            "1",
        ],
        &f.0,
    );
    let calls = Rc::new(RefCell::new(vec![]));
    let signal = inv.control.tracked_cancel_handle().unwrap();
    let response = crate::dispatch_formal_entry_with(&inv, |inv| {
        dispatch_with(inv, |_| {
            signal.cancel();
            Ok(fake(&f.0, calls.clone()))
        })
    })
    .unwrap();
    assert_eq!(response.exit_code(), 7);
    assert_eq!(response.error.unwrap().code, "command.cancelled");
    assert!(calls.borrow().is_empty());
}
#[test]
fn doctor_fix_without_target_projects_check_failed_at_query_stage() {
    let f = Fixture::new();
    let inv = invocation(&["live", "doctor", "--fix"], &f.0);
    let calls = Rc::new(RefCell::new(vec![]));
    let mut services = fake(&f.0, calls);
    services.outcome = Some(ServiceOutcome::Failed(
        ChannelError::from(ToolkitError::new(
            "doctor.check_failed",
            "doctor.check_failed",
            3,
        ))
        .into(),
    ));
    let response = run(&inv, services);
    let value: Value = serde_json::from_str(&rendered(&response, "json")).unwrap();
    assert_eq!(value["error"]["code"], "doctor.check_failed");
    assert_eq!(value["error"]["stage"], "query");
    assert_eq!(response.exit_code(), 3);
}
#[test]
fn top_level_doctor_omits_empty_next_step_and_retains_nonempty_guidance() {
    let mut report = DoctorResult::default();
    report.add("live.target", "ok", "selected", "");
    report.add(
        "live.input",
        "blocked",
        "not ready",
        "wait for fresh sample",
    );
    let checks = project_doctor_checks(report);
    assert!(checks[0].get("nextStep").is_none());
    assert_eq!(checks[0]["ok"], true);
    assert_eq!(checks[1]["status"], "error");
    assert_eq!(checks[1]["nextStep"], "wait for fresh sample");
}
#[test]
fn probe_read_close_and_control_failure_preserves_original_typed_causes() {
    let fixture = Fixture::new();
    let inv = invocation(&["live", "connect"], &fixture.0);
    fs::write(fixture.0.join("probe.lua"), b"return 1").unwrap();
    let signal = inv.control.tracked_cancel_handle().unwrap();
    let error = read_probe_file_with(
        &inv,
        Path::new("probe.lua"),
        |_| {
            signal.cancel();
            Err(ToolkitError::io(io::Error::other("modeled read failure")))
        },
        |file, result| {
            let joined = lycheedev_core::storage::finish_file(file, result, &inv.control)
                .unwrap_err();
            let mut error = joined.clone();
            error.details = json!({"schema":lycheedev_core::storage::FILE_CLEANUP_SCHEMA,"primary":joined,"cleanup":ToolkitError::io(io::Error::other("modeled close failure")),"control":null});
            Err(error)
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "external.io");
    assert_eq!(
        error.details["primary"]["details"]["primary"]["code"],
        "external.io"
    );
    assert_eq!(
        error.details["primary"]["details"]["control"]["code"],
        "command.cancelled"
    );
    assert_eq!(error.details["cleanup"]["message"], "modeled close failure");
    let details = error.details.clone();
    let value: Value = serde_json::from_str(&rendered(
        &Response::failure(crate::command_fault(&inv, error)),
        "json",
    ))
    .unwrap();
    assert_eq!(value["error"]["details"], details);
}

#[test]
fn legacy_resume_is_retired_before_workspace_or_native_factory() {
    let f = Fixture::new();
    for id in ["OP-unknown", "BTP-unknown", "SESSION-old", "unknown"] {
        let inv = invocation(&["live", "resume", id], &f.0);
        let response =
            dispatch_with::<Fake>(&inv, |_| panic!("retired recovery opened native services"));
        let error = response.error.unwrap();
        assert_eq!(error.exit_code, 2);
        assert_eq!(error.code, "command.invalid_arguments");
        assert_eq!(
            error.message,
            "legacy OP/BTP execution is retired; use live status or live abandon; Native recovery requires CON"
        );
        assert!(response.result.is_none());
        assert!(response.operation_id.is_empty());
        assert!(!inv.home.exists());
    }
}

#[test]
fn legacy_bootstrap_status_reads_workspace_without_native_factory() {
    let f = Fixture::new();
    let inv = invocation(&["live", "status", "BTP-old"], &f.0);
    lycheedev_core::workspace::initialize(&inv.home).unwrap();
    let mut metadata = lycheedev_core::metadata::Metadata::open(&inv.home, true).unwrap();
    metadata
        .create(
            "bootstrap/attempt/BTP-old",
            &json!({
                "schema":"lycheedev.bootstrap-receiver.v1", "id":"BTP-old", "phase":"unknown",
                "receiver":{"phase":"commit_queued", "attempt":1}
            }),
        )
        .unwrap();
    drop(metadata);
    let response =
        dispatch_with::<Fake>(&inv, |_| panic!("historical status opened native services"));
    assert_eq!(response.exit_code(), 0);
    assert_eq!(response.operation_id, "BTP-old");
    assert!(!response.context.contains_key("transport"));
    let mut bytes = vec![];
    render(&mut bytes, "json", &response).unwrap();
    let envelope: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(envelope["result"]["phase"], "unknown");
    assert_eq!(envelope["result"]["receiver"]["phase"], "commit_queued");
    assert!(!f.0.join(".lycheedev").exists());
}
#[test]
fn legacy_bootstrap_status_missing_keeps_id_and_no_result() {
    let f = Fixture::new();
    let inv = invocation(&["live", "status", "BTP-missing"], &f.0);
    lycheedev_core::workspace::initialize(&inv.home).unwrap();
    let response =
        dispatch_with::<Fake>(&inv, |_| panic!("historical status opened native services"));
    assert_eq!(response.operation_id, "BTP-missing");
    assert!(response.result.is_none());
    let error = response.error.unwrap();
    assert_eq!(error.code, "vault.record_missing");
    assert_eq!(error.message, "vault.record_missing");
    assert_eq!(error.exit_code, 2);
}

#[test]
fn legacy_bootstrap_unmarshalable_time_fails_only_at_render_without_partial_frame() {
    let f = Fixture::new();
    let inv = invocation(&["live", "status", "BTP-old"], &f.0);
    lycheedev_core::workspace::initialize(&inv.home).unwrap();
    let mut metadata = lycheedev_core::metadata::Metadata::open(&inv.home, true).unwrap();
    metadata
        .create(
            "bootstrap/attempt/BTP-old",
            &json!({
                "schema":"lycheedev.bootstrap-receiver.v1", "id":"BTP-old",
                "createdAt":"2026-10-15T02:00:00+24:00"
            }),
        )
        .unwrap();
    drop(metadata);
    let response =
        dispatch_with::<Fake>(&inv, |_| panic!("historical read opened native services"));
    assert_eq!(response.exit_code(), 0);
    for format in ["json", "jsonl", "text"] {
        let mut bytes = vec![];
        let error = render(&mut bytes, format, &response).unwrap_err();
        assert_eq!(
            error.message,
            "json: error calling MarshalJSON for type time.Time: Time.MarshalJSON: timezone hour outside of range [0,23]"
        );
        if format == "jsonl" {
            let begin: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(begin["type"], "begin");
        } else {
            assert!(bytes.is_empty());
        }
    }
}

#[path = "formal_live_history_tests.rs"]
mod history;

#[path = "formal_live_retire_tests.rs"]
mod retirement;

// The pre-enum CLI adapter is kept only as a test oracle. The production
// service cannot construct its old (None, None) state or bypass enum matching.
fn legacy_connection_response(
    project: &Path,
    result: Option<ProjectResult>,
    error: Option<AdmissionError>,
) -> Response {
    let mut response = Response::default();
    response.context.insert(
        "project".into(),
        json!(lycheedev_core::client::display_path(project)),
    );
    response
        .context
        .insert("transport".into(), json!(TRANSPORT));
    if let Some(result) = result.filter(|r| !r.session.is_empty()) {
        response.operation_id = result.session.clone();
        response
            .context
            .insert("session".into(), json!(result.session));
        response.context.insert("stage".into(), json!(result.stage));
        if !result.operation.is_empty() {
            response
                .context
                .insert("operation".into(), json!(result.operation));
        }
        response.result = Some(LiveResult::Connection(result));
    }
    response.error = error.as_ref().map(public_error);
    merge_context(&mut response);
    response
}

fn assert_legacy_outcome_wire(outcome: ServiceOutcome) {
    let (result, error) = match &outcome {
        ServiceOutcome::Completed(result) => (Some(result.clone()), None),
        ServiceOutcome::Failed(error) => (None, Some(error.clone())),
        ServiceOutcome::ReportedWithFailure { result, error } => {
            (Some(result.clone()), Some(error.clone()))
        }
    };
    let expected = legacy_connection_response(Path::new("fixture"), result, error);
    let actual = connection_response(Path::new("fixture"), outcome);
    assert_eq!(actual.exit_code(), expected.exit_code());
    for format in ["json", "jsonl", "text"] {
        assert_eq!(
            rendered(&actual, format),
            rendered(&expected, format),
            "{format}"
        );
    }
}

#[test]
fn service_outcome_variants_preserve_exact_legacy_wire_and_exit() {
    assert_legacy_outcome_wire(ServiceOutcome::Completed(project_result()));
    assert_legacy_outcome_wire(ServiceOutcome::Failed(
        ChannelError::DeadlineExceeded.into(),
    ));
    assert_legacy_outcome_wire(ServiceOutcome::ReportedWithFailure {
        result: project_result(),
        error: ChannelError::Pending { causes: vec![] }.into(),
    });
}

#[test]
fn service_outcome_wire_preserves_primary_and_cleanup_error_priority() {
    for primary in [
        ChannelError::Cancelled,
        ChannelError::Pending { causes: vec![] },
        ChannelError::DeadlineExceeded,
    ] {
        let error = AdmissionError::Joined(vec![
            primary.into(),
            ChannelError::Contract(lycheedev_live::go_mailbox_v1::ContractError(
                "live.channel_trace_flush",
            ))
            .into(),
        ]);
        assert_legacy_outcome_wire(ServiceOutcome::ReportedWithFailure {
            result: project_result(),
            error: error.clone(),
        });
        assert_legacy_outcome_wire(ServiceOutcome::Failed(error));
    }
}

#[test]
fn service_outcome_empty_session_stays_filtered_for_success_and_failure() {
    for failed in [false, true] {
        let mut result = project_result();
        result.session.clear();
        let outcome = if failed {
            ServiceOutcome::ReportedWithFailure {
                result,
                error: ChannelError::Cancelled.into(),
            }
        } else {
            ServiceOutcome::Completed(result)
        };
        assert_legacy_outcome_wire(outcome);
    }
}

#[test]
fn doctor_failed_without_result_preserves_prior_report_and_skips_reinspection() {
    let f = Fixture::new();
    let inv = invocation(
        &["live", "doctor", "--session", "CON-original", "--fix"],
        &f.0,
    );
    let calls = Rc::new(RefCell::new(vec![]));
    let mut services = fake(&f.0, calls.clone());
    services.report.connection = Some(project_result());
    services.outcome = Some(ServiceOutcome::Failed(ChannelError::Cancelled.into()));
    let response = run(&inv, services);
    assert_eq!(
        *calls.borrow(),
        [Call::Doctor("CON-original".into()), Call::Fix]
    );
    assert_eq!(response.exit_code(), 7);
    assert!(response.operation_id.is_empty());
    let Some(LiveResult::Doctor(report)) = response.result else {
        panic!("doctor report lost");
    };
    assert!(!report.healthy);
    assert_eq!(report.connection.unwrap().session, "CON-original");
    assert_eq!(
        report.checks.last().unwrap().next_step,
        "resolve the repair error, then run live doctor --fix for the same target"
    );
}

#[test]
fn doctor_completed_reinspects_and_retains_original_repair_projection() {
    let f = Fixture::new();
    let inv = invocation(
        &["live", "doctor", "--session", "CON-original", "--fix"],
        &f.0,
    );
    let calls = Rc::new(RefCell::new(vec![]));
    let mut services = fake(&f.0, calls.clone());
    services.report.healthy = true;
    services.outcome = Some(ServiceOutcome::Completed(project_result()));
    let response = run(&inv, services);
    assert_eq!(
        *calls.borrow(),
        [
            Call::Doctor("CON-original".into()),
            Call::Fix,
            Call::Doctor("CON-original".into())
        ]
    );
    assert_eq!(response.exit_code(), 0);
    let Some(LiveResult::Doctor(report)) = response.result else {
        panic!("doctor report lost");
    };
    assert_eq!(report.connection.unwrap().stage, "result_verified");
}

#[test]
fn doctor_empty_session_projection_does_not_replace_previous_connection() {
    for failed in [false, true] {
        let f = Fixture::new();
        let inv = invocation(
            &["live", "doctor", "--session", "CON-original", "--fix"],
            &f.0,
        );
        let calls = Rc::new(RefCell::new(vec![]));
        let mut services = fake(&f.0, calls.clone());
        services.report.connection = Some(project_result());
        let mut result = project_result();
        result.session.clear();
        services.outcome = Some(if failed {
            ServiceOutcome::ReportedWithFailure {
                result,
                error: ChannelError::Cancelled.into(),
            }
        } else {
            ServiceOutcome::Completed(result)
        });
        let response = run(&inv, services);
        assert!(response.operation_id.is_empty());
        assert_eq!(response.exit_code(), if failed { 7 } else { 0 });
        let expected = if failed {
            vec![Call::Doctor("CON-original".into()), Call::Fix]
        } else {
            vec![
                Call::Doctor("CON-original".into()),
                Call::Fix,
                Call::Doctor(String::new()),
            ]
        };
        assert_eq!(*calls.borrow(), expected);
        let Some(LiveResult::Doctor(report)) = response.result else {
            panic!("doctor report lost");
        };
        assert_eq!(report.connection.unwrap().session, "CON-original");
    }
}
