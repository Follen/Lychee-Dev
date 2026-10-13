// Source query prototype: investigated Lua is parsed and never executed.
use std::{collections::BTreeMap,path::PathBuf,sync::{Arc,atomic::AtomicBool},time::{Duration,Instant}};
use emmylua_code_analysis::{Emmyrc,EmmyrcLuaVersion};
use lycheedev_core::control::OperationControl;
use lycheedev_source::native::*;
use serde_json::{json,Value};
type Result<T> = std::result::Result<T,String>;
const FILES: &[(&str,&str)] = &[
    ("query-roles.lua",include_str!("../../../tests/rust-source-engine/wow/native/query-roles.lua")),
    ("query-captures.lua",include_str!("../../../tests/rust-source-engine/wow/native/query-captures.lua")),
    ("query-load-a.lua",include_str!("../../../tests/rust-source-engine/wow/native/query-load-a.lua")),
    ("query-load-b.lua",include_str!("../../../tests/rust-source-engine/wow/native/query-load-b.lua")),
    ("scope.lua",include_str!("../../../tests/rust-source-engine/golden/scope.lua")),
    ("writes-redeclaration.lua",include_str!("../../../tests/rust-source-engine/golden/writes-redeclaration.lua")),
    ("lexical-negative.lua",include_str!("../../../tests/rust-source-engine/wow/native/query-lexical-negative.lua")),
    ("query-loop-scopes.lua",include_str!("../../../tests/rust-source-engine/wow/native/query-loop-scopes.lua")),
    ("query-vararg-assignment.lua",include_str!("../../../tests/rust-source-engine/wow/native/query-vararg-assignment.lua")),
    ("query-empty-function.lua",include_str!("../../../tests/rust-source-engine/wow/native/query-empty-function.lua")),
];
fn text(file:&str)->&'static str{FILES.iter().find(|(name,_)|*name==file).expect("fixed source").1}
fn point(file:&str,marker:&str,token:&str)->Result<Site>{
    let source=text(file);let at=source.find(marker).ok_or("missing unique marker")?;
    if source[at+marker.len()..].contains(marker){return Err("duplicate source marker".into())}
    let start=at+marker.find(token).ok_or("missing marker token")?;
    Ok(Site{file:file.into(),start:start as u32,end:(start+token.len())as u32})
}
fn control()->OperationControl{OperationControl::new(Instant::now()+Duration::from_secs(30),Arc::new(AtomicBool::new(false)))}
fn prepare_case(files:&[&str],ordered:&[&str],c:&OperationControl)->Result<PreparedSnapshot>{
    let mut config=Emmyrc::default();config.runtime.version=EmmyrcLuaVersion::Lua51;
    let spec=SourceInputSpec{files:files.iter().map(|name|{let source=text(name);SourceFileInput{path:(*name).into(),original_utf8:source.into(),content_sha256:hash(source.as_bytes()),analysis_utf8:source.into(),map:SourceMap::Identity}}).collect(),workspace_root:PathBuf::from("C:/lycheedev-source-query-probe"),source_identity:json!({"kind":"fixed-query-facade-source"}),phase:"after_load".into(),ordered_occurrences:ordered.iter().map(|name|(*name).into()).collect(),config,host:TrustedHostEnvironment::from_host_material("closed-query-probe".into(),true,vec![],vec![],c).map_err(|e|e.to_string())?,luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"64cd0d82734a743ccbffe3a2d28727ce8e48c8cf1452dd69066f2c56643425f7".into(),executable_bytes:487424},limits:SourceLimits::default()};
    prepare(SourceInputs::admit(spec,c).map_err(|e|e.to_string())?,c).map_err(|e|e.to_string())
}
struct Output{checked:usize,failed:usize,bytes:usize}
impl Output{
    fn emit(&mut self,value:Value)->Result<()>{let mut frame=value;frame["schema"]=json!("lycheedev.source-query-probe.v1");let line=serde_json::to_string(&frame).map_err(|e|e.to_string())?;self.bytes+=line.len()+1;if self.bytes>2*1024*1024{return Err("output byte budget".into())}println!("{line}");Ok(())}
    fn check(&mut self,name:&str,expected:Value,actual:Value)->Result<()>{let passed=expected==actual;self.checked+=1;if !passed{self.failed+=1;}self.emit(json!({"event":"assertion","name":name,"expected":expected,"actual":actual,"passed":passed}))}
}
fn definition_json(result:&DefinitionResult)->Result<Value>{serde_json::to_value(result).map_err(|e|e.to_string())}
fn function_site(result:&DefinitionResult)->Value{match &result.selected{QueryFact::Known{value}=>json!(value.function_declaration),_=>Value::Null}}
fn lexical_site(result:&DefinitionResult)->Value{match &result.lexical{QueryFact::Known{value}=>json!(value.site),_=>Value::Null}}
fn merged_references(rows:&[ReferenceRecord])->Value {
    let mut merged:BTreeMap<(String,u32,u32),(Site,Vec<Value>)>=BTreeMap::new();
    for row in rows{let entry=merged.entry((row.site.file.clone(),row.site.start,row.site.end)).or_insert((row.site.clone(),vec![]));for role in &row.roles{let value=serde_json::to_value(role).expect("reference role");if !entry.1.contains(&value){entry.1.push(value);}}}
    json!(merged.into_values().map(|(site,mut roles)|{roles.sort_by_key(Value::to_string);json!({"site":site,"roles":roles})}).collect::<Vec<_>>())
}
fn roles_case(out:&mut Output)->Result<()>{
    let c=control();let prepared=prepare_case(&["query-roles.lua"],&["query-roles.lua"],&c)?;
    let graph=prepared.build("query-roles",EvaluatorProfile::Native,Some("query-facade-roles-v1"),&c).map_err(|e|e.to_string())?;
    let syntax=prepared.source_queries(&c).map_err(|e|e.to_string())?;
    let query=syntax.with_execution(&graph,QuerySelector::LoadOccurrence{occurrence:0},&c).map_err(|e|e.to_string())?;
    let file="query-roles.lua";let original=point(file,"local function Original(v)","Original")?;
    let replacement=point(file,"local function Replacement(v)","Replacement")?;
    let alias=point(file,"local alias = Original","alias")?;let saved=point(file,"local saved = alias","saved")?;let shadow=point(file,"    local saved = Replacement","saved")?;
    let cases=[
        ("alias_original","local alias = Original","Original",Some(original.clone()),original.clone()),
        ("saved_from_alias","local saved = alias","alias",Some(alias.clone()),original.clone()),
        ("function_value","local functionValueRead = saved","saved",Some(saved.clone()),original.clone()),
        ("saved_call","local firstCall = saved(1)","saved",Some(saved),original.clone()),
        ("alias_after_write","local secondCall = alias(\"two\")","alias",Some(alias),replacement.clone()),
        ("block_shadow","local shadowCall = saved(3)","saved",Some(shadow),replacement.clone()),
        ("member_saved","local memberSaved = object.run","object.run",None,original.clone()),
        ("saved_member_call","local savedMemberCall = memberSaved(4)","memberSaved",Some(point(file,"local memberSaved = object.run","memberSaved")?),original.clone()),
        ("current_member_call","local currentMemberCall = object.run(5)","object.run",None,replacement.clone()),
        ("other_object_call","local otherMemberCall = other.run(6)","other.run",None,replacement),
    ];let mut original_value=None;
    for(label,marker,token,lexical,selected)in cases{
        let site=point(file,marker,token)?;let result=query.definitions(&site).map_err(|e|e.to_string())?;
        out.emit(json!({"event":"definition","case":"roles","query":label,"result":definition_json(&result)?}))?;
        out.check(&format!("query:roles:{label}:lexical"),json!(lexical),lexical_site(&result))?;
        out.check(&format!("query:roles:{label}:selected"),json!(selected),function_site(&result))?;
        if label=="function_value"&&let QueryFact::Known{value}=&result.selected{original_value=Some(value.value_id.clone());}
    }
    let dynamic=point(file,"local dynamicMember = object[unknownKey]","object[unknownKey]")?;
    let result=query.definitions(&dynamic).map_err(|e|e.to_string())?;
    out.emit(json!({"event":"definition","case":"roles","query":"dynamic_member","result":definition_json(&result)?}))?;
    out.check("query:roles:dynamic_member:unavailable",json!(true),json!(matches!(result.selected,QueryFact::Unavailable{..})))?;
    let target=QueryTarget::Value(original_value.ok_or("selected original value identity")?);
    let page=query.references(target.clone(),ReferenceFilter{roles:vec![ReferenceRole::Read,ReferenceRole::Call]},PageRequest::default()).map_err(|e|e.to_string())?;
    let expected_reads=[
        ("local alias = Original","Original",vec!["read"]),
        ("local saved = alias","alias",vec!["read"]),
        ("local functionValueRead = saved","saved",vec!["read"]),
        ("local firstCall = saved(1)","saved",vec!["call","read"]),
        ("local object = { run = Original }","Original",vec!["read"]),
        ("local memberSaved = object.run","object.run",vec!["read"]),
        ("local savedMemberCall = memberSaved(4)","memberSaved",vec!["call","read"]),
    ].into_iter().map(|(marker,token,roles)|Ok(json!({"site":point(file,marker,token)?,"roles":roles}))).collect::<Result<Vec<_>>>()?;
    out.check("query:roles:original_exact_inverse_read_call_set",json!(expected_reads),merged_references(&page.rows))?;
    out.check("query:roles:original_reference_page_complete",json!(false),json!(page.has_more))?;
    out.emit(json!({"event":"value_references","case":"roles","target":target,"page":page}))?;
    let calls=query.calls(target.clone(),PageRequest::default()).map_err(|e|e.to_string())?;
    out.check("query:roles:original_exact_calls",json!([point(file,"local firstCall = saved(1)","saved(1)")?,point(file,"local savedMemberCall = memberSaved(4)","memberSaved(4)")?]),json!(calls.rows.iter().map(|row|&row.site).collect::<Vec<_>>()))?;
    out.emit(json!({"event":"value_calls","case":"roles","target":target,"page":calls}))?;
    let context=query.context(target,ContextOptions::default(),PageRequest::default()).map_err(|e|e.to_string())?;
    out.emit(json!({"event":"value_context","case":"roles","page":context}))?;
    out.emit(json!({"event":"scenario","name":"query-roles","graph":graph,"inputIdentity":prepared.inputs().identity(),"fullSourceAcceptance":false}))?;
    Ok(())
}
fn lexical_case(out:&mut Output)->Result<()>{
    let c=control();let prepared=prepare_case(&["scope.lua","writes-redeclaration.lua","lexical-negative.lua"],&[],&c)?;
    let query=prepared.source_queries(&c).map_err(|e|e.to_string())?;
    let model=prepared.analysis().semantic_model(prepared.files()["lexical-negative.lua"]);
    let facts=model.file_facts().ok_or("diagnostic lexical facts")?;
    let returned=point("lexical-negative.lua"," return value","value")?;
    out.emit(json!({"event":"lexical_scope_diagnostic","file":"lexical-negative.lua","returnSite":returned,"rawResolveName":format!("{:?}",model.resolve_name(returned.start.into())),"scopes":facts.scopes.iter().map(|scope|format!("{scope:?}")).collect::<Vec<_>>(),"declarations":facts.decls.iter().map(|decl|json!({"name":decl.name.to_string(),"kind":format!("{:?}",decl.kind),"range":[u32::from(decl.name_range.start()),u32::from(decl.name_range.end())],"scope":decl.scope_id,"owner":format!("{:?}",decl.owner_syntax)})).collect::<Vec<_>>()}))?;
    let outer=point("scope.lua","local value = 1","value")?;let inner=point("scope.lua","        local value = 2","value")?;
    let first=point("writes-redeclaration.lua","local reused = 0","reused")?;let second=point("writes-redeclaration.lua","local reused = 2","reused")?;
    let outer_negative=point("lexical-negative.lua","local value=1","value")?;
    let parameter=point("lexical-negative.lua","local function f(value)","value")?;
    let loop_decl=point("lexical-negative.lua","for value=1,2 do","value")?;
    let queries=[
        ("captured_initializer","scope.lua","local captured = value","value",outer.clone()),
        ("inner_read","scope.lua","local inner_read = value","value",inner),
        ("outer_return","scope.lua","return value, captured","value",outer),
        ("first_write","writes-redeclaration.lua","reused = 1","reused",first.clone()),
        ("old_value","writes-redeclaration.lua","local old = reused","reused",first),
        ("new_write","writes-redeclaration.lua","reused = 3","reused",second.clone()),
        ("new_return","writes-redeclaration.lua","return old, reused","reused",second),
        ("loop_local","lexical-negative.lua","local z=value","value",loop_decl),
        ("parameter_return","lexical-negative.lua"," return value","value",parameter),
        ("initializer_outer","lexical-negative.lua","local value=value","value",outer_negative),
    ];
    for(label,file,marker,token,expected)in queries{
        // The initializer query is the RHS name, not the new declaration.
        let mut site=point(file,marker,token)?;if label=="initializer_outer"{let at=text(file).find(marker).ok_or("initializer marker")?+marker.rfind(token).ok_or("initializer RHS")?;site.start=at as u32;site.end=(at+token.len())as u32;}
        let result=query.definitions(&site).map_err(|e|e.to_string())?;
        out.emit(json!({"event":"definition","case":"lexical","query":label,"result":definition_json(&result)?}))?;
        out.check(&format!("query:lexical:{label}:exact_binding"),json!(expected),lexical_site(&result))?;
        out.check(&format!("query:lexical:{label}:no_execution_invented"),json!(true),json!(matches!(result.selected,QueryFact::NotApplicable)))?;
    }
    let refs=query.references(QueryTarget::All,ReferenceFilter::default(),PageRequest::default()).map_err(|e|e.to_string())?;
    out.emit(json!({"event":"syntax_references","case":"lexical","page":refs}))?;
    let calls=query.calls(QueryTarget::All,PageRequest::default()).map_err(|e|e.to_string())?;
    out.check("query:lexical:comments_strings_not_calls",json!([]),json!(calls.rows))?;
    out.emit(json!({"event":"syntax_calls","case":"lexical","page":calls}))?;
    Ok(())
}
fn loop_scope_case(out:&mut Output)->Result<()>{
    let c=control();let file="query-loop-scopes.lua";let prepared=prepare_case(&[file],&[],&c)?;
    let query=prepared.source_queries(&c).map_err(|e|e.to_string())?;
    let outer=point(file,"local value = 10","value")?;
    let numeric=point(file,"for value = value, value + 2 do","value")?;
    let generic=point(file,"for value in Iterator(value) do","value")?;
    let nested_outer=point(file,"for value = 1, 2 do","value")?;
    let nested_inner=point(file,"    for value = value, value + 1 do","value")?;
    let recursive=point(file,"local function Recursive(n)","Recursive")?;
    let cases=[
        ("numeric_header_start","for value = value, value + 2 do","= value",outer.clone()),
        ("numeric_header_limit","for value = value, value + 2 do","value + 2",outer.clone()),
        ("numeric_body","local numericBody = value","value",numeric),
        ("numeric_after","local numericAfter = value","value",outer.clone()),
        ("generic_header","for value in Iterator(value) do","Iterator(value)",outer.clone()),
        ("generic_body","local genericBody = value","value",generic),
        ("generic_after","local genericAfter = value","value",outer.clone()),
        ("nested_header_start","    for value = value, value + 1 do","= value",nested_outer.clone()),
        ("nested_header_limit","    for value = value, value + 1 do","value + 1",nested_outer.clone()),
        ("nested_body","local nestedBody = value","value",nested_inner),
        ("nested_after_inner","local afterInner = value","value",nested_outer),
        ("nested_after_outer","local nestedAfter = value","value",outer.clone()),
        ("local_function_recursion","if n then return Recursive(false) end","Recursive",recursive),
        ("recursive_upvalue","    return value","value",outer),
    ];
    for(label,marker,expression,expected)in cases{
        let mut site=point(file,marker,expression)?;
        if expression.starts_with("= "){site.start+=2;site.end=site.start+5;}
        else if expression.starts_with("value +"){site.end=site.start+5;}
        else if expression.starts_with("Iterator("){site.start+=9;site.end=site.start+5;}
        let result=query.definitions(&site).map_err(|e|e.to_string())?;
        out.emit(json!({"event":"definition","case":"loop-scope","query":label,"result":definition_json(&result)?}))?;
        out.check(&format!("query:loop_scope:{label}:exact_binding"),json!(expected),lexical_site(&result))?;
    }
    Ok(())
}
fn captures_case(out:&mut Output)->Result<()>{
    let c=control();let prepared=prepare_case(&["query-captures.lua"],&["query-captures.lua"],&c)?;
    let graph=prepared.build("query-captures",EvaluatorProfile::Native,None,&c).map_err(|e|e.to_string())?;
    let syntax=prepared.source_queries(&c).map_err(|e|e.to_string())?;
    let file="query-captures.lua";let body=point(file,"local function Read() return value end","value")?;
    let ambiguous=syntax.with_execution(&graph,QuerySelector::AllOccurrences,&c).map_err(|e|e.to_string())?.definitions(&body).map_err(|e|e.to_string())?;
    out.check("query:captures:body_needs_selector",json!(true),json!(matches!(ambiguous.selected,QueryFact::SelectorRequired{..})))?;
    let mut selected=Vec::new();
    for(label,marker,expected_marker,expected_token)in [("first","local first = Read()","local value = 1","1"),("second","local second = Read()","value = \"later\"","\"later\"")]{
        let call_site=point(file,marker,"Read()")?;
        let call=graph.calls.iter().find(|call|call.site==call_site&&call.context.starts_with("load:")).ok_or("exact shared capture call")?;
        let frame=graph.native_invocations.as_ref().ok_or("native frames")?.frames.iter().find(|frame|frame.call==Some(call.id)).ok_or("exact invocation frame")?;
        let query=syntax.with_execution(&graph,QuerySelector::Invocation{frame:frame.id},&c).map_err(|e|e.to_string())?;
        let result=query.definitions(&body).map_err(|e|e.to_string())?;
        out.emit(json!({"event":"definition","case":"captures","query":label,"frame":frame.id,"callSource":call_site,"result":definition_json(&result)?}))?;
        let origin=match &result.selected{QueryFact::Known{value}=>{selected.push(value.clone());json!(value.origin)},_=>Value::Null};
        out.check(&format!("query:captures:{label}:origin"),json!(point(file,expected_marker,expected_token)?),origin)?;
        out.check(&format!("query:captures:{label}:lexical"),json!(point(file,"local value = 1","value")?),lexical_site(&result))?;
    }
    out.check("query:captures:shared_cell_distinct_write",json!(true),json!(selected.len()==2&&selected[0].writes!=selected[1].writes&&selected[0].value_id!=selected[1].value_id))?;
    let factory_decl=point(file,"    local value = seed","value")?;
    let captures=graph.native_invocations.as_ref().ok_or("capture facts")?.capture_facts.iter().filter(|capture|capture.declaration==factory_decl).collect::<Vec<_>>();
    out.emit(json!({"event":"factory_captures","facts":captures}))?;
    out.check("query:captures:factory_cells_are_distinct",json!(2),json!(captures.iter().map(|capture|&capture.cell).collect::<std::collections::BTreeSet<_>>().len()))?;
    out.emit(json!({"event":"scenario","name":"query-captures","graph":graph,"inputIdentity":prepared.inputs().identity(),"fullSourceAcceptance":false}))?;
    Ok(())
}
fn loads_case(out:&mut Output)->Result<()>{
    let files=["query-load-a.lua","query-load-b.lua"];
    let site=point(files[0],"local saved = Shared","Shared")?;
    let declaration=point(files[1],"function Shared(value)","Shared")?;
    for(name,order,occurrence,known)in [
        ("before",vec![files[0],files[1]],0,false),
        ("after",vec![files[1],files[0]],1,true),
        ("repeated_before",vec![files[0],files[1],files[0]],0,false),
        ("repeated_after",vec![files[0],files[1],files[0]],2,true),
    ]{
        let c=control();let prepared=prepare_case(&files,&order,&c)?;
        let graph=prepared.build(name,EvaluatorProfile::Native,None,&c).map_err(|e|e.to_string())?;
        let syntax=prepared.source_queries(&c).map_err(|e|e.to_string())?;
        let result=syntax.with_execution(&graph,QuerySelector::LoadOccurrence{occurrence},&c).map_err(|e|e.to_string())?.definitions(&site).map_err(|e|e.to_string())?;
        out.emit(json!({"event":"definition","case":"loads","query":name,"result":definition_json(&result)?}))?;
        out.check(&format!("query:loads:{name}:selected"),if known{json!(declaration)}else{Value::Null},function_site(&result))?;
        if !known{out.check(&format!("query:loads:{name}:known_absent"),json!(true),json!(matches!(result.selected,QueryFact::KnownAbsent{..})))?;}
        if name=="repeated_after"{
            let result=syntax.with_execution(&graph,QuerySelector::AllOccurrences,&c).map_err(|e|e.to_string())?.definitions(&site).map_err(|e|e.to_string())?;
            out.check("query:loads:repeated_requires_selector",json!(true),json!(matches!(result.selected,QueryFact::SelectorRequired{..})))?;
            out.emit(json!({"event":"ambiguous_definition","case":"loads","result":definition_json(&result)?}))?;
        }
        out.emit(json!({"event":"scenario","name":name,"graph":graph,"inputIdentity":prepared.inputs().identity(),"fullSourceAcceptance":false}))?;
    }
    Ok(())
}
fn paging_case(out:&mut Output)->Result<()>{
    let c=control();let prepared=prepare_case(&["query-roles.lua"],&["query-roles.lua"],&c)?;
    let graph=prepared.build("query-paging",EvaluatorProfile::Native,None,&c).map_err(|e|e.to_string())?;
    let syntax=prepared.source_queries(&c).map_err(|e|e.to_string())?;
    let query=syntax.with_execution(&graph,QuerySelector::LoadOccurrence{occurrence:0},&c).map_err(|e|e.to_string())?;
    let result=query.definitions(&point("query-roles.lua","local functionValueRead = saved","saved")?).map_err(|e|e.to_string())?;
    let QueryFact::Known{value}=result.selected else{return Err("paging selected value".into())};
    let target=QueryTarget::Value(value.value_id);let filter=ReferenceFilter{roles:vec![ReferenceRole::Read,ReferenceRole::Call]};
    let full=query.references(target.clone(),filter.clone(),PageRequest::default()).map_err(|e|e.to_string())?;
    let first=query.references(target.clone(),filter.clone(),PageRequest{limit:2,bytes:65536,cursor:None}).map_err(|e|e.to_string())?;
    let cursor=first.next_cursor.clone().ok_or("multiple page cursor")?;
    let mut joined=first.rows.clone();let mut next=first.next_cursor.clone();let mut pages=1usize;
    while let Some(cursor)=next{if pages>=20{return Err("page progress bound".into())}let page=query.references(target.clone(),filter.clone(),PageRequest{limit:2,bytes:65536,cursor:Some(cursor)}).map_err(|e|e.to_string())?;if page.rows.is_empty(){return Err("nonterminal empty page".into())}joined.extend(page.rows);next=page.next_cursor;pages+=1;}
    out.check("query:paging:exact_ordered_page_union",json!(full.rows.iter().map(|row|&row.id).collect::<Vec<_>>()),json!(joined.iter().map(|row|&row.id).collect::<Vec<_>>()))?;
    out.check("query:paging:no_duplicate_records",json!(joined.len()),json!(joined.iter().map(|row|&row.id).collect::<std::collections::BTreeSet<_>>().len()))?;
    let repeated=query.references(target.clone(),filter.clone(),PageRequest{limit:2,bytes:65536,cursor:None}).map_err(|e|e.to_string())?;
    out.check("query:paging:repeat_first_page_stable",json!(first.rows.iter().map(|row|&row.id).collect::<Vec<_>>()),json!(repeated.rows.iter().map(|row|&row.id).collect::<Vec<_>>()))?;
    let changed_filter=query.references(target.clone(),ReferenceFilter{roles:vec![ReferenceRole::Read]},PageRequest{limit:2,bytes:65536,cursor:Some(cursor.clone())}).err().map(|e|e.exit_code);
    out.check("query:paging:changed_filter_rejected",json!(4),json!(changed_filter))?;
    let changed_limit=query.references(target.clone(),filter.clone(),PageRequest{limit:3,bytes:65536,cursor:Some(cursor.clone())}).err().map(|e|e.exit_code);
    out.check("query:paging:changed_limit_rejected",json!(4),json!(changed_limit))?;
    let changed_target=query.references(QueryTarget::All,filter.clone(),PageRequest{limit:2,bytes:65536,cursor:Some(cursor.clone())}).err().map(|e|e.exit_code);
    out.check("query:paging:changed_target_rejected",json!(4),json!(changed_target))?;
    let all=syntax.with_execution(&graph,QuerySelector::AllOccurrences,&c).map_err(|e|e.to_string())?;
    let changed_selector=all.references(target.clone(),filter.clone(),PageRequest{limit:2,bytes:65536,cursor:Some(cursor.clone())}).err().map(|e|e.exit_code);
    out.check("query:paging:changed_selector_rejected",json!(4),json!(changed_selector))?;
    let foreign_prepared=prepare_case(&["query-roles.lua"],&["query-roles.lua"],&c)?;
    let foreign=foreign_prepared.source_queries(&c).map_err(|e|e.to_string())?;
    let foreign_cursor=foreign.references(QueryTarget::All,ReferenceFilter::default(),PageRequest{limit:2,bytes:65536,cursor:Some(cursor)}).err().map(|e|e.exit_code);
    out.check("query:paging:foreign_owner_rejected",json!(4),json!(foreign_cursor))?;
    let tiny=query.references(target.clone(),filter,PageRequest{limit:2,bytes:1,cursor:None}).err().map(|e|e.code);
    out.check("query:paging:single_row_budget_explicit",json!("source.native_query_record_too_large"),json!(tiny))?;
    let context=query.context(target,ContextOptions{depth:2,snippet_bytes:16},PageRequest::default()).map_err(|e|e.to_string())?;
    let exact=context.rows.iter().all(|row|{let(Some(origin),Some(exhibited),Some(snippet))=(row.origin.source(),row.exhibited.as_ref(),row.snippet.as_deref())else{return false};let source=text(&exhibited.file);source.get(exhibited.start as usize..exhibited.end as usize)==Some(snippet)&&exhibited.start>=origin.start&&exhibited.end<=origin.end});
    out.check("query:paging:context_snippets_exact_original_bytes",json!(true),json!(exact&&!context.rows.is_empty()))?;
    out.emit(json!({"event":"paging","pages":pages,"full":full,"joined":joined,"context":context}))?;
    c.cancel_handle().store(true,std::sync::atomic::Ordering::Release);
    let cancelled=query.definitions(&point("query-roles.lua","local functionValueRead = saved","saved")?).err().map(|e|json!({"code":e.code,"exit":e.exit_code}));
    out.check("query:paging:cancel_during_same_operation",json!({"code":"command.cancelled","exit":7}),json!(cancelled))?;
    Ok(())
}
fn syntax_paging_case(out:&mut Output)->Result<()>{
    let c=control();let prepared=prepare_case(&["query-captures.lua","query-roles.lua"],&[],&c)?;
    let query=prepared.source_queries(&c).map_err(|e|e.to_string())?;
    let full=query.references(QueryTarget::All,ReferenceFilter::default(),PageRequest{limit:200,bytes:1048576,cursor:None}).map_err(|e|e.to_string())?;
    out.check("query:syntax_paging:references_cover_both_files",json!(["query-captures.lua","query-roles.lua"]),json!(full.rows.iter().map(|row|row.site.file.as_str()).collect::<std::collections::BTreeSet<_>>()))?;
    out.check("query:syntax_paging:references_full_page_complete",json!(false),json!(full.has_more))?;
    let mut joined=Vec::new();let mut next=None;let mut pages=0;
    loop{if pages>=32{return Err("syntax reference page progress bound".into())}let page=query.references(QueryTarget::All,ReferenceFilter::default(),PageRequest{limit:7,bytes:65536,cursor:next}).map_err(|e|e.to_string())?;if page.has_more&&page.rows.is_empty(){return Err("syntax reference nonterminal empty page".into())}joined.extend(page.rows);next=page.next_cursor;pages+=1;if next.is_none(){break}}
    out.check("query:syntax_paging:references_exact_ordered_page_union",json!(full.rows),json!(joined))?;
    let full=query.calls(QueryTarget::All,PageRequest{limit:200,bytes:1048576,cursor:None}).map_err(|e|e.to_string())?;
    out.check("query:syntax_paging:calls_cover_both_files",json!(["query-captures.lua","query-roles.lua"]),json!(full.rows.iter().map(|row|row.site.file.as_str()).collect::<std::collections::BTreeSet<_>>()))?;
    out.check("query:syntax_paging:calls_full_page_complete",json!(false),json!(full.has_more))?;
    let mut joined=Vec::new();let mut next=None;let mut pages=0;
    loop{if pages>=32{return Err("syntax call page progress bound".into())}let page=query.calls(QueryTarget::All,PageRequest{limit:2,bytes:65536,cursor:next}).map_err(|e|e.to_string())?;if page.has_more&&page.rows.is_empty(){return Err("syntax call nonterminal empty page".into())}joined.extend(page.rows);next=page.next_cursor;pages+=1;if next.is_none(){break}}
    out.check("query:syntax_paging:calls_exact_ordered_page_union",json!(full.rows),json!(joined))?;
    Ok(())
}
fn vararg_assignment_case(out:&mut Output)->Result<()>{
    let file="query-vararg-assignment.lua";let c=control();let prepared=prepare_case(&[file],&[file],&c)?;
    let graph=prepared.build("query-vararg-assignment",EvaluatorProfile::Native,None,&c).map_err(|e|e.to_string())?;
    let syntax=prepared.source_queries(&c).map_err(|e|e.to_string())?;
    let query=syntax.with_execution(&graph,QuerySelector::LoadOccurrence{occurrence:0},&c).map_err(|e|e.to_string())?;
    for(label,marker,token)in [
        ("first","local observedFirst = first","first"),
        ("second","local observedSecond = second","second"),
        ("third","local observedThird = third","third"),
        ("shifted_second","local observedShiftedSecond = shiftedSecond","shiftedSecond"),
        ("shifted_third","local observedShiftedThird = shiftedThird","shiftedThird"),
        ("assigned_first","local observedAssignedFirst = assignedFirst","assignedFirst"),
        ("assigned_second","local observedAssignedSecond = assignedSecond","assignedSecond"),
        ("assigned_third","local observedAssignedThird = assignedThird","assignedThird"),
    ]{
        let result=query.definitions(&point(file,marker,token)?).map_err(|e|e.to_string())?;
        out.check(&format!("query:vararg_assignment:{label}:unknown_not_nil"),json!("variadic_argument_slot_unavailable"),match &result.selected{QueryFact::Unavailable{reason}=>json!(reason),_=>json!(result.selected)})?;
    }
    let result=query.definitions(&point(file,"local observedShiftedFirst = shiftedFirst","shiftedFirst")?).map_err(|e|e.to_string())?;
    out.check("query:vararg_assignment:fixed_prefix_retained",json!(point(file,"local shiftedFirst, shiftedSecond, shiftedThird = 7, ...","7")?),match &result.selected{QueryFact::Known{value}=>json!(value.origin),_=>Value::Null})?;
    for name in ["ReturnTail","CallTail"]{
        let EntryResolution::Unique{candidate}=prepared.entry_symbol_candidates(file,name,&c).map_err(|e|e.to_string())? else{return Err("unique vararg entry expected".into())};
        let entry=EntryRequest{candidate:candidate.id,load_occurrence:None,creation_context:None,arguments:EntryArguments::Unspecified};
        let failure=prepared.build_plan("query-vararg-open-tail","after_load",LoadPlan::Known(vec![file.into()]),Some(entry),None,&c).err().ok_or("open variadic sequence accepted")?;
        out.check(&format!("query:vararg_assignment:{name}:open_sequence_boundary"),json!(true),json!(failure.to_string().contains("native invocation stopped at variadic argument sequence boundary")))?;
    }
    Ok(())
}
fn empty_function_case(out:&mut Output)->Result<()>{
    let file="query-empty-function.lua";let c=control();let prepared=prepare_case(&[file],&[file],&c)?;
    let graph=prepared.build("query-empty-function",EvaluatorProfile::Native,None,&c).map_err(|e|e.to_string())?;
    let facts=graph.native_invocations.as_ref().ok_or("empty function native frames")?;
    for(label,marker,call_text)in [("named","one(1)","one(1)"),("anonymous","anonymous()","anonymous()"),("method","\nobject:empty()\n","object:empty()")]{
        let site=point(file,marker,call_text)?;
        let call=graph.calls.iter().find(|call|call.site==site&&call.context.starts_with("load:")).ok_or("empty function call")?;
        let frame=facts.frames.iter().find(|frame|frame.call==Some(call.id)).ok_or("empty function invocation")?;
        out.check(&format!("query:empty_function:{label}:normal_zero_return"),json!({"returns":0,"paths":1,"continue":true,"effects_complete":true,"writes":0,"body_coverage":"return_expressions_only"}),json!({"returns":frame.return_facts.len(),"paths":frame.return_paths.len(),"continue":frame.return_paths.iter().all(|path|matches!(path.outcome,NativeControl::Continue)),"effects_complete":frame.effects_complete,"writes":call.effect_write_versions.len(),"body_coverage":graph.functions[frame.function].body_check_coverage}))?;
    }
    let syntax=prepared.source_queries(&c).map_err(|e|e.to_string())?;
    let query=syntax.with_execution(&graph,QuerySelector::LoadOccurrence{occurrence:0},&c).map_err(|e|e.to_string())?;
    let mut nil_slots=Vec::new();
    for(marker,token)in [("local observedFirst = first","first"),("local observedSecond = second","second"),("local observedScalar = scalar","scalar"),("local observedExisting = existing","existing"),("local observedTailFirst = tailFirst","tailFirst"),("local observedTailSecond = tailSecond","tailSecond"),("local observedTailArgA = tailArgA","tailArgA"),("local observedTailArgB = tailArgB","tailArgB"),("local observedScalarArgA = scalarArgA","scalarArgA")]{
        let result=query.definitions(&point(file,marker,token)?).map_err(|e|e.to_string())?;
        nil_slots.push(matches!(result.selected,QueryFact::KnownAbsent{..}));
    }
    out.check("query:empty_function:scalar_and_assignment_nil",json!(vec![true;9]),json!(nil_slots))?;
    let result=query.definitions(&point(file,"local observedScalarArgB = scalarArgB","scalarArgB")?).map_err(|e|e.to_string())?;
    out.check("query:empty_function:non_tail_following_argument",json!(point(file,"local scalarArgA, scalarArgB = sink(one(7), 9)","9")?),match &result.selected{QueryFact::Known{value}=>json!(value.origin),_=>Value::Null})?;
    let mut arities=Vec::new();for text in ["sink(one(6))","sink(one(7), 9)"]{let site=point(file,text,text)?;arities.push(graph.calls.iter().find(|call|call.site==site&&call.context.starts_with("load:")).ok_or("empty function argument call")?.arguments.len());}
    out.check("query:empty_function:tail_and_non_tail_argument_arity",json!([0,2]),json!(arities))?;
    let forward=point(file,"local tailFirst, tailSecond = forward()","forward()")?;
    let call=graph.calls.iter().find(|call|call.site==forward&&call.context.starts_with("load:")).ok_or("forward call")?;
    let frame=facts.frames.iter().find(|frame|frame.call==Some(call.id)).ok_or("forward frame")?;
    out.check("query:empty_function:tail_return_zero_values",json!({"returns":0,"explicit_empty_return":true}),json!({"returns":frame.return_facts.len(),"explicit_empty_return":frame.return_paths.len()==1&&matches!(&frame.return_paths[0].outcome,NativeControl::Return(values) if values.is_empty())}))?;
    let EntryResolution::Unique{candidate}=prepared.entry_symbol_candidates(file,"one",&c).map_err(|e|e.to_string())? else{return Err("unique empty entry expected".into())};
    let entry=EntryRequest{candidate:candidate.id,load_occurrence:None,creation_context:None,arguments:EntryArguments::Exact(vec![EntryArgument::Number(1.0)])};
    let entered=prepared.build_plan("query-empty-entry","after_load",LoadPlan::Known(vec![file.into()]),Some(entry),None,&c).map_err(|e|e.to_string())?;
    let entry=entered.native_invocations.as_ref().and_then(|facts|facts.entries.first()).ok_or("empty entry fact")?;
    out.check("query:empty_function:explicit_entry_zero_values",json!({"returns":0,"effects_complete":true,"writes":0}),json!({"returns":entry.return_facts.len(),"effects_complete":entry.effects_complete,"writes":entry.effect_write_versions.len()}))?;
    Ok(())
}
fn run(out:&mut Output)->Result<()>{
    out.emit(json!({"event":"build_identity","compiledPatchSHA256":option_env!("LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256")}))?;
    for(file,source)in FILES{out.emit(json!({"event":"input_identity","file":file,"bytes":source.len(),"sha256":hash(source.as_bytes())}))?;}
    lexical_case(out)?;loop_scope_case(out)?;roles_case(out)?;captures_case(out)?;loads_case(out)?;paging_case(out)?;syntax_paging_case(out)?;vararg_assignment_case(out)?;empty_function_case(out)
}
fn main(){let mut out=Output{checked:0,failed:0,bytes:0};let result=run(&mut out);let complete=result.is_ok();let error=result.err();
    if let Err(e)=out.emit(json!({"event":"end","complete":complete,"error":error,"checked":out.checked,"failed":out.failed,"fullSourceAcceptance":false})){eprintln!("{e}");std::process::exit(1)}
    if !complete||out.failed!=0{std::process::exit(1)}
}
