// Migrated unchanged semantic assertions from the pinned reconstruction's tests.
// Ingestion, semantic queries, diagnostics and coverage use the owned session.
#[path = "support/public_ingestion.rs"]
mod public_ingestion;
use emmylua_code_analysis::FileId;
use lycheedev_emmy_adapter::session::*;
#[path = "support/owned_query.rs"]
mod owned_query;
use emmylua_code_analysis::{EmmyLuaAnalysis, LuaType};
use emmylua_parser::{LuaAstNode, LuaExpr};
use owned_query::{OwnedQueryTest, SessionCoverageTest};
use public_ingestion::PublicIngestionTest;
use std::collections::HashMap;
use std::sync::Arc;

struct Context {
    identity: String,
    decisions: HashMap<BindingSite, BindingDecision<BoundValue>>,
    calls: HashMap<BindingSite, BindingDecision<CallPermit>>,
    observations: HashMap<BindingSite, CallObservation>,
    shapes: HashMap<ObjectViewKey, ObjectShape>,
}
impl BindingContext for Context {
    fn identity(&self) -> &str {
        &self.identity
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        self.decisions
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
    fn call(&self, site: BindingSite) -> BindingDecision<CallPermit> {
        self.calls
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
    fn object_shape(&self, key: &ObjectViewKey) -> BindingDecision<ObjectShape> {
        self.shapes
            .get(key)
            .cloned()
            .map(BindingDecision::Bound)
            .unwrap_or(BindingDecision::Unavailable {
                proof: "missing".into(),
                reason: "shape_missing".into(),
            })
    }
    fn call_observation(&self, site: BindingSite) -> BindingDecision<CallObservation> {
        self.observations
            .get(&site)
            .cloned()
            .map(BindingDecision::Bound)
            .unwrap_or(BindingDecision::NotManaged)
    }
}
fn context(identity: &str) -> Context {
    Context {
        identity: identity.into(),
        decisions: HashMap::new(),
        calls: HashMap::new(),
        observations: HashMap::new(),
        shapes: HashMap::new(),
    }
}
fn site(analysis: &EmmyLuaAnalysis, file_id: FileId, text: &str) -> BindingSite {
    let model = analysis.semantic_model(file_id);
    let expr = model
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .filter_map(LuaExpr::cast)
        .find(|expr| expr.syntax().text().to_string() == text)
        .unwrap();
    BindingSite {
        file_id,
        syntax: expr.get_syntax_id(),
    }
}
fn load() -> (OwnedQueryTest, Vec<FileId>, Arc<str>) {
    let mut analysis = EmmyLuaAnalysis::new();
    let root = std::env::current_dir().unwrap().join("binding-test-input");
    analysis.add_main_workspace(root.clone());
    let text: Arc<str> = Arc::from("local x = 42\nreturn x, x + 1\n");
    let analysis = OwnedQueryTest::ingest(
        analysis,
        vec![
            (root.join("a.lua"), Some(text.clone())),
            (root.join("b.lua"), Some(Arc::from("return true\n"))),
        ],
    )
    .unwrap();
    let ids = analysis.file_ids().to_vec();
    (analysis, ids, text)
}
#[test]
fn public_ingestion_preserves_bytes_and_facts_with_distinct_text_allocation() {
    let (analysis, ids, text) = load();
    assert_eq!(analysis.db.get_file_text(ids[0]), Some(text.as_ref()));
    assert_ne!(
        analysis.db.get_file_text(ids[0]).unwrap().as_ptr(),
        text.as_ptr()
    );
    let mut other = EmmyLuaAnalysis::new();
    let path = std::env::current_dir()
        .unwrap()
        .join("binding-test-input/a.lua");
    let other_ids = other
        .owned_ingest_strings(vec![(path, Some(text.to_string()))], || Ok::<_, ()>(()))
        .unwrap();
    assert_eq!(
        analysis.semantic_model(ids[0]).file_facts(),
        other.semantic_model(other_ids[0]).file_facts()
    );
}
#[test]
fn public_ingestion_propagates_each_owned_checkpoint_and_removes() {
    let path = std::env::current_dir()
        .unwrap()
        .join("binding-test-input/c.lua");
    let mut checks = 0;
    let mut analysis = EmmyLuaAnalysis::new();
    let ids = analysis
        .owned_ingest_shared(vec![(path.clone(), Some(Arc::from("return 1")))], || {
            checks += 1;
            Ok::<_, &str>(())
        })
        .unwrap();
    for stop in 1..=checks {
        let mut interrupted = EmmyLuaAnalysis::new();
        let mut count = 0;
        assert!(matches!(
            interrupted.owned_ingest_shared(
                vec![(path.clone(), Some(Arc::from("return 1")))],
                || {
                    count += 1;
                    if count == stop {
                        Err("cancel-original")
                    } else {
                        Ok(())
                    }
                }
            ),
            Err(lycheedev_emmy_adapter::IngestionFailure::Interrupted(
                "cancel-original"
            ))
        ));
        assert_eq!(count, stop);
    }
    analysis
        .owned_ingest_shared(vec![(path, None)], || Ok::<_, ()>(()))
        .unwrap();
    assert!(analysis.db.get_file_text(ids[0]).is_none());
}
#[test]
fn managed_unavailable_and_absent_do_not_fall_back() {
    let (analysis, ids, _) = load();
    let use_site = site(&analysis, ids[0], "x");
    for (decision, expected) in [
        (
            BindingDecision::Unavailable {
                proof: "p".into(),
                reason: "blocked".into(),
            },
            LuaType::Unknown,
        ),
        (
            BindingDecision::KnownAbsent { proof: "p".into() },
            LuaType::Nil,
        ),
    ] {
        let mut provider = context("phase");
        provider.decisions.insert(use_site, decision);
        let session = owned_query::session(Arc::new(provider));
        owned_query::model!(model, analysis, ids[0], session.clone());
        assert_eq!(model.type_of_expr(use_site.syntax), expected);
        assert!(!session.coverage().is_empty());
    }
}
#[test]
fn model_for_inherits_context_and_context_caches_are_isolated() {
    let (analysis, ids, _) = load();
    let use_site = site(&analysis, ids[0], "x");
    let origin = site(&analysis, ids[1], "true");
    let mut provider = context("selected");
    provider.decisions.insert(
        use_site,
        BindingDecision::Bound(BoundValue {
            identity: "boolean-value".into(),
            proof: "p".into(),
            value: SelectedValue::Expression(origin),
        }),
    );
    let session = owned_query::session(Arc::new(provider));
    owned_query::model!(selected, analysis, ids[1], session.clone());
    owned_query::foreign_model!(foreign, selected, ids[0]);
    owned_query::model!(
        unmanaged,
        analysis,
        ids[0],
        owned_query::session(Arc::new(context("unmanaged")))
    );
    assert_eq!(foreign.binding_context_identity(), Some("selected"));
    assert!(matches!(
        foreign.binding_decision(use_site.syntax),
        BindingDecision::Bound(_)
    ));
    assert!(matches!(
        unmanaged.binding_decision(use_site.syntax),
        BindingDecision::NotManaged
    ));
    assert_eq!(
        foreign.type_of_expr(use_site.syntax),
        LuaType::BooleanConst(true)
    );
    assert_eq!(
        unmanaged.type_of_expr(use_site.syntax),
        LuaType::IntegerConst(42)
    );
    let raw_session = owned_query::session(Arc::new(context("raw")));
    owned_query::model!(raw, analysis, ids[0], raw_session);
    assert_eq!(raw.type_of_expr(use_site.syntax), LuaType::IntegerConst(42));
    assert_eq!(
        foreign.type_of_expr(use_site.syntax),
        LuaType::BooleanConst(true)
    );
}
#[test]
fn selected_expression_cycles_are_bounded_and_visible() {
    let (analysis, ids, _) = load();
    let use_site = site(&analysis, ids[0], "x");
    let mut provider = context("cycle");
    provider.decisions.insert(
        use_site,
        BindingDecision::Bound(BoundValue {
            identity: "cycle".into(),
            proof: "p".into(),
            value: SelectedValue::Expression(use_site),
        }),
    );
    let session = owned_query::session(Arc::new(provider));
    owned_query::model!(model, analysis, ids[0], session.clone());
    assert_eq!(model.type_of_expr(use_site.syntax), LuaType::Unknown);
    assert!(
        session
            .coverage()
            .iter()
            .any(|row| row.reason.as_deref() == Some("binding_projection_cycle"))
    );
}
#[test]
fn environment_records_keep_actual_slots_and_raw_evidence() {
    let value = serde_json::json!({"kind":"function","name":"Host","parameters":[{"index":1,"name":"unit","type":"UnitToken","nilable":false,"line":4,"raw":{"Name":"unit"}}],"returns":[{"index":1,"name":"ok","type":"bool","nilable":false,"line":5}],"raw":{"documentation":"完整"}});
    let record: EnvironmentRecord = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(record.parameters[0].type_name, "UnitToken");
    assert_eq!(record.returns[0].type_name, "bool");
    assert_eq!(record.raw["documentation"], "完整");
    assert_eq!(
        serde_json::to_value(&record).unwrap()["parameters"][0]["raw"],
        value["parameters"][0]["raw"]
    );
}

#[test]
fn managed_children_are_used_inside_vm_binary_and_flow_queries() {
    let (analysis, ids, _) = load();
    let model = analysis.semantic_model(ids[0]);
    let mut provider = context("all-reads");
    for expr in model
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .filter_map(LuaExpr::cast)
    {
        if expr.syntax().text().to_string() == "x" {
            provider.decisions.insert(
                BindingSite {
                    file_id: ids[0],
                    syntax: expr.get_syntax_id(),
                },
                BindingDecision::Unavailable {
                    proof: "p".into(),
                    reason: "bounded_graph_missing".into(),
                },
            );
        }
    }
    let binary = site(&analysis, ids[0], "x + 1");
    let use_site = site(&analysis, ids[0], "x");
    let session = owned_query::session(Arc::new(provider));
    owned_query::model!(managed, analysis, ids[0], session.clone());
    assert_eq!(managed.type_of_expr(binary.syntax), LuaType::Unknown);
    assert_eq!(
        managed.type_of_expr_at(use_site.syntax, use_site.syntax.get_range().start()),
        LuaType::Unknown
    );
    assert!(
        session
            .coverage()
            .iter()
            .any(|row| row.reason.as_deref() == Some("bounded_graph_missing"))
    );
}

fn object_setup(
    complete: bool,
) -> (
    OwnedQueryTest,
    Vec<FileId>,
    BindingSite,
    Arc<OwnedBindingSession>,
) {
    let (mut analysis, mut ids, _) = load();
    let root = std::env::current_dir().unwrap().join("binding-test-input");
    ids.extend(
        analysis
            .owned_ingest_strings(
                vec![(
                    root.join("object.lua"),
                    Some("local o = {raw_only=123}\nreturn o\n".into()),
                )],
                || Ok::<_, ()>(()),
            )
            .unwrap(),
    );
    let use_site = site(&analysis, ids[2], "o");
    let origin = site(&analysis, ids[2], "{raw_only=123}");
    let boolean = site(&analysis, ids[1], "true");
    let mut provider = context("objects");
    let key = ObjectViewKey {
        context: "objects".into(),
        observation: 0,
        object_id: 0,
    };
    let object = ObjectRef {
        identity: "object:0".into(),
        epoch: "epoch:0".into(),
        origin: ObjectOrigin::Lua(origin),
        view: key.clone(),
    };
    provider.decisions.insert(
        use_site,
        BindingDecision::Bound(BoundValue {
            identity: "v-object".into(),
            proof: "use-proof".into(),
            value: SelectedValue::Object(object),
        }),
    );
    let write = FieldWriteOrigin::Lua(SourceSpan {
        file_id: ids[2],
        range: owned_query::byte_range(origin.syntax.get_range()),
    });
    provider.shapes.insert(
        key.clone(),
        ObjectShape {
            key,
            proof: "shape-proof".into(),
            enumeration: if complete {
                ShapeEnumeration::Complete
            } else {
                ShapeEnumeration::Unavailable("dynamic_write".into())
            },
            fields: vec![
                ObjectField {
                    key: "visible".into(),
                    presence: FieldPresence::Present,
                    value: BindingDecision::Bound(BoundValue {
                        identity: "boolean".into(),
                        proof: "field-proof".into(),
                        value: SelectedValue::Expression(boolean),
                    }),
                    destination_write: write.clone(),
                    source_write: None,
                    copy_origin: None,
                },
                ObjectField {
                    key: "gone".into(),
                    presence: FieldPresence::Absent,
                    value: BindingDecision::KnownAbsent {
                        proof: "absent-proof".into(),
                    },
                    destination_write: write,
                    source_write: None,
                    copy_origin: None,
                },
            ],
        },
    );
    (
        analysis,
        ids,
        use_site,
        owned_query::session(Arc::new(provider)),
    )
}
#[test]
fn frozen_object_member_apis_use_only_frozen_fields_and_keep_context() {
    let (analysis, ids, use_site, session) = object_setup(true);
    owned_query::model!(model, analysis, ids[2], session.clone());
    let ty = model.value_of_expr(use_site.syntax);
    let key = emmylua_code_analysis::LuaMemberKey::Name("visible".into());
    let members = model.member_infos(&ty);
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].file_id, Some(ids[2]));
    assert_eq!(
        model.member_type(&ty, &key),
        Some(LuaType::BooleanConst(true))
    );
    assert_eq!(model.member_infos_with_key(&ty, &key).len(), 1);
    assert_eq!(model.member_infos_with_key_all(&ty, &key).len(), 1);
    assert!(
        model
            .member_info(
                &ty,
                &emmylua_code_analysis::LuaMemberKey::Name("raw_only".into())
            )
            .is_none()
    );
    assert!(
        model
            .member_info(
                &ty,
                &emmylua_code_analysis::LuaMemberKey::Name("gone".into())
            )
            .is_none()
    );
    owned_query::foreign_model!(foreign_file, model, ids[1]);
    assert_eq!(
        foreign_file.member_type(&ty, &key),
        Some(LuaType::BooleanConst(true))
    );
    let other_session = owned_query::session(Arc::new(context("different-context")));
    owned_query::model!(other, analysis, ids[2], other_session);
    assert!(other.member_infos(&ty).is_empty());
    assert!(other.member_infos_with_key_all(&ty, &key).is_empty());
    assert!(!other.type_check(&ty, &ty));
    let union = OwnedValue::raw_union(vec![ty.clone(), OwnedValue::plain(LuaType::Nil)]);
    assert!(other.member_infos(&union).is_empty());
    assert!(!other.type_check(&union, &union));
    assert!(model.type_check(&ty, &ty));
}
#[test]
fn incomplete_object_shape_never_claims_structural_compatibility() {
    let (analysis, ids, use_site, session) = object_setup(false);
    owned_query::model!(model, analysis, ids[2], session.clone());
    let ty = model.value_of_expr(use_site.syntax);
    assert!(!model.type_check(&ty, &ty));
    assert!(!model.type_check(&ty, &OwnedValue::plain(LuaType::Any)));
    assert!(
        session
            .coverage()
            .iter()
            .any(|row| row.operation == "structure_check" && row.state == "unavailable")
    );
}

fn call_setup(
    wrong_identity: bool,
    denied: bool,
) -> (
    OwnedQueryTest,
    Vec<FileId>,
    BindingSite,
    Arc<OwnedBindingSession>,
) {
    let (mut analysis, mut ids, _) = load();
    let root = std::env::current_dir().unwrap().join("binding-test-input");
    ids.extend(
        analysis
            .owned_ingest_strings(
                vec![(
                    root.join("calls.lua"),
                    Some("local function F() return 42, 'raw' end\nreturn F()\n".into()),
                )],
                || Ok::<_, ()>(()),
            )
            .unwrap(),
    );
    let call = site(&analysis, ids[2], "F()");
    let callee = site(&analysis, ids[2], "F");
    let closure = analysis
        .semantic_model(ids[2])
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .find_map(emmylua_parser::LuaClosureExpr::cast)
        .unwrap();
    let function = FunctionRef {
        file_id: ids[2],
        closure: closure.get_syntax_id(),
        declaration: None,
    };
    let mut provider = context("call-context");
    provider.decisions.insert(
        callee,
        BindingDecision::Bound(BoundValue {
            identity: "selected-function".into(),
            proof: "callee-proof".into(),
            value: SelectedValue::Function(function.clone()),
        }),
    );
    provider.calls.insert(
        call,
        if denied {
            BindingDecision::Unavailable {
                proof: "call-proof".into(),
                reason: "effect_unavailable".into(),
            }
        } else {
            BindingDecision::Bound(CallPermit {
                callee,
                proof: "call-proof".into(),
                capability_profile: "typed-call-v1".into(),
            })
        },
    );
    provider.observations.insert(
        call,
        CallObservation {
            callee: ObservedCallee::Source(function),
            selected_callee_identity: if wrong_identity {
                "wrong".into()
            } else {
                "selected-function".into()
            },
            closure_identity: "closure:0".into(),
            invocation_identity: "frame:0".into(),
            proof: "observation-proof".into(),
            returns: vec![
                BindingDecision::Bound(BoundValue {
                    identity: "boolean".into(),
                    proof: "slot-proof".into(),
                    value: SelectedValue::Expression(site(&analysis, ids[1], "true")),
                }),
                BindingDecision::KnownAbsent {
                    proof: "nil-proof".into(),
                },
            ],
        },
    );
    (
        analysis,
        ids,
        call,
        owned_query::session(Arc::new(provider)),
    )
}
#[test]
fn observed_return_slots_replace_raw_function_returns() {
    let (analysis, ids, call, session) = call_setup(false, false);
    owned_query::model!(model, analysis, ids[2], session);
    let LuaType::Variadic(values) = model.type_of_expr(call.syntax) else {
        panic!("expected observed returns")
    };
    let emmylua_code_analysis::VariadicType::Multi(slots) = values.as_ref() else {
        panic!("expected exact slots")
    };
    assert_eq!(slots, &[LuaType::BooleanConst(true), LuaType::Nil]);
}
#[test]
fn mismatched_call_observation_and_denied_call_never_fall_back() {
    for (wrong, denied) in [(true, false), (false, true)] {
        let (analysis, ids, call, session) = call_setup(wrong, denied);
        owned_query::model!(model, analysis, ids[2], session.clone());
        assert_eq!(model.type_of_expr(call.syntax), LuaType::Unknown);
        assert!(
            session
                .coverage()
                .iter()
                .any(|row| row.state == "unavailable")
        );
    }
}

#[test]
fn denied_calls_never_report_completed_native_checks() {
    let (analysis, ids, call, session) = call_setup(false, true);
    let run = analysis
        .diagnose(
            ids[2],
            Arc::new(emmylua_code_analysis::CheckConfig::default()),
            &session,
        )
        .unwrap();
    assert!(run.complete, "owned diagnostic registry did not complete");
    let _diagnostics = run.diagnostics;
    let rows = session.coverage();
    for operation in ["argument_check", "argument_count_check", "callable_check"] {
        assert!(
            rows.iter().any(|row| row.site == call
                && row.operation == operation
                && row.state == "unavailable"),
            "missing denial coverage for {operation}"
        );
        assert!(!rows.iter().any(|row| row.site == call
            && row.operation == operation
            && row.state == "native_checked"));
    }
}

#[test]
fn contextual_signature_body_and_initializer_do_not_use_raw_shells() {
    let (mut analysis, ids, _) = load();
    let root = std::env::current_dir().unwrap().join("binding-test-input");
    let file = analysis
        .owned_ingest_strings(
            vec![(
                root.join("signatures.lua"),
                Some("local x = 42\nlocal function F() return x end\nreturn x, F\n".into()),
            )],
            || Ok::<_, ()>(()),
        )
        .unwrap()[0];
    let raw_model = analysis.semantic_model(file);
    let closure = raw_model
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .find_map(emmylua_parser::LuaClosureExpr::cast)
        .unwrap();
    let mut provider = context("signature-context");
    let initializer = site(&analysis, file, "42");
    provider.decisions.insert(
        initializer,
        BindingDecision::Unavailable {
            proof: "blocked-initializer".into(),
            reason: "initializer_unavailable".into(),
        },
    );
    for expr in raw_model
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .filter_map(LuaExpr::cast)
    {
        if expr.syntax().text().to_string() == "x" {
            provider.decisions.insert(
                BindingSite {
                    file_id: file,
                    syntax: expr.get_syntax_id(),
                },
                BindingDecision::Bound(BoundValue {
                    identity: "boolean".into(),
                    proof: "selected".into(),
                    value: SelectedValue::Expression(site(&analysis, ids[1], "true")),
                }),
            );
        }
    }
    let session = owned_query::session(Arc::new(provider));
    owned_query::model!(model, analysis, file, session);
    assert_eq!(
        model.return_type(closure.get_syntax_id()),
        Some(LuaType::BooleanConst(true))
    );
    let signature = model.type_of_signature(closure.get_syntax_id()).unwrap();
    assert_eq!(signature.get_ret(), &LuaType::BooleanConst(true));
    assert_eq!(model.type_of_expr(initializer.syntax), LuaType::Unknown);
    let read = site(&analysis, file, "x");
    let declaration = model.resolve_name(read.syntax.get_range().start()).unwrap();
    assert_eq!(model.type_of_decl(&declaration), Some(LuaType::Unknown));
}

#[test]
fn selected_nil_callee_is_diagnosed_instead_of_raw_nil_tolerance() {
    let (mut analysis, _, _) = load();
    let root = std::env::current_dir().unwrap().join("binding-test-input");
    let ids = analysis
        .owned_ingest_strings(
            vec![
                (
                    root.join("nil-callee.lua"),
                    Some("local function F() return 42 end\nF()\n".into()),
                ),
                (root.join("nil-origin.lua"), Some("return nil\n".into())),
            ],
            || Ok::<_, ()>(()),
        )
        .unwrap();
    let call = site(&analysis, ids[0], "F()");
    let callee = site(&analysis, ids[0], "F");
    let mut provider = context("nil-callee");
    provider.decisions.insert(
        callee,
        BindingDecision::Bound(BoundValue {
            identity: "selected-nil".into(),
            proof: "nil-proof".into(),
            value: SelectedValue::Expression(site(&analysis, ids[1], "nil")),
        }),
    );
    provider.calls.insert(
        call,
        BindingDecision::Bound(CallPermit {
            callee,
            proof: "permit".into(),
            capability_profile: "typed-call-v1".into(),
        }),
    );
    let session = owned_query::session(Arc::new(provider));
    let run = analysis
        .diagnose(
            ids[0],
            Arc::new(emmylua_code_analysis::CheckConfig::default()),
            &session,
        )
        .unwrap();
    assert!(run.complete, "owned diagnostic registry did not complete");
    let diagnostics = run.diagnostics;
    assert!(diagnostics.iter().any(
        |diagnostic| diagnostic.code == emmylua_code_analysis::DiagnosticCode::CallNonCallable
    ));
}

#[test]
fn frozen_object_view_rejects_contradictory_identity_epoch_and_origin() {
    for difference in ["identity", "epoch", "origin"] {
        let (mut analysis, _, _) = load();
        let file = analysis
            .owned_ingest_strings(
                vec![(
                    std::env::current_dir()
                        .unwrap()
                        .join("binding-test-input/versions.lua"),
                    Some("return a, b\n".into()),
                )],
                || Ok::<_, ()>(()),
            )
            .unwrap()[0];
        let first = site(&analysis, file, "a");
        let second = site(&analysis, file, "b");
        let key = ObjectViewKey {
            context: "versions".into(),
            observation: 0,
            object_id: 0,
        };
        let object = ObjectRef {
            identity: "object".into(),
            epoch: "epoch".into(),
            origin: ObjectOrigin::Lua(first),
            view: key.clone(),
        };
        let mut contradictory = object.clone();
        match difference {
            "identity" => contradictory.identity = "other-object".into(),
            "epoch" => contradictory.epoch = "other-epoch".into(),
            _ => contradictory.origin = ObjectOrigin::Lua(second),
        }
        let mut provider = context("versions");
        for (site, object) in [(first, object), (second, contradictory)] {
            provider.decisions.insert(
                site,
                BindingDecision::Bound(BoundValue {
                    identity: "value".into(),
                    proof: "proof".into(),
                    value: SelectedValue::Object(object),
                }),
            );
        }
        provider.shapes.insert(
            key.clone(),
            ObjectShape {
                key,
                proof: "shape".into(),
                enumeration: ShapeEnumeration::Complete,
                fields: vec![],
            },
        );
        let session = owned_query::session(Arc::new(provider));
        owned_query::model!(model, analysis, file, session.clone());
        assert!(matches!(
            model.type_of_expr(first.syntax),
            LuaType::Object(_)
        ));
        assert_eq!(
            model.type_of_expr(second.syntax),
            LuaType::Unknown,
            "{difference}"
        );
        assert!(
            session
                .coverage()
                .iter()
                .any(|row| row.reason.as_deref() == Some("object_frozen_identity_mismatch"))
        );
    }
}

#[test]
fn same_identity_string_does_not_authorize_foreign_session_object_types() {
    let (analysis, ids, use_site, session) = object_setup(true);
    owned_query::model!(model, analysis, ids[2], session);
    let ty = model.value_of_expr(use_site.syntax);
    owned_query::model!(
        empty,
        analysis,
        ids[2],
        owned_query::session(Arc::new(context("objects")))
    );
    assert!(!empty.type_check(&ty, &ty));
    assert!(empty.member_infos(&ty).is_empty());
    let (other_analysis, other_ids, other_use_site, other_session) = object_setup(true);
    owned_query::model!(other, other_analysis, other_ids[2], other_session);
    let own_ty = other.value_of_expr(other_use_site.syntax);
    assert!(other.type_check(&own_ty, &own_ty));
    assert!(!other.type_check(&ty, &ty));
    assert!(other.member_infos(&ty).is_empty());
    assert_ne!(ty, own_ty);
}

#[test]
fn unavailable_tail_call_does_not_invent_a_fixed_signature_return_arity() {
    let (mut analysis, _, _) = load();
    let file = analysis.owned_ingest_strings(vec![(std::env::current_dir().unwrap().join("binding-test-input/unknown-tail.lua"), Some("local function F() return 42 end\nlocal function G() return 1, F() end\nreturn G\n".into()))], || Ok::<_, ()>(())).unwrap()[0];
    let call = site(&analysis, file, "F()");
    let mut provider = context("unknown-tail");
    provider.calls.insert(
        call,
        BindingDecision::Unavailable {
            proof: "blocked".into(),
            reason: "return_arity_unavailable".into(),
        },
    );
    owned_query::model!(
        model,
        analysis,
        file,
        owned_query::session(Arc::new(provider))
    );
    let closure = model
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .filter_map(emmylua_parser::LuaClosureExpr::cast)
        .find(|closure| {
            closure
                .syntax()
                .text()
                .to_string()
                .contains("return 1, F()")
        })
        .unwrap();
    let returned = model.return_type(closure.get_syntax_id()).unwrap();
    let LuaType::Variadic(slots) = returned else {
        panic!("expected return pack")
    };
    assert_eq!(slots.get_max_len(), None);
    assert_eq!(slots.get_type(0), Some(&LuaType::IntegerConst(1)));
    assert_eq!(slots.get_type(20), Some(&LuaType::Unknown));
}

#[test]
fn contextual_unknown_signature_never_retries_raw_body() {
    let (mut analysis, _, _) = load();
    let file = analysis
        .owned_ingest_strings(
            vec![(
                std::env::current_dir()
                    .unwrap()
                    .join("binding-test-input/unknown-signature.lua"),
                Some("local x = 42\nlocal function F() return x end\nreturn F\n".into()),
            )],
            || Ok::<_, ()>(()),
        )
        .unwrap()[0];
    let unknown = site(&analysis, file, "x");
    let mut provider = context("unknown-signature");
    provider.decisions.insert(
        unknown,
        BindingDecision::Unavailable {
            proof: "blocked".into(),
            reason: "read_unavailable".into(),
        },
    );
    owned_query::model!(
        model,
        analysis,
        file,
        owned_query::session(Arc::new(provider))
    );
    let closure = model
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .find_map(emmylua_parser::LuaClosureExpr::cast)
        .unwrap();
    assert_eq!(
        model.return_type(closure.get_syntax_id()),
        Some(LuaType::Unknown)
    );
    assert_eq!(
        model
            .type_of_signature(closure.get_syntax_id())
            .unwrap()
            .get_ret(),
        &LuaType::Unknown
    );
}

#[test]
fn resolve_member_never_recovers_a_raw_field_absent_from_the_frozen_shape() {
    let (mut analysis, _, _) = load();
    let file = analysis
        .owned_ingest_strings(
            vec![(
                std::env::current_dir()
                    .unwrap()
                    .join("binding-test-input/member-resolution.lua"),
                Some("local object = { stale = 42 }\nreturn object.stale\n".into()),
            )],
            || Ok::<_, ()>(()),
        )
        .unwrap()[0];
    owned_query::model!(
        raw,
        analysis,
        file,
        owned_query::session(Arc::new(context("unmanaged")))
    );
    let index = raw
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .find_map(emmylua_parser::LuaIndexExpr::cast)
        .unwrap();
    assert!(raw.resolve_member(&index).is_some());
    let prefix = site(&analysis, file, "object");
    let origin = site(&analysis, file, "{ stale = 42 }");
    let key = ObjectViewKey {
        context: "member-resolution".into(),
        observation: 0,
        object_id: 0,
    };
    let mut provider = context("member-resolution");
    provider.decisions.insert(
        prefix,
        BindingDecision::Bound(BoundValue {
            identity: "object".into(),
            proof: "selected".into(),
            value: SelectedValue::Object(ObjectRef {
                identity: "object".into(),
                epoch: "epoch".into(),
                origin: ObjectOrigin::Lua(origin),
                view: key.clone(),
            }),
        }),
    );
    provider.shapes.insert(
        key.clone(),
        ObjectShape {
            key,
            proof: "empty".into(),
            enumeration: ShapeEnumeration::Complete,
            fields: vec![],
        },
    );
    owned_query::model!(
        model,
        analysis,
        file,
        owned_query::session(Arc::new(provider))
    );
    assert!(model.resolve_member(&index).is_none());
}

#[test]
fn resolved_frozen_member_keeps_selected_type_and_does_not_alias_dynamic_keys() {
    let (mut analysis, ids, _) = load();
    let file = analysis.owned_ingest_strings(vec![(std::env::current_dir().unwrap().join("binding-test-input/frozen-member.lua"), Some("local object = { stale = 42 }\nreturn object.stale, object[1], object[key], object[\"[1]\"]\n".into()))], || Ok::<_, ()>(())).unwrap()[0];
    let raw = analysis.semantic_model(file);
    let key = ObjectViewKey {
        context: "frozen-member".into(),
        observation: 0,
        object_id: 0,
    };
    let object = ObjectRef {
        identity: "object".into(),
        epoch: "epoch".into(),
        origin: ObjectOrigin::Lua(site(&analysis, file, "{ stale = 42 }")),
        view: key.clone(),
    };
    let mut provider = context("frozen-member");
    for expr in raw
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .filter_map(LuaExpr::cast)
    {
        if expr.syntax().text().to_string() == "object" {
            provider.decisions.insert(
                BindingSite {
                    file_id: file,
                    syntax: expr.get_syntax_id(),
                },
                BindingDecision::Bound(BoundValue {
                    identity: "object".into(),
                    proof: "selected".into(),
                    value: SelectedValue::Object(object.clone()),
                }),
            );
        }
    }
    let boolean = site(&analysis, ids[1], "true");
    let fields = ["stale", "[1]", "[key]"]
        .into_iter()
        .map(|key| ObjectField {
            key: key.into(),
            presence: FieldPresence::Present,
            value: BindingDecision::Bound(BoundValue {
                identity: "boolean".into(),
                proof: "field".into(),
                value: SelectedValue::Expression(boolean),
            }),
            destination_write: FieldWriteOrigin::Xml(ExternalSourceSpan {
                path: "fixture.xml".into(),
                content_sha256: "b".repeat(64),
                range: owned_query::byte_range(boolean.syntax.get_range()),
            }),
            source_write: None,
            copy_origin: None,
        })
        .collect();
    provider.shapes.insert(
        key.clone(),
        ObjectShape {
            key,
            proof: "shape".into(),
            enumeration: ShapeEnumeration::Complete,
            fields,
        },
    );
    owned_query::model!(
        model,
        analysis,
        file,
        owned_query::session(Arc::new(provider))
    );
    owned_query::model!(
        foreign,
        analysis,
        file,
        owned_query::session(Arc::new(context("frozen-member")))
    );
    for index in raw
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .filter_map(emmylua_parser::LuaIndexExpr::cast)
    {
        let text = index.syntax().text().to_string();
        if text == "object.stale" || text == "object[\"[1]\"]" {
            let member = model.resolve_member(&index).unwrap();
            assert_eq!(
                member.member_type(&model),
                Some(LuaType::BooleanConst(true)),
                "{text}"
            );
            assert!(member.member_id.is_none());
            assert_eq!(member.member_type(&foreign), None);
        } else {
            assert!(model.resolve_member(&index).is_none(), "{text}");
        }
    }
}

// The original five authenticated fixtures stay byte-for-byte fixed. This
// exercises the real public intake, owned VM and owned diagnostic registry;
// no expected value is obtained from another invocation of the candidate.
fn fixed_return_quality_fixture() -> (
    OwnedQueryTest,
    Vec<FileId>,
    BindingSite,
    Context,
    Arc<emmylua_code_analysis::CheckConfig>,
) {
    use emmylua_code_analysis::{CheckConfig, DiagnosticCode, Emmyrc, EmmyrcLuaVersion};
    const INPUTS: [(&str, &str); 5] = [
        (
            "profile.lua",
            include_str!("../../../tests/rust-source-engine/wow/profile.lua"),
        ),
        (
            "generic-defs.lua",
            include_str!("../../../tests/rust-source-engine/wow/native/generic-defs.lua"),
        ),
        (
            "generic-calls.lua",
            include_str!("../../../tests/rust-source-engine/wow/native/generic-calls.lua"),
        ),
        (
            "generic-replace.lua",
            include_str!("../../../tests/rust-source-engine/wow/native/generic-replace.lua"),
        ),
        (
            "coverage-unknown-argument.lua",
            include_str!(
                "../../../tests/rust-source-engine/wow/native/coverage-unknown-argument.lua"
            ),
        ),
    ];
    let enabled = [
        DiagnosticCode::ParamTypeMismatch,
        DiagnosticCode::ReturnTypeMismatch,
        DiagnosticCode::MissingParameter,
        DiagnosticCode::RedundantParameter,
        DiagnosticCode::CallNonCallable,
        DiagnosticCode::MissingReturnValue,
        DiagnosticCode::RedundantReturnValue,
        DiagnosticCode::MissingReturn,
    ];
    let mut config = Emmyrc::default();
    config.runtime.version = EmmyrcLuaVersion::Lua51;
    config.diagnostics.disable = DiagnosticCode::all()
        .into_iter()
        .filter(|code| !enabled.contains(code))
        .collect();
    config.diagnostics.enables = enabled.to_vec();
    let checks = Arc::new(CheckConfig::new(&config));
    let mut analysis = EmmyLuaAnalysis::new();
    analysis.update_config(Arc::new(config));
    let root = std::env::current_dir()
        .unwrap()
        .join("fixed-return-quality-input");
    analysis.add_main_workspace(root.clone());
    let fixture = OwnedQueryTest::ingest(
        analysis,
        INPUTS
            .into_iter()
            .map(|(name, text)| (root.join(name), Some(Arc::from(text))))
            .collect(),
    )
    .unwrap();
    let ids = fixture.file_ids().to_vec();
    assert_eq!(ids.len(), 5);
    assert_eq!(fixture.db.get_file_text(ids[4]).unwrap().len(), 127);
    let call = site(&fixture, ids[4], "Replacement(object[key])");
    let callee = site(&fixture, ids[4], "Replacement");
    let argument = site(&fixture, ids[4], "object[key]");
    let closure = fixture
        .semantic_model(ids[1])
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .filter_map(emmylua_parser::LuaClosureExpr::cast)
        .nth(1)
        .unwrap();
    let mut provider = context("fixed-return-unknown-argument");
    provider.decisions.insert(
        callee,
        BindingDecision::Bound(BoundValue {
            identity: "Replacement@fixed-definitions".into(),
            proof: "selected-callee-proof".into(),
            value: SelectedValue::Function(FunctionRef {
                file_id: ids[1],
                closure: closure.get_syntax_id(),
                declaration: None,
            }),
        }),
    );
    provider.decisions.insert(
        argument,
        BindingDecision::Unavailable {
            proof: "unknown-argument-proof".into(),
            reason: "dynamic_key_unavailable".into(),
        },
    );
    provider.calls.insert(
        call,
        BindingDecision::Bound(CallPermit {
            callee,
            proof: "same-result-proof".into(),
            capability_profile: "typed-call-v1".into(),
        }),
    );
    (fixture, ids, call, provider, checks)
}

fn quality_receipt(
    site: BindingSite,
    operation: &'static str,
    state: &'static str,
    reason: Option<&str>,
) -> BindingCoverage {
    BindingCoverage {
        site,
        operation,
        state,
        proof: "same-result-proof".into(),
        reason: reason.map(str::to_owned),
        parameter: None,
        parameter_index: None,
        argument: None,
    }
}

#[test]
fn known_signature_result_and_actual_return_check_survive_unknown_argument() {
    use emmylua_code_analysis::DiagnosticCode;
    let (fixture, ids, call, provider, checks) = fixed_return_quality_fixture();
    let session = owned_query::session(Arc::new(provider));
    owned_query::model!(model, fixture, ids[4], session.clone());
    assert_eq!(model.type_of_expr(call.syntax), LuaType::String);
    let before_check: Vec<_> = session
        .coverage()
        .into_iter()
        .filter(|row| {
            row.site == call
                && matches!(
                    row.operation,
                    "call_result" | "argument_check" | "return_check"
                )
        })
        .collect();
    assert_eq!(
        before_check,
        [quality_receipt(
            call,
            "call_result",
            "native_projected",
            None
        )]
    );
    let run = fixture.diagnose(ids[4], checks, &session).unwrap();
    assert!(run.complete);
    assert!(run.execution.iter().any(|row| matches!(
        row,
        CheckerExecution::Completed(
            "lycheedev_emmy_adapter::return_type_mismatch::ReturnTypeMismatchChecker"
        )
    )));
    let actual: Vec<_> = run
        .diagnostics
        .iter()
        .map(|row| {
            (
                row.code,
                u32::from(row.range.start()),
                u32::from(row.range.end()),
            )
        })
        .collect();
    assert_eq!(actual, [(DiagnosticCode::ReturnTypeMismatch, 98, 122)]);
    let coverage = session.coverage();
    let quality: Vec<_> = coverage
        .into_iter()
        .filter(|row| {
            row.site == call
                && matches!(
                    row.operation,
                    "call_result" | "argument_check" | "return_check"
                )
        })
        .collect();
    assert_eq!(
        quality,
        [
            quality_receipt(call, "call_result", "native_projected", None),
            quality_receipt(
                call,
                "argument_check",
                "unavailable",
                Some("native_check_inputs_or_signature_unavailable")
            ),
            quality_receipt(call, "return_check", "native_checked", None),
        ]
    );
}

#[test]
fn signature_projection_does_not_bypass_denied_or_invalid_selected_calls() {
    for case in [
        "denied",
        "absent",
        "wrong-profile",
        "wrong-callee",
        "unavailable-callee",
    ] {
        let (fixture, ids, call, mut provider, checks) = fixed_return_quality_fixture();
        let BindingDecision::Bound(mut permit) = provider.calls[&call].clone() else {
            unreachable!()
        };
        match case {
            "denied" => {
                provider.calls.insert(
                    call,
                    BindingDecision::Unavailable {
                        proof: "denied-proof".into(),
                        reason: "effect_unavailable".into(),
                    },
                );
            }
            "absent" => {
                provider.calls.insert(
                    call,
                    BindingDecision::KnownAbsent {
                        proof: "absent-proof".into(),
                    },
                );
            }
            "wrong-profile" => {
                permit.capability_profile = "untrusted-profile".into();
                provider.calls.insert(call, BindingDecision::Bound(permit));
            }
            "wrong-callee" => {
                permit.callee = site(&fixture, ids[4], "object[key]");
                provider.calls.insert(call, BindingDecision::Bound(permit));
            }
            "unavailable-callee" => {
                provider.decisions.insert(
                    permit.callee,
                    BindingDecision::Unavailable {
                        proof: "callee-denied-proof".into(),
                        reason: "callee_unavailable".into(),
                    },
                );
            }
            _ => unreachable!(),
        }
        let session = owned_query::session(Arc::new(provider));
        owned_query::model!(model, fixture, ids[4], session.clone());
        assert_eq!(model.type_of_expr(call.syntax), LuaType::Unknown, "{case}");
        let run = fixture.diagnose(ids[4], checks, &session).unwrap();
        assert!(run.complete, "{case}");
        let rows = session.coverage();
        assert!(
            rows.iter().any(|row| row.site == call
                && row.operation == "call_result"
                && row.state == "unavailable"),
            "{case}: {rows:?}"
        );
        assert!(
            !rows.iter().any(|row| row.site == call
                && matches!(row.operation, "call_result" | "return_check")
                && matches!(row.state, "native_projected" | "native_checked")),
            "{case}: {rows:?}"
        );
    }
}

#[test]
fn generic_result_depending_on_unknown_argument_never_claims_known_result_or_return_check() {
    let (fixture, ids, call, mut provider, checks) = fixed_return_quality_fixture();
    let BindingDecision::Bound(permit) = provider.calls[&call].clone() else {
        unreachable!()
    };
    let closure = fixture
        .semantic_model(ids[1])
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .find_map(emmylua_parser::LuaClosureExpr::cast)
        .unwrap();
    let BindingDecision::Bound(bound) = provider.decisions.get_mut(&permit.callee).unwrap() else {
        unreachable!()
    };
    bound.value = SelectedValue::Function(FunctionRef {
        file_id: ids[1],
        closure: closure.get_syntax_id(),
        declaration: None,
    });
    let session = owned_query::session(Arc::new(provider));
    owned_query::model!(model, fixture, ids[4], session.clone());
    assert_eq!(model.type_of_expr(call.syntax), LuaType::Unknown);
    let run = fixture.diagnose(ids[4], checks, &session).unwrap();
    assert!(run.complete);
    let quality: Vec<_> = session
        .coverage()
        .into_iter()
        .filter(|row| {
            row.site == call
                && matches!(
                    row.operation,
                    "call_result" | "argument_check" | "return_check"
                )
        })
        .collect();
    assert_eq!(
        quality,
        [
            quality_receipt(
                call,
                "call_result",
                "unavailable",
                Some("native_call_result_type_unavailable")
            ),
            quality_receipt(
                call,
                "argument_check",
                "unavailable",
                Some("native_check_inputs_or_signature_unavailable")
            ),
            quality_receipt(
                call,
                "return_check",
                "unavailable",
                Some("native_return_check_inputs_unavailable")
            ),
        ]
    );
}

#[test]
fn interrupted_signature_projection_keeps_typed_control_and_never_caches_a_success() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct StopAtSelectedCallee {
        inner: Context,
        callee: BindingSite,
        armed: AtomicBool,
        stopped: Arc<AtomicBool>,
    }
    impl BindingContext for StopAtSelectedCallee {
        fn identity(&self) -> &str {
            self.inner.identity()
        }
        fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
            let result = self.inner.resolve(site);
            if site == self.callee && self.armed.swap(false, Ordering::SeqCst) {
                self.stopped.store(true, Ordering::SeqCst);
            }
            result
        }
        fn call(&self, site: BindingSite) -> BindingDecision<CallPermit> {
            self.inner.call(site)
        }
    }
    let (fixture, ids, call, provider, _) = fixed_return_quality_fixture();
    let BindingDecision::Bound(permit) = provider.calls[&call].clone() else {
        unreachable!()
    };
    let stopped = Arc::new(AtomicBool::new(false));
    let provider = StopAtSelectedCallee {
        inner: provider,
        callee: permit.callee,
        armed: AtomicBool::new(true),
        stopped: stopped.clone(),
    };
    let session = owned_query::session(Arc::new(provider));
    let mut check = || {
        if stopped.load(Ordering::SeqCst) {
            Err("original-deadline")
        } else {
            Ok(())
        }
    };
    let mut query = OwnedQuery::new_for_snapshot(&fixture, &session, ids[4], &mut check).unwrap();
    // The provider is reached inside an active selected guard and an in-progress
    // expression memo. Both must unwind on the original typed cancellation.
    assert!(matches!(
        query.type_of_expr(call.syntax),
        Err(OwnedQueryFailure::Control("original-deadline"))
    ));
    assert!(session.coverage().is_empty());
    stopped.store(false, Ordering::SeqCst);
    assert_eq!(
        query.type_of_expr(call.syntax).unwrap().structural_type(),
        &LuaType::String
    );
    stopped.store(true, Ordering::SeqCst);
    assert!(matches!(
        query.type_of_expr(call.syntax),
        Err(OwnedQueryFailure::Control("original-deadline"))
    ));
}
