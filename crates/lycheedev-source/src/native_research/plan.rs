//! Logical semantic work, before XML occurrence expansion. These are plans,
//! not completed batches; session retirement must count execution separately.
#![forbid(unsafe_code)]
use crate::{environment, index, native::{Site, digest}};
use lycheedev_core::{Result, ToolkitError};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub(crate) const BATCH_QUERIES: usize = 128;
pub(crate) const OUTGOING_QUERIES: usize = 128;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AuthenticatedSite {
    pub(crate) site: Site,
    pub(crate) file_sha256: String,
    pub(crate) position: Value,
    pub(crate) material: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "target", rename_all = "snake_case")]
pub(crate) enum LogicalOperation {
    References(AuthenticatedSite),
    Type(AuthenticatedSite),
    Definition(AuthenticatedSite),
}

#[derive(Debug, Serialize)]
pub(crate) struct ResearchPlan {
    schema: &'static str,
    source_identity: String,
    query_identity: String,
    operations: Vec<LogicalOperation>,
    batch_sizes: Vec<usize>,
    unavailable: Vec<&'static str>,
    outgoing_truncated: bool,
    scanner_diagnostics: Vec<Value>,
    scanner_diagnostics_omitted: usize,
    #[serde(skip)]
    owner: Arc<()>,
}
impl ResearchPlan {
    pub(crate) fn new(source_identity: &str, query: &impl Serialize, owner: Arc<()>) -> Self {
        Self {
            schema: "lycheedev.native-semantic-plan.v1",
            source_identity: source_identity.into(),
            query_identity: digest(query),
            operations: Vec::new(), batch_sizes: Vec::new(), unavailable: Vec::new(),
            outgoing_truncated: false, scanner_diagnostics:Vec::new(), scanner_diagnostics_omitted:0, owner,
        }
    }
    pub(crate) fn incoming(&mut self, site: AuthenticatedSite) {
        self.operations.push(LogicalOperation::References(site.clone()));
        self.operations.push(LogicalOperation::Type(site));
    }
    pub(crate) fn definition_full(&self) -> bool {
        self.operations.iter().filter(|op| matches!(op, LogicalOperation::Definition(_))).count() >= OUTGOING_QUERIES
    }
    pub(crate) fn truncate_outgoing(&mut self) {
        self.outgoing_truncated = true;
        self.unavailable("outgoing_operation_limit");
    }
    pub(crate) fn definition(&mut self, site: AuthenticatedSite) {
        if self.definition_full() { self.truncate_outgoing(); }
        else { self.operations.push(LogicalOperation::Definition(site)); }
    }
    pub(crate) fn unavailable(&mut self, reason: &'static str) {
        if !self.unavailable.contains(&reason) { self.unavailable.push(reason); }
    }
    pub(crate) fn scan_facts(&mut self,text:&str,path:&str,control:&lycheedev_core::control::OperationControl)->Result<crate::facts::Facts> {
        control.check()?;
        let facts=crate::facts::lua(text.as_bytes(),false);
        // Deadline/cancellation wins over a local scanner diagnostic.
        control.check()?;
        if !facts.diagnostics.is_empty(){self.unavailable("structural_scanner_incomplete");}
        for note in &facts.diagnostics {
            control.check()?;
            if self.scanner_diagnostics.len()<32 {
                self.scanner_diagnostics.push(serde_json::json!({"path":path,"line":note.line,"message":note.message}));
            } else {self.scanner_diagnostics_omitted=self.scanner_diagnostics_omitted.saturating_add(1);}
        }
        Ok(facts)
    }
    pub(crate) fn finish(mut self) -> Self {
        self.batch_sizes = self.operations.chunks(BATCH_QUERIES).map(<[_]>::len).collect();
        self
    }
    pub(crate) fn validate(&self, identity: &str, query: &impl Serialize, owner: &Arc<()>) -> Result<()> {
        if self.source_identity != identity || self.query_identity != digest(query) || !Arc::ptr_eq(&self.owner, owner) {
            return Err(ToolkitError::new("source.native_plan_identity", "semantic plan belongs to another source, query or request", 4));
        }
        Ok(())
    }
    pub(crate) fn execution_identity(&self)->(String,Arc<()>) {(digest(self),self.owner.clone())}
    pub(crate) fn validate_execution_identity(&self,identity:&str,owner:&Arc<()>)->Result<()> {
        if identity!=digest(self)||!Arc::ptr_eq(owner,&self.owner){return Err(ToolkitError::new("source.native_plan_identity","completion belongs to another semantic plan or request",4));}Ok(())
    }
    pub(crate) fn operations(&self)->&[LogicalOperation] { &self.operations }
    pub(crate) fn batch_count(&self)->usize { self.batch_sizes.len() }
    pub(crate) fn incomplete(&self)->bool { !self.unavailable.is_empty() }
    pub(crate) fn evidence(&self) -> Value {
        // Do not present a planned operation count as executed semantic work.
        serde_json::json!({"state":"planned_not_executed", "plan":self})
    }
}

/// Completion is issued only after every planned operation has an answer for
/// every successfully selected scope, with no failed/unexecuted scope hidden.
#[derive(Debug,Serialize)]
pub(crate) struct ExecutionReceipt {
    pub(super) planned_batches:usize,
    pub(super) completed_batches:usize,
    pub(super) scopes:usize,
    pub(super) unavailable_scopes:usize,
    pub(super) uncertain_answers:usize,
    pub(super) unexecuted_operations:usize,
    pub(super) plan_identity:String,
}
pub(crate) struct ResearchExecution<'a> {
    plan:&'a ResearchPlan, completed:Vec<bool>, unexecuted_operations:usize,
    answers:Vec<Value>, scopes:usize, unavailable_scopes:usize, uncertain_answers:usize, evidence_bytes:usize,
}
impl<'a> ResearchExecution<'a> {
    pub(crate) fn new(plan:&'a ResearchPlan)->Self {Self{plan,completed:vec![true;plan.batch_count()],unexecuted_operations:0,answers:Vec::new(),scopes:0,unavailable_scopes:0,uncertain_answers:0,evidence_bytes:0}}
    pub(crate) fn scope(&mut self,scope:Value,completion:crate::native::ResearchScopeCompletion)->Result<()> {
        let (answers,performed)=completion.consume(self.plan)?;
        // Match the existing worker output ceiling without allocating a second
        // serialization of all scope answers while their JSON trees are live.
        struct Counter(usize);
        impl std::io::Write for Counter {
            fn write(&mut self,bytes:&[u8])->std::io::Result<usize>{
                self.0=self.0.checked_add(bytes.len()).filter(|size|*size<=32*1024*1024)
                    .ok_or_else(||std::io::Error::other("semantic execution evidence exceeds output budget"))?;
                Ok(bytes.len())
            }
            fn flush(&mut self)->std::io::Result<()>{Ok(())}
        }
        let mut bytes=Counter(self.evidence_bytes);
        serde_json::to_writer(&mut bytes,&scope).and_then(|_|serde_json::to_writer(&mut bytes,&answers))
            .map_err(|_|ToolkitError::new("source.native_output_budget","semantic execution evidence exceeds output budget",3))?;
        self.evidence_bytes=bytes.0;
        for (index,batch) in performed.chunks(BATCH_QUERIES).enumerate(){self.completed[index]&=batch.iter().all(|done|*done);}
        self.unexecuted_operations+=performed.iter().filter(|done|!**done).count();
        self.uncertain_answers+=answers.iter().filter(|answer|answer["state"]!="known"&&answer["state"]!="known_declaration").count();
        self.scopes+=1;
        self.answers.push(serde_json::json!({"scope":scope,"operations":answers}));
        Ok(())
    }
    pub(crate) fn unavailable_scope(&mut self,reason:Value) {
        self.unavailable_scopes+=1;
        self.answers.push(serde_json::json!({"scope":reason,"state":"unavailable","operationsExecuted":false}));
    }
    pub(crate) fn finish(self)->(Value,ExecutionReceipt) {
        let completed=if self.scopes>0&&self.unavailable_scopes==0 {self.completed.iter().filter(|done|**done).count()} else {0};
        let receipt=ExecutionReceipt{planned_batches:self.plan.batch_count(),completed_batches:completed,scopes:self.scopes,unavailable_scopes:self.unavailable_scopes,uncertain_answers:self.uncertain_answers,unexecuted_operations:self.unexecuted_operations,plan_identity:digest(self.plan)};
        (serde_json::json!({"schema":"lycheedev.native-semantic-execution.v1","receipt":receipt,"answers":self.answers}),receipt)
    }
}

/// Rebuild the existing environment emitter from the exact admitted originals.
/// A caller's mapping list or generated coordinate is never a trust root.
pub(crate) struct VerifiedApiDefinitions {
    definitions: String,
    mappings: Vec<environment::Mapping>,
    manifest: environment::Manifest,
    analysis:emmylua_code_analysis::EmmyLuaAnalysis,
    file_id:emmylua_code_analysis::FileId,
}
impl VerifiedApiDefinitions {
    pub(crate) fn rebuild(files: &std::collections::BTreeMap<String,String>, api: &[environment::ApiRecord], manifest: Option<&environment::Manifest>, control: &lycheedev_core::control::OperationControl) -> Result<Option<Self>> {
        let Some(manifest)=manifest else {
            if !files.is_empty() {return Err(invalid_environment("environment originals have no manifest"));}
            return Ok(None)
        };
        let limits=environment::Limits::default();
        let mut documents=std::collections::BTreeMap::new();let mut total=0usize;
        for (path,text) in files {
            control.check()?;
            let lower=path.to_ascii_lowercase();
            if !crate::git::source_path(path) || !lower.contains("/blizzard_apidocumentationgenerated/") || !lower.ends_with(".lua") {
                return Err(invalid_environment("environment original path is outside the API document set"));
            }
            total=total.checked_add(text.len()).ok_or_else(|| invalid_environment("API input byte overflow"))?;
            if documents.len()>=limits.documents || text.len()>limits.file_bytes || total>limits.total_bytes {
                return Err(invalid_environment("API reconstruction exceeds environment limits"));
            }
            documents.insert(path.clone(),text.as_bytes().to_vec());
        }
        let rebuilt=environment::build(manifest.identity.clone(),&documents,limits,control)?;
        if serde_json::to_value(&rebuilt.manifest)?!=serde_json::to_value(manifest)?
            || serde_json::to_value(&rebuilt.facts)?!=serde_json::to_value(api)? {
            return Err(invalid_environment("API manifest or facts differ from admitted original source"));
        }
        let mut analysis=emmylua_code_analysis::EmmyLuaAnalysis::new();
        let mut config=emmylua_code_analysis::Emmyrc::default();config.runtime.version=emmylua_code_analysis::EmmyrcLuaVersion::Lua51;
        analysis.update_config(Arc::new(config));
        let root=std::path::PathBuf::from("C:/lycheedev/authenticated-environment-types");analysis.add_main_workspace(root.clone());
        let ids=analysis.update_files_by_path_shared_checked(vec![(root.join("definitions.lua"),Some(Arc::<str>::from(rebuilt.definitions.as_str())))],||control.check())?;
        let file_id=*ids.first().ok_or_else(||invalid_environment("API semantic input missing"))?;
        if ids.len()!=1||analysis.db.get_file_text(file_id)!=Some(rebuilt.definitions.as_str())
            ||!analysis.semantic_model(file_id).parse_errors().unwrap_or_default().is_empty(){return Err(invalid_environment("API semantic input differs or fails parsing"));}
        Ok(Some(Self {definitions:rebuilt.definitions,mappings:rebuilt.mappings,manifest:rebuilt.manifest,analysis,file_id}))
    }
    /// Query the authenticated environment declaration in a separate Emmy model.
    /// This is declaration semantics, never a claim about mutable source globals.
    pub(crate) fn semantic_type(&self,target:&AuthenticatedSite,bindings:&[crate::native::HostBinding],control:&lycheedev_core::control::OperationControl,mut step:impl FnMut()->Result<()>)->Result<Value> {
        use emmylua_code_analysis::binding_context::{BindingContext,BindingDecision,BindingSite,BoundValue,SelectedValue,BindingSession,EnvironmentCallable};
        use emmylua_parser::{LuaAstNode,LuaExpr};
        control.check()?;
        if target.file_sha256!=self.manifest.definitions_sha256||target.material["manifest"]!=serde_json::to_value(&self.manifest)? {return Err(invalid_environment("API Type target belongs to another environment"));}
        let mapping:environment::Mapping=serde_json::from_value(target.material["mapping"].clone())?;
        let mut matches=0usize;
        for admitted in &self.mappings {step()?;if serde_json::to_value(admitted)?==target.material["mapping"]{matches+=1;}}
        let start=crate::xml_inline::source_map::utf16_offset(&self.definitions,mapping.generated_line,mapping.generated_utf16_column).map_err(|_|invalid_environment("API Type coordinate outside verified definitions"))?;
        let name=mapping.callable.rsplit(['.',':']).next().unwrap_or("");
        let end=start.checked_add(name.len()).ok_or_else(||invalid_environment("API Type coordinate overflow"))?;
        let position=crate::xml_inline::source_map::utf16_range(&self.definitions,crate::xml_inline::source_map::Span::new(start,end)).map_err(|_|invalid_environment("API Type range outside verified definitions"))?;
        if matches!=1||target.site.file!="@native/api-definitions.lua"||target.site.start as usize!=start||target.site.end as usize!=end
            ||self.definitions.get(start..end)!=Some(name)||serde_json::to_value(position)?!=target.position{return Err(invalid_environment("API Type mapping or site differs from authenticated definition"));}
        let mut candidates=bindings.iter().filter(|binding|binding.name()==mapping.callable);
        let binding=candidates.next();
        let descriptor=if candidates.next().is_none(){binding.and_then(|binding|binding.native_descriptor())}else{None};
        let Some(descriptor)=descriptor else{return Ok(serde_json::json!({"kind":"type","state":"unavailable","executed":false,"reason":"authenticated_native_api_binding_unavailable"}));};
        let binding=binding.expect("descriptor binding");
        if binding.descriptor().is_none_or(|record|record["path"]!=mapping.source_path||record["line"]!=mapping.source_line){return Err(invalid_environment("API Type host origin differs from unique mapping"));}
        let model=self.analysis.semantic_model(self.file_id);
        let root=model.syntax_tree().ok_or_else(||invalid_environment("API semantic tree missing"))?.get_red_root();
        let mut selected=None;
        for node in root.descendants(){control.check()?;step()?;
            if let Some(expr)=LuaExpr::cast(node) {
                let range=expr.get_range();let start:u32=range.start().into();let end:u32=range.end().into();
                if start<=target.site.start&&end>=target.site.end&&selected.as_ref().is_none_or(|(length,_)|end-start<*length){selected=Some((end-start,expr.get_syntax_id()));}
            }
        }
        let Some((_,syntax))=selected else{return Ok(serde_json::json!({"kind":"type","state":"unavailable","executed":false,"reason":"authenticated_api_syntax_target_unavailable"}));};
        let site=BindingSite{file_id:self.file_id,syntax};
        struct DeclarationContext { identity:String,site:BindingSite,descriptor:Arc<EnvironmentCallable>,control:lycheedev_core::control::OperationControl }
        impl BindingContext for DeclarationContext {
            fn identity(&self)->&str{&self.identity}
            fn resolve(&self,site:BindingSite)->BindingDecision<BoundValue>{
                if let Err(error)=self.control.check(){return BindingDecision::Unavailable{proof:self.identity.clone(),reason:error.code};}
                if site!=self.site{return BindingDecision::Unavailable{proof:self.identity.clone(),reason:"outside_authenticated_api_type_target".into()};}
                BindingDecision::Bound(BoundValue{identity:self.identity.clone(),proof:self.identity.clone(),value:SelectedValue::EnvironmentCallable(self.descriptor.clone())})
            }
        }
        let identity=digest(&(target,binding.identity(),"authenticated-api-type-v1"));
        let session=BindingSession::new(Arc::new(DeclarationContext{identity:identity.clone(),site,descriptor:descriptor.clone(),control:control.clone()}));
        let model=self.analysis.semantic_model_with_context(self.file_id,session.clone());
        step()?;let inferred=model.type_of_expr(syntax);control.check()?;
        let coverage=session.coverage();
        let known=matches!(&inferred,emmylua_code_analysis::LuaType::DocFunction(_))&&coverage.iter().any(|row|row.site==site&&row.state=="bound");
        let coverage=coverage.iter().map(|row|serde_json::json!({"operation":row.operation,"state":row.state,"proof":row.proof,"reason":row.reason})).collect::<Vec<_>>();
        Ok(serde_json::json!({"kind":"type","state":if known{"known"}else{"unavailable"},"executed":true,
            "result":{"type":emmylua_code_analysis::humanize_semantic_type_detailed(&model,&inferred),"coverage":coverage,"binding":binding,"queryIdentity":identity,"target":target,"bindingSelection":"authenticated_environment_declaration","sourceExecution":false}}))
    }
    pub(crate) fn target(&self,symbol:&index::Symbol)->Result<Option<AuthenticatedSite>> {
        if symbol.category!="api-function" {return Ok(None);}
        let mut mappings=self.mappings.iter().filter(|mapping|mapping.kind=="function"&&mapping.callable==symbol.name
            &&mapping.source_path==symbol.path&&mapping.source_line>=symbol.line&&mapping.source_line<=symbol.end_line.max(symbol.line));
        let Some(mapping)=mappings.next() else {return Ok(None)};
        if mappings.next().is_some(){return Ok(None);}
        let start=crate::xml_inline::source_map::utf16_offset(&self.definitions,mapping.generated_line,mapping.generated_utf16_column)
            .map_err(|_|invalid_environment("generated API mapping is outside verified definitions"))?;
        let name=symbol.name.rsplit(['.', ':']).next().unwrap_or("");
        let end=start.checked_add(name.len()).ok_or_else(||invalid_environment("generated API range overflow"))?;
        if name.is_empty() || self.definitions.get(start..end)!=Some(name) {
            return Err(invalid_environment("generated API mapping does not identify its callable"));
        }
        let range=start..end;
        let position=crate::xml_inline::source_map::utf16_range(&self.definitions,crate::xml_inline::source_map::Span::new(range.start,range.end))
            .map_err(|_|invalid_environment("generated API coordinate is outside verified definitions"))?;
        let serialized=serde_json::to_value(&position)?;
        if serialized["start"]["line"].as_u64()!=Some(mapping.generated_line as u64)
            ||serialized["start"]["character"].as_u64()!=Some(mapping.generated_utf16_column as u64) {
            return Err(invalid_environment("generated API mapping disagrees with original definitions"));
        }
        Ok(Some(AuthenticatedSite {
            site:Site{file:"@native/api-definitions.lua".into(),start:range.start as u32,end:range.end as u32},
            file_sha256:self.manifest.definitions_sha256.clone(),position:serialized,
            material:serde_json::json!({"kind":"api_definition","mapping":mapping,"manifest":self.manifest}),
        }))
    }
}
fn invalid_environment(message:&str)->ToolkitError {ToolkitError::new("source.native_plan_environment",message,4)}

/// Go's query plan requires a unique spelling on the selected original line.
/// Return original UTF-8 bytes; UTF-16/XML coordinates are independently mapped
/// by the admitted Source snapshot, never accepted from a wire caller.
pub(crate) fn unique_name_range(text: &str, line: usize, name: &str) -> Option<std::ops::Range<usize>> {
    let name = name.rsplit(['.', ':']).next()?;
    if name.is_empty() || !name.bytes().enumerate().all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || i > 0 && c.is_ascii_digit()) { return None; }
    let mut offset = 0;
    for (i, part) in text.split('\n').enumerate() {
        if i + 1 == line {
            let start = part.find(name)?;
            if part[start + name.len()..].contains(name) { return None; }
            return Some(offset + start..offset + start + name.len());
        }
        offset += part.len() + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn site(n: u32) -> AuthenticatedSite {
        AuthenticatedSite { site: Site { file:"probe.lua".into(), start:n, end:n+1 }, file_sha256:"a".repeat(64), position:serde_json::json!({"bytes":{"start":n,"end":n+1}}), material:serde_json::json!({"kind":"source"}) }
    }
    #[test]
    fn references_type_and_128_definitions_split_128_then_2() {
        let owner=Arc::new(());
        let mut plan=ResearchPlan::new("source", &"both", owner.clone());
        plan.incoming(site(0));
        for n in 1..=128 { plan.definition(site(n)); }
        let plan=plan.finish();
        assert_eq!(plan.batch_sizes, [128,2]);
        assert_eq!(plan.operations.len(),130);
        assert!(matches!(plan.operations[0], LogicalOperation::References(_)));
        assert!(matches!(plan.operations[1], LogicalOperation::Type(_)));
        plan.validate("source", &"both", &owner).unwrap();
    }
    #[test]
    fn execution_receipt_without_a_native_completion_cannot_credit_work() {
        let mut plan=ResearchPlan::new("source",&"both",Arc::new(()));
        plan.incoming(site(0));for n in 1..=128{plan.definition(site(n));}let plan=plan.finish();
        let (_,receipt)=ResearchExecution::new(&plan).finish();assert_eq!(receipt.completed_batches,0);assert_eq!(receipt.planned_batches,2);
        let mut execution=ResearchExecution::new(&plan);execution.unavailable_scope(serde_json::json!({"reason":"graph_budget"}));
        let (evidence,receipt)=execution.finish();assert_eq!(receipt.completed_batches,0);assert_eq!(receipt.unavailable_scopes,1);assert_eq!(evidence["answers"][0]["operationsExecuted"],false);
    }
    #[test]
    fn zero_work_and_outgoing_limit_are_explicit() {
        assert!(ResearchPlan::new("source", &"none", Arc::new(())).finish().batch_sizes.is_empty());
        let mut plan=ResearchPlan::new("source", &"outgoing", Arc::new(()));
        for n in 0..130 { plan.definition(site(n)); }
        let plan=plan.finish();
        assert_eq!(plan.batch_sizes,[128]);
        assert!(plan.outgoing_truncated);
        assert_eq!(plan.unavailable,["outgoing_operation_limit"]);
    }
    #[test]
    fn plan_is_bound_to_exact_source_query_and_request_owner() {
        let owner=Arc::new(());
        let plan=ResearchPlan::new("source", &"query", owner.clone()).finish();
        assert!(plan.validate("changed", &"query", &owner).is_err());
        assert!(plan.validate("source", &"changed", &owner).is_err());
        assert!(plan.validate("source", &"query", &Arc::new(())).is_err());
    }
    #[test]
    fn api_definition_mapping_requires_rebuilt_source_and_unique_match() {
        let path="Interface/AddOns/Blizzard_APIDocumentationGenerated/TimerDocumentation.lua";
        let text="local API={Name=\"Test\",Type=\"System\",Namespace=\"C_Test\",Functions={{Name=\"Test\",Type=\"Function\"}}}\nAPIDocumentation:AddDocumentationTable(API)\n";
        let control=lycheedev_core::control::OperationControl::default();
        let api=environment::build(environment::Identity{repository:"wow-ui-source".into(),commit:"a".repeat(40),client:"retail".into(),dependency_commits:Default::default(),generator_version:environment::GENERATOR.into()},
            &std::collections::BTreeMap::from([(path.into(),text.as_bytes().to_vec())]),environment::Limits::default(),&control).unwrap();
        let files=std::collections::BTreeMap::from([(path.into(),text.into())]);
        let mut verified=VerifiedApiDefinitions::rebuild(&files,&api.facts,Some(&api.manifest),&control).unwrap().unwrap();
        let symbol=index::Symbol{id:String::new(),kind:"declaration".into(),name:"C_Test.Test".into(),target:String::new(),category:"api-function".into(),confidence:String::new(),path:path.into(),line:1,end_line:1,signature:String::new(),scope:String::new(),ordinal:0};
        assert!(verified.target(&symbol).unwrap().is_some());
        verified.mappings.push(verified.mappings[0].clone());
        assert!(verified.target(&symbol).unwrap().is_none());
        let changed=std::collections::BTreeMap::from([(path.into(),text.replace("Test","Later"))]);
        assert!(VerifiedApiDefinitions::rebuild(&changed,&api.facts,Some(&api.manifest),&control).is_err());
        let mut false_manifest=api.manifest.clone();false_manifest.definitions_sha256="b".repeat(64);
        assert!(VerifiedApiDefinitions::rebuild(&files,&api.facts,Some(&false_manifest),&control).is_err());
    }

    #[test]
    fn api_type_query_uses_managed_inference_and_rejects_foreign_targets() {
        for type_name in ["number","UnsupportedNominal"] {
            let path="Interface/AddOns/Blizzard_APIDocumentationGenerated/TestDocumentation.lua";
            let text=format!("local API={{Name=\"Test\",Type=\"System\",Namespace=\"C_Test\",Functions={{{{Name=\"Test\",Type=\"Function\",Arguments={{{{Name=\"value\",Type=\"{type_name}\"}}}}}}}}}}\nAPIDocumentation:AddDocumentationTable(API)\n");
            let control=lycheedev_core::control::OperationControl::default();
            let api=environment::build(environment::Identity{repository:"wow-ui-source".into(),commit:"a".repeat(40),client:"retail".into(),dependency_commits:Default::default(),generator_version:environment::GENERATOR.into()},&std::collections::BTreeMap::from([(path.into(),text.as_bytes().to_vec())]),environment::Limits::default(),&control).unwrap();
            let files=std::collections::BTreeMap::from([(path.into(),text)]);
            let verified=VerifiedApiDefinitions::rebuild(&files,&api.facts,Some(&api.manifest),&control).unwrap().unwrap();
            let record=api.facts.iter().find(|item|item.name=="C_Test.Test").unwrap();
            let symbol=index::Symbol{id:String::new(),kind:"declaration".into(),name:record.name.clone(),target:String::new(),category:"api-function".into(),confidence:String::new(),path:path.into(),line:record.line,end_line:record.end_line,signature:String::new(),scope:String::new(),ordinal:0};
            let target=verified.target(&symbol).unwrap().unwrap();
            let host=crate::native_research::api_environment(&api.facts,Some(&api.manifest),&control).unwrap();
            let answer=verified.semantic_type(&target,host.bindings(),&control,||Ok(())).unwrap();
            assert_eq!(answer["executed"],true);assert_eq!(answer["state"],if type_name=="number"{"known"}else{"unavailable"},"{answer}");
            assert!(!answer["result"]["coverage"].as_array().unwrap().is_empty());
            let missing=verified.semantic_type(&target,&[],&control,||Ok(())).unwrap();assert_eq!(missing["executed"],false);
            let mut foreign=target.clone();foreign.site.start+=1;assert!(verified.semantic_type(&foreign,host.bindings(),&control,||Ok(())).is_err());
            control.cancel_handle().store(true,std::sync::atomic::Ordering::Release);assert_eq!(verified.semantic_type(&target,host.bindings(),&control,||Ok(())).unwrap_err().code,"command.cancelled");
        }
    }

    #[test]
    fn scanner_depth_budget_is_visible_and_cancellation_has_priority() {
        let text=format!("{}Target(){}", "do ".repeat(300), " end".repeat(300));
        let control=lycheedev_core::control::OperationControl::default();
        let mut plan=ResearchPlan::new("source",&"query",Arc::new(()));
        let facts=plan.scan_facts(&text,"deep.lua",&control).unwrap();
        assert!(facts.diagnostics.iter().any(|note|note.message=="source_ast_depth_limit"));
        assert!(plan.unavailable.contains(&"structural_scanner_incomplete"));
        assert!(!plan.scanner_diagnostics.is_empty());
        control.cancel_handle().store(true,std::sync::atomic::Ordering::Release);
        assert_eq!(plan.scan_facts(&text,"deep.lua",&control).err().unwrap().code,"command.cancelled");
    }

    #[test]
    fn original_byte_coordinates_preserve_bom_unicode_and_ambiguity() {
        let text="\u{feff}-- 😀\r\nfunction Obj:Target()\r\n Target(); Target()\r\nend";
        let range=unique_name_range(text,2,"Obj:Target").unwrap();
        assert_eq!(&text[range],"Target");
        assert!(unique_name_range(text,3,"Target").is_none());
        assert!(unique_name_range(text,0,"Target").is_none());
        assert!(unique_name_range(text,2,"x['Target']").is_none());
    }
}
