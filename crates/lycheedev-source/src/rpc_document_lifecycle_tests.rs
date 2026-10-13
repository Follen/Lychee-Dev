//! Wire-lifecycle regressions at the real WorkspaceSession/Rpc batch seam.
//! The peer models a delayed scope, not actual LuaLS semantics or OOM causality.
use super::*;

const PEER: &str = r#"
const fs=require('fs'),mode=process.argv[1],log=process.argv[2];
let input=Buffer.alloc(0),events=[],active=new Set(),ready=false,pending=false;
const snapshot=()=>({events,active:[...active].sort(),ready,pending});
const save=()=>fs.writeFileSync(log,JSON.stringify(snapshot()));
const send=(id,result,error)=>{const raw=Buffer.from(JSON.stringify(error?{id,error}:{id,result}));process.stdout.write(Buffer.concat([Buffer.from('Content-Length: '+raw.length+'\r\n\r\n'),raw]));};
save();
process.stdin.on('data',chunk=>{
 input=Buffer.concat([input,chunk]);if(input.length>(1<<20))throw Error('fixture input budget');
 while(true){
  const end=input.indexOf('\r\n\r\n');if(end<0)return;
  const n=Number(/Content-Length: (\d+)/i.exec(input.subarray(0,end).toString())[1]);
  if(input.length<end+4+n)return;
  const msg=JSON.parse(input.subarray(end+4,end+4+n));input=input.subarray(end+4+n);
  if(events.length>=1024)throw Error('fixture event budget');
  const uri=msg.params?.textDocument?.uri;
  events.push({method:msg.method,uri,position:msg.params?.position,id:msg.id});
  if(msg.method==='textDocument/didOpen')active.add(uri);
  if(msg.method==='textDocument/didClose'){active.delete(uri);if(uri.endsWith('/AProbe.lua'))ready=false;}
  if(msg.method==='fixture/checkpoint'){save();send(msg.id,snapshot());continue;}
  const barrier=msg.method==='textDocument/definition'&&uri?.endsWith('/AProbe.lua')&&msg.params.position.line===0&&msg.params.position.character===0;
  if(barrier){ready=true;save();send(msg.id,null);continue;}
  if(msg.id!==undefined){
   if(mode==='pending'){pending=true;save();continue;}
   if(mode==='gate'&&uri?.endsWith('/api.d.lua')&&!ready){save();send(msg.id,null,{code:-32008,message:'fixture external API scope not preloaded'});continue;}
   save();send(msg.id,[{uri,range:{start:{line:0,character:0},end:{line:0,character:1}}}]);
  }else save();
 }
});
"#;

fn fresh_case(mode: &str, startup: &str) -> PathBuf {
    let owner = lycheedev_windows::Process::open(std::process::id()).unwrap();
    let created = owner.identity().created.to_string();
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp");
    fs::create_dir_all(&base).unwrap();
    let root = base.join(format!(
        "rust-lsp-document-{}-{created}-{}-{mode}-{startup}",
        owner.identity().pid,
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    root.canonicalize().unwrap()
}

fn fixture(mode: &str, startup: &str) -> (WorkspaceSession, PathBuf, lycheedev_windows::Process) {
    let root = fresh_case(mode, startup);
    let control = lycheedev_core::control::OperationControl::new(
        Instant::now() + Duration::from_secs(8),
        Arc::new(AtomicBool::new(false)),
    );
    let files = BTreeMap::from([
        (
            "AProbe.lua".into(),
            b"local marker = 1\nreturn marker\n".to_vec(),
        ),
        ("ZCaller.lua".into(), b"return FixtureAPI()\n".to_vec()),
    ]);
    let definitions = b"---@return number\nfunction FixtureAPI() end\n";
    let runtime = Runtime {
        root: root.clone(),
        inventory: BTreeMap::new(),
    };
    let frozen = runtime.freeze(&root, &files, definitions).unwrap();
    let held_inputs = frozen.lock_inputs(&files, definitions, &control).unwrap();
    let mut command =
        Command::new(std::env::var_os("LYCHEEDEV_TEST_NODE").unwrap_or_else(|| "node".into()));
    command
        .args(["-e", PEER, mode])
        .arg(root.join("trace.json"))
        .current_dir(&root);
    let rpc = Rpc::start(command, control).unwrap();
    let process = lycheedev_windows::Process::open(rpc.child.id()).unwrap();
    // Both corrected constructors initialize without opening documents.
    // Their separate labels preserve coverage of the two post-constructor
    // states; this seam still drives the actual production batch code.
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
        coverage: json!({"luaFiles":2,"luaBytes":files.values().map(Vec::len).sum::<usize>(),"excludedFiles":0}),
        unavailable: Default::default(),
        opened: Default::default(),
        lease: None,
        batches: 0,
        life_deadline: Instant::now() + Duration::from_secs(8),
    };
    (session, root, process)
}

fn checkpoint(session: &mut WorkspaceSession) -> Value {
    let raw = session
        .rpc
        .as_mut()
        .unwrap()
        .request("fixture/checkpoint", json!({}))
        .unwrap();
    serde_json::from_slice(raw.result()).unwrap()
}

fn save(root: &Path, name: &str, value: &Value) {
    let raw = serde_json::to_vec_pretty(value).unwrap();
    assert!(raw.len() <= 1 << 20);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))
        .unwrap()
        .write_all(&raw)
        .unwrap();
}

fn retire(session: &mut WorkspaceSession, process: &lycheedev_windows::Process) -> u32 {
    session.close();
    assert!(!session.usable());
    assert!(session.held_inputs.is_empty() && session.frozen.is_none());
    process
        .wait_exit(
            Instant::now() + Duration::from_secs(2),
            &AtomicBool::new(false),
        )
        .unwrap()
}

#[test]
fn completed_batches_close_documents_and_reopen_on_the_same_worker() {
    for startup in ["sdk", "source"] {
        let (mut session, root, process) = fixture("observe", startup);
        let query =
            json!({"kind":"references","path":"ZCaller.lua","position":{"line":0,"character":7}});
        let mut boundaries = Vec::new();
        for _ in 0..2 {
            let result = session
                .analyze_typed(
                    std::slice::from_ref(&query),
                    &Default::default(),
                    &mut ResultAdmission::new(),
                )
                .unwrap();
            assert_eq!(result.state, "complete");
            boundaries.push(checkpoint(&mut session));
            assert_eq!(session.pid(), Some(process.identity().pid));
        }
        let exit = retire(&mut session, &process);
        save(
            &root,
            "batch-report.json",
            &json!({
                "startup":startup,"boundaries":boundaries,"kernelExit":exit,
                "worker":{"pid":process.identity().pid,"created":process.identity().created.to_string()},
                "scope":"actual Rpc/WorkspaceSession batch wire contract with current constructor-state fixtures, not real LuaLS/OOM proof"
            }),
        );
        assert_eq!(exit, 7);
        assert!(
            boundaries
                .iter()
                .all(|boundary| boundary["active"] == json!([])),
            "completed batch left remote documents open: {boundaries:?}"
        );
        let events = boundaries[1]["events"].as_array().unwrap();
        let opens = events
            .iter()
            .filter(|event| {
                event["method"] == "textDocument/didOpen"
                    && event["uri"]
                        .as_str()
                        .is_some_and(|uri| uri.ends_with("/ZCaller.lua"))
            })
            .count();
        let closes = events
            .iter()
            .filter(|event| {
                event["method"] == "textDocument/didClose"
                    && event["uri"]
                        .as_str()
                        .is_some_and(|uri| uri.ends_with("/ZCaller.lua"))
            })
            .count();
        assert!(
            opens >= 2 && closes >= 2,
            "cross-batch document was not reopened/closed"
        );
    }
}

#[test]
fn worktree_scope_barrier_precedes_external_api_references() {
    let (mut session, root, process) = fixture("gate", "source");
    let query = json!({"kind":"references","path":"@environment/api.d.lua","position":{"line":1,"character":9}});
    let result = session.analyze_typed(&[query], &Default::default(), &mut ResultAdmission::new());
    let error = result
        .as_ref()
        .err()
        .map(|error| json!({"code":error.code,"message":error.message}));
    let state = result.as_ref().ok().map(|result| result.state.clone());
    let trace: Value = serde_json::from_slice(&fs::read(root.join("trace.json")).unwrap()).unwrap();
    let exit = retire(&mut session, &process);
    save(
        &root,
        "barrier-report.json",
        &json!({"trace":trace,"state":state,"error":error,"kernelExit":exit,"scope":"delayed-scope protocol peer, not evidence of actual LuaLS preload omission or memory causality"}),
    );
    assert_eq!(exit, 7);
    assert_eq!(
        state.as_deref(),
        Some("complete"),
        "external API query preceded the worktree preload barrier: {error:?}"
    );
    let events = trace["events"].as_array().unwrap();
    let barrier = events
        .iter()
        .position(|event| {
            event["method"] == "textDocument/definition"
                && event["uri"]
                    .as_str()
                    .is_some_and(|uri| uri.ends_with("/AProbe.lua"))
                && event["position"] == json!({"line":0,"character":0})
        })
        .unwrap();
    let external = events
        .iter()
        .position(|event| {
            event["method"] == "textDocument/references"
                && event["uri"]
                    .as_str()
                    .is_some_and(|uri| uri.ends_with("/api.d.lua"))
        })
        .unwrap();
    assert!(barrier < external);
}

#[test]
fn in_flight_batch_cancellation_retires_documents_and_owned_io() {
    let (mut session, root, process) = fixture("pending", "source");
    let trace_path = root.join("trace.json");
    let control = lycheedev_core::control::OperationControl::new(
        Instant::now() + Duration::from_secs(5),
        Arc::new(AtomicBool::new(false)),
    );
    let cancel = control.cancel_handle();
    let trigger = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let pending = fs::read(&trace_path)
                .ok()
                .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
                .is_some_and(|value| value["pending"] == true);
            if pending {
                cancel.store(true, Ordering::Release);
                return;
            }
            assert!(
                Instant::now() < deadline,
                "peer did not receive the actual pending query"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let query =
        json!({"kind":"references","path":"ZCaller.lua","position":{"line":0,"character":7}});
    let result = session.analyze_typed(&[query], &control, &mut ResultAdmission::new());
    trigger.join().unwrap();
    let error = match result {
        Ok(_) => panic!("cancelled pending query succeeded"),
        Err(error) => error,
    };
    let exit = retire(&mut session, &process);
    save(
        &root,
        "cancel-report.json",
        &json!({"errorCode":error.code,"exitCode":error.exit_code,"kernelExit":exit,"worker":{"pid":process.identity().pid,"created":process.identity().created.to_string()},"scope":"pending actual pipe query cancellation retires process/documents; no didClose required on error/cancel"}),
    );
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(exit, 7);
}
