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
    parse: impl Fn(usize, LuaInput) -> LuaOutput + Sync,
    body: impl FnOnce(&[Worker]),
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
        body(&workers);
        drop(workers);
        for handle in handles {
            handle.join().unwrap();
        }
    });
}

fn empty(input: LuaInput) -> LuaOutput {
    LuaOutput {
        data: input.data,
        facts: Some(Facts::default()),
        parse_elapsed: Duration::ZERO,
    }
}
fn pipeline<'a>(workers: &'a [Worker], profile: &'a Profile) -> Pipeline<'a> {
    Pipeline {
        workers,
        pending: VecDeque::new(),
        pending_bytes: 0,
        retained_bytes: 0,
        free_workers: (0..workers.len()).collect(),
        incoming_bytes: MAX_SOURCE_BYTES,
        failed: false,
        batch_bytes: GIT_BATCH_BYTES,
        profile,
    }
}

#[test]
fn only_bounded_ordinary_utf8_lua_is_speculative() {
    for path in ["A.lua", "A.LUA"] {
        assert!(eligible(&file(path, 1), b"x"));
    }
    for path in [
        "A.xml",
        "A.toc",
        "Interface/Blizzard_APIDocumentationGenerated/A.lua",
    ] {
        assert!(!eligible(&file(path, 1), b"x"));
    }
    let mut asset = file("A.lua", 1);
    asset.asset = true;
    assert!(!eligible(&asset, b"x"));
    assert!(!eligible(&file("A.lua", 1), b"\xff"));
    assert!(!eligible(
        &file("A.lua", MAX_SOURCE_BYTES + 1),
        &vec![b' '; MAX_SOURCE_BYTES + 1]
    ));
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
fn small_xml_retains_its_complete_ordered_coordinator_path() {
    with_workers(
        2,
        |_, input| empty(input),
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut seen = Vec::new();
            let mut consume =
                |f: &git::TreeFile, bytes: Vec<u8>, prepared: Option<PreparedFacts>| {
                    seen.push((f.path.clone(), bytes, prepared.is_some()));
                    Ok(())
                };
            p.push(&file("first.lua", 1), vec![b'a'], &mut consume)
                .unwrap();
            p.push(&file("second.xml", 4), b"<X/>".to_vec(), &mut consume)
                .unwrap();
            assert_eq!(p.pending.len(), 2);
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(
                seen,
                [
                    ("first.lua".into(), vec![b'a'], true),
                    ("second.xml".into(), b"<X/>".to_vec(), false)
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
fn an_abandoned_pure_result_is_reparsed_without_public_truncation() {
    with_workers(
        2,
        |_, input| LuaOutput {
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
fn lua_workers_overlap_across_small_xml_without_processing_xml_ahead() {
    let gate = (Mutex::new(0usize), Condvar::new());
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
            assert!(!timeout.timed_out() && *arrived == 2);
            empty(input)
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut order = Vec::new();
            let mut consume =
                |f: &git::TreeFile, data: Vec<u8>, prepared: Option<PreparedFacts>| {
                    if f.path.ends_with(".xml") {
                        assert!(prepared.is_none());
                        let facts = crate::facts::xml(&data);
                        assert_eq!(facts.loads[0].path, "Full.lua");
                    } else {
                        assert!(prepared.is_some());
                    }
                    order.push(f.path.clone());
                    Ok(())
                };
            let xml = b"<Ui><Script file=\"Full.lua\"/></Ui>".to_vec();
            let xml_capacity = xml.capacity();
            p.push(&file("first.lua", 1), vec![b'a'], &mut consume)
                .unwrap();
            let previous = p.pending_bytes;
            p.push(&file("middle.xml", xml.len()), xml, &mut consume)
                .unwrap();
            assert_eq!(
                p.pending_bytes - previous,
                xml_capacity + JOB_METADATA_BYTES
            );
            p.push(&file("last.lua", 1), vec![b'b'], &mut consume)
                .unwrap();
            assert_eq!(p.pending.len(), 3);
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(order, ["first.lua", "middle.xml", "last.lua"]);
        },
    );
}

#[test]
fn real_mixed_parser_pipeline_preserves_full_facts_order_and_measured_bounds() {
    let control = OperationControl::default();
    let active = AtomicUsize::new(0);
    let peak_active = AtomicUsize::new(0);
    let entered = AtomicUsize::new(0);
    let first_wave = (Mutex::new(0usize), Condvar::new());
    let mut inputs = (0..4)
        .map(|i| {
            (
                format!("Initial{i}.lua"),
                format!("function F{i}() RegisterEvent('EVENT_{i}') end\n").into_bytes(),
            )
        })
        .collect::<Vec<_>>();
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
        4,
        |_, input| {
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak_active.fetch_max(now, Ordering::SeqCst);
            if entered.fetch_add(1, Ordering::SeqCst) < 4 {
                let mut count = first_wave.0.lock().unwrap();
                *count += 1;
                first_wave.1.notify_all();
                let (count, timeout) = first_wave
                    .1
                    .wait_timeout_while(count, Duration::from_secs(5), |n| *n < 4)
                    .unwrap();
                assert!(!timeout.timed_out() && *count == 4);
            }
            let output = prepare_input(input, &control, true);
            active.fetch_sub(1, Ordering::SeqCst);
            output
        },
        |workers| {
            let profile = Profile::new();
            let mut p = pipeline(workers, &profile);
            let mut ordinal = 0usize;
            let mut prepared_documents = 0;
            let mut max_slots = 0;
            let mut max_owned_reservation = 0;
            let mut consume = |f: &git::TreeFile, raw: Vec<u8>, prepared: Option<PreparedFacts>| {
                assert_eq!(f.path, inputs[ordinal].0);
                assert_eq!(raw, inputs[ordinal].1);
                prepared_documents += usize::from(prepared.is_some());
                let facts =
                    prepared.map_or_else(|| parse(&f.path, &raw), |prepared| prepared.facts);
                assert_eq!(
                    serde_json::to_value(&facts).unwrap(),
                    serde_json::to_value(&expected[ordinal]).unwrap()
                );
                assert_eq!(
                    facts.raw_relationship_targets,
                    expected[ordinal].raw_relationship_targets
                );
                ordinal += 1;
                Ok(())
            };
            for (path, raw) in &inputs {
                p.push(&file(path, raw.len()), raw.clone(), &mut consume)
                    .unwrap();
                max_slots = max_slots.max(p.pending.len());
                max_owned_reservation = max_owned_reservation.max(p.pending_bytes);
                assert!(p.pending.len() <= 8);
                assert!(4 - p.free_workers.len() <= 4);
                assert!(fits(
                    p.retained_bytes,
                    p.pending_bytes,
                    0,
                    4,
                    p.incoming_bytes,
                    p.batch_bytes
                ));
            }
            p.finish(Ok(1), &mut consume).unwrap();
            assert_eq!(ordinal, inputs.len());
            assert!(prepared_documents >= 4);
            assert!(max_slots > 4 && max_slots <= 8);
            assert!(
                max_owned_reservation
                    <= SOURCE_RETAINED_LIMIT - p.batch_bytes - 4 * WORKER_STACK_BYTES
            );
            assert_eq!(p.pending_bytes, 0);
            assert_eq!(p.free_workers.len(), 4);
        },
    );
    assert_eq!(peak_active.load(Ordering::SeqCst), 4);
    assert_eq!(active.load(Ordering::SeqCst), 0);
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
