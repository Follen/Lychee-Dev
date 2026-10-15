// Original scanner body is copied below from the frozen syntax.rs preimage.
// It is a cfg(test)-only oracle, never a production fallback or a second cache.
use emmylua_parser::{LuaLanguageLevel,LuaParser,ParserConfig,LuaSyntaxNode,LuaSyntaxKind};
use std::{sync::atomic::{AtomicBool,Ordering},time::{Duration,Instant}};

fn parsed(source:&str)->LuaSyntaxNode {
    LuaParser::parse(source,ParserConfig::with_level(LuaLanguageLevel::Lua51)).get_red_root()
}
fn observed(root:&LuaSyntaxNode)->SourceSyntaxCounts {
    let mut value=SourceSyntaxCounts::default();
    for node in root.descendants(){value.observe(&node);}
    value
}
fn legacy_counts(root:&LuaSyntaxNode)->SourceSyntaxCounts {
    let mut count=SourceSyntaxCounts::default();
    for node in root.descendants(){
        count.nodes+=1;
        if matches!(LuaExpr::cast(node.clone()),Some(LuaExpr::NameExpr(_)|LuaExpr::IndexExpr(_))){count.references+=1;}
        if let Some(call)=emmylua_parser::LuaCallExpr::cast(node) && call.get_prefix_expr().is_some(){count.calls+=1;}
    }
    count
}
fn legacy_replay(root:&LuaSyntaxNode,count:&mut usize,limit:usize,mut check:impl FnMut()->Result<()>)->Result<()> {
    for _node in root.descendants(){
        check()?;*count+=1;if *count>limit{return Err("syntax index AST bound".into());}
    }
    Ok(())
}
fn check_control()->OperationControl {
    OperationControl::new(Instant::now()+Duration::from_secs(30),Arc::new(AtomicBool::new(false)))
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
    json!({"declarations":index.declarations.iter().map(|d|json!({"id":d.id,"name":d.name,"site":[d.site.file,d.site.start as usize,d.site.end as usize],
        "kind":d.kind,"owner":d.owner.map(|s|[s.file,s.start as usize,s.end as usize]),"implicitSelf":d.implicit_self})).collect::<Vec<_>>(),
        "ranges":index.declaration_ranges.iter().map(|r|[r.start,r.end]).collect::<Vec<_>>(),"byId":index.declarations_by_id,
        "bySite":index.declarations_by_site.iter().map(|(s,id)|([s.file,s.start as usize,s.end as usize],id)).collect::<Vec<_>>(),
        "references":index.reference_count,"calls":index.call_count})
}

#[test]
fn counts_match_original_casts_for_names_indexes_calls_and_recovery() {
    for source in ["", "local a=1; local b=a", "x.y=x[y]; x:y(a); (f)(b); f'x'; f{}",
        "function F(a,...) return a,g(...) end", "local f=function(x) return x end; f(1)",
        "local a = ; f(", "a.()", "a:", "function F( return G() end", "\u{feff}local text='你好'\r\nF(text)"] {
        let root=parsed(source);let before=root.to_string();let expected=legacy_counts(&root);let actual=observed(&root);
        assert_eq!(actual,expected,"{source:?}");
        assert_eq!(root.to_string(),before,"read-only counting must not alter recovery syntax");
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
    assert_eq!(actual,legacy_counts(&root));assert_eq!(actual.references,1);assert_eq!(actual.calls,1);
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
fn replay_preserves_every_control_check_and_ast_failure_boundary() {
    let root=parsed("local x=a.b; F(x); function G(y) return F(y) end");let counts=observed(&root);
    for initial in [0,3] {for limit in [0,initial,initial+counts.nodes-1,initial+counts.nodes,initial+counts.nodes+1] {
        for cancelled_at in 0..=counts.nodes+1 {
            let run=|legacy:bool| {
                let mut count=initial;let mut checks=0;let control=check_control();let cancel=control.cancel_handle();
                let check=||{checks+=1;if checks==cancelled_at{cancel.store(true,Ordering::Release);}control.check().map_err(|error|error.to_string())};
                let result=if legacy{legacy_replay(&root,&mut count,limit,check)}else{counts.replay(&mut count,limit,check)};
                (result,count,checks)
            };
            assert_eq!(run(false),run(true),"initial={initial}, limit={limit}, cancel={cancelled_at}");
        }
    }}
}

#[test]
fn prepared_index_and_public_reference_bytes_match_original_scanner() {
    let mut snapshot=prepared(&[("z.lua","local root=1; function F(x) local y=x; return y,root end; F(root)"),
        ("bom.lua","\u{feff}local text='你好'; Obj.value=text; Obj:Run(text)"),("empty.lua","")]);
    let identities=snapshot.inputs.identity_sha256().to_owned();
    let used=snapshot.inputs.shared_steps.load(Ordering::Relaxed);
    let expected=snapshot.legacy_syntax_index().unwrap();let legacy_used=snapshot.inputs.shared_steps.load(Ordering::Relaxed);
    snapshot.inputs.shared_steps.store(used,Ordering::Relaxed);
    let counted=snapshot.build_syntax_index(&counts(&snapshot)).unwrap();
    assert_eq!(snapshot.inputs.shared_steps.load(Ordering::Relaxed),legacy_used);
    assert_eq!(index_evidence(&snapshot.syntax),index_evidence(&expected));
    assert_eq!(serde_json::to_vec(&index_evidence(&counted)).unwrap(),serde_json::to_vec(&index_evidence(&expected)).unwrap());
    let query=|snapshot:&PreparedSnapshot|serde_json::to_vec(&snapshot.source_queries(&snapshot.control).unwrap()
        .references(QueryTarget::All,ReferenceFilter::default(),PageRequest{limit:200,..PageRequest::default()}).unwrap()).unwrap();
    snapshot.inputs.shared_steps.store(used,Ordering::Relaxed);let actual=query(&snapshot);
    snapshot.syntax=expected;snapshot.inputs.shared_steps.store(used,Ordering::Relaxed);
    assert_eq!(actual,query(&snapshot));assert_eq!(snapshot.inputs.identity_sha256(),identities);
}

#[test]
fn row_charge_and_cancellation_stay_at_the_original_final_boundary() {
    let snapshot=prepared(&[("a.lua","local x=1; F(x); x=x+1")]);let counts=counts(&snapshot);
    let rows=snapshot.syntax.declarations.len()+snapshot.syntax.reference_count+snapshot.syntax.call_count;
    let maximum=snapshot.inputs.corpus.limits.construction_steps;
    for used in [maximum-rows,maximum-rows+1,maximum] {
        snapshot.inputs.shared_steps.store(used,Ordering::Relaxed);
        let old=snapshot.legacy_syntax_index().map(|index|index_evidence(&index));let old_used=snapshot.inputs.shared_steps.load(Ordering::Relaxed);
        snapshot.inputs.shared_steps.store(used,Ordering::Relaxed);
        let new=snapshot.build_syntax_index(&counts).map(|index|index_evidence(&index));
        assert_eq!(old,new);assert_eq!(old_used,snapshot.inputs.shared_steps.load(Ordering::Relaxed));
    }
    snapshot.control.cancel_handle().store(true,Ordering::Release);
    snapshot.inputs.shared_steps.store(maximum,Ordering::Relaxed);
    let old=snapshot.legacy_syntax_index().unwrap_err();let new=snapshot.build_syntax_index(&counts).unwrap_err();
    assert_eq!(old,new);assert!(new.contains("cancelled"));assert_eq!(snapshot.inputs.shared_steps.load(Ordering::Relaxed),maximum);
}

#[test]
fn exact_row_ceiling_and_ast_ceiling_match_original_scanner() {
    for repetitions in [6000,6001] {
        let source="a=a\n".repeat(repetitions);let mut snapshot=prepared(&[("a.lua",&source)]);let counts=counts(&snapshot);
        let nodes=counts.iter().map(|value|value.nodes).sum::<usize>();
        // Private test mutation selects the existing normal row ceiling after
        // an admitted research preparation, so no extra production API exists.
        let corpus=Arc::get_mut(&mut Arc::get_mut(&mut snapshot.inputs).unwrap().corpus).unwrap();
        corpus.limits.research=false;corpus.limits.ast_nodes=nodes;
        let used=snapshot.inputs.shared_steps.load(Ordering::Relaxed);
        let old=snapshot.legacy_syntax_index().map(|index|index_evidence(&index));let old_used=snapshot.inputs.shared_steps.load(Ordering::Relaxed);
        snapshot.inputs.shared_steps.store(used,Ordering::Relaxed);
        let new=snapshot.build_syntax_index(&counts).map(|index|index_evidence(&index));
        assert_eq!(old,new);assert_eq!(old_used,snapshot.inputs.shared_steps.load(Ordering::Relaxed));
        assert_eq!(new.is_ok(),repetitions==6000);
        if repetitions==6001 {assert_eq!(new.unwrap_err(),"syntax index row bound");assert_eq!(old_used,used);}
        Arc::get_mut(&mut Arc::get_mut(&mut snapshot.inputs).unwrap().corpus).unwrap().limits.ast_nodes=nodes-1;
        assert_eq!(snapshot.legacy_syntax_index().unwrap_err(),snapshot.build_syntax_index(&counts).unwrap_err());
        assert_eq!(snapshot.build_syntax_index(&counts).unwrap_err(),"syntax index AST bound");
    }
}

impl PreparedSnapshot {
    fn legacy_syntax_index(&self)->Result<SourceSyntaxIndex> {
        let mut index=SourceSyntaxIndex::default();let mut count=0usize;
        for (file_index,source) in self.inputs.corpus.files.iter().enumerate() {
            self.control.check().map_err(|e|e.to_string())?;
            let file=source.path.as_str();let model=self.analysis.semantic_model(self.files[file]);
            let facts=model.file_facts().ok_or("syntax facts missing")?;
            let root=model.syntax_tree().ok_or("syntax tree missing")?.get_red_root();
            let source_site=|range|SyntaxSite::at(file_index,self.inputs.original_range(file,range).expect("admitted parser range"));
            let declaration_start=index.declarations.len();
            for decl in &facts.decls {
                self.control.check().map_err(|e|e.to_string())?;
                if !decl.kind.is_local(){continue}
                let site=source_site(decl.name_range);
                let owner=decl.owner_syntax.and_then(|syntax|syntax.to_node_from_root(&root)).and_then(|node|{
                    if let Some(closure)=LuaClosureExpr::cast(node.clone()){Some(source_site(closure.get_range()))}else{syntax_owner(&node,&self.inputs,file_index)}
                });
                let kind=match decl.kind {emmylua_code_analysis::DeclKind::Param=>"parameter",emmylua_code_analysis::DeclKind::Local{is_iter:true,..}=>"loop_local",_=>"local"}.into();
                index.declarations.push(IndexedDeclaration{id:source_decl_id(&self.inputs,&site.public(&self.inputs)),name:decl.name.to_string(),site,kind,owner,implicit_self:decl.is_implicit_self});
            }
            index.declaration_ranges.push(declaration_start..index.declarations.len());
            // Admit the same complete row count without retaining or hashing
            // every reference and call before any query needs them.
            for node in root.descendants(){
                self.control.check().map_err(|e|e.to_string())?;count+=1;if count>self.inputs.corpus.limits.ast_nodes{return Err("syntax index AST bound".into())}
                if matches!(LuaExpr::cast(node.clone()),Some(LuaExpr::NameExpr(_)|LuaExpr::IndexExpr(_))){index.reference_count+=1;}
                if let Some(call)=emmylua_parser::LuaCallExpr::cast(node) && call.get_prefix_expr().is_some(){index.call_count+=1;}
            }
        }
        let rows=index.declarations.len()+index.reference_count+index.call_count;
        if rows>if self.inputs.corpus.limits.research{5000000}else{12000}{return Err("syntax index row bound".into())}
        for (i,decl) in index.declarations.iter().enumerate(){index.declarations_by_id.insert(decl.id.clone(),i);index.declarations_by_site.insert(decl.site,i);}
        self.inputs.charge(rows).map_err(|e|e.to_string())?;Ok(index)
    }
}
