// Current graph assertions cover each projected source and XML handler variant.
#[test]
fn scope_projection_preserves_full_host_projection_and_charges() {
    for flow in [false, true] {
        let c = control();
        let source = "local one=1\nlocal two=2\nfunction F(x) return one,two,x end\nif Flag then A=one else A=two end\nlocal a,b,c=F(one)\nHost(a,b,c)\n";
        let prepared = prepare(graph_fixture_inputs(&c, flow, source, None), &c).unwrap();
        let graph = prepared.build_plan("scope-host", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &c).unwrap();
        let native = graph.native_invocations.as_ref().unwrap();
        let host = graph.host_runtime.as_ref().unwrap();
        assert!(native.control.complete);
        assert!(!native.capture_facts.is_empty());
        assert!(!native.control.write_joins.is_empty());
        for kind in ["storage_read", "storage_write", "conditional_write_candidates", "argument_slot", "return_slot", "scalar_adjustment", "scalar_result_use"] {
            assert!(host.transfers.iter().any(|fact| fact.kind == kind), "missing {kind}");
        }
        assert!(!host.capture_references.is_empty());
        assert_graph_rebuild(&prepared, &graph);
        assert_graph_cancelled(&prepared, &graph);
    }
}

#[test]
fn scope_projection_preserves_all_xml_handler_variants_and_charges() {
    let c = control();
    let source = "M={}\nfunction M:Run() return self end\nfunction Callback(self) return self end\n";
    let xml = b"<Ui><Script file=\"a.lua\"/><Frame name=\"A\" mixin=\"M\"><Scripts><OnLoad method=\"Run\"/><OnShow function=\"Callback\"/><OnHide/><OnEvent method=\"Run\" function=\"Callback\"/><OnUpdate>return self</OnUpdate></Scripts></Frame><Frame name=\"B\" mixin=\"M\"><Scripts><OnLoad method=\"Missing\"/><OnShow function=\"Bad()\"/></Scripts></Frame></Ui>";
    let base = graph_fixture_inputs(&c, false, source, None);
    let mut files = base.corpus.files.iter().map(|file| SourceFileInput {
        path: file.path.clone(), original_utf8: file.original.to_string(),
        content_sha256: hash(file.original.as_bytes()), analysis_utf8: file.analysis.to_string(),
        map: if file.prefix == 0 { SourceMap::Identity } else { SourceMap::Utf8BomPrefix },
    }).collect::<Vec<_>>();
    for script in crate::xml_inline::extract("ui.xml", xml).unwrap() {
        let text = std::str::from_utf8(script.source_bytes()).unwrap().to_string();
        files.push(SourceFileInput { path: script.key, content_sha256: hash(text.as_bytes()),
            original_utf8: text.clone(), analysis_utf8: text, map: SourceMap::Identity });
    }
    let input = SourceInputs::admit(SourceInputSpec {
        files, workspace_root: base.corpus.root.clone(), source_identity: base.corpus.source_identity.clone(),
        phase: base.phase.clone(), ordered_occurrences: base.ordered.clone(), config: base.corpus.config.clone(),
        host: base.host.clone(), luac: base.corpus.luac.clone(), limits: base.corpus.limits.clone(),
    }, &c).unwrap();
    let program = crate::xml_load::Program::from_sources("Main.toc", [
        ("Main.toc", b"ui.xml\n".as_slice()), ("ui.xml", xml.as_slice()), ("a.lua", source.as_bytes()),
    ], &c).unwrap();
    let prepared = prepare(input, &c).unwrap();
    let graph = prepared.build_xml_plan("scope-xml", Arc::new(program), None, &c).unwrap();
    assert!(graph.native_invocations.as_ref().unwrap().control.complete);
    let handlers = &graph.xml.as_ref().unwrap().handlers;
    assert_eq!(handlers.len(), 7);
    assert!(handlers[0].closure.is_some());
    assert!(handlers[1].closure.is_some());
    assert!(matches!(handlers[2].target, crate::xml_load::HandlerTarget::Empty));
    assert!(matches!(handlers[3].target, crate::xml_load::HandlerTarget::Unavailable { .. }));
    assert!(matches!(handlers[4].target, crate::xml_load::HandlerTarget::Inline { .. }));
    assert!(handlers[4].closure.is_some());
    assert!(handlers[5].closure.is_none());
    assert_eq!(handlers[6].selected, Decision::Unavailable("xml_reference_expression_not_static".into()));
    assert_graph_rebuild(&prepared, &graph);
    assert_graph_cancelled(&prepared, &graph);
}

// Reconstruct only the state consumed by host finalization or a script-free XML
// program. This deliberately retains existing transfers so append order/proofs
// and failure prefixes are compared, not just an empty destination.
fn scope_projection_builder(seed: &Graph, inputs: Arc<SourceInputs>) -> Builder {
    let facts = seed.native_invocations.as_ref().unwrap().clone();
    let mut native = NativeInvocationState {
        closure_values: facts.closures.iter().map(|closure| (closure.value, closure.id)).collect(),
        facts, ..Default::default()
    };
    let host = seed.host_runtime.as_ref().unwrap();
    native.host.open_tail_uses = host.open_tail_uses.clone();
    native.host.cardinality_uses = host.cardinality_uses.clone();
    Builder {
        inputs, control: seed.control.clone(),  graph: seed.facts.clone(),
        harvest: Harvest { declaration_sites: BTreeMap::new(), blocks: BTreeMap::new(),
            local_reads: BTreeMap::new(), signatures: BTreeMap::new(), intrinsic_declarations: BTreeMap::new(), raw: vec![] },
        cells: seed.writes.iter().map(|write| (write.cell.clone(), write.id)).collect(),
        objects: vec![], bodies: vec![], heap_observation: 0, object_versions: vec![],
        object_view_ids: BTreeMap::new(), object_slots: 0, field_write_observations: BTreeMap::new(),
        observation_failure: None, native, xml_objects: BTreeMap::new(),
    }
}

fn run_scope_projection(mut candidate: Builder, run: impl FnOnce(&mut Builder) -> Result<()>) -> (Result<()>, usize, Builder) {
    let before = candidate.inputs.shared_steps.load(Ordering::Relaxed);
    let result = run(&mut candidate);
    let charges = candidate.inputs.shared_steps.load(Ordering::Relaxed) - before;
    (result, charges, candidate)
}

#[test]
fn scope_projection_preserves_host_transfer_cap_prefix_and_first_error() {
    let c = control();
    let source = "function F(x) return x end\nA=1\nF(A)\nHost(A)\n";
    let prepared = prepare(graph_fixture_inputs(&c, true, source, None), &c).unwrap();
    let seed = prepared.build_plan("scope-transfer-cap", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &c).unwrap();
    assert!(seed.native_invocations.as_ref().unwrap().capture_facts.is_empty());
    let existing = seed.host_runtime.as_ref().unwrap().transfers.len();
    // A cap at successive positions crosses the read/write/argument/return
    // projection boundaries without changing production limits or shared quota.
    let (complete, total, _) = run_scope_projection(
        scope_projection_builder(&seed, seed.inputs.clone()),
        Builder::host_finish_facts,
    );
    complete.unwrap();
    for remaining in 0..total {
        let mut candidate = scope_projection_builder(&seed, seed.inputs.clone());
        candidate.native.host.invocation_transfers = HOST_TRANSFER_LIMIT - remaining;
        let (result, charges, builder) = run_scope_projection(candidate,
            Builder::host_finish_facts);
        assert_eq!(result, Err("host transfer fact budget exceeded".into()));
        assert_eq!(charges, remaining + 1);
        assert_eq!(builder.graph.host_runtime.as_ref().unwrap().transfers.len(), existing + remaining);
    }
    let mut candidate = scope_projection_builder(&seed, seed.inputs.clone());
    for builder in [&mut candidate] {
        builder.observation_failure = Some("original observation failure".into());
        builder.native.host.invocation_transfers = HOST_TRANSFER_LIMIT;
    }
    let (result, charges, _) = run_scope_projection(candidate,
        Builder::host_finish_facts);
    assert_eq!(result, Err("original observation failure".into()));
    assert_eq!(charges, 0);
    // Cancellation wins at the same existing budget check, before any charge.
    c.tracked_cancel_handle().unwrap().cancel();
    let expected = c.check().unwrap_err().to_string();
    let (result, charges, _) = run_scope_projection(
        scope_projection_builder(&seed, seed.inputs.clone()),
        Builder::host_finish_facts);
    assert_eq!(result, Err(expected));
    assert_eq!(charges, 0);
}

#[test]
fn scope_projection_preserves_capture_cap_after_transfer_prefix() {
    let c = control();
    let source = "local one=1\nlocal two=2\nfunction F() return one,two end\nHost()\n";
    let prepared = prepare(graph_fixture_inputs(&c, true, source, None), &c).unwrap();
    let seed = prepared.build_plan("scope-capture-cap", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &c).unwrap();
    let captures = seed.native_invocations.as_ref().unwrap().capture_facts.len();
    assert!(captures >= 2);
    let (result, total, complete) = run_scope_projection(
        scope_projection_builder(&seed, seed.inputs.clone()),
        Builder::host_finish_facts);
    result.unwrap();
    let transfers = complete.graph.host_runtime.as_ref().unwrap().transfers.len();
    for remaining in [0, 1] {
        let mut candidate = scope_projection_builder(&seed, seed.inputs.clone());
        for builder in [&mut candidate] {
            // Saturate the real destination cap with an existing reference. The
            // source captures and their original order are untouched.
            let references = &mut builder.graph.host_runtime.as_mut().unwrap().capture_references;
            references.resize(INVOCATION_CAPTURE_EDGE_LIMIT - remaining, references[0].clone());
        }
        let (result, charges, builder) = run_scope_projection(candidate,
            Builder::host_finish_facts);
        assert_eq!(result, Err("host capture reference budget exceeded".into()));
        assert_eq!(charges, total - captures + remaining + 1);
        let host = builder.graph.host_runtime.as_ref().unwrap();
        assert_eq!(host.transfers.len(), transfers);
        assert_eq!(host.capture_references.len(), INVOCATION_CAPTURE_EDGE_LIMIT);
    }
}

#[test]
fn scope_projection_preserves_xml_shared_budget_prefix_and_cancellation() {
    let c = control();
    let prepared = prepare(inputs(&c, false), &c).unwrap();
    let seed = prepared.build_plan("scope-xml-budget", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &c).unwrap();
    // No Lua actions: every charge belongs to the unchanged action loop or a
    // handler reference. Multiple handlers exercise failure after earlier updates.
    let program = crate::xml_load::Program::from_sources("Main.toc", [
        ("Main.toc", b"ui.xml\n".as_slice()),
        ("ui.xml", b"<Ui><Frame name=\"A\"><Scripts><OnLoad function=\"F\"/><OnShow function=\"Missing\"/><OnHide method=\"Run\"/><OnEvent/></Scripts></Frame></Ui>".as_slice()),
    ], &c).unwrap();
    let (result, total, _) = run_scope_projection(
        scope_projection_builder(&seed, seed.inputs.clone()),
        |builder| builder.xml_load(&program));
    result.unwrap();
    for remaining in 0..=total {
        let bounded = || {
            // Each independent admission is charged forward to the boundary;
            // no counter reset, replacement, relaxed limit, or production hook.
            let input = inputs(&c, false);
            let used = input.shared_steps.load(Ordering::Relaxed);
            input.charge(input.corpus.limits.construction_steps - used - remaining).unwrap();
            scope_projection_builder(&seed, input)
        };
        let (result, charges, _) = run_scope_projection(bounded(),
            |builder| builder.xml_load(&program));
        assert_eq!(charges, remaining);
        if remaining == total { result.unwrap(); }
        else { assert!(result.unwrap_err().contains("shared operation step budget exceeded")); }
    }
    c.tracked_cancel_handle().unwrap().cancel();
    let expected = c.check().unwrap_err().to_string();
    let (result, charges, _) = run_scope_projection(
        scope_projection_builder(&seed, seed.inputs.clone()),
        |builder| builder.xml_load(&program));
    assert_eq!(result, Err(expected));
    assert_eq!(charges, 0);
}
