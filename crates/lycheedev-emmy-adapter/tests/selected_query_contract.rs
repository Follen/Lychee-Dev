#[path = "support/selected_query_cases.rs"]
mod support;
use emmylua_code_analysis::{EmmyLuaAnalysis, LuaType, SemanticId, VariadicType};
use emmylua_parser::{LuaAstNode, LuaExpr};
use lycheedev_emmy_adapter::session::*;
use std::{collections::HashMap, sync::Arc};
use support::*;

#[test]
fn observed_call_identity_denial_and_result_pack_boundaries() {
    for (position, source, expands) in positions() {
        let (analysis, provider) = load(source);
        for case in cases(&provider) {
            let (value, rows, _) = run(&analysis, &case.provider);
            if let Some(reason) = case.reason {
                assert!(
                    rows.iter().any(|row| row.reason.as_deref() == Some(reason)),
                    "{position}/{}: {rows:?}",
                    case.name
                );
            }
            if case.name == "valid" {
                let expected = if expands {
                    LuaType::Variadic(Arc::new(VariadicType::Multi(vec![
                        LuaType::Number,
                        LuaType::Nil,
                    ])))
                } else {
                    LuaType::Number
                };
                assert_eq!(value, Some(expected), "{position}");
                assert_eq!(
                    rows.iter()
                        .map(|row| (row.operation, row.state, row.proof.as_str()))
                        .collect::<Vec<_>>(),
                    vec![
                        ("call_return", "bound", "number-slot-proof"),
                        ("call_return", "known_absent", "nil-slot-proof"),
                        ("call", "bound", "permit-proof")
                    ]
                );
            }
            if case.name == "zero-slots" {
                assert_eq!(
                    value,
                    Some(if expands {
                        LuaType::Variadic(Arc::new(VariadicType::Multi(vec![])))
                    } else {
                        LuaType::Nil
                    }),
                    "{position}"
                );
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].operation, "call");
                assert_eq!(rows[0].state, "bound");
            }
            if case.name == "fully-unmanaged" {
                assert!(value.is_none());
                assert!(rows.is_empty());
            }
            if case.name == "permit-absent" {
                assert_eq!(value, Some(LuaType::Unknown));
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].operation, "call");
                assert_eq!(rows[0].state, "known_absent");
            }
            if case.name == "environment-may-return-nothing" {
                assert_eq!(
                    value,
                    Some(if expands {
                        LuaType::Variadic(Arc::new(VariadicType::Base(LuaType::Unknown)))
                    } else {
                        LuaType::Unknown
                    })
                );
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].operation, "call_return");
                assert_eq!(rows[0].state, "unavailable");
            }
            for row in &rows {
                assert_eq!(row.site, provider.query);
                assert_eq!(row.parameter, None);
                assert_eq!(row.parameter_index, None);
                assert_eq!(row.argument, None);
            }
            println!(
                "SELECTED_QUERY_PUBLIC {}",
                serde_json::json!({"position":position,"case":case.name,"type":format!("{value:?}"),"coverage":format!("{rows:?}")})
            );
        }
    }
}

#[test]
fn cancellation_keeps_exact_completed_prefix_and_releases_query_guard() {
    for (_, source, _) in [positions()[0], positions()[4]] {
        let (analysis, provider) = load(source);
        for case in cases(&provider) {
            let (expected, full_rows, count) = run(&analysis, &case.provider);
            for ordinal in 1..=count {
                let session =
                    OwnedBindingSession::new_checked(Arc::new(case.provider.clone()), || {
                        Ok::<_, Stop>(())
                    })
                    .unwrap();
                let projector = SelectedProjector::new(&analysis, &session);
                let token = Arc::new(());
                let mut interrupted = Backend {
                    checkpoints: 0,
                    stop_at: ordinal,
                    token: token.clone(),
                };
                let error = projector
                    .project_selected(provider.query, &mut interrupted)
                    .unwrap_err();
                assert_eq!(error.ordinal, ordinal);
                assert!(Arc::ptr_eq(&error.token, &token));
                let prefix = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
                assert_eq!(
                    prefix,
                    full_rows[..prefix.len()],
                    "{} ordinal {ordinal}",
                    case.name
                );
                let value = projector
                    .project_selected(provider.query, &mut Backend::default())
                    .unwrap();
                assert_eq!(
                    value, expected,
                    "guard not released: {} ordinal {ordinal}",
                    case.name
                );
                assert_eq!(
                    session.coverage_checked(|| Ok::<_, Stop>(())).unwrap(),
                    full_rows
                );
            }
        }
    }
}

struct Chain {
    values: HashMap<BindingSite, BindingDecision<BoundValue>>,
}
impl BindingContext for Chain {
    fn identity(&self) -> &str {
        "cross-file-chain"
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        self.values
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
}
struct RecursiveBackend;
impl SelectedProjectionBackend for RecursiveBackend {
    type Value = LuaType;
    type Error = Stop;
    fn checkpoint(&mut self) -> Result<(), Stop> {
        Ok(())
    }
    fn plain(&mut self, ty: LuaType) -> LuaType {
        ty
    }
    fn expression(
        &mut self,
        projector: &SelectedProjector<'_>,
        _: BindingSite,
        origin: BindingSite,
    ) -> ProjectionResult<LuaType, Stop> {
        // The test provides only managed name expressions. This is not a raw
        // ordinary-inference fallback or the unfinished full production backend.
        let model = projector.analysis().semantic_model(origin.file_id);
        let valid = model
            .syntax_tree()
            .and_then(|tree| origin.syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaExpr::cast)
            .is_some();
        if !valid {
            return Ok(Err("selected_expression_missing"));
        }
        Ok(Ok(projector
            .project_selected(origin, self)?
            .expect("all chain nodes are selected")))
    }
    fn parameter(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &SemanticId,
    ) -> ProjectionResult<LuaType, Stop> {
        unreachable!()
    }
    fn function(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &FunctionRef,
    ) -> ProjectionResult<LuaType, Stop> {
        unreachable!()
    }
    fn object(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &ObjectRef,
    ) -> ProjectionResult<LuaType, Stop> {
        unreachable!()
    }
}
impl SelectedExpressionBackend for RecursiveBackend {
    fn result_pack(&mut self, slots: Vec<LuaType>) -> LuaType {
        LuaType::Variadic(Arc::new(VariadicType::Multi(slots)))
    }
    fn scalar_result(&mut self, value: LuaType) -> LuaType {
        value.get_result_slot_type(0).unwrap_or(LuaType::Nil)
    }
}
fn chain(count: usize, cycle: bool) -> (EmmyLuaAnalysis, Vec<BindingSite>, Chain) {
    let mut analysis = EmmyLuaAnalysis::new();
    let root = std::env::current_dir()
        .unwrap()
        .join("query-cross-file-chain");
    // A single database and two files, with the selected chain alternating
    // between file identities at every hop.
    let files = analysis.update_files_by_path(
        (0..2)
            .map(|file| {
                (
                    root.join(format!("{file}.lua")),
                    Some(format!(
                        "return {}",
                        (0..count)
                            .filter(|index| index % 2 == file)
                            .map(|index| format!("value{index}"))
                            .collect::<Vec<_>>()
                            .join(",")
                    )),
                )
            })
            .collect(),
    );
    let mut by_name = HashMap::new();
    for file in files {
        let model = analysis.semantic_model(file);
        for expr in model
            .syntax_tree()
            .unwrap()
            .get_red_root()
            .descendants()
            .filter_map(LuaExpr::cast)
        {
            let name = expr.syntax().text().to_string();
            if name.starts_with("value") {
                by_name.insert(
                    name,
                    BindingSite {
                        file_id: file,
                        syntax: expr.get_syntax_id(),
                    },
                );
            }
        }
    }
    let sites = (0..count)
        .map(|i| by_name[&format!("value{i}")])
        .collect::<Vec<_>>();
    let values = sites
        .iter()
        .enumerate()
        .map(|(index, site)| {
            let value = if index + 1 < count || cycle {
                BindingDecision::Bound(BoundValue {
                    identity: format!("node-{index}"),
                    proof: format!("proof-{index}"),
                    value: SelectedValue::Expression(sites[(index + 1) % count]),
                })
            } else {
                BindingDecision::KnownAbsent {
                    proof: "last-proof".into(),
                }
            };
            (*site, value)
        })
        .collect();
    (analysis, sites, Chain { values })
}

#[test]
fn cross_file_recursive_guard_preserves_64_limit_cycle_reason_and_unwind_order() {
    for (count, cycle) in [(2, true), (64, false), (65, false)] {
        let (analysis, sites, provider) = chain(count, cycle);
        let session =
            OwnedBindingSession::new_checked(Arc::new(provider), || Ok::<_, Stop>(())).unwrap();
        let value = SelectedProjector::new(&analysis, &session)
            .project_selected(sites[0], &mut RecursiveBackend)
            .unwrap();
        assert_eq!(
            value,
            Some(if count == 64 {
                LuaType::Nil
            } else {
                LuaType::Unknown
            })
        );
        let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
        let expected_first = if cycle { sites[0] } else { sites[count - 1] };
        assert_eq!(rows[0].site, expected_first);
        if count == 64 {
            assert_eq!(rows[0].state, "known_absent");
            assert_eq!(rows.len(), 64);
        } else {
            assert_eq!(rows[0].reason.as_deref(), Some("binding_projection_cycle"));
            assert_eq!(rows.len(), if cycle { 3 } else { 65 });
        }
        for (offset, row) in rows.iter().skip(1).enumerate() {
            let index = if cycle {
                count - 1 - offset
            } else {
                count - 2 - offset
            };
            assert_eq!(row.site, sites[index]);
            assert_eq!(row.state, "bound");
            assert_eq!(row.proof, format!("proof-{index}"));
        }
        // A repeated completed query reuses no stranded active keys and adds no
        // duplicate coverage rows; no global query cache was introduced.
        let repeated = SelectedProjector::new(&analysis, &session)
            .project_selected(sites[0], &mut RecursiveBackend)
            .unwrap();
        assert_eq!(repeated, value);
        assert_eq!(
            session.coverage_checked(|| Ok::<_, Stop>(())).unwrap(),
            rows
        );
    }
}

struct ObjectCarrier {
    session: Arc<OwnedBindingSession>,
    object: ObjectRef,
}
enum Carrier {
    Plain(LuaType),
    Object(Arc<ObjectCarrier>),
    Pack(Vec<Carrier>),
}
struct CarrierBackend {
    object: Arc<ObjectCarrier>,
}
impl SelectedProjectionBackend for CarrierBackend {
    type Value = Carrier;
    type Error = Stop;
    fn checkpoint(&mut self) -> Result<(), Stop> {
        Ok(())
    }
    fn plain(&mut self, value: LuaType) -> Carrier {
        Carrier::Plain(value)
    }
    fn expression(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: BindingSite,
    ) -> ProjectionResult<Carrier, Stop> {
        unreachable!()
    }
    fn parameter(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &SemanticId,
    ) -> ProjectionResult<Carrier, Stop> {
        unreachable!()
    }
    fn function(
        &mut self,
        _: &SelectedProjector<'_>,
        _: BindingSite,
        _: &FunctionRef,
    ) -> ProjectionResult<Carrier, Stop> {
        unreachable!()
    }
    fn object(
        &mut self,
        projector: &SelectedProjector<'_>,
        _: BindingSite,
        object: &ObjectRef,
    ) -> ProjectionResult<Carrier, Stop> {
        assert!(projector.session().same_session(&self.object.session));
        assert_eq!(object, &self.object.object);
        Ok(Ok(Carrier::Object(self.object.clone())))
    }
}
impl SelectedExpressionBackend for CarrierBackend {
    fn result_pack(&mut self, slots: Vec<Carrier>) -> Carrier {
        Carrier::Pack(slots)
    }
    fn scalar_result(&mut self, value: Carrier) -> Carrier {
        match value {
            Carrier::Pack(slots) => slots
                .into_iter()
                .next()
                .unwrap_or(Carrier::Plain(LuaType::Nil)),
            value => value,
        }
    }
}

#[test]
fn observed_pack_and_scalar_keep_same_session_object_identity_and_epoch_carrier() {
    for (_, source, expands) in [positions()[0], positions()[4]] {
        let (analysis, mut provider) = load(source);
        let object = ObjectRef {
            identity: "observed-object".into(),
            epoch: "epoch-9".into(),
            origin: ObjectOrigin::Lua(provider.callee),
            view: ObjectViewKey {
                context: "selected-query-oracle".into(),
                observation: 7,
                object_id: 3,
            },
        };
        let BindingDecision::Bound(observation) = &mut provider.observation else {
            unreachable!()
        };
        observation.returns[0] = BindingDecision::Bound(BoundValue {
            identity: object.identity.clone(),
            proof: "object-slot-proof".into(),
            value: SelectedValue::Object(object.clone()),
        });
        let session =
            OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(()))
                .unwrap();
        let marker = Arc::new(ObjectCarrier {
            session: session.clone(),
            object,
        });
        let mut backend = CarrierBackend {
            object: marker.clone(),
        };
        let value = SelectedProjector::new(&analysis, &session)
            .project_selected(provider.query, &mut backend)
            .unwrap()
            .unwrap();
        let value = if expands {
            let Carrier::Pack(mut slots) = value else {
                panic!("expected owned pack")
            };
            assert_eq!(slots.len(), 2);
            assert!(matches!(slots.pop(), Some(Carrier::Plain(LuaType::Nil))));
            slots.pop().unwrap()
        } else {
            value
        };
        let Carrier::Object(actual) = value else {
            panic!("owned object carrier erased")
        };
        assert!(Arc::ptr_eq(&actual, &marker));
        assert!(actual.session.same_session(&session));
        assert_eq!(actual.object.identity, "observed-object");
        assert_eq!(actual.object.epoch, "epoch-9");
        let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
        assert_eq!(rows[0].proof, "object-slot-proof");
        assert_eq!(rows.last().unwrap().operation, "call");
        // This tests carrier transport only. Full object shape/origin validation
        // is still a mandatory implementation of the unfinished owned backend.
    }
}

#[test]
fn actual_call_producers_retain_earlier_completion_before_coverage_sentinel() {
    let (analysis, provider) = load("return f()");
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let projector = SelectedProjector::new(&analysis, &session);
    let mut backend = Backend::default();
    for index in 0..16_380 {
        projector
            .project_decision(
                provider.callee,
                BindingDecision::KnownAbsent {
                    proof: format!("earlier-{index}"),
                },
                "resolve",
                &mut backend,
            )
            .unwrap();
    }
    projector
        .project_selected(provider.query, &mut backend)
        .unwrap();
    projector
        .project_decision(
            provider.callee,
            BindingDecision::KnownAbsent {
                proof: "next-producer".into(),
            },
            "resolve",
            &mut backend,
        )
        .unwrap();
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(rows.len(), 16_384);
    assert_eq!(rows[16_380].proof, "number-slot-proof");
    assert_eq!(rows[16_381].proof, "nil-slot-proof");
    assert_eq!(rows[16_382].operation, "call");
    assert_eq!(rows[16_382].state, "bound");
    assert_eq!(rows[16_382].proof, "permit-proof");
    assert_eq!(
        rows[16_383],
        BindingCoverage {
            site: provider.callee,
            operation: "coverage",
            state: "unavailable",
            proof: String::new(),
            reason: Some("binding_coverage_budget".into()),
            parameter: None,
            parameter_index: None,
            argument: None,
        }
    );
    // Ledger capacity is not a query-stop condition.
    let repeated = projector
        .project_selected(provider.query, &mut backend)
        .unwrap();
    assert_eq!(
        repeated,
        Some(LuaType::Variadic(Arc::new(VariadicType::Multi(vec![
            LuaType::Number,
            LuaType::Nil
        ]))))
    );
}
