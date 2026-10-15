//! Full result, provider-call and coverage comparisons through the actual old
//! contextual model and the concrete owned query (no fixture semantic backend).
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/function_call_cases.rs"]
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
                let selected = match value.value {
                    owned::SelectedValue::Expression(origin) => {
                        native::SelectedValue::Expression(to_native(origin))
                    }
                    owned::SelectedValue::Function(function) => {
                        native::SelectedValue::Function(native::FunctionRef {
                            file_id: function.file_id,
                            closure: function.closure,
                            declaration: function.declaration,
                        })
                    }
                    _ => panic!("fixture must not bypass unsupported selected values"),
                };
                native::BindingDecision::Bound(native::BoundValue {
                    identity: value.identity,
                    proof: value.proof,
                    value: selected,
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
        let value = new_model.type_of_expr(site.syntax).unwrap();
        assert!(
            value.is_context_free(),
            "inherited Function R1 cases have no selected object"
        );
        let new = value.structural_type().clone();
        assert_eq!(new, old, "complete LuaType at {:?}", site);
        assert_eq!(format!("{new:?}"), format!("{old:?}"), "all type fields");
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
        "OWNED_FUNCTION_CALL_NATIVE queries={} coverage={} providerCalls={}",
        queries.len(),
        rows(&new_session).len(),
        new_provider.events.lock().unwrap().len()
    );
    (results, rows(&new_session))
}

#[test]
fn real_function_signature_and_direct_call_paths_match_nonempty_and_empty_bodies() {
    let mut comparisons = 0;
    for body in ordinary_call_bodies() {
        for query in ["selected", "selected()"] {
            let source = format!("local function f() {body} end\nreturn {query}");
            let (analysis, site, _, provider) = load_function(&source, query);
            compare(&analysis, &[site, site], &provider);
            comparisons += 2;
        }
    }
    assert_eq!(comparisons, 32);
}

#[test]
fn contextual_children_unknown_calls_docs_and_union_rows_match_full_receipts() {
    let mut comparisons = 0;
    for (source, query) in [
        (
            "local function f() return absent,denied() end\nreturn selected()",
            "selected()",
        ),
        (
            "---@deprecated\nlocal function f() if gate then return absent end end\nreturn selected",
            "selected",
        ),
        (
            "local function f() if gate then return 1 end return 2 end\nreturn selected",
            "selected",
        ),
        (
            "---@param x? number\n---@async\n---@return string,boolean\nlocal function f(x) return unobserved end\nreturn selected",
            "selected",
        ),
        (
            "---@return number\nlocal function f() return unobserved end\nreturn selected()",
            "selected()",
        ),
        (
            "---@return_overload true,number\n---@return_overload false,string\nlocal function f() return unobserved end\nreturn selected",
            "selected",
        ),
        (
            "local function f(x,...) return 4 end\nreturn selected",
            "selected",
        ),
        (
            "---@deprecated\nlocal function f() return uncertain end\nreturn selected()",
            "selected()",
        ),
    ] {
        let (analysis, site, _, mut provider) = load_function(source, query);
        for name in ["absent", "unobserved"] {
            if provider.names.values().any(|text| text == name) {
                provider.select(name, absent(name));
            }
        }
        if provider.names.values().any(|text| text == "uncertain") {
            provider.select(
                "uncertain",
                owned::BindingDecision::Unavailable {
                    proof: "uncertain".into(),
                    reason: "missing".into(),
                },
            );
        }
        if provider.names.values().any(|text| text == "denied()") {
            provider.calls.insert(
                provider.named("denied()"),
                owned::BindingDecision::Unavailable {
                    proof: "denied".into(),
                    reason: "not-observed".into(),
                },
            );
        }
        compare(&analysis, &[site, site], &provider);
        comparisons += 2;
    }
    assert_eq!(comparisons, 16);
}

#[test]
fn fixed_32_slot_and_256_return_row_boundaries_keep_exact_producer_prefixes() {
    let mut comparisons = 0;
    for count in [32, 33] {
        let values = (0..count)
            .map(|index| format!("slot{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let source =
            format!("---@deprecated\nlocal function f() return {values} end\nreturn selected");
        let (analysis, site, _, mut provider) = load_function(&source, "selected");
        for index in 0..count {
            provider.select(
                &format!("slot{index}"),
                absent(&format!("slot-proof{index}")),
            );
        }
        let (_, ledger) = compare(&analysis, &[site, site], &provider);
        assert_eq!(ledger.len(), if count == 32 { 33 } else { 1 });
        comparisons += 2;
    }
    for count in [256, 257] {
        let mut body = String::new();
        for index in 0..count - 1 {
            body.push_str(&format!("if gate then return slot{index} end\n"));
        }
        body.push_str(&format!("return slot{}", count - 1));
        let source = format!("---@deprecated\nlocal function f()\n{body}\nend\nreturn selected");
        let (analysis, site, _, mut provider) = load_function(&source, "selected");
        for index in 0..count {
            provider.select(
                &format!("slot{index}"),
                absent(&format!("row-proof{index}")),
            );
        }
        let (_, ledger) = compare(&analysis, &[site, site], &provider);
        assert_eq!(ledger.len(), 257);
        assert_eq!(ledger.last().unwrap().proof, "selected-function-proof");
        comparisons += 2;
    }
    assert_eq!(comparisons, 8);
}

#[test]
fn foreign_function_scope_uses_the_same_provider_and_missing_signature_keeps_reason() {
    use emmylua_parser::{LuaAstNode, LuaClosureExpr};
    let (mut analysis, site, _, mut provider) =
        load_function("local function f() end\nreturn selected()", "selected()");
    let root = std::env::current_dir()
        .unwrap()
        .join("owned-query-backend-oracle");
    let foreign_file = analysis.update_files_by_path(vec![(
        root.join("foreign.lua"),
        Some("local function foreign() return evidence end".into()),
    )])[0];
    add_names(&analysis, foreign_file, &mut provider);
    let foreign = analysis
        .semantic_model(foreign_file)
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .find_map(LuaClosureExpr::cast)
        .unwrap();
    let target = provider.named("selected");
    bind_function(
        &mut provider,
        target,
        owned::BindingSite {
            file_id: foreign_file,
            syntax: foreign.get_syntax_id(),
        },
    );
    provider.select("evidence", absent("foreign-return"));
    compare(&analysis, &[site, site], &provider);

    // Source kind validity comes from the public signature index, not invented
    // fallback to a different closure or an uncontextual function type.
    bind_function(&mut provider, target, site);
    let (_, ledger) = compare(&analysis, &[site, site], &provider);
    assert_eq!(ledger.len(), 1);
    assert_eq!(
        ledger[0].reason.as_deref(),
        Some("selected_function_signature_unavailable")
    );
}

#[test]
fn signature_children_and_callee_keep_the_original_16384_ledger_boundary() {
    for count in [16_382, 16_383] {
        let names = (0..count)
            .map(|index| format!("prefix{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let dense = format!("false or {{{names}}}");
        let source =
            format!("local before={dense}\nlocal function f() return child end\nreturn selected()");
        let (analysis, site, _, mut provider) = load_function(&source, "selected()");
        // Populate the provider in one pass; repeated fixture name lookup
        // would add an unrelated O(n²) cost at the ledger boundary.
        for (site, name) in &provider.names {
            if let Some(index) = name
                .strip_prefix("prefix")
                .and_then(|suffix| suffix.parse::<usize>().ok())
            {
                assert!(index < count);
                provider
                    .values
                    .insert(*site, absent(&format!("prefix-proof{index}")));
            }
        }
        provider.select("child", absent("body-child"));
        let prefix_site = provider.named(&dense);
        let (_, ledger) = compare(&analysis, &[prefix_site, site], &provider);
        assert_eq!(ledger.len(), 16_384);
        assert_eq!(
            ledger.last().unwrap().reason.as_deref(),
            Some("binding_coverage_budget")
        );
        assert_eq!(
            ledger.last().unwrap().site,
            if count == 16_382 {
                provider.named("selected")
            } else {
                provider.named("child")
            }
        );
    }
}
