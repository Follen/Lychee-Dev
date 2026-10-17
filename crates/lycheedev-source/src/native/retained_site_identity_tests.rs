//! Exact retained closure/parameter identities, compared with the old String keys.
use super::*;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

fn fixture() -> PreparedSnapshot {
    let control = OperationControl::new(
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(false)),
    );
    let host = TrustedHostEnvironment::from_host_material(
        "retained-site-identity-test".into(),
        false,
        vec![],
        vec![],
        &control,
    )
    .unwrap();
    let files = [
        ("dir/same.lua", "function F(x) return x end\nlocal value=F(1)\n"),
        ("other/same.lua", "function F(x) return x end\nlocal value=F(1)\n"),
        (
            "bom.lua",
            "\u{feff}-- 你好 😀\r\nfunction B(y) return y end\r\nlocal obj={}\r\nfunction obj:Run(z) return z end\r\nobj:Run(3)\r\n",
        ),
        ("empty.lua", ""),
    ]
    .into_iter()
    .map(|(path, text)| SourceFileInput {
        path: path.into(),
        original_utf8: text.into(),
        content_sha256: hash(text.as_bytes()),
        analysis_utf8: text.strip_prefix('\u{feff}').unwrap_or(text).into(),
        map: if text.starts_with('\u{feff}') {
            SourceMap::Utf8BomPrefix
        } else {
            SourceMap::Identity
        },
    })
    .collect();
    let inputs = SourceInputs::admit(
        SourceInputSpec {
            files,
            workspace_root: std::env::current_dir().unwrap().join("retained-site-fixture"),
            source_identity: json!({"test":"retained-site-identity"}),
            phase: "after_load".into(),
            ordered_occurrences: vec!["dir/same.lua".into(), "other/same.lua".into(), "bom.lua".into(), "empty.lua".into()],
            config: crate::engine_config::research(),
            host,
            luac: LuacIdentity {
                version: "5.1.5".into(),
                platform: "windows-amd64".into(),
                executable_sha256: "a".repeat(64),
                executable_bytes: 1,
            },
            limits: SourceLimits::research(),
        },
        &control,
    )
    .unwrap();
    prepare(inputs, &control).unwrap()
}

fn previous_maps(prepared: &PreparedSnapshot) -> (BTreeMap<String, LuaSyntaxId>, BTreeMap<String, SemanticId>) {
    let mut closures = BTreeMap::new();
    let mut parameters = BTreeMap::new();
    // Replay the exact former production insertion order and complete values.
    for source in &prepared.inputs.corpus.files {
        let model = prepared.analysis.semantic_model(prepared.files[&source.path]);
        for closure in model.syntax_tree().unwrap().get_red_root().descendants().filter_map(LuaClosureExpr::cast) {
            closures.insert(prepared.inputs.site(&source.path, closure.get_range()).id(), closure.get_syntax_id());
        }
        for declaration in &model.file_facts().unwrap().decls {
            if matches!(declaration.kind, emmylua_code_analysis::DeclKind::Param) {
                parameters.insert(prepared.inputs.site(&source.path, declaration.name_range).id(), declaration.id.clone());
            }
        }
    }
    (closures, parameters)
}

#[test]
fn compact_retained_maps_match_previous_exact_lookup_for_all_original_ranges() {
    let prepared = fixture();
    let (closures, parameters) = previous_maps(&prepared);
    assert_eq!(prepared.closures.len(), closures.len());
    assert_eq!(prepared.parameters.len(), parameters.len());
    let identity = prepared.inputs.identity_sha256().to_owned();
    let catalog = serde_json::to_vec(&prepared.entry_catalog).unwrap();
    let steps = prepared.inputs.shared_steps.load(Ordering::Relaxed);
    for source in &prepared.inputs.corpus.files {
        for start in 0..=source.original.len() as u32 {
            for end in start..=source.original.len() as u32 {
                let site = Site { file: source.path.clone(), start, end };
                assert_eq!(prepared.closure_syntax(&site), closures.get(&site.id()).copied(), "{site:?}");
                assert_eq!(prepared.parameter_identity(&site), parameters.get(&site.id()), "{site:?}");
            }
        }
    }
    assert_eq!(prepared.inputs.identity_sha256(), identity);
    assert_eq!(serde_json::to_vec(&prepared.entry_catalog).unwrap(), catalog);
    assert_eq!(prepared.inputs.shared_steps.load(Ordering::Relaxed), steps);
}

#[test]
fn compact_retained_maps_keep_last_duplicate_and_complete_values() {
    let mut prepared = fixture();
    let syntaxes = prepared.closures.values().copied().collect::<Vec<_>>();
    let semantics = prepared.parameters.values().cloned().collect::<Vec<_>>();
    assert!(syntaxes.windows(2).any(|pair| pair[0] != pair[1]));
    assert_ne!(semantics[0], semantics[1]);
    prepared.closures.clear();
    prepared.parameters.clear();
    let mut old_closures = BTreeMap::new();
    let mut old_parameters = BTreeMap::new();
    let sites = [
        Site { file: "dir/same.lua".into(), start: 3, end: 9 },
        Site { file: "other/same.lua".into(), start: 3, end: 9 },
        Site { file: "dir/same.lua".into(), start: 3, end: 9 },
        Site { file: "dir/same.lua".into(), start: 0, end: 0 },
        Site { file: "dir/same.lua".into(), start: u32::MAX, end: u32::MAX },
        Site { file: "other/same.lua".into(), start: 3, end: 9 },
    ];
    for (i, site) in sites.iter().enumerate() {
        let syntax = syntaxes[i % syntaxes.len()];
        let semantic = semantics[i % semantics.len()].clone();
        let key = SyntaxSite::from_site(&prepared.inputs, site).unwrap();
        assert_eq!(prepared.closures.insert(key, syntax), old_closures.insert(site.id(), syntax));
        assert_eq!(prepared.parameters.insert(key, semantic.clone()), old_parameters.insert(site.id(), semantic));
    }
    for site in &sites {
        assert_eq!(prepared.closure_syntax(site), old_closures.get(&site.id()).copied());
        assert_eq!(prepared.parameter_identity(site), old_parameters.get(&site.id()));
    }
    assert_eq!(prepared.closures.len(), 4);
    assert_eq!(prepared.parameters.len(), 4);
    // The retained key has no path/String owner or drop allocation.
    fn require_copy<T: Copy>() {}
    require_copy::<SyntaxSite>();
    assert!(!std::mem::needs_drop::<SyntaxSite>());
}

#[test]
fn compact_retained_lookup_uses_exact_admitted_path_without_id_parsing() {
    let prepared = fixture();
    let key = *prepared.parameters.keys().find(|key| key.file == 0).unwrap();
    let mut site = key.public(&prepared.inputs);
    let first = prepared.parameter_identity(&site).unwrap().clone();
    site.file = "other/same.lua".into();
    assert_ne!(prepared.parameter_identity(&site).unwrap(), &first);
    let steps = prepared.inputs.shared_steps.load(Ordering::Relaxed);
    for file in ["same.lua", "Dir/same.lua", "dir\\same.lua", "dir/same.lua:3:9", "dir:3/same.lua", "missing.lua", ""] {
        site.file = file.into();
        assert_eq!(prepared.closure_syntax(&site), None);
        assert_eq!(prepared.parameter_identity(&site), None);
    }
    // Colon paths remain outside the admission contract. A lookup must never
    // reinterpret their trailing components as a Site::id range or an alias.
    assert!(!crate::git::source_path("dir:3/same.lua"));
    assert_eq!(prepared.inputs.shared_steps.load(Ordering::Relaxed), steps);
}

#[test]
fn compact_retained_bom_keys_use_original_offsets_and_analysis_values() {
    let prepared = fixture();
    let source = prepared.inputs.file("bom.lua").unwrap();
    let file = prepared.files[&source.path];
    let model = prepared.analysis.semantic_model(file);
    let mut closure_count = 0;
    for closure in model.syntax_tree().unwrap().get_red_root().descendants().filter_map(LuaClosureExpr::cast) {
        let range = closure.get_range();
        let original = prepared.inputs.site(&source.path, range);
        assert_eq!(original.start, u32::from(range.start()) + 3);
        assert_eq!(original.end, u32::from(range.end()) + 3);
        assert_eq!(prepared.closure_syntax(&original), Some(closure.get_syntax_id()));
        assert_eq!(prepared.closure_syntax(&Site::at(&source.path, range)), None);
        closure_count += 1;
    }
    let mut parameter_count = 0;
    for declaration in &model.file_facts().unwrap().decls {
        if matches!(declaration.kind, emmylua_code_analysis::DeclKind::Param) {
            let original = prepared.inputs.site(&source.path, declaration.name_range);
            assert_eq!(original.start, u32::from(declaration.name_range.start()) + 3);
            assert_eq!(prepared.parameter_identity(&original), Some(&declaration.id));
            assert_eq!(prepared.parameter_identity(&Site::at(&source.path, declaration.name_range)), None);
            parameter_count += 1;
        }
    }
    assert_eq!(closure_count, 2);
    assert!(parameter_count >= 2);
}

#[test]
fn compact_retained_maps_preserve_native_function_and_parameter_descriptors() {
    let prepared = fixture();
    let (closures, parameters) = previous_maps(&prepared);
    let graph = prepared.build("retained-site-values", EvaluatorProfile::Native, None, &prepared.control).unwrap();
    let before = serde_json::to_vec(&graph.facts).unwrap();
    let steps = prepared.inputs.shared_steps.load(Ordering::Relaxed);
    let mut functions = 0;
    let mut formal_parameters = 0;
    for value in &graph.values {
        if !matches!(value.kind, ValueKind::Function(_) | ValueKind::NativeParameter(_)) { continue; }
        let BindingDecision::Bound(bound) = prepared.native_value_decision(&graph, &Decision::Bound(value.id), None, "exact-proof").unwrap() else {
            panic!("source function/parameter must retain its descriptor");
        };
        assert_eq!(bound.identity, format!("{}:value:{}", graph.snapshot_key, value.id));
        assert_eq!(bound.proof, "exact-proof");
        match (&value.kind, bound.value) {
            (ValueKind::Function(id), SelectedValue::Function(actual)) => {
                let function = &graph.functions[*id];
                assert_eq!(actual, FunctionRef { file_id: prepared.files[&function.closure.file], closure: closures[&function.closure.id()], declaration: None });
                functions += 1;
            }
            (ValueKind::NativeParameter(site), SelectedValue::Parameter(actual)) => {
                assert_eq!(actual, parameters[&site.id()]);
                formal_parameters += 1;
            }
            _ => panic!("changed retained descriptor variant"),
        }
    }
    assert!(functions >= 4);
    assert!(formal_parameters >= 4);
    assert_eq!(serde_json::to_vec(&graph.facts).unwrap(), before);
    assert_eq!(prepared.inputs.shared_steps.load(Ordering::Relaxed), steps);
}

#[test]
fn compact_retained_lookup_preserves_explicit_entry_selection_and_public_catalog() {
    let prepared = fixture();
    let candidate = prepared.entry_catalog.iter().find(|candidate| candidate.closure.file == "dir/same.lua").unwrap().clone();
    let (closures, _) = previous_maps(&prepared);
    let expected = prepared.entry_candidate(&candidate.closure.file, closures[&candidate.closure.id()]).unwrap();
    assert_eq!(candidate, expected);
    let catalog = serde_json::to_vec(prepared.entry_catalog(&prepared.control).unwrap()).unwrap();
    let graph = prepared.build_plan(
        "retained-site-entry", "after_load", LoadPlan::Known(prepared.inputs.ordered.clone()),
        Some(EntryRequest { candidate: candidate.id.clone(), load_occurrence: None, creation_context: None, arguments: EntryArguments::Exact(vec![EntryArgument::Number(7.0)]) }),
        None, &prepared.control,
    ).unwrap();
    let facts = graph.native_invocations.as_ref().unwrap();
    assert_eq!(facts.entries.len(), 1);
    let entry = &facts.entries[0];
    assert_eq!(entry.request.candidate, candidate.id);
    assert_eq!(graph.functions[facts.closures[entry.closure].function].closure, candidate.closure);
    assert!(entry.effects_complete);
    assert_eq!(serde_json::to_vec(prepared.entry_catalog(&prepared.control).unwrap()).unwrap(), catalog);
}

#[test]
fn compact_retained_lookup_preserves_public_stop_causes_without_extra_charges() {
    for deadline in [false, true] {
        let mut prepared = fixture();
        let graph = prepared.build("retained-site-stop", EvaluatorProfile::Native, None, &prepared.control).unwrap();
        let site = prepared.entry_catalog[0].closure.clone();
        let syntax = prepared.closure_syntax(&site).unwrap();
        let stopped = OperationControl::new(
            if deadline { Instant::now() - Duration::from_secs(1) } else { prepared.control.deadline() },
            Arc::new(AtomicBool::new(!deadline)),
        );
        prepared.control = stopped.clone();
        let expected = serde_json::to_value(stopped.check().unwrap_err()).unwrap();
        let steps = prepared.inputs.shared_steps.load(Ordering::Relaxed);
        // The previous internal map lookup did not check/charge. Preserve that;
        // the existing public operation boundaries still enforce typed stops.
        assert_eq!(prepared.closure_syntax(&site), Some(syntax));
        let error = prepared.build_plan("stopped", "after_load", LoadPlan::Known(vec![]), None, None, &stopped).err().unwrap();
        assert_eq!(serde_json::to_value(error).unwrap(), expected);
        let error = prepared.freeze(&graph, NativeViewSelection::SourceTemplates, &stopped).err().unwrap();
        assert_eq!(serde_json::to_value(error).unwrap(), expected);
        let error = prepared.entry_catalog(&stopped).unwrap_err();
        assert_eq!(serde_json::to_value(error).unwrap(), expected);
        assert_eq!(prepared.inputs.shared_steps.load(Ordering::Relaxed), steps);
    }
}
