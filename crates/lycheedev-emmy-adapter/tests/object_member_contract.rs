#[path = "support/object_carrier_cases.rs"]
mod object_support;
#[path = "support/query_backend_cases.rs"]
mod query_support;
#[path = "support/object_member_cases.rs"]
mod support;
use emmylua_code_analysis::{LuaMemberKey, LuaType, SemanticId};
use lycheedev_emmy_adapter::session::*;
use std::{cell::Cell, sync::Arc};
use support::*;

#[test]
fn member_producer_keeps_field_order_destination_identity_and_complete_receipt() {
    let (analysis, site, mut provider, object) =
        member_case("local t={z=1, a=2}; return selected", "selected");
    let mut z = field("z", FieldPresence::Present, absent("z"));
    z.destination_write = lua_destination(&analysis, site, "z");
    let mut a = field("a", FieldPresence::Present, environment_any());
    a.source_write = Some(lua_destination(&analysis, site, "a")); // Source/copy do not replace destination.
    a.copy_origin = a.source_write.clone();
    replace_fields(
        &mut provider,
        &object,
        vec![
            z,
            field("omitted", FieldPresence::Absent, absent("absent")),
            a,
        ],
    );
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    let value = query.type_of_expr(site.syntax).unwrap();
    let members = query.selected_members(&value).unwrap().unwrap();
    assert_eq!(
        members
            .iter()
            .map(|m| m.key.name().unwrap())
            .collect::<Vec<_>>(),
        ["z", "a"]
    );
    assert_eq!(members[0].file_id, Some(site.file_id));
    assert!(members[0].id.is_some());
    assert_eq!(members[1].file_id, None);
    assert_eq!(members[1].id, None);
    assert!(!members.iter().any(|m| m.is_method));
    assert_eq!(members[0].typ.structural_type(), &LuaType::Nil);
    assert_eq!(members[1].typ.structural_type(), &LuaType::Any);
    let expected = rows(&session);
    assert_eq!(expected.last().unwrap().operation, "member_lookup");
    assert_eq!(expected.last().unwrap().state, "bound");
    assert_eq!(expected.last().unwrap().site, site);
    let count = provider.events.lock().unwrap().len();
    assert_eq!(query.selected_members(&value).unwrap().unwrap(), members);
    assert_eq!(rows(&session), expected);
    assert_eq!(provider.events.lock().unwrap().len(), count);
}

#[test]
fn foreign_token_with_identical_context_never_returns_structural_members() {
    let (analysis, site, provider, _) = basic_case();
    let first = session(&provider);
    let second = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let value = OwnedQuery::new(&analysis, &first, site.file_id, &mut check)
        .type_of_expr(site.syntax)
        .unwrap();
    let mut query = OwnedQuery::new(&analysis, &second, site.file_id, &mut check);
    assert!(query.selected_members(&value).unwrap().unwrap().is_empty());
    let rows = rows(&second);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].operation, "member_lookup");
    assert_eq!(rows[0].reason.as_deref(), Some("object_context_mismatch"));
    assert_eq!(rows[0].proof, "shape:object-1");
}

#[test]
fn actual_index_queries_preserve_missing_nil_unknown_any_and_nonliteral_behavior() {
    for (expr, expected) in [
        ("selected.z", LuaType::Nil),
        ("selected.a", LuaType::Any),
        ("selected.uncertain", LuaType::Unknown),
        ("selected.omitted", LuaType::Nil),
        ("selected.missing", LuaType::Nil),
        ("selected[1]", LuaType::Nil),
        ("selected['z']", LuaType::Nil),
        ("selected[true]", LuaType::Unknown),
        // The fixed parser presents a numeric index token, so the VM
        // executes static member lookup even for a non-integer literal.
        ("selected[1.5]", LuaType::Nil),
        ("selected[nil]", LuaType::Unknown),
    ] {
        let (analysis, site, provider, _) = member_case(&format!("return {expr}"), expr);
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
        let value = query.type_of_expr(site.syntax).unwrap();
        assert!(value.is_context_free());
        assert_eq!(value.structural_type(), &expected, "{expr}");
        let rows = rows(&session);
        let lookup = rows.iter().find(|r| r.operation == "member_lookup");
        if matches!(expr, "selected[true]" | "selected[nil]") {
            assert!(lookup.is_none());
        } else {
            assert_eq!(
                lookup.unwrap().reason.as_deref(),
                Some("object_shape_incomplete")
            );
        }
        assert_eq!(query.type_of_expr(site.syntax).unwrap(), value);
    }
}

#[test]
fn chained_object_members_keep_exact_nested_carrier_and_receiver() {
    let (analysis, site, mut provider, parent) =
        member_case("return selected.child.z", "selected.child.z");
    let child = object(provider.named("selected"), 2);
    provider.shapes.insert(
        child.view.clone(),
        BindingDecision::Bound(shape(
            &child,
            vec![field("z", FieldPresence::Present, absent("child-z"))],
        )),
    );
    replace_fields(
        &mut provider,
        &parent,
        vec![field("child", FieldPresence::Present, bound_object(&child))],
    );
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    let result = query.type_of_expr(site.syntax).unwrap();
    assert_eq!(result.structural_type(), &LuaType::Nil);
    let child_value = query
        .type_of_expr(provider.named("selected.child").syntax)
        .unwrap();
    assert_eq!(child_value.object_info().unwrap().identity, child.identity);
    let parent_value = query
        .type_of_expr(provider.named("selected").syntax)
        .unwrap();
    let projector = SelectedProjector::new(&analysis, &session);
    let indexed = ExpressionExecutionBackend::index_member(
        &mut query,
        &projector,
        site.file_id,
        ExpressionValue::plain(parent_value.clone()),
        &LuaMemberKey::Name("child".into()),
    )
    .unwrap();
    assert_eq!(indexed.value, child_value);
    assert_eq!(indexed.receiver, Some(parent_value));
    assert_eq!(indexed.owner, None);
    assert_eq!(indexed.closure_syntax, None);
    assert_eq!(
        rows(&session)
            .iter()
            .filter(|r| r.operation == "member_lookup")
            .map(|r| r.proof.as_str())
            .collect::<Vec<_>>(),
        ["shape:object-1", "shape:object-2"]
    );
}

#[test]
fn each_member_checkpoint_keeps_original_stop_prefix_and_retry() {
    let (analysis, site, mut provider, object) =
        member_case("local t={z=1,a=2}; return selected", "selected");
    let mut z = field("z", FieldPresence::Present, absent("z"));
    z.destination_write = lua_destination(&analysis, site, "z");
    replace_fields(
        &mut provider,
        &object,
        vec![z, field("a", FieldPresence::Present, environment_any())],
    );
    let total = Cell::new(0);
    let baseline = session(&provider);
    let mut okay = || Ok::<_, Stop>(());
    let value = OwnedQuery::new(&analysis, &baseline, site.file_id, &mut okay)
        .type_of_expr(site.syntax)
        .unwrap();
    let mut count = || {
        total.set(total.get() + 1);
        Ok::<_, Stop>(())
    };
    let expected = OwnedQuery::new(&analysis, &baseline, site.file_id, &mut count)
        .selected_members(&value)
        .unwrap()
        .unwrap();
    let expected_rows = rows(&baseline);
    for stop in 1..=total.get() {
        let session = session(&provider.isolated());
        let value = OwnedQuery::new(&analysis, &session, site.file_id, &mut okay)
            .type_of_expr(site.syntax)
            .unwrap();
        let token = Arc::new(());
        let ordinal = Cell::new(0);
        let mut check = || {
            ordinal.set(ordinal.get() + 1);
            if ordinal.get() == stop {
                Err(Stop {
                    ordinal: stop,
                    token: token.clone(),
                })
            } else {
                Ok(())
            }
        };
        let error = OwnedQuery::new(&analysis, &session, site.file_id, &mut check)
            .selected_members(&value)
            .unwrap_err();
        let OwnedQueryFailure::Control(error) = error else {
            panic!("typed control was replaced")
        };
        assert_eq!(error.ordinal, stop);
        assert!(Arc::ptr_eq(&error.token, &token));
        let prefix = rows(&session);
        assert_eq!(prefix, expected_rows[..prefix.len()]);
        let retry = OwnedQuery::new(&analysis, &session, site.file_id, &mut okay)
            .selected_members(&value)
            .unwrap()
            .unwrap();
        assert_eq!(retry, expected);
        assert_eq!(rows(&session), expected_rows);
    }
}

#[test]
fn source_member_callable_lookup_preserves_selected_value_and_destination_producer() {
    let (analysis, site, mut provider, object) =
        member_case("local t={z=1}; return selected.z", "selected.z");
    let mut z = field("z", FieldPresence::Present, absent("z"));
    z.destination_write = lua_destination(&analysis, site, "z");
    replace_fields(&mut provider, &object, vec![z]);
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query =
        OwnedQuery::new_for_snapshot(&analysis, &session, site.file_id, &mut check).unwrap();
    let value = query.type_of_expr(site.syntax).unwrap();
    // The destination identifies the producer; its source initializer (1) must
    // not override the selected KnownAbsent field while discovering callables.
    assert_eq!(value, OwnedValue::plain(LuaType::Nil));
    assert!(value.is_context_free());
    let object_site = provider.named("selected");
    let owner = query.type_of_expr(object_site.syntax).unwrap();
    let expected_member = SemanticId::member(
        site.file_id,
        rowan::TextRange::new(9.into(), 10.into()), // The source field's `z` token.
    );
    assert_eq!(
        query.selected_members(&owner).unwrap().unwrap(),
        [OwnedMemberInfo {
            key: LuaMemberKey::Name("z".into()),
            typ: OwnedValue::plain(LuaType::Nil),
            id: Some(expected_member),
            file_id: Some(site.file_id),
            is_method: false,
        }]
    );
    let projector = SelectedProjector::new(&analysis, &session);
    let indexed = ExpressionExecutionBackend::index_member(
        &mut query,
        &projector,
        site.file_id,
        ExpressionValue::plain(owner.clone()),
        &LuaMemberKey::Name("z".into()),
    )
    .unwrap();
    assert_eq!(indexed.value, value);
    assert_eq!(indexed.receiver, Some(owner));
    assert_eq!(indexed.owner, None);
    assert_eq!(indexed.closure_syntax, None);
    assert_eq!(query.type_of_expr(site.syntax).unwrap(), value);
    assert_eq!(
        rows(&session),
        [
            ("object_field", "known_absent", "z"),
            ("object_shape", "bound", "shape:object-1"),
            ("resolve", "bound", "selected:object-1"),
            ("member_lookup", "bound", "shape:object-1"),
        ]
        .map(|(operation, state, proof)| BindingCoverage {
            site: object_site,
            operation,
            state,
            proof: proof.into(),
            reason: None,
            parameter: None,
            parameter_index: None,
            argument: None,
        })
    );
    assert_eq!(rows(&session).last().unwrap().operation, "member_lookup");
}

#[test]
fn interrupted_index_query_never_caches_partial_success_or_loses_the_caller_stop() {
    let (analysis, site, provider, _) = member_case("return selected.z", "selected.z");
    let baseline = session(&provider.isolated());
    let total = Cell::new(0);
    let mut count = || {
        total.set(total.get() + 1);
        Ok::<_, Stop>(())
    };
    let expected = OwnedQuery::new(&analysis, &baseline, site.file_id, &mut count)
        .type_of_expr(site.syntax)
        .unwrap();
    let expected_rows = rows(&baseline);
    for stop in 1..=total.get() {
        let session = session(&provider.isolated());
        let token = Arc::new(());
        let ordinal = Cell::new(0);
        let mut check = || {
            ordinal.set(ordinal.get() + 1);
            if ordinal.get() == stop {
                Err(Stop {
                    ordinal: stop,
                    token: token.clone(),
                })
            } else {
                Ok(())
            }
        };
        let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
        let error = query.type_of_expr(site.syntax).unwrap_err();
        let OwnedQueryFailure::Control(error) = error else {
            panic!("stop became semantic continuation")
        };
        assert_eq!(error.ordinal, stop);
        assert!(Arc::ptr_eq(&error.token, &token));
        let prefix = rows(&session);
        assert_eq!(prefix, expected_rows[..prefix.len()]);
        assert_eq!(query.type_of_expr(site.syntax).unwrap(), expected);
        assert_eq!(rows(&session), expected_rows);
    }
}

#[test]
fn key_enumeration_and_value_completeness_are_independent_for_missing_members() {
    for complete_keys in [true, false] {
        for (expr, expected) in [
            (
                "selected.missing",
                if complete_keys {
                    LuaType::Nil
                } else {
                    LuaType::Unknown
                },
            ),
            (
                "selected[1]",
                if complete_keys {
                    LuaType::Nil
                } else {
                    LuaType::Unknown
                },
            ),
            (
                "selected[1.5]",
                if complete_keys {
                    LuaType::Nil
                } else {
                    LuaType::Unknown
                },
            ),
            ("selected.omitted", LuaType::Nil),
            ("selected.uncertain", LuaType::Unknown),
            ("selected.contradictory", LuaType::Unknown),
        ] {
            let (analysis, site, mut provider, object) =
                member_case(&format!("return {expr}"), expr);
            let BindingDecision::Bound(shape) = provider.shapes.get_mut(&object.view).unwrap()
            else {
                unreachable!()
            };
            shape.enumeration = if complete_keys {
                ShapeEnumeration::Complete
            } else {
                ShapeEnumeration::Unavailable("incomplete-key-set".into())
            };
            shape.fields.push(field(
                "contradictory",
                FieldPresence::Absent,
                environment_any(),
            ));
            let session = session(&provider);
            let mut check = || Ok::<_, Stop>(());
            let mut query =
                OwnedQuery::new_for_snapshot(&analysis, &session, site.file_id, &mut check)
                    .unwrap();
            assert_eq!(
                plain_type(&query.type_of_expr(site.syntax).unwrap()),
                expected,
                "{expr}"
            );
            let ledger = rows(&session);
            let lookup = ledger
                .iter()
                .find(|row| row.operation == "member_lookup")
                .unwrap();
            assert_eq!(lookup.state, "unavailable");
            assert_eq!(lookup.reason.as_deref(), Some("object_shape_incomplete"));
            assert_eq!(lookup.proof, "shape:object-1");
            let events = provider.events.lock().unwrap().clone();
            assert_eq!(
                plain_type(&query.type_of_expr(site.syntax).unwrap()),
                expected
            );
            assert_eq!(rows(&session), ledger);
            assert_eq!(*provider.events.lock().unwrap(), events);
        }
    }
}

fn plain_type(value: &OwnedValue) -> LuaType {
    assert!(value.is_context_free());
    value.structural_type().clone()
}
