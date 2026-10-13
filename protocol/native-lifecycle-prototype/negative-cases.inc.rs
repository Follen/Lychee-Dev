// New tests only. Included inside a test-only copy of original host::tests.
// No original Driver/business/receiver/helper function is repaired here.
// Assertions reproduce reviewed failures; passing means defects reproduced.
#[test]
fn original_p3_review_counterexamples_with_real_lua51() {
    let root=std::env::var_os("LYCHEEDEV_LIFECYCLE_NEGATIVE_ROOT").map(PathBuf::from).expect("explicit original-negative root");
    let lua=std::env::var_os("LYCHEEDEV_LUA51").map(PathBuf::from).unwrap_or_else(||PathBuf::from("C:/Program Files (x86)/Lua/5.1/lua.exe"));
    let evidence=root.join(".tmp/rust-supervision").join(format!("native-p3-original-negatives-{}",lycheedev_windows::random::token().unwrap()));
    fs::create_dir_all(&evidence).unwrap();
    let paths=[root.join("protocol/native-lifecycle-prototype/driver.rs"),root.join("protocol/native-lifecycle-prototype/business.lua"),
        root.join("protocol/native-lifecycle-prototype/receiver.lua"),root.join("protocol/native-lifecycle-prototype/commands.lua"),
        root.join("protocol/native-lifecycle-prototype/host.rs"),root.join("protocol/native-lifecycle-prototype/negative-original-host.rs"),
        root.join("protocol/native-lifecycle-prototype/negative-cases.inc.rs"),root.join("protocol/native-v1-prototype/gate.rs"),
        root.join("protocol/native-v1-prototype/crypto.lua"),root.join("protocol/native-v1-prototype/observation.lua"),
        lua.clone(),std::env::current_exe().unwrap(),root.join("crates/lycheedev-live/tests/native_p3_original_negative.rs")];
    let inputs:Vec<_>=paths.iter().map(|path|serde_json::json!({"path":path,"sha256":digest(&fs::read(path).unwrap())})).collect();
    for (path,expected) in [
        ("driver.rs","f3b491931433ea4bcdb0385ebbf06971e28fd436cc98e4f14faaf87759db2184"),
        ("business.lua","2213cf72c62a9f4e26c5c27d26e42d5b5253db50cc231f55edc468fd08f6e7de"),
        ("receiver.lua","09443328cfb94eb10618097ae53a8f987e1f249e5502e09a4435a69669ba07c9"),
        ("host.rs","af28d6bf7573420e2290ff034a5b772e17958d010673d5f90030682cd9e69679")
    ] {assert_eq!(digest(&fs::read(root.join("protocol/native-lifecycle-prototype").join(path)).unwrap()),expected,"original authority changed before negative reproduction");}
    record(&evidence.join("inputs.json"),&serde_json::to_vec_pretty(&inputs).unwrap());
    println!("P3_ORIGINAL_NEGATIVE_EVIDENCE={}",evidence.display());
    let mut reproduced=Vec::new();
    for transport in ["two","four"] {
        let mut maximum="return 'maximum-code' --".to_owned();maximum.push_str(&"x".repeat(262144-maximum.len()));
        scenario(&root,&lua,&evidence,transport,"s01-code-carrier-limit",&maximum,|w,d|{
            let c=prepare(d.advance(&w.mailbox()));let wire=c.wire().unwrap();assert_eq!(wire.len(),524322);
            // Invoke the unchanged original helper and catch its actual bound
            // panic. This occurs before STAGE/file exposure, not in business.
            let published=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||w.execute(d,&c)));
            assert!(published.is_err(),"original host unexpectedly carries MAX_CODE");
            assert_eq!(w.published_slots,0);assert_eq!(w.private_status()[6],"0");
            let commit=if transport=="four"{2}else{0};
            let raw=format!("P3S1|{}|{}|001|{commit:03}|{}|{}","1".repeat(32),"a".repeat(64),c.digest().unwrap(),hex::encode(wire));
            let source=format!("return \"{raw}\"\n");assert_eq!(raw.len(),1048820);assert_eq!(source.len(),1048830);
            // Bypass only the host publication ceiling to separately enter the
            // original real once-loader and observe its own unchanged ceiling.
            record(&w.directory.join("slots/slot-001.lua"),source.as_bytes());
            assert_eq!(w.send("STAGE"),"ERR|file_limit");assert_eq!(w.private_status()[6],"0");
            record(&w.directory.join("counterexample.json"),&serde_json::to_vec_pretty(&serde_json::json!({
                "id":"S01","maximumCodeBytes":262144,"wireBytes":524322,"stageBytes":1048820,"sourceBytes":1048830,
                "originalHostPublicationPanicked":true,"originalRealLuaStage":"ERR|file_limit","businessExecuted":false})).unwrap());
        });reproduced.push(format!("{transport}:S01"));
        for later in ["duplicate_run","cancel"] {
            scenario(&root,&lua,&evidence,transport,&format!("s02-last-command-{later}"),"return 'same-owned-body'",|w,d|{
                let original=prepare(d.advance(&w.mailbox()));w.execute(d,&original);w.save(d);w.fault("cleanup_false_before");
                let received=prepare(d.advance(&w.mailbox()));assert_eq!(received.number(),2);
                assert_eq!(w.execute(d,&received),"DISPATCH|cleanup_unknown|published");
                let initial_deadline=w.private_status()[5].clone();let expected;
                if later=="duplicate_run" {
                    assert_eq!(w.execute(d,&original),"DISPATCH|accepted|published");expected="received_input_unknown_no_replay";
                }else{
                    let cancel=d.prepare_cancel().unwrap();assert_eq!(cancel.number(),3);
                    assert_eq!(w.execute(d,&cancel),"DISPATCH|task_already_terminal|published");expected="exact_received_outcome_unavailable";
                }
                let signed=w.mailbox();let fields:Vec<_>=signed.split('|').collect();
                assert_eq!(fields[5],if later=="duplicate_run"{"00000002"}else{"00000003"});
                assert_eq!(fields[7],if later=="duplicate_run"{"00000001"}else{"00000003"});
                wait(d.advance(&signed),expected);wait(d.advance(&w.mailbox()),expected);
                let status=w.private_status();assert_eq!(status[2],"1");assert_eq!(status[6],"1");assert_eq!(status[7],"1");assert_eq!(status[8],"0");assert_eq!(status[5],initial_deadline);
                record(&w.directory.join("counterexample.json"),&serde_json::to_vec_pretty(&serde_json::json!({
                    "id":"S02","laterCommand":later,"authenticatedHighWater":fields[5],"authenticatedLastCommand":fields[7],
                    "observedAdvanceWait":expected,"codeCalls":1,"bodyRemains":true,"cleanupDeadline":initial_deadline,
                    "scope":"bounded repeated observations reproduce blocker; not proof of mathematical infinite wait or business duplicate"})).unwrap());
            });reproduced.push(format!("{transport}:S02:{later}"));
        }
        scenario(&root,&lua,&evidence,transport,"s03-lost-witness-alive-create","return 'exposed-not-yet-executed'",|w,d|{
            let original=prepare(d.advance(&w.mailbox()));assert_eq!(w.execute_inner(d,&original,true).unwrap(),"WITHHELD|final_key");
            fs::remove_file(w.directory.join("journal/input-pending-00000001.json")).unwrap();
            let same_alive=prepare(d.advance(&w.mailbox()));assert_eq!(same_alive,original);
            let reopened=Driver::create(w.directory.join("journal"),binding(),KEY.to_vec(),run("return 'exposed-not-yet-executed'",1000)).unwrap();
            let created_existing=prepare(reopened.advance(&w.mailbox()));assert_eq!(created_existing,original);
            quarantine(w.restore().advance(&w.mailbox()),"restored_unobserved_input_quarantine");
            assert_eq!(w.private_status()[6],"0");
            record(&w.directory.join("counterexample.json"),&serde_json::to_vec_pretty(&serde_json::json!({
                "id":"S03","exposure":"withheld_original_final_key","witnessDeleted":true,
                "sameAliveDriver":"PrepareTransport(Run#1)","createExistingIntent":"PrepareTransport(Run#1)",
                "restore":"Quarantined(restored_unobserved_input_quarantine)","businessExecuted":false,
                "scope":"replay preparation candidates observed; suspect action was not sent and no duplicate execution claimed"})).unwrap());
        });reproduced.push(format!("{transport}:S03"));
        // Use the existing World/Driver helpers directly because scenario()
        // fixes RUN to 1000 ms; S04 needs execution to outlive the 5000 ms
        // cleanup deadline without reaching its own execution timeout.
        {
            let mut w=World::start(&root,&lua,evidence.join(format!("{transport}-s04-cancel-spends-cleanup-deadline")),transport);
            let d=Driver::create(w.directory.join("journal"),binding(),KEY.to_vec(),run("return 'late-body-before-execution-deadline'",10000)).unwrap();
            assert_eq!(w.send("ASYNC_ON"),"OK|async");
            let original=prepare(d.advance(&w.mailbox()));assert_eq!(w.execute(&d,&original),"DISPATCH|accepted|published");
            let running=w.mailbox();let running_fields:Vec<_>=running.split('|').collect();
            let execution_deadline=u32::from_str_radix(running_fields[19],16).unwrap();assert_eq!(execution_deadline,20000);
            assert_eq!(w.private_status()[5],"0");
            w.fault("cancel_false_before");let cancel=d.prepare_cancel().unwrap();assert_eq!(cancel.number(),2);
            assert_eq!(w.execute(&d,&cancel),"DISPATCH|cancel_unconfirmed|published");
            let after_cancel=w.private_status();let cleanup_deadline=after_cancel[5].parse::<u32>().unwrap();
            assert_eq!(cleanup_deadline,15000);assert_eq!(after_cancel[1],"running");assert_eq!(after_cancel[6],"0");
            assert_eq!(after_cancel[4],"1");assert_eq!(after_cancel[9],"1");assert_eq!(after_cancel[10],"0");
            let late_clock=cleanup_deadline+1;assert!(late_clock<execution_deadline);
            assert_eq!(w.send(&format!("CLOCK|{late_clock}")),"OK|clock_model");
            wait(d.advance(&w.mailbox()),"executing_or_unknown");
            let before_done=w.private_status();assert_eq!(before_done[1],"running");assert_eq!(before_done[6],"0");assert_eq!(before_done[9],"1");
            assert_eq!(w.send("DONE"),"OK|completed");
            let terminal=w.mailbox();let terminal_fields:Vec<_>=terminal.split('|').collect();
            assert_eq!(terminal_fields[14],"terminal");assert_eq!(terminal_fields[15],"verified");
            assert_eq!(u32::from_str_radix(terminal_fields[19],16).unwrap(),execution_deadline);
            assert_eq!(u32::from_str_radix(terminal_fields[23],16).unwrap(),cleanup_deadline);
            w.save(&d);let saved=fs::read(w.directory.join("journal/result.bin")).unwrap();
            assert_eq!(saved,b"late-body-before-execution-deadline");assert_eq!(digest(&saved),terminal_fields[16]);
            let received=prepare(d.advance(&w.mailbox()));assert_eq!(received.number(),3);
            assert_eq!(w.execute(&d,&received),"DISPATCH|cleanup_budget_exhausted|published");
            assert!(matches!(d.advance(&w.mailbox()),Action::CleanupBlocked));
            assert!(matches!(w.restore().advance(&w.mailbox()),Action::CleanupBlocked));
            let blocked=w.private_status();assert_eq!(blocked[1],"terminal");assert_eq!(blocked[2],"1");assert_eq!(blocked[3],"blocked");
            assert_eq!(blocked[4],"1");assert_eq!(blocked[5],cleanup_deadline.to_string());assert_eq!(blocked[6],"1");
            assert_eq!(blocked[7],"0");assert_eq!(blocked[8],"0");assert_eq!(blocked[9],"1");assert_eq!(blocked[10],"0");assert_eq!(blocked[11],"0");
            let retained=w.send("BODY");let retained=hex::decode(retained.strip_prefix("BODY|").unwrap()).unwrap();assert_eq!(retained,saved);
            record(&w.directory.join("counterexample.json"),&serde_json::to_vec_pretty(&serde_json::json!({
                "id":"S04","executionBudgetMillis":10000,"executionDeadline":execution_deadline,
                "cancelFault":"cancel_false_before","originalCleanupDeadline":cleanup_deadline,"lateCompletionClock":late_clock,
                "actualBody":String::from_utf8(saved.clone()).unwrap(),"actualBodySHA256":digest(&saved),"bodyRemains":true,
                "privateStatusAfterCancel":after_cancel,"privateStatusBeforeDone":before_done,"privateStatusAfterReceived":blocked,
                "authenticatedTerminalState":terminal,"receivedOutcome":"cleanup_budget_exhausted","advance":"CleanupBlocked",
                "codeCalls":1,"resourceState":"producer_completed_resource_unclosed","closeCalls":0,"closeEffects":0,"cancelCalls":1,"cancelEffects":0,"bodyClears":0,
                "resourceEvidence":"real DONE completed producer; RECEIVED inspected owned resource as unclosed; private fixture counters show no close or cancel effect",
                "scope":"original deadline liveness defect reproduced with model clock and real Lua callbacks; no fix or real-game resource proof"})).unwrap());
            w.stop();
        }
        reproduced.push(format!("{transport}:S04"));
    }
    record(&evidence.join("result.json"),&serde_json::to_vec_pretty(&serde_json::json!({
        "schema":"lycheedev.protocol-prototype.p3-original-counterexamples.v1","state":"original_review_counterexamples_reproduced",
        "reproduced":reproduced,"originalAuthorityUnchanged":true,"finalProtocolPassed":false,
        "scope":"original Rust driver/helpers and real Lua51 receiver; declared once-loader/clock/resource/fixed-binding models; failures reproduced, no fixes",
        "excluded":["independent game evidence","P1/clock/B1/500 allocator/OS carrier","fixed-protocol audit"]})).unwrap());
}
