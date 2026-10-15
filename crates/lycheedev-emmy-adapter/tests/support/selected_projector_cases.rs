//! Shared oracle inputs. These are static Lua documents, never executed.
#![allow(dead_code)]
use emmylua_code_analysis::{EmmyLuaAnalysis, LuaType, SemanticId};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaForStat};
use lycheedev_emmy_adapter::{ByteRange, session::*};
use std::sync::Arc;

pub struct Case {
    pub name: String,
    pub decision: BindingDecision<BoundValue>,
    pub state: Option<&'static str>,
    pub reason: Option<&'static str>,
}

pub struct Provider;
impl BindingContext for Provider {
    fn identity(&self) -> &str {
        "selected-projector-oracle"
    }
    fn resolve(&self, _: BindingSite) -> BindingDecision<BoundValue> {
        panic!("atomic dispatch does not query an unrelated provider")
    }
}

#[derive(Clone, Debug)]
pub struct Stop {
    pub ordinal: usize,
    pub token: Arc<()>,
}
pub struct AtomicBackend {
    pub checkpoints: usize,
    pub stop_at: usize,
    pub token: Arc<()>,
}
impl Default for AtomicBackend {
    fn default() -> Self {
        Self {
            checkpoints: 0,
            stop_at: usize::MAX,
            token: Arc::new(()),
        }
    }
}
impl SelectedProjectionBackend for AtomicBackend {
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
        panic!("a context-recursive expression requires its owned backend")
    }
    fn parameter(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &SemanticId,
    ) -> ProjectionResult<LuaType, Stop> {
        panic!("a context-recursive parameter requires its owned backend")
    }
    fn function(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &FunctionRef,
    ) -> ProjectionResult<LuaType, Stop> {
        panic!("a context-recursive function requires its owned backend")
    }
    fn object(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &ObjectRef,
    ) -> ProjectionResult<LuaType, Stop> {
        panic!("an object requires a token-preserving owned backend")
    }
}

pub fn load() -> (EmmyLuaAnalysis, BindingSite, BindingSite) {
    let mut analysis = EmmyLuaAnalysis::new();
    let root = std::env::current_dir()
        .unwrap()
        .join("selected-projector-logical");
    analysis.add_main_workspace(root.clone());
    let file = analysis.update_files_by_path(vec![(root.join("input.lua"), Some(
        "-- 界🙂\r\nfor iteration=1,3 do local ignored=iteration end\r\nlocal selected=1\r\nreturn selected\r\n".into()
    ))])[0];
    let model = analysis.semantic_model(file);
    let tree = model.syntax_tree().unwrap();
    let root = tree.get_red_root();
    let statement = root.descendants().find_map(LuaForStat::cast).unwrap();
    let read = root
        .descendants()
        .filter_map(LuaExpr::cast)
        .find(|expr| expr.syntax().text().to_string() == "selected")
        .unwrap();
    let query = BindingSite {
        file_id: file,
        syntax: read.get_syntax_id(),
    };
    let statement = BindingSite {
        file_id: file,
        syntax: statement.get_syntax_id(),
    };
    (analysis, query, statement)
}

pub fn slot(index: usize, name: &str) -> EnvironmentSlot {
    EnvironmentSlot {
        index,
        name: format!("slot{index}"),
        type_name: name.into(),
        ..Default::default()
    }
}

pub fn callable() -> EnvironmentCallable {
    EnvironmentCallable {
        origin: EnvironmentOrigin {
            material: "api.json".into(),
            material_sha256: "a".repeat(64),
            pointer: "/functions/0".into(),
            identity: "material:origin".into(),
        },
        declaration: EnvironmentRecord {
            kind: "function".into(),
            name: "SelectedFunction".into(),
            parameters: vec![slot(1, "number")],
            returns: vec![slot(1, "string")],
            ..Default::default()
        },
        types: Arc::new(EnvironmentTypeCatalog::default()),
    }
}

pub fn bound(value: SelectedValue) -> BindingDecision<BoundValue> {
    BindingDecision::Bound(BoundValue {
        identity: "selected-value".into(),
        proof: "selection-proof".into(),
        value,
    })
}

fn add(
    cases: &mut Vec<Case>,
    name: impl Into<String>,
    value: SelectedValue,
    reason: Option<&'static str>,
) {
    cases.push(Case {
        name: name.into(),
        decision: bound(value),
        state: Some(if reason.is_some() {
            "unavailable"
        } else {
            "bound"
        }),
        reason,
    });
}

pub fn cases(query: BindingSite, statement: BindingSite) -> Vec<Case> {
    let mut cases = vec![
        Case {
            name: "not-managed".into(),
            decision: BindingDecision::NotManaged,
            state: None,
            reason: None,
        },
        Case {
            name: "known-absent".into(),
            decision: BindingDecision::KnownAbsent {
                proof: "absence-proof".into(),
            },
            state: Some("known_absent"),
            reason: None,
        },
        Case {
            name: "unavailable-is-authoritative".into(),
            decision: BindingDecision::Unavailable {
                proof: "unavailable-proof".into(),
                reason: "selected-evidence-missing".into(),
            },
            state: Some("unavailable"),
            reason: Some("selected-evidence-missing"),
        },
    ];
    let number = DerivedNumber {
        value: 2.5,
        origin: SourceSpan {
            file_id: query.file_id,
            range: ByteRange { start: 0, end: 7 },
        },
        occurrence_identity: "iteration-0".into(),
        proof: "iteration-proof".into(),
        rule: DerivedNumberRule::NumericForIteration {
            statement,
            header: vec![query, query],
            iteration: 0,
            predecessor: None,
        },
    };
    add(
        &mut cases,
        "derived-finite",
        SelectedValue::DerivedNumber(number.clone()),
        None,
    );
    for (name, value) in [
        ("nan", f64::NAN),
        ("positive-infinity", f64::INFINITY),
        ("negative-infinity", f64::NEG_INFINITY),
    ] {
        let mut n = number.clone();
        n.value = value;
        add(
            &mut cases,
            format!("derived-{name}"),
            SelectedValue::DerivedNumber(n),
            Some("derived_number_evidence_invalid"),
        );
    }
    for empty in ["occurrence", "proof"] {
        let mut n = number.clone();
        if empty == "occurrence" {
            n.occurrence_identity.clear();
        } else {
            n.proof.clear();
        }
        add(
            &mut cases,
            format!("derived-empty-{empty}"),
            SelectedValue::DerivedNumber(n),
            Some("derived_number_evidence_invalid"),
        );
    }
    for (name, ast, header, iteration, predecessor, valid) in [
        (
            "three-header",
            statement,
            3,
            1,
            Some("prior".to_string()),
            true,
        ),
        ("one-header", statement, 1, 0, None, false),
        ("four-header", statement, 4, 0, None, false),
        ("wrong-ast", query, 2, 0, None, false),
        ("missing-predecessor", statement, 2, 1, None, false),
        (
            "unexpected-predecessor",
            statement,
            2,
            0,
            Some("prior".to_string()),
            false,
        ),
    ] {
        let mut n = number.clone();
        n.rule = DerivedNumberRule::NumericForIteration {
            statement: ast,
            header: vec![query; header],
            iteration,
            predecessor,
        };
        add(
            &mut cases,
            format!("derived-{name}"),
            SelectedValue::DerivedNumber(n),
            (!valid).then_some("derived_number_rule_invalid"),
        );
    }
    for name in [
        "number",
        "integer",
        "string",
        "bool",
        "boolean",
        "nil",
        "any",
        "function",
        "LuaFunction",
        "table",
    ] {
        let mut f = callable();
        f.declaration.parameters[0].type_name = name.into();
        f.declaration.returns[0].type_name = name.into();
        add(
            &mut cases,
            format!("environment-{name}"),
            SelectedValue::EnvironmentCallable(Arc::new(f)),
            None,
        );
    }
    for name in [
        "nilable",
        "zero-returns",
        "multiple-returns",
        "may-return-nothing",
        "nonboolean-may-return",
        "alias",
        "31-aliases",
        "32-aliases",
        "alias-cycle",
        "opaque-nominal",
        "inner-table",
        "inner-number",
        "empty-origin",
        "sha63",
        "sha64-not-hex",
        "wrong-kind",
        "receiver",
        "33-parameters",
        "33-returns",
        "parameter-index",
        "return-index",
    ] {
        let mut f = callable();
        let reason = match name {
            "nilable" => {
                f.declaration.parameters[0].nilable = true;
                f.declaration.returns[0].nilable = true;
                None
            }
            "zero-returns" => {
                f.declaration.returns.clear();
                None
            }
            "multiple-returns" => {
                f.declaration.returns.push(slot(2, "integer"));
                None
            }
            "may-return-nothing" => {
                f.declaration
                    .raw
                    .insert("MayReturnNothing".into(), true.into());
                None
            }
            "nonboolean-may-return" => {
                f.declaration
                    .raw
                    .insert("MayReturnNothing".into(), "true".into());
                None
            }
            "alias" => {
                f.declaration.parameters[0].type_name = "Alias".into();
                Arc::make_mut(&mut f.types)
                    .aliases
                    .insert("Alias".into(), "number".into());
                None
            }
            "31-aliases" | "32-aliases" => {
                let count = if name == "31-aliases" { 31 } else { 32 };
                f.declaration.parameters[0].type_name = "Alias0".into();
                for i in 0..count {
                    Arc::make_mut(&mut f.types).aliases.insert(
                        format!("Alias{i}"),
                        if i + 1 == count {
                            "number".into()
                        } else {
                            format!("Alias{}", i + 1)
                        },
                    );
                }
                (count == 32).then_some("environment_type_depth")
            }
            "alias-cycle" => {
                f.declaration.parameters[0].type_name = "Loop".into();
                Arc::make_mut(&mut f.types)
                    .aliases
                    .insert("Loop".into(), "Loop".into());
                Some("environment_type_depth")
            }
            "opaque-nominal" => {
                f.declaration.parameters[0].type_name = "Frame".into();
                Some("environment_type_unavailable")
            }
            "inner-table" => {
                f.declaration.parameters[0].type_name = "table".into();
                f.declaration.parameters[0].inner_type = "string".into();
                Some("environment_type_unavailable")
            }
            "inner-number" => {
                f.declaration.parameters[0].inner_type = "ignored-by-fixed-baseline".into();
                None
            }
            "empty-origin" => {
                f.origin.identity.clear();
                Some("environment_callable_identity_invalid")
            }
            "sha63" => {
                f.origin.material_sha256 = "a".repeat(63);
                Some("environment_callable_identity_invalid")
            }
            "sha64-not-hex" => {
                f.origin.material_sha256 = "z".repeat(64);
                None
            }
            "wrong-kind" => {
                f.declaration.kind = "event".into();
                Some("environment_callable_identity_invalid")
            }
            "receiver" => {
                f.declaration.owner = Some(EnvironmentOwner {
                    kind: "script-object".into(),
                    name: "Frame".into(),
                    source: serde_json::Value::Null,
                });
                Some("environment_receiver_unavailable")
            }
            "33-parameters" => {
                f.declaration.parameters = (1..=33).map(|i| slot(i, "number")).collect();
                Some("environment_signature_budget")
            }
            "33-returns" => {
                f.declaration.returns = (1..=33).map(|i| slot(i, "number")).collect();
                Some("environment_signature_budget")
            }
            "parameter-index" => {
                f.declaration.parameters[0].index = 2;
                Some("environment_parameter_index_invalid")
            }
            "return-index" => {
                f.declaration.returns[0].index = 2;
                Some("environment_return_index_invalid")
            }
            _ => unreachable!(),
        };
        add(
            &mut cases,
            format!("environment-{name}"),
            SelectedValue::EnvironmentCallable(Arc::new(f)),
            reason,
        );
    }
    for name in [
        "valid",
        "nilable",
        "slot-zero",
        "slot-missing",
        "invocation-empty",
        "wrong-slot-index",
        "origin-empty",
        "sha63",
        "unknown-type",
        "alias-cycle",
        "independent-return-validation",
    ] {
        let mut f = callable();
        let mut index = 1;
        let mut invocation = "invocation".to_string();
        let reason = match name {
            "valid" => None,
            "nilable" => {
                f.declaration.returns[0].nilable = true;
                None
            }
            "slot-zero" => {
                index = 0;
                Some("environment_return_identity_invalid")
            }
            "slot-missing" => {
                index = 2;
                Some("environment_return_slot_unavailable")
            }
            "invocation-empty" => {
                invocation.clear();
                Some("environment_return_identity_invalid")
            }
            "wrong-slot-index" => {
                f.declaration.returns[0].index = 2;
                Some("environment_return_slot_or_origin_invalid")
            }
            "origin-empty" => {
                f.origin.identity.clear();
                Some("environment_return_slot_or_origin_invalid")
            }
            "sha63" => {
                f.origin.material_sha256 = "a".repeat(63);
                Some("environment_return_slot_or_origin_invalid")
            }
            "unknown-type" => {
                f.declaration.returns[0].type_name = "Frame".into();
                Some("environment_type_unavailable")
            }
            "alias-cycle" => {
                f.declaration.returns[0].type_name = "Loop".into();
                Arc::make_mut(&mut f.types)
                    .aliases
                    .insert("Loop".into(), "Loop".into());
                Some("environment_type_depth")
            }
            "independent-return-validation" => {
                f.declaration.kind = "not-checked-here".into();
                f.declaration.parameters[0].index = 88;
                None
            }
            _ => unreachable!(),
        };
        add(
            &mut cases,
            format!("return-{name}"),
            SelectedValue::EnvironmentReturn(EnvironmentReturn {
                callable: Arc::new(f),
                slot: index,
                invocation_identity: invocation,
            }),
            reason,
        );
    }
    cases
}

pub fn assert_case(
    case: &Case,
    value: &Option<LuaType>,
    rows: &[BindingCoverage],
    query: BindingSite,
) {
    match case.state {
        None => {
            assert!(value.is_none(), "{}", case.name);
            assert!(rows.is_empty(), "{}", case.name);
        }
        Some(state) => {
            assert_eq!(rows.len(), 1, "{}", case.name);
            let row = &rows[0];
            assert_eq!(row.site, query, "{}", case.name);
            assert_eq!(row.operation, "resolve");
            assert_eq!(row.state, state, "{}", case.name);
            assert_eq!(row.reason.as_deref(), case.reason, "{}", case.name);
            let proof = match &case.decision {
                BindingDecision::KnownAbsent { proof }
                | BindingDecision::Unavailable { proof, .. } => proof,
                BindingDecision::Bound(bound) => &bound.proof,
                BindingDecision::NotManaged => unreachable!(),
            };
            assert_eq!(&row.proof, proof);
            assert_eq!(row.parameter, None);
            assert_eq!(row.parameter_index, None);
            assert_eq!(row.argument, None);
            if state == "unavailable" {
                assert_eq!(value, &Some(LuaType::Unknown), "{}", case.name);
            }
            if state == "known_absent" {
                assert_eq!(value, &Some(LuaType::Nil), "{}", case.name);
            }
        }
    }
}
