//! Independent fixed-engine traces versus the concrete owned query.
//! Full types, provider calls and coverage fields retain their original checks.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/function_call_cases.rs"]
mod support;
#[path = "support/fixed_oracle_trace.rs"]
mod trace;
use emmylua_code_analysis::{EmmyLuaAnalysis, LuaType};
use lycheedev_emmy_adapter::session as owned;
use support::*;

fn compare(
    analysis: &EmmyLuaAnalysis,
    queries: &[owned::BindingSite],
    provider: &Provider,
) -> (Vec<LuaType>, Vec<owned::BindingCoverage>) {
    let new_provider = provider.isolated();
    let new_session = session(&new_provider);
    let mut check = || Ok::<_, Stop>(());
    let mut new_model =
        owned::OwnedQuery::new_for_snapshot(analysis, &new_session, queries[0].file_id, &mut check)
            .unwrap();
    let mut results = vec![];
    for site in queries {
        assert_eq!(site.file_id, queries[0].file_id);
        let value = new_model.type_of_expr(site.syntax).unwrap();
        assert!(
            value.is_context_free(),
            "inherited Function R1 cases have no selected object"
        );
        let new = value.structural_type().clone();
        trace::check("query.type", &new);
        trace::check("query.type_debug", &format!("{new:?}"));
        trace::check("query.coverage", &rows(&new_session));
        trace::check("query.provider", &*new_provider.events.lock().unwrap());
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
    let _trace = trace::begin_candidate();
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
    let _trace = trace::begin_candidate();
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
    let _trace = trace::begin_candidate();
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
    let _trace = trace::begin_candidate();
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
    let _trace = trace::begin_candidate();
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
