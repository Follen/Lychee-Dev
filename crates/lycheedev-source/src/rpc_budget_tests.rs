//! Actual pipe consumer resource fixtures; no LuaLS source-quality claim.
use super::*;

const PEER: &str = r#"
const fs=require('fs'),mode=process.argv[1],log=process.argv[2];
let input=Buffer.alloc(0),requests=0,barriers=0;
const frame=v=>{const b=Buffer.from(JSON.stringify(v));return Buffer.concat([Buffer.from('Content-Length: '+b.length+'\r\n\r\n'),b]);};
process.stdin.on('data',chunk=>{
 input=Buffer.concat([input,chunk]);
 while(true){
  const end=input.indexOf('\r\n\r\n');if(end<0)return;
  const n=Number(/Content-Length: (\d+)/i.exec(input.subarray(0,end).toString())[1]);
  if(input.length<end+4+n)return;
  const msg=JSON.parse(input.subarray(end+4,end+4+n));input=input.subarray(end+4+n);
  if(msg.id===undefined)continue;
  const barrier=msg.method==='textDocument/definition'&&msg.params.position.line===0&&msg.params.position.character===0;
  if(barrier)barriers++;else requests++;
  fs.writeFileSync(log,JSON.stringify({requests,barriers}));
  if(barrier){process.stdout.write(frame({id:msg.id,result:null}));continue;}
  const range={start:{line:0,character:0},end:{line:0,character:1}};
  const count=mode==='large'?1000:1;
  const result=Array.from({length:count},()=>({uri:msg.params.textDocument.uri,range}));
  process.stdout.write(frame({id:msg.id,result}));
 }
});
"#;

fn fresh_case(prefix: &str, mode: &str) -> PathBuf {
    let owner = lycheedev_windows::Process::open(std::process::id()).unwrap();
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp");
    fs::create_dir_all(&base).unwrap();
    let created = owner.identity().created.to_string();
    let root = base.join(format!(
        "{prefix}-{}-{}-{}-{mode}",
        owner.identity().pid,
        created,
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    root.canonicalize().unwrap()
}

fn fixture(mode: &str) -> (WorkspaceSession, PathBuf, lycheedev_windows::Process) {
    let root = fresh_case("rust-lsp-budget", mode);
    let control = lycheedev_core::control::OperationControl::new(
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(false)),
    );
    let files = BTreeMap::from([("Fixture.lua".into(), b"local x = 1\n".to_vec())]);
    let runtime = Runtime {
        root: root.clone(),
        inventory: BTreeMap::new(),
    };
    let frozen = runtime.freeze(&root, &files, b"").unwrap();
    let held_inputs = frozen.lock_inputs(&files, b"", &control).unwrap();
    let log = root.join("requests.json");
    fs::write(&log, b"{\"requests\":0}").unwrap();
    let mut command =
        Command::new(std::env::var_os("LYCHEEDEV_TEST_NODE").unwrap_or_else(|| "node".into()));
    command
        .args(["-e", PEER, mode])
        .arg(&log)
        .current_dir(&root);
    let rpc = Rpc::start(command, control).unwrap();
    let process = lycheedev_windows::Process::open(rpc.child.id()).unwrap();
    let session = WorkspaceSession {
        rpc: Some(rpc),
        frozen: Some(frozen),
        held_runtime: Vec::new(),
        held_inputs,
        files: files
            .iter()
            .map(|(path, bytes)| (path.clone(), bytes.len()))
            .collect(),
        capabilities: lsp_frame::Capabilities {
            definition: true,
            references: true,
            hover: false,
        },
        watches: Vec::new(),
        mutation: Arc::new(AtomicBool::new(false)),
        coverage: json!({"luaFiles":1,"luaBytes":12,"excludedFiles":0}),
        unavailable: Default::default(),
        opened: Default::default(),
        lease: None,
        batches: 0,
        life_deadline: Instant::now() + Duration::from_secs(30),
    };
    (session, log, process)
}

fn query() -> Value {
    json!({"kind":"references","path":"Fixture.lua","position":{"line":0,"character":6}})
}

fn requests(log: &Path) -> u64 {
    serde_json::from_slice::<Value>(&fs::read(log).unwrap()).unwrap()["requests"]
        .as_u64()
        .unwrap()
}
fn barriers(log: &Path) -> u64 {
    serde_json::from_slice::<Value>(&fs::read(log).unwrap()).unwrap()["barriers"]
        .as_u64()
        .unwrap_or(0)
}

fn reaped(session: &WorkspaceSession, process: &lycheedev_windows::Process) {
    assert!(session.rpc.is_none());
    assert!(session.frozen.is_none());
    assert!(session.held_inputs.is_empty());
    assert!(session.lease.is_none());
    assert_eq!(
        process
            .wait_exit(
                Instant::now() + Duration::from_secs(2),
                &AtomicBool::new(false)
            )
            .unwrap(),
        7
    );
}

#[test]
fn all_128_semantic_queries_execute_and_two_batches_share_exact_worker() {
    let (mut session, log, process) = fixture("small");
    let queries = vec![query(); 128];
    let identity = process.identity().clone();
    let job = session
        .rpc
        .as_ref()
        .unwrap()
        .job
        .memory_accounting()
        .unwrap();
    assert_eq!(job.job_memory_limit, LUALS_COMMIT_BUDGET as u64);
    assert_eq!(job.active_processes, 1);
    assert!(job.aggregate_commit.is_none());
    let members = session.rpc.as_ref().unwrap().job.membership().unwrap();
    assert_eq!(members.assigned_processes, 1);
    assert_eq!(members.member_pids, vec![identity.pid]);
    let control = Default::default();
    let mut admission = ResultAdmission::new();
    for batch in 1..=2 {
        let result = session
            .analyze_typed(&queries, &control, &mut admission)
            .unwrap();
        assert_eq!(result.state, "complete");
        assert_eq!(result.results.len(), 128);
        assert!(
            result
                .results
                .iter()
                .all(|r| r.state == "complete" && r.locations.as_ref().unwrap().len() == 1)
        );
        assert_eq!(requests(&log), (batch * 128) as u64);
        assert_eq!(barriers(&log), batch as u64);
        assert_eq!(session.pid(), Some(identity.pid));
        assert_eq!(session.batches(), batch);
    }
    session.close();
    reaped(&session, &process);
}

#[test]
fn query_129_refuses_before_any_request_and_reaps_worker() {
    let (mut session, log, process) = fixture("small");
    let error = match session.analyze_typed(
        &vec![query(); 129],
        &Default::default(),
        &mut ResultAdmission::new(),
    ) {
        Ok(_) => panic!("129-query batch was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.code, "luals.input_budget");
    assert_eq!(requests(&log), 0);
    assert_eq!(barriers(&log), 0);
    reaped(&session, &process);
}

#[test]
fn accumulated_results_refuse_across_two_batches_and_release_workspace() {
    let (mut session, log, process) = fixture("large");
    let mut admission = ResultAdmission::new();
    let queries = vec![query(); 64];
    let first = session
        .analyze_typed(&queries, &Default::default(), &mut admission)
        .unwrap();
    assert_eq!(first.results.len(), 64);
    assert_eq!(requests(&log), 64);
    assert_eq!(barriers(&log), 1);
    // Keep first results alive, as the production broker does while appending
    // batch two. The shared admission must fail before output exceeds 8 MiB.
    let error = match session.analyze_typed(&queries, &Default::default(), &mut admission) {
        Ok(_) => panic!("combined semantic result budget was bypassed"),
        Err(error) => error,
    };
    assert_eq!(error.code, "luals.input_budget");
    assert!(
        error
            .message
            .contains("Accumulated semantic result byte budget")
    );
    assert!((65..=128).contains(&requests(&log)));
    assert_eq!(barriers(&log), 2);
    assert_eq!(first.results.len(), 64);
    reaped(&session, &process);
}

#[test]
fn descriptor_admission_precedes_cloning_and_second_rpc_request() {
    let (mut session, log, process) = fixture("small");
    let mut large = query();
    large["unusedExtension"] = json!("x".repeat(5 << 20));
    let error = match session.analyze_typed(
        &[large.clone(), large],
        &Default::default(),
        &mut ResultAdmission::new(),
    ) {
        Ok(_) => panic!("combined descriptor allocation budget was bypassed"),
        Err(error) => error,
    };
    assert_eq!(error.code, "luals.input_budget");
    assert!(
        error
            .message
            .contains("Accumulated query descriptor allocation budget")
    );
    assert_eq!(requests(&log), 1);
    assert_eq!(barriers(&log), 1);
    reaped(&session, &process);
}

#[test]
fn actual_os_thread_rejection_reaps_each_partial_start() {
    for stage in 0..3 {
        let root = fresh_case("rust-lsp-thread-admission", &stage.to_string());
        let mut command =
            Command::new(std::env::var_os("LYCHEEDEV_TEST_NODE").unwrap_or_else(|| "node".into()));
        command
            .args(["-e", "setInterval(()=>{},1000);"])
            .current_dir(&root);
        let builders = std::array::from_fn(|index| {
            let builder = std::thread::Builder::new();
            if index == stage {
                builder.stack_size(usize::MAX)
            } else {
                builder
            }
        });
        let mut held = None;
        let control = lycheedev_core::control::OperationControl::default();
        let begun = Instant::now();
        let result = Rpc::start_with_threads(command, control.clone(), builders, |child| {
            held = Some(lycheedev_windows::Process::open(child.id()).unwrap());
            Ok(())
        });
        let error = match result {
            Ok(_) => panic!("OS admitted an impossible stack reservation"),
            Err(error) => error,
        };
        assert_eq!(error.code, "luals.input_budget");
        assert_eq!(error.exit_code, 3);
        assert_eq!(
            error.details["resource"],
            ["lspWriterThread", "lspReaderThread", "lspStderrThread"][stage]
        );
        assert!(error.details["osCode"].is_number());
        assert!(!control.token().load(Ordering::Acquire));
        assert!(
            begun.elapsed() < Duration::from_secs(2),
            "partial-start pipe threads did not join promptly"
        );
        let held = held.unwrap();
        let exit = held
            .wait_exit(
                Instant::now() + Duration::from_secs(1),
                &AtomicBool::new(false),
            )
            .unwrap();
        let report = serde_json::to_vec_pretty(&json!({
                "schema":"lycheedev.source-rpc-thread-admission.v1",
                "stage":stage,"errorCode":error.code,"exitCode":error.exit_code,
                "resource":error.details["resource"],"osCode":error.details["osCode"],
                "originalChild":{"pid":held.identity().pid,"created":held.identity().created.to_string()},
                "kernelExitCode":exit,"startupAndCleanupElapsedMs":begun.elapsed().as_millis(),
                "scope":"actual OS Builder admission rejection; cleanup owner dropped and admitted pipe threads joined before refusal returned; original held child handle kernel exit, not a reopened PID"
            })).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("admission-report.json"))
            .unwrap()
            .write_all(&report)
            .unwrap();
        assert_eq!(exit, 7);
    }
}
