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
    c.tracked_cancel_handle().unwrap().cancel();
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
    c.tracked_cancel_handle().unwrap().cancel();
    assert!(
        matches!(provider.resolve(all[0]), BindingDecision::Unavailable { reason, .. } if reason == "command.cancelled")
    );
    assert!(
        matches!(provider.call(all[0]), BindingDecision::Unavailable { reason, .. } if reason == "command.cancelled")
    );
}

// Reconstruct the former two-string lookup directly from the same public Emmy
// resolution API. Do not derive the oracle from the new packed representation.
fn eager_lexical_lookups(prepared: &PreparedSnapshot, names: &[&str])
    -> (BTreeMap<String, String>, BTreeMap<String, Site>, Vec<Site>)
{
    let mut reads = BTreeMap::new();
    let mut declarations = BTreeMap::new();
    let mut sites = Vec::new();
    for name in names {
        let model = prepared.analysis.semantic_model(prepared.files[*name]);
        let facts = model.file_facts().unwrap();
        for declaration in &facts.decls {
            if declaration.kind.is_local() {
                let site = prepared.inputs.site(name, declaration.name_range);
                declarations.insert(site.id(), site);
            }
        }
        for node in model.syntax_tree().unwrap().get_red_root().descendants() {
            if let Some(expression) = LuaNameExpr::cast(node) {
                let site = prepared.inputs.site(name, expression.get_range());
                let identity = model.resolve_name(expression.get_position());
                if let Some(declaration) = identity.as_ref().and_then(|id| facts.decl_by_id(id))
                    .filter(|declaration| declaration.kind.is_local())
                {
                    reads.insert(site.id(), prepared.inputs.site(name, declaration.name_range).id());
                }
                sites.push(site);
            }
        }
    }
    (reads, declarations, sites)
}

#[test]
fn compact_lexical_lookup_preserves_all_local_capture_shadow_and_bom_identities() {
    let c = control();
    let source = "local captured=1\nlocal function Outer(x) local x=x; return function(y) return captured+x+y end end\nM={}\nfunction M:Run(arg) return self,Outer(arg),Unknown end\nlocal a,b=1,2\nlocal a=a+b\n";
    let prepared = prepare(graph_fixture_inputs(&c, false, source, None), &c).unwrap();
    let names = ["a.lua", "bom.lua"];
    let compact = harvest(&prepared.analysis, &names, &prepared.inputs, &c).unwrap();
    let (reads, declarations, sites) = eager_lexical_lookups(&prepared, &names);
    assert!(!reads.is_empty());
    assert!(sites.iter().any(|site| site.file == "bom.lua" && site.start >= 3));
    assert!(sites.iter().any(|site| !reads.contains_key(&site.id())));
    assert_eq!(compact.local_reads.len(), reads.len());
    assert_eq!(compact.declaration_sites.len(), declarations.len());
    for site in sites {
        assert_eq!(compact.local_read(&prepared.inputs, &site).as_ref(), reads.get(&site.id()));
    }
    for (id, site) in declarations {
        assert_eq!(compact.declaration_sites[&id].public(&prepared.inputs), site);
    }
    // Original file identity remains part of the key. Unknown paths cannot
    // alias an admitted file even if their byte ranges happen to be identical.
    let (site, _) = compact.local_reads.first_key_value().unwrap();
    let mut unknown = site.public(&prepared.inputs);
    unknown.file = "not-admitted.lua".into();
    assert_eq!(compact.local_read(&prepared.inputs, &unknown), None);
    // Complete graph consumers include captures, storage identities and exact
    // caller/parameter summaries observed by the production graph itself.
    let graph = prepared.build_plan("compact-lexical", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &c).unwrap();
    assert!(!graph.native_invocations.as_ref().unwrap().capture_facts.is_empty());
}

#[test]
fn compact_lexical_lookup_keeps_last_write_without_per_read_path_heaps() {
    let c = control();
    let source = "local first,second=1,2\nlocal a=first+second\nlocal b=first+second\n";
    let prepared = prepare(graph_fixture_inputs(&c, false, source, None), &c).unwrap();
    let names = ["a.lua", "bom.lua"];
    let mut compact = harvest(&prepared.analysis, &names, &prepared.inputs, &c).unwrap();
    let (reads, declarations, _) = eager_lexical_lookups(&prepared, &names);
    let removed_read_string_capacities: usize = reads.iter().map(|(key, value)| key.capacity() + value.capacity()).sum();
    let removed_site_path_capacities: usize = declarations.values().map(|site| site.file.capacity()).sum();
    assert!(removed_read_string_capacities > 0);
    assert!(removed_site_path_capacities > 0);
    // These are exact payload/owned-String bounds, not claims about allocator
    // arenas, B-tree node overhead or process private-byte reduction.
    assert_eq!(std::mem::size_of::<SyntaxSite>(), 16);
    assert!(std::mem::size_of::<(SyntaxSite, SyntaxSite)>() < std::mem::size_of::<(String, String)>());
    let (&key, &first) = compact.local_reads.first_key_value().unwrap();
    let second = *compact.declaration_sites.values().find(|site| **site != first).unwrap();
    let public = key.public(&prepared.inputs);
    let mut old = reads;
    old.insert(public.id(), second.public(&prepared.inputs).id());
    compact.local_reads.insert(key, second);
    assert_eq!(compact.local_read(&prepared.inputs, &public).as_ref(), old.get(&public.id()));
    compact.local_reads.insert(key, first);
    old.insert(public.id(), first.public(&prepared.inputs).id());
    assert_eq!(compact.local_read(&prepared.inputs, &public).as_ref(), old.get(&public.id()));
}

#[test]
fn repeated_research_scope_keeps_snapshot_bytes_lazy_and_releases_prior_scope() {
    let initial = control();
    let mut prepared = prepare(inputs(&initial, false), &initial).unwrap();
    let program = Arc::new(crate::xml_load::Program::from_sources("Main.toc", [
        ("Main.toc", b"a.lua\nbom.lua\n".as_slice()),
        ("a.lua", prepared.inputs.original("a.lua").unwrap().as_bytes()),
        ("bom.lua", prepared.inputs.original("bom.lua").unwrap().as_bytes()),
    ], &initial).unwrap());
    let mut expected = None;
    let mut expected_steps = None;
    for _ in 0..3 {
        let current = control();
        prepared.begin_research_request(&current).unwrap();
        let graph = prepared.build_xml_plan("same-scope", Arc::clone(&program), None, &current).unwrap();
        assert!(!graph.snapshot_inputs_json.is_materialized());
        let snapshot = Arc::downgrade(&graph.snapshot_inputs);
        let execution_identity = Arc::downgrade(&graph.inputs.identity);
        let steps = prepared.inputs.shared_steps.load(Ordering::Relaxed);
        if let Some(expected) = expected_steps { assert_eq!(steps, expected); }
        else { expected_steps = Some(steps); }
        // The public complete graph still emits the identical string and tree;
        // serialization is its first actual string consumer.
        let bytes = serde_json::to_vec(&graph.facts).unwrap();
        assert!(graph.snapshot_inputs_json.is_materialized());
        if let Some(expected) = &expected { assert_eq!(&bytes, expected); }
        else { expected = Some(bytes); }
        drop(graph);
        assert!(snapshot.upgrade().is_none());
        assert!(execution_identity.upgrade().is_none());
        assert_eq!(Arc::strong_count(&prepared.inputs), 1);
    }
    assert_eq!(Arc::strong_count(&program), 1);
}
