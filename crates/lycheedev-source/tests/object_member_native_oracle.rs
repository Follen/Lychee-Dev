//! Full member records, query values, and producer receipts against the fixed engine.
#![cfg(feature = "emmy-prototype")]
#[path = "support/object_oracle.rs"]
mod object_oracle;
#[path = "../../lycheedev-emmy-adapter/tests/support/object_carrier_cases.rs"]
mod object_support;
#[path = "../../lycheedev-emmy-adapter/tests/support/query_backend_cases.rs"]
mod query_support;
#[path = "../../lycheedev-emmy-adapter/tests/support/object_member_cases.rs"]
mod support;
use emmylua_code_analysis::{EmmyLuaAnalysis, FileId, LuaType, binding_context as native};
use lycheedev_emmy_adapter::{ByteRange, session as owned};
use object_oracle::*;
use std::sync::Arc;
use support::*;

fn compare_queries(
    analysis: &EmmyLuaAnalysis,
    sites: &[owned::BindingSite],
    provider: &Provider,
) -> Vec<owned::BindingCoverage> {
    let old_provider = provider.isolated();
    let new_provider = provider.isolated();
    let old_session = native::BindingSession::new(Arc::new(Native(old_provider.clone())));
    let old_model = analysis.semantic_model_with_context(sites[0].file_id, old_session.clone());
    let new_session = session(&new_provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = owned::OwnedQuery::new(analysis, &new_session, sites[0].file_id, &mut check);
    for site in sites {
        let old = old_model.type_of_expr(site.syntax);
        let new = query.type_of_expr(site.syntax).unwrap();
        assert_eq!(new_type(&new), old_type(&old), "query at {site:?}");
    }
    let ledger = rows(&new_session);
    assert_eq!(
        ledger,
        old_session
            .coverage()
            .into_iter()
            .map(row)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        *new_provider.events.lock().unwrap(),
        *old_provider.events.lock().unwrap()
    );
    println!(
        "OWNED_MEMBER_NATIVE queryComparisons={} coverage={}",
        sites.len(),
        ledger.len()
    );
    ledger
}

#[test]
fn member_provenance_order_presence_and_real_source_identity_match_every_field() {
    let mut comparisons = 0;
    for case in 0..9 {
        let source = "local t={z=1,a=2}; local u={z=3}; return selected";
        let (analysis, site, mut provider, object) = member_case(source, "selected");
        let mut z = field("z", owned::FieldPresence::Present, absent("z"));
        z.destination_write = lua_destination(&analysis, site, "z");
        match case {
            0 => {}
            1 => z.destination_write = owned::FieldWriteOrigin::Xml(xml()),
            2 => {
                z.destination_write = owned::FieldWriteOrigin::Lua(owned::SourceSpan {
                    file_id: FileId::new(u32::MAX),
                    range: ByteRange { start: 0, end: 1 },
                })
            }
            3 => z.destination_write = lua_destination(&analysis, site, "a"),
            4 => {
                z.destination_write = owned::FieldWriteOrigin::Lua(owned::SourceSpan {
                    file_id: site.file_id,
                    range: ByteRange { start: 0, end: 0 },
                })
            }
            5 => {
                z.destination_write = owned::FieldWriteOrigin::Lua(owned::SourceSpan {
                    file_id: site.file_id,
                    range: ByteRange {
                        start: 0,
                        end: source.len() as u32,
                    },
                })
            }
            6 => z.presence = owned::FieldPresence::Unavailable,
            7 => z.presence = owned::FieldPresence::Absent,
            8 => {
                z.source_write = Some(lua_destination(&analysis, site, "a"));
                z.copy_origin = z.source_write.clone();
            }
            _ => unreachable!(),
        }
        replace_fields(
            &mut provider,
            &object,
            vec![
                z,
                field("a", owned::FieldPresence::Present, environment_any()),
                field("omitted", owned::FieldPresence::Absent, absent("omit")),
            ],
        );
        let old_provider = provider.isolated();
        let new_provider = provider.isolated();
        let old_session = native::BindingSession::new(Arc::new(Native(old_provider.clone())));
        let old_model = analysis.semantic_model_with_context(site.file_id, old_session.clone());
        let new_session = session(&new_provider);
        let mut check = || Ok::<_, Stop>(());
        let mut query = owned::OwnedQuery::new(&analysis, &new_session, site.file_id, &mut check);
        let old = old_model.type_of_expr(site.syntax);
        let new = query.type_of_expr(site.syntax).unwrap();
        assert_eq!(new_type(&new), old_type(&old));
        comparisons += 1;
        for _ in 0..2 {
            let old_members = old_model.member_infos(&old);
            let new_members = query.selected_members(&new).unwrap().unwrap();
            assert_eq!(
                new_members
                    .iter()
                    .map(|m| (
                        m.key.clone(),
                        new_type(&m.typ),
                        m.id.clone(),
                        m.file_id,
                        m.is_method
                    ))
                    .collect::<Vec<_>>(),
                old_members
                    .iter()
                    .map(|m| (
                        m.key.clone(),
                        old_type(&m.typ),
                        m.id.clone(),
                        m.file_id,
                        m.is_method
                    ))
                    .collect::<Vec<_>>(),
                "member case {case}"
            );
            comparisons += 1;
        }
        assert_eq!(
            rows(&new_session),
            old_session
                .coverage()
                .into_iter()
                .map(row)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            *new_provider.events.lock().unwrap(),
            *old_provider.events.lock().unwrap()
        );
    }
    assert_eq!(comparisons, 27);
}

#[test]
fn static_and_dynamic_object_index_execute_the_real_query_successor() {
    let expressions = [
        "selected.z",
        "selected.a",
        "selected.uncertain",
        "selected.omitted",
        "selected.missing",
        "selected[1]",
        "selected['z']",
        "selected[true]",
        "selected[1.5]",
        "selected[nil]",
        "selected[key]",
    ];
    let mut comparisons = 0;
    for expr in expressions {
        let source = if expr == "selected[key]" {
            format!("local key='z'; return {expr}")
        } else {
            format!("return {expr}")
        };
        let (analysis, site, provider, _) = member_case(&source, expr);
        compare_queries(&analysis, &[site, site], &provider);
        comparisons += 2;
    }
    // Public environment slot projection supplies truly broad runtime keys;
    // these must not be treated as a known constant or enumerate Object fields.
    for kind in ["string", "number", "integer", "boolean", "any"] {
        let (analysis, site, mut provider, _) =
            member_case("return selected[key]", "selected[key]");
        let owned::BindingDecision::Bound(mut value) = environment_any() else {
            unreachable!()
        };
        let owned::SelectedValue::EnvironmentReturn(mut ret) = value.value else {
            unreachable!()
        };
        let mut callable = (*ret.callable).clone();
        callable.declaration.returns[0].type_name = kind.into();
        ret.callable = Arc::new(callable);
        value.value = owned::SelectedValue::EnvironmentReturn(ret);
        provider.select("key", owned::BindingDecision::Bound(value));
        compare_queries(&analysis, &[site, site], &provider);
        comparisons += 2;
    }
    for callable_signature in [false, true] {
        let (analysis, site, mut provider, object) = member_case("return selected.f", "selected.f");
        let owned::BindingDecision::Bound(mut value) = environment_any() else {
            unreachable!()
        };
        let owned::SelectedValue::EnvironmentReturn(mut ret) = value.value else {
            unreachable!()
        };
        if callable_signature {
            value.value = owned::SelectedValue::EnvironmentCallable(ret.callable);
        } else {
            let mut callable = (*ret.callable).clone();
            callable.declaration.returns[0].type_name = "function".into();
            ret.callable = Arc::new(callable);
            value.value = owned::SelectedValue::EnvironmentReturn(ret);
        }
        replace_fields(
            &mut provider,
            &object,
            vec![field(
                "f",
                owned::FieldPresence::Present,
                owned::BindingDecision::Bound(value),
            )],
        );
        compare_queries(&analysis, &[site, site], &provider);
        comparisons += 2;
    }
    assert_eq!(comparisons, 36);
}

#[test]
fn nested_member_results_keep_full_carrier_and_subsequent_lookup_order() {
    let (analysis, site, mut provider, parent) =
        member_case("return selected.child.z", "selected.child.z");
    let child = object(provider.named("selected"), 2);
    provider.shapes.insert(
        child.view.clone(),
        owned::BindingDecision::Bound(shape(
            &child,
            vec![field("z", owned::FieldPresence::Present, absent("child-z"))],
        )),
    );
    replace_fields(
        &mut provider,
        &parent,
        vec![field(
            "child",
            owned::FieldPresence::Present,
            bound_object(&child),
        )],
    );
    let ledger = compare_queries(
        &analysis,
        &[site, provider.named("selected.child"), site],
        &provider,
    );
    assert_eq!(
        ledger
            .iter()
            .filter(|r| r.operation == "member_lookup")
            .map(|r| r.proof.as_str())
            .collect::<Vec<_>>(),
        ["shape:object-1", "shape:object-2"]
    );
}

#[test]
fn same_context_foreign_token_member_lookup_is_unavailable_in_both_engines() {
    let (analysis, site, provider, _) = basic_case();
    let old_provider = provider.isolated();
    let new_provider = provider.isolated();
    let old_a = native::BindingSession::new(Arc::new(Native(old_provider.clone())));
    let old_b = native::BindingSession::new(Arc::new(Native(old_provider.clone())));
    let old = analysis
        .semantic_model_with_context(site.file_id, old_a.clone())
        .type_of_expr(site.syntax);
    let new_a = session(&new_provider);
    let new_b = session(&new_provider);
    let mut check = || Ok::<_, Stop>(());
    let new = owned::OwnedQuery::new(&analysis, &new_a, site.file_id, &mut check)
        .type_of_expr(site.syntax)
        .unwrap();
    assert_eq!(new_type(&new), old_type(&old));
    let old_model = analysis.semantic_model_with_context(site.file_id, old_b.clone());
    let mut query = owned::OwnedQuery::new(&analysis, &new_b, site.file_id, &mut check);
    for _ in 0..2 {
        assert!(old_model.member_infos(&old).is_empty());
        assert!(query.selected_members(&new).unwrap().unwrap().is_empty());
    }
    assert_eq!(
        rows(&new_a),
        old_a.coverage().into_iter().map(row).collect::<Vec<_>>()
    );
    assert_eq!(
        rows(&new_b),
        old_b.coverage().into_iter().map(row).collect::<Vec<_>>()
    );
    assert_eq!(
        rows(&new_b)[0].reason.as_deref(),
        Some("object_context_mismatch")
    );
    assert_eq!(
        *new_provider.events.lock().unwrap(),
        *old_provider.events.lock().unwrap()
    );
}

#[test]
fn member_lookup_keeps_the_actual_16384_last_slot_producer() {
    for count in [16_380, 16_381, 16_382] {
        let names = (0..count)
            .map(|i| format!("prefix{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let dense = format!("false or {{{names}}}");
        let source = format!("local before={dense}; return selected.z");
        let (analysis, site, mut provider, object) = member_case(&source, "selected.z");
        replace_fields(
            &mut provider,
            &object,
            vec![field("z", owned::FieldPresence::Present, absent("z"))],
        );
        for (site, name) in &provider.names {
            if let Some(index) = name
                .strip_prefix("prefix")
                .and_then(|s| s.parse::<usize>().ok())
            {
                assert!(index < count);
                provider
                    .values
                    .insert(*site, absent(&format!("prefix-proof{index}")));
            }
        }
        let ledger = compare_queries(&analysis, &[provider.named(&dense), site], &provider);
        assert_eq!(ledger.len(), 16_384);
        assert_eq!(
            ledger.last().unwrap().reason.as_deref(),
            Some("binding_coverage_budget")
        );
        assert_eq!(ledger.last().unwrap().site, provider.named("selected"));
        if count == 16_380 {
            assert_eq!(ledger[16_382].operation, "resolve");
        }
        if count == 16_381 {
            assert_eq!(ledger[16_382].operation, "object_shape");
        }
        if count == 16_382 {
            assert_eq!(ledger[16_382].operation, "object_field");
        }
    }
}
