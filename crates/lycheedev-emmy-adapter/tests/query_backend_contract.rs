#[path = "support/query_backend_cases.rs"]
mod support;
use emmylua_code_analysis::LuaType;
use lycheedev_emmy_adapter::session::*;
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};
use support::*;

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
    assert_eq!(query.type_of_expr(site.syntax).unwrap(), LuaType::Nil);
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
    assert_eq!(query.type_of_expr(site.syntax).unwrap(), LuaType::Nil);
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
    assert_eq!(query.type_of_expr(site.syntax).unwrap(), LuaType::Number);
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
    assert_eq!(query.type_of_decl(&left).unwrap(), Some(LuaType::Nil));
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
    assert_eq!(query.type_of_expr(origin.syntax).unwrap(), LuaType::Nil);
    assert_eq!(query.type_of_expr(site.syntax).unwrap(), LuaType::Nil);
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
    assert_eq!(query.type_of_expr(site.syntax).unwrap(), LuaType::Unknown);
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
    let expected = OwnedQuery::new(&analysis, &reference_session, site.file_id, &mut check)
        .type_of_expr(site.syntax)
        .unwrap();
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
            query.type_of_expr(site.syntax).unwrap(),
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
    assert_eq!(query.type_of_expr(site.syntax).unwrap(), LuaType::Nil);
    assert_eq!(rows(&session).len(), 1);
    assert!(count.get() > 3);
}

#[test]
fn unsupported_metadata_flow_and_objects_are_typed_continuations_not_successful_unknown() {
    for (source, text, capability) in [
        (
            "---@type string\nlocal x=1\nreturn x",
            "x",
            OwnedQueryCapability::DeclarationMetadata,
        ),
        (
            "local x=1\nx=2\nreturn x",
            "x",
            OwnedQueryCapability::DeclarationFlow,
        ),
        (
            "return missing",
            "missing",
            OwnedQueryCapability::UnresolvedName,
        ),
        (
            "local f=function() end\nreturn f",
            "f",
            OwnedQueryCapability::Function,
        ),
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
        let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
        for _ in 0..2 {
            let OwnedQueryFailure::Continue(value) = query.type_of_expr(site.syntax).unwrap_err()
            else {
                panic!("missing typed continuation")
            };
            assert_eq!(value.capability, capability);
        }
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
            let OwnedQueryFailure::Continue(value) = query.type_of_expr(site.syntax).unwrap_err()
            else {
                panic!("object erased")
            };
            assert_eq!(value.capability, OwnedQueryCapability::Object);
            assert_eq!(value.syntax, Some(site.syntax));
        }
        assert_eq!(
            provider.events.lock().unwrap().len(),
            2,
            "neither failure cached Unknown"
        );
        assert!(
            rows(&session).is_empty(),
            "no projected/checked object receipt"
        );
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
        assert_eq!(query.type_of_expr(site.syntax).unwrap(), LuaType::Unknown);
        let coverage = rows(&session);
        assert_eq!(coverage.len(), 1);
        assert_eq!(coverage[0].site, call);
        assert_eq!(coverage[0].operation, "call");
        assert_eq!(
            coverage[0].state,
            if unavailable {
                "unavailable"
            } else {
                "known_absent"
            }
        );
    }
}
