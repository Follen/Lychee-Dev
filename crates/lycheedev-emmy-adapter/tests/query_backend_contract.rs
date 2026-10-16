#[path = "support/query_backend_cases.rs"]
mod support;
use emmylua_code_analysis::{AsyncState, LuaFunctionType, LuaType, VariadicType};
use lycheedev_emmy_adapter::session::*;
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};
use support::*;
fn plain_value(value: OwnedValue) -> LuaType {
    assert!(value.is_context_free());
    value.structural_type().clone()
}

#[test]
fn ready_hits_skip_provider_and_ledger_but_preserve_control_before_and_after() {
    let (analysis, site, mut provider) = load("return missing", "missing");
    provider.select("missing", absent("absent"));
    let session = session(&provider);
    let count = Cell::new(0);
    let stop_at = Cell::new(usize::MAX);
    let token = Arc::new(());
    let mut check = || {
        count.set(count.get() + 1);
        if count.get() == stop_at.get() {
            Err(Stop {
                ordinal: count.get(),
                token: token.clone(),
            })
        } else {
            Ok(())
        }
    };
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    assert_eq!(
        plain_value(query.type_of_expr(site.syntax).unwrap()),
        LuaType::Nil
    );
    let events = provider.events.lock().unwrap().clone();
    let coverage = rows(&session);
    for relative in [1, 2] {
        stop_at.set(count.get() + relative);
        let OwnedQueryFailure::Control(error) = query.type_of_expr(site.syntax).unwrap_err() else {
            panic!("wrong error kind")
        };
        assert!(Arc::ptr_eq(&error.token, &token));
        assert_eq!(error.ordinal, stop_at.get());
        assert_eq!(*provider.events.lock().unwrap(), events);
        assert_eq!(rows(&session), coverage);
    }
    stop_at.set(usize::MAX);
    let before = count.get();
    assert_eq!(
        plain_value(query.type_of_expr(site.syntax).unwrap()),
        LuaType::Nil
    );
    assert_eq!(count.get() - before, 2);
    assert_eq!(*provider.events.lock().unwrap(), events);
    assert_eq!(rows(&session), coverage);
}

#[test]
fn actual_names_use_selected_initializer_before_annotations_and_keep_compile_order() {
    let source = "---@type string\nlocal left=1\nlocal right=2\nreturn left+right";
    let (analysis, site, mut provider) = load(source, "left+right");
    provider.select("1", absent("initializer-first"));
    let right = provider
        .names
        .iter()
        .find(|(site, text)| {
            text.as_str() == "right"
                && u32::from(site.syntax.get_range().start())
                    > source.find("return").unwrap() as u32
        })
        .map(|(site, _)| *site)
        .unwrap();
    provider
        .values
        .insert(right, absent("compiled-right-first"));
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    assert_eq!(
        plain_value(query.type_of_expr(site.syntax).unwrap()),
        LuaType::Number
    );
    let coverage = rows(&session);
    assert_eq!(
        coverage
            .iter()
            .map(|row| row.proof.as_str())
            .collect::<Vec<_>>(),
        ["compiled-right-first", "initializer-first"]
    );
    let before = provider.events.lock().unwrap().len();
    let left = declaration(&analysis, site.file_id, "left");
    assert_eq!(
        query.type_of_decl(&left).unwrap().map(plain_value),
        Some(LuaType::Nil)
    );
    assert_eq!(
        provider.events.lock().unwrap().len(),
        before,
        "decl Ready hit must not repeat manages probes"
    );
}

#[test]
fn selected_origin_creates_fresh_scope_even_for_the_same_file() {
    let (analysis, site, mut provider) = load("local seed=1\nreturn selected", "selected");
    let origin = provider.named("1");
    provider.select("1", absent("origin"));
    provider.select("selected", expression(origin, "selected-origin"));
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    assert_eq!(
        plain_value(query.type_of_expr(origin.syntax).unwrap()),
        LuaType::Nil
    );
    assert_eq!(
        plain_value(query.type_of_expr(site.syntax).unwrap()),
        LuaType::Nil
    );
    assert_eq!(
        provider
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == Event::Resolve(origin))
            .count(),
        2
    );
    assert_eq!(
        rows(&session)
            .iter()
            .map(|row| row.proof.as_str())
            .collect::<Vec<_>>(),
        ["origin", "proof:selected-origin"],
        "fresh computation shares the one deduplicating producer ledger"
    );
}

#[test]
fn selected_cycle_uses_session_guard_not_a_file_global_expression_cache() {
    let (analysis, site, mut provider) = load("return loop", "loop");
    provider.select("loop", expression(site, "loop"));
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    assert_eq!(
        plain_value(query.type_of_expr(site.syntax).unwrap()),
        LuaType::Unknown
    );
    let coverage = rows(&session);
    assert_eq!(coverage.len(), 2);
    assert_eq!(
        coverage[0].reason.as_deref(),
        Some("binding_projection_cycle")
    );
    assert_eq!(coverage[1].state, "bound");
    assert_eq!(coverage[1].proof, "proof:loop");
}

#[test]
fn stop_at_every_checkpoint_drops_pending_entries_and_keeps_real_ledger_prefix() {
    let source = "local first=1\nreturn first+selected";
    let (analysis, site, mut provider) = load(source, "first+selected");
    provider.select("1", absent("initializer"));
    provider.select("selected", absent("compiled"));
    let reference_provider = provider.isolated();
    let reference_session = session(&reference_provider);
    let total = Cell::new(0);
    let mut check = || {
        total.set(total.get() + 1);
        Ok::<_, Stop>(())
    };
    let expected_owned = OwnedQuery::new(&analysis, &reference_session, site.file_id, &mut check)
        .type_of_expr(site.syntax)
        .unwrap();
    let expected = plain_value(expected_owned);
    let expected_rows = rows(&reference_session);
    let token = Arc::new(());
    for stop in 1..=total.get() {
        let provider = provider.isolated();
        let session = session(&provider);
        let ordinal = Cell::new(0);
        let enabled = Cell::new(true);
        let mut check = || {
            ordinal.set(ordinal.get() + 1);
            if enabled.get() && ordinal.get() == stop {
                Err(Stop {
                    ordinal: stop,
                    token: token.clone(),
                })
            } else {
                Ok(())
            }
        };
        let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
        let OwnedQueryFailure::Control(error) = query.type_of_expr(site.syntax).unwrap_err() else {
            panic!("stop lost")
        };
        assert!(Arc::ptr_eq(&error.token, &token));
        let prefix = rows(&session);
        assert_eq!(prefix, expected_rows[..prefix.len()]);
        enabled.set(false);
        assert_eq!(
            plain_value(query.type_of_expr(site.syntax).unwrap()),
            expected,
            "retry at {stop}"
        );
        assert_eq!(rows(&session), expected_rows);
    }
}

#[test]
fn unwind_removes_the_pending_root_without_resetting_session_or_deadline() {
    let (analysis, site, mut provider) = load("return missing", "missing");
    provider.select("missing", absent("proof"));
    let session = session(&provider);
    let count = Cell::new(0);
    let panic_now = Cell::new(true);
    let mut check = || {
        count.set(count.get() + 1);
        if count.get() == 3 && panic_now.get() {
            panic!("test control unwind");
        }
        Ok::<_, Stop>(())
    };
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    assert!(catch_unwind(AssertUnwindSafe(|| query.type_of_expr(site.syntax))).is_err());
    panic_now.set(false);
    assert_eq!(
        plain_value(query.type_of_expr(site.syntax).unwrap()),
        LuaType::Nil
    );
    assert_eq!(rows(&session).len(), 1);
    assert!(count.get() > 3);
}

#[test]
fn completed_metadata_flow_and_functions_keep_missing_object_shapes_authoritative_unknown() {
    let empty_function = LuaType::DocFunction(Arc::new(LuaFunctionType::new(
        AsyncState::None,
        false,
        false,
        vec![],
        LuaType::Variadic(Arc::new(VariadicType::Multi(vec![]))),
        None,
    )));
    for (source, text, expected) in [
        ("---@type string\nlocal x=1\nreturn x", "x", LuaType::String),
        ("local x=1\nx=2\nreturn x", "x", LuaType::IntegerConst(2)),
        ("return missing", "missing", LuaType::Unknown),
        ("local f=function() end\nreturn f", "f", empty_function),
    ] {
        // The repeated assignment name has two AST occurrences: select the last.
        let query_text = if source.contains("x=2") { "2" } else { text };
        let (analysis, mut site, provider) = load(source, query_text);
        if source.contains("x=2") {
            site = *provider
                .names
                .iter()
                .filter(|(_, name)| name.as_str() == text)
                .max_by_key(|(site, _)| site.syntax.get_range().start())
                .unwrap()
                .0;
        }
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let mut query =
            OwnedQuery::new_for_snapshot(&analysis, &session, site.file_id, &mut check).unwrap();
        assert_eq!(
            plain_value(query.type_of_expr(site.syntax).unwrap()),
            expected
        );
        let events = provider.events.lock().unwrap().clone();
        assert_eq!(
            plain_value(query.type_of_expr(site.syntax).unwrap()),
            expected
        );
        assert_eq!(
            *provider.events.lock().unwrap(),
            events,
            "completed query is cached"
        );
        assert!(rows(&session).is_empty());
    }
    for epoch in ["epoch-a", "epoch-b"] {
        let (analysis, site, mut provider) = load("return object", "object");
        provider.select(
            "object",
            BindingDecision::Bound(BoundValue {
                identity: "selected-object".into(),
                proof: "proof".into(),
                value: SelectedValue::Object(ObjectRef {
                    identity: "same-shape".into(),
                    epoch: epoch.into(),
                    origin: ObjectOrigin::Lua(site),
                    view: ObjectViewKey {
                        context: "owned-query-backend-oracle".into(),
                        observation: 1,
                        object_id: 1,
                    },
                }),
            }),
        );
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
        for _ in 0..2 {
            assert_eq!(
                plain_value(query.type_of_expr(site.syntax).unwrap()),
                LuaType::Unknown
            );
        }
        assert_eq!(
            provider
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| **event == Event::Resolve(site))
                .count(),
            1,
            "the completed unavailable projection is Ready"
        );
        let coverage = rows(&session);
        assert_eq!(coverage.len(), 1);
        assert_eq!(
            coverage[0].reason.as_deref(),
            Some("object_shape_unavailable")
        );
        assert_eq!(coverage[0].operation, "resolve");
    }
}

#[test]
fn denied_call_initializer_slots_preserve_unknown_and_exact_call_producer() {
    for unavailable in [false, true] {
        let (analysis, site, mut provider) =
            load("local first,second=f()\nreturn second", "second");
        let call = provider.named("f()");
        provider.calls.insert(
            call,
            if unavailable {
                BindingDecision::Unavailable {
                    proof: "denied".into(),
                    reason: "capability_missing".into(),
                }
            } else {
                BindingDecision::KnownAbsent {
                    proof: "absent".into(),
                }
            },
        );
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
        assert_eq!(
            plain_value(query.type_of_expr(site.syntax).unwrap()),
            LuaType::Unknown
        );
        let coverage = rows(&session);
        let proof = if unavailable { "denied" } else { "absent" };
        let reason = unavailable.then(|| "capability_missing".to_owned());
        assert_eq!(
            coverage,
            vec![
                BindingCoverage {
                    site: call,
                    operation: "call",
                    state: if unavailable {
                        "unavailable"
                    } else {
                        "known_absent"
                    },
                    proof: proof.into(),
                    reason: reason.clone(),
                    parameter: None,
                    parameter_index: None,
                    argument: None,
                },
                BindingCoverage {
                    site: call,
                    operation: "call_result",
                    state: "unavailable",
                    proof: proof.into(),
                    reason: Some(reason.unwrap_or_else(|| "known_absent_callee".into())),
                    parameter: None,
                    parameter_index: None,
                    argument: None,
                },
            ]
        );
    }
}

#[test]
fn initializer_owner_reuses_the_producer_probe_without_changing_its_order() {
    for mode in 0..3 {
        let (analysis, site, mut provider) = load("local x=1\nreturn x", "x");
        let initializer = provider.named("1");
        let (expected_type, initializer_events) = match mode {
            0 => (
                LuaType::IntegerConst(1),
                vec![
                    Event::Resolve(initializer),
                    Event::Call(initializer),
                    Event::Resolve(initializer),
                    Event::Resolve(initializer),
                ],
            ),
            1 => {
                provider.select("1", absent("initializer-absent"));
                (LuaType::Nil, vec![Event::Resolve(initializer); 2])
            }
            _ => {
                provider.select(
                    "1",
                    BindingDecision::Unavailable {
                        proof: "initializer-unavailable".into(),
                        reason: "missing".into(),
                    },
                );
                (LuaType::Unknown, vec![Event::Resolve(initializer); 2])
            }
        };
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let mut expected_events = vec![Event::Resolve(site)];
        expected_events.extend(initializer_events);
        for _ in 0..2 {
            // A new query has independent inference state, even in one session.
            let mut query =
                OwnedQuery::new_for_snapshot(&analysis, &session, site.file_id, &mut check)
                    .unwrap();
            provider.events.lock().unwrap().clear();
            assert_eq!(
                plain_value(query.type_of_expr(site.syntax).unwrap()),
                expected_type
            );
            assert_eq!(*provider.events.lock().unwrap(), expected_events);
            let ledger = rows(&session);
            assert_eq!(
                plain_value(query.type_of_expr(site.syntax).unwrap()),
                expected_type
            );
            assert_eq!(*provider.events.lock().unwrap(), expected_events);
            assert_eq!(rows(&session), ledger);
        }
    }
}

#[test]
fn selected_origin_still_executes_fresh_initializer_probes_after_an_ordinary_query() {
    let (analysis, site, mut provider) = load("local seed=1\nreturn selected,seed", "selected");
    let origin = provider.named("seed");
    let initializer = provider.named("1");
    provider.select("selected", expression(origin, "selected-origin"));
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query =
        OwnedQuery::new_for_snapshot(&analysis, &session, site.file_id, &mut check).unwrap();
    for syntax in [origin.syntax, site.syntax] {
        assert_eq!(
            plain_value(query.type_of_expr(syntax).unwrap()),
            LuaType::IntegerConst(1)
        );
    }
    assert_eq!(
        *provider.events.lock().unwrap(),
        [
            Event::Resolve(origin),
            Event::Resolve(initializer),
            Event::Call(initializer),
            Event::Resolve(initializer),
            Event::Resolve(initializer),
            Event::Resolve(site),
            Event::Resolve(origin),
            Event::Resolve(initializer),
            Event::Call(initializer),
            Event::Resolve(initializer),
            Event::Resolve(initializer),
        ]
    );
    let before = provider.events.lock().unwrap().clone();
    assert_eq!(
        plain_value(query.type_of_expr(origin.syntax).unwrap()),
        LuaType::IntegerConst(1)
    );
    assert_eq!(*provider.events.lock().unwrap(), before);
}

#[test]
fn call_quality_consumes_the_original_gate_and_retains_every_result_receipt_field() {
    for mode in 0..5 {
        let (analysis, call, mut provider) = load("return f()", "f()");
        let callee = provider.named("f");
        let (operation, state, proof, reason, expected_type, expected_events, quality) = match mode
        {
            0 => {
                provider.values.insert(call, absent("nil-result"));
                (
                    "resolve",
                    "known_absent",
                    "nil-result",
                    None,
                    LuaType::Nil,
                    vec![Event::Call(call), Event::Resolve(call)],
                    None,
                )
            }
            1 => {
                provider.values.insert(
                    call,
                    BindingDecision::Unavailable {
                        proof: "unknown-result".into(),
                        reason: "missing".into(),
                    },
                );
                (
                    "resolve",
                    "unavailable",
                    "unknown-result",
                    Some("missing"),
                    LuaType::Unknown,
                    vec![Event::Call(call), Event::Resolve(call)],
                    None,
                )
            }
            2 => {
                provider.calls.insert(
                    call,
                    BindingDecision::KnownAbsent {
                        proof: "denied".into(),
                    },
                );
                (
                    "call",
                    "known_absent",
                    "denied",
                    None,
                    LuaType::Unknown,
                    vec![Event::Call(call)],
                    Some(("unavailable", "denied", Some("known_absent_callee"))),
                )
            }
            3 => {
                provider.calls.insert(
                    call,
                    BindingDecision::Unavailable {
                        proof: "denied-unknown".into(),
                        reason: "missing-permit".into(),
                    },
                );
                (
                    "call",
                    "unavailable",
                    "denied-unknown",
                    Some("missing-permit"),
                    LuaType::Unknown,
                    vec![Event::Call(call)],
                    Some(("unavailable", "denied-unknown", Some("missing-permit"))),
                )
            }
            _ => {
                provider.calls.insert(
                    call,
                    BindingDecision::Bound(CallPermit {
                        callee,
                        proof: "permit".into(),
                        capability_profile: "typed-call-v1".into(),
                    }),
                );
                provider.values.insert(call, absent("selected-result"));
                (
                    "call_return",
                    "known_absent",
                    "selected-result",
                    None,
                    LuaType::Nil,
                    vec![
                        Event::Call(call),
                        Event::Observation(call),
                        Event::Resolve(call),
                    ],
                    Some(("native_projected", "permit", None)),
                )
            }
        };
        let receipt = |operation, state, proof: &str, reason: Option<&str>| BindingCoverage {
            site: call,
            operation,
            state,
            proof: proof.into(),
            reason: reason.map(str::to_owned),
            parameter: None,
            parameter_index: None,
            argument: None,
        };
        let mut expected_rows = vec![receipt(operation, state, proof, reason)];
        if let Some((state, proof, reason)) = quality {
            expected_rows.push(receipt("call_result", state, proof, reason));
        }
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let mut query =
            OwnedQuery::new_for_snapshot(&analysis, &session, call.file_id, &mut check).unwrap();
        for _ in 0..2 {
            assert_eq!(
                plain_value(query.type_of_expr(call.syntax).unwrap()),
                expected_type
            );
            assert_eq!(*provider.events.lock().unwrap(), expected_events);
            assert_eq!(rows(&session), expected_rows);
        }
    }
}
