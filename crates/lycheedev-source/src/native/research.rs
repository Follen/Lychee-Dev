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
        if query.direction!="outgoing" && query.symbol.category.starts_with("api-") {
            if let Some(target)=api.map(|api|api.target(&query.symbol)).transpose()?.flatten() {plan.incoming(target);}
            else {plan.unavailable("incoming_unique_api_definition_mapping_unavailable");}
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
                if inline.is_some() {
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
                        if edge.from!="<file>" {continue;}
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
                    if !origins.within(self,&query.symbol,&site)? {continue;}
                    plan.definition(self.research_authenticated_site(site,origins)?);
                }
            }
        }
        self.control.check()?;
        Ok(plan.finish())
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
    pub(crate) fn research_steps(&self)->usize {
        self.inputs.shared_steps.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn research_relations(&self,symbol:&crate::index::Symbol,direction:&str,graph:Option<&Graph>,origins:&crate::native_research::XmlResearch)->lycheedev_core::Result<Value> {
        self.control.check()?;
        let target=self.syntax.declarations.iter().filter(|decl|decl.name==symbol.name.rsplit(['.',':']).next().unwrap_or(&symbol.name)
            && origins.matches_location(self,symbol,&decl.site.public(&self.inputs)).unwrap_or(false)).map(|decl|decl.id.as_str()).collect::<std::collections::BTreeSet<_>>();
        let target_values=graph.map(|graph|self.research_target_values(graph,symbol,origins)).transpose()?.unwrap_or_default();
        let is_target=|decision:&Decision|matches!(decision,Decision::Bound(value) if target_values.contains(value));
        let facade=self.source_queries(&self.control)?;
        let query=graph.map(|graph|facade.with_execution(graph,QuerySelector::AllOccurrences,&self.control)).transpose()?;
        let mut uses=BTreeMap::<(&str,u32,u32),Vec<&UseFact>>::new();
        if let Some(graph)=graph{for fact in &graph.uses{if fact.host_return_slot.is_none(){uses.entry((fact.site.file.as_str(),fact.site.start,fact.site.end)).or_default().push(fact);}}}
        let mut relations=Vec::new();let mut unresolved=0usize;
        for file in 0..self.inputs.corpus.files.len() {
            self.control.check()?;
            if direction=="outgoing"&&!origins.may_contain_outgoing(symbol,&self.inputs.corpus.files[file].path){continue;}
            let syntax=public_result(self.file_syntax(file),&self.control)?;
            for reference in &syntax.references {
            self.control.check()?;
            let site=reference.site.public(&self.inputs);
            let lexical_incoming=reference.declaration.is_some_and(|id|target.contains(self.syntax.declarations[id].id.as_str()));
            let spelling=site.text(&self.inputs)?;
            let candidate_incoming=reference.declaration.is_none()&&spelling==symbol.name;
            let outgoing=direction!="incoming"&&origins.within(self,symbol,&site)?;
            let mut observations=Vec::new();
            let observed=uses.get(&(site.file.as_str(),site.start,site.end));
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
            let line=original_line_at(self.inputs.original(&site.file)?,site.start as usize);
            let kind=if reference.roles.contains(&ReferenceRole::Call){"call"}else if reference.roles.contains(&ReferenceRole::Write){"write"}else if reference.roles.contains(&ReferenceRole::Capture){"capture"}else{"reference"};
            let mut row=json!({"source":if outgoing{symbol.name.as_str()}else{spelling},"target":if outgoing{spelling}else{symbol.name.as_str()},"kind":kind,"state":if proved{"resolved"}else{"static_candidate"},"path":site.file,"line":line,"site":site,"basis":["native_common_source_facts"],"lexical":definition.lexical,"observations":observations,"proof":reference.id,"reason":if proved{"lexical declaration or selected abstract value"}else{"source candidate; execution binding unavailable"}});
            origins.project_row(self,&mut row,&site)?;relations.push(row);
            }
        }
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
        let control=graph.and_then(|graph|graph.native_invocations.as_ref()).map(|facts|&facts.control);
        Ok(json!({"relations":relations,"analysis":{"state":if unresolved==0&&control.is_some_and(|control|control.complete){"complete"}else{"partial"},"engine":"native_common_facts","lexicalDeclarations":self.syntax.declarations.len(),"sourceReferences":self.syntax.reference_count,"sourceCalls":self.syntax.call_count,"unresolvedRelations":unresolved,"graphControl":control,"sourceFiles":self.inputs.corpus.files.len(),"inputIdentity":self.inputs.identity_sha256(),"sourceExecution":false}}))
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
            let line=original_line_at(self.inputs.original(&site.file)?,site.start as usize);
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
