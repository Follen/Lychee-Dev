#![allow(dead_code)]
use emmylua_code_analysis::{EmmyLuaAnalysis, LuaType, SemanticId, VariadicType};
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaClosureExpr};
use lycheedev_emmy_adapter::session::*;
use std::sync::Arc;

#[derive(Clone)]
pub struct Provider {
    pub query: BindingSite,
    pub callee: BindingSite,
    pub call: BindingDecision<CallPermit>,
    pub observation: BindingDecision<CallObservation>,
    pub selected_callee: BindingDecision<BoundValue>,
    pub selected_query: BindingDecision<BoundValue>,
}
impl BindingContext for Provider {
    fn identity(&self) -> &str {
        "selected-query-oracle"
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        if site == self.callee {
            self.selected_callee.clone()
        } else if site == self.query {
            self.selected_query.clone()
        } else {
            BindingDecision::NotManaged
        }
    }
    fn call(&self, site: BindingSite) -> BindingDecision<CallPermit> {
        if site == self.query {
            self.call.clone()
        } else {
            BindingDecision::NotManaged
        }
    }
    fn call_observation(&self, site: BindingSite) -> BindingDecision<CallObservation> {
        if site == self.query {
            self.observation.clone()
        } else {
            BindingDecision::NotManaged
        }
    }
}
#[derive(Clone, Debug)]
pub struct Stop {
    pub ordinal: usize,
    pub token: Arc<()>,
}
pub struct Backend {
    pub checkpoints: usize,
    pub stop_at: usize,
    pub token: Arc<()>,
}
impl Default for Backend {
    fn default() -> Self {
        Self {
            checkpoints: 0,
            stop_at: usize::MAX,
            token: Arc::new(()),
        }
    }
}
impl SelectedProjectionBackend for Backend {
    type Value = LuaType;
    type Error = Stop;
    fn checkpoint(&mut self) -> Result<(), Stop> {
        self.checkpoints += 1;
        if self.checkpoints == self.stop_at {
            return Err(Stop {
                ordinal: self.checkpoints,
                token: self.token.clone(),
            });
        }
        Ok(())
    }
    fn plain(&mut self, ty: LuaType) -> LuaType {
        ty
    }
    fn expression(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: BindingSite,
    ) -> ProjectionResult<LuaType, Stop> {
        panic!("context recursive backend not part of atomic call oracle")
    }
    fn parameter(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &SemanticId,
    ) -> ProjectionResult<LuaType, Stop> {
        panic!("parameter backend not part of atomic call oracle")
    }
    fn function(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &FunctionRef,
    ) -> ProjectionResult<LuaType, Stop> {
        panic!("function backend not part of atomic call oracle")
    }
    fn object(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &ObjectRef,
    ) -> ProjectionResult<LuaType, Stop> {
        panic!("object backend must preserve its carrier")
    }
}
impl SelectedExpressionBackend for Backend {
    fn result_pack(&mut self, slots: Vec<LuaType>) -> LuaType {
        LuaType::Variadic(Arc::new(VariadicType::Multi(slots)))
    }
    fn scalar_result(&mut self, value: LuaType) -> LuaType {
        value.get_result_slot_type(0).unwrap_or(LuaType::Nil)
    }
}

pub fn load(tail: &str) -> (EmmyLuaAnalysis, Provider) {
    let mut analysis = EmmyLuaAnalysis::new();
    let root = std::env::current_dir()
        .unwrap()
        .join("selected-query-logical");
    analysis.add_main_workspace(root.clone());
    let file = analysis.update_files_by_path(vec![(
        root.join("input.lua"),
        Some(format!("-- 界🙂\r\nlocal function f() end\r\n{tail}\r\n")),
    )])[0];
    let model = analysis.semantic_model(file);
    let root = model.syntax_tree().unwrap().get_red_root();
    let call = root
        .descendants()
        .filter_map(LuaCallExpr::cast)
        .find(|call| {
            call.get_prefix_expr()
                .is_some_and(|prefix| prefix.syntax().text().to_string() == "f")
        })
        .unwrap();
    let closure = root.descendants().find_map(LuaClosureExpr::cast).unwrap();
    let query = BindingSite {
        file_id: file,
        syntax: call.get_syntax_id(),
    };
    let callee = BindingSite {
        file_id: file,
        syntax: call.get_prefix_expr().unwrap().get_syntax_id(),
    };
    let function = FunctionRef {
        file_id: file,
        closure: closure.get_syntax_id(),
        declaration: None,
    };
    let provider = Provider {
        query,
        callee,
        call: BindingDecision::Bound(CallPermit {
            callee,
            proof: "permit-proof".into(),
            capability_profile: "typed-call-v1".into(),
        }),
        observation: BindingDecision::Bound(CallObservation {
            callee: ObservedCallee::Source(function.clone()),
            selected_callee_identity: "selected-function".into(),
            closure_identity: "closure-identity".into(),
            invocation_identity: "invocation-identity".into(),
            returns: vec![
                number_slot(),
                BindingDecision::KnownAbsent {
                    proof: "nil-slot-proof".into(),
                },
            ],
            proof: "observation-proof".into(),
        }),
        selected_callee: BindingDecision::Bound(BoundValue {
            identity: "selected-function".into(),
            proof: "callee-proof".into(),
            value: SelectedValue::Function(function),
        }),
        selected_query: BindingDecision::NotManaged,
    };
    (analysis, provider)
}
fn environment() -> EnvironmentCallable {
    EnvironmentCallable {
        origin: EnvironmentOrigin {
            material: "api.json".into(),
            material_sha256: "a".repeat(64),
            pointer: "/functions/0".into(),
            identity: "material-origin".into(),
        },
        declaration: EnvironmentRecord {
            kind: "function".into(),
            name: "f".into(),
            returns: vec![EnvironmentSlot {
                index: 1,
                type_name: "number".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
        types: Arc::new(EnvironmentTypeCatalog::default()),
    }
}
pub fn number_slot() -> BindingDecision<BoundValue> {
    BindingDecision::Bound(BoundValue {
        identity: "number-slot".into(),
        proof: "number-slot-proof".into(),
        value: SelectedValue::EnvironmentReturn(EnvironmentReturn {
            callable: Arc::new(environment()),
            slot: 1,
            invocation_identity: "slot-invocation".into(),
        }),
    })
}
pub struct Case {
    pub name: &'static str,
    pub provider: Provider,
    pub reason: Option<&'static str>,
}
pub fn cases(original: &Provider) -> Vec<Case> {
    let mut cases = vec![];
    for name in [
        "valid",
        "zero-slots",
        "32-slots",
        "33-slots",
        "slot-unmanaged",
        "slot-unavailable",
        "slot-known-absent",
        "permit-absent",
        "permit-unavailable",
        "permit-wrong-callee",
        "permit-wrong-profile",
        "permit-environment-profile",
        "permit-intrinsic-profile",
        "callee-unavailable",
        "callee-identity-mismatch",
        "source-function-mismatch",
        "empty-invocation",
        "empty-source-closure",
        "observation-unavailable",
        "observation-absent",
        "observation-unmanaged",
        "observation-unmanaged-selected-return",
        "call-unmanaged-selected-return",
        "fully-unmanaged",
        "environment-valid",
        "environment-pointer-mismatch",
        "environment-sha-mismatch",
        "environment-identity-mismatch",
        "environment-material-diff",
        "environment-empty-closure",
        "environment-may-return-nothing",
        "environment-nonboolean-may-return",
    ] {
        let mut provider = original.clone();
        let reason = match name {
            "valid" => None,
            "zero-slots" => {
                observation(&mut provider).returns.clear();
                None
            }
            "32-slots" => {
                observation(&mut provider).returns = vec![number_slot(); 32];
                None
            }
            "33-slots" => {
                observation(&mut provider).returns = vec![number_slot(); 33];
                Some("call_observation_identity_or_budget")
            }
            "slot-unmanaged" => {
                observation(&mut provider).returns[0] = BindingDecision::NotManaged;
                Some("observed_return_slot_not_managed")
            }
            "slot-unavailable" => {
                observation(&mut provider).returns[0] = BindingDecision::Unavailable {
                    proof: "slot-unavailable-proof".into(),
                    reason: "slot-missing".into(),
                };
                Some("slot-missing")
            }
            "slot-known-absent" => {
                observation(&mut provider).returns[0] = BindingDecision::KnownAbsent {
                    proof: "absent-slot-proof".into(),
                };
                None
            }
            "permit-absent" => {
                provider.call = BindingDecision::KnownAbsent {
                    proof: "permit-absent-proof".into(),
                };
                None
            }
            "permit-unavailable" => {
                provider.call = BindingDecision::Unavailable {
                    proof: "permit-unavailable-proof".into(),
                    reason: "permit-missing".into(),
                };
                Some("permit-missing")
            }
            "permit-wrong-callee" => {
                permit(&mut provider).callee = original.query;
                Some("call_permit_callee_or_profile_mismatch")
            }
            "permit-wrong-profile" => {
                permit(&mut provider).capability_profile = "unapproved".into();
                Some("call_permit_callee_or_profile_mismatch")
            }
            "permit-environment-profile" => {
                permit(&mut provider).capability_profile =
                    "authenticated_environment_signature_v1".into();
                None
            }
            "permit-intrinsic-profile" => {
                permit(&mut provider).capability_profile = "fixture_intrinsic_value".into();
                None
            }
            "callee-unavailable" => {
                provider.selected_callee = BindingDecision::Unavailable {
                    proof: "callee-missing-proof".into(),
                    reason: "callee-missing".into(),
                };
                Some("call_observation_identity_or_budget")
            }
            "callee-identity-mismatch" => {
                observation(&mut provider).selected_callee_identity = "other-function".into();
                Some("call_observation_identity_or_budget")
            }
            "source-function-mismatch" => {
                if let ObservedCallee::Source(function) = &mut observation(&mut provider).callee {
                    function.closure = original.query.syntax;
                }
                Some("call_observation_identity_or_budget")
            }
            "empty-invocation" => {
                observation(&mut provider).invocation_identity.clear();
                Some("call_observation_identity_or_budget")
            }
            "empty-source-closure" => {
                observation(&mut provider).closure_identity.clear();
                Some("call_observation_identity_or_budget")
            }
            "observation-unavailable" => {
                provider.observation = BindingDecision::Unavailable {
                    proof: "observation-unavailable-proof".into(),
                    reason: "observation-missing".into(),
                };
                Some("observation-missing")
            }
            "observation-absent" => {
                provider.observation = BindingDecision::KnownAbsent {
                    proof: "observation-absent-proof".into(),
                };
                Some("call_return_observation_absent")
            }
            "observation-unmanaged" => {
                provider.observation = BindingDecision::NotManaged;
                Some("call_return_observation_missing")
            }
            "observation-unmanaged-selected-return" => {
                provider.observation = BindingDecision::NotManaged;
                provider.selected_query = number_slot();
                None
            }
            "call-unmanaged-selected-return" => {
                provider.call = BindingDecision::NotManaged;
                provider.selected_query = number_slot();
                None
            }
            "fully-unmanaged" => {
                provider.call = BindingDecision::NotManaged;
                None
            }
            _ => {
                let mut selected = environment();
                if name == "environment-may-return-nothing" {
                    selected
                        .declaration
                        .raw
                        .insert("MayReturnNothing".into(), true.into());
                }
                if name == "environment-nonboolean-may-return" {
                    selected
                        .declaration
                        .raw
                        .insert("MayReturnNothing".into(), "true".into());
                }
                let mut observed = selected.clone();
                match name {
                    "environment-pointer-mismatch" => observed.origin.pointer.push_str("/other"),
                    "environment-sha-mismatch" => observed.origin.material_sha256 = "b".repeat(64),
                    "environment-identity-mismatch" => observed.origin.identity.push_str("other"),
                    "environment-material-diff" => {
                        observed.origin.material = "different-display-name.json".into()
                    }
                    _ => (),
                }
                provider.selected_callee = BindingDecision::Bound(BoundValue {
                    identity: "selected-function".into(),
                    proof: "callee-proof".into(),
                    value: SelectedValue::EnvironmentCallable(Arc::new(selected)),
                });
                observation(&mut provider).callee = ObservedCallee::Environment(Arc::new(observed));
                if name == "environment-empty-closure" {
                    observation(&mut provider).closure_identity.clear();
                }
                match name {
                    "environment-pointer-mismatch"
                    | "environment-sha-mismatch"
                    | "environment-identity-mismatch" => {
                        Some("call_observation_identity_or_budget")
                    }
                    "environment-may-return-nothing" => {
                        Some("environment_return_cardinality_unavailable")
                    }
                    _ => None,
                }
            }
        };
        cases.push(Case {
            name,
            provider,
            reason,
        });
    }
    cases
}
fn observation(provider: &mut Provider) -> &mut CallObservation {
    let BindingDecision::Bound(observation) = &mut provider.observation else {
        panic!("expected observation")
    };
    observation
}
fn permit(provider: &mut Provider) -> &mut CallPermit {
    let BindingDecision::Bound(permit) = &mut provider.call else {
        panic!("expected permit")
    };
    permit
}
pub fn positions() -> [(&'static str, &'static str, bool); 14] {
    [
        ("return-last", "return f()", true),
        ("return-first", "return f(),1", false),
        ("return-after-value", "return 1,f()", true),
        ("local-expanding", "local a,b=f()", true),
        ("local-scalar", "local a=f()", false),
        ("assign-expanding", "a,b=f()", true),
        ("assign-scalar", "a=f()", false),
        ("argument-first", "g(f(),1)", false),
        ("argument-last", "g(1,f())", true),
        ("parentheses", "return (f())", false),
        ("table-first", "return {f(),1}", false),
        ("table-last", "return {1,f()}", true),
        ("table-keyed", "return {key=f()}", false),
        ("generic-for", "for a,b in f() do end", true),
    ]
}
pub fn run(
    analysis: &EmmyLuaAnalysis,
    provider: &Provider,
) -> (Option<LuaType>, Vec<BindingCoverage>, usize) {
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let mut backend = Backend::default();
    let value = SelectedProjector::new(analysis, &session)
        .project_selected(provider.query, &mut backend)
        .unwrap();
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    (value, rows, backend.checkpoints)
}
