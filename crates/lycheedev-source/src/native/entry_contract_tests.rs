//! Original host and explicit-entry fixtures exercised through production native APIs.
use super::*;
use emmylua_code_analysis::{EmmyrcLuaVersion, LuaType};
use lycheedev_emmy_adapter::session::{OwnedBindingSession, OwnedQuery};
use std::time::{Duration, Instant};

const DATA: &str =
    include_str!("../../../../tests/fixtures/source-native/host-facts-materials.json");
const FILES: &[(&str, &str, &str, bool)] = &[
    (
        "direct",
        "host-direct.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-direct.lua"),
        false,
    ),
    (
        "shadow",
        "host-shadow.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-shadow.lua"),
        false,
    ),
    (
        "colon",
        "host-colon.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-colon.lua"),
        false,
    ),
    (
        "slots",
        "host-slots.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-slots.lua"),
        false,
    ),
    (
        "transfers",
        "host-transfers.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-transfers.lua"),
        false,
    ),
    (
        "phi",
        "host-phi.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-phi.lua"),
        false,
    ),
    (
        "open-arity",
        "host-open-arity.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-open-arity.lua"),
        true,
    ),
    (
        "unknown-effects",
        "host-unknown.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-unknown.lua"),
        false,
    ),
    (
        "capture-mutation",
        "host-capture-mutation.lua",
        include_str!(
            "../../../../tests/fixtures/source-native/wow/native/host-capture-mutation.lua"
        ),
        false,
    ),
    (
        "cardinality",
        "host-cardinality.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-cardinality.lua"),
        false,
    ),
    (
        "return-join",
        "host-return-join.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-return-join.lua"),
        false,
    ),
    (
        "unknown-tail",
        "host-unknown-tail.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-unknown-tail.lua"),
        false,
    ),
];
const ENTRY_SOURCE: &str =
    include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry.lua");
const AMBIGUOUS_SOURCE: &str =
    include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry-ambiguous.lua");
const ORDER_SOURCES: &[(&str, &str)] = &[
    (
        "host-order-a.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-order-a.lua"),
    ),
    (
        "host-order-b.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-order-b.lua"),
    ),
    (
        "host-order-main.lua",
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-order-main.lua"),
    ),
];
const DEPTH_SOURCE: &str =
    include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry-depth.lua");
const DEPTH_CALL: &str =
    include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry-depth-call.lua");
const CONDITIONAL_ENTRY: &str =
    include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry-conditional.lua");
const UNREACHABLE_ENTRY: &str =
    include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry-unreachable.lua");
const VARARG_ENTRY: &str =
    include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry-vararg.lua");
fn entry_control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn reference(value: &Value) -> HostMaterialRef {
    HostMaterialRef {
        material: value["material"].as_str().unwrap().into(),
        pointer: value["pointer"].as_str().unwrap().into(),
    }
}
fn prepare_case(
    file: &str,
    source: &str,
    open: bool,
    control: &OperationControl,
) -> PreparedSnapshot {
    prepare_files(&[(file, source)], open, control)
}
// Admit original fixture bytes and authenticated material through production.
fn prepare_files(
    sources: &[(&str, &str)],
    open: bool,
    control: &OperationControl,
) -> PreparedSnapshot {
    let data: Value = serde_json::from_str(DATA).unwrap();
    let materials = data["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| HostMaterialInput {
            name: row["name"].as_str().unwrap().into(),
            bytes: row["bytes"].as_str().unwrap().as_bytes().to_vec(),
            expected_sha256: row["sha256"].as_str().unwrap().into(),
        })
        .collect();
    let bindings = data["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let mut declaration = reference(&row["declaration"]);
            if open {
                declaration.material = "fixed-host-open-arity-v1".into();
            }
            match row["kind"].as_str().unwrap() {
                "callable" => HostBindingInput::Callable {
                    declaration,
                    rules: Some(reference(&row["rules"])),
                },
                "predicate" => HostBindingInput::Predicate { declaration },
                "empty_object" => HostBindingInput::EmptyObject { declaration },
                kind => panic!("unexpected fixture binding {kind}"),
            }
        })
        .collect();
    let host = TrustedHostEnvironment::from_host_material(
        "fixed-host-common-facts-probe".into(),
        false,
        materials,
        vec![],
        control,
    )
    .unwrap()
    .with_bindings(bindings, control)
    .unwrap();
    let mut config = Emmyrc::default();
    config.runtime.version = EmmyrcLuaVersion::Lua51;
    let inputs = SourceInputs::admit(
        SourceInputSpec {
            files: sources
                .iter()
                .map(|(path, text)| SourceFileInput {
                    path: (*path).into(),
                    original_utf8: (*text).into(),
                    analysis_utf8: (*text).into(),
                    content_sha256: hash(text.as_bytes()),
                    map: SourceMap::Identity,
                })
                .collect(),
            workspace_root: PathBuf::from("C:/lycheedev-source-host-execution-probe"),
            source_identity: json!({"kind":"fixed-host-common-facts-probe"}),
            phase: "after_load".into(),
            ordered_occurrences: sources.iter().map(|(file, _)| (*file).into()).collect(),
            config,
            host,
            luac: LuacIdentity {
                version: "5.1.5".into(),
                platform: "windows-amd64".into(),
                executable_sha256:
                    "64cd0d82734a743ccbffe3a2d28727ce8e48c8cf1452dd69066f2c56643425f7".into(),
                executable_bytes: 487424,
            },
            limits: SourceLimits::default(),
        },
        control,
    )
    .unwrap();
    prepare(inputs, control).unwrap()
}
fn unique_entry(
    prepared: &PreparedSnapshot,
    file: &str,
    line: usize,
    control: &OperationControl,
) -> EntryCandidate {
    match prepared.entry_candidates(file, line, control).unwrap() {
        EntryResolution::Unique { candidate } => candidate,
        other => panic!("unique entry candidate expected: {other:?}"),
    }
}
fn entry_request(candidate: &EntryCandidate, arguments: EntryArguments) -> EntryRequest {
    EntryRequest {
        candidate: candidate.id.clone(),
        load_occurrence: None,
        creation_context: None,
        arguments,
    }
}
#[test]
fn host_propagation_return_join_and_query_types_use_original_materials() {
    for (name, file, source, open) in FILES {
        let c = OperationControl::uncancelable(Instant::now() + Duration::from_secs(30));
        let prepared = prepare_case(file, source, *open, &c);
        let graph = prepared
            .build_plan(
                name,
                "after_load",
                LoadPlan::Known(vec![(*file).into()]),
                None,
                Some("host-common-facts-v1"),
                &c,
            )
            .unwrap();
        let native = graph
            .native_invocations
            .as_ref()
            .expect("native invocation facts");
        let host = graph.host_runtime.as_ref().expect("host common facts");
        assert!(native.control.complete, "{name}:complete_bounded_analysis");
        assert_eq!(
            json!(
                host.bindings
                    .iter()
                    .filter(|binding| graph.values[binding.value].origin.environment()
                        == Some(binding.declaration.declaration()))
                    .count()
            ),
            json!(7),
            "{}",
            &format!("{name}:initial_environment_origins")
        );
        assert_eq!(
            json!(host.calls.len()),
            json!(graph.calls.len()),
            "{}",
            &format!("{name}:all_call_slots_recorded")
        );
        assert_eq!(
            json!(host.capture_references.len()),
            json!(native.capture_facts.len()),
            "{}",
            &format!("{name}:capture_reference_count")
        );
        assert!(
            host.transfers
                .iter()
                .all(|edge| edge.kind != "capture_cell_reference"),
            "{name}:captures_not_value_transfers"
        );
        let mut values = std::collections::BTreeSet::new();
        let mut fresh = true;
        for call in &host.calls {
            if let Some(binding) = call.host_binding {
                for slot in &call.returns {
                    let Decision::Bound(value) = slot.selected else {
                        fresh = false;
                        continue;
                    };
                    fresh &= values.insert(value)
                        && matches!(graph.values[value].kind,ValueKind::HostReturn{binding:b,call:c,slot:s} if b==binding&&c==call.call&&s==slot.slot);
                }
            }
        }
        assert!(fresh, "{name}:fresh_host_return_slots");
        assert!(graph.uses.iter().filter(|fact|matches!(fact.decision,Decision::Bound(value) if matches!(graph.values[value].kind,ValueKind::HostCallable(_)|ValueKind::HostReturn{..}))).all(|fact|Facade{graph:&graph}.type_view(fact)["state"]=="unavailable"), "{name}:host_type_unavailable");
        if !matches!(*name, "unknown-effects" | "unknown-tail") {
            assert_eq!(
                json!(native.escape_events.len()),
                json!(0),
                "{}",
                &format!("{name}:nonretaining_host_calls")
            );
        }
        if *name == "capture-mutation" {
            let call = graph
                .calls
                .iter()
                .find(|call| {
                    call.site
                        .text(prepared.inputs())
                        .is_ok_and(|text| text == "f()")
                })
                .expect("capture mutation invocation");
            let returned = &graph.uses[call.result_fact.expect("capture mutation result")];
            let nil = |decision: &Decision| {
                matches!(decision, Decision::KnownAbsent(_))
                    || matches!(decision,Decision::Bound(value) if matches!(&graph.values[*value].kind,ValueKind::Primitive(kind) if kind=="nil"))
            };
            assert!(
                nil(&returned.decision),
                "capture-mutation:invocation_returns_current_nil"
            );
            let reference = host
                .capture_references
                .first()
                .expect("captured cell reference");
            let frame = native
                .frames
                .iter()
                .find(|frame| frame.call == Some(call.id))
                .expect("capture mutation frame");
            let read = frame
                .return_facts
                .first()
                .map(|fact| &graph.uses[*fact])
                .expect("capture return read");
            let latest = graph
                .writes
                .iter()
                .rev()
                .find(|write| write.cell == reference.cell)
                .expect("capture current write");
            assert!(
                read.writes == vec![latest.id] && nil(&latest.decision),
                "capture-mutation:read_uses_latest_cell_write"
            );
            let consumer = host
                .calls
                .iter()
                .find(|call| {
                    call.host_binding
                        .is_some_and(|binding| host.bindings[binding].declaration.name() == "Use")
                })
                .expect("capture consumer");
            assert!(
                consumer
                    .arguments
                    .first()
                    .is_some_and(|slot| nil(&slot.selected)),
                "capture-mutation:consumer_receives_nil"
            );
        }
        if *name == "transfers" {
            let cells = host
                .capture_references
                .iter()
                .map(|reference| reference.cell.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                json!(cells.len()),
                json!(2),
                "{}",
                "transfers:factory_capture_cells_distinct"
            );
            let mut selected = Vec::new();
            for expression in ["a()", "b()"] {
                let call = graph
                    .calls
                    .iter()
                    .find(|call| {
                        call.site
                            .text(prepared.inputs())
                            .is_ok_and(|text| text == expression)
                    })
                    .expect("factory closure call");
                let result = &graph.uses[call.result_fact.expect("factory result")].decision;
                selected.push(match result {
                    Decision::Bound(value) => match graph.values[*value].kind {
                        ValueKind::HostReturn { binding, slot, .. } => {
                            json!({"host":host.bindings[binding].declaration.name(),"slot":slot})
                        }
                        _ => Value::Null,
                    },
                    _ => Value::Null,
                });
            }
            assert_eq!(
                json!(selected),
                json!([{"host":"Source","slot":1},{"host":"Pair","slot":1}]),
                "{}",
                "transfers:factory_invocations_read_own_values"
            );
        }
        if *name == "cardinality" {
            let variable = host
                .calls
                .iter()
                .filter(|call| {
                    graph.calls[call.call]
                        .site
                        .text(prepared.inputs())
                        .is_ok_and(|text| text == "Variable()")
                })
                .collect::<Vec<_>>();
            assert!(
                variable.len() == 2
                    && variable.iter().all(|call| call.return_arity.is_none()
                        && call.return_arities == Some(vec![0, 2])
                        && call.return_open_tail.is_none()
                        && call.returns.iter().all(|slot| slot.presence == "unknown")),
                "cardinality:bounded_zero_or_two_returns"
            );
            let outer = host
                .calls
                .iter()
                .filter(|call| {
                    call.host_binding.is_some_and(|binding| {
                        host.bindings[binding].declaration.name() == "UseSecond"
                    })
                })
                .map(|call| call.argument_arities.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                json!(outer),
                json!([[0, 2], [1]]),
                "{}",
                "cardinality:bare_and_parenthesized_arguments"
            );
        }
        if *name == "return-join" {
            let choose = host
                .calls
                .iter()
                .find(|call| {
                    graph.calls[call.call]
                        .site
                        .text(prepared.inputs())
                        .is_ok_and(|text| text == "Choose()")
                })
                .expect("return join call");
            assert_eq!(
                json!(choose.return_arities),
                json!([1, 2]),
                "{}",
                "return-join:bounded_one_or_two_returns"
            );
            let slot = choose.returns.first().expect("joined slot");
            let edge=host.transfers.iter().find(|edge|matches!(edge.destination,TransferDestination::Use{use_fact} if use_fact==slot.use_fact)&&edge.kind=="conditional_return_candidates").expect("conditional return candidate edge");
            let candidates = edge
                .return_candidates
                .as_ref()
                .expect("conditional return candidates");
            assert!(candidates.len()==2&&edge.source_uses.len()==2&&candidates.iter().all(|candidate|candidate.use_fact.is_some()&&candidate.paths.iter().any(|path|!path.is_empty())&&matches!(candidate.selected,Decision::Bound(value) if matches!(graph.values[value].kind,ValueKind::HostReturn{slot:1,..}))), "return-join:first_slot_preserves_both_host_sources");
            let polarities = candidates
                .iter()
                .flat_map(|candidate| {
                    candidate
                        .paths
                        .iter()
                        .flatten()
                        .map(|predicate| predicate.outcome)
                })
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(
                json!(polarities),
                json!([false, true]),
                "{}",
                "return-join:candidate_path_polarities"
            );
        }
        if *name == "unknown-tail" {
            let outer = host
                .calls
                .iter()
                .filter(|call| {
                    call.host_binding
                        .is_some_and(|binding| host.bindings[binding].declaration.name() == "Use")
                })
                .collect::<Vec<_>>();
            assert_eq!(
                json!(
                    outer
                        .iter()
                        .map(|call| call.argument_arities.clone())
                        .collect::<Vec<_>>()
                ),
                json!([null, [1], [2]]),
                "{}",
                "unknown-tail:bare_parenthesis_relay_arities"
            );
            let relay = host
                .calls
                .iter()
                .find(|call| {
                    graph.calls[call.call]
                        .site
                        .text(prepared.inputs())
                        .is_ok_and(|text| text == "Relay(UnknownCall())")
                })
                .expect("unknown tail relay");
            assert!(
                relay.argument_open_tail.is_some() && relay.argument_arities.is_none(),
                "unknown-tail:relay_argument_tail_preserved"
            );
            assert!(
                graph.calls[relay.call]
                    .parameter_bindings
                    .get(1)
                    .is_some_and(|parameter| matches!(
                        parameter.selected_value,
                        Decision::Unavailable(_)
                    )),
                "unknown-tail:relay_missing_formal_unavailable"
            );
            assert!(
                relay.return_arities == Some(vec![2])
                    && relay.returns.len() == 2
                    && relay
                        .returns
                        .iter()
                        .all(|slot| matches!(slot.selected, Decision::Unavailable(_))),
                "unknown-tail:relay_returns_two_unknown_values"
            );
        }
        let selected_host=graph.uses.iter().find(|fact|matches!(fact.decision,Decision::Bound(value) if matches!(graph.values[value].kind,ValueKind::HostCallable(_)))).expect("host callable use");
        let binding = prepared.binding_site(&selected_host.site).unwrap();
        let (native_view, selector) = match native
            .query_views
            .get(&selected_host.context)
            .expect("selected host view")
        {
            NativeQueryView::Load { occurrence } => (
                NativeViewSelection::Load(*occurrence),
                QuerySelector::LoadOccurrence {
                    occurrence: *occurrence,
                },
            ),
            NativeQueryView::Invocation { frame } => (
                NativeViewSelection::Invocation(*frame),
                QuerySelector::Invocation { frame: *frame },
            ),
            NativeQueryView::Entry { frame } => (
                NativeViewSelection::Entry(*frame),
                QuerySelector::Entry { frame: *frame },
            ),
            NativeQueryView::Static { function } => (
                NativeViewSelection::SourceTemplates,
                QuerySelector::SourceTemplate {
                    function: *function,
                },
            ),
            NativeQueryView::Xml { action } => (
                NativeViewSelection::Occurrence(
                    *native
                        .control
                        .context_occurrences
                        .get(&selected_host.context)
                        .expect("XML host control occurrence missing"),
                ),
                QuerySelector::XmlOccurrence { action: *action },
            ),
        };
        let syntax = prepared.source_queries(&c).unwrap();
        let queries = syntax.with_execution(&graph, selector, &c).unwrap();
        let definition = queries.definitions(&selected_host.site).unwrap();
        let QueryFact::Known { value } = definition.selected else {
            panic!("{name}: selected host definition must preserve its environment identity");
        };
        assert!(
            value.origin.environment().is_some()
                && value.host_binding.is_some()
                && value.function_declaration.is_none()
                && value.function.is_none(),
            "{name}:environment_has_no_source_function"
        );
        let native_type = {
            let provider = prepared.freeze(&graph, native_view, &c).unwrap();
            let session = OwnedBindingSession::new_checked(provider, || c.check()).unwrap();
            let mut check = || c.check();
            let mut model = OwnedQuery::new_with_contribution_order(
                prepared.analysis(),
                &session,
                binding.file_id,
                &mut check,
                prepared.contribution_order(),
            )
            .with_workspace_facts(prepared.workspace_facts());
            let ty = model.type_of_expr(binding.syntax).unwrap();
            c.check().unwrap();
            matches!(ty.structural_type(), LuaType::Unknown)
        };
        assert!(native_type, "{name}:native_host_type_unknown");

        c.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(
            json!(
                prepared
                    .freeze(&graph, NativeViewSelection::Load(0), &c)
                    .err()
                    .map(|error| error.exit_code)
            ),
            json!(7),
            "{}",
            &format!("{name}:same_control_cancel")
        );
    }
}
#[test]
fn explicit_arguments_bind_current_host_returns_and_real_query_view() {
    let c = entry_control();
    let prepared = prepare_case("host-entry.lua", ENTRY_SOURCE, false, &c);
    let candidate = unique_entry(&prepared, "host-entry.lua", 1, &c);
    let mut keys = Vec::new();
    for (name, arguments, expected) in [
        ("entry", EntryArguments::Unspecified, "unavailable"),
        ("entry-empty", EntryArguments::Exact(vec![]), "known_absent"),
        (
            "entry-literal",
            EntryArguments::Exact(vec![EntryArgument::Boolean(true)]),
            "bound",
        ),
    ] {
        let request = entry_request(&candidate, arguments);
        let graph = prepared
            .build_plan(
                name,
                "after_load",
                LoadPlan::Known(vec!["host-entry.lua".into()]),
                Some(request.clone()),
                Some("explicit-entry-v1"),
                &c,
            )
            .unwrap();
        keys.push(graph.snapshot_key.clone());
        let native = graph
            .native_invocations
            .as_ref()
            .expect("entry native facts");
        let entry = native.entries.first().expect("entry fact");
        let frame = native
            .frames
            .get(entry.frame.expect("entry frame"))
            .expect("entry frame missing");
        let host = graph.host_runtime.as_ref().expect("entry host facts");
        let selected = |expression: &str| -> &CommonCallSlots {
            host.calls
                .iter()
                .find(|call| {
                    matches!(call.view, NativeQueryView::Entry { .. })
                        && graph.calls[call.call]
                            .site
                            .text(prepared.inputs())
                            .is_ok_and(|text| text == expression)
                })
                .unwrap_or_else(|| panic!("entry call missing: {expression}"))
        };
        assert!(native.control.complete, "execution:{name}:complete");
        assert!(
            frame.call.is_none()
                && frame.closure == Some(entry.closure)
                && matches!(
                    frame.origin,
                    Some(NativeInvocationOrigin::ExplicitEntry { entry: 0 })
                )
                && native.entries.len() == 1,
            "execution:{name}:real_entry_origin"
        );
        assert_eq!(
            json!(
                graph
                    .calls
                    .iter()
                    .filter(|call| matches!(
                        native.query_views.get(&call.context),
                        Some(NativeQueryView::Entry { .. })
                    ))
                    .count()
            ),
            json!(3),
            "{}",
            &format!("execution:{name}:source_body_calls_only")
        );
        assert_eq!(
            serde_json::to_value(&entry.parameters[0].selected_value).unwrap()["state"].clone(),
            json!(expected),
            "{}",
            &format!("execution:{name}:formal_argument_state")
        );
        let source = selected("Source()").returns[0].selected.clone();
        assert!(
            selected("Use(x)").arguments[0].selected == source,
            "execution:{name}:source_assignment_to_use"
        );
        assert!(
            selected("Use(value)").arguments[0].selected == entry.parameters[0].selected_value,
            "execution:{name}:formal_to_use"
        );
        assert!(
            entry.return_facts.len() == 1 && graph.uses[entry.return_facts[0]].decision == source,
            "execution:{name}:return_source_slot"
        );
        let syntax = prepared.source_queries(&c).unwrap();
        let queries = syntax
            .with_execution(&graph, QuerySelector::Entry { frame: frame.id }, &c)
            .unwrap();
        let calls = queries
            .calls(QueryTarget::All, PageRequest::default())
            .unwrap();
        assert!(
            calls.rows.len() == 3
                && calls
                    .rows
                    .iter()
                    .all(|call| call.occurrence == QuerySelector::Entry { frame: frame.id }),
            "execution:{name}:entry_query_view"
        );
        let fact = &graph.uses[graph.calls[selected("Use(value)").call].arguments[0]];
        let site = prepared.binding_site(&fact.site).unwrap();
        let ty = {
            let provider = prepared
                .freeze(&graph, NativeViewSelection::Entry(frame.id), &c)
                .unwrap();
            let session = OwnedBindingSession::new_checked(provider, || c.check()).unwrap();
            let mut check = || c.check();
            let mut model = OwnedQuery::new_with_contribution_order(
                prepared.analysis(),
                &session,
                site.file_id,
                &mut check,
                prepared.contribution_order(),
            )
            .with_workspace_facts(prepared.workspace_facts());
            let ty = model
                .type_of_expr(site.syntax)
                .unwrap()
                .structural_type()
                .clone();
            c.check().unwrap();
            ty
        };
        assert_eq!(
            ty,
            if name == "entry-empty" {
                LuaType::Nil
            } else {
                LuaType::Unknown
            },
            "execution:{name}:native_entry_provider"
        );
        assert_eq!(
            graph.snapshot_inputs.admission()["explicitEntry"].clone(),
            json!(request),
            "{}",
            &format!("execution:{name}:request_in_snapshot")
        );
    }
    assert_eq!(
        json!(keys.iter().collect::<std::collections::BTreeSet<_>>().len()),
        json!(3),
        "{}",
        "execution:entry:argument_identity_distinct"
    );
    assert_eq!(
        json!(
            prepared
                .entry_candidates("host-entry.lua", 0, &c)
                .err()
                .map(|error| error.exit_code)
        ),
        json!(4),
        "{}",
        "execution:entry:invalid_line_rejected"
    );
    let duplicate = prepared.build_plan(
        "entry-duplicate",
        "after_load",
        LoadPlan::Known(vec!["host-entry.lua".into(), "host-entry.lua".into()]),
        Some(entry_request(&candidate, EntryArguments::Unspecified)),
        None,
        &c,
    );
    assert!(
        duplicate.is_err_and(|error| {
            error
                .to_string()
                .contains("requires one loaded closure instance")
        }),
        "execution:entry:duplicate_closure_requires_selector"
    );
    let mut request = entry_request(&candidate, EntryArguments::Exact(vec![]));
    request.load_occurrence = Some(1);
    let graph = prepared
        .build_plan(
            "entry-selected-load",
            "after_load",
            LoadPlan::Known(vec!["host-entry.lua".into(), "host-entry.lua".into()]),
            Some(request),
            None,
            &c,
        )
        .unwrap();
    let native = graph
        .native_invocations
        .as_ref()
        .expect("selected load facts");
    assert_eq!(
        json!(native.closures[native.entries[0].closure].load_occurrence),
        json!(1),
        "{}",
        "execution:entry:exact_load_owns_closure"
    );
}

#[test]
fn ambiguous_entry_requires_exact_syntax_and_load_selectors() {
    let c = entry_control();
    let prepared = prepare_case("host-entry-ambiguous.lua", AMBIGUOUS_SOURCE, false, &c);
    let resolution = prepared
        .entry_candidates("host-entry-ambiguous.lua", 1, &c)
        .unwrap();
    let EntryResolution::SelectorRequired { candidates } = resolution else {
        panic!("line ambiguity not preserved");
    };
    assert_eq!(
        json!(candidates.len()),
        json!(2),
        "{}",
        "execution:entry-ambiguous:two_candidates"
    );
    let graph = prepared
        .build_plan(
            "entry-ambiguous",
            "after_load",
            LoadPlan::Known(vec!["host-entry-ambiguous.lua".into()]),
            None,
            None,
            &c,
        )
        .unwrap();
    assert!(
        graph.calls.is_empty()
            && graph
                .native_invocations
                .as_ref()
                .is_some_and(|native| native.entries.is_empty()),
        "execution:entry-ambiguous:no_arbitrary_execution"
    );

    let mut keys = Vec::new();
    for (index, name, count) in [(0, "entry-exact-a", 2), (1, "entry-exact-b", 0)] {
        let graph = prepared
            .build_plan(
                name,
                "after_load",
                LoadPlan::Known(vec!["host-entry-ambiguous.lua".into()]),
                Some(entry_request(
                    &candidates[index],
                    EntryArguments::Exact(vec![]),
                )),
                None,
                &c,
            )
            .unwrap();
        keys.push(graph.snapshot_key.clone());
        let native = graph
            .native_invocations
            .as_ref()
            .expect("exact native facts");
        let frame = native.entries[0].frame.expect("exact frame");
        assert_eq!(
            json!(
                graph
                    .calls
                    .iter()
                    .filter(|call| matches!(
                        native.query_views.get(&call.context),
                        Some(NativeQueryView::Entry { .. })
                    ))
                    .count()
            ),
            json!(count),
            "{}",
            &format!("execution:{name}:chosen_body_call_count")
        );
        assert_eq!(
            json!(graph.functions[native.frames[frame].function].closure),
            json!(candidates[index].closure),
            "{}",
            &format!("execution:{name}:exact_source_closure")
        );
    }
    assert!(
        keys[0] != keys[1],
        "execution:entry-ambiguous:target_identity_distinct"
    );
}

#[test]
fn factory_entry_selects_the_owned_capture_instance() {
    let c = entry_control();
    let source =
        include_str!("../../../../tests/fixtures/source-native/wow/native/host-transfers.lua");
    let prepared = prepare_case("host-transfers.lua", source, false, &c);
    let resolution = prepared
        .entry_candidates("host-transfers.lua", 7, &c)
        .unwrap();
    let EntryResolution::SelectorRequired { candidates } = resolution else {
        panic!("nested closure line must keep both candidates");
    };
    let candidate = candidates
        .iter()
        .min_by_key(|candidate| candidate.closure.end - candidate.closure.start)
        .expect("capture candidate");
    let baseline = prepared
        .build_plan(
            "entry-capture-baseline",
            "after_load",
            LoadPlan::Known(vec!["host-transfers.lua".into()]),
            None,
            None,
            &c,
        )
        .unwrap();
    let native = baseline
        .native_invocations
        .as_ref()
        .expect("capture baseline facts");
    let instances = native
        .closures
        .iter()
        .filter(|closure| baseline.functions[closure.function].closure == candidate.closure)
        .collect::<Vec<_>>();
    let duplicate = prepared.build_plan(
        "entry-capture-ambiguous",
        "after_load",
        LoadPlan::Known(vec!["host-transfers.lua".into()]),
        Some(entry_request(candidate, EntryArguments::Exact(vec![]))),
        None,
        &c,
    );
    assert!(
        duplicate.is_err_and(|error| {
            error
                .to_string()
                .contains("requires one loaded closure instance")
        }),
        "execution:entry-capture:instance_selector_required"
    );
    for (index, name, expected) in [
        (0, "entry-capture-a", "Source"),
        (1, "entry-capture-b", "Pair"),
    ] {
        let mut request = entry_request(candidate, EntryArguments::Exact(vec![]));
        request.creation_context = Some(
            instances
                .get(index)
                .expect("factory closure instance")
                .creation_context
                .clone(),
        );
        let graph = prepared
            .build_plan(
                name,
                "after_load",
                LoadPlan::Known(vec!["host-transfers.lua".into()]),
                Some(request),
                None,
                &c,
            )
            .unwrap();
        let native = graph
            .native_invocations
            .as_ref()
            .expect("capture entry facts");
        let entry = &native.entries[0];
        let frame = &native.frames[entry.frame.expect("capture entry frame")];
        assert!(
            frame.captures == native.closures[entry.closure].captures && !frame.captures.is_empty(),
            "execution:{name}:owns_exact_capture_cells"
        );
        let selected = &graph.uses[entry.return_facts[0]].decision;
        let host = graph.host_runtime.as_ref().expect("capture host");
        let actual = match selected {
            Decision::Bound(value) => match graph.values[*value].kind {
                ValueKind::HostReturn { binding, .. } => host.bindings[binding].declaration.name(),
                _ => "other",
            },
            _ => "unknown",
        };
        assert_eq!(
            json!(actual),
            json!(expected),
            "{}",
            &format!("execution:{name}:returns_owned_capture")
        );
    }
}

#[test]
fn known_and_unknown_load_order_preserve_identity_and_knowledge_boundary() {
    let c = entry_control();
    let prepared = prepare_files(ORDER_SOURCES, false, &c);
    let mut keys = Vec::new();
    for (name, order, expected) in [
        (
            "known-order-ab",
            vec![
                "host-order-a.lua".into(),
                "host-order-b.lua".into(),
                "host-order-main.lua".into(),
            ],
            "nil",
        ),
        (
            "known-order-ba",
            vec![
                "host-order-b.lua".into(),
                "host-order-a.lua".into(),
                "host-order-main.lua".into(),
            ],
            "source",
        ),
    ] {
        let graph = prepared
            .build_plan(name, "unknown", LoadPlan::Known(order), None, None, &c)
            .unwrap();
        keys.push(graph.snapshot_key.clone());
        let host = graph.host_runtime.as_ref().expect("ordered host facts");
        let use_ = host
            .calls
            .iter()
            .find(|call| {
                call.host_binding
                    .is_some_and(|binding| host.bindings[binding].declaration.name() == "Use")
            })
            .expect("ordered consumer");
        let selected = &use_.arguments[0].selected;
        let actual = match selected {
            Decision::KnownAbsent(_) => "nil",
            Decision::Bound(value) => match &graph.values[*value].kind {
                ValueKind::Primitive(kind) if kind == "nil" => "nil",
                ValueKind::HostReturn { binding, .. }
                    if host.bindings[*binding].declaration.name() == "Source" =>
                {
                    "source"
                }
                _ => "other",
            },
            _ => "unknown",
        };
        assert_eq!(
            json!(actual),
            json!(expected),
            "{}",
            &format!("execution:{name}:selected_wrap_return")
        );
        assert!(
            graph.phase == "unknown"
                && graph
                    .native_invocations
                    .as_ref()
                    .is_some_and(|native| native.control.complete)
                && graph.script_load_occurrences.len() == 3,
            "execution:{name}:phase_does_not_erase_known_order"
        );
    }
    assert!(
        keys[0] != keys[1],
        "execution:known-order:identity_distinct"
    );
    let files = ORDER_SOURCES
        .iter()
        .map(|(file, _)| (*file).into())
        .collect();
    let graph = prepared
        .build_plan(
            "unknown-order",
            "after_load",
            LoadPlan::Unknown(files),
            None,
            None,
            &c,
        )
        .unwrap();
    let native = graph
        .native_invocations
        .as_ref()
        .expect("unknown order native facts");
    assert!(
        !native.control.complete
            && !native.control.truncated
            && native.execution_boundaries.len() == 1
            && native.execution_boundaries[0].reason == "load_order_unknown",
        "execution:unknown-order:knowledge_boundary"
    );
    assert!(
        graph.script_load_occurrences.is_empty()
            && graph.calls.is_empty()
            && graph.writes.is_empty()
            && native.frames.is_empty()
            && native.control.occurrences.is_empty(),
        "execution:unknown-order:no_fabricated_load"
    );
    let declarations = [
        unique_entry(&prepared, "host-order-a.lua", 1, &c),
        unique_entry(&prepared, "host-order-b.lua", 1, &c),
    ];
    assert!(
        declarations[0].id != declarations[1].id,
        "execution:unknown-order:both_definition_candidates"
    );
    let syntax_calls = prepared
        .source_queries(&c)
        .and_then(|query| query.calls(QueryTarget::All, PageRequest::default()))
        .unwrap();
    assert_eq!(
        json!(syntax_calls.rows.len()),
        json!(3),
        "{}",
        "execution:unknown-order:syntax_calls_retained"
    );
    assert!(
        prepared
            .freeze(&graph, NativeViewSelection::SourceTemplates, &c)
            .is_err(),
        "execution:unknown-order:provider_rejects_incomplete"
    );
    assert!(
        keys.iter().all(|key| key != &graph.snapshot_key),
        "execution:unknown-order:identity_distinct"
    );
}
fn boundary_nil(graph: &Graph, decision: &Decision) -> bool {
    matches!(decision, Decision::KnownAbsent(_))
        || matches!(decision,Decision::Bound(value) if matches!(&graph.values[*value].kind,ValueKind::Primitive(kind) if kind=="nil"))
}
#[test]
fn depth_budget_keeps_source_and_explicit_entry_prefixes_honest() {
    for explicit in [false, true] {
        let name = if explicit {
            "depth-entry"
        } else {
            "depth-source-call"
        };
        let c = entry_control();
        let prepared = prepare_files(
            &[
                ("host-entry-depth.lua", DEPTH_SOURCE),
                ("host-entry-depth-call.lua", DEPTH_CALL),
            ],
            false,
            &c,
        );
        let candidate = unique_entry(&prepared, "host-entry-depth.lua", 1, &c);
        let (files, request) = if explicit {
            (
                vec!["host-entry-depth.lua".into()],
                Some(entry_request(&candidate, EntryArguments::Exact(vec![]))),
            )
        } else {
            (
                vec![
                    "host-entry-depth.lua".into(),
                    "host-entry-depth-call.lua".into(),
                ],
                None,
            )
        };
        let graph = prepared
            .build_plan(
                name,
                "after_load",
                LoadPlan::Known(files),
                request,
                None,
                &c,
            )
            .unwrap();
        let native = graph
            .native_invocations
            .as_ref()
            .expect("depth native prefix");
        let host = graph.host_runtime.as_ref().expect("depth host prefix");
        let frame = native
            .frames
            .iter()
            .find(|frame| frame.closure.is_some())
            .expect("depth concrete frame");
        let boundary = native
            .control
            .boundaries
            .first()
            .expect("depth budget boundary");
        assert_eq!(
            json!(native.control.coverage),
            json!("verified_prefix_with_explicit_boundaries"),
            "{}",
            &format!("boundary:{name}:verified_prefix")
        );
        assert!(
            !native.control.complete
                && native.control.truncated
                && matches!(boundary.kind, NativeBoundaryKind::Budget)
                && boundary.reason == "native block depth budget exceeded",
            "boundary:{name}:budget_incomplete_truncated"
        );
        assert!(
            boundary
                .site
                .text(prepared.inputs())
                .is_ok_and(|text| text.starts_with("do"))
                && boundary
                    .occurrence
                    .and_then(|id| native.control.occurrences.get(id))
                    .is_some_and(|occurrence| occurrence.site == boundary.site
                        && occurrence.context == boundary.context
                        && occurrence.frame == Some(frame.id)),
            "boundary:{name}:real_boundary_occurrence"
        );
        let prefix = host
            .calls
            .iter()
            .filter_map(|call| {
                call.host_binding
                    .map(|binding| host.bindings[binding].declaration.name())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            json!(prefix),
            json!(["Source", "Use"]),
            "{}",
            &format!("boundary:{name}:prefix_host_calls")
        );
        assert!(
            graph.calls.iter().all(|call| {
                call.site
                    .text(prepared.inputs())
                    .is_ok_and(|text| text != "Use(Source())")
            }),
            "boundary:{name}:suffix_not_evaluated"
        );
        assert!(
            !frame.effects_complete && frame.return_facts.is_empty(),
            "boundary:{name}:frame_effects_incomplete"
        );
        let write = graph
            .writes
            .iter()
            .find(|write| {
                write
                    .site
                    .text(prepared.inputs())
                    .is_ok_and(|text| text == "prefix")
            })
            .expect("verified prefix write");
        let effects = if explicit {
            &native.entries[0].effect_write_versions
        } else {
            &graph.calls[frame.call.expect("source frame call")].effect_write_versions
        };
        assert!(
            effects.contains(&write.id)
                && matches!(write.decision,Decision::Bound(value) if matches!(graph.values[value].kind,ValueKind::HostReturn{..})),
            "boundary:{name}:prefix_writes_retained"
        );
        let selection = if explicit {
            NativeViewSelection::Entry(frame.id)
        } else {
            NativeViewSelection::Invocation(frame.id)
        };
        assert!(
            prepared.freeze(&graph, selection, &c).is_err(),
            "boundary:{name}:complete_provider_rejected"
        );
        let queries = prepared
            .source_queries(&c)
            .and_then(|query| query.with_execution(&graph, QuerySelector::AllOccurrences, &c))
            .unwrap();
        let calls = queries
            .calls(QueryTarget::All, PageRequest::default())
            .unwrap();
        assert_eq!(
            json!(calls.coverage),
            json!("verified_prefix_with_explicit_boundaries"),
            "{}",
            &format!("boundary:{name}:prefix_query_coverage")
        );
        let entry_honest = if explicit {
            native.entries.len() == 1
                && !native.entries[0].effects_complete
                && native.entries[0].return_facts.is_empty()
        } else {
            native.entries.is_empty()
        };
        assert!(entry_honest, "boundary:{name}:entry_completion_honest");
    }
}

#[test]
fn conditional_entry_preserves_predicate_and_current_capture_write() {
    let c = entry_control();
    let prepared = prepare_case("host-entry-conditional.lua", CONDITIONAL_ENTRY, false, &c);
    let candidate = unique_entry(&prepared, "host-entry-conditional.lua", 4, &c);
    let graph = prepared
        .build_plan(
            "conditional-entry",
            "after_load",
            LoadPlan::Known(vec!["host-entry-conditional.lua".into()]),
            Some(entry_request(&candidate, EntryArguments::Exact(vec![]))),
            None,
            &c,
        )
        .unwrap();
    let native = graph
        .native_invocations
        .as_ref()
        .expect("conditional native");
    let entry = &native.entries[0];
    let frame = &native.frames[entry.frame.expect("conditional frame")];
    let creation = &native.control.occurrences[entry
        .assumption
        .creation_occurrence
        .expect("conditional creation occurrence")];
    let flag = graph
        .uses
        .iter()
        .find(|fact| {
            fact.site
                .text(prepared.inputs())
                .is_ok_and(|text| text == "Flag")
        })
        .expect("Flag condition");
    let expected = vec![vec![NativePredicate {
        condition: flag.id,
        outcome: true,
    }]];
    assert!(
        entry.assumption.rule == "selected_closure_exists"
            && entry.assumption.paths == expected
            && entry.assumption.conditional_branch.is_some(),
        "boundary:conditional-entry:existence_assumption"
    );
    assert!(
        !creation.loops.is_empty() && creation.paths == expected,
        "boundary:conditional-entry:creation_loop_evidence"
    );
    let first = native
        .control
        .occurrences
        .iter()
        .find(|occurrence| occurrence.frame == Some(frame.id))
        .expect("entry initial occurrence");
    assert!(
        first.parent.is_none() && first.loops.is_empty() && first.paths == expected,
        "boundary:conditional-entry:no_old_control_state"
    );
    let call = graph
        .calls
        .iter()
        .find(|call| {
            matches!(
                native.query_views.get(&call.context),
                Some(NativeQueryView::Entry { .. })
            )
        })
        .expect("conditional body call");
    let occurrence = &native.control.occurrences[native.control.context_occurrences[&call.context]];
    assert_eq!(
        json!(occurrence.paths),
        json!(expected),
        "{}",
        "boundary:conditional-entry:body_preserves_predicate"
    );
    assert_eq!(
        json!(call.conditional_branch),
        json!(entry.assumption.conditional_branch),
        "{}",
        "boundary:conditional-entry:body_preserves_branch"
    );
    let argument = &graph.uses[call.arguments[0]];
    assert!(
        boundary_nil(&graph, &argument.decision)
            && entry
                .return_facts
                .iter()
                .all(|fact| boundary_nil(&graph, &graph.uses[*fact].decision)),
        "boundary:conditional-entry:latest_capture_value"
    );
    let cell = frame
        .captures
        .values()
        .next()
        .expect("conditional capture cell");
    let latest = graph
        .writes
        .iter()
        .rev()
        .find(|write| &write.cell == cell)
        .expect("latest captured write");
    assert_eq!(
        json!(argument.writes),
        json!(vec![latest.id]),
        "{}",
        "boundary:conditional-entry:latest_capture_write"
    );
    let host = graph.host_runtime.as_ref().expect("conditional host");
    let old = host
        .calls
        .iter()
        .find(|call| {
            call.host_binding
                .is_some_and(|binding| host.bindings[binding].declaration.name() == "Source")
        })
        .expect("original Source");
    assert!(
        old.returns[0].selected != argument.decision,
        "boundary:conditional-entry:no_creation_heap_restore"
    );
    let calls = prepared
        .source_queries(&c)
        .and_then(|query| {
            query.with_execution(&graph, QuerySelector::Entry { frame: frame.id }, &c)
        })
        .and_then(|query| query.calls(QueryTarget::All, PageRequest::default()))
        .unwrap();
    assert!(
        calls.rows.len() == 1
            && calls.rows[0].evidence_kind == "conditional_abstract_call"
            && calls.rows[0].conditional_branch == entry.assumption.conditional_branch,
        "boundary:conditional-entry:conditional_query_evidence"
    );
    assert!(
        native.control.complete && entry.effects_complete,
        "boundary:conditional-entry:bounded_assumption_complete"
    );
}

#[test]
fn unreachable_entry_retains_syntax_without_fabricating_loaded_instance() {
    let c = entry_control();
    let prepared = prepare_case("host-entry-unreachable.lua", UNREACHABLE_ENTRY, false, &c);
    let candidate = unique_entry(&prepared, "host-entry-unreachable.lua", 3, &c);
    assert!(
        candidate.closure.file == "host-entry-unreachable.lua",
        "boundary:unreachable-entry:syntax_candidate_retained"
    );
    let graph = prepared
        .build_plan(
            "unreachable-load",
            "after_load",
            LoadPlan::Known(vec!["host-entry-unreachable.lua".into()]),
            None,
            None,
            &c,
        )
        .unwrap();
    assert!(
        graph
            .native_invocations
            .as_ref()
            .is_some_and(|native| native.closures.is_empty())
            && graph.calls.is_empty(),
        "boundary:unreachable-entry:no_loaded_closure"
    );
    let failure = prepared
        .build_plan(
            "unreachable-entry",
            "after_load",
            LoadPlan::Known(vec!["host-entry-unreachable.lua".into()]),
            Some(entry_request(&candidate, EntryArguments::Exact(vec![]))),
            None,
            &c,
        )
        .err()
        .expect("unreachable entry accepted");
    assert!(
        failure
            .to_string()
            .contains("requires one loaded closure instance; candidates=[]"),
        "boundary:unreachable-entry:selection_rejected"
    );
}

#[test]
fn exact_vararg_fixture_and_original_control_stop_priority() {
    let c = entry_control();
    let prepared = prepare_case("host-entry-vararg.lua", VARARG_ENTRY, false, &c);
    let candidate = unique_entry(&prepared, "host-entry-vararg.lua", 1, &c);
    // Exact tail varargs are supported; unknown/open tails remain covered by
    // invocation::vararg_adjustment_tests without retaining the old rejection.
    let graph = prepared
        .build_plan(
            "exact-vararg-entry",
            "after_load",
            LoadPlan::Known(vec!["host-entry-vararg.lua".into()]),
            Some(entry_request(&candidate, EntryArguments::Exact(vec![]))),
            None,
            &c,
        )
        .unwrap();
    let native = graph.native_invocations.as_ref().unwrap();
    assert!(native.control.complete && native.entries[0].effects_complete);
    assert!(native.entries[0].return_facts.is_empty());
    c.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        json!(
            prepared
                .build_plan(
                    "cancelled-entry",
                    "after_load",
                    LoadPlan::Known(vec!["host-entry-vararg.lua".into()]),
                    Some(entry_request(&candidate, EntryArguments::Exact(vec![]))),
                    None,
                    &c
                )
                .err()
                .map(|error| error.exit_code)
        ),
        json!(7),
        "{}",
        "boundary:entry:cancel_priority"
    );
    let c = OperationControl::uncancelable(Instant::now() + Duration::from_secs(1));
    let prepared = prepare_case("host-entry-vararg.lua", VARARG_ENTRY, false, &c);
    let candidate = unique_entry(&prepared, "host-entry-vararg.lua", 1, &c);
    std::thread::sleep(c.remaining().unwrap() + Duration::from_millis(2));
    assert_eq!(
        json!(
            prepared
                .build_plan(
                    "expired-entry",
                    "after_load",
                    LoadPlan::Known(vec!["host-entry-vararg.lua".into()]),
                    Some(entry_request(&candidate, EntryArguments::Exact(vec![]))),
                    None,
                    &c
                )
                .err()
                .map(|error| error.exit_code)
        ),
        json!(5),
        "{}",
        "boundary:entry:deadline_priority"
    );
}
