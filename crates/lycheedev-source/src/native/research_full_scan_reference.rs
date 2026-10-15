// Exact pre-filter projection body, test-only oracle. No production consumer.
impl PreparedSnapshot {
    pub(crate) fn full_scan_research_relations(&self,symbol:&crate::index::Symbol,direction:&str,graph:Option<&Graph>,origins:&crate::native_research::XmlResearch)->lycheedev_core::Result<Value> {
        self.control.check()?;
        #[cfg(test)]
        RESEARCH_PROJECTION_PASSES.with(|count|count.set(count.get()+1));
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
            let line=self.original_line_at(&site.file,site.start as usize)?;
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

}
