//! Public-API regressions for the new public-base binding reconstruction.
// Migrated unchanged semantic assertions from the pinned reconstruction's tests.
// Ingestion, semantic queries, diagnostics and coverage use the owned session.
use lycheedev_emmy_adapter::session::*;
#[path = "support/owned_query.rs"]
mod owned_query;
use emmylua_code_analysis::{EmmyLuaAnalysis, FileId, LuaType, VariadicType};
use emmylua_parser::{LuaAstNode, LuaClosureExpr, LuaExpr, LuaReturnStat};
use owned_query::{OwnedQueryTest, SessionCoverageTest};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
struct Context {
    decisions: HashMap<BindingSite, BindingDecision<BoundValue>>,
    calls: HashMap<BindingSite, BindingDecision<CallPermit>>,
    observations: HashMap<BindingSite, BindingDecision<CallObservation>>,
    shapes: HashMap<ObjectViewKey, ObjectShape>,
}

impl BindingContext for Context {
    fn identity(&self) -> &str {
        "cardinality-context"
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

    fn call_observation(&self, site: BindingSite) -> BindingDecision<CallObservation> {
        self.observations
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
                proof: "missing-shape".into(),
                reason: "shape_missing".into(),
            })
    }
}

fn site(analysis: &EmmyLuaAnalysis, file_id: FileId, text: &str) -> BindingSite {
    let expr = analysis
        .semantic_model(file_id)
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .filter_map(LuaExpr::cast)
        .find(|expr| expr.syntax().text().to_string() == text)
        .unwrap_or_else(|| panic!("missing expression: {text}"));
    BindingSite {
        file_id,
        syntax: expr.get_syntax_id(),
    }
}

fn load(source: &str) -> (OwnedQueryTest, FileId) {
    let mut analysis = EmmyLuaAnalysis::new();
    let root = std::env::current_dir()
        .unwrap()
        .join("binding-cardinality-input");
    analysis.add_main_workspace(root.clone());
    let analysis = OwnedQueryTest::ingest(
        analysis,
        vec![(root.join("cardinality.lua"), Some(Arc::from(source)))],
    )
    .unwrap();
    let file = analysis.file_ids()[0];
    (analysis, file)
}

fn fixture(statement: &str, return_count: usize) -> (OwnedQueryTest, FileId, BindingSite, Context) {
    let source =
        format!("local observed = true\nlocal function F() return 901, 'raw' end\n{statement}\n");
    let (analysis, file_id) = load(&source);
    let call = site(&analysis, file_id, "F()");
    let callee = site(&analysis, file_id, "F");
    let closure = analysis
        .semantic_model(file_id)
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .find_map(LuaClosureExpr::cast)
        .unwrap();
    let function = FunctionRef {
        file_id,
        closure: closure.get_syntax_id(),
        declaration: None,
    };
    let mut context = Context::default();
    context.decisions.insert(
        callee,
        BindingDecision::Bound(BoundValue {
            identity: "selected-function".into(),
            proof: "callee-proof".into(),
            value: SelectedValue::Function(function.clone()),
        }),
    );
    context.calls.insert(
        call,
        BindingDecision::Bound(CallPermit {
            callee,
            proof: "call-proof".into(),
            capability_profile: "typed-call-v1".into(),
        }),
    );
    let returns = (0..return_count)
        .map(|index| {
            if index == 0 {
                BindingDecision::Bound(BoundValue {
                    identity: "observed-boolean".into(),
                    proof: "return-proof".into(),
                    value: SelectedValue::Expression(site(&analysis, file_id, "true")),
                })
            } else {
                BindingDecision::KnownAbsent {
                    proof: "known-nil-slot".into(),
                }
            }
        })
        .collect();
    context.observations.insert(
        call,
        BindingDecision::Bound(CallObservation {
            callee: ObservedCallee::Source(function),
            selected_callee_identity: "selected-function".into(),
            closure_identity: "closure-identity".into(),
            invocation_identity: "invocation-identity".into(),
            returns,
            proof: "observation-proof".into(),
        }),
    );
    (analysis, file_id, call, context)
}

fn exact_returns(slots: Vec<LuaType>) -> LuaType {
    LuaType::Variadic(Arc::new(VariadicType::Multi(slots)))
}

const SCALAR_STATEMENTS: &[&str] = &[
    "local first = F()",
    "local first, second = F(), 0",
    "local before, first = 0, F()",
    "first = F()",
    "first, second = F(), 0",
    "before, first = 0, F()",
    "return F(), 0",
    "sink(F(), 0)",
    "return (F())",
    "return ((F()))",
    "local first, second = (F())",
    "sink((F()))",
    "local object = { F(), 0 }",
    "local object = { key = F() }",
    "local value = F() + 1",
    "local value = F() and 0",
    "local value = F().key",
    "F()()",
    "if F() then end",
];

const EXPANDING_STATEMENTS: &[&str] = &[
    "return F()",
    "return 0, F()",
    "sink(F())",
    "sink(0, F())",
    "local first, second = F()",
    "local before, first, second = 0, F()",
    "first, second = F()",
    "before, first, second = 0, F()",
    "local object = { F() }",
    "local object = { 0, F(), }",
    "for key, value in F() do end",
    "for key, value in 0, F() do end",
];

#[test]
fn managed_calls_adjust_to_one_result_in_scalar_positions() {
    for statement in SCALAR_STATEMENTS {
        let (analysis, file_id, call, context) = fixture(statement, 2);
        owned_query::model!(
            model,
            analysis,
            file_id,
            owned_query::session(Arc::new(context))
        );
        assert_eq!(
            model.type_of_expr(call.syntax),
            LuaType::BooleanConst(true),
            "{statement}"
        );
        if statement.contains("(F())") {
            assert_eq!(
                model.type_of_expr(site(&analysis, file_id, "(F())").syntax),
                LuaType::BooleanConst(true),
                "parenthesized public query: {statement}"
            );
        }
    }
}

#[test]
fn managed_list_tails_keep_exact_return_packs_including_one_and_zero_slots() {
    for statement in EXPANDING_STATEMENTS {
        for count in 0..=2 {
            let (analysis, file_id, call, context) = fixture(statement, count);
            owned_query::model!(
                model,
                analysis,
                file_id,
                owned_query::session(Arc::new(context))
            );
            let expected = match count {
                0 => vec![],
                1 => vec![LuaType::BooleanConst(true)],
                _ => vec![LuaType::BooleanConst(true), LuaType::Nil],
            };
            assert_eq!(
                model.type_of_expr(call.syntax),
                exact_returns(expected),
                "{statement}: {count} slots"
            );
        }
    }
}

#[test]
fn known_empty_returns_are_nil_in_scalar_positions() {
    for statement in SCALAR_STATEMENTS {
        let (analysis, file_id, call, context) = fixture(statement, 0);
        owned_query::model!(
            model,
            analysis,
            file_id,
            owned_query::session(Arc::new(context))
        );
        assert_eq!(model.type_of_expr(call.syntax), LuaType::Nil, "{statement}");
    }
}

#[test]
fn public_expression_lists_do_not_expand_nonlast_managed_calls() {
    for (statement, expected) in [
        (
            "return F(), 7",
            vec![LuaType::BooleanConst(true), LuaType::IntegerConst(7)],
        ),
        (
            "return 7, F()",
            vec![
                LuaType::IntegerConst(7),
                LuaType::BooleanConst(true),
                LuaType::Nil,
            ],
        ),
        ("return (F())", vec![LuaType::BooleanConst(true)]),
    ] {
        let (analysis, file_id, _, context) = fixture(statement, 2);
        owned_query::model!(
            model,
            analysis,
            file_id,
            owned_query::session(Arc::new(context))
        );
        let exprs = model
            .syntax_tree()
            .unwrap()
            .get_red_root()
            .descendants()
            .filter_map(LuaReturnStat::cast)
            .last()
            .unwrap()
            .get_expr_list()
            .collect::<Vec<_>>();
        let actual = model
            .infer_expr_list_types(&exprs, None)
            .into_iter()
            .map(|(ty, _)| ty)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{statement}");
    }
}

#[test]
fn unknown_observations_and_unknown_slots_never_recover_raw_return_types() {
    for statement in ["return F()", "return (F())", "local value = F()"] {
        for unavailable in [
            BindingDecision::NotManaged,
            BindingDecision::KnownAbsent {
                proof: "missing-observation".into(),
            },
            BindingDecision::Unavailable {
                proof: "blocked-observation".into(),
                reason: "observed_return_unavailable".into(),
            },
        ] {
            let (analysis, file_id, call, mut context) = fixture(statement, 2);
            context.observations.insert(call, unavailable);
            let session = owned_query::session(Arc::new(context));
            owned_query::model!(model, analysis, file_id, session.clone());
            assert_eq!(
                model.type_of_expr(call.syntax),
                LuaType::Unknown,
                "{statement}"
            );
            assert!(
                session
                    .coverage()
                    .iter()
                    .any(|row| row.operation == "call_return")
            );
        }
        for unavailable in [
            BindingDecision::NotManaged,
            BindingDecision::Unavailable {
                proof: "blocked-slot".into(),
                reason: "observed_slot_unavailable".into(),
            },
        ] {
            let (analysis, file_id, call, mut context) = fixture(statement, 1);
            let BindingDecision::Bound(observation) = context.observations.get_mut(&call).unwrap()
            else {
                panic!("fixture must have an observation")
            };
            observation.returns = vec![unavailable];
            owned_query::model!(
                model,
                analysis,
                file_id,
                owned_query::session(Arc::new(context))
            );
            assert_eq!(
                model.type_of_expr(call.syntax),
                if statement == "return F()" {
                    exact_returns(vec![LuaType::Unknown])
                } else {
                    LuaType::Unknown
                },
                "{statement}"
            );
        }
    }
}

#[test]
fn assignment_padding_distinguishes_known_empty_from_unknown_arity() {
    for unavailable in [false, true] {
        let (analysis, file_id, call, mut context) = fixture("local first, second = F()", 0);
        if unavailable {
            context.calls.insert(
                call,
                BindingDecision::Unavailable {
                    proof: "denied-call".into(),
                    reason: "call_effect_unavailable".into(),
                },
            );
        }
        owned_query::model!(
            model,
            analysis,
            file_id,
            owned_query::session(Arc::new(context))
        );
        for name in ["first", "second"] {
            let decl = model
                .file_facts()
                .unwrap()
                .decls
                .iter()
                .find(|decl| decl.name == name)
                .unwrap();
            assert_eq!(
                model.type_of_decl(&decl.id),
                Some(if unavailable {
                    LuaType::Unknown
                } else {
                    LuaType::Nil
                }),
                "{name}: unavailable={unavailable}"
            );
        }
    }
}

#[test]
fn parenthesized_calls_assign_one_value_and_pad_remaining_targets_with_nil() {
    for count in 0..=2 {
        let (analysis, file_id, _, context) = fixture("local first, second = (F())", count);
        owned_query::model!(
            model,
            analysis,
            file_id,
            owned_query::session(Arc::new(context))
        );
        for (name, expected) in [
            (
                "first",
                if count == 0 {
                    LuaType::Nil
                } else {
                    LuaType::BooleanConst(true)
                },
            ),
            ("second", LuaType::Nil),
        ] {
            let decl = model
                .file_facts()
                .unwrap()
                .decls
                .iter()
                .find(|decl| decl.name == name)
                .unwrap();
            assert_eq!(
                model.type_of_decl(&decl.id),
                Some(expected),
                "{name}: {count} slots"
            );
        }
    }
}

#[test]
fn environment_receiver_without_native_representation_is_unavailable() {
    let (analysis, file_id) = load("local F = function() return 901 end\nreturn F\n");
    let callee = site(&analysis, file_id, "F");
    let callable = EnvironmentCallable {
        origin: EnvironmentOrigin {
            material: "fixture-api".into(),
            material_sha256: "a".repeat(64),
            pointer: "/functions/0".into(),
            identity: "host-function".into(),
        },
        declaration: EnvironmentRecord {
            kind: "function".into(),
            name: "F".into(),
            owner: Some(EnvironmentOwner {
                kind: "script-object".into(),
                name: "Frame".into(),
                source: serde_json::Value::Null,
            }),
            ..Default::default()
        },
        types: Arc::new(EnvironmentTypeCatalog::default()),
    };
    let mut context = Context::default();
    context.decisions.insert(
        callee,
        BindingDecision::Bound(BoundValue {
            identity: "selected-environment-callable".into(),
            proof: "environment-proof".into(),
            value: SelectedValue::EnvironmentCallable(Arc::new(callable)),
        }),
    );
    let session = owned_query::session(Arc::new(context));
    owned_query::model!(model, analysis, file_id, session.clone());
    assert_eq!(model.type_of_expr(callee.syntax), LuaType::Unknown);
    assert!(session.coverage().iter().any(|row| {
        row.reason.as_deref() == Some("environment_receiver_unavailable")
            && row.state == "unavailable"
    }));
}

#[test]
fn object_shape_admission_is_bounded_without_evicting_frozen_views() {
    let mut source = String::from("local origin = { raw_only = 901 }\nreturn ");
    source.push_str(
        &(0..=1024)
            .map(|index| format!("object_{index}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    let (analysis, file_id) = load(&source);
    let origin = site(&analysis, file_id, "{ raw_only = 901 }");
    let model = analysis.semantic_model(file_id);
    let sites = model
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .filter_map(LuaExpr::cast)
        .filter(|expr| expr.syntax().text().to_string().starts_with("object_"))
        .map(|expr| BindingSite {
            file_id,
            syntax: expr.get_syntax_id(),
        })
        .collect::<Vec<_>>();
    assert_eq!(sites.len(), 1025);
    let mut context = Context::default();
    for (index, site) in sites.iter().enumerate() {
        let key = ObjectViewKey {
            context: context.identity().into(),
            observation: index,
            object_id: index,
        };
        context.decisions.insert(
            *site,
            BindingDecision::Bound(BoundValue {
                identity: format!("value:{index}"),
                proof: "object-proof".into(),
                value: SelectedValue::Object(ObjectRef {
                    identity: format!("object:{index}"),
                    epoch: "epoch-1".into(),
                    origin: ObjectOrigin::Lua(origin),
                    view: key.clone(),
                }),
            }),
        );
        context.shapes.insert(
            key.clone(),
            ObjectShape {
                key,
                fields: vec![],
                enumeration: ShapeEnumeration::Complete,
                proof: "shape-proof".into(),
            },
        );
    }
    let session = owned_query::session(Arc::new(context));
    owned_query::model!(model, analysis, file_id, session.clone());
    for site in &sites[..1024] {
        assert!(matches!(
            model.type_of_expr(site.syntax),
            LuaType::Object(_)
        ));
    }
    assert_eq!(model.type_of_expr(sites[1024].syntax), LuaType::Unknown);
    assert!(session.coverage().iter().any(|row| {
        row.site == sites[1024] && row.reason.as_deref() == Some("object_shape_budget")
    }));
    // A new semantic model cannot hide eviction behind its expression cache.
    owned_query::model!(next_model, analysis, file_id, session);
    assert!(matches!(
        next_model.type_of_expr(sites[0].syntax),
        LuaType::Object(_)
    ));
}
