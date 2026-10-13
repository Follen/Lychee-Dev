// Isolated revision assertions. No production addon, OS carrier or game input.
fn signed_mutation(wire:&str,changes:&[(usize,String)])->String {
    let mut fields:Vec<String>=wire.split('|').map(str::to_owned).collect();assert_eq!(fields.len(),35);
    for (index,value) in changes{fields[*index]=value.clone();}
    let raw=fields[..34].join("|");let mac=gate::hmac(&KEY,&[b"lycheedev.lifecycle.state.v2\0".as_slice(),raw.as_bytes()].concat());format!("{raw}|{mac}")
}
fn counterexample_record(w:&World,name:&str,value:serde_json::Value){record(&w.directory.join(name),&serde_json::to_vec_pretty(&value).unwrap());}

#[test]
fn revised_lifecycle_and_review_regressions_with_real_lua51(){
    let root=std::env::var_os("LYCHEEDEV_LIFECYCLE_REVISION_ROOT").map(PathBuf::from).expect("explicit revision root");
    let lua=std::env::var_os("LYCHEEDEV_LUA51").map(PathBuf::from).unwrap_or_else(||PathBuf::from("C:/Program Files (x86)/Lua/5.1/lua.exe"));
    let evidence=root.join(".tmp/rust-supervision").join(format!("native-revised-lifecycle-{}",lycheedev_windows::random::token().unwrap()));fs::create_dir_all(&evidence).unwrap();
    let paths=[root.join("protocol/native-lifecycle-revision/driver.rs"),root.join("protocol/native-lifecycle-revision/business.lua"),
        root.join("protocol/native-lifecycle-revision/receiver.lua"),root.join("protocol/native-lifecycle-revision/commands.lua"),root.join("protocol/native-lifecycle-revision/host.rs"),
        root.join("protocol/native-lifecycle-revision/revision-cases.inc.rs"),root.join("protocol/native-v1-prototype/gate.rs"),
        root.join("protocol/native-v1-prototype/crypto.lua"),root.join("protocol/native-v1-prototype/observation.lua"),lua.clone(),std::env::current_exe().unwrap(),
        root.join("crates/lycheedev-live/tests/native_revised_lifecycle.rs")];
    let inputs:Vec<_>=paths.iter().map(|p|serde_json::json!({"path":p,"sha256":digest(&fs::read(p).unwrap())})).collect();
    record(&evidence.join("inputs.json"),&serde_json::to_vec_pretty(&inputs).unwrap());println!("P3_REVISED_EVIDENCE={}",evidence.display());
    // Pointer and consumed identities exist before any positive/regression
    // scenario, so the runner can seal the first actual failure too.
    let positive_evidence=shared_lifecycle_fault_matrix_with_real_lua51();
    record(&evidence.join("positive-matrix.json"),&serde_json::to_vec_pretty(&serde_json::json!({"evidence":positive_evidence,"state":"passed_scoped_shared_matrix"})).unwrap());
    let mut revised=Vec::new();let mut maximum_failures=Vec::new();
    for transport in ["two","four"]{
        for later in ["duplicate_run","cancel"]{
            scenario(&root,&lua,&evidence,transport,&format!("s02-{later}"),"return 'retained-receipt'",|w,d|{
                let original=prepare(d.advance(&w.mailbox()));w.execute(d,&original);w.save(d);w.fault("cleanup_false_before");
                let received=prepare(d.advance(&w.mailbox()));assert_eq!(w.execute(d,&received),"DISPATCH|cleanup_unknown|published");let deadline=w.private_status()[5].clone();
                if later=="duplicate_run"{assert_eq!(w.execute(d,&original),"DISPATCH|accepted|published");}
                else{let cancel=d.prepare_cancel().unwrap();assert_eq!(w.execute(d,&cancel),"DISPATCH|task_already_terminal|published");}
                let observed=w.mailbox();let fields:Vec<_>=observed.split('|').collect();assert_eq!(fields[29],"00000002");assert_eq!(fields[30],received.digest().unwrap());assert_eq!(fields[31],"cleanup_unknown");
                let restored=w.restore();let next=prepare(restored.advance(&observed));assert!(next.number()>received.number());
                assert_eq!(w.private_status()[5],deadline);assert_eq!(w.execute(&restored,&next),"DISPATCH|cleanup_complete|published");complete(d.advance(&w.mailbox()),"verified");
                let status=w.private_status();assert_eq!(status[6],"1");assert_eq!(status[8],"1");assert_eq!(status[11],"1");
                counterexample_record(w,"regression.json",serde_json::json!({"id":"S02","later":later,"signedReceiptBeforeRetry":observed,"retryNumber":next.number(),"codeCalls":1,"closeEffects":1,"bodyClears":1}));
            });revised.push(format!("{transport}:S02:{later}"));
        }
        scenario(&root,&lua,&evidence,transport,"s02-unknown-received-higher-cancel","return 'unknown-received'",|w,d|{
            let original=prepare(d.advance(&w.mailbox()));w.execute(d,&original);w.save(d);let received=prepare(d.advance(&w.mailbox()));
            assert_eq!(w.execute_inner(d,&received,true).unwrap(),"WITHHELD|final_key");w.send("ESCAPE");
            let cancel=d.prepare_cancel().unwrap();assert_eq!(w.execute(d,&cancel),"DISPATCH|task_already_terminal|published");
            let state=w.mailbox();let f:Vec<_>=state.split('|').collect();assert_eq!(f[5],"00000003");assert_eq!(f[29],"00000000");
            wait(d.advance(&state),"received_input_unknown_no_replay");wait(w.restore().advance(&state),"received_input_unknown_no_replay");
            fs::remove_file(w.directory.join("journal/input-pending-00000002.json")).unwrap();
            quarantine(d.advance(&state),"unobserved_input_quarantine");quarantine(w.restore().advance(&state),"unobserved_input_quarantine");
            let s=w.private_status();assert_eq!(s[2],"1");assert_eq!(s[7],"0");assert_eq!(s[11],"0");
            counterexample_record(w,"regression.json",serde_json::json!({"id":"S02-reject-high-water-inference","signedState":state,"unknownReceivedNotReplayed":true,"missingWitness":"quarantined"}));
        });
        scenario(&root,&lua,&evidence,transport,"s02-signed-receipt-mutations","return 'exact-receipt'",|w,d|{
            let original=prepare(d.advance(&w.mailbox()));w.execute(d,&original);w.save(d);w.fault("cleanup_false_before");
            let received=prepare(d.advance(&w.mailbox()));w.execute(d,&received);let cancel=d.prepare_cancel().unwrap();w.execute(d,&cancel);let state=w.mailbox();
            for changes in [vec![(29,"00000004".into())],vec![(29,"00000000".into())],vec![(30,"f".repeat(64))],
                vec![(31,"cleanup_complete".into())],vec![(31,"accepted".into())],vec![(29,"00000003".into()),(30,cancel.digest().unwrap()),(31,"task_already_terminal".into())],
                vec![(20,"blocked".into())],vec![(16,"e".repeat(64))],vec![(7,"00000000".into())],
                vec![(21,"05".into())],vec![(23,"00000000".into())],vec![(33,"01".into())],vec![(0,"LDBC1".into())]]{
                assert!(matches!(d.advance(&signed_mutation(&state,&changes)),Action::Quarantined(_)));
            }
            let mut tampered=state.clone();tampered.pop();tampered.push('z');quarantine(d.advance(&tampered),"state_mac");
            assert_eq!(w.private_status()[7],"1");assert_eq!(w.private_status()[11],"0");
        });
        scenario(&root,&lua,&evidence,transport,"s03-lost-witness-all-entrypoints","return 'must-not-replay'",|w,d|{
            let original=prepare(d.advance(&w.mailbox()));assert_eq!(w.execute_inner(d,&original,true).unwrap(),"WITHHELD|final_key");
            fs::remove_file(w.directory.join("journal/input-pending-00000001.json")).unwrap();let state=w.mailbox();
            quarantine(d.advance(&state),"unobserved_input_quarantine");quarantine(w.restore().advance(&state),"unobserved_input_quarantine");
            assert_eq!(Driver::create(w.directory.join("journal"),binding(),KEY.to_vec(),original.clone()).err().unwrap(),"journal_exists_use_restore");
            assert_eq!(d.mark_business_input_pending(&original,&state),Err("input_preparation_unavailable_quarantine".into()));assert_eq!(w.private_status()[6],"0");
            counterexample_record(w,"regression.json",serde_json::json!({"id":"S03","sameAlive":"quarantined","restore":"quarantined","createExisting":"rejected_use_restore","codeCalls":0}));
        });revised.push(format!("{transport}:S03"));
        scenario(&root,&lua,&evidence,transport,"s03-mark-pending-write-fails","return 'write-failure'",|w,d|{
            let original=prepare(d.advance(&w.mailbox()));let path=w.directory.join("journal/input-pending-00000001.json");fs::create_dir(&path).unwrap();
            assert!(d.mark_business_input_pending(&original,&w.mailbox()).is_err());fs::remove_dir(&path).unwrap();
            quarantine(d.advance(&w.mailbox()),"unobserved_input_quarantine");quarantine(w.restore().advance(&w.mailbox()),"unobserved_input_quarantine");
            assert_eq!(w.private_status()[6],"0");assert_eq!(w.published_slots,0);
        });
        scenario(&root,&lua,&evidence,transport,"s03-mark-auth-fails","return 'wrong-binding'",|w,d|{
            let original=prepare(d.advance(&w.mailbox()));let state=signed_mutation(&w.mailbox(),&[(1,"f".repeat(32))]);
            assert_eq!(d.mark_business_input_pending(&original,&state),Err("state_binding".into()));quarantine(d.advance(&w.mailbox()),"unobserved_input_quarantine");
            assert_eq!(w.published_slots,0);
        });
        scenario(&root,&lua,&evidence,transport,"s03-corrupt-witness","return 'corrupt-witness'",|w,d|{
            let original=prepare(d.advance(&w.mailbox()));w.execute_inner(d,&original,true).unwrap();
            fs::write(w.directory.join("journal/input-pending-00000001.json"),b"{}").unwrap();
            quarantine(d.advance(&w.mailbox()),"input_witness_changed_quarantine");quarantine(w.restore().advance(&w.mailbox()),"input_witness_changed_quarantine");assert_eq!(w.private_status()[6],"0");
        });
        scenario(&root,&lua,&evidence,transport,"s03-lost-known-tail-intent","return 'known-intent'",|w,d|{
            let original=prepare(d.advance(&w.mailbox()));w.execute(d,&original);let cancel=d.prepare_cancel().unwrap();assert_eq!(cancel.number(),2);
            fs::remove_file(w.directory.join("journal/intent-00000002.json")).unwrap();quarantine(d.advance(&w.mailbox()),"known_intent_lost_or_changed_quarantine");
        });
        scenario(&root,&lua,&evidence,transport,"s03-lost-middle-intent","return 'intent-gap'",|w,d|{
            d.prepare_cancel().unwrap();d.prepare_cancel().unwrap();fs::remove_file(w.directory.join("journal/intent-00000002.json")).unwrap();
            quarantine(d.advance(&w.mailbox()),"journal_intent_gap_quarantine");assert_eq!(Driver::restore(w.directory.join("journal"),binding(),KEY.to_vec(),1).err().unwrap(),"journal_intent_gap_quarantine");
        });
        scenario(&root,&lua,&evidence,transport,"s03-unexposed-restoration-gap","return 'not-exposed'",|w,d|{
            assert!(matches!(d.advance(&w.mailbox()),Action::PrepareTransport(_)));quarantine(w.restore().advance(&w.mailbox()),"unobserved_input_quarantine");
            assert_eq!(w.published_slots,0);counterexample_record(w,"remaining-gap.json",serde_json::json!({"state":"conservative_unexposed_intent_quarantine","livenessClosed":false}));
        });
        scenario(&root,&lua,&evidence,transport,"s03-lost-result-meta","return 'saved-result'",|w,d|{
            let original=prepare(d.advance(&w.mailbox()));w.execute(d,&original);w.save(d);fs::remove_file(w.directory.join("journal/result-meta.json")).unwrap();
            quarantine(d.advance(&w.mailbox()),"body_without_meta_quarantine");quarantine(w.restore().advance(&w.mailbox()),"body_without_meta_quarantine");assert_eq!(w.private_status()[11],"0");
        });
        {
            let mut w=World::start(&root,&lua,evidence.join(format!("{transport}-s04-separated-deadlines")),transport);
            let d=Driver::create(w.directory.join("journal"),binding(),KEY.to_vec(),run("return 'late-body-before-execution-deadline'",10000)).unwrap();w.send("ASYNC_ON");
            let original=prepare(d.advance(&w.mailbox()));w.execute(&d,&original);w.fault("cancel_false_before");let cancel=d.prepare_cancel().unwrap();assert_eq!(w.execute(&d,&cancel),"DISPATCH|cancel_unconfirmed|published");
            let cancelled=w.private_status();assert_eq!(cancelled[5],"0");assert_eq!(cancelled[14],"15000");assert_eq!(cancelled[15],"1");assert_eq!(cancelled[4],"0");
            w.send("CLOCK|15001");let second=d.prepare_cancel().unwrap();assert_eq!(w.execute(&d,&second),"DISPATCH|cancel_budget_exhausted|published");
            assert_eq!(w.private_status()[9],"1");assert_eq!(w.private_status()[14],"15000");assert_eq!(w.send("DONE"),"OK|completed");
            let terminal=w.mailbox();let f:Vec<_>=terminal.split('|').collect();assert_eq!(f[19],"00004e20");assert_eq!(f[23],"00000000");assert_eq!(f[32],"00003a98");
            w.save(&d);assert_eq!(fs::read(w.directory.join("journal/result.bin")).unwrap(),b"late-body-before-execution-deadline");
            w.fault("cleanup_false_before");let received=prepare(d.advance(&w.mailbox()));assert_eq!(w.execute(&d,&received),"DISPATCH|cleanup_unknown|published");
            assert_eq!(w.private_status()[5],"20001");w.send("CLOCK|15002");let restored=w.restore();let retry=prepare(restored.advance(&w.mailbox()));
            assert_eq!(w.execute(&restored,&retry),"DISPATCH|cleanup_complete|published");complete(d.advance(&w.mailbox()),"verified");
            let s=w.private_status();assert_eq!(s[2],"0");assert_eq!(s[6],"1");assert_eq!(s[7],"2");assert_eq!(s[8],"1");assert_eq!(s[9],"1");assert_eq!(s[10],"0");assert_eq!(s[11],"1");
            counterexample_record(&w,"regression.json",serde_json::json!({"id":"S04","originalCancelDeadline":15000,"executionDeadline":20000,"firstReceivedCleanupDeadline":20001,"retryClock":15002,"signedTerminalState":terminal,"codeCalls":1,"closeEffects":1,"bodyClears":1}));w.stop();
        }revised.push(format!("{transport}:S04"));
        {
            let mut w=World::start(&root,&lua,evidence.join(format!("{transport}-phase-attempt-bounds")),transport);
            let d=Driver::create(w.directory.join("journal"),binding(),KEY.to_vec(),run("return 'phase-bounds'",10000)).unwrap();w.send("ASYNC_ON");let original=prepare(d.advance(&w.mailbox()));w.execute(&d,&original);
            for _ in 0..4{w.fault("cancel_false_before");let c=d.prepare_cancel().unwrap();assert_eq!(w.execute(&d,&c),"DISPATCH|cancel_unconfirmed|published");assert_eq!(w.private_status()[14],"15000");}
            let fifth=d.prepare_cancel().unwrap();assert_eq!(w.execute(&d,&fifth),"DISPATCH|cancel_budget_exhausted|published");assert_eq!(w.private_status()[9],"4");
            w.send("CLOCK|20001");assert_eq!(w.private_status()[9],"4");assert_eq!(w.private_status()[14],"15000");assert_eq!(w.send("DONE"),"OK|completed");w.save(&d);
            for _ in 0..4{w.fault("cleanup_false_before");let c=prepare(d.advance(&w.mailbox()));assert_eq!(w.execute(&d,&c),"DISPATCH|cleanup_unknown|published");assert_eq!(w.private_status()[5],"25001");}
            assert!(matches!(d.advance(&w.mailbox()),Action::CleanupBlocked));let s=w.private_status();assert_eq!(s[9],"4");assert_eq!(s[7],"4");assert_eq!(s[8],"0");assert_eq!(s[11],"0");assert_eq!(s[2],"1");
            counterexample_record(&w,"bounds.json",serde_json::json!({"cancelCalls":4,"closeCalls":4,"clearCalls":0,"fifthCancellationBlocked":true,"timeoutDidNotResetCancelPhase":true,"fifthReceivedNotPrepared":true,"bodyRetained":true}));w.stop();
        }
        // Carrier boundaries are tested separately from the full MAX_CODE
        // lifecycle below. They cannot turn a freshness failure into a pass.
        {
            let mut maximum="return 'maximum-code' --".to_owned();maximum.push_str(&"x".repeat(super::driver::MAX_CODE-maximum.len()));
            let command=run(&maximum,1000);let wire=command.wire().unwrap();assert_eq!(wire.len(),super::driver::MAX_COMMAND_WIRE);
            assert_eq!(run(&(maximum.clone()+"x"),1000).wire(),Err("run_fields".into()));
            let commit=if transport=="four"{2}else{0};let raw=format!("P3S1|{}|{}|001|{commit:03}|{}|{}","1".repeat(32),"a".repeat(64),command.digest().unwrap(),hex::encode(wire));
            assert_eq!(raw.len(),super::driver::MAX_STAGE_WIRE);assert_eq!(format!("return \"{raw}\"\n").len(),super::driver::MAX_SLOT_SOURCE);
            let mut w=World::start(&root,&lua,evidence.join(format!("{transport}-s01-carrier-exact")),transport);let started=Instant::now();w.publish_slot(1,&raw);let reply=w.send("STAGE");assert!(reply.starts_with("STATE|"));
            assert_eq!(gate::decode(reply.strip_prefix("STATE|").unwrap(),&KEY).unwrap().facts,0);assert_eq!(w.private_status()[6],"0");
            counterexample_record(&w,"carrier.json",serde_json::json!({"maximumCodeBytes":super::driver::MAX_CODE,"wireBytes":super::driver::MAX_COMMAND_WIRE,"stageBytes":super::driver::MAX_STAGE_WIRE,"sourceBytes":super::driver::MAX_SLOT_SOURCE,"stageElapsedMillis":started.elapsed().as_millis(),"fullLifecyclePassed":false}));w.stop();
            let mut over=World::start(&root,&lua,evidence.join(format!("{transport}-s01-carrier-over")),transport);
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||over.publish_slot(1,&(raw.clone()+"x")))).is_err());
            assert_eq!(over.published_slots,0);record(&over.directory.join("slots/slot-001.lua"),format!("return \"{raw}x\"\n").as_bytes());assert_eq!(over.send("STAGE"),"ERR|file_limit");assert_eq!(over.private_status()[6],"0");over.stop();
            let mut full=World::start(&root,&lua,evidence.join(format!("{transport}-s01-full-lifecycle")),transport);
            let d=Driver::create(full.directory.join("journal"),binding(),KEY.to_vec(),command).unwrap();let c=prepare(d.advance(&full.mailbox()));let started=Instant::now();
            let attempted=full.execute_inner(&d,&c,false);
            match attempted{
                Ok(answer)=>{assert_eq!(answer,"DISPATCH|accepted|published");full.finish(&d,"verified");assert_eq!(fs::read(full.directory.join("journal/result.bin")).unwrap(),b"maximum-code");let s=full.private_status();assert_eq!(s[6],"1");assert_eq!(s[8],"1");assert_eq!(s[11],"1");revised.push(format!("{transport}:S01-full-lifecycle"));counterexample_record(&full,"full-lifecycle.json",serde_json::json!({"state":"passed","fullLifecyclePassed":true,"elapsedMillis":started.elapsed().as_millis(),"codeCalls":1,"closeEffects":1,"bodyClears":1}));},
                Err(error)=>{maximum_failures.push(format!("{transport}:{error}"));let status=full.private_status();counterexample_record(&full,"full-lifecycle.json",serde_json::json!({"state":"failed","fullLifecyclePassed":false,"error":error,"elapsedMillis":started.elapsed().as_millis(),"privateStatus":status,"freshnessMillis":750}));},
            }full.stop();
        }
    }
    let state=if maximum_failures.is_empty(){"passed_scoped_revision_matrix"}else{"failed_maximum_code_full_lifecycle"};
    record(&evidence.join("result.json"),&serde_json::to_vec_pretty(&serde_json::json!({"schema":"lycheedev.protocol-prototype.lifecycle-revision.v1","state":state,
        "positiveEvidence":positive_evidence,"revised":revised,"maximumCodeFailures":maximum_failures,"finalProtocolPassed":false,
        "scope":"shared Rust driver and actual Lua51 receiver; once-loader/model-clock/owned-resource fixtures; no final protocol claim",
        "excluded":["unexposed intent restoration liveness","independent audit","P1/clock/B1/500 allocator/OS carrier","real game acceptance","full product mapping/performance"]})).unwrap());
    assert!(maximum_failures.is_empty(),"MAX_CODE full lifecycle failed; exact failures are sealed: {maximum_failures:?}");
}
