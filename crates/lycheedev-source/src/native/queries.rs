const QUERY_RETURN_RESERVE:usize=1024;
fn query_policy()->Value {json!({"schema":"source-query-facade-v1","axes":"lexical-and-selected-independent","roles":["read","write","capture","call","declaration"],"pageRows":200,"defaultPageRows":50,"contextDepth":4,"contextNodes":512,"snippetBytes":65536,"syntaxRows":12000,"typedStorages":8192,"contextPendingNodes":4096,"sharedOperationSteps":10000,"returnStepReserve":QUERY_RETURN_RESERVE,"cursor":"in-process-owner-target-filter-selector-options-bound","calls":"syntax-or-explicit-input-abstract-invocation-not-game-runtime"})}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum QuerySelector { SyntaxOnly, SourceTemplate{function:usize}, LoadOccurrence{occurrence:usize}, XmlOccurrence{action:usize}, Invocation{frame:usize}, Entry{frame:usize}, Occurrence{occurrence:usize}, AllOccurrences }
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag="kind",content="identity",rename_all="snake_case")]
pub enum QueryTarget { All, Site(Site), Declaration(String), Storage(String), Value(String) }
#[derive(Clone, Debug, Default, Serialize)]
pub struct ReferenceFilter { pub roles:Vec<ReferenceRole> }
#[derive(Clone, Debug, Serialize)]
#[serde(tag="state",rename_all="snake_case")]
pub enum QueryFact<T> { Known{value:T}, KnownAbsent{reason:String}, Unavailable{reason:String}, NotApplicable, SelectorRequired{candidates:Vec<QuerySelector>} }
#[derive(Clone, Debug, Serialize)]
pub struct ValueDefinition {
    pub value_id:String, pub origin:ValueOrigin, pub function_declaration:Option<Site>,
    pub function:Option<usize>, pub object:Option<usize>, pub closure:Option<usize>,
    pub writes:Vec<String>, pub proof:String,
    #[serde(skip_serializing_if="Option::is_none")]
    pub host_binding:Option<HostBinding>,
}
#[derive(Clone, Debug, Serialize)]
pub struct DefinitionResult {
    pub site:Site, pub lexical:QueryFact<SourceDeclaration>, pub selected:QueryFact<ValueDefinition>,
    pub selector:QuerySelector, pub coverage:String,
}
#[derive(Clone, Debug, Serialize)]
pub struct ReferenceRecord {
    pub id:String, pub site:Site, pub roles:Vec<ReferenceRole>, pub lexical_declaration:Option<String>,
    pub storage:Option<String>, pub storage_owner:Option<StorageOwner>, pub write:Option<String>,
    pub closure:Option<usize>, pub selected:QueryFact<ValueDefinition>,
    pub occurrence:QuerySelector, pub coverage:String, pub proof:String,
}
#[derive(Clone, Debug, Serialize)]
pub struct CallRecord {
    pub id:String, pub site:Site, pub callee:Site, pub lexical_declaration:Option<String>,
    pub selected:QueryFact<ValueDefinition>, pub occurrence:QuerySelector,
    pub evidence_kind:String, pub conditional_branch:Option<usize>, pub invocation_coverage:Option<CallCoverage>,
    pub effect_coverage:Option<CallCoverage>, pub frame:Option<usize>, pub arguments:Vec<Site>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContextOptions { pub depth:usize, pub snippet_bytes:usize }
impl Default for ContextOptions {fn default()->Self{Self{depth:2,snippet_bytes:4096}}}
#[derive(Clone, Debug, Serialize)]
pub struct ContextRecord {
    pub id:String, pub kind:String, pub origin:ValueOrigin,
    #[serde(skip_serializing_if="Option::is_none")]
    pub exhibited:Option<Site>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub snippet:Option<String>,
    pub snippet_truncated:bool, pub depth:usize, pub related:Vec<String>, pub detail:Value,
}
#[derive(Clone, Debug, Serialize)]
pub struct QueryCursor {
    pub token:String,
    #[serde(skip)] owner:Arc<()>, #[serde(skip)] query:String, #[serde(skip)] offset:usize,
}
#[derive(Clone, Debug)]
pub struct PageRequest { pub limit:usize, pub bytes:usize, pub cursor:Option<QueryCursor> }
impl Default for PageRequest {fn default()->Self{Self{limit:50,bytes:1048576,cursor:None}}}
#[derive(Clone, Debug, Serialize)]
pub struct QueryPage<T> {
    pub rows:Vec<T>, pub next_cursor:Option<QueryCursor>, pub has_more:bool,
    pub truncation:Option<String>, pub coverage:String,
}
pub struct SourceQueries<'a> { prepared:&'a PreparedSnapshot, graph:Option<&'a Graph>, selector:QuerySelector }
impl PreparedSnapshot {
    pub fn source_queries(&self,control:&OperationControl)->lycheedev_core::Result<SourceQueries<'_>> {
        require_same_control(&self.control,control)?;Ok(SourceQueries{prepared:self,graph:None,selector:QuerySelector::SyntaxOnly})
    }
}
impl<'a> SourceQueries<'a> {
    pub fn with_execution(&self,graph:&'a Graph,selector:QuerySelector,control:&OperationControl)->lycheedev_core::Result<Self> {
        require_same_control(&self.prepared.control,control)?;graph.check()?;
        if !Arc::ptr_eq(&self.prepared.owner,&graph.owner){return Err(input_error("query graph belongs to another prepared snapshot"))}
        let facts=graph.native_invocations.as_ref().ok_or_else(||input_error("queries require native provenance"))?;
        match selector {
            QuerySelector::Occurrence{occurrence} if occurrence>=facts.control.occurrences.len()=>return Err(input_error("control occurrence missing")),
            QuerySelector::SourceTemplate{function} if function>=graph.functions.len()=>return Err(input_error("source template function missing")),
            QuerySelector::LoadOccurrence{occurrence} if occurrence>=graph.inputs.ordered.len()=>return Err(input_error("load occurrence missing")),
            QuerySelector::XmlOccurrence{action} if graph.inputs.xml.as_ref().is_none_or(|program|action>=program.actions().len())=>return Err(input_error("XML occurrence missing")),
            QuerySelector::Invocation{frame} if facts.frames.get(frame).is_none_or(|frame|frame.call.is_none())=>return Err(input_error("concrete abstract invocation frame missing")),
            QuerySelector::Entry{frame} if facts.frames.get(frame).is_none_or(|frame|!matches!(frame.origin,Some(NativeInvocationOrigin::ExplicitEntry{..})))=>return Err(input_error("explicit entry frame missing")),
            _=>{},
        }
        Ok(Self{prepared:self.prepared,graph:Some(graph),selector})
    }
    fn step(&self)->lycheedev_core::Result<()> {
        let inputs=&self.prepared.inputs;inputs.admission_control.check()?;
        let used=inputs.shared_steps.load(std::sync::atomic::Ordering::Relaxed);
        if used.saturating_add(QUERY_RETURN_RESERVE)>=inputs.corpus.limits.construction_steps{return Err(ToolkitError::new("source.native_query_budget","query traversal exhausted shared steps with bounded return reserve",3))}
        inputs.charge(1)
    }
    fn validate_site(&self,site:&Site)->lycheedev_core::Result<()> {
        self.prepared.control.check()?;self.site_text(site)?;Ok(())
    }
    fn site_text(&self,site:&Site)->lycheedev_core::Result<&str>{
        if self.prepared.inputs.corpus.index.contains_key(&site.file){return site.text(&self.prepared.inputs);}
        self.graph.and_then(|graph|graph.inputs.xml.as_ref()).and_then(|program|program.original(&site.file)).and_then(|text|text.get(site.start as usize..site.end as usize)).ok_or_else(||input_error("query source range unavailable"))
    }
    fn validate_target(&self,target:&QueryTarget)->lycheedev_core::Result<Option<DeclarationId>> {
        self.prepared.control.check()?;
        if matches!(target,QueryTarget::Storage(id)|QueryTarget::Value(id) if !valid_sha(id)){return Err(input_error("invalid query identity"))}
        if self.selector==QuerySelector::SyntaxOnly&&matches!(target,QueryTarget::Storage(_)|QueryTarget::Value(_)){return Err(input_error("storage and selected-value queries require an explicit execution selector"))}
        match target {
            QueryTarget::Site(site)=>{self.validate_site(site)?;Ok(None)},
            QueryTarget::Declaration(id)=>{
                let key=DeclarationId::parse(id).ok_or_else(||input_error("invalid query identity"))?;
                if self.prepared.syntax.declaration_by_key(&key).is_some(){Ok(Some(key))}else{Err(input_error("foreign or unknown declaration identity"))}
            },
            QueryTarget::Value(id)=>{if let Some(graph)=self.graph{for value in &graph.values{self.step()?;if self.value_id(value.id)==*id{return Ok(None)}}}Err(input_error("foreign or unknown selected value identity"))},
            QueryTarget::Storage(id)=>{if let Some(facts)=self.graph.and_then(|graph|graph.native_invocations.as_ref()){for storage in &facts.storages{self.step()?;if self.storage_id(&storage.cell)==*id{return Ok(None)}}}Err(input_error("foreign or unknown storage identity"))},
            QueryTarget::All=>Ok(None),
        }
    }
    fn value_id(&self,value:usize)->String {digest(&(self.graph.map(|graph|graph.snapshot_key.as_str()),"value",value))}
    fn write_id(&self,write:usize)->String {digest(&(self.graph.map(|graph|graph.snapshot_key.as_str()),"write",write))}
    fn storage_id(&self,cell:&str)->String {digest(&(self.graph.map(|graph|graph.snapshot_key.as_str()),"storage",cell))}
    fn occurrence(&self,context:&str)->QuerySelector {
        match self.graph.and_then(|graph|graph.native_invocations.as_ref()).and_then(|facts|facts.query_views.get(context)) {
            Some(NativeQueryView::Load{occurrence})=>QuerySelector::LoadOccurrence{occurrence:*occurrence},
            Some(NativeQueryView::Static{function})=>QuerySelector::SourceTemplate{function:*function},
            Some(NativeQueryView::Invocation{frame})=>QuerySelector::Invocation{frame:*frame},
            Some(NativeQueryView::Entry{frame})=>QuerySelector::Entry{frame:*frame},
            Some(NativeQueryView::Xml{action})=>QuerySelector::XmlOccurrence{action:*action},
            None=>QuerySelector::SyntaxOnly,
        }
    }
    fn exact_occurrence(&self,context:&str)->QuerySelector {self.graph.and_then(|graph|graph.native_invocations.as_ref()).and_then(|facts|facts.control.context_occurrences.get(context)).map(|id|QuerySelector::Occurrence{occurrence:*id}).unwrap_or_else(||self.occurrence(context))}
    fn accepts(&self,context:&str)->bool {self.selector==QuerySelector::AllOccurrences || self.occurrence(context)==self.selector || self.exact_occurrence(context)==self.selector}
    fn conditional_context(&self,context:&str)->bool {self.graph.and_then(|graph|graph.native_invocations.as_ref()).and_then(|facts|facts.context_branches.get(context)).is_some_and(Option::is_some)}
    fn lexical_at(&self,site:&Site)->Option<&IndexedDeclaration> {
        self.prepared.lexical_declaration_at(site)
    }
    fn lexical_id_at(&self,site:&Site)->Option<DeclarationId>{self.lexical_at(site).map(|decl|decl.id)}
    fn selected(&self,decision:&Decision,writes:&[usize],proof:&str)->QueryFact<ValueDefinition> {
        let Some(graph)=self.graph else{return QueryFact::NotApplicable};
        match decision {
            Decision::Bound(id)=>match graph.values.get(*id) {
                Some(value)=>{
                    let function=match value.kind{ValueKind::Function(id)=>Some(id),_=>None};
                    let object=match value.kind{ValueKind::Object(id)=>Some(id),_=>None};
                    let closure=graph.native_invocations.as_ref().and_then(|facts|facts.closures.iter().find(|closure|closure.value==*id)).map(|closure|closure.id);
                    let host_binding=graph.host_runtime.as_ref().and_then(|host|{let binding=match value.kind{ValueKind::HostCallable(binding)|ValueKind::HostReturn{binding,..}=>Some(binding),_=>host.bindings.iter().find(|binding|binding.value==*id).map(|binding|binding.binding)};binding.and_then(|binding|host.bindings.get(binding)).map(|binding|binding.declaration.clone())});
                    QueryFact::Known{value:ValueDefinition{value_id:self.value_id(*id),origin:value.origin.clone(),function_declaration:function.and_then(|id|graph.functions.get(id)).map(|function|function.declaration.clone()),function,object,closure,writes:writes.iter().map(|id|self.write_id(*id)).collect(),proof:proof.into(),host_binding}}
                },None=>QueryFact::Unavailable{reason:"value_identity_missing".into()},
            },
            Decision::Unavailable(reason)=>QueryFact::Unavailable{reason:reason.clone()},
            Decision::KnownAbsent(reason)=>QueryFact::KnownAbsent{reason:reason.clone()},
            Decision::NotManaged=>QueryFact::Unavailable{reason:"not_managed_by_selected_graph".into()},
        }
    }
    pub fn definitions(&self,site:&Site)->lycheedev_core::Result<DefinitionResult> {
        self.validate_site(site)?;self.step()?;
        let lexical=self.lexical_at(site).map(|value|QueryFact::Known{value:value.public(&self.prepared.inputs)}).unwrap_or(QueryFact::NotApplicable);
        let mut selected=QueryFact::NotApplicable;let mut coverage="syntax_only";
        if let Some(graph)=self.graph && self.selector!=QuerySelector::SyntaxOnly {
            let mut matches=Vec::new();
            for fact in &graph.uses {self.step()?;if fact.host_return_slot.is_none()&&fact.site==*site&&self.accepts(&fact.context){matches.push((fact.context.as_str(),self.selected(&fact.decision,&fact.writes,&fact.proof_id)));}}
            if matches.is_empty(){for write in &graph.writes{self.step()?;if write.site==*site&&self.accepts(&write.context){matches.push((write.context.as_str(),self.selected(&write.decision,&[write.id],&self.write_id(write.id))));}}}
            selected=if matches.len()==1{
                let(context,value)=matches.remove(0);coverage=if self.conditional_context(context){"conditional_abstract_selection"}else if matches!(self.occurrence(context),QuerySelector::SourceTemplate{..}){"symbolic_template_selection"}else{"explicit_input_abstract_selection"};value
            }else if matches.is_empty(){coverage="unavailable_in_selected_view";QueryFact::Unavailable{reason:"site_not_observed_in_selected_abstract_execution".into()}}else{
                coverage="multiple_abstract_observations";
                let mut candidates=Vec::new();for(context,_)in &matches{let selector=self.occurrence(context);if !candidates.contains(&selector){candidates.push(selector)}}
                if candidates.len()==1 {candidates.clear();for(context,_)in matches{let selector=self.exact_occurrence(context);if !candidates.contains(&selector){candidates.push(selector)}}}
                if candidates.len()>1{QueryFact::SelectorRequired{candidates}}else{QueryFact::Unavailable{reason:"multiple_conditional_observations_in_selected_view".into()}}
            };
        }
        let result=DefinitionResult{site:site.clone(),lexical,selected,selector:self.selector.clone(),coverage:coverage.into()};
        self.checked_serialize(&result,self.prepared.inputs.corpus.limits.output_bytes)?;Ok(result)
    }
    fn storage(&self,cell:&str)->Option<&StorageFact>{let storages=&self.graph?.native_invocations.as_ref()?.storages;storages.binary_search_by(|storage|storage.cell.as_str().cmp(cell)).ok().map(|id|&storages[id])}
    fn use_cell(&self,id:usize)->Option<&str>{let storages=&self.graph?.native_invocations.as_ref()?.use_storages;storages.binary_search_by_key(&id,|storage|storage.use_fact).ok().map(|index|storages[index].cell.as_str())}
    fn call_callee(&self,call:&CallFact,fact:&UseFact)->Site {
        self.prepared.syntax_callee_at(&call.site).unwrap_or_else(||fact.site.clone())
    }
    fn call_cell(&self,fact:&UseFact)->Option<&str>{self.use_cell(fact.id).or_else(||fact.writes.first().and_then(|id|self.graph?.writes.get(*id)).map(|write|write.cell.as_str()))}
    fn target_matches(&self,target:&QueryTarget,declaration_target:Option<DeclarationId>,site:&Site,lexical:Option<DeclarationId>,cell:Option<&str>,decision:Option<&Decision>)->bool {
        match target {
            QueryTarget::All=>true,QueryTarget::Site(expected)=>expected==site,
            QueryTarget::Declaration(_)=>lexical==declaration_target,
            QueryTarget::Storage(expected)=>cell.is_some_and(|cell|self.storage_id(cell)==*expected),
            QueryTarget::Value(expected)=>matches!(decision,Some(Decision::Bound(value)) if self.value_id(*value)==*expected),
        }
    }
    fn role_match(filter:&ReferenceFilter,roles:&[ReferenceRole])->bool {filter.roles.is_empty()||filter.roles.iter().any(|role|roles.contains(role))}
    pub fn references(&self,target:QueryTarget,filter:ReferenceFilter,page:PageRequest)->lycheedev_core::Result<QueryPage<ReferenceRecord>> {
        let declaration_target=self.validate_target(&target)?;
        if filter.roles.len()>5||filter.roles.iter().enumerate().any(|(i,role)|filter.roles[..i].contains(role)){return Err(input_error("invalid or duplicate reference roles"))}
        self.page_key(&("references",&target,&filter),&page)?;let mut rows=Vec::new();
        if self.selector==QuerySelector::SyntaxOnly || self.graph.is_none() {
            let offset=page.cursor.as_ref().map_or(0,|cursor|cursor.offset);let mut total=0;
            for file in 0..self.prepared.inputs.corpus.files.len() {
                let syntax=public_result(self.prepared.file_syntax(file),&self.prepared.control)?;
                let mut file_rows=Vec::new();
                for reference in &syntax.references {
                    self.step()?;
                    let site=reference.site.public(&self.prepared.inputs);let lexical=reference.declaration.map(|id|self.prepared.syntax.declarations[id].id);
                    if Self::role_match(&filter,&reference.roles)&&self.target_matches(&target,declaration_target,&site,lexical,None,None){file_rows.push(ReferenceRecord{id:reference.id.clone(),site,roles:reference.roles.clone(),lexical_declaration:lexical.map(DeclarationId::public),storage:None,storage_owner:None,write:None,closure:None,selected:QueryFact::NotApplicable,occurrence:QuerySelector::SyntaxOnly,coverage:"syntax_only".into(),proof:reference.id.clone()});}
                }
                for declaration in &self.prepared.syntax.declarations[self.prepared.syntax.declaration_ranges[file].clone()] {
                    self.step()?;let roles=vec![ReferenceRole::Declaration];
                    let site=declaration.site.public(&self.prepared.inputs);
                    if Self::role_match(&filter,&roles)&&self.target_matches(&target,declaration_target,&site,Some(declaration.id),None,None){let public_id=declaration.id.public();file_rows.push(ReferenceRecord{id:digest(&(&public_id,"declaration_reference")),site,roles,lexical_declaration:Some(public_id.clone()),storage:None,storage_owner:None,write:None,closure:None,selected:QueryFact::NotApplicable,occurrence:QuerySelector::SyntaxOnly,coverage:"syntax_only".into(),proof:public_id});}
                }
                file_rows.sort_by_key(|row|self.sort_key(&row.site,&row.occurrence,&row.roles,&row.id));
                for row in file_rows {if total>=offset&&rows.len()<page.limit{rows.push(row)}total+=1;}
            }
            return self.page_window(rows,total,&("references",target,filter),page,None);
        }else if let Some(graph)=self.graph {
            let mut file_syntax:Option<(usize,SourceFileSyntax)>=None;
            if let Some(xml)=&graph.xml{for reference in &xml.references{
                self.step()?;let fact=&graph.uses[reference.use_fact];if !self.accepts(&fact.context){continue;}
                let roles=vec![ReferenceRole::Read];let cell=self.use_cell(fact.id);
                if Self::role_match(&filter,&roles)&&self.target_matches(&target,declaration_target,&fact.site,None,cell,Some(&fact.decision)){
                    rows.push(ReferenceRecord{id:digest(&(graph.snapshot_key.as_str(),"xml_reference",fact.id)),site:fact.site.clone(),roles,lexical_declaration:None,storage:cell.map(|cell|self.storage_id(cell)),storage_owner:cell.and_then(|cell|self.storage(cell)).map(|storage|storage.owner.clone()),write:fact.writes.first().map(|id|self.write_id(*id)),closure:None,selected:self.selected(&fact.decision,&fact.writes,&fact.proof_id),occurrence:self.occurrence(&fact.context),coverage:"xml_declared_reference_observation".into(),proof:fact.proof_id.clone()});
                }
            }}
            for fact in &graph.uses {
                self.step()?;if !self.accepts(&fact.context){continue}
                let Some(&file)=self.prepared.inputs.corpus.index.get(&fact.site.file)else{continue};
                if file_syntax.as_ref().is_none_or(|(current,_)|*current!=file){
                    // Drop the previous file before constructing the next one.
                    drop(file_syntax.take());
                    file_syntax=Some((file,public_result(self.prepared.file_syntax(file),&self.prepared.control)?));
                }
                let Some(syntax)=file_syntax.as_ref().unwrap().1.reference_at(&self.prepared.inputs,&fact.site)else{continue};
                let roles:Vec<_>=syntax.roles.iter().filter(|role|matches!(role,ReferenceRole::Read)).cloned().collect();if roles.is_empty(){continue}
                let cell=self.use_cell(fact.id);let lexical=syntax.declaration.map(|id|self.prepared.syntax.declarations[id].id);
                if Self::role_match(&filter,&roles)&&self.target_matches(&target,declaration_target,&fact.site,lexical,cell,Some(&fact.decision)){
                    rows.push(ReferenceRecord{id:digest(&(graph.snapshot_key.as_str(),"read",fact.id)),site:fact.site.clone(),roles,lexical_declaration:lexical.map(DeclarationId::public),storage:cell.map(|cell|self.storage_id(cell)),storage_owner:cell.and_then(|cell|self.storage(cell)).map(|storage|storage.owner.clone()),write:fact.writes.first().map(|id|self.write_id(*id)),closure:None,selected:self.selected(&fact.decision,&fact.writes,&fact.proof_id),occurrence:self.occurrence(&fact.context),coverage:if self.conditional_context(&fact.context){"conditional_abstract_read"}else if matches!(self.occurrence(&fact.context),QuerySelector::SourceTemplate{..}){"symbolic_template_read"}else{"abstract_input_observation"}.into(),proof:fact.proof_id.clone()});
                }
            }
            for write in &graph.writes {
                self.step()?;if !self.accepts(&write.context){continue}
                let storage=self.storage(&write.cell);let lexical=match storage.map(|storage|&storage.owner){Some(StorageOwner::Local{declaration,..})=>Some(source_decl_key(&self.prepared.inputs,declaration)),_=>self.lexical_id_at(&write.site)};
                let roles=vec![ReferenceRole::Write];
                if Self::role_match(&filter,&roles)&&self.target_matches(&target,declaration_target,&write.site,lexical,Some(&write.cell),Some(&write.decision)){
                    rows.push(ReferenceRecord{id:self.write_id(write.id),site:write.site.clone(),roles,lexical_declaration:lexical.map(DeclarationId::public),storage:Some(self.storage_id(&write.cell)),storage_owner:storage.map(|storage|storage.owner.clone()),write:Some(self.write_id(write.id)),closure:None,selected:self.selected(&write.decision,&[write.id],&self.write_id(write.id)),occurrence:self.occurrence(&write.context),coverage:if write.conditional_branch.is_some(){"conditional_abstract_write"}else{"abstract_input_write"}.into(),proof:self.write_id(write.id)});
                }
            }
            if let Some(facts)=&graph.native_invocations {for capture in &facts.capture_facts{
                self.step()?;if !self.accepts(&capture.creation_context){continue}
                let Some(closure)=facts.closures.get(capture.closure)else{continue};let site=graph.functions[closure.function].closure.clone();
                let lexical=source_decl_key(&self.prepared.inputs,&capture.declaration);let roles=vec![ReferenceRole::Capture];
                if Self::role_match(&filter,&roles)&&self.target_matches(&target,declaration_target,&site,Some(lexical),Some(&capture.cell),None){let proof=digest(&(graph.snapshot_key.as_str(),"capture",capture));rows.push(ReferenceRecord{id:proof.clone(),site,roles,lexical_declaration:Some(lexical.public()),storage:Some(self.storage_id(&capture.cell)),storage_owner:self.storage(&capture.cell).map(|storage|storage.owner.clone()),write:None,closure:Some(capture.closure),selected:QueryFact::NotApplicable,occurrence:self.occurrence(&capture.creation_context),coverage:if self.conditional_context(&capture.creation_context){"conditional_abstract_closure_capture"}else{"abstract_closure_cell_capture"}.into(),proof});}
            }}
            if filter.roles.is_empty()||filter.roles.contains(&ReferenceRole::Call){for call in &graph.calls{
                self.step()?;if !self.accepts(&call.context){continue}let fact=&graph.uses[call.callee];let callee=self.call_callee(call,fact);let lexical=self.lexical_id_at(&callee);let cell=self.call_cell(fact);
                if self.target_matches(&target,declaration_target,&callee,lexical,cell,Some(&fact.decision)){rows.push(ReferenceRecord{id:digest(&(graph.snapshot_key.as_str(),"callee",call.id)),site:callee,roles:vec![ReferenceRole::Call],lexical_declaration:lexical.map(DeclarationId::public),storage:cell.map(|cell|self.storage_id(cell)),storage_owner:cell.and_then(|cell|self.storage(cell)).map(|storage|storage.owner.clone()),write:fact.writes.first().map(|id|self.write_id(*id)),closure:None,selected:self.selected(&fact.decision,&fact.writes,&fact.proof_id),occurrence:self.occurrence(&call.context),coverage:if call.conditional_branch.is_some(){"conditional_abstract_call"}else{"abstract_input_call"}.into(),proof:fact.proof_id.clone()});}
            }}
        }
        rows.sort_by_key(|row|self.sort_key(&row.site,&row.occurrence,&row.roles,&row.id));
        self.page(rows,&("references",target,filter),page,None)
    }
    pub fn calls(&self,target:QueryTarget,page:PageRequest)->lycheedev_core::Result<QueryPage<CallRecord>> {
        let declaration_target=self.validate_target(&target)?;self.page_key(&("calls",&target),&page)?;let mut rows=Vec::new();
        if self.selector==QuerySelector::SyntaxOnly || self.graph.is_none() {
            let offset=page.cursor.as_ref().map_or(0,|cursor|cursor.offset);let mut total=0;
            for file in 0..self.prepared.inputs.corpus.files.len() {
                let syntax=public_result(self.prepared.file_syntax(file),&self.prepared.control)?;
                let mut file_rows=Vec::new();
                for call in &syntax.calls{self.step()?;let callee=call.callee.public(&self.prepared.inputs);let lexical=self.lexical_id_at(&callee);let site=call.site.public(&self.prepared.inputs);
                    if self.target_matches(&target,declaration_target,&callee,lexical,None,None)||matches!(&target,QueryTarget::Site(expected) if expected==&site){file_rows.push(CallRecord{id:call.id.clone(),site,callee,lexical_declaration:lexical.map(DeclarationId::public),selected:QueryFact::NotApplicable,occurrence:QuerySelector::SyntaxOnly,evidence_kind:"syntactic_call_site".into(),conditional_branch:None,invocation_coverage:None,effect_coverage:None,frame:None,arguments:vec![]});}
                }
                file_rows.sort_by_key(|row|self.sort_key(&row.site,&row.occurrence,&[],&row.id));
                for row in file_rows {if total>=offset&&rows.len()<page.limit{rows.push(row)}total+=1;}
            }
            return self.page_window(rows,total,&("calls",target),page,None);
        }else if let Some(graph)=self.graph{for call in &graph.calls{
            self.step()?;if !self.accepts(&call.context){continue}let fact=&graph.uses[call.callee];let callee=self.call_callee(call,fact);let lexical=self.lexical_id_at(&callee);let cell=self.call_cell(fact);
            if self.target_matches(&target,declaration_target,&callee,lexical,cell,Some(&fact.decision))||matches!(&target,QueryTarget::Site(site) if site==&call.site){rows.push(CallRecord{id:digest(&(graph.snapshot_key.as_str(),"call",call.id)),site:call.site.clone(),callee,lexical_declaration:lexical.map(DeclarationId::public),selected:self.selected(&fact.decision,&fact.writes,&fact.proof_id),occurrence:self.occurrence(&call.context),evidence_kind:if call.conditional_branch.is_some(){"conditional_abstract_call"}else if matches!(self.occurrence(&call.context),QuerySelector::SourceTemplate{..}){"symbolic_template_call"}else{"explicit_input_abstract_call"}.into(),conditional_branch:call.conditional_branch,invocation_coverage:Some(call.invocation_coverage.clone()),effect_coverage:Some(call.effect_coverage.clone()),frame:graph.native_invocations.as_ref().and_then(|facts|facts.frames.iter().find(|frame|frame.call==Some(call.id))).map(|frame|frame.id),arguments:call.arguments.iter().map(|id|graph.uses[*id].site.clone()).collect()});}
        }}
        rows.sort_by_key(|row|self.sort_key(&row.site,&row.occurrence,&[],&row.id));self.page(rows,&("calls",target),page,None)
    }
    fn sort_key(&self,site:&Site,occurrence:&QuerySelector,roles:&[ReferenceRole],id:&str)->(usize,u32,u32,(u8,usize),String,String) {
        let occurrence=match occurrence{QuerySelector::SyntaxOnly=>(0,0),QuerySelector::LoadOccurrence{occurrence}=>(1,*occurrence),QuerySelector::SourceTemplate{function}=>(2,*function),QuerySelector::Invocation{frame}=>(3,*frame),QuerySelector::Occurrence{occurrence}=>(4,*occurrence),QuerySelector::AllOccurrences=>(5,0),QuerySelector::Entry{frame}=>(6,*frame),QuerySelector::XmlOccurrence{action}=>(7,*action)};
        (self.prepared.inputs.corpus.index.get(&site.file).copied().unwrap_or(usize::MAX),site.start,site.end,occurrence,format!("{roles:?}"),id.into())
    }
    fn checked_serialize<T:Serialize>(&self,value:&T,limit:usize)->lycheedev_core::Result<Vec<u8>> {
        self.prepared.inputs.charge(1)?;
        let mut writer=QueryWriter{inputs:&self.prepared.inputs,bytes:Vec::new(),limit};
        let result=serde_json::to_writer(&mut writer,value);self.prepared.control.check()?;
        result.map_err(|error|ToolkitError::new("source.native_query_output",error.to_string(),3))?;Ok(writer.bytes)
    }
    fn page<T:Serialize+Clone>(&self,rows:Vec<T>,query:&impl Serialize,request:PageRequest,truncation:Option<String>)->lycheedev_core::Result<QueryPage<T>> {
        let total=rows.len();let offset=request.cursor.as_ref().map_or(0,|cursor|cursor.offset);
        let window=rows.into_iter().skip(offset).take(request.limit).collect();
        self.page_window(window,total,query,request,truncation)
    }
    // `rows` is the cursor window in the original global sort order; `total`
    // includes every matching row, including files outside that window.
    fn page_window<T:Serialize+Clone>(&self,rows:Vec<T>,total:usize,query:&impl Serialize,request:PageRequest,truncation:Option<String>)->lycheedev_core::Result<QueryPage<T>> {
        let key=self.page_key(query,&request)?;
        let offset=request.cursor.as_ref().map_or(0,|cursor|cursor.offset);
        if offset>total{return Err(input_error("query cursor outside stable result"))}
        let coverage=if self.selector==QuerySelector::SyntaxOnly{"syntax_facts_only"}else if self.graph.and_then(|graph|graph.native_invocations.as_ref()).is_some_and(|facts|!facts.control.complete){if self.graph.and_then(|graph|graph.native_invocations.as_ref()).is_some_and(|facts|!facts.execution_boundaries.is_empty()){"unavailable_execution_plan"}else{"verified_prefix_with_explicit_boundaries"}}else{"bounded_explicit_input_abstract_facts"}.to_string();
        let cursor_at=|end|QueryCursor{token:digest(&(&key,end)),owner:self.prepared.owner.clone(),query:key.clone(),offset:end};
        let empty_more=QueryPage::<T>{rows:vec![],next_cursor:Some(cursor_at(offset)),has_more:true,truncation:truncation.clone(),coverage:coverage.clone()};
        let empty_end=QueryPage::<T>{rows:vec![],next_cursor:None,has_more:false,truncation:truncation.clone(),coverage:coverage.clone()};
        let more_bytes=self.checked_serialize(&empty_more,self.prepared.inputs.corpus.limits.output_bytes)?.len();
        let end_bytes=self.checked_serialize(&empty_end,self.prepared.inputs.corpus.limits.output_bytes)?.len();
        let mut end=offset;let mut row_bytes=0usize;
        for row in rows.iter().take(request.limit){
            let encoded=self.checked_serialize(row,self.prepared.inputs.corpus.limits.output_bytes)?;
            let next_bytes=row_bytes+encoded.len()+usize::from(end>offset);
            let base=if end+1<total{more_bytes}else{end_bytes};
            if next_bytes.saturating_add(base)>request.bytes{break}
            row_bytes=next_bytes;end+=1;
        }
        if end==offset&&offset<total{return Err(ToolkitError::new("source.native_query_record_too_large","one result and page metadata exceed requested byte budget",3))}
        let has_more=end<total;let result=QueryPage{rows:rows[..end-offset].to_vec(),next_cursor:has_more.then(||cursor_at(end)),has_more,truncation,coverage};
        let encoded=self.checked_serialize(&result,self.prepared.inputs.corpus.limits.output_bytes)?;
        if encoded.len()>request.bytes{return Err(ToolkitError::new("source.native_query_record_too_large","page metadata exceeds requested byte budget",3))}Ok(result)
    }
    fn page_key(&self,query:&impl Serialize,request:&PageRequest)->lycheedev_core::Result<String>{
        self.prepared.control.check()?;
        if request.limit==0||request.limit>200||request.bytes==0||request.bytes>self.prepared.inputs.corpus.limits.output_bytes{return Err(input_error("invalid query page row or byte limit"))}
        let key=digest(&(self.prepared.inputs.identity_sha256(),self.graph.map(|graph|graph.snapshot_key.as_str()),&self.selector,query,request.limit,request.bytes,query_policy()));
        if let Some(cursor)=&request.cursor && (!Arc::ptr_eq(&cursor.owner,&self.prepared.owner)||cursor.query!=key||cursor.token!=digest(&(&key,cursor.offset))){return Err(input_error("foreign cursor or changed query options"))}Ok(key)
    }
}

// Count serialized chunks against the same admission as traversal. Writes are
// buffered to avoid charging serde's punctuation-sized write calls individually.
struct QueryWriter<'a>{inputs:&'a SourceInputs,bytes:Vec<u8>,limit:usize}
impl Write for QueryWriter<'_>{
    fn write(&mut self,bytes:&[u8])->std::io::Result<usize>{
        self.inputs.admission_control.check().map_err(std::io::Error::other)?;
        let next=self.bytes.len().checked_add(bytes.len()).ok_or_else(||std::io::Error::other("query output overflow"))?;
        if next>self.limit{return Err(std::io::Error::other("query output byte budget"))}
        let before=self.bytes.len().div_ceil(16384);let after=next.div_ceil(16384);
        self.inputs.charge(after-before).map_err(std::io::Error::other)?;
        self.bytes.extend_from_slice(bytes);Ok(bytes.len())
    }
    fn flush(&mut self)->std::io::Result<()>{self.inputs.admission_control.check().map_err(std::io::Error::other)}
}

#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord)]
enum ContextNode { Declaration(usize), Use(usize), Write(usize), Value(usize), Function(usize), Call(usize), Frame(usize), Copy(usize), XmlCopy(usize), Capture(usize), Load(usize) }
impl SourceQueries<'_> {
    fn context_node_id(&self,node:&ContextNode)->String {
        match node {
            ContextNode::Declaration(id)=>self.prepared.syntax.declarations[*id].id.public(),
            ContextNode::Write(id)=>self.write_id(*id),ContextNode::Value(id)=>self.value_id(*id),
            other=>digest(&(self.prepared.inputs.identity_sha256(),self.graph.map(|graph|graph.snapshot_key.as_str()),"context",format!("{other:?}"))),
        }
    }
    pub fn context(&self,target:QueryTarget,options:ContextOptions,page:PageRequest)->lycheedev_core::Result<QueryPage<ContextRecord>> {
        let declaration_target=self.validate_target(&target)?;
        if options.depth>4||options.snippet_bytes==0||options.snippet_bytes>65536{return Err(input_error("context depth or snippet byte bounds"))}
        self.page_key(&("context",&target,&options),&page)?;
        let mut work=std::collections::VecDeque::new();let mut truncation=None;
        let seed=|work:&mut std::collections::VecDeque<(ContextNode,usize)>,node,truncation:&mut Option<String>|{if work.len()<4096{work.push_back((node,0));}else{*truncation=Some("context_pending_node_budget".into());}};
        let index=&self.prepared.syntax;
        for (id,decl) in index.declarations.iter().enumerate(){
            self.step()?;
            let matches=match &target{QueryTarget::All=>true,QueryTarget::Declaration(_)=>declaration_target==Some(decl.id),QueryTarget::Site(site)=>self.lexical_id_at(site)==Some(decl.id),_=>false};
            if matches{seed(&mut work,ContextNode::Declaration(id),&mut truncation);}
        }
        if let Some(graph)=self.graph && self.selector!=QuerySelector::SyntaxOnly {
            for fact in &graph.uses{self.step()?;if !self.accepts(&fact.context){continue}
                if self.target_matches(&target,declaration_target,&fact.site,self.lexical_id_at(&fact.site),self.use_cell(fact.id),Some(&fact.decision)){seed(&mut work,ContextNode::Use(fact.id),&mut truncation);}
            }
            for write in &graph.writes{self.step()?;if !self.accepts(&write.context){continue}
                let lexical=match self.storage(&write.cell).map(|storage|&storage.owner){Some(StorageOwner::Local{declaration,..})=>Some(source_decl_key(&self.prepared.inputs,declaration)),_=>self.lexical_id_at(&write.site)};
                if self.target_matches(&target,declaration_target,&write.site,lexical,Some(&write.cell),Some(&write.decision)){seed(&mut work,ContextNode::Write(write.id),&mut truncation);}
            }
            for call in &graph.calls {self.step()?;if self.accepts(&call.context)&&matches!(&target,QueryTarget::Site(site) if site==&call.site){seed(&mut work,ContextNode::Call(call.id),&mut truncation);}}
            if let QueryTarget::Value(expected)=&target {for value in &graph.values {self.step()?;if self.value_id(value.id)==*expected{seed(&mut work,ContextNode::Value(value.id),&mut truncation);}}}
        }
        let mut visited=std::collections::BTreeSet::new();let mut rows=Vec::new();
        while let Some((node,depth))=work.pop_front(){
            self.step()?;if !visited.insert(node.clone()){continue}
            if rows.len()>=512{truncation=Some("context_node_budget".into());break}
            let (kind,origin,detail,next)=self.context_detail(&node)?;
            let xml=match &origin{ValueOrigin::Xml{xml}=>Some(xml_site(xml)),_=>None};
            let (exhibited,snippet,snippet_truncated)=if let Some(site)=origin.source().or(xml.as_ref()){
                let source=self.site_text(site)?;
                let mut len=source.len().min(options.snippet_bytes);while !source.is_char_boundary(len){len-=1}
                (Some(Site{file:site.file.clone(),start:site.start,end:site.start+len as u32}),Some(source[..len].to_string()),len<source.len())
            }else{(None,None,false)};
            let related=next.iter().map(|next|self.context_node_id(next)).collect();
            rows.push(ContextRecord{id:self.context_node_id(&node),kind,origin,exhibited,snippet,snippet_truncated,depth,related,detail});
            if depth<options.depth {
                for next in next{if work.len()>=4096{truncation=Some("context_pending_node_budget".into());break}work.push_back((next,depth+1));}
            }else if !next.is_empty()&&truncation.is_none(){truncation=Some("context_depth_limit".into());}
        }
        rows.sort_by_key(|row|{let (file,start,end)=row.origin.source().map(|site|(self.prepared.inputs.corpus.index.get(&site.file).copied().unwrap_or(usize::MAX),site.start,site.end)).unwrap_or((usize::MAX,0,0));(file,start,end,row.depth,row.id.clone())});
        self.page(rows,&("context",target,options),page,truncation)
    }
    fn context_detail(&self,node:&ContextNode)->lycheedev_core::Result<(String,ValueOrigin,Value,Vec<ContextNode>)> {
        self.step()?;let mut next=Vec::new();
        if let ContextNode::Declaration(id)=node {
            let declaration=self.prepared.syntax.declarations[*id].public(&self.prepared.inputs);
            return Ok(("lexical_declaration".into(),ValueOrigin::Source(declaration.site.clone()),json!({"declaration":declaration,"sourceIdentity":self.prepared.inputs.identity_sha256(),"syntaxCheck":"pending"}),next));
        }
        let graph=self.graph.ok_or_else(||input_error("execution context node without graph"))?;
        let load_for=|context:&str|match self.occurrence(context){QuerySelector::LoadOccurrence{occurrence}=>Some(occurrence),QuerySelector::Invocation{frame}|QuerySelector::Entry{frame}=>graph.native_invocations.as_ref().and_then(|facts|facts.frames.get(frame)).and_then(|frame|frame.closure).and_then(|closure|graph.native_invocations.as_ref()?.closures.get(closure)).map(|closure|closure.load_occurrence),_=>None};
        let add_decl=|site:&Site,next:&mut Vec<ContextNode>|{if let Some(id)=self.lexical_id_at(site).and_then(|id|self.prepared.syntax.declaration_by_key(&id)){next.push(ContextNode::Declaration(id));}};
        let result=match node {
            ContextNode::Use(id)=>{
                let fact=&graph.uses[*id];add_decl(&fact.site,&mut next);if let Decision::Bound(value)=fact.decision{next.push(ContextNode::Value(value))}
                next.extend(fact.writes.iter().copied().map(ContextNode::Write));
                if let QuerySelector::Invocation{frame}|QuerySelector::Entry{frame}=self.occurrence(&fact.context){next.push(ContextNode::Frame(frame))}
                if let Some(occurrence)=load_for(&fact.context){next.push(ContextNode::Load(occurrence))}
                for call in &graph.calls{self.step()?;if call.callee==*id||call.result_fact==Some(*id){next.push(ContextNode::Call(call.id));}}
                ("selected_use",fact.site.clone(),json!({"fact":fact,"occurrence":self.occurrence(&fact.context),"selected":self.selected(&fact.decision,&fact.writes,&fact.proof_id),"storage":self.use_cell(*id).and_then(|cell|self.storage(cell))}))
            },
            ContextNode::Write(id)=>{
                let write=&graph.writes[*id];add_decl(&write.site,&mut next);
                if let Some(previous)=write.previous{next.push(ContextNode::Write(previous))}
                next.extend(write.inputs.iter().copied().map(ContextNode::Use));if let Decision::Bound(value)=write.decision{next.push(ContextNode::Value(value))}
                for copy in &graph.copies{self.step()?;if copy.destination_field_write==*id{next.push(ContextNode::Copy(copy.id));}}
                if let Some(xml)=&graph.xml{for copy in &xml.copies{self.step()?;if copy.destination_field_write==*id{next.push(ContextNode::XmlCopy(copy.id));}}}
                if let Some(occurrence)=load_for(&write.context){next.push(ContextNode::Load(occurrence))}
                ("storage_write",write.site.clone(),json!({"write":write,"storage":self.storage(&write.cell),"storageId":self.storage_id(&write.cell),"occurrence":self.occurrence(&write.context)}))
            },
            ContextNode::Value(id)=>{
                let value=&graph.values[*id];
                let mut detail=json!({"value":value,"selected":self.selected(&Decision::Bound(*id),&[],&self.value_id(*id))});
                if let Some(host)=&graph.host_runtime {
                    let binding=match value.kind{ValueKind::HostCallable(binding)|ValueKind::HostReturn{binding,..}=>Some(binding),_=>host.bindings.iter().find(|binding|binding.value==*id).map(|binding|binding.binding)};
                    if let Some(binding)=binding{detail["hostBinding"]=json!(host.bindings.get(binding));}
                }
                let Some(origin)=value.origin.source() else{return Ok((match value.origin{ValueOrigin::Entry{..}=>"entry_argument_origin",ValueOrigin::Xml{..}=>"xml_object_origin",_=>"environment_value_origin"}.into(),value.origin.clone(),detail,next))};
                add_decl(origin,&mut next);
                if let ValueKind::Function(function)=value.kind{next.push(ContextNode::Function(function))}
                if let Some(facts)=&graph.native_invocations {for (capture_id,capture) in facts.capture_facts.iter().enumerate(){self.step()?;if facts.closures[capture.closure].value==*id{next.push(ContextNode::Capture(capture_id));}}}
                ("selected_value_origin",origin.clone(),detail)
            },
            ContextNode::Call(id)=>{
                let call=&graph.calls[*id];next.push(ContextNode::Use(call.callee));next.extend(call.arguments.iter().copied().map(ContextNode::Use));if let Some(result)=call.result_fact{next.push(ContextNode::Use(result))}
                if let Some(facts)=&graph.native_invocations {for frame in &facts.frames {self.step()?;if frame.call==Some(*id){next.push(ContextNode::Frame(frame.id));}}}
                if let Some(occurrence)=load_for(&call.context){next.push(ContextNode::Load(occurrence))}
                let mut detail=json!({"call":call,"evidenceKind":"explicit_input_abstract_analysis","occurrence":self.occurrence(&call.context),"conditional":call.conditional_branch.is_some()});
                if let Some(host)=&graph.host_runtime{detail["commonSlots"]=json!(host.calls.iter().find(|slots|slots.call==*id));}
                ("abstract_call",call.site.clone(),detail)
            },
            ContextNode::Function(id)=>{
                let function=&graph.functions[*id];add_decl(&function.declaration,&mut next);
                ("source_function_definition",function.closure.clone(),json!({"function":function,"executionIdentity":graph.snapshot_key}))
            },
            ContextNode::Frame(id)=>{
                let facts=graph.native_invocations.as_ref().ok_or_else(||input_error("invocation facts missing"))?;let frame=&facts.frames[*id];
                next.push(ContextNode::Function(frame.function));next.extend(frame.return_facts.iter().copied().map(ContextNode::Use));
                if let Some(call)=frame.call{next.push(ContextNode::Call(call))}
                for (index,capture) in facts.capture_facts.iter().enumerate(){self.step()?;if frame.closure==Some(capture.closure){next.push(ContextNode::Capture(index));}}
                ("abstract_invocation_frame",graph.functions[frame.function].closure.clone(),json!({"frame":frame,"evidenceKind":"explicit_input_abstract_analysis"}))
            },
            ContextNode::Copy(id)=>{
                let copy=&graph.copies[*id];next.push(ContextNode::Write(copy.source_field_write));next.push(ContextNode::Call(copy.call));
                ("profile_copy_edge",graph.writes[copy.destination_field_write].site.clone(),json!({"copy":copy,"destinationWrite":self.write_id(copy.destination_field_write),"sourceWrite":self.write_id(copy.source_field_write)}))
            },
            ContextNode::XmlCopy(id)=>{
                let copy=&graph.xml.as_ref().ok_or_else(||input_error("XML facts missing"))?.copies[*id];
                next.push(ContextNode::Write(copy.source_field_write));next.push(ContextNode::Use(copy.reference_use));next.push(ContextNode::Use(copy.field_use));
                ("xml_mixin_copy_edge",xml_site(&copy.origin),json!({"copy":copy,"destinationWrite":self.write_id(copy.destination_field_write),"sourceWrite":self.write_id(copy.source_field_write)}))
            },
            ContextNode::Capture(id)=>{
                let facts=graph.native_invocations.as_ref().ok_or_else(||input_error("capture facts missing"))?;let capture=&facts.capture_facts[*id];let closure=&facts.closures[capture.closure];
                if let Some(declaration)=self.prepared.syntax.declaration_at(&self.prepared.inputs,&capture.declaration){next.push(ContextNode::Declaration(declaration))}
                next.push(ContextNode::Load(closure.load_occurrence));
                ("closure_cell_capture",graph.functions[closure.function].closure.clone(),json!({"capture":capture,"storage":self.storage(&capture.cell),"storageId":self.storage_id(&capture.cell),"valueReadAtCreation":false}))
            },
            ContextNode::Load(occurrence)=>{
                let file=graph.inputs.ordered.get(*occurrence).ok_or_else(||input_error("context load occurrence missing"))?;
                ("load_occurrence",Site{file:file.clone(),start:0,end:0},json!({"occurrence":occurrence,"file":file,"phase":graph.phase,"executionIdentity":graph.snapshot_key,"admissionIdentity":graph.inputs.identity_sha256(),"orderedOccurrences":graph.inputs.ordered,"sourceExecution":false,"syntaxCheck":"pending"}))
            },
            ContextNode::Declaration(_)=>return Err(input_error("unexpected declaration context node")),
        };
        Ok((result.0.into(),graph.inputs.origin_at(&result.1),result.2,next))
    }
}
