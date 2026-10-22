#[path = "support/expression_execution_cases.rs"]
mod support;
use emmylua_code_analysis::{LuaMemberKey, LuaTupleStatus, LuaType, SemanticId};
use lycheedev_emmy_adapter::session::*;
use std::sync::Arc;
use support::{Backend, Marker, Stop, Value, absent, load, select};

fn value(ty: LuaType) -> Value {
    Value {
        ty,
        marker: None,
        slots: vec![],
    }
}
fn tuple_types(value: &Value) -> Vec<LuaType> {
    let LuaType::Tuple(tuple) = &value.ty else {
        panic!("expected tuple")
    };
    assert_eq!(tuple.status, LuaTupleStatus::InferResolve);
    tuple.get_types().to_vec()
}

#[test]
fn member_query_order_precedes_sorting_and_tuple_slots_keep_numeric_key_order() {
    let (analysis, query, mut provider) = load("return {[2]=two,[1]=one}", "{[2]=two,[1]=one}");
    select(
        &mut provider,
        "two",
        BindingDecision::Unavailable {
            proof: "two-proof".into(),
            reason: "two-unknown".into(),
        },
    );
    select(&mut provider, "one", absent("one-proof"));
    let model = analysis.semantic_model(query.file_id);
    let ty = model.type_of_expr(query.syntax);
    let LuaType::TableConst(table) = &ty else {
        unreachable!()
    };
    let owner = SemanticId::member(table.file_id, table.value);
    let expected = model
        .members_of_owner(&owner)
        .iter()
        .map(|member| {
            model
                .file_facts_of(member.file_id)
                .unwrap()
                .member_by_id(&member.id)
                .unwrap()
                .value_syntax
                .unwrap()
        })
        .collect::<Vec<_>>();
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let mut backend = Backend::new(provider.events.clone());
    let DenseTupleProjection::Projected(result) = SelectedProjector::new(&analysis, &session)
        .project_dense_table_tuple(query.file_id, value(ty), &mut backend)
        .unwrap()
    else {
        panic!("dense table rejected")
    };
    assert_eq!(tuple_types(&result), [LuaType::Nil, LuaType::Unknown]);
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(
        rows.iter().map(|row| row.site.syntax).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(result.slots.len(), 2);
}

#[test]
fn invalid_key_stops_at_its_real_position_while_holes_are_rejected_after_all_queries() {
    for table_text in [
        "{[1]=first,named=second,[2]=third}",
        "{[1]=first,[3]=third}",
    ] {
        let (analysis, query, mut provider) = load(&format!("return {table_text}"), table_text);
        for name in ["first", "second", "third"] {
            if provider.names.values().any(|text| text == name) {
                select(&mut provider, name, absent(name));
            }
        }
        let model = analysis.semantic_model(query.file_id);
        let ty = model.type_of_expr(query.syntax);
        let LuaType::TableConst(table) = &ty else {
            unreachable!()
        };
        let mut expected = Vec::new();
        for member in model
            .members_of_owner(&SemanticId::member(table.file_id, table.value))
            .iter()
        {
            let info = model
                .file_facts_of(member.file_id)
                .unwrap()
                .member_by_id(&member.id)
                .unwrap();
            if !matches!(info.key,LuaMemberKey::Integer(key) if key>0) {
                break;
            }
            expected.push(info.value_syntax.unwrap());
        }
        let marker = Arc::new(Marker {
            identity: "original-table".into(),
            epoch: "epoch".into(),
            token: Arc::new(()),
        });
        let input = Value {
            ty: ty.clone(),
            marker: Some(marker.clone()),
            slots: vec![],
        };
        let session =
            OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(()))
                .unwrap();
        let mut backend = Backend::new(provider.events.clone());
        let DenseTupleProjection::Unchanged(result) = SelectedProjector::new(&analysis, &session)
            .project_dense_table_tuple(query.file_id, input, &mut backend)
            .unwrap()
        else {
            panic!("invalid table accepted")
        };
        assert_eq!(result.ty, ty);
        assert!(Arc::ptr_eq(result.marker.as_ref().unwrap(), &marker));
        let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
        assert_eq!(
            rows.iter().map(|row| row.site.syntax).collect::<Vec<_>>(),
            expected
        );
        if table_text.contains("[3]") {
            assert_eq!(rows.len(), 2, "both values queried before density failure");
        }
    }
}

#[test]
fn non_table_and_empty_table_return_the_exact_carrier_without_field_queries() {
    let (analysis, query, provider) = load("return {}", "{}");
    let empty = analysis
        .semantic_model(query.file_id)
        .type_of_expr(query.syntax);
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    for ty in [empty, LuaType::Nil, LuaType::Table, LuaType::Unknown] {
        let marker = Arc::new(Marker {
            identity: "same".into(),
            epoch: "epoch".into(),
            token: Arc::new(()),
        });
        let mut backend = Backend::new(provider.events.clone());
        let input = Value {
            ty: ty.clone(),
            marker: Some(marker.clone()),
            slots: vec![],
        };
        let DenseTupleProjection::Unchanged(result) = SelectedProjector::new(&analysis, &session)
            .project_dense_table_tuple(query.file_id, input, &mut backend)
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(result.ty, ty);
        assert!(Arc::ptr_eq(result.marker.as_ref().unwrap(), &marker));
    }
    assert!(provider.events.lock().unwrap().is_empty());
    assert!(
        session
            .coverage_checked(|| Ok::<_, Stop>(()))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn selected_object_carrier_moves_into_tuple_slot_without_token_epoch_loss() {
    let (analysis, query, mut provider) = load("return {object}", "{object}");
    let site = *provider
        .names
        .iter()
        .find(|(_, text)| text.as_str() == "object")
        .unwrap()
        .0;
    let object = ObjectRef {
        identity: "selected-object".into(),
        epoch: "frozen-9".into(),
        origin: ObjectOrigin::Lua(site),
        view: ObjectViewKey {
            context: "expression-execution-oracle".into(),
            observation: 7,
            object_id: 3,
        },
    };
    select(
        &mut provider,
        "object",
        BindingDecision::Bound(BoundValue {
            identity: object.identity.clone(),
            proof: "object-proof".into(),
            value: SelectedValue::Object(object),
        }),
    );
    let ty = analysis
        .semantic_model(query.file_id)
        .type_of_expr(query.syntax);
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let mut backend = Backend::new(provider.events.clone());
    let token = backend.token.clone();
    let DenseTupleProjection::Projected(result) = SelectedProjector::new(&analysis, &session)
        .project_dense_table_tuple(query.file_id, value(ty), &mut backend)
        .unwrap()
    else {
        unreachable!()
    };
    let marker = result.slots[0].marker.as_ref().unwrap();
    assert_eq!(
        (marker.identity.as_str(), marker.epoch.as_str()),
        ("selected-object", "frozen-9")
    );
    assert!(Arc::ptr_eq(&marker.token, &token));
    assert_eq!(tuple_types(&result), [LuaType::Table]);
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].proof, "object-proof");
    assert_eq!(
        rows[0].operation, "resolve",
        "transport fixture must not claim shape/checker validation"
    );
}

#[test]
fn every_field_sort_and_tuple_checkpoint_keeps_typed_error_and_completed_prefix() {
    let (analysis, query, mut provider) = load("return {[2]=two,[1]=one}", "{[2]=two,[1]=one}");
    select(&mut provider, "two", absent("two"));
    select(&mut provider, "one", absent("one"));
    let ty = analysis
        .semantic_model(query.file_id)
        .type_of_expr(query.syntax);
    let full =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let mut baseline = Backend::new(provider.events.clone());
    SelectedProjector::new(&analysis, &full)
        .project_dense_table_tuple(query.file_id, value(ty.clone()), &mut baseline)
        .unwrap();
    let full_rows = full.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    for ordinal in 1..=baseline.checkpoints {
        let session =
            OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(()))
                .unwrap();
        let mut backend = Backend::new(provider.events.clone());
        backend.stop_at = ordinal;
        let token = backend.token.clone();
        let error = SelectedProjector::new(&analysis, &session)
            .project_dense_table_tuple(query.file_id, value(ty.clone()), &mut backend)
            .unwrap_err();
        assert_eq!(error.ordinal, ordinal);
        assert!(Arc::ptr_eq(&error.token, &token));
        let prefix = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
        assert_eq!(prefix, full_rows[..prefix.len()]);
        backend.stop_at = usize::MAX;
        SelectedProjector::new(&analysis, &session)
            .project_dense_table_tuple(query.file_id, value(ty.clone()), &mut backend)
            .unwrap();
        assert_eq!(
            session.coverage_checked(|| Ok::<_, Stop>(())).unwrap(),
            full_rows
        );
    }
}
