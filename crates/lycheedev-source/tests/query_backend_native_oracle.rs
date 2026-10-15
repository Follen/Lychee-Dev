//! Full result, provider-call and coverage comparisons through the actual old
//! contextual model and the concrete owned query (no fixture semantic backend).
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/query_backend_cases.rs"]
mod support;
use emmylua_code_analysis::{EmmyLuaAnalysis, LuaType, binding_context as native};
use lycheedev_emmy_adapter::session as owned;
use std::sync::Arc;
use support::*;

struct NativeProvider(Provider);
fn to_owned(site: native::BindingSite) -> owned::BindingSite {
    owned::BindingSite {
        file_id: site.file_id,
        syntax: site.syntax,
    }
}
fn to_native(site: owned::BindingSite) -> native::BindingSite {
    native::BindingSite {
        file_id: site.file_id,
        syntax: site.syntax,
    }
}
impl native::BindingContext for NativeProvider {
    fn identity(&self) -> &str {
        "owned-query-backend-oracle"
    }
    fn resolve(&self, site: native::BindingSite) -> native::BindingDecision<native::BoundValue> {
        match owned::BindingContext::resolve(&self.0, to_owned(site)) {
            owned::BindingDecision::NotManaged => native::BindingDecision::NotManaged,
            owned::BindingDecision::KnownAbsent { proof } => {
                native::BindingDecision::KnownAbsent { proof }
            }
            owned::BindingDecision::Unavailable { proof, reason } => {
                native::BindingDecision::Unavailable { proof, reason }
            }
            owned::BindingDecision::Bound(value) => {
                let owned::SelectedValue::Expression(origin) = value.value else {
                    panic!("this oracle's accepted bound value is Expression")
                };
                native::BindingDecision::Bound(native::BoundValue {
                    identity: value.identity,
                    proof: value.proof,
                    value: native::SelectedValue::Expression(to_native(origin)),
                })
            }
        }
    }
    fn call(&self, site: native::BindingSite) -> native::BindingDecision<native::CallPermit> {
        match owned::BindingContext::call(&self.0, to_owned(site)) {
            owned::BindingDecision::NotManaged => native::BindingDecision::NotManaged,
            owned::BindingDecision::KnownAbsent { proof } => {
                native::BindingDecision::KnownAbsent { proof }
            }
            owned::BindingDecision::Unavailable { proof, reason } => {
                native::BindingDecision::Unavailable { proof, reason }
            }
            owned::BindingDecision::Bound(_) => {
                panic!("permit validation belongs to the existing observed-call oracle")
            }
        }
    }
}
fn row(row: native::BindingCoverage) -> owned::BindingCoverage {
    owned::BindingCoverage {
        site: to_owned(row.site),
        operation: row.operation,
        state: row.state,
        proof: row.proof,
        reason: row.reason,
        parameter: row.parameter,
        parameter_index: row.parameter_index,
        argument: row.argument.map(to_owned),
    }
}
fn compare(
    analysis: &EmmyLuaAnalysis,
    queries: &[owned::BindingSite],
    provider: &Provider,
) -> (Vec<LuaType>, Vec<owned::BindingCoverage>) {
    let old_provider = provider.isolated();
    let new_provider = provider.isolated();
    let old_session = native::BindingSession::new(Arc::new(NativeProvider(old_provider.clone())));
    let old_model = analysis.semantic_model_with_context(queries[0].file_id, old_session.clone());
    let new_session = session(&new_provider);
    let mut check = || Ok::<_, Stop>(());
    let mut new_model =
        owned::OwnedQuery::new(analysis, &new_session, queries[0].file_id, &mut check);
    let mut results = vec![];
    for site in queries {
        assert_eq!(site.file_id, queries[0].file_id);
        let old = old_model.type_of_expr(site.syntax);
        let new = new_model.type_of_expr(site.syntax).unwrap();
        assert_eq!(new, old, "complete LuaType at {:?}", site);
        assert_eq!(
            rows(&new_session),
            old_session
                .coverage()
                .into_iter()
                .map(row)
                .collect::<Vec<_>>(),
            "every ledger field and producer order after each query"
        );
        assert_eq!(
            *new_provider.events.lock().unwrap(),
            *old_provider.events.lock().unwrap(),
            "actual provider query sequence, including cache hits and manages probes"
        );
        results.push(new);
    }
    println!(
        "OWNED_QUERY_NATIVE queries={} coverage={} providerCalls={}",
        queries.len(),
        rows(&new_session).len(),
        new_provider.events.lock().unwrap().len()
    );
    (results, rows(&new_session))
}

#[test]
fn real_local_declarations_literals_uninitialized_nil_and_selected_docs_use_the_complete_query_path()
 {
    let mut comparisons = 0;
    for (source, text) in [
        ("local x=1\nreturn x", "x"),
        ("local x=1\nlocal y=x+2\nreturn y", "y"),
        ("local x\nreturn x", "x"),
        ("local x=1\nlocal y=2\nreturn x+y", "x+y"),
        ("local x=1\nreturn -x", "-x"),
        ("local x='text'\nreturn #x", "#x"),
        ("local x=true\nreturn x and {one,two}", "x and {one,two}"),
        ("local x=false\nreturn x or {one,two}", "x or {one,two}"),
        ("local a,b=1\nreturn b", "b"),
        ("return (1+2)*3", "(1+2)*3"),
        ("return not unused", "not unused"),
    ] {
        let (analysis, site, mut provider) = load(source, text);
        for name in ["one", "two"] {
            if provider.names.values().any(|text| text == name) {
                provider.select(name, absent(name));
            }
        }
        compare(&analysis, &[site, site], &provider);
        comparisons += 2;
    }
    for mode in 0..3 {
        let (analysis, site, mut provider) =
            load("local origin=2\n---@type string\nlocal x=1\nreturn x", "x");
        provider.select(
            "1",
            match mode {
                0 => absent("absent-over-doc"),
                1 => owned::BindingDecision::Unavailable {
                    proof: "unknown-over-doc".into(),
                    reason: "no_evidence".into(),
                },
                _ => expression(provider.named("2"), "expression-over-doc"),
            },
        );
        let (values, _) = compare(&analysis, &[site, site], &provider);
        assert_eq!(
            values[0],
            match mode {
                0 => LuaType::Nil,
                1 => LuaType::Unknown,
                _ => LuaType::IntegerConst(2),
            }
        );
        comparisons += 2;
    }
    assert_eq!(comparisons, 28);
}

#[test]
fn two_phase_order_and_same_file_origin_fresh_scope_match_native_cache_and_all_provider_calls() {
    let (analysis, site, mut provider) = load(
        "local left=1\nlocal right=2\nreturn left+right",
        "left+right",
    );
    provider.select("1", absent("initializer"));
    provider.select("right", absent("compile-right"));
    let (_, coverage) = compare(&analysis, &[site, provider.named("1"), site], &provider);
    assert_eq!(
        coverage
            .iter()
            .map(|row| row.proof.as_str())
            .collect::<Vec<_>>(),
        ["compile-right", "initializer"]
    );
    let (analysis, site, mut provider) =
        load("local origin=1\nreturn first+second", "first+second");
    let origin = provider.named("1");
    provider.select("1", absent("origin"));
    provider.select("first", expression(origin, "first"));
    provider.select("second", expression(origin, "second"));
    compare(&analysis, &[origin, site, origin, site], &provider);
}

#[test]
fn selected_cycles_and_foreign_origin_share_guard_and_ledger_but_not_model_caches() {
    let (analysis, site, mut provider) = load("return loop", "loop");
    provider.select("loop", expression(site, "cycle"));
    let (values, coverage) = compare(&analysis, &[site, site], &provider);
    assert_eq!(values, [LuaType::Unknown, LuaType::Unknown]);
    assert_eq!(
        coverage[0].reason.as_deref(),
        Some("binding_projection_cycle")
    );
    let (mut analysis, site, mut provider) = load("return selected", "selected");
    let path = std::env::current_dir()
        .unwrap()
        .join("owned-query-backend-oracle/foreign.lua");
    let file = analysis.update_files_by_path(vec![(path, Some("local x=101\nreturn x".into()))])[0];
    add_names(&analysis, file, &mut provider);
    let origin = provider.named("x");
    provider.select("101", absent("foreign-initializer"));
    provider.select("selected", expression(origin, "foreign-origin"));
    let (values, coverage) = compare(&analysis, &[site, site], &provider);
    assert_eq!(values, [LuaType::Nil, LuaType::Nil]);
    assert_eq!(coverage[0].site.file_id, file);
    assert_eq!(coverage[1].site.file_id, site.file_id);
}

#[test]
fn selected_absent_unknown_and_denied_calls_keep_initializer_slot_nil_unknown_distinction() {
    let mut comparisons = 0;
    for index in ["first", "second"] {
        for mode in 0..4 {
            let (analysis, site, mut provider) =
                load(&format!("local first,second=f()\nreturn {index}"), index);
            let call = provider.named("f()");
            match mode {
                0 => {
                    provider.values.insert(call, absent("nil-result"));
                }
                1 => {
                    provider.values.insert(
                        call,
                        owned::BindingDecision::Unavailable {
                            proof: "unknown-result".into(),
                            reason: "missing".into(),
                        },
                    );
                }
                2 => {
                    provider.calls.insert(
                        call,
                        owned::BindingDecision::KnownAbsent {
                            proof: "denied".into(),
                        },
                    );
                }
                _ => {
                    provider.calls.insert(
                        call,
                        owned::BindingDecision::Unavailable {
                            proof: "denied-unknown".into(),
                            reason: "missing_permit".into(),
                        },
                    );
                }
            }
            let (values, coverage) = compare(&analysis, &[site, site], &provider);
            assert_eq!(
                values[0],
                if mode == 0 {
                    LuaType::Nil
                } else {
                    LuaType::Unknown
                }
            );
            assert_eq!(coverage.len(), 1);
            comparisons += 2;
        }
    }
    assert_eq!(comparisons, 16);
}

#[test]
fn concrete_query_runs_later_fields_after_the_exact_16384_coverage_boundary() {
    let count = 16_386;
    let names = (0..count)
        .map(|index| format!("field_{index}"))
        .collect::<Vec<_>>();
    let expression = format!("true and {{{}}}", names.join(","));
    let (analysis, site, mut provider) = load(&format!("return {expression}"), &expression);
    // One name map traversal, not O(calls * 16384) ledger snapshots or searches.
    for (site, name) in &provider.names {
        if name.starts_with("field_") {
            provider.values.insert(*site, absent(name));
        }
    }
    let (values, coverage) = compare(&analysis, &[site, site], &provider);
    assert_eq!(coverage.len(), 16_384);
    assert_eq!(
        coverage.last().unwrap().reason.as_deref(),
        Some("binding_coverage_budget")
    );
    assert_eq!(coverage.last().unwrap().site, provider.named("field_16383"));
    let LuaType::Tuple(tuple) = &values[0] else {
        panic!("tuple missing")
    };
    assert_eq!(tuple.get_types().len(), count);
    assert!(
        tuple
            .get_types()
            .iter()
            .all(|ty| matches!(ty, LuaType::Nil)),
        "cap never skips later semantic work"
    );
}
