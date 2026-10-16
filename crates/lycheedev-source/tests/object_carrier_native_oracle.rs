//! Real object projection comparison: every structural and visible metadata field,
//! all provider calls/coverage, and opaque session-token equality relationships.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/query_backend_cases.rs"]
mod query_support;
#[path = "../../lycheedev-emmy-adapter/tests/support/object_carrier_cases.rs"]
mod support;
#[path = "support/fixed_oracle_trace.rs"]
mod trace;
use emmylua_code_analysis::{EmmyLuaAnalysis, LuaType};
use lycheedev_emmy_adapter::session as owned;
use std::collections::HashSet;
use support::*;
#[path = "support/object_oracle.rs"]
mod object_oracle;
use object_oracle::*;
fn compare(
    analysis: &EmmyLuaAnalysis,
    sites: &[owned::BindingSite],
    provider: &Provider,
) -> (Vec<owned::OwnedValue>, Vec<owned::BindingCoverage>) {
    let new_provider = provider.isolated();
    let new_session = session(&new_provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query =
        owned::OwnedQuery::new_for_snapshot(analysis, &new_session, sites[0].file_id, &mut check)
            .unwrap();
    let mut new_values = vec![];
    for site in sites {
        let new = query.type_of_expr(site.syntax).unwrap();
        trace::check("object.query.type", &new_type(&new));
        new_values.push(new);
    }
    let coverage = rows(&new_session);
    trace::check("object.query.coverage", &coverage);
    trace::check(
        "object.query.provider",
        &*new_provider.events.lock().unwrap(),
    );
    println!(
        "OWNED_OBJECT_NATIVE queries={} coverage={} providerCalls={}",
        sites.len(),
        coverage.len(),
        new_provider.events.lock().unwrap().len()
    );
    (new_values, coverage)
}

#[test]
fn object_presence_origin_shape_and_field_failures_match_all_metadata_and_producer_rows() {
    let _trace = trace::begin_candidate();
    let mut comparisons = 0;
    for case in 0..23 {
        let (analysis, site, mut provider, mut selected) = basic_case();
        let mut fields = vec![field("nil", owned::FieldPresence::Present, absent("nil"))];
        let mut enumeration = owned::ShapeEnumeration::Complete;
        match case {
            0 => {}
            1 => {
                fields = vec![field(
                    "any",
                    owned::FieldPresence::Present,
                    environment_any(),
                )]
            }
            2 => {
                fields = vec![field(
                    "absent",
                    owned::FieldPresence::Absent,
                    absent("absent"),
                )]
            }
            3 => {
                fields = vec![field(
                    "absent",
                    owned::FieldPresence::Absent,
                    owned::BindingDecision::NotManaged,
                )]
            }
            4 => {
                fields = vec![field(
                    "unavailable",
                    owned::FieldPresence::Unavailable,
                    environment_any(),
                )]
            }
            5 => {
                fields = vec![field(
                    "unmanaged",
                    owned::FieldPresence::Present,
                    owned::BindingDecision::NotManaged,
                )]
            }
            6 => {
                fields = vec![field(
                    "unavailable",
                    owned::FieldPresence::Present,
                    owned::BindingDecision::Unavailable {
                        proof: "missing-field".into(),
                        reason: "not-observed".into(),
                    },
                )]
            }
            7 => {
                fields = vec![field(
                    "cycle",
                    owned::FieldPresence::Present,
                    expression(site, "recursive-expression"),
                )]
            }
            8 => {
                fields = vec![field(
                    "cycle",
                    owned::FieldPresence::Present,
                    bound_object(&selected),
                )]
            }
            9 => {
                fields = vec![
                    field("", owned::FieldPresence::Present, absent("empty-key")),
                    field("1", owned::FieldPresence::Present, absent("numeric-name")),
                ]
            }
            10 => fields.push(field(
                "nil",
                owned::FieldPresence::Absent,
                absent("duplicate-even-absent"),
            )),
            11 => enumeration = owned::ShapeEnumeration::Unavailable("not-enumerated".into()),
            12 => selected.view.context = "foreign-context".into(),
            13 => selected.identity.clear(),
            14 => selected.epoch.clear(),
            15 => selected.origin = owned::ObjectOrigin::Xml(xml()),
            16 => {
                let mut origin = xml();
                origin.path.clear();
                selected.origin = owned::ObjectOrigin::Xml(origin);
            }
            17 => {
                let mut origin = xml();
                origin.content_sha256.pop();
                selected.origin = owned::ObjectOrigin::Xml(origin);
            }
            18 => {
                let mut origin = site;
                origin.file_id.id = u32::MAX;
                selected.origin = owned::ObjectOrigin::Lua(origin);
            }
            19 => {
                fields[0].source_write = Some(owned::FieldWriteOrigin::Xml(xml()));
                fields[0].copy_origin = Some(owned::FieldWriteOrigin::Xml(xml()));
            }
            20 => {
                fields = (0..128)
                    .map(|i| {
                        field(
                            &format!("key{i}"),
                            owned::FieldPresence::Present,
                            absent(&format!("proof{i}")),
                        )
                    })
                    .collect()
            }
            21 => {
                fields = (0..129)
                    .map(|i| {
                        field(
                            &format!("key{i}"),
                            owned::FieldPresence::Present,
                            absent(&format!("proof{i}")),
                        )
                    })
                    .collect()
            }
            22 => {
                use emmylua_parser::{LuaAstNode, LuaReturnStat};
                let ret = analysis
                    .semantic_model(site.file_id)
                    .syntax_tree()
                    .unwrap()
                    .get_red_root()
                    .descendants()
                    .find_map(LuaReturnStat::cast)
                    .unwrap();
                selected.origin = owned::ObjectOrigin::Lua(owned::BindingSite {
                    file_id: site.file_id,
                    syntax: ret.get_syntax_id(),
                });
            }
            _ => unreachable!(),
        }
        let mut object_shape = shape(&selected, fields);
        object_shape.enumeration = enumeration;
        select_shape(&mut provider, site, &selected, object_shape);
        compare(&analysis, &[site, site], &provider);
        comparisons += 2;
    }
    assert_eq!(comparisons, 46);
}

#[test]
fn nested_incomplete_fields_and_active_object_depth_match_without_flattening_identity() {
    let _trace = trace::begin_candidate();
    for depth in [2, 64, 65] {
        let (analysis, site, mut provider, root) = basic_case();
        for index in 1..=depth {
            let current = object(site, index);
            let fields = if index == depth {
                vec![field(
                    "missing",
                    owned::FieldPresence::Unavailable,
                    absent("ignored"),
                )]
            } else {
                vec![field(
                    "child",
                    owned::FieldPresence::Present,
                    bound_object(&object(site, index + 1)),
                )]
            };
            provider.shapes.insert(
                current.view.clone(),
                owned::BindingDecision::Bound(shape(&current, fields)),
            );
        }
        provider.values.insert(site, bound_object(&root));
        let (values, _) = compare(&analysis, &[site, site], &provider);
        assert!(values[0].object_info().unwrap().complete);
    }
}

#[test]
fn tokens_projection_sites_hash_sets_and_cache_hits_have_the_same_equality_relations() {
    let _trace = trace::begin_candidate();
    let (analysis, site, mut provider) = load("return first,second", "first");
    let second = provider.named("second");
    let selected = object(site, 1);
    select_shape(&mut provider, site, &selected, shape(&selected, vec![]));
    provider.values.insert(second, bound_object(&selected));
    let (mut new, _) = compare(&analysis, &[site, site, second], &provider);
    let (other_new, _) = compare(&analysis, &[site, site, second], &provider);
    new.extend(other_new);
    for a in 0..new.len() {
        for b in 0..new.len() {
            trace::check(
                &format!("token/site equality ({a},{b})"),
                &(new[a] == new[b]),
            );
        }
    }
    trace::check(
        "object.token.unique",
        &new.into_iter().collect::<HashSet<_>>().len(),
    );
}

#[test]
fn actual_unary_tuple_logical_declaration_and_observed_return_transports_keep_nested_carriers() {
    let _trace = trace::begin_candidate();
    for expression in [
        "-selected",
        "#selected",
        "true and {selected}",
        "selected or {}",
    ] {
        let (analysis, site, mut provider) = load(&format!("return {expression}"), expression);
        let selected_site = provider.named("selected");
        let selected = object(selected_site, 1);
        select_shape(
            &mut provider,
            selected_site,
            &selected,
            shape(&selected, vec![]),
        );
        compare(&analysis, &[site, site], &provider);
    }
    for source in [
        "local function f() end\nreturn f()",
        "local function f() end\nlocal first,second=f()\nreturn second",
    ] {
        let query = if source.contains("second") {
            "second"
        } else {
            "f()"
        };
        let (analysis, site, mut provider) = load(source, query);
        let call = provider.named("f()");
        let selected = object(call, 1);
        provider.shapes.insert(
            selected.view.clone(),
            owned::BindingDecision::Bound(shape(&selected, vec![])),
        );
        observed_call(
            &analysis,
            &mut provider,
            call,
            vec![absent("nil-slot"), bound_object(&selected)],
        );
        compare(&analysis, &[site, site], &provider);
    }
}

#[test]
fn the_1025th_shape_keeps_its_real_unavailable_receipt_after_the_shared_freeze_budget() {
    let _trace = trace::begin_candidate();
    let names = (0..1025)
        .map(|i| format!("object{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let (analysis, _, mut provider) = load(&format!("return {names}"), "object0");
    let mut sites = vec![];
    for i in 0..1025 {
        let site = provider.named(&format!("object{i}"));
        let selected = object(site, i + 1);
        select_shape(&mut provider, site, &selected, shape(&selected, vec![]));
        sites.push(site);
    }
    let (new, ledger) = compare(&analysis, &sites, &provider);
    assert!(new[..1024].iter().all(|v| v.object_info().is_some()));
    assert_eq!(new[1024].structural_type(), &LuaType::Unknown);
    assert_eq!(
        ledger.last().unwrap().reason.as_deref(),
        Some("object_shape_budget")
    );
}

#[test]
fn a_second_projection_with_the_same_view_and_different_epoch_keeps_the_frozen_identity_failure() {
    let _trace = trace::begin_candidate();
    let (analysis, first, mut provider) = load("return first,second", "first");
    let second = provider.named("second");
    let selected = object(first, 1);
    select_shape(
        &mut provider,
        first,
        &selected,
        shape(
            &selected,
            vec![field("nil", owned::FieldPresence::Present, absent("nil"))],
        ),
    );
    let mut changed = selected.clone();
    changed.epoch = "epoch-2".into();
    provider.values.insert(second, bound_object(&changed));
    let (new, ledger) = compare(&analysis, &[first, second], &provider);
    assert!(new[0].object_info().is_some());
    assert_eq!(new[1].structural_type(), &LuaType::Unknown);
    assert_eq!(
        ledger.last().unwrap().reason.as_deref(),
        Some("object_frozen_identity_mismatch")
    );
}
