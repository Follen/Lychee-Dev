// The former eager raw-row formula remains independent of the compact row
// serializer. Existing graph/scope reference files are deliberately unchanged.
fn eager_harvest_rows(prepared: &PreparedSnapshot, names: &[&str]) -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    let mut nodes = 0usize;
    for name in names {
        prepared
            .control
            .check()
            .map_err(|error| error.to_string())?;
        let model = prepared.analysis.semantic_model(prepared.files[*name]);
        let tree = model.syntax_tree().ok_or("tree")?;
        if !model.parse_errors().unwrap_or_default().is_empty() {
            return Err(format!("fixture parse error: {name}"));
        }
        for node in tree.get_red_root().descendants() {
            nodes += 1;
            prepared
                .control
                .check()
                .map_err(|error| error.to_string())?;
            if nodes > prepared.inputs.corpus.limits.ast_nodes {
                return Err("AST node budget exceeded".into());
            }
            if let Some(expr) = LuaNameExpr::cast(node) {
                let site = prepared.inputs.site(name, expr.get_range());
                let raw = model.resolve_name(expr.get_position());
                rows.push(json!({"site":site,"rawIdentity":format!("{raw:?}"),"usedFor":"lexical-local-identity-only; global/value selection excluded"}));
            }
        }
    }
    Ok(rows)
}

#[test]
fn compact_harvest_rows_preserve_original_json_bytes_order_and_engine_independence() {
    let c = control();
    let source = "local captured=1\nlocal function Outer(x) local x=x; return function(y) return captured+x+y end end\nM={}\nfunction M:Run(arg) return self,Outer(arg),Unknown end\nlocal name='引号\\\"和反斜杠\\\\'\n";
    let prepared = prepare(graph_fixture_inputs(&c, false, source, None), &c).unwrap();
    let names = ["a.lua", "bom.lua"];
    let before = prepared.inputs.shared_steps.load(Ordering::Relaxed);
    let mut compact = harvest(&prepared.analysis, &names, &prepared.inputs, &c).unwrap();
    let expected = eager_harvest_rows(&prepared, &names).unwrap();
    assert!(!expected.is_empty());
    let bytes = serde_json::to_vec(&expected).unwrap();
    assert_eq!(serde_json::to_vec(&compact.raw).unwrap(), bytes);
    assert_eq!(serde_json::to_value(&compact.raw).unwrap(), json!(expected));
    assert_eq!(prepared.inputs.shared_steps.load(Ordering::Relaxed), before);
    for file in names {
        let rows = compact
            .raw
            .iter()
            .filter(|row| row.file.as_ref() == file)
            .collect::<Vec<_>>();
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|row| Arc::ptr_eq(&row.file, &rows[0].file)));
    }
    // Own the exact resolution IDs, not syntax nodes or model borrows. Debug
    // output must remain identical after dropping both harvest AST and engine.
    let rows = std::mem::take(&mut compact.raw);
    drop(compact);
    drop(prepared);
    assert_eq!(serde_json::to_vec(&rows).unwrap(), bytes);
    assert_eq!(serde_json::to_value(&rows).unwrap(), json!(expected));
    assert!(std::mem::size_of::<RawEmmyEvidence>() <= 48);
}

#[test]
fn compact_harvest_preserves_preexisting_cancellation_and_serialization_boundary() {
    let c = control();
    let prepared = prepare(inputs(&c, false), &c).unwrap();
    let names = ["a.lua", "bom.lua"];
    let compact = harvest(&prepared.analysis, &names, &prepared.inputs, &c).unwrap();
    let expected = eager_harvest_rows(&prepared, &names).unwrap();
    let bytes = serde_json::to_vec(&expected).unwrap();
    for limit in [1, bytes.len() / 2, bytes.len() - 1, bytes.len()] {
        assert_eq!(
            serialize_json(&compact.raw, &c, limit).map_err(|error| (
                error.code,
                error.message,
                error.exit_code
            )),
            serialize_json(&expected, &c, limit).map_err(|error| (
                error.code,
                error.message,
                error.exit_code
            )),
        );
    }
    let before = prepared.inputs.shared_steps.load(Ordering::Relaxed);
    c.cancel_handle().store(true, Ordering::Release);
    let expected_error = c.check().unwrap_err().to_string();
    assert_eq!(
        harvest(&prepared.analysis, &names, &prepared.inputs, &c).err(),
        Some(expected_error.clone())
    );
    assert_eq!(
        eager_harvest_rows(&prepared, &names).err(),
        Some(expected_error)
    );
    assert_eq!(prepared.inputs.shared_steps.load(Ordering::Relaxed), before);
}

#[test]
fn compact_harvest_retains_every_public_semantic_id_debug_variant() {
    use emmylua_code_analysis::{SemanticId, TypeScope};
    use emmylua_parser::LuaSyntaxKind;
    let file_id = FileId::new(42);
    let range = TextRange::new(7.into(), 19.into());
    let syntax = LuaSyntaxId::new(LuaSyntaxKind::ClosureExpr.into(), range);
    let identities = [
        None,
        Some(SemanticId::decl(file_id, range)),
        Some(SemanticId::member(file_id, range)),
        Some(SemanticId::type_def(TypeScope::Global, "全局.类型".into())),
        Some(SemanticId::type_def(
            TypeScope::File(file_id),
            "局部类型".into(),
        )),
        Some(SemanticId::signature(file_id, syntax)),
        Some(SemanticId::name("未知.名称\\\"\n".into())),
    ];
    let file: Arc<str> = Arc::from("路径/引用\"\\😀.lua");
    let site = Site {
        file: file.to_string(),
        start: 7,
        end: 19,
    };
    let expected = identities.iter().map(|raw| {
        json!({"site":site,"rawIdentity":format!("{raw:?}"),"usedFor":"lexical-local-identity-only; global/value selection excluded"})
    }).collect::<Vec<_>>();
    let compact = identities
        .into_iter()
        .map(|identity| RawEmmyEvidence::new(Arc::clone(&file), &site, identity))
        .collect::<Vec<_>>();
    let expected_bytes = serde_json::to_vec(&expected).unwrap();
    assert_eq!(serde_json::to_vec(&compact).unwrap(), expected_bytes);
    assert_eq!(serde_json::to_value(&compact).unwrap(), json!(expected));
    assert_eq!(
        serde_json::to_vec_pretty(&compact).unwrap(),
        serde_json::to_vec_pretty(&expected).unwrap()
    );
}

#[test]
fn packed_managed_sites_match_original_sets_for_every_admitted_syntax_identity() {
    let c = control();
    let source = "local captured=1\nfunction F(x) return (x+captured), Obj:Run(x), Obj.field end\nF(captured)\n";
    let prepared = prepare(graph_fixture_inputs(&c, false, source, None), &c).unwrap();
    let mut reads = HashSet::new();
    let mut calls = HashSet::new();
    let mut all = Vec::new();
    for file in prepared.files.values() {
        let model = prepared.analysis.semantic_model(*file);
        for node in model.syntax_tree().unwrap().get_red_root().descendants() {
            let site = BindingSite {
                file_id: *file,
                syntax: LuaSyntaxId::from_node(&node),
            };
            if let Some(expr) = LuaExpr::cast(node) {
                if matches!(expr, LuaExpr::NameExpr(_) | LuaExpr::IndexExpr(_)) {
                    reads.insert(site);
                }
                if matches!(expr, LuaExpr::CallExpr(_)) {
                    calls.insert(site);
                }
            }
            all.push(site);
        }
    }
    let before = prepared.inputs.shared_steps.load(Ordering::Relaxed);
    for site in &all {
        assert_eq!(
            prepared.managed_sites.contains_read(site),
            reads.contains(site)
        );
        assert_eq!(
            prepared.managed_sites.contains_call(site),
            calls.contains(site)
        );
        let foreign = BindingSite {
            file_id: FileId::VIRTUAL,
            ..*site
        };
        assert!(!prepared.managed_sites.contains_read(&foreign));
        assert!(!prepared.managed_sites.contains_call(&foreign));
    }
    assert_eq!(prepared.inputs.shared_steps.load(Ordering::Relaxed), before);
    // An empty known execution plan has no per-use decisions: every observed read
    // and call must still be Managed/Unavailable, not a raw-Emmy fallback.
    let graph = prepared
        .build_plan(
            "packed-managed-sites",
            "after_load",
            LoadPlan::Known(vec![]),
            None,
            None,
            &c,
        )
        .unwrap();
    let provider = prepared
        .freeze(&graph, NativeViewSelection::SourceTemplates, &c)
        .unwrap();
    assert!(Arc::ptr_eq(
        &provider.managed_sites,
        &prepared.managed_sites
    ));
    let before_queries = prepared.inputs.shared_steps.load(Ordering::Relaxed);
    for site in &all {
        match provider.resolve(*site) {
            BindingDecision::Unavailable { reason, .. } => {
                assert!(reads.contains(site));
                assert_eq!(reason, "managed_use_missing_from_bounded_graph");
            }
            BindingDecision::NotManaged => assert!(!reads.contains(site)),
            _ => panic!("empty graph selected an unexpected read"),
        }
        match provider.call(*site) {
            BindingDecision::Unavailable { reason, .. } => {
                assert!(calls.contains(site));
                assert_eq!(reason, "managed_call_missing_from_bounded_graph");
            }
            BindingDecision::NotManaged => assert!(!calls.contains(site)),
            _ => panic!("empty graph selected an unexpected call"),
        }
    }
    assert_eq!(
        prepared.inputs.shared_steps.load(Ordering::Relaxed),
        before_queries
    );
    c.cancel_handle().store(true, Ordering::Release);
    assert!(
        matches!(provider.resolve(all[0]), BindingDecision::Unavailable { reason, .. } if reason == "command.cancelled")
    );
    assert!(
        matches!(provider.call(all[0]), BindingDecision::Unavailable { reason, .. } if reason == "command.cancelled")
    );
}
