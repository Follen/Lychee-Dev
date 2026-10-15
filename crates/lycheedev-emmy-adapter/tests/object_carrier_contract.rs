#[path = "support/query_backend_cases.rs"]
mod query_support;
#[path = "support/object_carrier_cases.rs"]
mod support;
use emmylua_code_analysis::{LuaMemberKey, LuaType};
use lycheedev_emmy_adapter::session::*;
use std::{cell::Cell, collections::HashSet, sync::Arc};
use support::*;
#[test]
fn concrete_object_fields_keep_exact_producer_order_presence_and_completeness() {
    let (analysis, site, mut provider, object) = basic_case();
    provider.shapes.insert(
        object.view.clone(),
        BindingDecision::Bound(shape(
            &object,
            vec![
                field("omitted", FieldPresence::Absent, absent("omitted")),
                field("nil", FieldPresence::Present, absent("nil")),
                field("any", FieldPresence::Present, environment_any()),
                field(
                    "unavailable",
                    FieldPresence::Unavailable,
                    absent("must-not-project"),
                ),
                field(
                    "unmanaged",
                    FieldPresence::Present,
                    BindingDecision::NotManaged,
                ),
            ],
        )),
    );
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let value = OwnedQuery::new(&analysis, &session, site.file_id, &mut check)
        .type_of_expr(site.syntax)
        .unwrap();
    let info = value.object_info().unwrap();
    assert!(!info.complete);
    assert_eq!(info.context, provider.identity());
    assert_eq!(info.epoch, object.epoch);
    let fields = value.carried_fields().unwrap();
    assert_eq!(fields.len(), 4);
    assert!(!fields.contains_key(&LuaMemberKey::Name("omitted".into())));
    assert_eq!(
        fields[&LuaMemberKey::Name("any".into())].structural_type(),
        &LuaType::Any
    );
    let coverage = rows(&session);
    assert_eq!(
        coverage
            .iter()
            .map(|row| (row.operation, row.proof.as_str()))
            .collect::<Vec<_>>(),
        [
            ("object_field", "nil"),
            ("object_field", "any-proof"),
            ("object_shape", "shape:object-1"),
            ("resolve", "selected:object-1")
        ]
    );
    assert_eq!(
        coverage[2].reason.as_deref(),
        Some("object_shape_incomplete")
    );
    assert_eq!(coverage[3].state, "bound");
}
#[test]
fn nested_incomplete_object_does_not_incorrectly_mark_its_parent_incomplete() {
    let (analysis, site, mut provider, parent) = basic_case();
    let child = object(site, 2);
    provider.shapes.insert(
        child.view.clone(),
        BindingDecision::Bound(shape(
            &child,
            vec![field(
                "missing",
                FieldPresence::Unavailable,
                absent("ignored"),
            )],
        )),
    );
    provider.shapes.insert(
        parent.view.clone(),
        BindingDecision::Bound(shape(
            &parent,
            vec![field("child", FieldPresence::Present, bound_object(&child))],
        )),
    );
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let value = OwnedQuery::new(&analysis, &session, site.file_id, &mut check)
        .type_of_expr(site.syntax)
        .unwrap();
    assert!(value.object_info().unwrap().complete);
    assert!(
        !value.carried_fields().unwrap()[&LuaMemberKey::Name("child".into())]
            .object_info()
            .unwrap()
            .complete
    );
    assert_eq!(
        rows(&session)
            .iter()
            .map(|row| row.operation)
            .collect::<Vec<_>>(),
        ["object_shape", "object_field", "object_shape", "resolve"]
    );
}
#[test]
fn metadata_token_and_nested_slot_identity_survive_all_accepted_transports() {
    let (analysis, site, provider, _) = basic_case();
    let first_session = session(&provider);
    let second_session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut first_query = OwnedQuery::new(&analysis, &first_session, site.file_id, &mut check);
    let value = first_query.type_of_expr(site.syntax).unwrap();
    let ready = first_query.type_of_expr(site.syntax).unwrap();
    assert_eq!(value, ready);
    let token_peer = OwnedQuery::new(&analysis, &second_session, site.file_id, &mut || {
        Ok::<_, Stop>(())
    })
    .type_of_expr(site.syntax)
    .unwrap();
    assert_eq!(value.structural_type(), token_peer.structural_type());
    assert_ne!(value, token_peer);
    let mut set = HashSet::new();
    set.insert(value.clone());
    set.insert(ready);
    set.insert(token_peer.clone());
    assert_eq!(set.len(), 2);
    let three = first_query.plain(LuaType::IntegerConst(3));
    let pack = first_query.result_pack(vec![value.clone(), three]);
    let scalar = first_query.scalar_result(pack.clone());
    assert_eq!(scalar, value);
    assert_eq!(pack.carried_slots().unwrap()[0], value);
    let inferred_tuple = first_query
        .tuple_value(
            vec![value.clone()],
            emmylua_code_analysis::LuaTupleStatus::InferResolve,
        )
        .unwrap();
    let documented_tuple = first_query
        .tuple_value(
            vec![value.clone()],
            emmylua_code_analysis::LuaTupleStatus::DocResolve,
        )
        .unwrap();
    assert_ne!(inferred_tuple, documented_tuple);
    let mut tuples = HashSet::new();
    tuples.insert(inferred_tuple.clone());
    tuples.insert(inferred_tuple);
    tuples.insert(documented_tuple);
    assert_eq!(tuples.len(), 2);
    assert!(matches!(
        first_query.merge_value(value, token_peer),
        Err(OwnedQueryFailure::Continue(OwnedQueryContinuation {
            capability: OwnedQueryCapability::SelectedTypeMerge,
            ..
        }))
    ));
}
#[test]
fn unary_tuple_selected_initializer_and_observed_result_slots_retain_the_carrier() {
    for expression in [
        "-selected",
        "#selected",
        "true and {selected}",
        "selected or {}",
    ] {
        let (analysis, site, mut provider) = load(&format!("return {expression}"), expression);
        let object_site = provider.named("selected");
        let selected = object(object_site, 1);
        select_shape(
            &mut provider,
            object_site,
            &selected,
            shape(&selected, vec![]),
        );
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let value = OwnedQuery::new(&analysis, &session, site.file_id, &mut check)
            .type_of_expr(site.syntax)
            .unwrap();
        if expression.contains("{selected}") {
            assert_eq!(
                value.carried_slots().unwrap()[0]
                    .object_info()
                    .unwrap()
                    .identity,
                selected.identity
            );
        } else {
            assert_eq!(value.object_info().unwrap().site, object_site);
        }
    }
    let (analysis, site, mut provider) = load(
        "local function f() end\nlocal first,second=f()\nreturn second",
        "second",
    );
    let call = provider.named("f()");
    let selected = object(call, 1);
    provider.shapes.insert(
        selected.view.clone(),
        BindingDecision::Bound(shape(&selected, vec![])),
    );
    observed_call(
        &analysis,
        &mut provider,
        call,
        vec![absent("first-nil"), bound_object(&selected)],
    );
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    let value = query.type_of_expr(site.syntax).unwrap();
    assert_eq!(value.object_info().unwrap().site, call);
    let decl = declaration(&analysis, site.file_id, "second");
    assert_eq!(query.type_of_decl(&decl).unwrap().unwrap(), value);
}
#[test]
fn duplicate_failure_keeps_earlier_receipts_and_shape_freeze_without_faking_completion() {
    let (analysis, site, mut provider, object) = basic_case();
    provider.shapes.insert(
        object.view.clone(),
        BindingDecision::Bound(shape(
            &object,
            vec![
                field("same", FieldPresence::Present, absent("first")),
                field("same", FieldPresence::Absent, absent("duplicate")),
            ],
        )),
    );
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    let value = query.type_of_expr(site.syntax).unwrap();
    assert_eq!(value.structural_type(), &LuaType::Unknown);
    let coverage = rows(&session);
    assert_eq!(coverage.len(), 2);
    assert_eq!(coverage[0].proof, "first");
    assert_eq!(
        coverage[1].reason.as_deref(),
        Some("object_duplicate_field")
    );
    assert!(coverage.iter().all(|row| row.operation != "object_shape"));
    let mut child_check = || Ok::<_, Stop>(());
    let mut child = OwnedQuery::new(&analysis, &session, site.file_id, &mut child_check);
    assert_eq!(child.type_of_expr(site.syntax).unwrap(), value);
    assert_eq!(
        provider
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, Event::Shape(_)))
            .count(),
        1
    );
}
#[test]
fn every_object_checkpoint_stop_releases_guard_and_keeps_original_error_and_prefix() {
    let (analysis, site, provider, _) = basic_case();
    let reference = provider.isolated();
    let reference_session = session(&reference);
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
        let count = Cell::new(0);
        let enabled = Cell::new(true);
        let mut check = || {
            count.set(count.get() + 1);
            if enabled.get() && count.get() == stop {
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
        let retry = query.type_of_expr(site.syntax).unwrap();
        assert_eq!(retry.structural_type(), expected.structural_type());
        assert_eq!(
            retry.object_info().unwrap().identity,
            expected.object_info().unwrap().identity
        );
        assert_eq!(rows(&session), expected_rows);
    }
}

#[test]
fn a_function_returning_a_selected_object_preserves_child_work_and_defers_instead_of_erasing_identity()
 {
    use emmylua_parser::{LuaAstNode, LuaClosureExpr};
    let (analysis, site, mut provider) = load(
        "local function f() return object_value end\nreturn function_value",
        "function_value",
    );
    let object_site = provider.named("object_value");
    let selected = object(object_site, 1);
    select_shape(
        &mut provider,
        object_site,
        &selected,
        shape(&selected, vec![]),
    );
    let closure = analysis
        .semantic_model(site.file_id)
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .find_map(LuaClosureExpr::cast)
        .unwrap();
    provider.values.insert(
        site,
        BindingDecision::Bound(BoundValue {
            identity: "function".into(),
            proof: "function-proof".into(),
            value: SelectedValue::Function(FunctionRef {
                file_id: site.file_id,
                closure: closure.get_syntax_id(),
                declaration: None,
            }),
        }),
    );
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    let Err(OwnedQueryFailure::Continue(continuation)) = query.type_of_expr(site.syntax) else {
        panic!("selected object return carrier must not be erased")
    };
    assert_eq!(
        continuation.capability,
        OwnedQueryCapability::FunctionReturnType
    );
    assert_eq!(continuation.syntax, Some(object_site.syntax));
    let coverage = rows(&session);
    assert_eq!(
        coverage.iter().map(|row| row.operation).collect::<Vec<_>>(),
        ["object_shape", "resolve"]
    );
    assert!(coverage.iter().all(|row| row.proof != "function-proof"));
}
