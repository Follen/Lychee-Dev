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
        let expected_gate = match case {
            "denied" => Some(("unavailable", "denied-proof", Some("effect_unavailable"))),
            "absent" => Some(("known_absent", "absent-proof", None)),
            "wrong-profile" | "wrong-callee" => Some((
                "unavailable",
                "same-result-proof",
                Some("call_permit_callee_or_profile_mismatch"),
            )),
            "unavailable-callee" => None,
            _ => unreachable!(),
        };
        let gate_row = expected_gate.map(|(state, proof, reason)| BindingCoverage {
            site: call,
            operation: "call",
            state,
            proof: proof.into(),
            reason: reason.map(str::to_owned),
            parameter: None,
            parameter_index: None,
            argument: None,
        });
        let before_check = session.coverage();
        if let Some(gate) = &gate_row {
            // No admitted projection ran. The original gate is the complete
            // explanation for Unknown; no result-completion row is synthesized.
            assert_eq!(before_check, vec![gate.clone()], "{case}");
        } else {
            assert_eq!(
                before_check,
                vec![
                    quality_receipt(
                        call,
                        "call_return",
                        "unavailable",
                        Some("call_return_observation_missing")
                    ),
                    quality_receipt(
                        call,
                        "call_result",
                        "unavailable",
                        Some("native_call_result_type_unavailable")
                    ),
                ],
                "{case}"
            );
        }
        assert_eq!(model.type_of_expr(call.syntax), LuaType::Unknown, "{case}");
        assert_eq!(session.coverage(), before_check, "cached query: {case}");
        let run = fixture.diagnose(ids[4], checks, &session).unwrap();
        assert!(run.complete, "{case}");
        let rows = session.coverage();
        if let Some(gate) = gate_row {
            assert!(rows.contains(&gate), "{case}: {rows:?}");
            assert!(
                !rows
                    .iter()
                    .any(|row| row.site == call && row.operation == "call_result"),
                "a rejected gate cannot complete a result: {case}: {rows:?}"
            );
        } else {
            assert!(
                rows.contains(&quality_receipt(
                    call,
                    "call_result",
                    "unavailable",
                    Some("native_call_result_type_unavailable")
                )),
                "{case}: {rows:?}"
            );
        }
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

fn one_checker_fixture(
    name: &str,
    source: &str,
    code: emmylua_code_analysis::DiagnosticCode,
) -> (
    OwnedQueryTest,
    FileId,
    Arc<emmylua_code_analysis::CheckConfig>,
) {
    use emmylua_code_analysis::{CheckConfig, DiagnosticCode, Emmyrc};
    let mut config = Emmyrc::default();
    config.diagnostics.disable = DiagnosticCode::all()
        .into_iter()
        .filter(|candidate| *candidate != code)
        .collect();
    config.diagnostics.enables = vec![code];
    let checks = Arc::new(CheckConfig::new(&config));
    let mut analysis = EmmyLuaAnalysis::new();
    analysis.update_config(Arc::new(config));
    let root = std::env::current_dir()
        .unwrap()
        .join("owned-consumer-contracts");
    analysis.add_main_workspace(root.clone());
    let fixture =
        OwnedQueryTest::ingest(analysis, vec![(root.join(name), Some(Arc::from(source)))]).unwrap();
    let file = fixture.file_ids()[0];
    (fixture, file, checks)
}

#[test]
fn return_query_and_checker_normalize_method_self_and_generic_alias_like_fixed_upstream() {
    use emmylua_code_analysis::{DiagnosticCode, LuaTypeDeclId};
    use emmylua_parser::LuaClosureExpr;
    const CASES: [(&str, &str, usize); 3] = [
        (
            "self-mismatch.lua",
            "---@class A\nlocal A = {}\n---@class B\nlocal B = {}\n---@param b B\n---@return self\nfunction A:copy(b) return b end\n",
            1,
        ),
        (
            "self-match.lua",
            "---@class A\nlocal A = {}\n---@return self\nfunction A:copy() return self end\n",
            0,
        ),
        (
            "return-alias.lua",
            "---@alias Identity<T> T\n---@return Identity<string>\nlocal function f() return 'ok' end\n",
            0,
        ),
    ];
    for (name, source, expected_count) in CASES {
        let (fixture, file, checks) =
            one_checker_fixture(name, source, DiagnosticCode::ReturnTypeMismatch);
        let raw = fixture.semantic_model(file);
        let closure = raw
            .syntax_tree()
            .unwrap()
            .get_red_root()
            .descendants()
            .find_map(LuaClosureExpr::cast)
            .unwrap();
        let expected_type = if name == "return-alias.lua" {
            LuaType::String
        } else {
            LuaType::Ref(LuaTypeDeclId::global("A"))
        };
        // Independent ordinary upstream is the source of this normal contract.
        // The archived patched session's early-return gap stays in its traces.
        assert_eq!(
            raw.return_type(closure.get_syntax_id()),
            Some(expected_type.clone()),
            "{name}"
        );
        let expected = fixture
            .diagnose_file_with_config(file, checks.clone())
            .unwrap();
        assert_eq!(expected.len(), expected_count, "{name}: {expected:?}");
        let session = owned_query::session(Arc::new(context("return-normalization")));
        owned_query::model!(model, fixture, file, session.clone());
        assert_eq!(
            model.return_type(closure.get_syntax_id()),
            Some(expected_type),
            "{name}"
        );
        let run = fixture.diagnose(file, checks, &session).unwrap();
        assert!(run.complete, "{name}");
        assert_eq!(run.diagnostics.len(), expected.len(), "{name}: {run:?}");
        let index = fixture.db.line_index(file).unwrap();
        for (actual, expected) in run.diagnostics.iter().zip(expected) {
            let start = index.get_line_col(actual.range.start(), source).unwrap();
            let end = index.get_line_col(actual.range.end(), source).unwrap();
            assert_eq!(
                expected.range,
                lsp_types::Range::new(
                    lsp_types::Position::new(start.0 as u32, start.1 as u32),
                    lsp_types::Position::new(end.0 as u32, end.1 as u32),
                )
            );
            assert_eq!(
                expected.code,
                Some(lsp_types::NumberOrString::String(
                    actual.code.get_name().into()
                ))
            );
            assert_eq!(expected.severity, Some(actual.severity));
            assert_eq!(expected.message, actual.message);
            assert_eq!(expected.tags, actual.tags);
            assert_eq!(expected.data, actual.data);
        }
        assert!(session.coverage().is_empty(), "{name}");
    }
}

#[test]
fn normalized_method_return_checks_selected_actual_value_with_its_original_proof() {
    use emmylua_code_analysis::{DiagnosticCode, LuaTypeDeclId};
    const SOURCE: &str = "---@class A\nlocal A = {}\n---@class B\nlocal B = {}\n---@param b B\n---@return self\nfunction A:copy(b) return b end\n";
    for absent in [true, false] {
        let (fixture, file, checks) = one_checker_fixture(
            "selected-method.lua",
            SOURCE,
            DiagnosticCode::ReturnTypeMismatch,
        );
        let returned = site(&fixture, file, "b");
        let origin = site(&fixture, file, "A");
        let mut provider = context("selected-method-return");
        provider.decisions.insert(
            returned,
            if absent {
                BindingDecision::KnownAbsent {
                    proof: "selected-return-proof".into(),
                }
            } else {
                BindingDecision::Bound(BoundValue {
                    identity: "selected-A".into(),
                    proof: "selected-return-proof".into(),
                    value: SelectedValue::Expression(origin),
                })
            },
        );
        let session = owned_query::session(Arc::new(provider));
        owned_query::model!(model, fixture, file, session.clone());
        assert_eq!(
            model.type_of_expr(returned.syntax),
            if absent {
                LuaType::Nil
            } else {
                LuaType::Ref(LuaTypeDeclId::global("A"))
            }
        );
        let run = fixture.diagnose(file, checks, &session).unwrap();
        assert!(run.complete);
        assert_eq!(run.diagnostics.len(), usize::from(absent), "{run:?}");
        if absent {
            let diagnostic = &run.diagnostics[0];
            assert_eq!(diagnostic.code, DiagnosticCode::ReturnTypeMismatch);
            assert_eq!(diagnostic.range, returned.syntax.get_range());
            assert_eq!(diagnostic.message, "expected `A` but found `nil`. ");
            assert_eq!(diagnostic.tags, None);
            assert_eq!(diagnostic.data, None);
        }
        assert_eq!(
            session.coverage(),
            [BindingCoverage {
                site: returned,
                operation: "resolve",
                state: if absent { "known_absent" } else { "bound" },
                proof: "selected-return-proof".into(),
                reason: None,
                parameter: None,
                parameter_index: None,
                argument: None,
            }]
        );
    }
}

#[test]
fn table_assignment_union_keeps_selected_shape_identity_before_compatibility() {
    use emmylua_code_analysis::DiagnosticCode;
    const SOURCE: &str = "---@param flag boolean\nlocal function f(flag)\n---@type { x: table }\nlocal out = { x = flag and selected }\nend\n";
    for complete in [true, false] {
        let (fixture, file, checks) = one_checker_fixture(
            "union-assignment.lua",
            SOURCE,
            DiagnosticCode::AssignTypeMismatch,
        );
        let selected = site(&fixture, file, "selected");
        let value_site = site(&fixture, file, "flag and selected");
        let mut provider = context("union-assignment");
        let object = ObjectRef {
            identity: "selected-object".into(),
            epoch: "epoch-1".into(),
            origin: ObjectOrigin::Lua(selected),
            view: ObjectViewKey {
                context: provider.identity.clone(),
                observation: 1,
                object_id: 1,
            },
        };
        provider.decisions.insert(
            selected,
            BindingDecision::Bound(BoundValue {
                identity: object.identity.clone(),
                proof: "selected-object-proof".into(),
                value: SelectedValue::Object(object.clone()),
            }),
        );
        provider.shapes.insert(
            object.view.clone(),
            ObjectShape {
                key: object.view,
                fields: vec![],
                enumeration: if complete {
                    ShapeEnumeration::Complete
                } else {
                    ShapeEnumeration::Unavailable("enumeration-unavailable".into())
                },
                proof: "object-shape-proof".into(),
            },
        );
        let session = owned_query::session(Arc::new(provider));
        owned_query::model!(model, fixture, file, session.clone());
        let value = model.value_of_expr(value_site.syntax);
        let components = value
            .carried_union()
            .expect("selected union must retain its evidence");
        assert_eq!(components.len(), 2);
        assert_eq!(
            components[0].structural_type(),
            &LuaType::BooleanConst(false)
        );
        let info = components[1].object_info().unwrap();
        assert_eq!(info.identity, "selected-object");
        assert_eq!(info.complete, complete);
        let run = fixture.diagnose(file, checks, &session).unwrap();
        assert!(run.complete);
        assert_eq!(run.diagnostics.len(), usize::from(!complete), "{run:?}");
        if !complete {
            let diagnostic = &run.diagnostics[0];
            assert_eq!(diagnostic.code, DiagnosticCode::AssignTypeMismatch);
            assert_eq!(diagnostic.range, value_site.syntax.get_range());
            assert!(
                diagnostic
                    .message
                    .ends_with("binding object context or shape unavailable")
            );
            assert_eq!(diagnostic.tags, None);
            assert_eq!(diagnostic.data, None);
        }
        assert_eq!(
            session.coverage(),
            [
                (
                    "object_shape",
                    if complete { "bound" } else { "unavailable" },
                    "object-shape-proof",
                    (!complete).then_some("object_shape_incomplete")
                ),
                ("resolve", "bound", "selected-object-proof", None),
                (
                    "structure_check",
                    if complete {
                        "native_checked"
                    } else {
                        "unavailable"
                    },
                    "object-shape-proof",
                    (!complete).then_some("object_context_or_shape_unavailable")
                ),
            ]
            .map(|(operation, state, proof, reason)| BindingCoverage {
                site: selected,
                operation,
                state,
                proof: proof.into(),
                reason: reason.map(str::to_owned),
                parameter: None,
                parameter_index: None,
                argument: None,
            })
        );
    }
}

#[test]
fn return_expected_generic_context_preserves_unconstrained_shell_identity_and_rich_fields() {
    use emmylua_code_analysis::{DiagnosticCode, GenericTplId, LuaMemberKey};
    use emmylua_parser::LuaClosureExpr;
    for generic in ["T", "T: string"] {
        let source =
            format!("---@generic {generic}\n---@return T\nlocal function f() return 42 end\n");
        let (fixture, file, checks) = one_checker_fixture(
            "return-generic.lua",
            &source,
            DiagnosticCode::ReturnTypeMismatch,
        );
        let raw = fixture.semantic_model(file);
        let closure = raw
            .syntax_tree()
            .unwrap()
            .get_red_root()
            .descendants()
            .find_map(LuaClosureExpr::cast)
            .unwrap();
        let expected = raw.return_type(closure.get_syntax_id()).unwrap();
        let LuaType::TplRef(template) = &expected else {
            panic!("{expected:?}");
        };
        assert_eq!(template.get_tpl_id(), GenericTplId::Type(0));
        assert_eq!(template.get_name(), "T");
        assert_eq!(template.get_constraint(), None);
        assert_eq!(template.get_default_type(), None);
        assert!(!template.is_const());
        assert!(
            fixture
                .diagnose_file_with_config(file, checks.clone())
                .unwrap()
                .is_empty()
        );
        let session = owned_query::session(Arc::new(context("return-generic")));
        owned_query::model!(model, fixture, file, session.clone());
        assert_eq!(model.return_type(closure.get_syntax_id()), Some(expected));
        let run = fixture.diagnose(file, checks, &session).unwrap();
        assert!(run.complete);
        assert!(run.diagnostics.is_empty(), "{run:?}");
        assert!(session.coverage().is_empty());
    }
    for annotation in ["fun<T>(value: T): T", "fun<U>(value: T): U"] {
        let source = format!(
            "---@generic T\n---@return {annotation}\nlocal function f() return missing end\n"
        );
        let (fixture, file, _) = one_checker_fixture(
            "nested-generic.lua",
            &source,
            DiagnosticCode::ReturnTypeMismatch,
        );
        let raw = fixture.semantic_model(file);
        let signature = &raw.file_facts().unwrap().signatures[0];
        let syntax = signature.docs.as_ref().unwrap().returns[0];
        let expected = raw.doc_type_lua_rich_in(file, syntax);
        let LuaType::DocFunction(function) = &expected else {
            panic!("{expected:?}");
        };
        assert!(
            function
                .get_generic_params()
                .iter()
                .all(|param| param.get_tpl_id().is_func())
        );
        let session = owned_query::session(Arc::new(context("nested-return-generic")));
        owned_query::model!(model, fixture, file, session.clone());
        assert_eq!(model.return_type(signature.closure_syntax), Some(expected));
        assert!(session.coverage().is_empty());
    }
    // The session's approved rich annotation survives generic normalization.
    // Falling back to doc shell alone would discard x and yield broad Table.
    let (fixture, file, _) = one_checker_fixture(
        "rich-return-generic.lua",
        "---@generic T\n---@return { x: T }\nlocal function f() return missing end\n",
        DiagnosticCode::ReturnTypeMismatch,
    );
    let closure = fixture
        .semantic_model(file)
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .find_map(LuaClosureExpr::cast)
        .unwrap();
    let session = owned_query::session(Arc::new(context("rich-return-generic")));
    owned_query::model!(model, fixture, file, session.clone());
    let expected = model.return_type(closure.get_syntax_id()).unwrap();
    let LuaType::Object(object) = expected else {
        panic!("{expected:?}");
    };
    assert_eq!(object.get_fields().len(), 1);
    let LuaType::TplRef(template) = &object.get_fields()[&LuaMemberKey::Name("x".into())] else {
        panic!("field lost its generic identity");
    };
    assert_eq!(template.get_tpl_id(), GenericTplId::Type(0));
    assert_eq!(template.get_name(), "T");
    assert_eq!(template.get_constraint(), None);
    assert!(session.coverage().is_empty());
}

#[test]
fn nil_checker_uses_selected_reads_before_declaration_flow() {
    use emmylua_code_analysis::DiagnosticCode;
    for expression in ["value()", "value.field"] {
        let source = format!("local origin = true\nlocal value = {{}}\nreturn {expression}\n");
        for selected in ["absent", "unavailable", "bound"] {
            let (fixture, file, checks) =
                one_checker_fixture("selected-nil.lua", &source, DiagnosticCode::NeedCheckNil);
            assert!(
                fixture
                    .diagnose_file_with_config(file, checks.clone())
                    .unwrap()
                    .is_empty()
            );
            let prefix = site(&fixture, file, "value");
            let mut provider = context("nil-checker-selected-read");
            let decision = match selected {
                "absent" => BindingDecision::KnownAbsent {
                    proof: "selected-read-proof".into(),
                },
                "unavailable" => BindingDecision::Unavailable {
                    proof: "selected-read-proof".into(),
                    reason: "read-not-observed".into(),
                },
                _ => BindingDecision::Bound(BoundValue {
                    identity: "selected-boolean".into(),
                    proof: "selected-read-proof".into(),
                    value: SelectedValue::Expression(site(&fixture, file, "true")),
                }),
            };
            provider.decisions.insert(prefix, decision);
            let session = owned_query::session(Arc::new(provider));
            let run = fixture.diagnose(file, checks, &session).unwrap();
            assert!(run.complete);
            assert!(run.execution.contains(&CheckerExecution::Completed(
                "lycheedev_emmy_adapter::need_check_nil::NeedCheckNilChecker"
            )));
            assert_eq!(run.diagnostics.len(), usize::from(selected == "absent"));
            if selected == "absent" {
                let diagnostic = &run.diagnostics[0];
                assert_eq!(diagnostic.code, DiagnosticCode::NeedCheckNil);
                assert_eq!(diagnostic.range, prefix.syntax.get_range());
                assert_eq!(
                    diagnostic.message,
                    if expression == "value()" {
                        "function `value` may be nil"
                    } else {
                        "value may be nil"
                    }
                );
                assert_eq!(diagnostic.tags, None);
                assert_eq!(diagnostic.data, None);
            }
            assert_eq!(
                session.coverage(),
                [BindingCoverage {
                    site: prefix,
                    operation: "resolve",
                    state: match selected {
                        "absent" => "known_absent",
                        "unavailable" => "unavailable",
                        _ => "bound",
                    },
                    proof: "selected-read-proof".into(),
                    reason: (selected == "unavailable").then(|| "read-not-observed".into()),
                    parameter: None,
                    parameter_index: None,
                    argument: None,
                }]
            );
        }
    }
}

#[test]
fn nil_checker_does_not_replace_selected_index_unknown_with_array_fallback() {
    use emmylua_code_analysis::DiagnosticCode;
    const SOURCE: &str = "---@type (string?)[]\nlocal items = {}\nreturn items[1].field\n";
    for absent in [false, true] {
        let (fixture, file, checks) = one_checker_fixture(
            "selected-index-nil.lua",
            SOURCE,
            DiagnosticCode::NeedCheckNil,
        );
        let expected = fixture
            .diagnose_file_with_config(file, checks.clone())
            .unwrap();
        assert_eq!(expected.len(), 1, "ordinary nullable array element");
        let prefix = site(&fixture, file, "items[1]");
        let mut provider = context("nil-checker-selected-index");
        provider.decisions.insert(
            prefix,
            if absent {
                BindingDecision::KnownAbsent {
                    proof: "index-proof".into(),
                }
            } else {
                BindingDecision::Unavailable {
                    proof: "index-proof".into(),
                    reason: "index-not-observed".into(),
                }
            },
        );
        let session = owned_query::session(Arc::new(provider));
        let run = fixture.diagnose(file, checks, &session).unwrap();
        assert!(run.complete);
        assert_eq!(run.diagnostics.len(), usize::from(absent));
        if absent {
            let diagnostic = &run.diagnostics[0];
            assert_eq!(diagnostic.code, DiagnosticCode::NeedCheckNil);
            assert_eq!(diagnostic.range, prefix.syntax.get_range());
            assert_eq!(diagnostic.message, "items[1] may be nil");
            assert_eq!(diagnostic.severity, expected[0].severity.unwrap());
            assert_eq!(diagnostic.tags, expected[0].tags);
            assert_eq!(diagnostic.data, expected[0].data);
        }
        assert_eq!(
            session.coverage(),
            [BindingCoverage {
                site: prefix,
                operation: "resolve",
                state: if absent {
                    "known_absent"
                } else {
                    "unavailable"
                },
                proof: "index-proof".into(),
                reason: (!absent).then(|| "index-not-observed".into()),
                parameter: None,
                parameter_index: None,
                argument: None,
            }]
        );
    }
}

#[test]
fn unmanaged_nil_checker_preserves_normal_flow_and_all_upstream_diagnostic_fields() {
    use emmylua_code_analysis::DiagnosticCode;
    for (source, count) in [
        ("local value = nil\nreturn value.field\n", 1),
        ("local value = nil\nvalue = {}\nreturn value.field\n", 0),
        (
            "---@param value table?\nlocal function f(value)\nif value then return value.field end\nend\n",
            0,
        ),
        (
            "---@param value table?\nlocal function f(value)\nif not value then return value.field end\nend\n",
            1,
        ),
    ] {
        let (fixture, file, checks) = one_checker_fixture(
            "ordinary-nil-flow.lua",
            source,
            DiagnosticCode::NeedCheckNil,
        );
        let expected = fixture
            .diagnose_file_with_config(file, checks.clone())
            .unwrap();
        assert_eq!(expected.len(), count, "{source}: {expected:?}");
        let session = owned_query::session(Arc::new(context("ordinary-nil-flow")));
        let run = fixture.diagnose(file, checks, &session).unwrap();
        assert!(run.complete);
        assert_eq!(run.diagnostics.len(), expected.len(), "{source}: {run:?}");
        let lines = fixture.db.line_index(file).unwrap();
        for (actual, expected) in run.diagnostics.iter().zip(expected) {
            let start = lines.get_line_col(actual.range.start(), source).unwrap();
            let end = lines.get_line_col(actual.range.end(), source).unwrap();
            assert_eq!(
                expected.range,
                lsp_types::Range::new(
                    lsp_types::Position::new(start.0 as u32, start.1 as u32),
                    lsp_types::Position::new(end.0 as u32, end.1 as u32),
                )
            );
            assert_eq!(
                expected.code,
                Some(lsp_types::NumberOrString::String(
                    actual.code.get_name().into()
                ))
            );
            assert_eq!(expected.severity, Some(actual.severity));
            assert_eq!(expected.message, actual.message);
            assert_eq!(expected.tags, actual.tags);
            assert_eq!(expected.data, actual.data);
        }
        assert!(session.coverage().is_empty());
    }
}

#[test]
fn undefined_field_checker_keeps_xml_selected_shape_instead_of_raw_named_type() {
    use emmylua_code_analysis::DiagnosticCode;
    use emmylua_parser::LuaIndexExpr;
    const SOURCE: &str = "---@class Raw\nlocal Raw = {}\nlocal truth = true\n---@type Raw\nlocal value = {}\nreturn value.visible\n";
    for complete in [true, false] {
        let (fixture, file, checks) = one_checker_fixture(
            "selected-xml-field.lua",
            SOURCE,
            DiagnosticCode::UndefinedField,
        );
        let raw = fixture
            .diagnose_file_with_config(file, checks.clone())
            .unwrap();
        assert_eq!(
            raw.len(),
            1,
            "the ordinary class has no visible field: {raw:?}"
        );
        assert_eq!(raw[0].message, "Undefined field: `visible`");
        let prefix = site(&fixture, file, "value");
        let index_site = site(&fixture, file, "value.visible");
        let mut provider = context("selected-xml-field");
        let object = ObjectRef {
            identity: "xml-object".into(),
            epoch: "epoch-1".into(),
            origin: ObjectOrigin::Lua(prefix),
            view: ObjectViewKey {
                context: provider.identity.clone(),
                observation: 1,
                object_id: 1,
            },
        };
        provider.decisions.insert(
            prefix,
            BindingDecision::Bound(BoundValue {
                identity: object.identity.clone(),
                proof: "object-proof".into(),
                value: SelectedValue::Object(object.clone()),
            }),
        );
        provider.shapes.insert(
            object.view.clone(),
            ObjectShape {
                key: object.view,
                proof: "shape-proof".into(),
                enumeration: if complete {
                    ShapeEnumeration::Complete
                } else {
                    ShapeEnumeration::Unavailable("other-fields-unknown".into())
                },
                fields: vec![ObjectField {
                    key: "visible".into(),
                    presence: FieldPresence::Present,
                    value: BindingDecision::Bound(BoundValue {
                        identity: "field-boolean".into(),
                        proof: "field-proof".into(),
                        value: SelectedValue::Expression(site(&fixture, file, "true")),
                    }),
                    destination_write: FieldWriteOrigin::Xml(ExternalSourceSpan {
                        path: "fixture.xml".into(),
                        content_sha256: "a".repeat(64),
                        range: lycheedev_emmy_adapter::ByteRange { start: 0, end: 1 },
                    }),
                    source_write: None,
                    copy_origin: None,
                }],
            },
        );
        let session = owned_query::session(Arc::new(provider));
        let run = fixture.diagnose(file, checks, &session).unwrap();
        assert!(run.complete);
        assert!(run.diagnostics.is_empty(), "{run:?}");
        assert!(run.execution.contains(&CheckerExecution::Completed(
            "lycheedev_emmy_adapter::undefined_field::UndefinedFieldChecker"
        )));
        owned_query::model!(model, fixture, file, session.clone());
        let index = LuaIndexExpr::cast(
            index_site
                .syntax
                .to_node_from_root(
                    &fixture
                        .semantic_model(file)
                        .syntax_tree()
                        .unwrap()
                        .get_red_root(),
                )
                .unwrap(),
        )
        .unwrap();
        let member = model.resolve_member(&index).unwrap();
        assert_eq!(member.member_id, None);
        assert_eq!(member.file_id, None);
        assert_eq!(member.name, "visible");
        assert_eq!(
            member.member_type.as_ref().unwrap().structural_type(),
            &LuaType::BooleanConst(true)
        );
        assert_eq!(
            session.coverage(),
            [
                ("object_field", "bound", "field-proof", None),
                (
                    "object_shape",
                    if complete { "bound" } else { "unavailable" },
                    "shape-proof",
                    (!complete).then_some("object_shape_incomplete")
                ),
                ("resolve", "bound", "object-proof", None),
                (
                    "member_lookup",
                    if complete { "bound" } else { "unavailable" },
                    "shape-proof",
                    (!complete).then_some("object_shape_incomplete")
                ),
            ]
            .map(|(operation, state, proof, reason)| BindingCoverage {
                site: prefix,
                operation,
                state,
                proof: proof.into(),
                reason: reason.map(str::to_owned),
                parameter: None,
                parameter_index: None,
                argument: None,
            })
        );
    }
}

#[test]
fn await_checker_does_not_resurrect_raw_async_docs_over_a_selected_callee() {
    use emmylua_code_analysis::DiagnosticCode;
    use emmylua_parser::LuaClosureExpr;
    const SOURCE: &str =
        "---@async\nlocal function work() end\nlocal function replacement() end\nwork()\n";
    for selected in ["unmanaged", "absent", "unavailable", "sync", "async"] {
        let (fixture, file, checks) =
            one_checker_fixture("selected-async.lua", SOURCE, DiagnosticCode::AwaitInSync);
        let expected = fixture
            .diagnose_file_with_config(file, checks.clone())
            .unwrap();
        assert_eq!(
            expected.len(),
            1,
            "ordinary async declaration: {expected:?}"
        );
        let prefix = site(&fixture, file, "work");
        let closures = fixture
            .semantic_model(file)
            .syntax_tree()
            .unwrap()
            .get_red_root()
            .descendants()
            .filter_map(LuaClosureExpr::cast)
            .collect::<Vec<_>>();
        assert_eq!(closures.len(), 2);
        let mut provider = context("selected-async-callee");
        let decision = match selected {
            "unmanaged" => BindingDecision::NotManaged,
            "absent" => BindingDecision::KnownAbsent {
                proof: "callee-proof".into(),
            },
            "unavailable" => BindingDecision::Unavailable {
                proof: "callee-proof".into(),
                reason: "callee-not-observed".into(),
            },
            _ => BindingDecision::Bound(BoundValue {
                identity: "actual-callee".into(),
                proof: "callee-proof".into(),
                value: SelectedValue::Function(FunctionRef {
                    file_id: file,
                    closure: closures[usize::from(selected == "sync")].get_syntax_id(),
                    declaration: None,
                }),
            }),
        };
        provider.decisions.insert(prefix, decision);
        let session = owned_query::session(Arc::new(provider));
        let run = fixture.diagnose(file, checks, &session).unwrap();
        assert!(run.complete);
        assert!(run.execution.contains(&CheckerExecution::Completed(
            "lycheedev_emmy_adapter::await_in_sync::AwaitInSyncChecker"
        )));
        let reports = matches!(selected, "unmanaged" | "async");
        assert_eq!(
            run.diagnostics.len(),
            usize::from(reports),
            "{selected}: {run:?}"
        );
        if reports {
            let diagnostic = &run.diagnostics[0];
            assert_eq!(diagnostic.code, DiagnosticCode::AwaitInSync);
            assert_eq!(diagnostic.range, prefix.syntax.get_range());
            assert_eq!(diagnostic.message, expected[0].message);
            assert_eq!(diagnostic.severity, expected[0].severity.unwrap());
            assert_eq!(diagnostic.tags, expected[0].tags);
            assert_eq!(diagnostic.data, expected[0].data);
        }
        if selected == "unmanaged" {
            assert!(session.coverage().is_empty());
        } else {
            assert_eq!(
                session.coverage(),
                [BindingCoverage {
                    site: prefix,
                    operation: "resolve",
                    state: match selected {
                        "absent" => "known_absent",
                        "unavailable" => "unavailable",
                        _ => "bound",
                    },
                    proof: "callee-proof".into(),
                    reason: (selected == "unavailable").then(|| "callee-not-observed".into()),
                    parameter: None,
                    parameter_index: None,
                    argument: None,
                }]
            );
        }
    }
}

#[test]
fn unnecessary_assert_uses_selected_argument_before_raw_literal_annotation() {
    use emmylua_code_analysis::DiagnosticCode;
    const SOURCE: &str =
        "local replacement = false\n---@type true\nlocal value = true\nassert(value)\n";
    for selected in ["unmanaged", "absent", "unavailable", "false"] {
        let (fixture, file, checks) = one_checker_fixture(
            "selected-assert.lua",
            SOURCE,
            DiagnosticCode::UnnecessaryAssert,
        );
        let expected = fixture
            .diagnose_file_with_config(file, checks.clone())
            .unwrap();
        assert_eq!(expected.len(), 1);
        assert_eq!(
            expected[0].message,
            "Unnecessary assert: this expression is always truthy"
        );
        let argument = site(&fixture, file, "value");
        let call = site(&fixture, file, "assert(value)");
        let mut provider = context("selected-assert-argument");
        provider.decisions.insert(
            argument,
            match selected {
                "unmanaged" => BindingDecision::NotManaged,
                "absent" => BindingDecision::KnownAbsent {
                    proof: "argument-proof".into(),
                },
                "unavailable" => BindingDecision::Unavailable {
                    proof: "argument-proof".into(),
                    reason: "argument-not-observed".into(),
                },
                _ => BindingDecision::Bound(BoundValue {
                    identity: "actual-argument".into(),
                    proof: "argument-proof".into(),
                    value: SelectedValue::Expression(site(&fixture, file, "false")),
                }),
            },
        );
        let session = owned_query::session(Arc::new(provider));
        let run = fixture.diagnose(file, checks, &session).unwrap();
        assert!(run.complete);
        assert!(run.execution.contains(&CheckerExecution::Completed(
            "lycheedev_emmy_adapter::unnecessary_assert::UnnecessaryAssertChecker"
        )));
        assert_eq!(
            run.diagnostics.len(),
            usize::from(selected != "unavailable")
        );
        if selected != "unavailable" {
            let diagnostic = &run.diagnostics[0];
            assert_eq!(diagnostic.code, DiagnosticCode::UnnecessaryAssert);
            assert_eq!(diagnostic.range, call.syntax.get_range());
            assert_eq!(
                diagnostic.message,
                if selected == "unmanaged" {
                    "Unnecessary assert: this expression is always truthy"
                } else {
                    "Impossible assert: this expression is always falsy; prefer `error()`"
                }
            );
            assert_eq!(diagnostic.severity, expected[0].severity.unwrap());
            assert_eq!(diagnostic.tags, expected[0].tags);
            assert_eq!(diagnostic.data, expected[0].data);
        }
        if selected == "unmanaged" {
            assert!(session.coverage().is_empty());
        } else {
            assert_eq!(
                session.coverage(),
                [BindingCoverage {
                    site: argument,
                    operation: "resolve",
                    state: match selected {
                        "absent" => "known_absent",
                        "unavailable" => "unavailable",
                        _ => "bound",
                    },
                    proof: "argument-proof".into(),
                    reason: (selected == "unavailable").then(|| "argument-not-observed".into()),
                    parameter: None,
                    parameter_index: None,
                    argument: None,
                }]
            );
        }
    }
}

#[test]
fn module_export_checker_does_not_recover_a_replaced_import_surface() {
    use emmylua_code_analysis::{CheckConfig, DiagnosticCode, Emmyrc};
    for (code, suffix) in [
        (
            DiagnosticCode::UndefinedField,
            "local result = imported.selected\n",
        ),
        (DiagnosticCode::InjectField, "imported.selected = 1\n"),
    ] {
        for selected in [
            "unmanaged",
            "unavailable",
            "absent",
            "bound",
            "initializer-unavailable",
        ] {
            let source = format!("local truth = true\nlocal imported = require('m')\n{suffix}");
            let mut config = Emmyrc::default();
            config.diagnostics.disable = DiagnosticCode::all()
                .into_iter()
                .filter(|other| *other != code)
                .collect();
            config.diagnostics.enables = vec![code];
            let checks = Arc::new(CheckConfig::new(&config));
            let mut analysis = EmmyLuaAnalysis::new();
            analysis.update_config(Arc::new(config));
            let root = std::env::current_dir()
                .unwrap()
                .join("owned-export-contracts");
            analysis.add_main_workspace(root.clone());
            let fixture = OwnedQueryTest::ingest(
                analysis,
                vec![
                    (root.join("main.lua"), Some(Arc::from(source.as_str()))),
                    (root.join("m.lua"), Some(Arc::from("return { raw = 1 }\n"))),
                ],
            )
            .unwrap();
            let file = fixture.file_ids()[0];
            assert_eq!(
                fixture.semantic_model(file).module_file_of("m"),
                Some(fixture.file_ids()[1])
            );
            let expected = fixture
                .diagnose_file_with_config(file, checks.clone())
                .unwrap();
            assert!(!expected.is_empty(), "the raw module has no selected field");
            let prefix = site(&fixture, file, "imported");
            let index = site(&fixture, file, "imported.selected");
            let managed = if selected == "initializer-unavailable" {
                site(&fixture, file, "require('m')")
            } else {
                prefix
            };
            let mut provider = context("selected-module-surface");
            let decision = match selected {
                "unmanaged" => BindingDecision::NotManaged,
                "unavailable" | "initializer-unavailable" => BindingDecision::Unavailable {
                    proof: "selected-import-proof".into(),
                    reason: "import-not-observed".into(),
                },
                "absent" => BindingDecision::KnownAbsent {
                    proof: "selected-import-proof".into(),
                },
                _ => {
                    let object = ObjectRef {
                        identity: "selected-import".into(),
                        epoch: "epoch-1".into(),
                        origin: ObjectOrigin::Lua(prefix),
                        view: ObjectViewKey {
                            context: provider.identity.clone(),
                            observation: 1,
                            object_id: 1,
                        },
                    };
                    provider.shapes.insert(
                        object.view.clone(),
                        ObjectShape {
                            key: object.view.clone(),
                            proof: "import-shape-proof".into(),
                            enumeration: ShapeEnumeration::Complete,
                            fields: vec![ObjectField {
                                key: "selected".into(),
                                presence: FieldPresence::Present,
                                value: BindingDecision::Bound(BoundValue {
                                    identity: "selected-field".into(),
                                    proof: "import-field-proof".into(),
                                    value: SelectedValue::Expression(site(&fixture, file, "true")),
                                }),
                                destination_write: FieldWriteOrigin::Xml(ExternalSourceSpan {
                                    path: "fixture.xml".into(),
                                    content_sha256: "b".repeat(64),
                                    range: lycheedev_emmy_adapter::ByteRange { start: 0, end: 1 },
                                }),
                                source_write: None,
                                copy_origin: None,
                            }],
                        },
                    );
                    BindingDecision::Bound(BoundValue {
                        identity: object.identity.clone(),
                        proof: "selected-import-proof".into(),
                        value: SelectedValue::Object(object),
                    })
                }
            };
            provider.decisions.insert(managed, decision);
            let session = owned_query::session(Arc::new(provider));
            let run = fixture.diagnose(file, checks, &session).unwrap();
            assert!(run.complete);
            assert!(run.execution.contains(&CheckerExecution::Completed(
                "lycheedev_emmy_adapter::check_export::CheckExportChecker"
            )));
            if selected == "unmanaged" {
                assert_eq!(run.diagnostics.len(), expected.len());
                let lines = fixture.db.line_index(file).unwrap();
                for (actual, expected) in run.diagnostics.iter().zip(expected) {
                    let start = lines.get_line_col(actual.range.start(), &source).unwrap();
                    let end = lines.get_line_col(actual.range.end(), &source).unwrap();
                    assert_eq!(
                        expected.range,
                        lsp_types::Range::new(
                            lsp_types::Position::new(start.0 as u32, start.1 as u32),
                            lsp_types::Position::new(end.0 as u32, end.1 as u32),
                        )
                    );
                    assert_eq!(
                        expected.code,
                        Some(lsp_types::NumberOrString::String(
                            actual.code.get_name().into()
                        ))
                    );
                    assert_eq!(expected.severity, Some(actual.severity));
                    assert_eq!(expected.message, actual.message);
                    assert_eq!(expected.tags, actual.tags);
                    assert_eq!(expected.data, actual.data);
                }
                assert!(session.coverage().is_empty());
                continue;
            }
            assert!(run.diagnostics.is_empty(), "{code:?}/{selected}: {run:?}");
            let receipt =
                |site, operation, state, proof: &str, reason: Option<&str>| BindingCoverage {
                    site,
                    operation,
                    state,
                    proof: proof.into(),
                    reason: reason.map(str::to_owned),
                    parameter: None,
                    parameter_index: None,
                    argument: None,
                };
            let mut coverage = if selected == "bound" {
                vec![
                    receipt(prefix, "object_field", "bound", "import-field-proof", None),
                    receipt(prefix, "object_shape", "bound", "import-shape-proof", None),
                    receipt(prefix, "resolve", "bound", "selected-import-proof", None),
                ]
            } else {
                vec![receipt(
                    managed,
                    "resolve",
                    if selected == "absent" {
                        "known_absent"
                    } else {
                        "unavailable"
                    },
                    "selected-import-proof",
                    (selected != "absent").then_some("import-not-observed"),
                )]
            };
            if selected == "bound" {
                if code == DiagnosticCode::InjectField {
                    coverage.push(receipt(
                        prefix,
                        "structure_check",
                        "native_checked",
                        "import-shape-proof",
                        None,
                    ));
                }
                coverage.push(receipt(
                    prefix,
                    "member_lookup",
                    "bound",
                    "import-shape-proof",
                    None,
                ));
            } else if code == DiagnosticCode::UndefinedField {
                coverage.push(receipt(
                    index,
                    "resolve_member",
                    "unavailable",
                    "",
                    Some("managed_member_declaration_unavailable"),
                ));
            }
            assert_eq!(session.coverage(), coverage, "{code:?}/{selected}");
        }
    }
}
