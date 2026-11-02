use super::*;
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn inputs(c: &OperationControl, flow: bool) -> Arc<SourceInputs> {
    graph_fixture_inputs(
        c,
        flow,
        "function F(x) return x end\nA=1\nHost()\n",
        None,
    )
}
fn graph_fixture_inputs(
    c: &OperationControl,
    flow: bool,
    first_source: &str,
    max_paths: Option<usize>,
) -> Arc<SourceInputs> {
    let material = json!({"sourceExecution":false,"configExecution":false,"callables":[{
        "name":"Host","identity":"sharing-host","effect":"no_lua_heap_write","retainsArguments":false,"returnArity":0,"sourceExecution":false,
        "descriptor":{"kind":"function","name":"Host","raw":{"documentation":"完整 API evidence","nested":{"z":1,"a":[3,2,1]}}}
    }]});
    let bytes = serde_json::to_vec(&material).unwrap();
    let host = TrustedHostEnvironment::from_host_material(
        "sharing-test".into(),
        false,
        vec![HostMaterialInput {
            name: "api".into(),
            expected_sha256: hash(&bytes),
            bytes,
        }],
        vec![],
        c,
    )
    .unwrap()
    .with_bindings(
        vec![HostBindingInput::Callable {
            declaration: HostMaterialRef {
                material: "api".into(),
                pointer: "/callables/0".into(),
            },
            rules: None,
        }],
        c,
    )
    .unwrap();
    let mut config = Emmyrc::default();
    config.runtime.version = emmylua_code_analysis::EmmyrcLuaVersion::Lua51;
    let mut limits = SourceLimits::research();
    if flow {
        limits = SourceLimits::default();
        limits.flow = Some(FlowBudget {
            max_millis: 30000,
            max_paths: max_paths.unwrap_or(FlowBudget::default().max_paths),
            ..FlowBudget::default()
        });
    }
    let files = [
        ("a.lua", first_source),
        ("bom.lua", "\u{feff}local text='你好'\nB=A\n"),
    ]
    .into_iter()
    .map(|(path, original)| {
        let bom = original.starts_with('\u{feff}');
        SourceFileInput {
            path: path.into(),
            original_utf8: original.into(),
            content_sha256: hash(original.as_bytes()),
            analysis_utf8: original.trim_start_matches('\u{feff}').into(),
            map: if bom {
                SourceMap::Utf8BomPrefix
            } else {
                SourceMap::Identity
            },
        }
    })
    .collect();
    SourceInputs::admit(
        SourceInputSpec {
            files,
            workspace_root: std::env::current_dir()
                .unwrap()
                .join("logical-sharing-test"),
            source_identity: json!({"kind":"sharing-regression","nested":{"z":0,"a":1}}),
            phase: "after_load".into(),
            ordered_occurrences: vec!["a.lua".into(), "bom.lua".into()],
            config,
            host,
            luac: LuacIdentity {
                version: "5.1.5".into(),
                platform: "windows-amd64".into(),
                executable_sha256: "a".repeat(64),
                executable_bytes: 1,
            },
            limits,
        },
        c,
    )
    .unwrap()
}

include!("graph_move_tests.rs");
include!("scope_copy_tests.rs");
include!("harvest_evidence_tests.rs");

fn assert_identity(i: &SourceInputs, c: &OperationControl) {
    c.check().unwrap();
    let actual = serde_json::to_vec(i.identity()).unwrap();
    assert_eq!(hash(&actual), i.identity_sha256());
    assert_eq!(serde_json::from_slice::<Value>(&actual).unwrap(), serde_json::to_value(i.identity()).unwrap());
}
fn assert_graph(graph: &Graph, c: &OperationControl, scope: Option<&str>) {
    let i = graph.inputs();
    assert_identity(i, c);
    let bytes = serde_json::to_vec(graph.snapshot_inputs.as_ref()).unwrap();
    assert_eq!(graph.snapshot_inputs_json.as_bytes(), bytes);
    assert_eq!(graph.snapshot_key, hash(&bytes));
    let value:Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["evaluator"], "native-invocations");
    if let Some(scope) = scope { assert_eq!(value["validationScope"], scope); }
    assert!(Arc::ptr_eq(&graph.snapshot_inputs.admission, &i.identity));
    assert!(Arc::ptr_eq(&graph.snapshot_inputs.common, &i.graph_common));
}

#[test]
fn public_engine_text_is_copied_and_host_and_evidence_remain_shared() {
    let c = control();
    let input = inputs(&c, false);
    assert_identity(&input, &c);
    let prepared = prepare(input.clone(), &c).unwrap();
    for file in &input.corpus.files {
        let actual = prepared
            .analysis
            .db
            .get_file_text(prepared.files[&file.path])
            .unwrap();
        assert_ne!(actual.as_ptr(), file.analysis.as_ptr());
        assert_eq!(actual, file.analysis.as_ref());
    }
    let binding = input.host.bindings()[0].clone();
    assert!(Arc::ptr_eq(&binding.data, &input.host.bindings()[0].data));
    assert!(std::ptr::eq(
        binding.descriptor().unwrap(),
        input.host.bindings()[0].descriptor().unwrap()
    ));
    let original_binding = serde_json::to_value(binding.data.as_ref()).unwrap();
    assert_eq!(serde_json::to_value(&binding).unwrap(), original_binding);
    let base = prepared.build_plan("base", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &c)
        .unwrap();
    assert_graph(&base, &c, None);
    let duplicate = prepared
        .build_plan(
            "duplicate",
            "after_load",
            LoadPlan::Known(vec!["a.lua".into(), "a.lua".into(), "bom.lua".into()]),
            None,
            Some("test-scope"),
            &c,
        )
        .unwrap();
    assert_graph(&duplicate, &c, Some("test-scope"));
    assert_ne!(base.snapshot_key, duplicate.snapshot_key);
    assert_eq!(duplicate.inputs.ordered, vec!["a.lua", "a.lua", "bom.lua"]);
    for graph in [&base, &duplicate] {
        assert!(Arc::ptr_eq(&input.corpus, &graph.inputs.corpus));
        assert!(Arc::ptr_eq(&input.host.data, &graph.inputs.host.data));
        assert!(Arc::ptr_eq(
            &input.identity.common,
            &graph.inputs.identity.common
        ));
        assert!(Arc::ptr_eq(&input.shared_steps, &graph.inputs.shared_steps));
        for row in &graph.host_runtime.as_ref().unwrap().bindings {
            assert!(Arc::ptr_eq(&row.declaration.data, &binding.data));
        }
    }
    let unknown = prepared
        .build_plan(
            "unknown",
            "unknown",
            LoadPlan::Unknown(vec!["a.lua".into(), "bom.lua".into()]),
            None,
            None,
            &c,
        )
        .unwrap();
    assert_graph(&unknown, &c, None);
    let request = EntryRequest {
        candidate: prepared.entry_catalog[0].id.clone(),
        load_occurrence: None,
        creation_context: None,
        arguments: EntryArguments::Exact(vec![EntryArgument::Boolean(true)]),
    };
    let entry = prepared
        .build_plan(
            "entry",
            "after_load",
            LoadPlan::Known(vec!["a.lua".into(), "bom.lua".into()]),
            Some(request),
            None,
            &c,
        )
        .unwrap();
    assert_graph(&entry, &c, None);
    let xml = crate::xml_load::Program::from_sources(
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
        .build_xml_plan("xml", Arc::new(xml), None, &c)
        .unwrap();
    assert_graph(&xml, &c, Some(crate::xml_load::RULES));
    assert_eq!(xml.inputs.ordered, vec!["a.lua", "a.lua"]);
    let flow = inputs(&c, true);
    assert_identity(&flow, &c);
}

#[test]
fn shared_corpus_does_not_share_request_ownership_or_cancellation() {
    let c = control();
    let input = inputs(&c, false);
    let mut prepared = prepare(input.clone(), &c).unwrap();
    let old = prepared.build_plan("old", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &c)
        .unwrap();
    let next = control();
    prepared.begin_research_request(&next).unwrap();
    assert!(Arc::ptr_eq(&prepared.inputs.corpus, &old.inputs.corpus));
    assert!(Arc::ptr_eq(
        &prepared.inputs.host.data,
        &old.inputs.host.data
    ));
    assert!(!Arc::ptr_eq(
        &prepared.inputs.shared_steps,
        &old.inputs.shared_steps
    ));
    assert!(!Arc::ptr_eq(&prepared.owner, &old.owner));
    assert!(require_same_control(&old.inputs.host.admission_control, &c).is_ok());
    assert!(require_same_control(&prepared.inputs.host.admission_control, &next).is_ok());
    assert!(
        prepared
            .source_queries(&next)
            .unwrap()
            .with_execution(&old, QuerySelector::AllOccurrences, &next)
            .is_err()
    );
    assert!(
        prepared
            .freeze(&old, NativeViewSelection::SourceTemplates, &next)
            .is_err()
    );
    let fresh = prepared.build_plan("fresh", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &next)
        .unwrap();
    assert_eq!(old.snapshot_key, fresh.snapshot_key);
    assert!(
        prepared
            .source_queries(&next)
            .unwrap()
            .with_execution(&fresh, QuerySelector::AllOccurrences, &next)
            .is_ok()
    );
    let signal = lycheedev_core::control::TrackedCancellation::new();
    signal.cancel();
    let cancelled = OperationControl::tracked(next.deadline(), signal);
    assert!(prepared.begin_research_request(&cancelled).is_err());
    assert!(require_same_control(&prepared.control, &next).is_ok());
}

#[test]
fn public_emmy_batch_matches_upstream_facts_and_propagates_owned_checkpoints() {
    use lycheedev_emmy_adapter::{IngestionFailure, ingest_public_batch};
    let root = std::env::current_dir().unwrap().join("logical-public-emmy");
    let path = root.join("a.lua");
    let text: Arc<str> = Arc::from("local a=1\nreturn a\n");
    let mut actual = EmmyLuaAnalysis::new();
    let mut ordinary = EmmyLuaAnalysis::new();
    for analysis in [&mut actual, &mut ordinary] {
        analysis.add_main_workspace(root.clone());
    }
    let batch = ingest_public_batch(&mut actual, vec![(path.clone(), Some(text.clone()))], text.len(), || Ok::<_, &str>(())).unwrap();
    let expected = ordinary.update_files_by_path(vec![(path.clone(), Some(text.to_string()))]);
    assert_eq!(batch.file_ids, expected);
    assert_eq!(batch.text_cost.retained_bytes, text.len());
    assert_eq!(batch.text_cost.largest_file_bytes, text.len());
    assert_eq!(actual.db.get_file_text(batch.file_ids[0]), Some(text.as_ref()));
    assert_ne!(actual.db.get_file_text(batch.file_ids[0]).unwrap().as_ptr(), text.as_ptr());
    assert_eq!(actual.semantic_model(batch.file_ids[0]).file_facts(), ordinary.semantic_model(expected[0]).file_facts());
    ingest_public_batch(&mut actual, vec![(path.clone(), None)], text.len(), || Ok::<_, &str>(())).unwrap();
    assert!(actual.db.get_file_text(batch.file_ids[0]).is_none());
    let mut completed_checks = 0usize;
    ingest_public_batch(&mut EmmyLuaAnalysis::new(), vec![(path.clone(), Some(text.clone()))], text.len(), || { completed_checks += 1; Ok::<_, &str>(()) }).unwrap();
    for stop in 1..=completed_checks {
        let mut interrupted = EmmyLuaAnalysis::new();
        let mut checks = 0;
        let result = ingest_public_batch(&mut interrupted, vec![(path.clone(), Some(text.clone()))], text.len(), || {
            checks += 1;
            if checks == stop { Err("original cancellation") } else { Ok(()) }
        });
        assert!(matches!(result, Err(IngestionFailure::Interrupted("original cancellation"))));
        assert_eq!(checks, stop);
        // This checks actual owned boundaries, not cancellation inside the
        // synchronous public batch. Interrupted analyses are discarded.
    }
}


#[test]
fn entry_line_lookup_preserves_typed_stop_causes() {
    // Exercise real prepared inputs; removing the optional cache selects the
    // same production fallback used for allocation/cap exhaustion.
    for fallback in [false, true] {
        for deadline in [false, true] {
            let c = control();
            let mut prepared = prepare(inputs(&c, false), &c).unwrap();
            let syntax = *prepared.closures.values().next().unwrap();
            let before = prepared.entry_candidate("a.lua", syntax).unwrap();
            if fallback { prepared.original_lines.clear(); }
            assert_eq!(prepared.entry_candidate("a.lua", syntax).unwrap(), before);
            assert!(matches!(prepared.entry_candidates("a.lua", 1, &c).unwrap(), EntryResolution::Unique { .. }));

            let stopped = stopped_control(deadline, Instant::now() + Duration::from_secs(30));
            prepared.control = stopped.clone();
            let expected = stopped.check().unwrap_err();
            // This private producer is the exact boundary that formerly
            // stringified line-lookup errors before public input_error mapping.
            let actual = prepared.entry_candidate("a.lua", syntax).unwrap_err();
            assert_eq!((actual.code, actual.exit_code), (expected.code.clone(), expected.exit_code));
            let actual = prepared.entry_candidates("a.lua", 1, &stopped).unwrap_err();
            assert_eq!((actual.code, actual.exit_code), (expected.code, expected.exit_code));
        }
    }

    // Trigger a real typed cancellation only after fallback has entered its
    // scan. No sleeps, timing race, stub control, or production test hook.
    let token = lycheedev_core::control::TrackedCancellation::new();
    let c = OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let text = "x\n".repeat(16384);
    let mut polls = 0;
    let actual = original_lines::controlled_line_at(&text, text.len(), || {
        polls += 1;
        if polls == 2 { token.cancel(); }
        c.check()
    }).unwrap_err();
    let expected = c.check().unwrap_err();
    assert_eq!(polls, 2);
    assert_eq!((actual.code, actual.exit_code), (expected.code, expected.exit_code));
}

#[test]
fn incoming_caller_missing_owner_is_explicit_and_cancellation_wins() {
    let control=control();
    let input=graph_fixture_inputs(&control,false,"function Caller() Host() end\n",None);
    let prepared=prepare(input,&control).unwrap();
    let mut syntax=prepared.file_syntax(prepared.inputs.corpus.index["a.lua"]).unwrap();
    let reference=syntax.references.iter().find(|reference|reference.roles.contains(&ReferenceRole::Call)).unwrap().clone();
    assert!(reference.closure.is_some());syntax.callable_owners.clear();
    let origins=crate::native_research::XmlResearch::prepare(&mut BTreeMap::new(),BTreeMap::new(),&[],&json!({}),&control).unwrap();
    let mut row=json!({"source":"Host","target":"Host","kind":"call","proof":"unchanged"});
    prepared.project_incoming_caller(&mut row,&syntax,&reference,&origins).unwrap();
    assert_eq!(row["source"],"<unknown>");assert_eq!(row["caller"]["kind"],"unknown");assert!(row["caller"].get("name").is_none());
    assert_eq!(row["referenceExpression"],"Host");assert_eq!(row["proof"],"unchanged");
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(prepared.project_incoming_caller(&mut row,&syntax,&reference,&origins).unwrap_err().code,"command.cancelled");
}

// Defense-in-depth tests use the actual admitted parser and production name
// predicate directly. ResearchSession still rejects these malformed documents;
// this is not a fabricated successful native snapshot or product relation run.
#[test]
fn incoming_function_binding_rejects_parser_recovery_names() {
    use emmylua_parser::{LuaLanguageLevel,LuaParser,ParserConfig,LuaParseErrorKind};
    let cases=[
        "function Obj.() Target() end",
        "function Obj:() Target() end",
        "function Obj[key]() Target() end",
        "function Obj().Run() Target() end",
        "function Obj()() Target() end",
        "function Obj a) Target() end",
        "function Obj(a Target() end",
        "local function Obj.() Target() end",
        "local function Obj[key]() Target() end",
        "local function Obj()() Target() end",
        "function BodyBroken() local x = ; Target() end",
    ];
    for malformed in cases {
        let source=format!("function HealthyBefore() Target() end\nfunction Outer()\n{malformed}\nend\nfunction HealthyAfter() Target() end\n");
        let tree=LuaParser::parse(&source,ParserConfig::with_level(LuaLanguageLevel::Lua51));
        assert!(tree.has_syntax_errors(),"fixture must exercise real recovery: {malformed}");
        let root=tree.get_red_root();
        let errors=tree.get_errors().iter().filter(|error|error.kind==LuaParseErrorKind::SyntaxError).map(|error|error.range.start()).collect::<std::collections::BTreeSet<_>>();
        let call_sites=||root.descendants().filter_map(emmylua_parser::LuaCallExpr::cast).filter(|call|{
            matches!(call.get_prefix_expr(),Some(LuaExpr::NameExpr(name)) if name.get_name_text().as_deref()==Some("Target"))
        }).map(|call|call.get_range()).collect::<Vec<_>>();
        let before=call_sites();assert_eq!(before.len(),3,"recovery must retain each original Target call: {malformed}");
        let control=control();
        if ["function Obj.()","function Obj:()","function Obj[key]()","function Obj a)"].iter().any(|prefix|malformed.starts_with(prefix)) {
            let offset=source.find(malformed).unwrap();
            let stat=root.descendants().filter_map(LuaStat::cast).find(|stat|usize::from(stat.get_range().start())==offset).unwrap();
            assert!(PreparedSnapshot::research_function_binding(&control,stat,&std::collections::BTreeSet::new()).unwrap().is_none(),"shape/header authentication is independently conservative: {malformed}");
        }
        let mut named=Vec::new();let mut unbound=0;
        for stat in root.descendants().filter_map(LuaStat::cast).filter(|stat|matches!(stat,LuaStat::FuncStat(_)|LuaStat::LocalFuncStat(_))) {
            match PreparedSnapshot::research_function_binding(&control,stat,&errors).unwrap() {
                Some((_,name))=>named.push(source[usize::from(name.start())..usize::from(name.end())].to_string()),
                None=>unbound+=1,
            }
        }
        assert_eq!(named,["HealthyBefore","HealthyAfter"],"neither recovered name nor outer name is proven: {malformed}");
        assert_eq!(unbound,2,"malformed function and containing declaration conservatively remain unbound");
        assert_eq!(call_sites(),before,"name projection cannot remove, reorder, or mutate recovered syntax calls");
        let middle=before[1];
        let call=root.descendants().filter_map(emmylua_parser::LuaCallExpr::cast).find(|call|call.get_range()==middle).unwrap();
        let nearest=call.syntax().ancestors().find_map(LuaClosureExpr::cast).unwrap();
        let stat=nearest.syntax().parent().and_then(LuaStat::cast).unwrap();
        assert!(PreparedSnapshot::research_function_binding(&control,stat,&errors).unwrap().is_none(),"do not inherit an outer callable for a recovered nearest closure");
    }
}

#[test]
fn incoming_function_binding_accepts_complete_headers_and_honors_control() {
    use emmylua_parser::{LuaLanguageLevel,LuaParser,ParserConfig};
    let source="function Plain(a, ...) Target(a) end\nfunction Obj.Deep:Method(a) Target(a) end\nlocal function Local() Target() end\nfunction Commented -- header comment\n(a) Target(a) end\n";
    let tree=LuaParser::parse(source,ParserConfig::with_level(LuaLanguageLevel::Lua51));
    assert!(!tree.has_syntax_errors());
    let root=tree.get_red_root();let errors=std::collections::BTreeSet::new();let control=control();
    let stats=root.descendants().filter_map(LuaStat::cast).filter(|stat|matches!(stat,LuaStat::FuncStat(_)|LuaStat::LocalFuncStat(_))).collect::<Vec<_>>();
    let names=stats.iter().cloned().map(|stat|{
        let(_,name)=PreparedSnapshot::research_function_binding(&control,stat,&errors).unwrap().expect("complete static function declaration");
        &source[usize::from(name.start())..usize::from(name.end())]
    }).collect::<Vec<_>>();
    assert_eq!(names,["Plain","Obj.Deep:Method","Local","Commented"]);
    control.tracked_cancel_handle().unwrap().cancel();
    assert!(PreparedSnapshot::research_function_binding(&control,stats[0].clone(),&errors).is_err());
}
