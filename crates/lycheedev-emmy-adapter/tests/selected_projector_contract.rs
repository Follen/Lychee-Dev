#[path = "support/selected_projector_cases.rs"]
mod support;
use emmylua_code_analysis::{LuaType, SemanticId};
use lycheedev_emmy_adapter::session::*;
use std::sync::Arc;
use support::*;

#[test]
fn atomic_decision_boundaries_and_exact_unavailable_reasons() {
    let (analysis, query, statement) = load();
    for case in cases(query, statement) {
        let session =
            OwnedBindingSession::new_checked(Arc::new(Provider), || Ok::<_, Stop>(())).unwrap();
        let projector = SelectedProjector::new(&analysis, &session);
        let value = projector
            .project_decision(
                query,
                case.decision.clone(),
                "resolve",
                &mut AtomicBackend::default(),
            )
            .unwrap();
        let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
        assert_case(&case, &value, &rows, query);
        if case.name == "derived-finite" {
            assert_eq!(value, Some(LuaType::FloatConst(2.5)));
        }
        println!(
            "SELECTED_ATOMIC_PUBLIC {}",
            serde_json::json!({"case":case.name,"type":format!("{value:?}"),"coverage":format!("{rows:?}")})
        );
    }
}

#[test]
fn original_typed_failure_at_every_atomic_checkpoint_never_records_success() {
    let (analysis, query, statement) = load();
    for case in cases(query, statement) {
        let session =
            OwnedBindingSession::new_checked(Arc::new(Provider), || Ok::<_, Stop>(())).unwrap();
        let mut completed = AtomicBackend::default();
        SelectedProjector::new(&analysis, &session)
            .project_decision(query, case.decision.clone(), "resolve", &mut completed)
            .unwrap();
        for ordinal in 1..=completed.checkpoints {
            let session =
                OwnedBindingSession::new_checked(Arc::new(Provider), || Ok::<_, Stop>(())).unwrap();
            let token = Arc::new(());
            let mut interrupted = AtomicBackend {
                checkpoints: 0,
                stop_at: ordinal,
                token: token.clone(),
            };
            let error = SelectedProjector::new(&analysis, &session)
                .project_decision(query, case.decision.clone(), "resolve", &mut interrupted)
                .unwrap_err();
            assert_eq!(error.ordinal, ordinal, "{}", case.name);
            assert!(Arc::ptr_eq(&error.token, &token));
            assert!(
                session
                    .coverage_checked(|| Ok::<_, Stop>(()))
                    .unwrap()
                    .is_empty(),
                "{} at {ordinal}",
                case.name
            );
        }
    }
}

// This deliberately uses an owned carrier rather than LuaType. A dispatcher
// which flattens backend results would erase this identity/epoch/session marker.
#[derive(Debug)]
struct Marker {
    identity: String,
    epoch: String,
    session: Arc<OwnedMarker>,
}
#[derive(Debug)]
struct OwnedMarker;
#[derive(Debug)]
enum Value {
    Plain(LuaType),
    Selected(Arc<Marker>),
}
struct RecursiveBackend {
    calls: Vec<&'static str>,
    child: BindingSite,
    marker: Arc<Marker>,
    fail_after_child: bool,
    token: Arc<()>,
}
impl SelectedProjectionBackend for RecursiveBackend {
    type Value = Value;
    type Error = Stop;
    fn checkpoint(&mut self) -> Result<(), Stop> {
        Ok(())
    }
    fn plain(&mut self, ty: LuaType) -> Value {
        Value::Plain(ty)
    }
    fn expression(
        &mut self,
        projector: &SelectedProjector<'_>,
        _: BindingSite,
        origin: BindingSite,
    ) -> ProjectionResult<Value, Stop> {
        self.calls.push("expression");
        assert_eq!(origin, self.child);
        let result = projector.project_decision(
            origin,
            BindingDecision::KnownAbsent {
                proof: "child-proof".into(),
            },
            "resolve",
            self,
        )?;
        assert!(matches!(result, Some(Value::Plain(LuaType::Nil))));
        if self.fail_after_child {
            return Err(Stop {
                ordinal: 77,
                token: self.token.clone(),
            });
        }
        Ok(Ok(Value::Selected(self.marker.clone())))
    }
    fn parameter(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &SemanticId,
    ) -> ProjectionResult<Value, Stop> {
        self.calls.push("parameter");
        Ok(Err("selected_parameter_type_unavailable"))
    }
    fn function(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &FunctionRef,
    ) -> ProjectionResult<Value, Stop> {
        self.calls.push("function");
        Ok(Ok(Value::Selected(self.marker.clone())))
    }
    fn object(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        object: &ObjectRef,
    ) -> ProjectionResult<Value, Stop> {
        self.calls.push("object");
        assert_eq!(object.identity, self.marker.identity);
        assert_eq!(object.epoch, self.marker.epoch);
        Ok(Ok(Value::Selected(self.marker.clone())))
    }
}
fn recursive(child: BindingSite) -> RecursiveBackend {
    RecursiveBackend {
        calls: vec![],
        child,
        marker: Arc::new(Marker {
            identity: "object-id".into(),
            epoch: "epoch-7".into(),
            session: Arc::new(OwnedMarker),
        }),
        fail_after_child: false,
        token: Arc::new(()),
    }
}

#[test]
fn recursive_children_append_before_parent_and_preserve_owned_carrier() {
    let (analysis, query, statement) = load();
    let session =
        OwnedBindingSession::new_checked(Arc::new(Provider), || Ok::<_, Stop>(())).unwrap();
    let projector = SelectedProjector::new(&analysis, &session);
    let mut backend = recursive(statement);
    let value = projector
        .project_decision(
            query,
            bound(SelectedValue::Expression(statement)),
            "resolve",
            &mut backend,
        )
        .unwrap()
        .unwrap();
    let Value::Selected(marker) = value else {
        panic!("owned carrier was erased")
    };
    assert!(Arc::ptr_eq(&marker, &backend.marker));
    assert!(Arc::ptr_eq(&marker.session, &backend.marker.session));
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].site, statement);
    assert_eq!(rows[0].proof, "child-proof");
    assert_eq!(rows[1].site, query);
    assert_eq!(rows[1].proof, "selection-proof");
    assert_eq!(backend.calls, vec!["expression"]);
}

#[test]
fn unfinished_recursive_parent_has_no_completion_receipt() {
    let (analysis, query, statement) = load();
    let session =
        OwnedBindingSession::new_checked(Arc::new(Provider), || Ok::<_, Stop>(())).unwrap();
    let mut backend = recursive(statement);
    backend.fail_after_child = true;
    let error = SelectedProjector::new(&analysis, &session)
        .project_decision(
            query,
            bound(SelectedValue::Expression(statement)),
            "resolve",
            &mut backend,
        )
        .unwrap_err();
    assert_eq!(error.ordinal, 77);
    assert!(Arc::ptr_eq(&error.token, &backend.token));
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].proof, "child-proof");
}

#[test]
fn context_recursive_branches_are_required_and_never_fall_back_to_raw_types() {
    let (analysis, query, statement) = load();
    let session =
        OwnedBindingSession::new_checked(Arc::new(Provider), || Ok::<_, Stop>(())).unwrap();
    let projector = SelectedProjector::new(&analysis, &session);
    let mut backend = recursive(statement);
    let function = FunctionRef {
        file_id: query.file_id,
        closure: statement.syntax,
        declaration: None,
    };
    let object = ObjectRef {
        identity: "object-id".into(),
        epoch: "epoch-7".into(),
        origin: ObjectOrigin::Lua(statement),
        view: ObjectViewKey {
            context: session.identity().into(),
            observation: 0,
            object_id: 0,
        },
    };
    for value in [
        SelectedValue::Function(function),
        SelectedValue::Object(object),
    ] {
        let result = projector
            .project_decision(query, bound(value), "resolve", &mut backend)
            .unwrap()
            .unwrap();
        let Value::Selected(marker) = result else {
            panic!("selected carrier lost")
        };
        assert!(Arc::ptr_eq(&marker, &backend.marker));
    }
    // Use a real semantic declaration ID, not a fabricated engine-private key.
    let model = analysis.semantic_model(query.file_id);
    let name = query
        .syntax
        .to_node_from_root(&model.syntax_tree().unwrap().get_red_root())
        .and_then(emmylua_parser::LuaNameExpr::cast)
        .unwrap();
    use emmylua_parser::LuaAstNode;
    let id = model
        .resolve_name(name.syntax().text_range().start())
        .unwrap();
    let result = projector
        .project_decision(
            query,
            bound(SelectedValue::Parameter(id)),
            "parameter",
            &mut backend,
        )
        .unwrap();
    assert!(matches!(result, Some(Value::Plain(LuaType::Unknown))));
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(
        rows.last().unwrap().reason.as_deref(),
        Some("selected_parameter_type_unavailable")
    );
    assert_eq!(backend.calls, vec!["function", "object", "parameter"]);
}

#[test]
fn real_dispatch_producers_keep_nested_order_at_16384_boundary() {
    let (analysis, query, statement) = load();
    let session =
        OwnedBindingSession::new_checked(Arc::new(Provider), || Ok::<_, Stop>(())).unwrap();
    let projector = SelectedProjector::new(&analysis, &session);
    let mut atomic = AtomicBackend::default();
    for index in 0..16_381 {
        projector
            .project_decision(
                query,
                BindingDecision::KnownAbsent {
                    proof: format!("prior-{index}"),
                },
                "resolve",
                &mut atomic,
            )
            .unwrap();
    }
    let mut backend = recursive(statement);
    projector
        .project_decision(
            query,
            bound(SelectedValue::Expression(statement)),
            "resolve",
            &mut backend,
        )
        .unwrap();
    // The first child/parent occupy 16381/16382. A different child proof must
    // trigger the sentinel at its actual site, not lose the earlier parent row.
    projector
        .project_decision(
            statement,
            BindingDecision::KnownAbsent {
                proof: "next-child-proof".into(),
            },
            "resolve",
            &mut atomic,
        )
        .unwrap();
    projector
        .project_decision(
            query,
            BindingDecision::Unavailable {
                proof: "later".into(),
                reason: "later".into(),
            },
            "resolve",
            &mut atomic,
        )
        .unwrap();
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(rows.len(), 16_384);
    assert_eq!(rows[16_381].proof, "child-proof");
    assert_eq!(rows[16_382].proof, "selection-proof");
    assert_eq!(
        rows[16_383],
        BindingCoverage {
            site: statement,
            operation: "coverage",
            state: "unavailable",
            proof: String::new(),
            reason: Some("binding_coverage_budget".into()),
            parameter: None,
            parameter_index: None,
            argument: None
        }
    );
}
