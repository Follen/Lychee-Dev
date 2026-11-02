// Current syntax index counting, admission and cancellation boundaries.
use emmylua_parser::{LuaLanguageLevel,LuaParser,ParserConfig,LuaSyntaxNode,LuaSyntaxKind};
use std::{sync::atomic::Ordering,time::{Duration,Instant}};

fn parsed(source:&str)->LuaSyntaxNode {
    LuaParser::parse(source,ParserConfig::with_level(LuaLanguageLevel::Lua51)).get_red_root()
}
fn observed(root:&LuaSyntaxNode)->SourceSyntaxCounts {
    let mut value=SourceSyntaxCounts::default();
    for node in root.descendants(){value.observe(&node);}
    value
}
fn check_control()->OperationControl {
    OperationControl::uncancelable(Instant::now()+Duration::from_secs(30))
}
fn prepared(sources:&[(&str,&str)])->PreparedSnapshot {
    let control=check_control();
    let host=TrustedHostEnvironment::from_host_material("syntax-count-test".into(),false,vec![],vec![],&control).unwrap();
    let mut config=Emmyrc::default();config.runtime.version=emmylua_code_analysis::EmmyrcLuaVersion::Lua51;
    let files=sources.iter().map(|(path,source)|SourceFileInput{
        path:(*path).into(),original_utf8:(*source).into(),analysis_utf8:source.strip_prefix('\u{feff}').unwrap_or(source).into(),
        content_sha256:hash(source.as_bytes()),map:if source.starts_with('\u{feff}'){SourceMap::Utf8BomPrefix}else{SourceMap::Identity},
    }).collect();
    let inputs=SourceInputs::admit(SourceInputSpec{files,workspace_root:std::env::current_dir().unwrap().join("syntax-count-fixture"),
        source_identity:json!({"test":"syntax-count"}),phase:"unknown".into(),ordered_occurrences:vec![],config,host,
        luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
        limits:SourceLimits::research()},&control).unwrap();
    prepare(inputs,&control).unwrap()
}
fn counts(snapshot:&PreparedSnapshot)->Box<[SourceSyntaxCounts]> {
    snapshot.inputs.corpus.files.iter().map(|file|{
        let model=snapshot.analysis.semantic_model(snapshot.files[&file.path]);
        observed(&model.syntax_tree().unwrap().get_red_root())
    }).collect::<Vec<_>>().into_boxed_slice()
}
fn index_evidence(index:&SourceSyntaxIndex)->Value {
    json!({"declarations":index.declarations.iter().map(|d|json!({"id":d.id.public(),"name":d.name,"site":[d.site.file,d.site.start as usize,d.site.end as usize],
        "kind":d.kind,"owner":d.owner.map(|s|[s.file,s.start as usize,s.end as usize]),"implicitSelf":d.implicit_self})).collect::<Vec<_>>(),
        "ranges":index.declaration_ranges.iter().map(|r|[r.start,r.end]).collect::<Vec<_>>(),"byId":index.declarations_by_id.iter().map(|&id|(index.declarations[id].id.public(),id)).collect::<BTreeMap<_,_>>(),
        "bySite":index.declarations_by_site.iter().map(|&id|{let s=index.declarations[id].site;([s.file,s.start as usize,s.end as usize],id)}).collect::<Vec<_>>(),
        "references":index.reference_count,"calls":index.call_count})
}

#[test]
fn counts_cover_names_indexes_calls_and_recovery_without_mutating_syntax() {
    for source in ["", "local a=1; local b=a", "x.y=x[y]; x:y(a); (f)(b); f'x'; f{}", "function F(a,...) return a,g(...) end", "local a = ; f(", "a.()", "a:", "function F( return G() end", "\u{feff}local text='你好'\r\nF(text)"] {
        let root=parsed(source); let before=root.to_string(); let actual=observed(&root);
        assert_eq!(actual.nodes, root.descendants().count());
        assert!(actual.references <= actual.nodes); assert!(actual.calls <= actual.nodes);
        assert_eq!(root.to_string(),before);
    }
}

#[test]
fn counts_exclude_calls_without_a_prefix_even_with_missing_tokens() {
    let mut builder=rowan::GreenNodeBuilder::new();
    builder.start_node(LuaSyntaxKind::Chunk.into());
    builder.start_node(LuaSyntaxKind::CallExpr.into());builder.finish_node();
    builder.start_node(LuaSyntaxKind::CallExpr.into());
    builder.start_node(LuaSyntaxKind::NameExpr.into());builder.finish_node();
    builder.finish_node();builder.finish_node();
    let root=LuaSyntaxNode::new_root(builder.finish());let actual=observed(&root);
    assert_eq!(actual.references,1);assert_eq!(actual.calls,1);
    assert_eq!(root.descendants().filter_map(emmylua_parser::LuaCallExpr::cast).count(),2);
}

#[test]
fn fixed_count_storage_is_admitted_and_platform_sized() {
    assert_eq!(std::mem::size_of::<SourceSyntaxCounts>(),3*std::mem::size_of::<usize>());
    for files in [0,1,20000] {
        let counts=SourceSyntaxCounts::for_files(files,20000).unwrap();
        assert_eq!(counts.len(),files);assert!(counts.iter().all(|value|*value==SourceSyntaxCounts::default()));
    }
    assert!(SourceSyntaxCounts::for_files(20001,20000).is_err());
    assert_eq!(SourceSyntaxCounts::for_files(usize::MAX,usize::MAX).unwrap_err(),"syntax count payload overflow");
}

#[test]
fn replay_observes_cancellation_and_exact_ast_boundary() {
    let root=parsed("local x=a.b; F(x); function G(y) return F(y) end");let counts=observed(&root);
    let mut count=0;let mut checks=0;
    counts.replay(&mut count, counts.nodes, || {checks+=1;Ok(())}).unwrap();
    assert_eq!(count,counts.nodes); assert_eq!(checks,counts.nodes);
    let mut count=0;
    assert_eq!(counts.replay(&mut count, counts.nodes-1, ||Ok(())).unwrap_err(), "syntax index AST bound");
    assert_eq!(count, counts.nodes);
    for cancelled_at in 1..=counts.nodes {
        let mut count=0;let mut checks=0;let control=check_control();let signal=control.tracked_cancel_handle().unwrap();
        let error=counts.replay(&mut count,counts.nodes,|| { checks+=1;if checks==cancelled_at{signal.cancel();}control.check().map_err(|error|error.to_string()) }).unwrap_err();
        assert!(error.contains("cancelled")); assert_eq!(count,cancelled_at-1); assert_eq!(checks,cancelled_at);
    }
}

#[test]
fn prepared_index_and_call_queries_preserve_original_source_identity() {
    let snapshot=prepared(&[("z.lua","local root=1; function F(x) local y=x; return y,root end; F(root)"),("bom.lua","\u{feff}local text='你好'; Obj.value=text; Obj:Run(text)"),("empty.lua","")]);
    let identity=snapshot.inputs.identity_sha256().to_owned();
    let rebuilt=snapshot.build_syntax_index(&counts(&snapshot)).unwrap();
    assert_eq!(index_evidence(&snapshot.syntax),index_evidence(&rebuilt));
    let calls=snapshot.source_queries(&snapshot.control).unwrap().calls(QueryTarget::All,PageRequest{limit:200,..PageRequest::default()}).unwrap();
    assert_eq!(calls.rows.len(),2); assert!(!calls.has_more);
    assert_eq!(snapshot.inputs.identity_sha256(),identity);
}

#[test]
fn row_charge_and_cancellation_preserve_the_admitted_boundary() {
    let snapshot=prepared(&[("a.lua","local x=1; F(x); x=x+1")]);let counts=counts(&snapshot);
    let rows=snapshot.syntax.declarations.len()+snapshot.syntax.reference_count+snapshot.syntax.call_count;
    let maximum=snapshot.inputs.corpus.limits.construction_steps;
    snapshot.inputs.shared_steps.store(maximum-rows,Ordering::Relaxed);
    snapshot.build_syntax_index(&counts).unwrap();
    assert_eq!(snapshot.inputs.shared_steps.load(Ordering::Relaxed),maximum);
    assert!(snapshot.build_syntax_index(&counts).unwrap_err().contains("shared operation step budget"));
    snapshot.control.tracked_cancel_handle().unwrap().cancel();
    assert!(snapshot.build_syntax_index(&counts).unwrap_err().contains("cancelled"));
    assert_eq!(snapshot.inputs.shared_steps.load(Ordering::Relaxed),maximum);
}

#[test]
fn exact_row_and_ast_ceilings_are_enforced() {
    for repetitions in [6000,6001] {
        let source="a=a\n".repeat(repetitions);let mut snapshot=prepared(&[("a.lua",&source)]);let counts=counts(&snapshot);
        let nodes=counts.iter().map(|value|value.nodes).sum::<usize>();
        let corpus=Arc::get_mut(&mut Arc::get_mut(&mut snapshot.inputs).unwrap().corpus).unwrap();corpus.limits.research=false;corpus.limits.ast_nodes=nodes;
        let result=snapshot.build_syntax_index(&counts);
        assert_eq!(result.is_ok(), repetitions==6000);
        if repetitions==6001 {assert_eq!(result.unwrap_err(),"syntax index row bound");}
        Arc::get_mut(&mut Arc::get_mut(&mut snapshot.inputs).unwrap().corpus).unwrap().limits.ast_nodes=nodes-1;
        assert_eq!(snapshot.build_syntax_index(&counts).unwrap_err(),"syntax index AST bound");
    }
}
