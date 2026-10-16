use super::*;
use std::sync::atomic::AtomicUsize;

#[test]
fn private_frames_reject_unknown_fields_and_missing_wire() {
    for value in [
        json!({"processing_deadline_unix_ms":1}),
        json!({"wire":PRIVATE_WIRE,"processing_deadline_unix_ms":1,"extra":false}),
    ] {
        assert!(serde_json::from_value::<WorkerCall>(value).is_err());
    }
    assert!(
        serde_json::from_value::<WorkerMeta>(
            json!({"wire":PRIVATE_WIRE,"accepted":false,"cached":null,"extra":1})
        )
        .is_err()
    );
    let call: WorkerCall =
        serde_json::from_value(json!({"wire":PRIVATE_WIRE,"processing_deadline_unix_ms":123}))
            .unwrap();
    assert_eq!(call.processing_deadline_unix_ms, 123);
}
#[test]
fn clean_retirement_preserves_exact_primary_error_and_details() {
    let original = ToolkitError::new("command.deadline", "original", 5)
        .with_details(json!({"location":"original-byte"}));
    let expected = serde_json::to_value(&original).unwrap();
    assert_eq!(
        serde_json::to_value(with_retirement(original, Ok(()))).unwrap(),
        expected
    );
}
#[test]
fn private_terminal_is_optional_and_does_not_change_public_error_bytes() {
    let original = ToolkitError::new("external.io", "failed to fill whole buffer", 5);
    let bytes = serde_json::to_vec(&original).unwrap();
    let previous = json!({"value":null,"error":original});
    let old: Response = serde_json::from_value(previous.clone()).unwrap();
    assert!(old.terminal.is_none());
    assert_eq!(serde_json::to_value(&old).unwrap(), previous);
    let response = Response { value:None, error:Some(original), terminal:Some(terminal_process().terminal(true)) };
    let mut framed = Vec::new();
    write_frame(&mut framed, &response, RESPONSE_BYTES).unwrap();
    let decoded: Response = read_frame(&mut std::io::Cursor::new(framed), RESPONSE_BYTES).unwrap();
    assert!(decoded.terminal.is_some());
    assert_eq!(serde_json::to_vec(decoded.error.as_ref().unwrap()).unwrap(), bytes);
}

#[test]
fn terminal_event_requires_exact_authenticated_owner_and_keeps_unknown_exit() {
    let process = terminal_process();
    let terminal = process.terminal(true);
    let mut hello = Hello { wire:WIRE.into(), executable:"same-image".into(), pid:41, created:6 };
    let event = terminal.event(&hello, 1234).unwrap();
    assert_eq!(event["terminal"]["analysisPid"], 42);
    assert_eq!(event["terminal"]["analysisCreated"], 7);
    assert!(event["terminal"]["exit"].is_null());
    assert!(event["terminal"]["jobMemory"].is_null());
    assert_eq!(event["limitNotification"], "unavailable_not_observed");
    assert_eq!(event["requestDeadlineUnixMs"], 1234);
    hello.created += 1;
    assert!(terminal.event(&hello, 1234).is_none());
    hello.created = 6; hello.pid += 1;
    assert!(terminal.event(&hello, 1234).is_none());
}

#[test]
fn natural_guarded_child_exit_is_retained_before_cleanup_and_handle_close() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.arg("--list").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let deadline = Instant::now() + Duration::from_secs(5);
    let (child, job) = guarded_spawn::spawn_suspended_guarded_until(command, 2 << 30, deadline, deadline, || false).unwrap();
    let mut process = terminal_process();
    process.identity = Some(guarded_spawn::child_identity(&child).unwrap());
    process.child = Some(child); process.job = Some(job); process.reaped = false;
    process.job_empty = false; process.terminate_requested = false; process.job_terminate_accepted = false;
    while !process.poll_exit().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(POLL);
    }
    assert_eq!(process.exit.as_ref().unwrap().code, Some(0));
    assert!(process.exit.as_ref().unwrap().observed_before_termination);
    let mut slot = Some(process);
    let (retired, terminal) = retire_slot_observed(&mut slot, deadline);
    retired.unwrap(); assert!(slot.is_none());
    let terminal = terminal.unwrap();
    assert_eq!(terminal.exit.as_ref().unwrap().code, Some(0));
    assert!(terminal.exit.unwrap().observed_before_termination);
    assert_eq!(terminal.job_memory.as_ref().unwrap().job_memory_limit, 2u64 << 30);
    assert_eq!(terminal.job_memory.unwrap().active_processes, 0);
    assert!(terminal.root_reaped && terminal.job_empty && terminal.analysis_retired);
}

#[test]
fn cleanup_observation_cannot_be_reclassified_as_original_exit() {
    let mut process = terminal_process();
    process.exit = Some(ChildExit { code:Some(7), observed_before_termination:false });
    let mut slot = Some(process);
    let (retired, terminal) = retire_slot_observed(&mut slot, Instant::now());
    retired.unwrap();
    assert!(!terminal.unwrap().exit.unwrap().observed_before_termination);
}
#[test]
fn pending_retirement_keeps_primary_and_actual_cleanup_facts() {
    let original = ToolkitError::new("command.cancelled", "original", 7)
        .with_details(json!({"location":"original-byte"}));
    let cleanup = fault("unconfirmed").with_details(json!({"rootReaped":true,"jobEmpty":false}));
    let result = with_retirement(original, Err(cleanup));
    assert_eq!(result.code, "command.cancelled");
    assert_eq!(result.exit_code, 7);
    assert_eq!(result.details["location"], "original-byte");
    assert_eq!(
        result.details["analysisRetirementError"]["details"]["jobEmpty"],
        false
    );
}
#[test]
fn private_worker_rejects_marker_wrong_arity_and_duplicate_flags_before_startup() {
    for args in [
        vec![],
        vec!["--source-native-session-worker"],
        vec![
            "--wrong",
            "--home",
            "x",
            "--owner-pid",
            "1",
            "--owner-created",
            "1",
            "--start-token",
            "x",
        ],
        vec![
            "--source-native-session-worker",
            "--home",
            "x",
            "--home",
            "x",
            "--owner-created",
            "1",
            "--start-token",
            "x",
        ],
    ] {
        let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(worker(&args).unwrap_err().exit_code, 2);
    }
    let args = [
        "--source-native-session-worker",
        "--home",
        "x",
        "--owner-pid",
        "1",
        "--owner-created",
        "1",
        "--start-token",
        "not-a-token",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    assert!(worker(&args).unwrap_err().message.contains("token differs"));
}

#[test]
fn actual_guarded_test_child_has_exact_identity_and_checked_terminal_handles() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    // The already-running, Cargo-governed test image only lists its own tests.
    // No third-party executable, game, study source, configuration or test runs.
    command
        .arg("--list")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let end = Instant::now() + Duration::from_secs(5);
    let (child, job) =
        guarded_spawn::spawn_suspended_guarded_until(command, 2 << 30, end, end, || false).unwrap();
    let identity = guarded_spawn::child_identity(&child).unwrap();
    assert_eq!(identity.pid, child.id());
    assert_ne!(identity.created, 0);
    assert_eq!(
        job.memory_accounting().unwrap().job_memory_limit,
        2u64 << 30
    );
    let mut process=terminal_process();
    process.identity=Some(identity);
    process.child=Some(child);
    process.job=Some(job);
    process.reaped=false;
    process.job_empty=false;
    process.terminate_requested=false;
    process.job_terminate_accepted=false;
    process.retire(end).unwrap();
    let proof=process.observation(true);
    for field in ["rootReaped","pipeWorkerJoined","jobEmpty","analysisRetired"] { assert_eq!(proof[field],true,"{proof}"); }
    assert!(process.child.is_none());assert!(process.job.is_none());
    assert_eq!(proof["exposedHandleCloseErrors"],json!([]));
}
#[test]
fn expired_startup_cleanup_returns_custody_instead_of_renewing_its_deadline() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg("--list")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let checks = AtomicUsize::new(0);
    let original_cutoff = Instant::now();
    let failure = guarded_spawn::spawn_suspended_guarded_until(
        command,
        2 << 30,
        original_cutoff + Duration::from_secs(5),
        original_cutoff,
        || checks.fetch_add(1, Ordering::AcqRel) >= 1,
    );
    let mut failure = match failure {
        Err(error) => error,
        Ok(_) => panic!("cancelled suspended child resumed"),
    };
    assert_eq!(failure.cause, "startup.cancelled");
    let expected = failure
        .cleanup
        .identity
        .clone()
        .expect("exact child was created suspended");
    if let Some((mut child, job)) = failure.take_pending() {
        assert_eq!(guarded_spawn::child_identity(&child).unwrap(), expected);
        // This is explicit fixture recovery of returned pending custody, not a
        // production extension or a claim that the expired cleanup completed.
        assert!(!failure.cleanup.child_waited || failure.cleanup.job_empty == Some(false));
        if let Some(job) = &job {
            job.terminate().unwrap();
        } else {
            let _ = child.kill();
        }
        let recovery = Instant::now() + Duration::from_secs(5);
        while child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < recovery);
            std::thread::sleep(POLL);
        }
        if let Some(job) = job {
            while job.memory_accounting().unwrap().active_processes != 0 {
                assert!(Instant::now() < recovery);
                std::thread::sleep(POLL);
            }
            job.close().unwrap();
        }
        guarded_spawn::close_child(child).unwrap();
    } else {
        assert!(failure.cleanup.child_waited);
        assert!(failure.cleanup.job_empty == Some(true) || failure.cleanup.job_empty.is_none());
        assert!(failure.cleanup.error.is_none());
    }
}

#[test]
fn checked_close_uncertainty_survives_terminal_process_facts_and_retry() {
    let mut process = terminal_process();
    process.close_errors.push("startup stdin close unconfirmed".into());
    for _ in 0..2 {
        let error = process.retire(Instant::now()).unwrap_err();
        assert_eq!(error.details["rootReaped"], true);
        assert_eq!(error.details["jobEmpty"], true);
        assert_eq!(error.details["analysisRetired"], false);
        assert_eq!(process.close_errors.len(), 1);
        assert!(process.retiring());
    }
}

fn terminal_process() -> AnalysisProcess {
    AnalysisProcess {
        child: None,
        job: None,
        identity: Some(ProcessIdentity {
            pid: 42,
            created: 7,
        }),
        token: String::new(),
        owner: ProcessIdentity {
            pid: 41,
            created: 6,
        },
        pipes: None,
        exchange: None,
        initialized: false,
        cached: None,
        retiring: true,
        reaped: true,
        job_empty: true,
        terminate_requested: true,
        job_terminate_accepted: true,
        root_kill_accepted: false,
        close_errors: Vec::new(),
        exit: None, exit_wait_error: None, job_observation: None, job_memory_error: None,
    }

}

#[test]
fn stopped_phase_observation_never_starts_optional_disk_work() {
    let expired=OperationControl::uncancelable(Instant::now());
    observe_if_active(&expired,||panic!("expired phase must not start optional storage"));
    let cancelled=Arc::new(AtomicBool::new(true));
    let control=OperationControl::new(Instant::now()+Duration::from_secs(1),cancelled);
    observe_if_active(&control,||panic!("cancelled phase must not start optional storage"));
}
#[test]
fn optional_phase_failure_does_not_replace_the_operation_control() {
    let control=OperationControl::uncancelable(Instant::now()+Duration::from_secs(1));
    let calls=AtomicUsize::new(0);
    observe_if_active(&control,||{calls.fetch_add(1,Ordering::AcqRel);Err(fault("fixture diagnostic write failed"))});
    assert_eq!(calls.load(Ordering::Acquire),1);
    control.check().unwrap();
}
#[test]
fn already_reaped_slot_releases_without_diagnostic_io_or_new_grace() {
    let mut process=terminal_process();
    let expired=Instant::now();
    process.retire(expired).unwrap();
    let proof=process.observation(true);
    for field in ["rootReaped","pipeWorkerJoined","jobEmpty","analysisRetired"] { assert_eq!(proof[field],true); }
    assert_eq!(proof["exposedHandleCloseErrors"],json!([]));
    let mut slot=Some(process);
    retire_slot(&mut slot,expired).unwrap();
    assert!(slot.is_none());
}


fn phase_test_memory(private_commit:u64,working_set:u64)->lycheedev_windows::process::ProcessMemory {
    lycheedev_windows::process::ProcessMemory{identity:ProcessIdentity{pid:42,created:7},
        private_commit,working_set,peak_private_commit:private_commit,peak_working_set:working_set,
        scope:"test held handle".into()}
}
#[test]
fn private_phase_policy_bounds_file_detail_and_preserves_late_stages() {
    let mut sampling=PhaseSampling::default();
    for _ in 0..4036 {for phase in ["query_relations_file_syntax","query_relations_reference_rows","query_relations_file_complete"] {sampling.admit(phase,0);}}
    assert_eq!(sampling.details,PHASE_DETAIL_LIMIT);assert_eq!(sampling.records,PHASE_DETAIL_LIMIT);
    assert_eq!(sampling.file_visits,4036);assert_eq!(sampling.omitted,4036*3-PHASE_DETAIL_LIMIT);
    for phase in ["query_references_digest","query_references_retain","query_type_api","query_execution_identity","query_operations_executed","query_scope_evidence"] {assert!(sampling.admit(phase,0),"lost {phase}");}
    while sampling.admit("query_definition_resolve",0){}
    for phase in ["query_assemble_reply","query_reply_assembled","retiring_analysis","analysis_retired"] {assert!(sampling.admit(phase,0),"lost final {phase}");}
    assert!(sampling.records<=PHASE_SAMPLE_LIMIT);
    assert!(!sampling.admit("x",PHASE_BYTE_LIMIT));
    assert!(!sampling.admit(&"x".repeat(65),0));
}
#[test]
fn private_phase_policy_reserves_bytes_for_late_errors() {
    let mut sampling=PhaseSampling::default();
    let normal_end=PHASE_BYTE_LIMIT-PHASE_FINAL_RESERVE*PHASE_LINE_LIMIT;
    assert!(!sampling.admit("query_relations_file_syntax",normal_end));
    assert!(sampling.admit("retiring_analysis",normal_end));
    assert!(!sampling.admit("analysis_retired",PHASE_BYTE_LIMIT));
}
#[test]
fn private_phase_journal_appends_bounded_lines_and_never_reopens_collision() {
    let home=std::env::temp_dir().join(format!("lycheedev-phase-journal-{}-{}",std::process::id(),utc_ms().unwrap()));
    std::fs::create_dir_all(&home).unwrap();
    let control=OperationControl::default();let own=ProcessIdentity{pid:42,created:7};let owner=ProcessIdentity{pid:41,created:6};
    let mut journal=PhaseJournal::open(&home,&own,&control);assert!(journal.file.is_some());
    let name=journal.name.clone();
    let occupied=(1..PHASE_FILE_SLOTS).map(|_|{
        let slot=PhaseJournal::open(&home,&own,&control);assert!(slot.file.is_some());assert_ne!(slot.name,name);slot
    }).collect::<Vec<_>>();
    let collision=PhaseJournal::open(&home,&own,&control);assert!(collision.file.is_none());
    assert_eq!(collision.state,"slots_exhausted");assert!(collision.metadata()["file"].is_null());
    assert_eq!(std::fs::read_dir(home.join("tmp")).unwrap().count(),PHASE_FILE_SLOTS);
    let cases=[("preparing",Some(phase_test_memory(100,200))),("query_build_scope",Some(phase_test_memory(150,180))),("query_scope_built",None),("query_type_api",Some(phase_test_memory(1000,2000))),("query_operation_answer",Some(phase_test_memory(900,2100)))];
    for (i,(phase,memory)) in cases.iter().enumerate(){assert!(journal.admit(phase));journal.append(phase,i as u128,memory.as_ref(),&owner,&own,1234,&control);}
    drop(journal);drop(collision);drop(occupied);
    let bytes=std::fs::read(home.join("tmp").join(name)).unwrap();
    assert!(bytes.len()<=PHASE_BYTE_LIMIT);assert!(bytes.ends_with(b"\n"));
    let samples=bytes.split(|byte|*byte==b'\n').filter(|line|!line.is_empty()).map(|line|{assert!(line.len()<PHASE_LINE_LIMIT);serde_json::from_slice::<Value>(line).unwrap()}).collect::<Vec<_>>();
    assert_eq!(samples.len(),5);assert_eq!(samples[1]["privateCommitDelta"],50);assert_eq!(samples[1]["workingSetDelta"],-20);
    assert!(samples[0]["privateCommitDelta"].is_null());assert!(samples[2]["processMemory"].is_null());assert!(samples[3]["privateCommitDelta"].is_null());assert_eq!(samples[4]["privateCommitDelta"],-100);
    for (i,sample) in samples.iter().enumerate(){assert_eq!(sample["sequence"],i);assert_eq!(sample["workerPid"],41);assert_eq!(sample["workerCreated"],6);assert_eq!(sample["analysisPid"],42);assert_eq!(sample["analysisCreated"],7);assert_eq!(sample["requestDeadlineUnixMs"],1234);}
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn private_phase_policy_keeps_real_multifile_query_and_stops_on_cancellation() {
    for cancel_type in [false,true] {
        let control=OperationControl::default();
        let mut files=BTreeMap::from([("target.lua".into(),"function Target() end\n".into())]);
        for id in 0..48 {files.insert(format!("caller-{id:03}.lua"),"Target()\n".into());}
        let snapshot=crate::native_research::SnapshotRequest{files,load_documents:BTreeMap::new(),environment_documents:BTreeMap::new(),
            source:json!({"repository":"phase-cap-fixture","product":"retail"}),
            luac:crate::native::LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
            api:vec![],environment_manifest:None,load_plans:vec![],input_gaps:vec![]};
        let query=crate::native_research::QueryRequest{symbol:crate::index::Symbol{id:String::new(),kind:"declaration".into(),name:"Target".into(),target:String::new(),category:"function".into(),confidence:String::new(),path:"target.lua".into(),line:1,end_line:1,signature:String::new(),scope:String::new(),ordinal:0},direction:"incoming".into()};
        let sampling=std::cell::RefCell::new(PhaseSampling::default());let recorded=std::cell::RefCell::new(Vec::new());
        let cancel=control.cancel_handle();let mut cached=None;
        let result=execute_cached_query(&mut cached,Some(WorkerRequest{snapshot,query}),None,&control,41,|phase,_|{
            // Match production: cancellation suppresses optional diagnostics,
            // including retirement callbacks. Policy-only reserve is tested above.
            observe_if_active(&control,||{
                if sampling.borrow_mut().admit(phase,0){recorded.borrow_mut().push(phase.to_owned());}Ok(())
            });
            if cancel_type&&phase=="query_type_observations"{cancel.store(true,Ordering::Release);}
        });
        let sampling=sampling.into_inner();let recorded=recorded.into_inner();
        assert_eq!(sampling.details,PHASE_DETAIL_LIMIT);assert!(sampling.file_visits>=48);assert!(sampling.omitted>0);
        for phase in ["query_references_digest","query_references_retain","query_type_observations"] {assert!(recorded.iter().any(|item|item==phase),"missing {phase}");}
        if cancel_type {
            assert_eq!(result.unwrap_err().code,"command.cancelled");assert!(cached.is_none());
            for phase in ["retiring_analysis","analysis_retired"] {assert!(!recorded.iter().any(|item|item==phase),"observed cancelled {phase}");}
        } else {result.unwrap();assert!(recorded.iter().any(|item|item=="query_scope_evidence"));assert!(recorded.iter().any(|item|item=="query_reply_assembled"));}
        assert!(sampling.records<=PHASE_SAMPLE_LIMIT);
    }
}

#[test]
fn raw_exchange_authentication_keeps_exact_wire_token_child_and_owner() {
    let expected=ProcessIdentity{pid:42,created:7};
    let owner=ProcessIdentity{pid:41,created:6};
    for field in 0..7 {
        let mut hello=WorkerHello{wire:PRIVATE_WIRE.into(),token:"exact-token".into(),identity:expected.clone(),owner:owner.clone()};
        match field {
            0=>{},1=>hello.wire.push('x'),2=>hello.token.push('x'),
            3=>hello.identity.pid+=1,4=>hello.identity.created+=1,
            5=>hello.owner.pid+=1,6=>hello.owner.created+=1,_=>unreachable!(),
        }
        let mut bytes=Vec::new();write_frame(&mut bytes,&hello,4096).unwrap();
        let hello:WorkerHello=read_frame(&mut std::io::Cursor::new(bytes),4096).unwrap();
        match hello.authenticate(&expected,&owner,"exact-token") {
            Ok(())=>assert_eq!(field,0),
            Err(error)=>{assert_ne!(field,0);assert_eq!(error.code,"source.native_session");assert_eq!(error.message,"Native analysis child identity differs");}
        }
    }
}

#[test]
fn cancelled_and_expired_requests_retire_completed_raw_exchange_on_default_stack() {
    std::thread::spawn(|| {
        for cancelled in [false,true] {
            let deadline=Instant::now()+Duration::from_secs(5);
            let executable=std::env::current_exe().unwrap();
            let current=Identity{_file:File::open(&executable).unwrap(),path:executable.clone(),digest:"fixture".into()};
            let mut command=Command::new(executable);
            // A real owned child supplies the handles; its --list output is not
            // treated as an authenticated protocol reply. The completed reply
            // below is the deliberate transport fixture being abandoned.
            command.arg("--list").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
            let (mut child,job)=guarded_spawn::spawn_suspended_guarded_until(command,2<<30,deadline,deadline,||false).unwrap();
            let identity=guarded_spawn::child_identity(&child).unwrap();
            let pipes=(child.stdin.take().unwrap(),child.stdout.take().unwrap());
            let mut process=terminal_process();
            process.identity=Some(identity);process.child=Some(child);process.job=Some(job);
            process.reaped=false;process.job_empty=false;process.retiring=false;
            process.terminate_requested=false;process.job_terminate_accepted=false;
            process.exchange=Some(std::thread::spawn(move || {
                let mut bytes=b"{\"value\":".to_vec();
                bytes.extend(std::iter::repeat_n(b'[',126));bytes.push(b'"');
                bytes.extend(std::iter::repeat_n(b'x',1<<20));bytes.push(b'"');
                bytes.extend(std::iter::repeat_n(b']',126));bytes.extend_from_slice(b",\"error\":null}");
                let response=RawResponse::decode(bytes).unwrap();
                Exchange{pipes,result:Ok((WorkerMeta{wire:PRIVATE_WIRE.into(),accepted:true,cached:None},Some(response))),authenticated:true}
            }));
            let owner=process.owner.clone();
            let mut slot=Some(process);let mut terminal=None;
            let control=OperationControl::new(if cancelled{deadline}else{Instant::now()},Arc::new(AtomicBool::new(cancelled)));
            let result=query(&mut slot,Vec::new(),None,&current,std::path::Path::new("unused"),&owner,&control,deadline,&mut terminal);
            let error=match result{Err(error)=>error,Ok(_)=>panic!("stopped request admitted")};
            assert_eq!(error.code,if cancelled{"command.cancelled"}else{"command.deadline"});
            assert!(slot.is_none());
            let terminal=terminal.unwrap();
            assert!(terminal.root_reaped&&terminal.pipe_worker_joined&&terminal.job_empty&&terminal.analysis_retired);
            assert!(terminal.child_hello_authenticated);
            assert!(terminal.handle_close_errors.is_empty());
        }
    }).join().unwrap();
}
