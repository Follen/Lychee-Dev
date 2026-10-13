//! Fixed whole-source research inputs and a reusable single-thread native snapshot.
use crate::{environment,index,native::*};
use lycheedev_core::{Result,ToolkitError,control::OperationControl,storage};
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::{collections::BTreeMap,path::PathBuf};

mod xml;
pub(crate) use xml::XmlResearch;
pub const RESEARCH_RULES:&str="source-native-research-v3-load-context";
#[derive(Clone,Serialize,Deserialize)]
pub struct LoadEvidence { pub manifest:String,pub ordered:Vec<String>,pub issues:Vec<Value> }
#[derive(Clone,Serialize,Deserialize)]
pub struct SnapshotRequest {
    pub files:BTreeMap<String,String>,pub load_documents:BTreeMap<String,String>,pub source:Value,pub luac:LuacIdentity,
    pub api:Vec<environment::ApiRecord>,pub environment_manifest:Option<environment::Manifest>,
    pub load_plans:Vec<LoadEvidence>,pub input_gaps:Vec<Value>,
}
#[derive(Clone,Serialize,Deserialize)]
pub struct QueryRequest {pub symbol:index::Symbol,pub direction:String}
#[derive(Serialize,Deserialize)]
pub struct WorkerRequest {pub snapshot:SnapshotRequest,pub query:QueryRequest}
impl SnapshotRequest {
    /// Hash the same JSON representation without holding a second whole-source
    /// serialization beside a live analysis in the bounded worker.
    pub(crate) fn cache_key(&self,control:&OperationControl)->Result<String> {
        struct HashWriter<'a>{hash:Sha256,control:&'a OperationControl}
        impl std::io::Write for HashWriter<'_>{
            fn write(&mut self,bytes:&[u8])->std::io::Result<usize>{self.control.check().map_err(std::io::Error::other)?;self.hash.update(bytes);Ok(bytes.len())}
            fn flush(&mut self)->std::io::Result<()>{Ok(())}
        }
        let mut writer=HashWriter{hash:Sha256::new(),control};
        let result=serde_json::to_writer(&mut writer,self);control.check()?;result?;
        Ok(format!("{:x}",writer.hash.finalize()))
    }
}
struct ResearchMetadata {
    api:Vec<environment::ApiRecord>,environment_manifest:Option<environment::Manifest>,
    load_plans:Vec<LoadEvidence>,input_gaps:Vec<Value>,
}
pub struct ResearchSession {snapshot:ResearchMetadata,prepared:PreparedSnapshot,xml:XmlResearch}
impl ResearchSession {
    pub fn prepare(snapshot:SnapshotRequest,control:&OperationControl)->Result<Self> {
        Self::prepare_observed(snapshot,control,|_|{})
    }
    pub fn prepare_observed(snapshot:SnapshotRequest,control:&OperationControl,observer:impl FnMut(&'static str))->Result<Self> {
        control.check()?;
        let SnapshotRequest{mut files,load_documents,source,luac,api,environment_manifest,load_plans,input_gaps}=snapshot;
        let xml=XmlResearch::prepare(&mut files,load_documents,&load_plans,&source,control)?;
        let xml_identity=xml.identity(control)?;
        let host=api_environment(&api,environment_manifest.as_ref(),control)?;
        let files=files.into_iter().map(|(path,text)|{
            let content_sha256=storage::sha256(text.as_bytes());
            let analysis_utf8=text.strip_prefix('\u{feff}').unwrap_or(&text).into();
            let map=if text.starts_with('\u{feff}'){SourceMap::Utf8BomPrefix}else{SourceMap::Identity};
            SourceFileInput{path,original_utf8:text,content_sha256,analysis_utf8,map}
        }).collect();
        let mut config=emmylua_code_analysis::Emmyrc::default();config.runtime.version=emmylua_code_analysis::EmmyrcLuaVersion::Lua51;
        let inputs=SourceInputs::admit(SourceInputSpec{files,workspace_root:PathBuf::from("C:/lycheedev/pinned-source-research"),source_identity:json!({"source":source,"environment":environment_manifest,"researchRules":RESEARCH_RULES,"xml":xml_identity}),phase:"unknown".into(),ordered_occurrences:vec![],config,host,luac,limits:SourceLimits::research()},control)?;
        let prepared=prepare_observed(inputs,control,observer)?;
        Ok(Self{snapshot:ResearchMetadata{api,environment_manifest,load_plans,input_gaps},prepared,xml})
    }
    pub fn query(&mut self,query:QueryRequest,control:&OperationControl)->Result<Value> {
        self.query_observed(query,control,|_|{})
    }
    pub fn query_observed(&mut self,query:QueryRequest,control:&OperationControl,mut observer:impl FnMut(&'static str))->Result<Value> {
        self.prepared.begin_research_request(control)?;
        observer("query_inputs_renewed");
        if !["incoming","outgoing","both"].contains(&query.direction.as_str()){return Err(ToolkitError::arguments("invalid native relation direction"));}
        let programs=self.xml.programs.iter().filter(|program|program.documents.contains_key(&query.symbol.path)).collect::<Vec<_>>();
        let mut gaps=self.snapshot.input_gaps.clone();gaps.extend(self.xml.gaps.clone());
        for plan in &self.snapshot.load_plans{if plan.ordered.contains(&query.symbol.path)||programs.iter().any(|program|program.manifest==plan.manifest){for issue in &plan.issues{let mut issue=issue.clone();issue["manifest"]=json!(plan.manifest);gaps.push(issue);}}}

        let mut rows=Vec::new();let mut scopes=Vec::new();let mut complete=true;let mut analysis=json!({});let mut entry_candidates=BTreeMap::new();let mut entry_facts=Vec::new();
        for program in &programs {
            control.check()?;

            observer("query_build_scope");
            let graph=match self.prepared.build_xml_plan("source-research-load",(*program).clone(),None,control){
                Ok(graph)=>graph,Err(error)=>{control.check()?;gaps.push(json!({"code":error.code,"detail":error.message,"manifest":program.manifest}));continue;}
            };
            observer("query_scope_built");
            let mut requests=Vec::new();
            if query.direction!="incoming" {
                let entries=self.prepared.entry_catalog(control)?;
                if let (Some(xml),Some(native))=(&graph.xml,&graph.native_invocations){for handler in &xml.handlers{
                    let Some(instance)=handler.closure.and_then(|id|native.closures.get(id))else{continue};
                    let function=&graph.functions[instance.function];
                    if !self.xml.matches_function(&self.prepared,&query.symbol,&function.declaration)?&&!self.xml.owns_handler(&query.symbol,handler,program)?{continue;}
                    let candidate=entries.iter().find(|candidate|candidate.closure==function.closure).expect("registered source closure");
                    requests.push(EntryRequest{candidate:candidate.id.clone(),load_occurrence:Some(instance.load_occurrence),creation_context:Some(instance.creation_context.clone()),arguments:EntryArguments::XmlHandler(handler.id)});
                }}
                for candidate in entries{
                    if requests.iter().any(|request|request.candidate==candidate.id){continue;}
                    let matches=graph.functions.iter().any(|function|function.closure==candidate.closure&&self.xml.matches_function(&self.prepared,&query.symbol,&function.declaration).unwrap_or(false));
                    if !matches{continue;}
                    if let Some(native)=&graph.native_invocations{for instance in &native.closures{
                        if graph.functions[instance.function].closure==candidate.closure{requests.push(EntryRequest{candidate:candidate.id.clone(),load_occurrence:Some(instance.load_occurrence),creation_context:Some(instance.creation_context.clone()),arguments:EntryArguments::Unspecified});}
                    }}
                }
            }
            for request in &requests{if let Some(candidate)=self.prepared.entry_catalog(control)?.iter().find(|candidate|candidate.id==request.candidate){entry_candidates.insert(candidate.id.clone(),candidate.clone());}}
            if let Some(xml)=&graph.xml{gaps.extend(xml.gaps.clone());}
            let mut loaded=if requests.is_empty(){Some(graph)}else{drop(graph);None};
            let requests=if requests.is_empty(){vec![None]}else{requests.into_iter().map(Some).collect()};
            for request in requests {
                let graph=if let Some(request)=request{observer("query_build_entry");match self.prepared.build_xml_plan("source-research-entry",(*program).clone(),Some(request),control){
                    Ok(graph)=>graph,Err(error)=>{control.check()?;gaps.push(json!({"code":error.code,"detail":error.message,"manifest":program.manifest}));continue;}
                }}else{loaded.take().expect("load-only graph")};
                observer("query_project");
                let mut result=self.prepared.research_relations(&query.symbol,&query.direction,Some(&graph),&self.xml)?;
                observer("query_projected");
                complete&=result["analysis"]["state"]=="complete";analysis=result["analysis"].clone();
                if let Some(native)=&graph.native_invocations{entry_facts.extend(native.entries.iter().map(|entry|json!(entry)));}
                let scope=json!({"manifest":program.manifest,"loadContext":program.context,"snapshot":graph.snapshot_key,"entries":graph.native_invocations.as_ref().map(|facts|&facts.entries),"kind":"alternative_manifest_hypothetical_after_load"});
                for mut row in result["relations"].as_array_mut().expect("relation rows").drain(..){row["loadScope"]=scope.clone();rows.push(row);}
                let mut scope_result=json!({"xmlFacts":graph.xml});
                scope_result["scope"]=scope;scope_result["analysis"]=result["analysis"].take();
                scopes.push(scope_result);
            }
        }
        if programs.is_empty(){
            gaps.push(json!({"code":"load_order_unknown","detail":"no TOC/XML program contains the selected declaration"}));
            observer("query_project");
            let mut result=self.prepared.research_relations(&query.symbol,&query.direction,None,&self.xml)?;
            observer("query_projected");
            rows=take_relation_rows(&mut result);analysis=result["analysis"].take();complete=false;
        }
        let mut candidates=entry_candidates.into_values().collect::<Vec<_>>();
        let entry_resolution=match candidates.len(){0=>EntryResolution::Absent,1=>EntryResolution::Unique{candidate:candidates.remove(0)},_=>EntryResolution::SelectorRequired{candidates}};
        analysis["state"]=json!(if complete&&gaps.is_empty(){"complete"}else{"partial"});
        observer("query_assemble_reply");
        let header=json!({"schema":"lycheedev.source-native-research.v1",
            "inputIdentity":self.prepared.inputs().identity_sha256(),"loadEvidence":self.snapshot.load_plans,
            "loadContexts":self.xml.programs.iter().map(|program|json!({"manifest":program.manifest,"context":program.context})).collect::<Vec<_>>(),
            "entryResolution":entry_resolution,
            "entryEvidence":{"kind":"hypothetical_selected_declaration_entry","phase":"after_load","sourceExecution":false,"xmlSelf":"exact_registered_instance"}});
        let has_gaps=!gaps.is_empty();
        let mut output=assemble_reply(header,rows,scopes,self.xml.mapping_manifests(),gaps,entry_facts,analysis);
        output["environmentManifest"]=json!(self.snapshot.environment_manifest);
        // Declaration/type metadata is immutable evidence. It does not assert that a
        // mutable global lookup still points at the initial client declaration.
        let mut names=std::collections::BTreeSet::from([query.symbol.name.as_str()]);
        if let Some(rows)=output["relations"].as_array(){for row in rows{
            if let Some(name)=row["target"].as_str(){names.insert(name);}
            if let Some(name)=row["source"].as_str(){names.insert(name);}
        }}
        let declarations=self.snapshot.api.iter().filter(|record|names.contains(record.name.as_str())).collect::<Vec<_>>();
        let descriptors=json!(declarations);
        output["apiDeclarations"]=descriptors;
        output["analysis"]["apiDeclarationCount"]=json!(self.snapshot.api.len());
        output["analysis"]["cacheHit"]=json!(false);
        if has_gaps{output["analysis"]["state"]=json!("partial");}
        observer("query_reply_assembled");
        Ok(output)
    }
}

// These are already-owned JSON trees. json! would serialize borrowed values
// into a second tree while the originals (including all source maps) are live.
fn assemble_reply(mut header:Value,rows:Vec<Value>,scopes:Vec<Value>,maps:Vec<Value>,gaps:Vec<Value>,entries:Vec<Value>,analysis:Value)->Value {
    header["relations"]=Value::Array(rows);
    header["loadScopes"]=Value::Array(scopes);
    header["sourceMaps"]=Value::Array(maps);
    header["gaps"]=Value::Array(gaps);
    header["entryEvidence"]["entries"]=Value::Array(entries);
    header["analysis"]=analysis;
    header
}
fn take_relation_rows(result:&mut Value)->Vec<Value> {
    match result["relations"].take(){Value::Array(rows)=>rows,_=>panic!("relation rows")}
}

pub(crate) fn api_environment(api:&[environment::ApiRecord],manifest:Option<&environment::Manifest>,control:&OperationControl)->Result<TrustedHostEnvironment>{
    // These associations are audited against the actual self-copy declarations,
    // not inferred by stripping an API suffix. The source spans remain in the
    // authenticated catalog so callers can inspect the supporting records.
    let mut nominal_objects=BTreeMap::new();
    for (owner,name) in [("LuaDurationObjectAPI","LuaDurationObject"),("LuaCurveObjectAPI","LuaCurveObject")] {
        let roots=api.iter().filter(|record|record.kind=="script-object"&&record.name==owner&&record.raw.get("ObjectType")==Some(&json!("Userdata"))).collect::<Vec<_>>();
        let copies=api.iter().filter(|record|record.kind=="function"&&record.name=="Copy"&&record.owner.as_ref().is_some_and(|record|record.kind=="script-object"&&record.name==owner)
            &&record.parameters.is_empty()&&record.returns.len()==1&&record.returns[0].type_name==name&&!record.returns[0].nilable&&record.raw.get("MayReturnNothing")!=Some(&Value::Bool(true))).collect::<Vec<_>>();
        if let ([root],[copy])=(roots.as_slice(),copies.as_slice()){
            nominal_objects.insert(name.to_owned(),json!({"owner":owner,"evidence":{"rule":"audited-self-copy-runtime-type-v1","root":root.observed_span,"copy":copy.observed_span}}));
        }
    }
    let mut counts=BTreeMap::<&str,usize>::new();for record in api{if record.kind=="function"&&record.owner.is_none(){*counts.entry(&record.name).or_default()+=1;}}
    let mut callables=Vec::new();let mut bindings=Vec::new();
    for record in api {
        control.check()?;
        if record.kind!="function"||record.owner.is_some()||counts[record.name.as_str()]!=1{continue}
        // The initial global namespace accepts literal Lua identifiers only. Other
        // declarations remain available as metadata and method candidates.
        if !record.name.split('.').all(|part|!part.is_empty()&&part.bytes().enumerate().all(|(index,byte)|byte==b'_'||byte.is_ascii_alphabetic()||(index>0&&byte.is_ascii_digit()))){continue}
        let id=callables.len();
        let mut declaration=json!({"name":record.name,"identity":format!("{}:{}:{}",record.path,record.line,record.name),"effect":"unknown","sourceExecution":false,"descriptor":record});
        if record.returns.len()<=32&&record.raw.get("MayReturnNothing")!=Some(&Value::Bool(true)){declaration["returnArity"]=json!(record.returns.len());}
        callables.push(declaration);bindings.push(HostBindingInput::Callable{declaration:HostMaterialRef{material:"client-api".into(),pointer:format!("/callables/{id}")},rules:None});
    }
    let mut method_counts=BTreeMap::new();
    for record in api{if let Some(owner)=record.owner.as_ref().filter(|owner|owner.kind=="script-object")
        && record.kind=="function"{*method_counts.entry((owner.name.as_str(),record.name.as_str())).or_insert(0usize)+=1;}
    }
    let mut methods=Vec::new();
    for record in api{
        control.check()?;
        let Some(owner)=record.owner.as_ref().filter(|owner|owner.kind=="script-object")else{continue};
        if record.kind!="function"||method_counts.get(&(owner.name.as_str(),record.name.as_str()))!=Some(&1)
            ||!nominal_objects.values().any(|object|object["owner"]==owner.name){continue;}
        let id=methods.len();
        let mut declaration=json!({"name":format!("{}.{}",owner.name,record.name),"identity":format!("{}:{}:{}",record.path,record.line,record.name),"effect":"unknown","sourceExecution":false,"descriptor":record});
        if record.returns.len()<=32&&record.raw.get("MayReturnNothing")!=Some(&Value::Bool(true)){declaration["returnArity"]=json!(record.returns.len());}
        methods.push(declaration);bindings.push(HostBindingInput::Method{declaration:HostMaterialRef{material:"client-api".into(),pointer:format!("/methods/{id}")}});
    }
    let type_declarations=api.iter().filter(|record|!["function","event"].contains(&record.kind.as_str())).collect::<Vec<_>>();
    // These fixed host aliases are the existing environment generator policy.
    // They are authenticated with the catalog; projection stays in the library.
    let type_aliases=json!({"UnitToken":"string","UnitTokenVariant":"string","luaIndex":"integer","fileID":"integer","textureID":"integer"});
    let bytes=serialize_json(&json!({"sourceExecution":false,"configExecution":false,"callables":callables,"methods":methods,"nominalObjects":nominal_objects,"typeDeclarations":type_declarations,"typeAliases":type_aliases,"environment":manifest}),control,32<<20)?;
    TrustedHostEnvironment::from_host_material("pinned-client-research-v1".into(),false,vec![HostMaterialInput{name:"client-api".into(),expected_sha256:storage::sha256(&bytes),bytes}],vec![],control)?.with_bindings(bindings,control)
}

pub fn worker_stdio()->Result<()> {
    use std::io::{Read,Write};
    let control=OperationControl::new(std::time::Instant::now()+std::time::Duration::from_secs(180),std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)));
    let mut bytes=Vec::new();std::io::stdin().take((768u64<<20)+1).read_to_end(&mut bytes)?;
    if bytes.len()>768<<20{return Err(ToolkitError::new("source.native_budget","native worker input exceeds budget",3));}
    let request:WorkerRequest=serde_json::from_slice(&bytes)?;
    drop(bytes);
    let mut session=ResearchSession::prepare(request.snapshot,&control)?;
    let result=session.query(request.query,&control)?;
    let bytes=serde_json::to_vec(&result)?;
    if bytes.len()>32<<20{return Err(ToolkitError::new("source.native_budget","native worker result exceeds budget",3));}
    std::io::stdout().write_all(&bytes)?;Ok(())
}

/// Parent-side immutable bytes. The same request is used by one-shot and session owners.
pub fn snapshot_request(inv:&lycheedev_core::Invocation,pin:&lycheedev_core::selection::SourcePin,index:&index::Index,environment:Option<&environment::Environment>,runtime:&crate::luac::Runtime)->Result<SnapshotRequest> {
    snapshot_material(inv,pin,index,environment,runtime).map(|(snapshot,_)|snapshot)
}
/// Keep the full authenticated XML/TOC observations on the parent side for
/// context output; the worker only needs the load plans derived from them.
pub(crate) fn snapshot_material(inv:&lycheedev_core::Invocation,pin:&lycheedev_core::selection::SourcePin,index:&index::Index,environment:Option<&environment::Environment>,runtime:&crate::luac::Runtime)->Result<(SnapshotRequest,Vec<index::LoadFact>)> {
    let tree=crate::research_material::measured("source.snapshot.git_tree", || crate::git::tree_with_control(&inv.home,pin,&inv.control))?;
    let documents=tree.into_iter().filter(|file|[".lua",".xml",".toc"].iter().any(|extension|file.path.to_ascii_lowercase().ends_with(extension))).collect::<Vec<_>>();
    let limits=SourceLimits::research();
    if documents.len()>limits.files||documents.iter().map(|file|file.bytes).sum::<usize>()>limits.input_bytes{return Err(ToolkitError::new("source.native_budget","whole pinned source exceeds research admission budget",3));}
    let mut files=BTreeMap::new();let mut load_documents=BTreeMap::new();let mut gaps=Vec::new();
    crate::research_material::measured("source.snapshot.source_blobs", || crate::git::visit_blobs_with_control(&inv.home,pin,&documents,&inv.control,|file,raw|{
        let text=String::from_utf8(raw).map_err(|_|ToolkitError::new("codebase.source_integrity","source document is not UTF8",4))?;
        if file.path.to_ascii_lowercase().ends_with(".lua"){files.insert(file.path.clone(),text);}else{load_documents.insert(file.path.clone(),text);}Ok(())
    }))?;
    for file in &documents{if !files.contains_key(&file.path)&&!load_documents.contains_key(&file.path){gaps.push(json!({"code":"source_file_unavailable","path":file.path}));}}
    let facts=crate::research_material::measured("source.snapshot.load_facts", || index::load_facts_from_documents_with_control(&inv.home,index,&load_documents,&inv.control))?;
    let mut loads=BTreeMap::<String,Vec<&index::LoadFact>>::new();for fact in &facts{loads.entry(fact.path.clone()).or_default().push(fact);}
    let mut plans=Vec::new();
    let source=serde_json::to_value(pin)?;
    let paths=crate::xml_load::LoadPaths::new(files.keys().chain(load_documents.keys()).map(String::as_str),&inv.control)?;
    crate::research_material::measured("source.snapshot.load_plans", || { for doc in &index.documents{if doc.path.to_ascii_lowercase().ends_with(".toc"){
        let mut plan=LoadEvidence{manifest:doc.path.clone(),ordered:vec![],issues:vec![]};
        let context=crate::xml_load::LoadContext::for_manifest(&doc.path,load_documents[&doc.path].as_bytes(),Some(&source));
        let inputs=LoadInputs{loads:&loads,files:&files,documents:&load_documents,context:&context,paths:&paths};
        walk_load(&doc.path,&inputs,&mut std::collections::BTreeSet::new(),&mut plan,&inv.control)?;plans.push(plan);
    }} Ok(()) })?;
    drop(paths);
    let manifest=runtime.manifest();
    Ok((SnapshotRequest{files,load_documents,source,luac:LuacIdentity{version:manifest.version.clone(),platform:manifest.platform.clone(),executable_sha256:manifest.sha256.clone(),executable_bytes:manifest.bytes},api:environment.map(|environment|environment.facts.clone()).unwrap_or_default(),environment_manifest:environment.map(|environment|environment.manifest.clone()),load_plans:plans,input_gaps:gaps},facts))
}
struct LoadInputs<'a>{loads:&'a BTreeMap<String,Vec<&'a index::LoadFact>>,files:&'a BTreeMap<String,String>,documents:&'a BTreeMap<String,String>,context:&'a crate::xml_load::LoadContext,paths:&'a crate::xml_load::LoadPaths<'a>}
fn walk_load(path:&str,inputs:&LoadInputs<'_>,active:&mut std::collections::BTreeSet<String>,plan:&mut LoadEvidence,control:&OperationControl)->Result<()> {
    use crate::xml_load::LoadDecision;
    control.check()?;
    if plan.ordered.len()>=20000{plan.issues.push(json!({"code":"load_occurrence_budget","path":path}));return Ok(())}
    if path.to_ascii_lowercase().ends_with(".lua"){
        if inputs.files.contains_key(path){plan.ordered.push(path.into())}else{plan.issues.push(json!({"code":"load_file_missing","path":path}));}return Ok(())
    }
    let Some(raw)=inputs.documents.get(path)else{plan.issues.push(json!({"code":"load_file_missing","path":path}));return Ok(())};
    if path.to_ascii_lowercase().ends_with(".toc") {match inputs.context.manifest_admission(raw.as_bytes()){
        LoadDecision::Include(_)=>{},LoadDecision::Exclude=>return Ok(()),LoadDecision::Unavailable(reason)=>{plan.issues.push(json!({"code":"toc_manifest_condition_unavailable","path":path,"reason":reason}));return Ok(())}
    }}
    if !active.insert(path.into()){plan.issues.push(json!({"code":"load_cycle","path":path}));return Ok(())}
    if let Some(edges)=inputs.loads.get(path){for edge in edges{
        match inputs.context.target(&edge.target){
            LoadDecision::Include(target)=>if let Some(target)=inputs.paths.resolve(path,&target,|path|inputs.files.contains_key(path)||inputs.documents.contains_key(path)){
                walk_load(&target,inputs,active,plan,control)?;
            }else{plan.issues.push(json!({"code":"load_file_missing","path":path,"target":edge.target,"resolvedTarget":target,"line":edge.line}));},
            LoadDecision::Exclude=>{},
            LoadDecision::Unavailable(reason)=>plan.issues.push(json!({"code":"load_condition_unavailable","path":path,"target":edge.target,"line":edge.line,"reason":reason})),
        }
    }}
    active.remove(path);Ok(())
}

pub fn one_shot(inv:&lycheedev_core::Invocation,request:WorkerRequest)->Result<Value> {
    let bytes=serde_json::to_vec(&request)?;
    if bytes.len()>768<<20{return Err(ToolkitError::new("source.native_budget","native research input exceeds transport budget",3));}
    let mut command=std::process::Command::new(std::env::current_exe()?);command.arg("--source-native-worker");
    let output=crate::process::run_controlled(command,32<<20,inv.control.remaining()?.min(std::time::Duration::from_secs(180)),Some(bytes),Some(inv.control.token()))?;
    inv.control.check()?;
    if output.status!=0{return Err(ToolkitError::new("source.native_worker_failed",String::from_utf8_lossy(&output.stderr).into_owned(),5));}
    let mut result:Value=serde_json::from_slice(&output.stdout)?;
    if result["schema"]!="lycheedev.source-native-research.v1"{return Err(ToolkitError::new("source.native_worker_protocol","native research worker returned an invalid schema",4));}
    result["lifecycle"]=json!({"owner":"source_owned_process","retired":true,"mode":"one_shot"});Ok(result)
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn indexed_projection_preserves_eager_rows_and_cross_xml_owners() {
        let control=OperationControl::default();
        let files=BTreeMap::from([
            ("logic.lua".into(),"\u{feff}-- 中文 😀\r\nfunction Target(x)\r\n return x\r\nend\r\nAlias=Target\r\nfunction Caller()\r\n local a=Target(1)\r\n return Alias(a)\r\nend\r\nMixin={OnLoad=Caller}\r\nCaller()\r\nCaller()\r\n".into()),
            ("outside.lua".into(),"local foreign=Target\nfunction Outside() return Target(4) end\n".into()),
        ]);
        let documents=BTreeMap::from([
            ("Main.toc".into(),"logic.lua\nTemplates.xml\nObjects.xml\n".into()),
            ("Templates.xml".into(),"\u{feff}<Ui>\r\n<Frame name=\"Template\" virtual=\"true\"><Scripts><OnLoad>local text = &quot;😀&quot;&#10;Target(self); Target(self)</OnLoad></Scripts></Frame>\r\n</Ui>\r\n".into()),
            ("Objects.xml".into(),"<Ui>\r\n<Frame name=\"Actual\" inherits=\"Template\" mixin=\"Mixin\"/>\r\n</Ui>\r\n".into()),
        ]);
        let snapshot=SnapshotRequest{
            files,load_documents:documents,source:json!({"repository":"projection-test","product":"retail"}),
            luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
            api:vec![],environment_manifest:None,
            load_plans:vec![LoadEvidence{manifest:"Main.toc".into(),ordered:vec!["logic.lua".into()],issues:vec![]}],input_gaps:vec![],
        };
        let session=ResearchSession::prepare(snapshot,&control).unwrap();
        let graph=session.prepared.build_xml_plan("projection-test",session.xml.programs[0].clone(),None,&control).unwrap();
        assert!(!graph.uses.is_empty());
        for(name,path,line,end_line)in [("Target","logic.lua",2,4),("Caller","logic.lua",6,9),("Mixin","logic.lua",10,10),("Actual","Objects.xml",2,2)]{
            let symbol=index::Symbol{id:String::new(),kind:"function".into(),name:name.into(),target:String::new(),category:String::new(),confidence:String::new(),path:path.into(),line,end_line,signature:String::new(),scope:String::new(),ordinal:0};
            for direction in ["incoming","outgoing","both"]{
                let old=session.prepared.eager_research_relations(&symbol,direction,Some(&graph),&session.xml).unwrap();
                let new=session.prepared.research_relations(&symbol,direction,Some(&graph),&session.xml).unwrap();
                assert_eq!(serde_json::to_vec(&new).unwrap(),serde_json::to_vec(&old).unwrap(),"{name} {direction}");
                if name=="Target"&&direction=="incoming"{assert!(new["relations"].as_array().unwrap().iter().any(|row|row["path"]=="outside.lua"),"unloaded source references remain visible");}
                if name=="Actual"&&direction=="outgoing"{assert!(new["relations"].as_array().unwrap().iter().any(|row|row["path"]=="Templates.xml"),"inherited handler keeps original XML source and selected owner");}
            }
        }
    }
}

#[cfg(test)]
mod load_context_tests {
    use super::*;
    use crate::xml_load::{GameMode,LoadContext,LoadFamily,Program};

    fn plan(manifest:&str,source:&Value,files:&BTreeMap<String,String>,documents:&BTreeMap<String,String>,facts:&[index::LoadFact],control:&OperationControl)->LoadEvidence {
        let mut loads=BTreeMap::<String,Vec<&index::LoadFact>>::new();for fact in facts{loads.entry(fact.path.clone()).or_default().push(fact);}
        let context=LoadContext::for_manifest(manifest,documents[manifest].as_bytes(),Some(source));
        let paths=crate::xml_load::LoadPaths::new(files.keys().chain(documents.keys()).map(String::as_str),control).unwrap();
        let inputs=LoadInputs{loads:&loads,files,documents,context:&context,paths:&paths};
        let mut result=LoadEvidence{manifest:manifest.into(),ordered:vec![],issues:vec![]};
        walk_load(manifest,&inputs,&mut Default::default(),&mut result,control).unwrap();result
    }
    fn facts(documents:&BTreeMap<String,String>)->Vec<index::LoadFact>{
        documents.iter().flat_map(|(path,raw)|{
            let parsed=if path.ends_with(".toc"){crate::facts::toc(raw.as_bytes())}else{crate::facts::xml(raw.as_bytes())};
            parsed.loads.into_iter().map(|load|index::LoadFact{path:path.clone(),line:load.line,kind:load.kind,target:load.path,source_blob:Value::Null}).collect::<Vec<_>>()
        }).collect()
    }
    #[test]
    fn alternative_manifests_keep_family_mode_repeats_and_nested_xml_in_one_scope(){
        let control=OperationControl::default();let source=json!({"repository":"wow-ui-source","product":"retail"});
        let documents=BTreeMap::from([
            ("Addon_Mainline.toc".into(),"## AllowLoadGameType: mainline\n[Family]\\Defs.lua\nShared.xml\n[Family]/Defs.lua\n[Game]/Mode.lua\nStandard.lua [AllowLoadGameType standard]\nSelected.lua [AllowLoadGameType classic, mainline]\nSpecial.lua [AllowLoadGameType plunderstorm wowhack]\nExcluded.lua [ExcludeLoadGameType mainline]\nUnknown.lua [AllowLoadGameType future]\n".into()),
            ("Addon_Classic.toc".into(),"## AllowLoadGameType: classic\n[Family]\\Defs.lua\nShared.xml\n[Family]/Defs.lua\nStandard.lua [AllowLoadGameType standard]\nSelected.lua [AllowLoadGameType classic mainline]\nSpecial.lua [AllowLoadGameType plunderstorm, wowhack]\n".into()),
            ("Shared.xml".into(),"<Ui><Include file=\"[Family]/Nested.xml\"/></Ui>".into()),
            ("Mainline/Nested.xml".into(),"<Ui><Script file=\"Nested.lua\"/></Ui>".into()),
            ("Classic/Nested.xml".into(),"<Ui><Script file=\"Nested.lua\"/></Ui>".into()),
        ]);
        let mut files=["Mainline/Defs.lua","Mainline/Nested.lua","Classic/Defs.lua","Classic/Nested.lua","Standard/Mode.lua","Standard.lua","Selected.lua","Special.lua","Excluded.lua","Unknown.lua"].into_iter().map(|path|(path.into(),"local marker = 1\n".into())).collect::<BTreeMap<_,_>>();
        let facts=facts(&documents);
        assert!(facts.iter().any(|fact|fact.target=="Special.lua [AllowLoadGameType plunderstorm wowhack]"),"raw observations must retain their conditions");
        let plans=["Addon_Mainline.toc","Addon_Classic.toc"].map(|manifest|plan(manifest,&source,&files,&documents,&facts,&control));
        assert_eq!(plans[0].ordered,vec!["Mainline/Defs.lua","Mainline/Nested.lua","Mainline/Defs.lua","Standard/Mode.lua","Standard.lua","Selected.lua"]);
        assert_eq!(plans[1].ordered,vec!["Classic/Defs.lua","Classic/Nested.lua","Classic/Defs.lua","Standard.lua","Selected.lua"]);
        assert!(plans[0].issues.iter().any(|issue|issue["code"]=="load_condition_unavailable"&&issue["target"]=="Unknown.lua [AllowLoadGameType future]"));
        assert!(plans[1].issues.is_empty());
        let xml=XmlResearch::prepare(&mut files,documents,&plans,&source,&control).unwrap();
        assert_eq!(xml.programs.len(),2);
        for(program,plan)in xml.programs.iter().zip(&plans){assert_eq!(program.lua_files(),plan.ordered);assert_eq!(program.context.mode,GameMode::Standard);}
        assert_eq!(xml.programs[0].context.family,Some(LoadFamily::Mainline));assert_eq!(xml.programs[1].context.family,Some(LoadFamily::Classic));
        assert_eq!(xml.programs[0].context.game.as_deref(),Some("Standard"));assert_eq!(xml.programs[1].context.game,None);
        assert!(xml.programs[0].gaps.iter().any(|gap|gap["code"]=="toc_load_condition_unavailable"));
        assert_ne!(storage::sha256(&serde_json::to_vec(xml.programs[0].as_ref()).unwrap()),storage::sha256(&serde_json::to_vec(xml.programs[1].as_ref()).unwrap()));
        let first=xml.identity(&control).unwrap();
        let mut altered=xml;let program=std::sync::Arc::make_mut(&mut altered.programs[0]);program.context.mode=GameMode::Wowhack;
        assert_ne!(altered.identity(&control).unwrap()["loadContextsSHA256"],first["loadContextsSHA256"]);
    }
    #[test]
    fn generic_pin_defaults_and_explicit_modes_keep_unknown_predicates_visible(){
        let control=OperationControl::default();
        let toc=b"## AllowLoadGameType: classic, standard\n[Family]/Defs.lua\nDeprecated_Specialization_[Game].lua [AllowLoadGameType mainline]\nStandard.lua [AllowLoadGameType standard]\nSpecial.lua [AllowLoadGameType plunderstorm]\n";
        let sources=[("Shared.toc",toc.as_slice()),("Mainline/Defs.lua",b"local mainline=true".as_slice()),("Classic/Defs.lua",b"local classic=true".as_slice()),("Deprecated_Specialization_Standard.lua",b"local standard=true".as_slice()),("Standard.lua",b"local standard=true".as_slice()),("Special.lua",b"local special=true".as_slice())];
        for(product,family,file)in [("retail",LoadFamily::Mainline,"Mainline/Defs.lua"),("classic",LoadFamily::Classic,"Classic/Defs.lua"),("titan",LoadFamily::Classic,"Classic/Defs.lua"),("forever",LoadFamily::Mainline,"Mainline/Defs.lua")]{
            let source=json!({"repository":"wow-ui-source","product":product});
            let program=Program::from_sources_for_pin("Shared.toc",sources,Some(&source),&control).unwrap();
            assert_eq!(program.context.family,Some(family));
            let expected=if family==LoadFamily::Mainline{assert_eq!(program.context.game.as_deref(),Some("Standard"));vec![file,"Deprecated_Specialization_Standard.lua","Standard.lua"]}else{assert_eq!(program.context.game,None);vec![file,"Standard.lua"]};
            assert_eq!(program.lua_files(),expected);
        }
        for(tag,mode)in [("plunderstorm",GameMode::Plunderstorm),("wowhack",GameMode::Wowhack)]{
            let special=format!("## AllowLoadGameType: {tag}\nSpecial.lua [AllowLoadGameType {tag}]\nStandard.lua [AllowLoadGameType standard]\n");
            let source=json!({"repository":"wow-ui-source","product":"retail"});
            let program=Program::from_sources_for_pin("Special.toc",[("Special.toc",special.as_bytes()),("Special.lua",b"".as_slice()),("Standard.lua",b"".as_slice())],Some(&source),&control).unwrap();
            assert_eq!(program.context.mode,mode);assert_eq!(program.context.game,None);assert_eq!(program.lua_files(),vec!["Special.lua"]);
        }
        let unknown=b"## AllowLoadEnvironment: future\nUnknown.lua\n";
        let program=Program::from_sources("Unknown.toc",[("Unknown.toc",unknown.as_slice()),("Unknown.lua",b"".as_slice())],&control).unwrap();
        assert!(program.lua_files().is_empty());assert!(program.gaps.iter().any(|gap|gap["code"]=="toc_manifest_condition_unavailable"));
        let conflict=b"## AllowLoadGameType: classic\nUnknown.lua\n";
        let program=Program::from_sources("Conflict_Mainline.toc",[("Conflict_Mainline.toc",conflict.as_slice()),("Unknown.lua",b"".as_slice())],&control).unwrap();
        assert!(program.lua_files().is_empty());assert!(program.gaps.iter().any(|gap|gap["code"]=="toc_manifest_condition_unavailable"));
        let subtype=b"[Game]/Subtype.lua\nUnknown.lua [AllowLoadGameType mists]\n";
        let source=json!({"repository":"wow-ui-source","product":"titan"});
        let program=Program::from_sources_for_pin("Shared.toc",[("Shared.toc",subtype.as_slice()),("Mists/Subtype.lua",b"".as_slice()),("Unknown.lua",b"".as_slice())],Some(&source),&control).unwrap();
        assert!(program.lua_files().is_empty());assert_eq!(program.gaps.len(),2);assert!(program.gaps.iter().all(|gap|gap["code"]=="toc_load_condition_unavailable"));
    }
    #[test]
    fn casefold_paths_resolve_unique_names_and_reject_ambiguous_exact_matches(){
        let control=OperationControl::default();let source=json!({"repository":"wow-ui-source","product":"retail"});
        let documents=BTreeMap::from([("Main.toc".into(),"dir/only.LUA\nCase.lua\ncase.lua\n".into())]);
        let mut files=["Dir/Only.lua","Case.lua","case.lua"].into_iter().map(|path|(path.into(),"local value=1\n".into())).collect::<BTreeMap<_,_>>();
        let facts=facts(&documents);let plan=plan("Main.toc",&source,&files,&documents,&facts,&control);
        assert_eq!(plan.ordered,vec!["Dir/Only.lua"]);assert_eq!(plan.issues.len(),2);
        let xml=XmlResearch::prepare(&mut files,documents,&[plan],&source,&control).unwrap();
        assert_eq!(xml.programs[0].lua_files(),vec!["Dir/Only.lua"]);
        assert_eq!(xml.programs[0].gaps.len(),2);assert!(xml.programs[0].gaps.iter().all(|gap|gap["code"]=="toc_load_target_unavailable"));
    }
    #[test]
    #[ignore="explicit fixed Retail home/pin; original FrameXMLUtil TOC/XML closure and CAS"]
    fn fixed_retail_family_loads_use_original_toc_and_complete_matching_program(){
        let home=PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("fixed source home"));
        let selected=lycheedev_core::selection::load_pinned(&home,&std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed Retail pin")).unwrap();
        let pin=selected.source.as_ref().unwrap();assert_eq!(pin.product,"retail");assert_eq!(pin.exact_commit,"31c7f7b9cc79e56c986b365c06a6afbcf3c9177b");
        let control=OperationControl::default();let prefix="Interface/AddOns/Blizzard_FrameXMLUtil/";let game_prefix="Interface/AddOns/Blizzard_DeprecatedSpecialization/";let manifest=format!("{prefix}Blizzard_FrameXMLUtil.toc");
        let tree=crate::git::tree_with_control(&home,pin,&control).unwrap().into_iter().filter(|file|(file.path.starts_with(prefix)||file.path.starts_with(game_prefix))&&[".lua",".xml",".toc"].iter().any(|extension|file.path.ends_with(extension))).collect::<Vec<_>>();
        let mut files=BTreeMap::new();let mut documents=BTreeMap::new();
        crate::git::visit_blobs_with_control(&home,pin,&tree,&control,|file,raw|{let raw=String::from_utf8(raw).unwrap();if file.path.ends_with(".lua"){files.insert(file.path.clone(),raw);}else{documents.insert(file.path.clone(),raw);}Ok(())}).unwrap();
        let mut index=index::build_with_control(&home,pin,&control).unwrap();index.documents.retain(|document|document.path.starts_with(prefix)||document.path.starts_with(game_prefix));
        let facts=index::load_facts_from_documents_with_control(&home,&index,&documents,&control).unwrap();
        let original=facts.iter().find(|fact|fact.path==manifest&&fact.line==15).unwrap();assert_eq!(original.target,"AuraUtil.lua");assert_eq!(original.source_blob["bytes"],1944);assert_eq!(original.source_blob["sha256"],"8ac7af18429a0a43b96b523709cd6b63d599ee40e479d5e88cc4bd03c240a3a5");
        assert_eq!(lycheedev_core::evidence::read_blob(&home,&original.source_blob,crate::git::MAX_SOURCE_BYTES).unwrap(),documents[&manifest].as_bytes());
        let source=serde_json::to_value(pin).unwrap();let plan=plan(&manifest,&source,&files,&documents,&facts,&control);assert!(plan.issues.is_empty(),"{:?}",plan.issues);
        for required in ["Mainline/QuestUtils.lua","Mainline/MapUtil.lua","Mainline/AuraUtil.lua","AchievementUtil.lua","Mainline/RAFUtil.lua"]{assert!(plan.ordered.contains(&format!("{prefix}{required}")),"missing {required}");}
        assert!(!plan.ordered.iter().any(|file|file.contains("/Classic/")||file.contains('[')));
        let game_manifest=format!("{game_prefix}Blizzard_DeprecatedSpecialization.toc");
        let game_plan=self::plan(&game_manifest,&source,&files,&documents,&facts,&control);
        assert!(game_plan.issues.is_empty(),"{:?}",game_plan.issues);assert_eq!(game_plan.ordered,vec![format!("{game_prefix}Deprecated_Specialization_Standard.lua"),format!("{game_prefix}Deprecated_TalentConsts.lua")]);
        let xml=XmlResearch::prepare(&mut files,documents,&[plan.clone(),game_plan.clone()],&source,&control).unwrap();let program=&xml.programs[0];
        let actual=program.lua_files().into_iter().filter(|file|!file.contains("/__inline_")).collect::<Vec<_>>();assert_eq!(actual,plan.ordered);
        assert!(!program.gaps.iter().any(|gap|gap["code"].as_str().is_some_and(|code|code.contains("load_"))),"{:?}",program.gaps);
        assert_eq!(xml.programs[1].lua_files(),game_plan.ordered);assert_eq!(xml.programs[1].context.game.as_deref(),Some("Standard"));
    }
}

#[cfg(test)]
mod reply_assembly_tests {
    use super::*;

    fn assert_move_and_bytes(maps:Vec<Value>) {
        let rows=vec![json!({"path":"ui.xml","line":3,"basis":["native_common_source_facts"],"observations":[{"selected":{"state":"known","value":"完整 value"}}]})];
        let scopes=vec![json!({"scope":{"manifest":"Main.toc","snapshot":"one","kind":"alternative_manifest_hypothetical_after_load"},"analysis":{"state":"partial"},"xmlFacts":{"objects":[{"origin":{"path":"ui.xml","bytes":{"start":0,"end":8}}}]}})];
        let gaps=vec![json!({"code":"native_ui_unknown","detail":{"z":2,"a":1}})];
        let entries=vec![json!({"parameters":[{"slot":1,"selected_value":{"state":"known_absent"}}]})];
        let analysis=json!({"state":"partial","graphControl":{"nodes":[3,2,1]},"sourceFiles":maps.len()+2});
        let load_evidence=vec![LoadEvidence{manifest:"Main.toc".into(),ordered:vec!["a.lua".into(),"a.lua".into()],issues:vec![]}];
        let resolution=EntryResolution::Absent;
        // Exact former eager assembly, including full nested fields and array order.
        let expected=json!({"schema":"lycheedev.source-native-research.v1","relations":rows,
            "inputIdentity":"fixed-source-identity","loadEvidence":load_evidence,"loadScopes":scopes,
            "entryResolution":resolution,"sourceMaps":maps,"gaps":gaps,
            "entryEvidence":{"kind":"hypothetical_selected_declaration_entry","phase":"after_load","sourceExecution":false,"xmlSelf":"exact_registered_instance","entries":entries},"analysis":analysis});
        let expected_bytes=serde_json::to_vec(&expected).unwrap();drop(expected);
        let pointers=[rows.as_ptr(),scopes.as_ptr(),maps.as_ptr(),gaps.as_ptr(),entries.as_ptr()];
        let nested_pointer=maps.first().map(|map|map["xmlPath"].as_str().unwrap().as_ptr());
        let header=json!({"schema":"lycheedev.source-native-research.v1","inputIdentity":"fixed-source-identity","loadEvidence":load_evidence,"entryResolution":resolution,
            "entryEvidence":{"kind":"hypothetical_selected_declaration_entry","phase":"after_load","sourceExecution":false,"xmlSelf":"exact_registered_instance"}});
        let actual=assemble_reply(header,rows,scopes,maps,gaps,entries,analysis);
        for (index,key) in ["relations","loadScopes","sourceMaps","gaps"].into_iter().enumerate(){assert_eq!(actual[key].as_array().unwrap().as_ptr(),pointers[index]);}
        assert_eq!(actual["entryEvidence"]["entries"].as_array().unwrap().as_ptr(),pointers[4]);
        if let Some(pointer)=nested_pointer{assert_eq!(actual["sourceMaps"][0]["xmlPath"].as_str().unwrap().as_ptr(),pointer);}
        assert_eq!(serde_json::to_vec(&actual).unwrap(),expected_bytes);
    }

    #[test]
    fn reply_moves_complete_xml_maps_and_matches_eager_bytes() {
        let scripts=crate::xml_inline::extract("ui.xml","\u{feff}<Ui>\r\n<Frame name=\"A\"><Scripts><OnLoad>\r\nlocal text = &quot;你好&quot;\r\n</OnLoad></Scripts></Frame><Script>local n=1</Script></Ui>".as_bytes()).unwrap();
        assert_eq!(scripts.len(),2);
        let maps=scripts.iter().map(crate::xml_inline::Script::mapping_manifest).collect::<Vec<_>>();
        assert!(maps.iter().any(|map|map["segments"].as_array().unwrap().iter().any(|segment|segment["origin"]=="Entity")));
        assert_move_and_bytes(maps);assert_move_and_bytes(vec![]);
        let mut result=json!({"relations":[{"path":"a.lua","observations":[{"state":"unknown"}]}],"analysis":{"state":"partial"}});
        let pointer=result["relations"].as_array().unwrap().as_ptr();
        let before=serde_json::to_vec(&result["relations"]).unwrap();
        let rows=take_relation_rows(&mut result);
        assert_eq!(rows.as_ptr(),pointer);assert_eq!(serde_json::to_vec(&rows).unwrap(),before);assert!(result["relations"].is_null());
    }

    #[test]
    #[ignore="explicit existing fixed Retail material; extract and measure complete maps without preparing Emmy or launching a worker"]
    fn full_retail_reply_keeps_all_maps_with_identical_bytes() {
        #[derive(Deserialize)]
        struct Material { snapshot:MappingInputs }
        #[derive(Deserialize)]
        struct MappingInputs { load_documents:BTreeMap<String,String>, source:Value }
        let path=std::env::var_os("LYCHEEDEV_SOURCE_REPLY_MATERIAL").expect("existing full Retail material path");
        let raw=std::fs::read(path).unwrap();let material_sha256=storage::sha256(&raw);
        let material:Material=serde_json::from_slice(&raw).unwrap();drop(raw);
        assert_eq!(material.snapshot.source["product"],"retail");
        assert_eq!(material.snapshot.source["exactCommit"],"31c7f7b9cc79e56c986b365c06a6afbcf3c9177b");
        let mut maps=BTreeMap::new();
        for(path,text)in &material.snapshot.load_documents {
            if !path.to_ascii_lowercase().ends_with(".xml"){continue;}
            for script in crate::xml_inline::extract(path,text.as_bytes()).unwrap(){maps.insert(script.key.clone(),script.mapping_manifest());}
        }
        let maps=maps.into_values().collect::<Vec<_>>();assert_eq!(maps.len(),4018);
        let segment_count:usize=maps.iter().map(|map|map["segments"].as_array().unwrap().len()).sum();
        let bytes=serde_json::to_vec(&maps).unwrap();
        let mut nodes=0usize;let mut objects=0usize;let mut arrays=0usize;let mut string_and_key_bytes=0usize;
        let mut pending=maps.iter().collect::<Vec<_>>();
        while let Some(value)=pending.pop(){nodes+=1;match value{
            Value::Object(fields)=>{objects+=1;string_and_key_bytes+=fields.keys().map(String::len).sum::<usize>();pending.extend(fields.values());},
            Value::Array(values)=>{arrays+=1;pending.extend(values);},Value::String(text)=>string_and_key_bytes+=text.len(),_=>{}
        }}
        println!("{}",json!({"materialSHA256":material_sha256,"source":material.snapshot.source,"sourceMaps":maps.len(),"compilerSegments":segment_count,"compactJSONBytes":bytes.len(),"compactJSONSHA256":storage::sha256(&bytes),"jsonNodes":nodes,"jsonObjects":objects,"jsonArrays":arrays,"stringAndKeyBytes":string_and_key_bytes,"scope":"complete current production mapping manifests; structure and encoded bytes, not Rust heap or OOM proof"}));
        drop(bytes);assert_move_and_bytes(maps);
    }
}
