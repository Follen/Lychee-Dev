// Rebuilding the same current immutable graph preserves facts and ownership.
fn assert_graph_rebuild(prepared: &PreparedSnapshot, seed: &Graph) {
    let candidate = build_graph(prepared, &seed.scenario, seed.snapshot_inputs.as_ref().clone(), seed.inputs.clone()).unwrap();
    assert_eq!(serde_json::to_vec(&candidate.facts).unwrap(), serde_json::to_vec(&seed.facts).unwrap());
    assert!(!candidate.raw_emmy.is_empty());
    assert!(Arc::ptr_eq(&candidate.inputs, &seed.inputs));
    assert!(Arc::ptr_eq(&candidate.owner, &seed.owner));
    assert!(candidate.control.same_scope(&seed.control));
    let facts = candidate.native_invocations.as_ref().unwrap();
    if facts.control.complete {
        assert!(candidate.calls.iter().all(|call| call.result_fact.is_some()));
        assert!(candidate.object_views.iter().all(|view| !view.proof_id.is_empty()));
    }
}
fn assert_graph_cancelled(prepared: &PreparedSnapshot, seed: &Graph) {
    let control = &prepared.control;
    control.tracked_cancel_handle().unwrap().cancel();
    let expected = control.check().unwrap_err();
    let before = seed.inputs.shared_steps.load(Ordering::Relaxed);
    let result = build_graph(prepared, &seed.scenario, seed.snapshot_inputs.as_ref().clone(), seed.inputs.clone());
    assert_eq!(result.as_ref().err(), Some(&expected.to_string()));
    assert_eq!(seed.inputs.shared_steps.load(Ordering::Relaxed), before);
    let error = prepared.build("cancelled-public-build", None, control).err().unwrap();
    assert_eq!(serde_json::to_value(error).unwrap(), serde_json::to_value(&expected).unwrap());
    assert_eq!(serde_json::to_value(seed.check().unwrap_err()).unwrap(), serde_json::to_value(expected).unwrap());
}

#[test]
fn current_graph_rebuild_preserves_complete_facts_and_cancellation() {
    let c = control();
    let prepared = prepare(inputs(&c, false), &c).unwrap();
    let graph = prepared.build("graph-rebuild-complete", None, &c).unwrap();
    assert_graph(&graph, &c, None);
    assert!(!graph.calls.is_empty());
    let facts = graph.native_invocations.as_ref().unwrap();
    assert!(facts.control.complete);
    assert!(!facts.storages.is_empty());
    assert!(!facts.frames.is_empty());
    assert!(!facts.control.context_observations.is_empty());
    assert!(graph.host_runtime.is_some());
    assert_graph_rebuild(&prepared, &graph);
    assert_graph_cancelled(&prepared, &graph);
}

#[test]
fn graph_moves_preserve_unknown_plan_and_flow_budget_partial_graphs() {
    for budget in [false, true] {
        let c = control();
        let input = if budget {
            graph_fixture_inputs(
                &c,
                true,
                "function F(x) return x end\nA=1\nHost()\n",
                Some(1),
            )
        } else {
            inputs(&c, false)
        };
        let prepared = prepare(input, &c).unwrap();
        let graph = if budget {
            prepared
                .build("graph-move-budget", None, &c)
                .unwrap()
        } else {
            prepared
                .build_plan(
                    "graph-move-unknown",
                    "unknown",
                    LoadPlan::Unknown(vec!["a.lua".into(), "bom.lua".into()]),
                    None,
                    None,
                    &c,
                )
                .unwrap()
        };
        let facts = graph.native_invocations.as_ref().unwrap();
        assert!(!facts.control.complete);
        if budget {
            assert!(facts.control.truncated);
            assert!(
                facts
                    .control
                    .boundaries
                    .iter()
                    .any(|boundary| boundary.reason == "flow_path_budget")
            );
            assert!(!facts.control.occurrences.is_empty());
        } else {
            assert!(!facts.control.truncated);
            assert_eq!(facts.control.coverage, "unavailable_execution_plan");
            assert!(
                facts
                    .execution_boundaries
                    .iter()
                    .any(|boundary| boundary.reason == "load_order_unknown")
            );
        }
        assert!(
            prepared
                .freeze(&graph, NativeViewSelection::SourceTemplates, &c)
                .is_err()
        );
        assert_graph(&graph, &c, None);
        assert_graph_rebuild(&prepared, &graph);
        assert_graph_cancelled(&prepared, &graph);
    }
}

#[test]
fn graph_moves_preserve_entry_and_xml_graphs() {
    let c = control();
    let input = inputs(&c, false);
    let prepared = prepare(input.clone(), &c).unwrap();
    let entry = prepared
        .build_plan(
            "graph-move-entry",
            "after_load",
            LoadPlan::Known(vec!["a.lua".into(), "bom.lua".into()]),
            Some(EntryRequest {
                candidate: prepared.entry_catalog[0].id.clone(),
                load_occurrence: None,
                creation_context: None,
                arguments: EntryArguments::Exact(vec![EntryArgument::Boolean(true)]),
            }),
            None,
            &c,
        )
        .unwrap();
    let facts = entry.native_invocations.as_ref().unwrap();
    assert!(facts.control.complete);
    assert_eq!(facts.entries.len(), 1);
    assert!(facts.entries[0].effects_complete);
    assert_graph_rebuild(&prepared, &entry);
    let program = crate::xml_load::Program::from_sources(
        "Main.toc",
        [
            ("Main.toc", b"ui.xml\n".as_slice()),
            (
                "ui.xml",
                b"<Ui><Script file=\"a.lua\"/><Script file=\"a.lua\"/></Ui>".as_slice(),
            ),
            ("a.lua", input.original("a.lua").unwrap().as_bytes()),
        ],
        &c,
    )
    .unwrap();
    let xml = prepared
        .build_xml_plan("graph-move-xml", Arc::new(program), None, &c)
        .unwrap();
    assert!(xml.native_invocations.as_ref().unwrap().control.complete);
    assert!(xml.xml.is_some());
    assert_eq!(xml.inputs.ordered, vec!["a.lua", "a.lua"]);
    assert_graph_rebuild(&prepared, &xml);
    assert_graph_cancelled(&prepared, &xml);
}

#[test]
fn graph_moves_preserve_checkpoint_objects_cells_roots_and_full_json() {
    let c = control();
    // Same admission/host/BOM fixture; this small source makes checkpoint
    // containers and their nested object/cell payloads nonempty.
    let source = "function F(x) return x end\nlocal t={value=1}\nif Flag then t.value=2 else t.value=3 end\nA=t.value\nHost()\n";
    let prepared = prepare(graph_fixture_inputs(&c, false, source, None), &c).unwrap();
    let graph = prepared
        .build("graph-move-checkpoint", None, &c)
        .unwrap();
    let facts = graph.native_invocations.as_ref().unwrap();
    assert!(facts.control.complete);
    assert!(!facts.checkpoints.is_empty());
    assert!(!facts.restores.is_empty());
    assert!(!graph.object_views.is_empty());
    assert!(
        facts
            .checkpoints
            .iter()
            .any(|checkpoint| !checkpoint.objects.is_empty())
    );
    assert!(
        facts
            .checkpoints
            .iter()
            .any(|checkpoint| !checkpoint.cells.materialize().is_empty())
    );
    // GraphFacts serialization includes the full materialized checkpoint maps,
    // objects, escaped roots, proof IDs, and control coverage, with no masking.
    assert_graph_rebuild(&prepared, &graph);
    assert_graph_cancelled(&prepared, &graph);
}
