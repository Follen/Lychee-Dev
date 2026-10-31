//! Immutable declaration lookup ownership and compatibility regressions.
use super::*;
use std::{sync::atomic::Ordering,time::{Duration,Instant}};

fn declaration(id:usize,file:usize,start:u32,end:u32)->IndexedDeclaration {
    IndexedDeclaration{id:DeclarationId::parse(&format!("{id:064x}")).unwrap(),name:format!("name_{id}"),
        site:SyntaxSite{file,start,end},kind:match id%3{0=>"local",1=>"parameter",_=>"loop_local"},
        owner:None,implicit_self:false}
}
fn old_lookups(index:&SourceSyntaxIndex)->(BTreeMap<String,usize>,BTreeMap<SyntaxSite,usize>) {
    let mut by_id=BTreeMap::new();let mut by_site=BTreeMap::new();
    for (id,declaration) in index.declarations.iter().enumerate(){
        by_id.insert(declaration.id.public(),id);by_site.insert(declaration.site,id);
    }
    (by_id,by_site)
}

#[test]
fn packed_declaration_lookups_match_old_maps_for_large_unordered_duplicate_input() {
    let mut index=SourceSyntaxIndex::default();
    for n in 0..20_000usize {
        let key=n.wrapping_mul(7919)%15_001;
        let start=(n.wrapping_mul(23)%8009)as u32;
        index.declarations.push(declaration(key,n%7,start,start+(n%11)as u32));
    }
    index.declarations.push(declaration(20_001,usize::MAX,u32::MAX,u32::MAX));
    let (ids,sites)=old_lookups(&index);index.finish_declaration_lookups();
    for (key,&expected) in &ids{assert_eq!(index.declaration_by_id(key),Some(expected));}
    for (&key,&expected) in &sites{assert_eq!(index.declaration_by_site(key),Some(expected));}
    assert_eq!(index.declarations_by_id.as_ref(),ids.values().copied().collect::<Vec<_>>());
    assert_eq!(index.declarations_by_site.as_ref(),sites.values().copied().collect::<Vec<_>>());
    for missing in ["","z","0","ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"] {
        assert_eq!(index.declaration_by_id(missing),None);
    }
    assert_eq!(index.declaration_by_site(SyntaxSite{file:8,start:0,end:0}),None);
}

#[test]
fn independent_duplicate_id_and_site_keys_keep_the_last_original_declaration() {
    let mut index=SourceSyntaxIndex::default();
    index.declarations=vec![declaration(7,0,3,9),declaration(2,1,0,0),
        declaration(7,2,4,8),declaration(4,0,3,9),declaration(7,2,4,8)];
    index.finish_declaration_lookups();
    assert_eq!(index.declaration_by_id(&format!("{:064x}",7)),Some(4));
    assert_eq!(index.declaration_by_id(&format!("{:064x}",4)),Some(3));
    assert_eq!(index.declaration_by_site(SyntaxSite{file:0,start:3,end:9}),Some(3));
    assert_eq!(index.declaration_by_site(SyntaxSite{file:2,start:4,end:8}),Some(4));
    assert_eq!(index.declaration_by_site(SyntaxSite{file:1,start:0,end:0}),Some(1));
    assert_eq!(index.declarations.len(),5,"lookup deduplication must never remove source rows");
}

#[test]
fn packed_lookups_retain_only_two_row_indexes_and_one_digest_owner() {
    let mut index=SourceSyntaxIndex::default();
    for n in 0..100_000usize{index.declarations.push(declaration(n,n/1000,n as u32,n as u32+1));}
    let keys=index.declarations.iter().map(|declaration|declaration.id).collect::<Vec<_>>();
    index.finish_declaration_lookups();
    for (declaration,key) in index.declarations.iter().zip(keys){assert_eq!(declaration.id,key);}
    assert_eq!(std::mem::size_of::<DeclarationId>(),32);
    let retained=std::mem::size_of_val(index.declarations_by_id.as_ref())
        +std::mem::size_of_val(index.declarations_by_site.as_ref());
    let rows=index.declarations.len();
    assert_eq!(retained,2*rows*std::mem::size_of::<usize>());
    // This conservative old-map count excludes node metadata, unused capacity,
    // allocator overhead, and the declaration rows retained by BOTH versions.
    let old_initialized=rows*(std::mem::size_of::<String>()+std::mem::size_of::<usize>()
        +std::mem::size_of::<SyntaxSite>()+std::mem::size_of::<usize>())
        +rows*64;
    assert_eq!(old_initialized-retained,rows*(std::mem::size_of::<String>()+std::mem::size_of::<SyntaxSite>()+64));
    if usize::BITS==64{assert_eq!(old_initialized-retained,10_400_000);}
}

#[test]
fn empty_singleton_and_repeat_freeze_preserve_lookup_order() {
    let mut index=SourceSyntaxIndex::default();index.finish_declaration_lookups();
    assert!(index.declarations_by_id.is_empty());assert!(index.declarations_by_site.is_empty());
    assert_eq!(index.declaration_by_id(""),None);
    index.declarations.push(declaration(9,0,0,0));index.finish_declaration_lookups();
    assert_eq!(index.declarations_by_id.as_ref(),[0]);assert_eq!(index.declarations_by_site.as_ref(),[0]);
    index.finish_declaration_lookups();
    assert_eq!(index.declaration_by_id(&format!("{:064x}",9)),Some(0));
    assert_eq!(index.declaration_by_site(SyntaxSite{file:0,start:0,end:0}),Some(0));
}

fn admitted(control:&OperationControl)->Arc<SourceInputs> {
    let text="local value = 1\nreturn value\n";
    let host=TrustedHostEnvironment::from_host_material("syntax-index-test".into(),false,vec![],vec![],control).unwrap();
    SourceInputs::admit(SourceInputSpec{files:vec![SourceFileInput{path:"a.lua".into(),original_utf8:text.into(),
        analysis_utf8:text.into(),content_sha256:hash(text.as_bytes()),map:SourceMap::Identity}],
        workspace_root:std::env::current_dir().unwrap().join("syntax-index-fixture"),source_identity:json!({"test":"syntax-index"}),
        phase:"unknown".into(),ordered_occurrences:vec![],config:crate::engine_config::research(),host,
        luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
        limits:SourceLimits::research()},control).unwrap()
}

#[test]
fn lookup_freeze_preserves_public_declaration_bytes_and_static_kind_text() {
    let control=OperationControl::uncancelable(Instant::now()+Duration::from_secs(30));
    let inputs=admitted(&control);let mut index=SourceSyntaxIndex::default();
    for id in 0..6{index.declarations.push(declaration(id,0,6,11));}
    let before=index.declarations.iter().map(|declaration|declaration.public(&inputs)).collect::<Vec<_>>();
    let expected=serde_json::to_vec(&before).unwrap();index.finish_declaration_lookups();
    let after=index.declarations.iter().map(|declaration|declaration.public(&inputs)).collect::<Vec<_>>();
    assert_eq!(serde_json::to_vec(&after).unwrap(),expected);
    for (id,declaration) in after.iter().enumerate(){
        assert_eq!(declaration.id,format!("{id:064x}"));
        assert_eq!(declaration.kind,match id%3{0=>"local",1=>"parameter",_=>"loop_local"});
    }
    // The retained binary key hashes precisely the former public tuple,
    // including JSON escaping and the full original source range.
    for site in [Site{file:"a.lua".into(),start:6,end:11},
        Site{file:"路径/\"quoted\"\\name.lua".into(),start:0,end:u32::MAX}] {
        let public=source_decl_id(&inputs,&site);
        assert_eq!(source_decl_key(&inputs,&site).public(),public);
        assert_eq!(DeclarationId::parse(&public),Some(source_decl_key(&inputs,&site)));
    }
    for invalid in ["".to_owned(),"0".repeat(63),"0".repeat(65),"A".repeat(64),"é".repeat(32)] {
        assert_eq!(DeclarationId::parse(&invalid),None);
    }
}

#[test]
fn packed_declaration_target_keeps_public_reference_proof_and_context_identity() {
    let control=OperationControl::uncancelable(Instant::now()+Duration::from_secs(30));
    let prepared=prepare(admitted(&control),&control).unwrap();
    let declaration=&prepared.syntax.declarations[0];
    let public_id=source_decl_id(&prepared.inputs,&declaration.site.public(&prepared.inputs));
    let query=prepared.source_queries(&control).unwrap();
    let references=query.references(QueryTarget::Declaration(public_id.clone()),
        ReferenceFilter::default(),PageRequest::default()).unwrap();
    assert!(!references.has_more);
    assert!(references.rows.iter().any(|row|row.roles.contains(&ReferenceRole::Read)));
    for row in &references.rows{assert_eq!(row.lexical_declaration.as_deref(),Some(public_id.as_str()));}
    let declared=references.rows.iter().find(|row|row.roles.contains(&ReferenceRole::Declaration)).unwrap();
    assert_eq!(declared.id,digest(&(&public_id,"declaration_reference")));
    assert_eq!(declared.proof,public_id);
    let context=query.context(QueryTarget::Declaration(public_id.clone()),ContextOptions::default(),PageRequest::default()).unwrap();
    let row=context.rows.iter().find(|row|row.kind=="lexical_declaration").unwrap();
    assert_eq!(row.id,public_id);
    assert_eq!(row.detail["declaration"]["id"],public_id);
}

#[test]
fn compacted_lookup_keeps_original_query_cancellation_and_final_charge_boundary() {
    let control=OperationControl::uncancelable(Instant::now()+Duration::from_secs(30));
    let prepared=prepare(admitted(&control),&control).unwrap();
    let counts=prepared.inputs.corpus.files.iter().map(|file|{
        let model=prepared.analysis.semantic_model(prepared.files[&file.path]);
        let mut count=SourceSyntaxCounts::default();
        for node in model.syntax_tree().unwrap().get_red_root().descendants(){count.observe(&node);}
        count
    }).collect::<Vec<_>>();
    let rows=prepared.syntax.declarations.len()+prepared.syntax.reference_count+prepared.syntax.call_count;
    let limit=prepared.inputs.corpus.limits.construction_steps;
    prepared.inputs.shared_steps.store(limit-rows,Ordering::Relaxed);
    assert!(prepared.build_syntax_index(&counts).is_ok());
    assert_eq!(prepared.inputs.shared_steps.load(Ordering::Relaxed),limit);
    prepared.inputs.shared_steps.store(limit-rows+1,Ordering::Relaxed);
    assert!(prepared.build_syntax_index(&counts).unwrap_err().contains("shared operation step budget"));
    assert_eq!(prepared.inputs.shared_steps.load(Ordering::Relaxed),limit-rows+1);
    let query=prepared.source_queries(&control).unwrap();
    control.tracked_cancel_handle().unwrap().cancel();
    let error=query.references(QueryTarget::Declaration(prepared.syntax.declarations[0].id.public()),
        ReferenceFilter::default(),PageRequest::default()).unwrap_err();
    assert_eq!(error.code,"command.cancelled");
    assert_eq!(prepared.inputs.shared_steps.load(Ordering::Relaxed),limit-rows+1);
}
