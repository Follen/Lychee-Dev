use super::*;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn file(path: &str, bytes: usize) -> git::TreeFile {
    git::TreeFile {
        path: path.into(),
        object: "a".repeat(40),
        bytes,
        asset: false,
        oversized: false,
    }
}

// This helper uses the production typed channels and Pipeline, with explicit
// scoped ownership and the production 2 MiB stack, including the real-parser
// regression. The fake parser is only for deterministic scheduling faults.
fn with_workers(
    count: usize,
    parse: impl Fn(usize, ParseInput) -> ParseOutput + Sync,
    body: impl FnOnce(Vec<Worker>),
) {
    thread::scope(|scope| {
        let parse = &parse;
        let mut workers = Vec::new();
        let mut handles = Vec::new();
        for ordinal in 0..count {
            let (input, receive) = mpsc::sync_channel(1);
            let (send, output) = mpsc::sync_channel(1);
            handles.push(
                thread::Builder::new()
                    .stack_size(WORKER_STACK_BYTES)
                    .spawn_scoped(scope, move || {
                        while let Ok(input) = receive.recv() {
                            if send.send(parse(ordinal, input)).is_err() {
                                break;
                            }
                        }
                    })
                    .unwrap(),
            );
            workers.push(Worker { input, output });
        }
        body(workers);
        for handle in handles {
            handle.join().unwrap();
        }
    });
}

fn empty(input: ParseInput) -> ParseOutput {
    ParseOutput {
        mode: input.mode,
        data: input.data,
        facts: Some(Facts::default()),
        parse_elapsed: Duration::ZERO,
    }
}
fn pipeline(workers: Vec<Worker>, profile: &Profile) -> Pipeline<'_> {
    let free_workers = (0..workers.len()).collect();
    Pipeline {
        workers,
        pending: VecDeque::new(),
        pending_bytes: 0,
        retained_bytes: 0,
        free_workers,
        incoming_bytes: MAX_SOURCE_BYTES,
        failed: false,
        batch_bytes: GIT_BATCH_BYTES,
        profile,
    }
}

#[test]
fn only_bounded_utf8_supported_modes_are_speculative() {
    for path in [
        "A.lua",
        "A.LUA",
        "A.xml",
        "A.XmL",
        "A.toc",
        "A.TOC",
        "Interface/Blizzard_APIDocumentationGenerated/A.lua",
        "Interface/Blizzard_APIDocumentationGenerated/A.LUA",
    ] {
        assert!(eligible(&file(path, 1), b"x"), "{path}");
        let mut asset = file(path, 1);
        asset.asset = true;
        assert!(!eligible(&asset, b"x"));
        let mut oversized = file(path, 1);
        oversized.oversized = true;
        assert!(!eligible(&oversized, b"x"));
        assert!(!eligible(&file(path, 1), b"\xff"));
        assert!(!eligible(&file(path, MAX_SOURCE_BYTES + 1), b"x"));
        assert!(!eligible(&file(path, 1), &vec![b' '; MAX_SOURCE_BYTES + 1]));
        assert!(eligible(
            &file(path, MAX_SOURCE_BYTES),
            &vec![b' '; MAX_SOURCE_BYTES]
        ));
    }
    for path in ["A.txt", "A.luax", "lua", ""] {
        assert!(!eligible(&file(path, 1), b"x"), "{path}");
    }
}

#[test]
fn parse_modes_keep_exact_generated_marker_and_ascii_insensitive_extensions() {
    for (path, mode, worker) in [
        ("A.LuA", ParseMode::LuaOrdinary, true),
        ("A.XmL", ParseMode::Xml, true),
        ("A.ToC", ParseMode::Toc, true),
        (
            "Interface/Blizzard_APIDocumentationGenerated/A.LUA",
            ParseMode::LuaGenerated,
            true,
        ),
        (
            "Blizzard_APIDocumentationGenerated/A.lua",
            ParseMode::LuaOrdinary,
            true,
        ),
        (
            "Interface/blizzard_APIDocumentationGenerated/A.lua",
            ParseMode::LuaOrdinary,
            true,
        ),
        (
            "Interface/Blizzard_APIDocumentationGeneratedSuffix/A.lua",
            ParseMode::LuaOrdinary,
            true,
        ),
        (
            "Interface/Blizzard_APIDocumentationGenerated/A.xml",
            ParseMode::Xml,
            true,
        ),
        (
            "Interface/Blizzard_APIDocumentationGenerated/A.toc",
            ParseMode::Toc,
            true,
        ),
    ] {
        assert_eq!(ParseMode::for_path(path), mode, "{path}");
        assert_eq!(eligible(&file(path, 1), b"x"), worker, "{path}");
    }
}

fn original_facts(mode: ParseMode, data: &[u8]) -> Facts {
    match mode {
        ParseMode::LuaOrdinary => crate::facts::lua(data, false),
        ParseMode::LuaGenerated => crate::facts::lua(data, true),
        ParseMode::Xml => crate::facts::xml(data),
        ParseMode::Toc => crate::facts::toc(data),
    }
}

#[test]
fn typed_worker_tasks_preserve_mode_and_the_original_allocation() {
    let control = OperationControl::default();
    let sources = [
        ("Source.LUA", b"function Exact() Call() end".as_slice()),
        (
            "Source.XML",
            b"<Ui><Script file=\"Exact.lua\"/></Ui>".as_slice(),
        ),
        (
            "Source.TOC",
            b"## Interface: 120100\nExact.lua\n".as_slice(),
        ),
        (
            "Interface/Blizzard_APIDocumentationGenerated/Source.lua",
            b"local T={Name='Exact',Type='System'}".as_slice(),
        ),
    ];
    let inputs = sources
        .iter()
        .map(|(path, raw)| {
            let mut data = Vec::with_capacity(1024);
            data.extend_from_slice(raw);
            (*path, data)
        })
        .collect::<Vec<_>>();
    let owners = inputs
        .iter()
        .map(|(path, data)| {
            (
                *path,
                data.as_ptr() as usize,
                data.capacity(),
                ParseMode::for_path(path),
                original_facts(ParseMode::for_path(path), data),
            )
        })
        .collect::<Vec<_>>();
    let jobs = AtomicUsize::new(0);
    with_workers(
        2,
        |_, input| {
            jobs.fetch_add(1, Ordering::SeqCst);
            let original = input.data.as_ptr() as usize;
            let capacity = input.data.capacity();
            let mode = input.mode;
            let output = prepare_input(input, &control, false);
            assert_eq!(output.mode, mode);
            assert_eq!(output.data.as_ptr() as usize, original);
            assert_eq!(output.data.capacity(), capacity);
            assert_eq!(output.parse_elapsed, Duration::ZERO);
            output
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut ordinal = 0;
            let mut consume =
                |file: &git::TreeFile, data: Vec<u8>, prepared: Option<PreparedFacts>| {
                    let (path, original, capacity, mode, expected) = &owners[ordinal];
                    assert_eq!(file.path, *path);
                    assert_eq!(data.as_ptr() as usize, *original);
                    assert_eq!(data.capacity(), *capacity);
                    let prepared = prepared.expect("supported mode must return owned facts");
                    assert_eq!(prepared.mode, *mode);
                    assert_eq!(prepared.parse_elapsed, Duration::ZERO);
                    let facts = prepared.facts;
                    assert_eq!(
                        serde_json::to_vec(&facts).unwrap(),
                        serde_json::to_vec(expected).unwrap()
                    );
                    assert_eq!(
                        facts.raw_relationship_targets,
                        expected.raw_relationship_targets
                    );
                    ordinal += 1;
                    Ok(())
                };
            for (path, data) in inputs {
                p.push(&file(path, data.len()), data, &mut consume).unwrap();
            }
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(ordinal, owners.len());
            assert_eq!(p.pending_bytes, 0);
        },
    );
    assert_eq!(jobs.load(Ordering::SeqCst), 4);
}

#[test]
fn real_xml_and_toc_workers_overlap_with_full_facts_in_source_order() {
    let control = OperationControl::default();
    let barrier = (Mutex::new(0usize), Condvar::new());
    let modes = Mutex::new(Vec::new());
    let sources = [
        ("First.XML", b"<Ui><Frame name=\"Owner\"><Scripts><OnLoad>Run()</OnLoad></Scripts></Frame><Script file=\"Full.lua\"/></Ui>".as_slice(), ParseMode::Xml),
        ("Second.TOC", b"## Interface: 120100\n## Title: Complete\nFull.lua\n".as_slice(), ParseMode::Toc),
    ];
    with_workers(
        2,
        |_, input| {
            modes.lock().unwrap().push(input.mode);
            let mut arrived = barrier.0.lock().unwrap();
            *arrived += 1;
            barrier.1.notify_all();
            let (arrived, timeout) = barrier
                .1
                .wait_timeout_while(arrived, Duration::from_secs(5), |n| *n < 2)
                .unwrap();
            assert!(
                !timeout.timed_out() && *arrived == 2,
                "XML and TOC must both enter real worker tasks before ordered consumption"
            );
            drop(arrived);
            prepare_input(input, &control, true)
        },
        |workers| {
            let profile = Profile::enabled(true);
            let mut p = pipeline(workers, &profile);
            let mut ordinal = 0;
            let mut consume =
                |file: &git::TreeFile, data: Vec<u8>, prepared: Option<PreparedFacts>| {
                    let (path, raw, mode) = sources[ordinal];
                    assert_eq!(file.path, path);
                    assert_eq!(data, raw);
                    let prepared = prepared.expect("XML/TOC must have real worker facts");
                    assert_eq!(prepared.mode, mode);
                    let expected = original_facts(mode, raw);
                    assert_eq!(
                        serde_json::to_vec(&prepared.facts).unwrap(),
                        serde_json::to_vec(&expected).unwrap()
                    );
                    assert_eq!(
                        prepared.facts.raw_relationship_targets,
                        expected.raw_relationship_targets
                    );
                    ordinal += 1;
                    Ok(())
                };
            for (path, raw, _) in sources {
                p.push(&file(path, raw.len()), raw.to_vec(), &mut consume)
                    .unwrap();
            }
            assert_eq!(p.pending.len(), 2);
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(ordinal, 2);
            assert_eq!(profile.calls(Phase::FactsWorker), 2);
            assert_eq!(profile.calls(Phase::FactsWorkerAbandoned), 0);
            assert_eq!(p.pending_bytes, 0);
        },
    );
    let modes = modes.into_inner().unwrap();
    assert_eq!(modes.len(), 2);
    assert!(modes.contains(&ParseMode::Xml));
    assert!(modes.contains(&ParseMode::Toc));
}

#[test]
fn cancelled_or_expired_xml_toc_jobs_abandon_and_preserve_the_original_stop() {
    for expired in [false, true] {
        let control = if expired {
            OperationControl::uncancelable(Instant::now())
        } else {
            let control = OperationControl::default();
            control.tracked_cancel_handle().unwrap().cancel();
            control
        };
        let completed = AtomicUsize::new(0);
        with_workers(
            2,
            |_, input| {
                let output = prepare_input(input, &control, false);
                assert!(output.facts.is_none());
                completed.fetch_add(1, Ordering::SeqCst);
                output
            },
            |workers| {
                let profile = Profile::enabled(true);
                let mut p = pipeline(workers, &profile);
                let mut consumed = Vec::new();
                let mut consume =
                    |file: &git::TreeFile, _: Vec<u8>, prepared: Option<PreparedFacts>| {
                        assert!(prepared.is_none());
                        consumed.push(file.path.clone());
                        control.check()
                    };
                p.push(&file("First.xml", 5), b"<Ui/>".to_vec(), &mut consume)
                    .unwrap();
                p.push(&file("Second.toc", 9), b"Full.lua\n".to_vec(), &mut consume)
                    .unwrap();
                let error = p.finish(Ok(1), &mut consume).unwrap_err();
                assert_eq!(
                    error.code,
                    if expired {
                        "command.deadline"
                    } else {
                        "command.cancelled"
                    }
                );
                assert_eq!(consumed, ["First.xml"]);
                assert_eq!(profile.calls(Phase::FactsWorkerAbandoned), 1);
                assert_eq!(profile.calls(Phase::FactsRefill), 0);
            },
        );
        assert_eq!(completed.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn all_parser_modes_keep_the_existing_worker_resource_envelope() {
    assert_eq!(MAX_SOURCE_BYTES, 64 << 10);
    assert_eq!(MAX_FACT_BYTES, 4 << 20);
    assert_eq!(PARSER_SCRATCH_BYTES, 32 << 20);
    assert_eq!(WORKER_STACK_BYTES, 2 << 20);
    assert_eq!(SOURCE_RETAINED_LIMIT, 256 << 20);
    assert_eq!(MAX_PENDING_SLOTS, 8);
    for path in ["Source.lua", "Source.xml", "Source.toc"] {
        let mut data = Vec::with_capacity(MAX_SOURCE_BYTES * 2);
        data.extend_from_slice(b"x");
        assert!(eligible(&file(path, data.len()), &data));
        assert_eq!(
            reservation(&data),
            Some(data.capacity() + MAX_FACT_BYTES + PARSER_SCRATCH_BYTES + JOB_METADATA_BYTES)
        );
    }
    assert_eq!(reservation_for_capacity(usize::MAX), None);
}

#[test]
fn reservations_share_retained_source_and_both_git_buffers() {
    let data = vec![b'x'; MAX_SOURCE_BYTES];
    let reserved = reservation(&data).unwrap();
    let fixed = GIT_BATCH_BYTES + MAX_SOURCE_BYTES + 4 * WORKER_STACK_BYTES;
    let remaining = SOURCE_RETAINED_LIMIT - fixed - reserved;
    assert!(fits(
        remaining,
        0,
        reserved,
        4,
        MAX_SOURCE_BYTES,
        GIT_BATCH_BYTES
    ));
    assert!(!fits(
        remaining + 1,
        0,
        reserved,
        4,
        MAX_SOURCE_BYTES,
        GIT_BATCH_BYTES
    ));
    assert!(!fits(
        usize::MAX,
        0,
        reserved,
        4,
        MAX_SOURCE_BYTES,
        GIT_BATCH_BYTES
    ));
    assert!(!fits(
        0,
        usize::MAX,
        reserved,
        4,
        MAX_SOURCE_BYTES,
        GIT_BATCH_BYTES
    ));
    assert!(fits(
        0,
        3 * reserved,
        reserved,
        4,
        MAX_SOURCE_BYTES,
        GIT_BATCH_BYTES
    ));
}

#[test]
fn two_workers_really_overlap_but_results_commit_in_source_order() {
    let barrier = (Mutex::new(0usize), Condvar::new());
    let reached = AtomicUsize::new(0);
    with_workers(
        2,
        |_, input| {
            reached.fetch_add(1, Ordering::SeqCst);
            let mut arrived = barrier.0.lock().unwrap();
            *arrived += 1;
            barrier.1.notify_all();
            let (arrived, timeout) = barrier
                .1
                .wait_timeout_while(arrived, Duration::from_secs(5), |n| *n < 2)
                .unwrap();
            assert!(
                !timeout.timed_out() && *arrived == 2,
                "both workers must run before ordered consumption"
            );
            empty(input)
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut committed = Vec::new();
            let mut consume =
                |file: &git::TreeFile, _: Vec<u8>, prepared: Option<PreparedFacts>| {
                    assert!(prepared.is_some());
                    committed.push(file.path.clone());
                    Ok(())
                };
            p.push(&file("0.lua", 1), vec![b'a'], &mut consume).unwrap();
            p.push(&file("1.lua", 1), vec![b'b'], &mut consume).unwrap();
            assert_eq!(p.pending.len(), 2);
            assert_eq!(p.finish(Ok(1), &mut consume).unwrap(), 1);
            assert_eq!(committed, ["0.lua", "1.lua"]);
            assert_eq!(reached.load(Ordering::SeqCst), 2);
            assert_eq!(p.pending_bytes, 0);
        },
    );
}

#[test]
fn admitted_refill_starts_before_the_earlier_consumer_returns() {
    let (started, entered) = mpsc::sync_channel(1);
    with_workers(
        2,
        |_, input| {
            if input.data == b"c" {
                started.send(()).unwrap();
            }
            empty(input)
        },
        |workers| {
            let profile = Profile::enabled(true);
            let mut p = pipeline(workers, &profile);
            let mut order = Vec::new();
            let mut consume = |f: &git::TreeFile, _: Vec<u8>, prepared: Option<PreparedFacts>| {
                assert!(prepared.is_some());
                if f.path == "0.lua" {
                    entered.recv_timeout(Duration::from_secs(5)).expect(
                        "next pure parse must start before synchronous ordered publication returns",
                    );
                }
                order.push(f.path.clone());
                Ok(())
            };
            for (ordinal, byte) in [b'a', b'b', b'c'].into_iter().enumerate() {
                p.push(
                    &file(&format!("{ordinal}.lua"), 1),
                    vec![byte],
                    &mut consume,
                )
                .unwrap();
            }
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(order, ["0.lua", "1.lua", "2.lua"]);
            assert_eq!(profile.calls(Phase::FactsRefill), 1);
            assert_eq!(profile.calls(Phase::FactsWorkerWait), 3);
            assert_eq!(p.pending_bytes, 0);
            assert_eq!(p.retained_bytes, 3);
        },
    );
}

#[test]
fn refill_counts_ready_facts_and_incoming_scratch_at_the_exact_byte_boundary() {
    for (extra_retained, expected_refills) in [(0, 1), (1, 0)] {
        with_workers(
            2,
            |_, input| empty(input),
            |workers| {
                let profile = Profile::enabled(true);
                let mut p = pipeline(workers, &profile);
                let job = reservation_for_capacity(1).unwrap();
                let ready = job - PARSER_SCRATCH_BYTES;
                let fixed = p.batch_bytes + p.incoming_bytes + 2 * WORKER_STACK_BYTES;
                p.retained_bytes =
                    SOURCE_RETAINED_LIMIT - fixed - (2 * job + ready) + extra_retained;
                let mut consume = |_: &git::TreeFile, _: Vec<u8>, _: Option<PreparedFacts>| Ok(());
                for ordinal in 0..3 {
                    p.push(
                        &file(&format!("{ordinal}.lua"), 1),
                        vec![b'x'],
                        &mut consume,
                    )
                    .unwrap();
                    assert!(fits(
                        p.retained_bytes,
                        p.pending_bytes,
                        0,
                        2,
                        p.incoming_bytes,
                        p.batch_bytes
                    ));
                }
                assert_eq!(profile.calls(Phase::FactsRefill), expected_refills);
                p.finish(Ok(1), &mut consume).unwrap();
                assert_eq!(p.pending_bytes, 0);
            },
        );
    }
}

#[test]
fn ready_owner_keeps_its_slot_when_the_mixed_queue_is_full() {
    with_workers(
        4,
        |_, input| empty(input),
        |workers| {
            let profile = Profile::enabled(true);
            let mut p = pipeline(workers, &profile);
            let mut order = Vec::new();
            let mut consume = |f: &git::TreeFile, raw: Vec<u8>, prepared: Option<PreparedFacts>| {
                assert_eq!(prepared.is_some(), std::str::from_utf8(&raw).is_ok());
                if f.path == "0.lua" {
                    assert_eq!(
                        profile.calls(Phase::FactsRefill),
                        0,
                        "the ready owner still occupies the eighth slot during consumption"
                    );
                }
                order.push(f.path.clone());
                Ok(())
            };
            for ordinal in 0..4 {
                p.push(
                    &file(&format!("{ordinal}.lua"), 1),
                    vec![b'x'],
                    &mut consume,
                )
                .unwrap();
            }
            // Generated metadata is now eligible. Deliberately invalid UTF-8
            // supplies real small Sequential owners without changing admission.
            for ordinal in 4..8 {
                let file = file(
                    &format!("Interface/Blizzard_APIDocumentationGenerated/{ordinal}.lua"),
                    1,
                );
                let raw = vec![0xff];
                assert!(!eligible(&file, &raw));
                p.push(&file, raw, &mut consume).unwrap();
            }
            assert_eq!(p.pending.len(), MAX_PENDING_SLOTS);
            assert_eq!(
                p.pending
                    .iter()
                    .filter(|pending| matches!(&pending.preparation, Preparation::Sequential(_)))
                    .count(),
                4
            );
            p.push(&file("8.lua", 1), vec![b'x'], &mut consume).unwrap();
            assert_eq!(profile.calls(Phase::FactsRefill), 0);
            assert_eq!(p.pending.len(), MAX_PENDING_SLOTS);
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(
                order,
                [
                    "0.lua",
                    "1.lua",
                    "2.lua",
                    "3.lua",
                    "Interface/Blizzard_APIDocumentationGenerated/4.lua",
                    "Interface/Blizzard_APIDocumentationGenerated/5.lua",
                    "Interface/Blizzard_APIDocumentationGenerated/6.lua",
                    "Interface/Blizzard_APIDocumentationGenerated/7.lua",
                    "8.lua"
                ]
            );
        },
    );
}

#[test]
fn abandoned_results_keep_the_complete_path_and_do_not_trigger_refill() {
    with_workers(
        2,
        |_, input| ParseOutput {
            mode: input.mode,
            data: input.data,
            facts: None,
            parse_elapsed: Duration::ZERO,
        },
        |workers| {
            let profile = Profile::enabled(true);
            let mut p = pipeline(workers, &profile);
            let mut consumed = 0;
            let mut consume = |_: &git::TreeFile, _: Vec<u8>, prepared: Option<PreparedFacts>| {
                assert!(prepared.is_none());
                consumed += 1;
                Ok(())
            };
            for ordinal in 0..3 {
                p.push(
                    &file(&format!("{ordinal}.lua"), 1),
                    vec![b'x'],
                    &mut consume,
                )
                .unwrap();
            }
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(consumed, 3);
            assert_eq!(profile.calls(Phase::FactsWorkerAbandoned), 3);
            assert_eq!(profile.calls(Phase::FactsRefill), 0);
            assert_eq!(p.pending_bytes, 0);
        },
    );
}

#[test]
fn early_refill_never_consumes_after_the_first_error_and_all_jobs_finish() {
    let completed = AtomicUsize::new(0);
    with_workers(
        2,
        |_, input| {
            completed.fetch_add(1, Ordering::SeqCst);
            empty(input)
        },
        |workers| {
            let profile = Profile::enabled(true);
            let mut p = pipeline(workers, &profile);
            let mut consumed = Vec::new();
            let mut consume = |f: &git::TreeFile, _: Vec<u8>, _: Option<PreparedFacts>| {
                consumed.push(f.path.clone());
                Err(ToolkitError::new("test.first", "original ordered error", 4))
            };
            p.push(&file("0.lua", 1), vec![b'a'], &mut consume).unwrap();
            p.push(&file("1.lua", 1), vec![b'b'], &mut consume).unwrap();
            let error = p
                .push(&file("2.lua", 1), vec![b'c'], &mut consume)
                .unwrap_err();
            assert_eq!(error.code, "test.first");
            assert_eq!(profile.calls(Phase::FactsRefill), 1);
            assert_eq!(consumed, ["0.lua"]);
        },
    );
    assert_eq!(completed.load(Ordering::SeqCst), 3);
}

#[test]
fn early_refill_preserves_the_original_typed_cancellation() {
    let control = OperationControl::default();
    let token = control.cancel_handle();
    let completed = AtomicUsize::new(0);
    with_workers(
        2,
        |_, input| {
            completed.fetch_add(1, Ordering::SeqCst);
            empty(input)
        },
        |workers| {
            let profile = Profile::enabled(true);
            let mut p = pipeline(workers, &profile);
            let mut calls = 0;
            let mut consume = |_: &git::TreeFile, _: Vec<u8>, _: Option<PreparedFacts>| {
                calls += 1;
                token.store(true, Ordering::Release);
                control.check()
            };
            p.push(&file("0.lua", 1), vec![b'a'], &mut consume).unwrap();
            p.push(&file("1.lua", 1), vec![b'b'], &mut consume).unwrap();
            let error = p
                .push(&file("2.lua", 1), vec![b'c'], &mut consume)
                .unwrap_err();
            assert_eq!(error.code, "command.cancelled");
            assert_eq!(calls, 1);
            assert_eq!(profile.calls(Phase::FactsRefill), 1);
        },
    );
    assert_eq!(completed.load(Ordering::SeqCst), 3);
}

#[test]
fn refill_send_failure_is_deferred_until_the_earlier_consumer_selects_its_result() {
    for earlier_fails in [false, true] {
        let mut workers = Vec::new();
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for _ in 0..2 {
            let (input, receive) = mpsc::sync_channel(1);
            let (send, output) = mpsc::sync_channel(1);
            workers.push(Worker { input, output });
            inputs.push(receive);
            outputs.push(send);
        }
        let profile = Profile::enabled(true);
        let mut p = pipeline(workers, &profile);
        let mut calls = 0;
        let mut consume = |_: &git::TreeFile, _: Vec<u8>, _: Option<PreparedFacts>| {
            calls += 1;
            if earlier_fails {
                Err(ToolkitError::new("test.first", "original ordered error", 4))
            } else {
                Ok(())
            }
        };
        p.push(&file("0.lua", 1), vec![b'a'], &mut consume).unwrap();
        p.push(&file("1.lua", 1), vec![b'b'], &mut consume).unwrap();
        let first = inputs.remove(0);
        assert!(outputs[0].send(empty(first.recv().unwrap())).is_ok());
        drop(first); // Its completed result is valid, but refill cannot send.
        let error = p
            .push(&file("2.lua", 1), vec![b'c'], &mut consume)
            .unwrap_err();
        assert_eq!(
            error.code,
            if earlier_fails {
                "test.first"
            } else {
                "codebase.source_worker_failed"
            }
        );
        assert_eq!(calls, 1);
        assert_eq!(profile.calls(Phase::FactsRefill), 0);
    }
}

#[test]
fn a_later_ready_result_cannot_commit_ahead_of_the_first() {
    let (released, wait) = mpsc::sync_channel(1);
    let wait = std::sync::Mutex::new(wait);
    with_workers(
        2,
        |ordinal, input| {
            if ordinal == 0 {
                wait.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            } else {
                released.send(()).unwrap();
            }
            empty(input)
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut order = Vec::new();
            let mut consume = |f: &git::TreeFile, _: Vec<u8>, _: Option<PreparedFacts>| {
                order.push(f.path.clone());
                Ok(())
            };
            p.push(&file("first.lua", 1), vec![b'a'], &mut consume)
                .unwrap();
            p.push(&file("second.lua", 1), vec![b'b'], &mut consume)
                .unwrap();
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(order, ["first.lua", "second.lua"]);
        },
    );
}

#[test]
fn an_earlier_document_error_wins_over_a_later_git_error() {
    with_workers(
        2,
        |_, input| empty(input),
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut called = 0;
            let mut consume = |_: &git::TreeFile, _: Vec<u8>, _: Option<PreparedFacts>| {
                called += 1;
                Err(ToolkitError::new(
                    "test.first",
                    "original first document",
                    4,
                ))
            };
            p.push(&file("first.lua", 1), vec![b'a'], &mut consume)
                .unwrap();
            p.push(&file("second.lua", 1), vec![b'b'], &mut consume)
                .unwrap();
            let error = p
                .finish(
                    Err(ToolkitError::new("test.later_git", "later", 4)),
                    &mut consume,
                )
                .unwrap_err();
            assert_eq!(error.code, "test.first");
            assert_eq!(called, 1);
        },
    );
}

#[test]
fn a_callback_error_never_commits_remaining_speculative_documents() {
    with_workers(
        2,
        |_, input| empty(input),
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut called = 0;
            let mut consume = |_: &git::TreeFile, _: Vec<u8>, _: Option<PreparedFacts>| {
                called += 1;
                Err(ToolkitError::new("test.first", "first", 4))
            };
            p.push(&file("first.lua", 1), vec![b'a'], &mut consume)
                .unwrap();
            p.push(&file("second.lua", 1), vec![b'b'], &mut consume)
                .unwrap();
            let error = p
                .push(
                    &file("third.xml", MAX_SOURCE_BYTES + 1),
                    vec![b'c'; MAX_SOURCE_BYTES + 1],
                    &mut consume,
                )
                .unwrap_err();
            p.failed = true;
            assert_eq!(
                p.finish(Err(error), &mut consume).unwrap_err().code,
                "test.first"
            );
            assert_eq!(called, 1);
        },
    );
}

#[test]
fn small_xml_prepares_complete_facts_for_its_ordered_consumer() {
    let control = OperationControl::default();
    with_workers(
        2,
        |_, input| prepare_input(input, &control, false),
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut seen = Vec::new();
            let mut consume =
                |f: &git::TreeFile, bytes: Vec<u8>, prepared: Option<PreparedFacts>| {
                    let prepared = prepared.expect("small XML and ordinary Lua are worker tasks");
                    assert_eq!(prepared.mode, ParseMode::for_path(&f.path));
                    if prepared.mode == ParseMode::Xml {
                        assert_eq!(prepared.facts.loads[0].path, "Full.lua");
                    }
                    seen.push((f.path.clone(), bytes, prepared.mode));
                    Ok(())
                };
            let lua = b"function First() end".to_vec();
            p.push(&file("first.lua", lua.len()), lua.clone(), &mut consume)
                .unwrap();
            let xml = b"<Ui><Script file=\"Full.lua\"/></Ui>".to_vec();
            p.push(&file("second.xml", xml.len()), xml.clone(), &mut consume)
                .unwrap();
            assert_eq!(p.pending.len(), 2);
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(
                seen,
                [
                    ("first.lua".into(), lua, ParseMode::LuaOrdinary),
                    ("second.xml".into(), xml, ParseMode::Xml)
                ]
            );
        },
    );
}

#[test]
fn exhausted_speculative_allowance_uses_the_original_complete_path() {
    let jobs = AtomicUsize::new(0);
    with_workers(
        2,
        |_, input| {
            jobs.fetch_add(1, Ordering::SeqCst);
            empty(input)
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            p.retained_bytes = SOURCE_RETAINED_LIMIT - GIT_BATCH_BYTES;
            let raw = b"function Complete() return 7 end".to_vec();
            let expected = serde_json::to_value(crate::facts::lua(&raw, false)).unwrap();
            let mut consume =
                |_: &git::TreeFile, data: Vec<u8>, prepared: Option<PreparedFacts>| {
                    assert!(prepared.is_none());
                    assert_eq!(
                        serde_json::to_value(crate::facts::lua(&data, false)).unwrap(),
                        expected
                    );
                    Ok(())
                };
            p.push(&file("full.lua", raw.len()), raw, &mut consume)
                .unwrap();
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(jobs.load(Ordering::SeqCst), 0);
        },
    );
}

#[test]
fn xml_toc_ineligible_inputs_and_exhausted_allowance_keep_full_original_facts() {
    for (path, mode, valid, invalid) in [
        (
            "Full.xml",
            ParseMode::Xml,
            b"<Ui><Script file=\"Full.lua\"/></Ui>".as_slice(),
            b"<Ui><Script file=\"\xff.lua\"/></Ui>".as_slice(),
        ),
        (
            "Full.toc",
            ParseMode::Toc,
            b"## Title: Complete\nFull.lua\n".as_slice(),
            b"## Title: \xff\nFull.lua\n".as_slice(),
        ),
    ] {
        for scenario in 0..3 {
            let jobs = AtomicUsize::new(0);
            let mut raw = if scenario == 0 {
                invalid.to_vec()
            } else {
                valid.to_vec()
            };
            if scenario == 1 {
                raw.resize(MAX_SOURCE_BYTES + 1, b' ');
            }
            let original = raw.as_ptr() as usize;
            let capacity = raw.capacity();
            let expected = original_facts(mode, &raw);
            with_workers(
                2,
                |_, input| {
                    jobs.fetch_add(1, Ordering::SeqCst);
                    empty(input)
                },
                |workers| {
                    let profile = Profile::new();
                    let mut p = pipeline(workers, &profile);
                    if scenario == 2 {
                        p.retained_bytes = SOURCE_RETAINED_LIMIT - GIT_BATCH_BYTES;
                    }
                    let mut consumed = 0;
                    let mut consume =
                        |file: &git::TreeFile, data: Vec<u8>, prepared: Option<PreparedFacts>| {
                            assert_eq!(file.path, path);
                            assert!(prepared.is_none());
                            assert_eq!(data.as_ptr() as usize, original);
                            assert_eq!(data.capacity(), capacity);
                            let actual = original_facts(mode, &data);
                            assert_eq!(
                                serde_json::to_vec(&actual).unwrap(),
                                serde_json::to_vec(&expected).unwrap()
                            );
                            assert_eq!(
                                actual.raw_relationship_targets,
                                expected.raw_relationship_targets
                            );
                            consumed += 1;
                            Ok(())
                        };
                    p.push(&file(path, raw.len()), raw, &mut consume).unwrap();
                    p.finish(Ok(1), &mut consume).unwrap();
                    assert_eq!(consumed, 1);
                    assert_eq!(p.pending_bytes, 0);
                },
            );
            assert_eq!(
                jobs.load(Ordering::SeqCst),
                0,
                "{path}: scenario {scenario}"
            );
        }
    }
}

#[test]
fn an_abandoned_pure_result_is_reparsed_without_public_truncation() {
    with_workers(
        2,
        |_, input| ParseOutput {
            mode: input.mode,
            data: input.data,
            facts: None,
            parse_elapsed: Duration::ZERO,
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let raw = b"function Full() Call() end".to_vec();
            let expected = serde_json::to_value(crate::facts::lua(&raw, false)).unwrap();
            let mut consume =
                |_: &git::TreeFile, data: Vec<u8>, prepared: Option<PreparedFacts>| {
                    assert!(prepared.is_none());
                    assert_eq!(
                        serde_json::to_value(crate::facts::lua(&data, false)).unwrap(),
                        expected
                    );
                    Ok(())
                };
            p.push(&file("full.lua", raw.len()), raw, &mut consume)
                .unwrap();
            p.finish(Ok(1), &mut consume).unwrap();
        },
    );
}

#[test]
fn cancellation_is_selected_by_the_original_control_and_all_jobs_are_joined() {
    let control = OperationControl::default();
    let cancelled = control.tracked_cancel_handle().unwrap();
    let completed = Arc::new(AtomicUsize::new(0));
    with_workers(
        2,
        |_, input| {
            completed.fetch_add(1, Ordering::SeqCst);
            empty(input)
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut consumed = 0;
            let mut consume = |_: &git::TreeFile, _: Vec<u8>, _: Option<PreparedFacts>| {
                consumed += 1;
                control.check()
            };
            p.push(&file("0.lua", 1), vec![b'a'], &mut consume).unwrap();
            p.push(&file("1.lua", 1), vec![b'b'], &mut consume).unwrap();
            cancelled.cancel();
            assert_eq!(
                p.finish(Ok(1), &mut consume).unwrap_err().code,
                "command.cancelled"
            );
            assert_eq!(consumed, 1);
        },
    );
    assert_eq!(completed.load(Ordering::SeqCst), 2);
}

#[test]
fn slot_backpressure_never_grows_a_result_queue_past_worker_count() {
    let jobs = AtomicUsize::new(0);
    with_workers(
        2,
        |_, input| {
            jobs.fetch_add(1, Ordering::SeqCst);
            empty(input)
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut committed = Vec::new();
            let mut consume = |f: &git::TreeFile, _: Vec<u8>, _: Option<PreparedFacts>| {
                committed.push(f.path.clone());
                Ok(())
            };
            for ordinal in 0..17 {
                p.push(
                    &file(&format!("{ordinal}.lua"), 1),
                    vec![b'a'],
                    &mut consume,
                )
                .unwrap();
                assert!(p.pending.len() <= 2);
                assert!(fits(
                    p.retained_bytes,
                    p.pending_bytes,
                    0,
                    2,
                    MAX_SOURCE_BYTES,
                    p.batch_bytes
                ));
            }
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(
                committed,
                (0..17).map(|i| format!("{i}.lua")).collect::<Vec<_>>()
            );
            assert_eq!(jobs.load(Ordering::SeqCst), 17);
            assert_eq!(p.retained_bytes, 17);
            assert_eq!(p.pending_bytes, 0);
        },
    );
}

#[test]
fn an_existing_public_error_is_not_replaced_during_worker_join() {
    let original = Err::<usize, _>(ToolkitError::new("test.original", "publication failed", 5));
    let result = finish_historical(original, Err(worker_failed())).unwrap_err();
    assert_eq!(result.code, "test.original");
    assert_eq!(result.exit_code, 5);
}

#[test]
fn generated_lua_worker_preserves_complete_facts_and_order_among_lua_inputs() {
    let gate = (Mutex::new(0usize), Condvar::new());
    let control = OperationControl::default();
    with_workers(
        2,
        |_, input| {
            let mut arrived = gate.0.lock().unwrap();
            *arrived += 1;
            gate.1.notify_all();
            let (arrived, timeout) = gate
                .1
                .wait_timeout_while(arrived, Duration::from_secs(5), |n| *n < 2)
                .unwrap();
            assert!(!timeout.timed_out() && *arrived >= 2);
            drop(arrived);
            prepare_input(input, &control, false)
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut order = Vec::new();
            let mut consume =
                |f: &git::TreeFile, data: Vec<u8>, prepared: Option<PreparedFacts>| {
                    let prepared = prepared
                        .expect("small generated documents now share the full guarded parser");
                    assert_eq!(prepared.mode, ParseMode::for_path(&f.path));
                    let expected = original_facts(prepared.mode, &data);
                    assert_eq!(
                        serde_json::to_vec(&prepared.facts).unwrap(),
                        serde_json::to_vec(&expected).unwrap()
                    );
                    assert_eq!(
                        prepared.facts.raw_relationship_targets,
                        expected.raw_relationship_targets
                    );
                    order.push(f.path.clone());
                    Ok(())
                };
            let generated = b"local T={Name=\"Full\", Type=\"System\", Namespace=\"C_Full\", Functions={{Name=\"Run\",Type=\"Function\"}}}\nCall()".to_vec();
            let generated_capacity = generated.capacity();
            let generated_path = "Interface/Blizzard_APIDocumentationGenerated/middle.lua";
            p.push(&file("first.lua", 7), b"First()".to_vec(), &mut consume)
                .unwrap();
            let previous = p.pending_bytes;
            p.push(
                &file(generated_path, generated.len()),
                generated,
                &mut consume,
            )
            .unwrap();
            assert_eq!(
                p.pending_bytes - previous,
                reservation_for_capacity(generated_capacity).unwrap()
            );
            p.push(&file("last.lua", 6), b"Last()".to_vec(), &mut consume)
                .unwrap();
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(order, ["first.lua", generated_path, "last.lua"]);
            assert_eq!(p.pending_bytes, 0);
        },
    );
}

fn real_mixed_parser_run(cpu_limit: usize) -> (usize, Vec<Facts>) {
    // This fixture reserves a small transport allocation, so a CPU limit of
    // eight can admit six complete workers under the same 256 MiB allowance. Eight actual workers cannot fit the unchanged scratch policy.
    let batch_bytes = 1 << 20;
    let worker_count = adaptive_worker_count(cpu_limit, 0, MAX_SOURCE_BYTES, batch_bytes);
    assert!(worker_count >= 2);
    let mut observed = Vec::new();
    let control = OperationControl::default();
    let active = AtomicUsize::new(0);
    let peak_active = AtomicUsize::new(0);
    let entered = AtomicUsize::new(0);
    let first_wave = (Mutex::new(0usize), Condvar::new());
    let mut inputs = (0..8)
        .map(|i| {
            (
                match i % 3 {
                    0 => format!("Initial{i}.lua"),
                    1 => format!("Initial{i}.xml"),
                    _ => format!("Initial{i}.toc"),
                },
                match i % 3 {
                    0 => format!("function F{i}() RegisterEvent('EVENT_{i}') end\n").into_bytes(),
                    1 => {
                        format!("<Ui><Frame name=\"Initial{i}\"/><Script file=\"Full.lua\"/></Ui>")
                            .into_bytes()
                    }
                    _ => format!("## Title: Initial{i}\nFull.lua\n").into_bytes(),
                },
            )
        })
        .collect::<Vec<_>>();
    // Retain every original four-mode worker input. These four additional
    // non-UTF8 Lua documents exercise the genuine ordered serial route, making
    // the mixed queue reach eight slots at both CPU limits without extra budget.
    for i in 0..4 {
        inputs.push((
            format!("Sequential/InvalidUtf8{i}.lua"),
            b"Before(); \xff".to_vec(),
        ));
    }
    for i in 0..8 {
        inputs.push((format!("{i}.xml"), b"<Ui><Frame name=\"Owner\"><Scripts><OnLoad>Run()</OnLoad></Scripts></Frame><Script file=\"Full.lua\"/></Ui>".to_vec()));
        inputs.push((
            format!("{i}.toc"),
            b"## Interface: 120100\nFull.lua\n".to_vec(),
        ));
        inputs.push((format!("{i}.lua"), b"function Exact() Call(1) end\nlocal function Nested() Exact() end\nFrame:RegisterEvent('\\255')".to_vec()));
        inputs.push((
            format!("Interface/Blizzard_APIDocumentationGenerated/{i}.lua"),
            b"local T = {Name='Test', Type='System', Namespace='C_Test'}\n".to_vec(),
        ));
    }
    let parse = |path: &str, data: &[u8]| {
        if path.ends_with(".lua") {
            crate::facts::lua(data, path.contains("/Blizzard_APIDocumentationGenerated/"))
        } else if path.ends_with(".toc") {
            crate::facts::toc(data)
        } else {
            crate::facts::xml(data)
        }
    };
    let expected = inputs
        .iter()
        .map(|(path, data)| parse(path, data))
        .collect::<Vec<_>>();
    with_workers(
        worker_count,
        |_, input| {
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak_active.fetch_max(now, Ordering::SeqCst);
            if entered.fetch_add(1, Ordering::SeqCst) < worker_count {
                let mut count = first_wave.0.lock().unwrap();
                *count += 1;
                first_wave.1.notify_all();
                let (count, timeout) = first_wave
                    .1
                    .wait_timeout_while(count, Duration::from_secs(5), |n| *n < worker_count)
                    .unwrap();
                assert!(!timeout.timed_out() && *count == worker_count);
            }
            let output = prepare_input(input, &control, true);
            active.fetch_sub(1, Ordering::SeqCst);
            output
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            p.batch_bytes = batch_bytes;
            let mut ordinal = 0usize;
            let mut prepared_documents = 0;
            let mut sequential_documents = 0;
            let mut prepared_modes = [0usize; 4];
            let mut max_slots = 0;
            let mut max_owned_reservation = 0;
            let mut consume = |f: &git::TreeFile, raw: Vec<u8>, prepared: Option<PreparedFacts>| {
                assert_eq!(f.path, inputs[ordinal].0);
                assert_eq!(raw, inputs[ordinal].1);
                let mode = ParseMode::for_path(&f.path);
                let serial = f.path.starts_with("Sequential/InvalidUtf8");
                assert_eq!(std::str::from_utf8(&raw).is_err(), serial);
                assert_eq!(eligible(f, &raw), !serial);
                assert_eq!(
                    prepared.is_some(),
                    !serial,
                    "all four bounded UTF-8 modes are guarded; explicit non-UTF8 inputs stay serial: {}",
                    f.path
                );
                if let Some(prepared) = &prepared {
                    assert_eq!(prepared.mode, mode);
                    prepared_modes[match mode {
                        ParseMode::LuaOrdinary => 0,
                        ParseMode::LuaGenerated => 1,
                        ParseMode::Xml => 2,
                        ParseMode::Toc => 3,
                    }] += 1;
                }
                prepared_documents += usize::from(prepared.is_some());
                sequential_documents += usize::from(serial);
                let facts =
                    prepared.map_or_else(|| parse(&f.path, &raw), |prepared| prepared.facts);
                if serial {
                    assert!(facts.declarations.is_empty());
                    assert!(facts.relationships.is_empty());
                    assert_eq!(facts.diagnostics.len(), 1);
                    assert_eq!(facts.diagnostics[0].line, 0);
                    assert!(
                        facts.diagnostics[0]
                            .message
                            .contains("Lua structural input must be UTF-8 source without NUL")
                    );
                }
                assert_eq!(
                    serde_json::to_value(&facts).unwrap(),
                    serde_json::to_value(&expected[ordinal]).unwrap()
                );
                assert_eq!(
                    facts.raw_relationship_targets,
                    expected[ordinal].raw_relationship_targets
                );
                observed.push(facts);
                ordinal += 1;
                Ok(())
            };
            for (path, raw) in &inputs {
                p.push(&file(path, raw.len()), raw.clone(), &mut consume)
                    .unwrap();
                max_slots = max_slots.max(p.pending.len());
                if path == "Sequential/InvalidUtf83.lua" {
                    assert_eq!(p.pending.len(), MAX_PENDING_SLOTS);
                    assert_eq!(
                        p.pending
                            .iter()
                            .filter(|pending| matches!(
                                &pending.preparation,
                                Preparation::Sequential(_)
                            ))
                            .count(),
                        4,
                        "the full queue must contain four real serial owners plus four worker owners"
                    );
                }
                max_owned_reservation = max_owned_reservation.max(p.pending_bytes);
                assert!(p.pending.len() <= MAX_PENDING_SLOTS);
                assert!(worker_count - p.free_workers.len() <= worker_count);
                assert!(fits(
                    p.retained_bytes,
                    p.pending_bytes,
                    0,
                    worker_count,
                    p.incoming_bytes,
                    p.batch_bytes
                ));
            }
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(ordinal, inputs.len());
            assert_eq!(prepared_documents, 40);
            assert_eq!(prepared_modes, [11, 8, 11, 10]);
            assert_eq!(sequential_documents, 4);
            assert_eq!(prepared_documents + sequential_documents, inputs.len());
            assert_eq!(max_slots, MAX_PENDING_SLOTS);
            assert!(
                max_owned_reservation
                    <= SOURCE_RETAINED_LIMIT - p.batch_bytes - worker_count * WORKER_STACK_BYTES
            );
            assert_eq!(p.pending_bytes, 0);
            assert_eq!(p.free_workers.len(), worker_count);
        },
    );
    assert_eq!(peak_active.load(Ordering::SeqCst), worker_count);
    assert_eq!(active.load(Ordering::SeqCst), 0);
    (worker_count, observed)
}

#[test]
fn real_mixed_parser_pipeline_preserves_full_facts_order_and_measured_bounds() {
    let (workers, _) = real_mixed_parser_run(4);
    assert_eq!(workers, 4);
}

#[test]
fn four_and_eight_cpu_limits_run_equivalent_full_facts_on_production_sized_stacks() {
    let (four_workers, four) = real_mixed_parser_run(4);
    let (eight_cpu_workers, eight) = real_mixed_parser_run(8);
    assert_eq!((four_workers, eight_cpu_workers), (4, 6));
    assert_eq!(
        serde_json::to_vec(&four).unwrap(),
        serde_json::to_vec(&eight).unwrap()
    );
    for (four, eight) in four.iter().zip(eight) {
        assert_eq!(
            four.raw_relationship_targets,
            eight.raw_relationship_targets
        );
    }
}

#[test]
fn actual_git_capacity_above_logical_length_forces_complete_fallback() {
    let jobs = AtomicUsize::new(0);
    with_workers(
        4,
        |_, input| {
            jobs.fetch_add(1, Ordering::SeqCst);
            empty(input)
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            // The transport retained more allocation than its logical output cap.
            p.batch_bytes = 240 << 20;
            let raw = b"function Original() Run() end".to_vec();
            let expected = serde_json::to_value(crate::facts::lua(&raw, false)).unwrap();
            let mut consumed = 0;
            let mut consume = |_: &git::TreeFile, raw: Vec<u8>, prepared: Option<PreparedFacts>| {
                consumed += 1;
                assert!(prepared.is_none());
                assert_eq!(
                    serde_json::to_value(crate::facts::lua(&raw, false)).unwrap(),
                    expected
                );
                Ok(())
            };
            p.push(&file("Original.lua", raw.len()), raw, &mut consume)
                .unwrap();
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(consumed, 1);
            assert_eq!(jobs.load(Ordering::SeqCst), 0);
        },
    );
}

#[test]
fn cpu_limits_one_two_four_eight_and_large_hosts_obey_the_shared_byte_limit() {
    for (cpus, expected) in [
        (0, 0),
        (1, 0),
        (2, 2),
        (4, 4),
        (8, 6),
        (128, 6),
        (usize::MAX, 6),
    ] {
        assert_eq!(
            adaptive_worker_count(cpus, 0, MAX_SOURCE_BYTES, 1 << 20),
            expected
        );
    }
    let job = reservation_for_capacity(MAX_SOURCE_BYTES).unwrap();
    // No fixed CPU ceiling or count-down scan: the immutable byte allowance
    // directly bounds even an effectively unlimited logical CPU count.
    assert_eq!(
        adaptive_worker_count(usize::MAX, 0, 0, 0),
        SOURCE_RETAINED_LIMIT / (job + WORKER_STACK_BYTES)
    );
    assert!(!fits(0, 0, 8 * job, 8, 0, 0));
    // Large real Git allocations lower the initial thread count before spawn.
    assert_eq!(
        adaptive_worker_count(8, 0, MAX_SOURCE_BYTES, GIT_BATCH_BYTES),
        5
    );
    assert_eq!(adaptive_worker_count(8, 0, MAX_SOURCE_BYTES, 128 << 20), 3);
}

#[test]
fn adaptive_initial_worker_count_uses_remaining_bytes_and_exact_boundaries() {
    let job = reservation_for_capacity(MAX_SOURCE_BYTES).unwrap();
    let incoming = MAX_SOURCE_BYTES;
    let batch = 1 << 20;
    let two_workers = 2 * (job + WORKER_STACK_BYTES);
    let remaining_for_retained = SOURCE_RETAINED_LIMIT - incoming - batch - two_workers;
    assert_eq!(
        adaptive_worker_count(8, remaining_for_retained, incoming, batch),
        2
    );
    assert_eq!(
        adaptive_worker_count(8, remaining_for_retained + 1, incoming, batch),
        0
    );
    for (retained, incoming, batch) in [
        (usize::MAX, 0, 0),
        (0, usize::MAX, 0),
        (0, 0, usize::MAX),
        (0, 64 << 20, 192 << 20),
    ] {
        assert_eq!(adaptive_worker_count(8, retained, incoming, batch), 0);
    }
}

#[test]
fn no_admitted_workers_take_the_complete_original_path_without_queueing() {
    for cpu_limit in [1, 8] {
        let batch = if cpu_limit == 1 { 1 << 20 } else { 240 << 20 };
        let count = adaptive_worker_count(cpu_limit, 0, MAX_SOURCE_BYTES, batch);
        assert_eq!(count, 0);
        with_workers(
            count,
            |_, _| panic!("no speculative job can be sent"),
            |workers| {
                let profile = Profile::new();
                let mut p = pipeline(workers, &profile);
                p.batch_bytes = batch;
                let raw = b"function Complete() Full() end".to_vec();
                let expected = serde_json::to_value(crate::facts::lua(&raw, false)).unwrap();
                let mut consumed = 0;
                let mut consume =
                    |_: &git::TreeFile, data: Vec<u8>, prepared: Option<PreparedFacts>| {
                        assert!(prepared.is_none());
                        assert_eq!(
                            serde_json::to_value(crate::facts::lua(&data, false)).unwrap(),
                            expected
                        );
                        consumed += 1;
                        Ok(())
                    };
                p.push(&file("Complete.lua", raw.len()), raw, &mut consume)
                    .unwrap();
                assert_eq!(consumed, 1);
                assert!(p.pending.is_empty());
                assert_eq!(p.pending_bytes, 0);
                assert!(p.workers.is_empty());
            },
        );
    }
}

#[test]
fn later_batches_reuse_existing_workers_and_charge_idle_stacks_when_slots_shrink() {
    let count = adaptive_worker_count(8, 0, MAX_SOURCE_BYTES, 1 << 20);
    assert_eq!(count, 6);
    let jobs = AtomicUsize::new(0);
    with_workers(
        count,
        |_, input| {
            jobs.fetch_add(1, Ordering::SeqCst);
            empty(input)
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            p.batch_bytes = 1 << 20;
            let mut prepared = 0;
            let mut sequential = 0;
            let mut consume = |_: &git::TreeFile, _: Vec<u8>, facts: Option<PreparedFacts>| {
                if facts.is_some() {
                    prepared += 1;
                } else {
                    sequential += 1;
                }
                Ok(())
            };
            for ordinal in 0..count {
                p.push(
                    &file(&format!("{ordinal}.lua"), 1),
                    vec![b'x'],
                    &mut consume,
                )
                .unwrap();
            }
            assert_eq!(p.pending.len(), count);
            p.drain(&mut consume).unwrap();
            // A later batch retains the same worker channels, but its real buffer
            // and the original-source accumulation leave no full job reservation.
            p.batch_bytes = 128 << 20;
            p.retained_bytes = 100 << 20;
            assert_eq!(p.workers.len(), count);
            assert!(!fits(
                p.retained_bytes,
                0,
                reservation_for_capacity(1).unwrap(),
                count,
                p.incoming_bytes,
                p.batch_bytes
            ));
            p.push(&file("later.lua", 1), vec![b'x'], &mut consume)
                .unwrap();
            p.finish(Ok(2), &mut consume).unwrap();
            assert_eq!((prepared, sequential), (count, 1));
            assert_eq!(p.workers.len(), count);
            assert_eq!(p.free_workers.len(), count);
            assert_eq!(jobs.load(Ordering::SeqCst), count);
        },
    );
}

#[test]
fn actual_git_pipeline_cpu_limits_preserve_full_facts_order_and_first_error() {
    let home = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/rust-source-adaptive-pipeline-tests")
        .join(format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    lycheedev_core::workspace::initialize(&home).unwrap();
    std::fs::create_dir_all(home.join("mirrors")).unwrap();
    let dir = git::mirror(&home, "wow-ui-source");
    git::bytes(
        None,
        &[
            "init",
            "--bare",
            "--object-format=sha1",
            &dir.to_string_lossy(),
        ],
        4096,
    )
    .unwrap();
    let input_file = home.join("fixture-input");
    let mut inputs = (0..8)
        .map(|ordinal| {
            (
                format!("{ordinal:02}.lua"),
                format!("function F{ordinal}() Frame:RegisterEvent('\\255'); Call() end\n")
                    .into_bytes(),
            )
        })
        .collect::<Vec<_>>();
    inputs.extend([
        (
            "08.xml".into(),
            b"<Ui><Script file=\"00.lua\"/></Ui>".to_vec(),
        ),
        ("09.toc".into(), b"## Interface: 120100\n00.lua\n".to_vec()),
        ("10.lua".into(), b"function Malformed(".to_vec()),
        (
            "Interface/Blizzard_APIDocumentationGenerated/11.lua".into(),
            b"local T={Name='Test',Type='System',Namespace='C_Test'}\n".to_vec(),
        ),
    ]);
    for (path, raw) in &inputs {
        std::fs::write(&input_file, raw).unwrap();
        let object = String::from_utf8(
            git::bytes(
                Some(&dir),
                &["hash-object", "-w", "--", &input_file.to_string_lossy()],
                128,
            )
            .unwrap(),
        )
        .unwrap();
        git::bytes(
            Some(&dir),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{},{}", object.trim(), path),
            ],
            4096,
        )
        .unwrap();
    }
    let tree = String::from_utf8(git::bytes(Some(&dir), &["write-tree"], 128).unwrap()).unwrap();
    let commit = String::from_utf8(
        git::bytes(
            Some(&dir),
            &[
                "-c",
                "user.name=Rust fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit-tree",
                tree.trim(),
                "-m",
                "isolated adaptive pipeline fixture",
            ],
            128,
        )
        .unwrap(),
    )
    .unwrap();
    let pin = SourcePin {
        repository: "wow-ui-source".into(),
        product: "retail".into(),
        requested_ref: "fixture".into(),
        exact_commit: commit.trim().into(),
        parser_revision: git::PARSER.into(),
    };
    let files = git::tree_with_control(&home, &pin, &OperationControl::default()).unwrap();
    assert_eq!(files.len(), inputs.len());
    let parse = |path: &str, raw: &[u8]| {
        if path.ends_with(".lua") {
            crate::facts::lua(raw, path.contains("/Blizzard_APIDocumentationGenerated/"))
        } else if path.ends_with(".toc") {
            crate::facts::toc(raw)
        } else {
            crate::facts::xml(raw)
        }
    };
    let expected = inputs
        .iter()
        .map(|(path, raw)| parse(path, raw))
        .collect::<Vec<_>>();
    for cpu_limit in [1, 2, 4, 8, 128, usize::MAX] {
        let control = OperationControl::default();
        let profile = Profile::new();
        let mut ordinal = 0;
        let mut prepared_count = 0;
        let batches = visit_with_cpu_limit(
            &home,
            &pin,
            &files,
            &control,
            &profile,
            cpu_limit,
            |file, raw, prepared| {
                assert_eq!(file.path, inputs[ordinal].0);
                assert_eq!(raw, inputs[ordinal].1);
                if let Some(prepared) = &prepared {
                    assert_eq!(prepared.mode, ParseMode::for_path(&file.path));
                }
                prepared_count += usize::from(prepared.is_some());
                let facts = prepared.map_or_else(|| parse(&file.path, &raw), |value| value.facts);
                assert_eq!(
                    serde_json::to_vec(&facts).unwrap(),
                    serde_json::to_vec(&expected[ordinal]).unwrap()
                );
                assert_eq!(
                    facts.raw_relationship_targets,
                    expected[ordinal].raw_relationship_targets
                );
                ordinal += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(batches, 1);
        assert_eq!(ordinal, inputs.len());
        assert_eq!(prepared_count == 0, cpu_limit == 1);

        let mut consumed = 0;
        let error = visit_with_cpu_limit(
            &home,
            &pin,
            &files,
            &control,
            &profile,
            cpu_limit,
            |_, _, _| {
                consumed += 1;
                if consumed == 2 {
                    Err(ToolkitError::new(
                        "test.original_failure",
                        "ordered publication",
                        5,
                    ))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "test.original_failure");
        assert_eq!(consumed, 2);
    }
    // XML/TOC alone must count as candidates and create the production scoped
    // worker set. No ordinary Lua file may accidentally make this pass.
    let xml_toc = files
        .iter()
        .filter(|file| {
            matches!(
                ParseMode::for_path(&file.path),
                ParseMode::Xml | ParseMode::Toc
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(xml_toc.len(), 2);
    let control = OperationControl::default();
    let profile = Profile::enabled(true);
    let mut prepared_modes = Vec::new();
    let batches = visit_with_cpu_limit(
        &home,
        &pin,
        &xml_toc,
        &control,
        &profile,
        2,
        |file, raw, prepared| {
            let prepared = prepared.expect("two XML/TOC candidates must start production workers");
            assert_eq!(prepared.mode, ParseMode::for_path(&file.path));
            let expected = original_facts(prepared.mode, &raw);
            assert_eq!(
                serde_json::to_vec(&prepared.facts).unwrap(),
                serde_json::to_vec(&expected).unwrap()
            );
            assert_eq!(
                prepared.facts.raw_relationship_targets,
                expected.raw_relationship_targets
            );
            prepared_modes.push(prepared.mode);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(batches, 1);
    assert_eq!(prepared_modes, [ParseMode::Xml, ParseMode::Toc]);
    assert_eq!(profile.calls(Phase::FactsWorker), 2);
    std::fs::remove_dir_all(home).unwrap();
}
