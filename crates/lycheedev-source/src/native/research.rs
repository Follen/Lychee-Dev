include!("research_caller.rs");
#[path = "research_projection.rs"]
mod research_projection;
#[path = "research_context_projection.rs"]
mod research_context_projection;
#[path = "research_use_index.rs"]
mod research_use_index;
use research_context_projection::{ContextFiles, FileProjectionRange};
use research_use_index::ResearchUses;
#[path = "research_output.rs"]
mod research_output;
pub(crate) use research_output::{ResearchOperationAnswer,ResearchProjectionResult,ResearchAnalysis,ResearchScopeEvidence,owned_analysis};
type RetainedResearchProjection<'a, R = ResearchProjectionResult> = research_projection::RetainedProjection<'a, PreparedSnapshot, crate::native_research::QueryRequest, Graph, crate::native_research::XmlResearch, R>;
pub(crate) enum ResearchProjection<'a> {
    Incoming(RetainedResearchProjection<'a>),
    Context(RetainedResearchProjection<'a, ContextFiles>),
}

#[cfg(test)]
thread_local! { static RESEARCH_PROJECTION_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(crate) fn research_projection_passes()->usize {RESEARCH_PROJECTION_PASSES.with(|count|count.get())}
#[cfg(test)]
pub(crate) fn context_projection_reused_files()->usize {research_context_projection::reused_files()}

// Production projection over the same syntax identities and abstract value facts.
impl PreparedSnapshot {
    /// The session owner calls this only between requests on its single worker thread.
    pub fn begin_research_request(&mut self, control:&OperationControl)->lycheedev_core::Result<()> {
        control.check()?;
        if !self.inputs.corpus.limits.research{return Err(input_error("request renewal requires a research snapshot"));}
        let inputs=Arc::make_mut(&mut self.inputs);
        inputs.admission_control=control.clone();inputs.host.admission_control=control.clone();
        inputs.shared_steps=Arc::new(std::sync::atomic::AtomicUsize::new(0));
        self.control=control.clone();self.owner=Arc::new(());Ok(())
    }

    /// Plan from admitted original bytes and Emmy syntax identities. No caller
    /// supplied coordinates, batch totals or source hashes are trusted here.
    pub(crate) fn plan_research_operations(&self, query:&crate::native_research::QueryRequest, origins:&crate::native_research::XmlResearch,api:Option<&crate::native_research::plan::VerifiedApiDefinitions>)->lycheedev_core::Result<crate::native_research::plan::ResearchPlan> {
        use crate::native_research::plan::{ResearchPlan,unique_name_range};
        self.control.check()?;
        if !["incoming","outgoing","both"].contains(&query.direction.as_str()) {return Err(input_error("invalid native relation direction"));}
        let mut plan=ResearchPlan::new(self.inputs.identity_sha256(),query,self.owner.clone());
        let named_xml=query.symbol.path.to_ascii_lowercase().ends_with(".xml")&&!query.symbol.category.starts_with("xml-");
        let mut named_scopes=BTreeMap::new();
        if named_xml {
            for (file,script) in &origins.scripts {
                self.control.check()?;self.inputs.charge(1)?;
                if script.path!=query.symbol.path {continue;}
                let body=script.body_text();
                let original=self.inputs.original(file)?;
                let Some(offset)=original.find(body).filter(|offset|original.rfind(body)==Some(*offset)) else {plan.unavailable("xml_decoded_body_position_unavailable");continue;};
                let facts=plan.scan_facts(body,file,&self.control)?;
                for declaration in &facts.declarations {
                    self.inputs.charge(1)?;
                    if declaration.name==query.symbol.name&&script.original_body_line(declaration.line)?==query.symbol.line
                        &&script.original_body_line(declaration.end_line)?==query.symbol.end_line.max(query.symbol.line) {
                        named_scopes.entry(file.clone()).or_insert_with(Vec::new).push((declaration.line,declaration.end_line,offset));
                    }
                }
            }
            if named_scopes.values().map(Vec::len).sum::<usize>()!=1 {
                plan.unavailable("xml_named_function_selector_unavailable");named_scopes.clear();
            }
        }
        if query.direction!="outgoing" && query.symbol.category.starts_with("api-") {
            if let Some(target)=api.map(|api|api.target(&query.symbol)).transpose()?.flatten() {plan.incoming(target);}
            else {plan.unavailable("incoming_unique_api_definition_mapping_unavailable");}
        } else if query.direction!="outgoing" && named_xml {
            let mut target=None;
            if let Some((file,scopes))=named_scopes.iter().next() {
                let (line,_,offset)=scopes[0];
                let script=&origins.scripts[file];
                if let Some(range)=unique_name_range(script.body_text(),line,&query.symbol.name) {
                    target=self.research_syntax_target(self.inputs.corpus.index[file],range.start+offset..range.end+offset)?;
                }
            }
            if let Some(site)=target {plan.incoming(self.research_authenticated_site(site,origins)?);}
            else {plan.unavailable("incoming_original_syntax_position_unavailable");}
        } else if query.direction!="outgoing" {
            let target=if let Some(&file)=self.inputs.corpus.index.get(&query.symbol.path) {
                let text=self.inputs.original(&query.symbol.path)?;
                if let Some(range)=unique_name_range(text,query.symbol.line,&query.symbol.name) {
                    let syntax=public_result(self.file_syntax(file),&self.control)?;
                    self.inputs.charge(syntax.references.len().saturating_add(syntax.calls.len()).saturating_add(1))?;
                    let mut sites=syntax.references.iter().map(|item|item.site.public(&self.inputs))
                        .chain(self.syntax.declarations.iter().filter(|item|item.site.file==file).map(|item|item.site.public(&self.inputs)))
                        .filter(|site|site.start as usize<=range.start && site.end as usize>=range.end)
                        .collect::<Vec<_>>();
                    // The smallest authenticated expression containing the
                    // exact spelling is the target; duplicate records agree.
                    sites.sort_by_key(|site|(site.end-site.start,site.start));sites.dedup();
                    sites.first().cloned()
                } else { None }
            } else { None };
            if let Some(site)=target {plan.incoming(self.research_authenticated_site(site,origins)?);}
            else {plan.unavailable("incoming_original_syntax_position_unavailable");}
        }
        if query.direction!="incoming" {
            'files: for file in 0..self.inputs.corpus.files.len() {
                self.control.check()?;self.inputs.charge(1)?;
                if !origins.may_contain_outgoing(&query.symbol,&self.inputs.corpus.files[file].path){continue;}
                let original=self.inputs.original(&self.inputs.corpus.files[file].path)?;
                // The same business scanner used by the source index assigns
                // each edge to its nearest named/anonymous caller. A containing
                // line interval alone would incorrectly charge nested callers.
                let inline=origins.scripts.get(&self.inputs.corpus.files[file].path);
                if inline.is_some() && named_xml {
                    if !named_scopes.contains_key(&self.inputs.corpus.files[file].path){continue;}
                } else if inline.is_some() {
                    match origins.planner_script_eligibility(&query.symbol,&self.inputs.corpus.files[file].path,||self.inputs.charge(1))? {
                        Some(true)=>{}, Some(false)=>continue,
                        None=>{plan.unavailable("xml_script_owner_unavailable");continue;},
                    }
                }
                let (scan_text,body_offset)=if let Some(script)=inline {
                    let body=script.body_text();
                    // Authenticate the exact decoded body inside the admitted
                    // generated source; never compare its line numbers to XML.
                    let Some(start)=original.find(body).filter(|start|original.rfind(body)==Some(*start)) else {
                        plan.unavailable("xml_decoded_body_position_unavailable");continue;
                    };
                    (body,start)
                } else {(original,0)};
                let facts=plan.scan_facts(scan_text,&self.inputs.corpus.files[file].path,&self.control)?;
                self.inputs.charge(facts.relationships.len().saturating_add(1))?;
                let syntax=public_result(self.file_syntax(file),&self.control)?;
                self.inputs.charge(syntax.references.len().saturating_add(syntax.calls.len()).saturating_add(1))?;
                for edge in &facts.relationships {
                    self.control.check()?;self.inputs.charge(1)?;
                    if edge.category!="call" {continue;}
                    if inline.is_some() {
                        // Scan the decoded body without its synthetic anonymous
                        // wrapper. Root-body calls belong to the authenticated
                        // XML owner; nested named/anonymous functions do not.
                        if named_xml {
                            let scopes=&named_scopes[&self.inputs.corpus.files[file].path];
                            if edge.from!=query.symbol.name||!scopes.iter().any(|(first,last,_)|edge.line>=*first&&edge.line<=*last){continue;}
                        } else if edge.from!="<file>" {continue;}
                    } else if edge.from!=query.symbol.name||edge.line<query.symbol.line||edge.line>query.symbol.end_line.max(query.symbol.line) {continue;}
                    // Match Go: the first further eligible edge proves the cut;
                    // do not resolve/authenticate all remaining sites to count it.
                    if plan.definition_full(){plan.truncate_outgoing();break 'files;}
                    let Some(range)=unique_name_range(scan_text,edge.line,&edge.to) else {
                        plan.unavailable("outgoing_original_syntax_position_ambiguous");continue;
                    };
                    let range=range.start+body_offset..range.end+body_offset;
                    let mut candidates=syntax.calls.iter().map(|call|call.callee.public(&self.inputs))
                        .filter(|site|site.start as usize<=range.start&&site.end as usize>=range.end);
                    let Some(site)=candidates.next() else {plan.unavailable("outgoing_original_syntax_position_unavailable");continue;};
                    if candidates.next().is_some(){plan.unavailable("outgoing_original_syntax_position_ambiguous");continue;}
                    if inline.is_none()&&!origins.within(self,&query.symbol,&site)? {continue;}
                    plan.definition(self.research_authenticated_site(site,origins)?);
                }
            }
        }
        self.control.check()?;
        Ok(plan.finish())
    }
    fn research_syntax_target(&self,file:usize,range:std::ops::Range<usize>)->lycheedev_core::Result<Option<Site>> {
        let syntax=public_result(self.file_syntax(file),&self.control)?;
        self.inputs.charge(syntax.references.len().saturating_add(syntax.calls.len()).saturating_add(1))?;
        let mut sites=syntax.references.iter().map(|item|item.site.public(&self.inputs))
            .chain(self.syntax.declarations.iter().filter(|item|item.site.file==file).map(|item|item.site.public(&self.inputs)))
            .filter(|site|site.start as usize<=range.start&&site.end as usize>=range.end).collect::<Vec<_>>();
        sites.sort_by_key(|site|(site.end-site.start,site.start));sites.dedup();
        Ok(sites.first().cloned())
    }
    fn research_authenticated_site(&self,site:Site,origins:&crate::native_research::XmlResearch)->lycheedev_core::Result<crate::native_research::plan::AuthenticatedSite> {
        site.text(&self.inputs)?;self.inputs.charge(1)?;
        let file=self.inputs.file(&site.file).map_err(|message|input_error(&message))?;
        let sha=file.identity["sha256"].as_str().ok_or_else(||input_error("admitted source identity missing"))?;
        Ok(crate::native_research::plan::AuthenticatedSite{position:origins.position(self,&site)?,site,file_sha256:sha.into(),material:json!({"kind":"source"})})
    }
    pub(crate) fn validate_research_plan(&self,plan:&crate::native_research::plan::ResearchPlan,query:&crate::native_research::QueryRequest)->lycheedev_core::Result<()> {
        self.control.check()?;plan.validate(self.inputs.identity_sha256(),query,&self.owner)
    }
    pub(crate) fn carry_research_steps(&self,spent:usize,control:&OperationControl)->lycheedev_core::Result<()> {
        require_same_control(&self.control,control)?;self.inputs.charge(spent)
    }
    pub(crate) fn research_steps(&self)->usize {
        self.inputs.shared_steps.load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn execute_research_operations<'a>(&'a self,plan:&crate::native_research::plan::ResearchPlan,query:&'a crate::native_research::QueryRequest,graph:Option<&'a Graph>,origins:&'a crate::native_research::XmlResearch,api:Option<&crate::native_research::plan::VerifiedApiDefinitions>)->lycheedev_core::Result<ResearchScopeCompletion<'a>> {
        self.execute_research_operations_observed(plan,query,graph,origins,api,&mut |_|{})
    }
    pub(crate) fn execute_research_operations_observed<'a>(&'a self,plan:&crate::native_research::plan::ResearchPlan,query:&'a crate::native_research::QueryRequest,graph:Option<&'a Graph>,origins:&'a crate::native_research::XmlResearch,api:Option<&crate::native_research::plan::VerifiedApiDefinitions>,observer:&mut impl FnMut(&'static str))->lycheedev_core::Result<ResearchScopeCompletion<'a>> {
        use crate::native_research::plan::LogicalOperation;
        self.validate_research_plan(plan,query)?;
        let mut answers=Vec::with_capacity(plan.operations().len());
        let mut performed=Vec::with_capacity(plan.operations().len());
        let mut projection=None;
        for (batch,operations) in plan.operations().chunks(crate::native_research::plan::BATCH_QUERIES).enumerate(){
            for operation in operations {
                self.control.check()?;self.inputs.charge(1)?;
                observer("query_operation_authenticate");
                let target=match operation {LogicalOperation::References(target)|LogicalOperation::Type(target)|LogicalOperation::Definition(target)=>target};
                if target.material["kind"]=="source" {
                    let file=self.inputs.file(&target.site.file).map_err(|message|input_error(&message))?;
                    if file.identity["sha256"]!=target.file_sha256||origins.position(self,&target.site)?!=target.position {return Err(input_error("semantic operation source identity differs"));}
                    target.site.text(&self.inputs)?;
                }
                let mut reference_analysis=None;
                let answer=match operation {
                    LogicalOperation::References(_)=>{
                        let before=self.research_steps();
                        let mut files=Vec::new();
                        observer("query_references_project");
                        let result=self.research_relations_with_context_observed(&query.symbol,"incoming",graph,origins,
                            (query.direction=="both").then_some(&mut files),None,observer)?;
                        let steps=self.research_steps().checked_sub(before).ok_or_else(||input_error("projection step counter moved backwards"))?;
                        let rows=&result.relations;
                        observer("query_references_digest");
                        let answer=json!({"kind":"references","state":if result.analysis["state"]=="complete"{"known"}else{"partial"},
                            "result":{"relationsSHA256":digest(&rows),"relationCount":rows.len()},
                            "evidence":"full relation projection digest and coverage; source research retains the complete projected rows"});
                        reference_analysis=Some(Arc::clone(&result.analysis));
                        observer("query_references_retain");
                        if query.direction=="incoming"&&projection.is_none(){
                            projection=RetainedResearchProjection::new(self,query,graph,origins,result,steps,||self.control.check())?.map(ResearchProjection::Incoming);
                        } else if query.direction=="both"&&projection.is_none()&&!files.is_empty(){
                            // Only files whose original path and XML ownership
                            // exclude every outgoing row are direction-invariant.
                            // Move those rows; graphControl and XML tails are dropped.
                            let files=ContextFiles::from_projection(result.into_context_rows(),files,||self.control.check())?;
                            projection=RetainedResearchProjection::new(self,query,graph,origins,files,0,||self.control.check())?.map(ResearchProjection::Context);
                        }
                        answer
                    },
                    LogicalOperation::Definition(_)=>{
                        observer("query_definition_resolve");
                        let facade=self.source_queries(&self.control)?;
                        let facade=if let Some(graph)=graph {facade.with_execution(graph,QuerySelector::AllOccurrences,&self.control)?}else{facade};
                        let result=facade.definitions(&target.site)?;
                        let known=matches!(&result.selected,QueryFact::Known{..});
                        json!({"kind":"definition","state":if known{"known"}else{"partial"},"result":result})
                    },
                    LogicalOperation::Type(_) if target.material["kind"]=="api_definition"=>{
                        observer("query_type_api");
                        if let Some(api)=api {api.semantic_type(target,self.inputs.host.bindings(),&self.control,||self.inputs.charge(1))?}
                        else{json!({"kind":"type","state":"unavailable","executed":false,"reason":"authenticated_api_type_model_unavailable"})}
                    },
                    LogicalOperation::Type(_)=>{
                        observer("query_type_observations");
                        if let Some(graph)=graph {
                            let facade=Facade{graph};let mut observations=Vec::new();let mut omitted=0usize;
                            for fact in &graph.uses {self.inputs.charge(1)?;if fact.site==target.site&&fact.host_return_slot.is_none(){
                                if observations.len()<64 {observations.push(facade.type_view(fact));}else{omitted+=1;}
                            }}
                            for write in &graph.writes {self.inputs.charge(1)?;if write.site==target.site {
                                if observations.len()<64 {observations.push(match facade.value_type(&write.decision,0){Ok(kind)=>json!({"state":"known","type":kind,"write":write.id}),Err(reason)=>json!({"state":"unavailable","reason":reason,"write":write.id})});}else{omitted+=1;}
                            }}
                            if observations.is_empty(){json!({"kind":"type","state":"unavailable","reason":"target_not_observed_in_selected_execution"})}
                            else{let known=omitted==0&&observations.iter().all(|item|item["state"]=="known");json!({"kind":"type","state":if known{"known"}else{"partial"},"result":{"observations":observations,"observationsOmitted":omitted}})}
                        } else {json!({"kind":"type","state":"unavailable","reason":"no_execution_scope_for_selected_type"})}
                    },
                };
                let mut answer=ResearchOperationAnswer::new(answer,reference_analysis);
                observer("query_operation_answer");
                let executed=if matches!(operation,LogicalOperation::Type(target) if target.material["kind"]=="api_definition") {answer["executed"].as_bool().unwrap_or(false)}else{graph.is_some()};
                answer["executed"]=json!(executed);
                answer["operationIdentity"]=json!(digest(operation));
                answer["batch"]=json!(batch);answer["operation"]=json!(answers.len());
                performed.push(executed);
                answers.push(answer);
            }
        }
        self.control.check()?;
        observer("query_execution_identity");
        let (identity,owner)=plan.execution_identity();
        observer("query_operations_executed");
        Ok(ResearchScopeCompletion{identity,owner,answers,performed,projection})
    }

    // Projection costs include both reserve-checked query steps and direct
    // serialization charges. Their sum is safe to replay as steps ONLY when
    // every possible prefix is strictly below the original return reserve.
    // Near that boundary the original path must preserve charge kinds/order.
    fn research_replay_has_reserve(&self,steps:usize)->bool {
        steps==0||self.research_steps().checked_add(steps).and_then(|used|used.checked_add(QUERY_RETURN_RESERVE))
            .is_some_and(|used|used<self.inputs.corpus.limits.construction_steps)
    }

    /// Only the immediately completed operation can supply these rows. Exact
    /// borrowed inputs bind source, request/options, graph and XML maps; the plan
    /// still authenticates execution and the original control remains live.
    #[cfg(test)]
    pub(crate) fn project_research_relations(&self,plan:&crate::native_research::plan::ResearchPlan,query:&crate::native_research::QueryRequest,graph:Option<&Graph>,origins:&crate::native_research::XmlResearch,projection:Option<ResearchProjection<'_>>)->lycheedev_core::Result<Value> {
        self.project_research_relations_observed(plan,query,graph,origins,projection,&mut |_|{}).map(ResearchProjectionResult::into_value)
    }
    pub(crate) fn project_research_relations_observed(&self,plan:&crate::native_research::plan::ResearchPlan,query:&crate::native_research::QueryRequest,graph:Option<&Graph>,origins:&crate::native_research::XmlResearch,projection:Option<ResearchProjection<'_>>,observer:&mut impl FnMut(&'static str))->lycheedev_core::Result<ResearchProjectionResult> {
        self.validate_research_plan(plan,query)?;
        match projection {
            Some(ResearchProjection::Incoming(projection))=>{
                if query.direction!="incoming"||!projection.matches(self,query,graph,origins){return Err(input_error("relation projection belongs to another source, query or scope"));}
                if !self.research_replay_has_reserve(projection.steps()) {
                    drop(projection);
                    return self.research_relations_with_context_observed(&query.symbol,&query.direction,graph,origins,None,None,observer);
                }
                let facade=self.source_queries(&self.control)?;
                let _selected=graph.map(|graph|facade.with_execution(graph,QuerySelector::AllOccurrences,&self.control)).transpose()?;
                projection.finish(||self.control.check(),||facade.step())
            },
            Some(ResearchProjection::Context(projection))=>{
                if query.direction!="both"||!projection.matches(self,query,graph,origins){return Err(input_error("relation projection belongs to another source, query or scope"));}
                let files=projection.finish(||self.control.check(),||Ok(()))?;
                self.research_relations_with_context_observed(&query.symbol,&query.direction,graph,origins,None,Some(files),observer)
            },
            None=>self.research_relations_with_context_observed(&query.symbol,&query.direction,graph,origins,None,None,observer),
        }
    }

    pub(crate) fn research_relations(&self,symbol:&crate::index::Symbol,direction:&str,graph:Option<&Graph>,origins:&crate::native_research::XmlResearch)->lycheedev_core::Result<Value> {
        self.research_relations_with_context(symbol,direction,graph,origins,None,None)
    }
    fn research_relations_with_context(&self,symbol:&crate::index::Symbol,direction:&str,graph:Option<&Graph>,origins:&crate::native_research::XmlResearch,capture:Option<&mut Vec<FileProjectionRange>>,retained:Option<ContextFiles>)->lycheedev_core::Result<Value> {
        self.research_relations_with_context_observed(symbol,direction,graph,origins,capture,retained,&mut |_|{}).map(ResearchProjectionResult::into_value)
    }
    fn research_relations_with_context_observed(&self,symbol:&crate::index::Symbol,direction:&str,graph:Option<&Graph>,origins:&crate::native_research::XmlResearch,mut capture:Option<&mut Vec<FileProjectionRange>>,mut retained:Option<ContextFiles>,observer:&mut impl FnMut(&'static str))->lycheedev_core::Result<ResearchProjectionResult> {
        self.control.check()?;
        observer("query_relations_target_declarations");
        #[cfg(test)]
        RESEARCH_PROJECTION_PASSES.with(|count|count.set(count.get()+1));
        let mut target_files=std::collections::BTreeSet::new();
        let target=self.syntax.declarations.iter().filter(|decl|decl.name==symbol.name.rsplit(['.',':']).next().unwrap_or(&symbol.name)
            && origins.matches_location(self,symbol,&decl.site.public(&self.inputs)).unwrap_or(false)).map(|decl|{target_files.insert(decl.site.file);decl.id.as_str()}).collect::<std::collections::BTreeSet<_>>();
        observer("query_relations_target_values");
        let target_values=graph.map(|graph|self.research_target_values(graph,symbol,origins)).transpose()?.unwrap_or_default();
        let is_target=|decision:&Decision|matches!(decision,Decision::Bound(value) if target_values.contains(value));
        let facade=self.source_queries(&self.control)?;
        let query=graph.map(|graph|facade.with_execution(graph,QuerySelector::AllOccurrences,&self.control)).transpose()?;
        observer("query_relations_use_index");
        let mut uses=ResearchUses::default();
        let mut selected_files=std::collections::BTreeSet::new();
        if let Some(graph)=graph{for fact in &graph.uses{if fact.host_return_slot.is_none(){
            self.control.check()?;
            if is_target(&fact.decision){selected_files.insert(fact.site.file.as_str());}
            uses.push(fact);
        }}}
        observer("query_relations_use_sort");
        uses.finish(||self.control.check())?;
        let mut relations=Vec::new();let mut unresolved=0usize;
        observer("query_relations_source_rows");
        for file in 0..self.inputs.corpus.files.len() {
            self.control.check()?;
            let path=self.inputs.corpus.files[file].path.as_str();
            let outgoing_file=direction!="incoming"&&origins.may_contain_outgoing(symbol,path);
            // Every incoming predicate below is covered: a lexical reference
            // uses a file-local declaration, selected abstract values carry an
            // observed site, and candidate/uncertain references require the
            // exact original spelling. Generated XML Lua is admitted here too.
            // This only excludes impossible files; rows, charges, ordering,
            // XML references/copies and complete-corpus counts stay unchanged.
            let incoming_file=direction!="outgoing"&&(target_files.contains(&file)
                ||selected_files.contains(path)||self.inputs.original(path)?.contains(&symbol.name));
            self.control.check()?;
            if !incoming_file&&!outgoing_file{continue;}
            if let Some(cached)=retained.as_mut().and_then(|files|files.take(file)) {
                // may_contain_outgoing is exactly the disjunction used by
                // within: original source path OR inherited XML object owner.
                // The same immutable query/origins/file ID was bound above.
                if outgoing_file{return Err(input_error("retained context file gained outgoing scope"));}
                if self.research_replay_has_reserve(cached.steps()) {
                    cached.append(&mut relations,&mut unresolved,||self.control.check(),||facade.step())?;
                    continue;
                }
                // No cached charge has run. Reconstruct this exact file below
                // so mixed step/direct charges keep their original sequence.
                drop(cached);
            }
            let first=relations.len();let prior_unresolved=unresolved;let prior_steps=self.research_steps();
            observer("query_relations_file_syntax");
            let syntax=public_result(self.file_syntax(file),&self.control)?;
            observer("query_relations_reference_rows");
            for reference in &syntax.references {
            self.control.check()?;
            let site=reference.site.public(&self.inputs);
            let lexical_incoming=reference.declaration.is_some_and(|id|target.contains(self.syntax.declarations[id].id.as_str()));
            let spelling=site.text(&self.inputs)?;
            let candidate_incoming=reference.declaration.is_none()&&spelling==symbol.name;
            let outgoing=direction!="incoming"&&origins.within(self,symbol,&site)?;
            let mut observations=Vec::new();
            let observed=uses.get(&site);
            let mut selected_incoming=false;let mut uncertain_incoming=false;
            if let (Some(query),Some(facts))=(query.as_ref(),observed) {
                for fact in facts {
                    let selected_target=is_target(&fact.decision);
                    let uncertain=candidate_incoming&&matches!(fact.decision,Decision::Unavailable(_)|Decision::NotManaged);
                    selected_incoming|=selected_target;uncertain_incoming|=uncertain;
                    if outgoing||direction!="outgoing"&&(selected_target||lexical_incoming||uncertain){observations.push(json!({"context":fact.context,"occurrence":query.occurrence(&fact.context),"selected":query.selected(&fact.decision,&fact.writes,&fact.proof_id),"proof":fact.proof_id}));}
                }
            }
            let incoming=direction!="outgoing"&&(lexical_incoming||selected_incoming||uncertain_incoming||candidate_incoming&&observed.is_none());
            if !incoming&&!outgoing{continue}
            let definition=facade.definitions(&site)?;
            // Lexical identity resolves a reference to storage, but does not
            // prove that a call still selects that declaration's original value.
            let proved=(!reference.roles.contains(&ReferenceRole::Call)&&matches!(&definition.lexical,QueryFact::Known{..}))
                ||!observations.is_empty()&&observations.iter().all(|row|row["selected"]["state"]=="known");
            if !proved{unresolved+=1;}
            let line=self.original_line_at(&site.file,site.start as usize)?;
            let kind=if reference.roles.contains(&ReferenceRole::Call){"call"}else if reference.roles.contains(&ReferenceRole::Write){"write"}else if reference.roles.contains(&ReferenceRole::Capture){"capture"}else{"reference"};
            let mut row=json!({"source":if outgoing{symbol.name.as_str()}else{spelling},"target":if outgoing{spelling}else{symbol.name.as_str()},"kind":kind,"state":if proved{"resolved"}else{"static_candidate"},"path":site.file,"line":line,"site":site,"basis":["native_common_source_facts"],"lexical":definition.lexical,"observations":observations,"proof":reference.id,"reason":if proved{"lexical declaration or selected abstract value"}else{"source candidate; execution binding unavailable"}});
            if !outgoing&&kind=="call" {self.project_incoming_caller(&mut row,&syntax,reference,origins)?;}
            origins.project_row(self,&mut row,&site)?;relations.push(row);
            }
            observer("query_relations_file_complete");
            if let Some(capture)=capture.as_mut()&& !origins.may_contain_outgoing(symbol,path) {
                capture.push(FileProjectionRange{file,first,end:relations.len(),steps:self.research_steps()-prior_steps,unresolved:unresolved-prior_unresolved});
            }
        }
        if retained.as_ref().is_some_and(|files|!files.is_empty()) {return Err(input_error("retained context files differ from current traversal"));}
        observer("query_relations_xml_rows");
        // XML declared references and mixin copies have additional owner rules;
        // always retain their original traversal, observations and ordering.
        if let (Some(graph),Some(query))=(graph,query.as_ref())&&let Some(xml)=&graph.xml{
            for reference in &xml.references{
                self.control.check()?;let fact=&graph.uses[reference.use_fact];
                let selected_target=is_target(&fact.decision);
                let uncertain=reference.name==symbol.name&&matches!(fact.decision,Decision::Unavailable(_)|Decision::NotManaged);
                let outgoing=direction!="incoming"&&(origins.within(self,symbol,&fact.site)?||xml.handlers.iter().any(|handler|handler.selected_use==Some(fact.id)&&xml.objects.iter().any(|object|object.instance==handler.instance&&object.name.as_deref()==Some(symbol.name.as_str())&&origins.matches_origin(symbol,&object.origin))));
                if !outgoing&&(direction=="outgoing"||!selected_target&&!uncertain){continue;}
                let selected=query.selected(&fact.decision,&fact.writes,&fact.proof_id);
                let proved=matches!(selected,QueryFact::Known{..});if !proved{unresolved+=1;}
                let mut row=json!({"source":if outgoing{symbol.name.as_str()}else{reference.name.as_str()},"target":if outgoing{reference.name.as_str()}else{symbol.name.as_str()},"kind":reference.role,"state":if proved{"resolved"}else{"static_candidate"},"basis":["native_common_source_facts","xml_declared_reference_observation"],"observations":[{"context":fact.context,"occurrence":query.occurrence(&fact.context),"selected":selected,"proof":fact.proof_id}],"xmlOrigin":reference.origin,"proof":fact.proof_id});
                origins.project_row(self,&mut row,&fact.site)?;relations.push(row);
            }
            for copy in &xml.copies{
                if direction=="outgoing"||!is_target(&copy.selected_value){continue;}
                let fact=&graph.uses[copy.field_use];let mut row=json!({"source":copy.field,"target":symbol.name,"kind":"xml_mixin_copy","state":"resolved","basis":["native_common_source_facts"],"copy":copy,"observations":[{"context":fact.context,"occurrence":query.occurrence(&fact.context),"selected":query.selected(&fact.decision,&fact.writes,&fact.proof_id),"proof":fact.proof_id}]});
                origins.project_row(self,&mut row,&xml_site(&copy.origin))?;relations.push(row);
            }
        }
        observer("query_relations_control_json");
        let control=graph.and_then(|graph|graph.native_invocations.as_ref()).map(|facts|&facts.control);
        let analysis=json!({"state":if unresolved==0&&control.is_some_and(|control|control.complete){"complete"}else{"partial"},"engine":"native_common_facts","lexicalDeclarations":self.syntax.declarations.len(),"sourceReferences":self.syntax.reference_count,"sourceCalls":self.syntax.call_count,"unresolvedRelations":unresolved,"sourceFiles":self.inputs.corpus.files.len(),"inputIdentity":self.inputs.identity_sha256(),"sourceExecution":false});
        let graph_control=graph.map(Graph::research_control).unwrap_or_else(||Arc::new(Value::Null));
        observer("query_relations_assemble");
        Ok(ResearchProjectionResult{relations,analysis:Arc::new(ResearchAnalysis::new(analysis,graph_control))})
    }

    // The graph and selected symbol are immutable for this projection. Resolve
    // each function/binding/write identity once, then retain every observation.
    fn research_target_values(&self,graph:&Graph,symbol:&crate::index::Symbol,origins:&crate::native_research::XmlResearch)->lycheedev_core::Result<std::collections::BTreeSet<usize>> {
        let mut functions=std::collections::BTreeSet::new();
        for (id,function) in graph.functions.iter().enumerate(){
            self.control.check()?;
            if origins.matches_function(self,symbol,&function.declaration).unwrap_or(false){functions.insert(id);}
        }
        let mut bindings=std::collections::BTreeSet::new();
        if let Some(host)=&graph.host_runtime{for (id,binding) in host.bindings.iter().enumerate(){
            self.control.check()?;
            if binding.declaration.name()==symbol.name&&binding.declaration.descriptor().is_some_and(|descriptor|descriptor["path"]==symbol.path&&descriptor["line"].as_u64().is_some_and(|line|line>=symbol.line as u64&&line<=symbol.end_line.max(symbol.line)as u64)){bindings.insert(id);}
        }}
        let mut objects=std::collections::BTreeSet::new();
        if let Some(xml)=&graph.xml{for object in &xml.objects{
            self.control.check()?;
            if object.name.as_deref()==Some(symbol.name.as_str())&&origins.matches_origin(symbol,&object.origin){objects.insert(object.value);}
        }}
        for write in &graph.writes{
            self.control.check()?;
            if let Decision::Bound(value)=write.decision&&matches!(graph.values[value].kind,ValueKind::Object(_))
                &&origins.matches_function(self,symbol,&write.site).unwrap_or(false){objects.insert(value);}
        }
        let mut targets=std::collections::BTreeSet::new();
        for (id,value) in graph.values.iter().enumerate(){
            self.control.check()?;
            let selected=match value.kind{ValueKind::Function(function)=>functions.contains(&function),ValueKind::HostCallable(binding)=>bindings.contains(&binding),ValueKind::Object(_)=>objects.contains(&id),_=>false};
            if selected{targets.insert(id);}
        }
        Ok(targets)
    }
}

#[cfg(test)]
impl PreparedSnapshot {
    pub(crate) fn eager_research_relations(&self,symbol:&crate::index::Symbol,direction:&str,graph:Option<&Graph>,origins:&crate::native_research::XmlResearch)->lycheedev_core::Result<Value> {
        self.control.check()?;
        let target=self.syntax.declarations.iter().filter(|decl|decl.name==symbol.name.rsplit(['.',':']).next().unwrap_or(&symbol.name)
            && origins.eager_matches_location(self,symbol,&decl.site.public(&self.inputs)).unwrap_or(false)).map(|decl|decl.id.as_str()).collect::<std::collections::BTreeSet<_>>();
        let is_target=|graph:&Graph,decision:&Decision|->bool{
            let Decision::Bound(value)=decision else{return false};
            match graph.values[*value].kind{
                ValueKind::Function(function)=>origins.eager_matches_function(self,symbol,&graph.functions[function].declaration).unwrap_or(false),
                ValueKind::HostCallable(binding)=>graph.host_runtime.as_ref().and_then(|host|host.bindings.get(binding)).is_some_and(|binding|binding.declaration.name()==symbol.name&&binding.declaration.descriptor().is_some_and(|descriptor|descriptor["path"]==symbol.path&&descriptor["line"].as_u64().is_some_and(|line|line>=symbol.line as u64&&line<=symbol.end_line.max(symbol.line)as u64))),
                ValueKind::Object(_)=>graph.xml.as_ref().is_some_and(|xml|xml.objects.iter().any(|object|object.value==*value&&object.name.as_deref()==Some(symbol.name.as_str())&&origins.matches_origin(symbol,&object.origin)))||graph.writes.iter().any(|write|matches!(write.decision,Decision::Bound(id) if id==*value)&&origins.eager_matches_function(self,symbol,&write.site).unwrap_or(false)),
                _=>false,
            }
        };
        let facade=self.source_queries(&self.control)?;
        let query=graph.map(|graph|facade.with_execution(graph,QuerySelector::AllOccurrences,&self.control)).transpose()?;
        let mut uses=BTreeMap::<String,Vec<&UseFact>>::new();
        if let Some(graph)=graph{for fact in &graph.uses{if fact.host_return_slot.is_none(){uses.entry(fact.site.id()).or_default().push(fact);}}}
        let mut relations=Vec::new();let mut unresolved=0usize;
        for file in 0..self.inputs.corpus.files.len() {
            let syntax=public_result(self.file_syntax(file),&self.control)?;
            for reference in &syntax.references {
            self.control.check()?;
            let site=reference.site.public(&self.inputs);
            let lexical_incoming=reference.declaration.is_some_and(|id|target.contains(self.syntax.declarations[id].id.as_str()));
            let spelling=site.text(&self.inputs)?;
            let candidate_incoming=reference.declaration.is_none()&&spelling==symbol.name;
            let outgoing=direction!="incoming"&&origins.eager_within(self,symbol,&site)?;
            let mut observations=Vec::new();
            let observed=uses.get(&site.id());
            let mut selected_incoming=false;let mut uncertain_incoming=false;
            if let (Some(graph),Some(query),Some(facts))=(graph,query.as_ref(),observed) {
                for fact in facts {
                    let selected_target=is_target(graph,&fact.decision);
                    let uncertain=candidate_incoming&&matches!(fact.decision,Decision::Unavailable(_)|Decision::NotManaged);
                    selected_incoming|=selected_target;uncertain_incoming|=uncertain;
                    if outgoing||direction!="outgoing"&&(selected_target||lexical_incoming||uncertain){observations.push(json!({"context":fact.context,"occurrence":query.occurrence(&fact.context),"selected":query.selected(&fact.decision,&fact.writes,&fact.proof_id),"proof":fact.proof_id}));}
                }
            }
            let incoming=direction!="outgoing"&&(lexical_incoming||selected_incoming||uncertain_incoming||candidate_incoming&&observed.is_none());
            if !incoming&&!outgoing{continue}
            let definition=facade.definitions(&site)?;
            // Lexical identity resolves a reference to storage, but does not
            // prove that a call still selects that declaration's original value.
            let proved=(!reference.roles.contains(&ReferenceRole::Call)&&matches!(&definition.lexical,QueryFact::Known{..}))
                ||!observations.is_empty()&&observations.iter().all(|row|row["selected"]["state"]=="known");
            if !proved{unresolved+=1;}
            let line=self.original_line_at(&site.file,site.start as usize)?;
            let kind=if reference.roles.contains(&ReferenceRole::Call){"call"}else if reference.roles.contains(&ReferenceRole::Write){"write"}else if reference.roles.contains(&ReferenceRole::Capture){"capture"}else{"reference"};
            let mut row=json!({"source":if outgoing{symbol.name.as_str()}else{spelling},"target":if outgoing{spelling}else{symbol.name.as_str()},"kind":kind,"state":if proved{"resolved"}else{"static_candidate"},"path":site.file,"line":line,"site":site,"basis":["native_common_source_facts"],"lexical":definition.lexical,"observations":observations,"proof":reference.id,"reason":if proved{"lexical declaration or selected abstract value"}else{"source candidate; execution binding unavailable"}});
            origins.project_row(self,&mut row,&site)?;relations.push(row);
            }
        }
        if let (Some(graph),Some(query))=(graph,query.as_ref())&&let Some(xml)=&graph.xml{
            for reference in &xml.references{
                self.control.check()?;let fact=&graph.uses[reference.use_fact];
                let selected_target=is_target(graph,&fact.decision);
                let uncertain=reference.name==symbol.name&&matches!(fact.decision,Decision::Unavailable(_)|Decision::NotManaged);
                let outgoing=direction!="incoming"&&(origins.eager_within(self,symbol,&fact.site)?||xml.handlers.iter().any(|handler|handler.selected_use==Some(fact.id)&&xml.objects.iter().any(|object|object.instance==handler.instance&&object.name.as_deref()==Some(symbol.name.as_str())&&origins.matches_origin(symbol,&object.origin))));
                if !outgoing&&(direction=="outgoing"||!selected_target&&!uncertain){continue;}
                let selected=query.selected(&fact.decision,&fact.writes,&fact.proof_id);
                let proved=matches!(selected,QueryFact::Known{..});if !proved{unresolved+=1;}
                let mut row=json!({"source":if outgoing{symbol.name.as_str()}else{reference.name.as_str()},"target":if outgoing{reference.name.as_str()}else{symbol.name.as_str()},"kind":reference.role,"state":if proved{"resolved"}else{"static_candidate"},"basis":["native_common_source_facts","xml_declared_reference_observation"],"observations":[{"context":fact.context,"occurrence":query.occurrence(&fact.context),"selected":selected,"proof":fact.proof_id}],"xmlOrigin":reference.origin,"proof":fact.proof_id});
                origins.project_row(self,&mut row,&fact.site)?;relations.push(row);
            }
            for copy in &xml.copies{
                if direction=="outgoing"||!is_target(graph,&copy.selected_value){continue;}
                let fact=&graph.uses[copy.field_use];let mut row=json!({"source":copy.field,"target":symbol.name,"kind":"xml_mixin_copy","state":"resolved","basis":["native_common_source_facts"],"copy":copy,"observations":[{"context":fact.context,"occurrence":query.occurrence(&fact.context),"selected":query.selected(&fact.decision,&fact.writes,&fact.proof_id),"proof":fact.proof_id}]});
                origins.project_row(self,&mut row,&xml_site(&copy.origin))?;relations.push(row);
            }
        }
        let control=graph.and_then(|graph|graph.native_invocations.as_ref()).map(|facts|&facts.control);
        Ok(json!({"relations":relations,"analysis":{"state":if unresolved==0&&control.is_some_and(|control|control.complete){"complete"}else{"partial"},"engine":"native_common_facts","lexicalDeclarations":self.syntax.declarations.len(),"sourceReferences":self.syntax.reference_count,"sourceCalls":self.syntax.call_count,"unresolvedRelations":unresolved,"graphControl":control,"sourceFiles":self.inputs.corpus.files.len(),"inputIdentity":self.inputs.identity_sha256(),"sourceExecution":false}}))
    }

}

/// Constructed only by the native operation executor. JSON projections alone
/// cannot mint execution credit or change the bound request after execution.
pub(crate) struct ResearchScopeCompletion<'a> {
    identity:String, owner:Arc<()>, answers:Vec<ResearchOperationAnswer>, performed:Vec<bool>, projection:Option<ResearchProjection<'a>>,
}
impl<'a> ResearchScopeCompletion<'a> {
    pub(crate) fn consume(self,plan:&crate::native_research::plan::ResearchPlan)->lycheedev_core::Result<(Vec<ResearchOperationAnswer>,Vec<bool>,Option<ResearchProjection<'a>>)> {
        use crate::native_research::plan::{LogicalOperation,BATCH_QUERIES};
        plan.validate_execution_identity(&self.identity,&self.owner)?;
        if self.answers.len()!=plan.operations().len()||self.performed.len()!=self.answers.len(){return Err(input_error("semantic completion operation count differs"));}
        for (index,((answer,performed),operation)) in self.answers.iter().zip(&self.performed).zip(plan.operations()).enumerate(){
            let kind=match operation{LogicalOperation::References(_)=>"references",LogicalOperation::Type(_)=>"type",LogicalOperation::Definition(_)=>"definition"};
            if answer["operation"]!=index||answer["batch"]!=index/BATCH_QUERIES||answer["kind"]!=kind||answer["operationIdentity"]!=digest(operation)||answer["executed"]!=*performed {
                return Err(input_error("semantic completion operation identity or order differs"));
            }
        }
        Ok((self.answers,self.performed,self.projection))
    }
}

#[cfg(test)]
mod research_completion_identity_tests {
    use super::*;
    use crate::native_research::plan::{AuthenticatedSite,ResearchPlan};
    fn plan(owner:Arc<()>,query:&str)->ResearchPlan {
        let mut plan=ResearchPlan::new("source",&query,owner);
        for n in 0..2 {plan.definition(AuthenticatedSite{site:Site{file:"a.lua".into(),start:n,end:n+1},file_sha256:"a".repeat(64),position:Value::Null,material:json!({"kind":"source"})});}plan.finish()
    }
    fn completion(plan:&ResearchPlan)->ResearchScopeCompletion<'static> {
        let(identity,owner)=plan.execution_identity();
        let answers=plan.operations().iter().enumerate().map(|(index,operation)|json!({"kind":"definition","state":"unavailable","executed":true,"operation":index,"batch":0,"operationIdentity":digest(operation)}).into()).collect();
        ResearchScopeCompletion{identity,owner,answers,performed:vec![true;2],projection:None}
    }
    #[test]
    fn completion_rejects_duplicate_reordered_missing_and_foreign_answers() {
        let owner=Arc::new(());let plan=plan(owner.clone(),"query");
        let mut duplicate=completion(&plan);duplicate.answers[1]=duplicate.answers[0].clone();assert!(duplicate.consume(&plan).is_err());
        let mut swapped=completion(&plan);swapped.answers.swap(0,1);assert!(swapped.consume(&plan).is_err());
        let mut missing=completion(&plan);missing.answers.pop();assert!(missing.consume(&plan).is_err());
        let mut wrong_kind=completion(&plan);wrong_kind.answers[0]["kind"]=json!("type");assert!(wrong_kind.consume(&plan).is_err());
        let mut wrong_batch=completion(&plan);wrong_batch.answers[0]["batch"]=json!(1);assert!(wrong_batch.consume(&plan).is_err());
        let other= self::plan(owner,"other query");assert!(completion(&plan).consume(&other).is_err());
        let renewed=self::plan(Arc::new(()),"query");assert!(completion(&plan).consume(&renewed).is_err());
        assert!(completion(&plan).consume(&plan).is_ok());
    }
}

#[cfg(test)]
include!("research_full_scan_reference.rs");
