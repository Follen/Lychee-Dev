// Frozen whole-Vec-copy implementations before scope-copy removal.
// Included only by sharing_tests; never a production evaluator.
// host_finish_facts SHA256: a585489420b0e672b5d00f38ad108abf7263a323005aa982d61ffd12e36d19f2
// xml_load SHA256: 932d653bae8af404fdaefeb792393126b96247479b82bc0548e993966be70140
impl Builder {
    fn host_finish_facts_copying_reference(&mut self)->Result<()> {
        if self.graph.host_runtime.is_none(){return Ok(())}
        for fact in self.graph.uses.clone(){
            if !fact.writes.is_empty(){self.host_transfer("storage_read",vec![],fact.writes,TransferDestination::Use{use_fact:fact.id},fact.decision,vec![])?;}
        }
        for write in self.graph.writes.clone(){
            let conditions=write.conditional_branch.map(|branch|self.graph.branches[branch].condition_facts.clone()).unwrap_or_default();
            self.host_transfer("storage_write",write.inputs,vec![],TransferDestination::Write{write:write.id},write.decision,conditions)?;
        }
        for join in self.native.facts.control.write_joins.clone(){
            let sources=join.candidates.iter().filter_map(|candidate|candidate.write).collect();
            self.host_transfer("conditional_write_candidates",vec![],sources,TransferDestination::Write{write:join.destination},self.graph.writes[join.destination].decision.clone(),self.graph.branches[join.branch].condition_facts.clone())?;
        }
        for call in self.graph.host_runtime.as_ref().expect("host facts").calls.clone(){
            for slot in call.arguments{self.host_transfer("argument_slot",vec![slot.use_fact],slot.writes,TransferDestination::Argument{call:call.call,slot:slot.slot},slot.selected,vec![])?;}
            for slot in &call.returns{self.host_transfer("return_slot",vec![slot.use_fact],slot.writes.clone(),TransferDestination::Return{call:call.call,slot:slot.slot},slot.selected.clone(),vec![])?;}
            self.host_transfer("scalar_adjustment",call.returns.first().map(|slot|slot.use_fact).into_iter().collect(),vec![],TransferDestination::ScalarResult{call:call.call},self.graph.uses[call.scalar_result].decision.clone(),vec![])?;
            self.host_transfer("scalar_result_use",call.returns.first().map(|slot|slot.use_fact).into_iter().collect(),vec![],TransferDestination::Use{use_fact:call.scalar_result},self.graph.uses[call.scalar_result].decision.clone(),vec![])?;
        }
        for capture in self.native.facts.capture_facts.clone(){
            self.budget()?;
            let owner_value=self.native.facts.closures[capture.closure].value;
            let proof_id=digest(&(self.graph.snapshot_key.as_str(),"structural_capture_reference",capture.closure,&capture.cell,&capture.declaration,&capture.creation_context,owner_value));
            let facts=self.graph.host_runtime.as_mut().expect("host facts");
            if !self.inputs.corpus.limits.research&&facts.capture_references.len()>=INVOCATION_CAPTURE_EDGE_LIMIT{return Err("host capture reference budget exceeded".into())}
            facts.capture_references.push(CommonCaptureReference{closure:capture.closure,cell:capture.cell,declaration:capture.declaration,creation_context:capture.creation_context,owner_value,proof_id});
        }
        let facts=self.graph.host_runtime.as_mut().expect("host facts");facts.calls.sort_by_key(|call|call.call);facts.open_tail_uses=self.native.host.open_tail_uses.clone();facts.cardinality_uses=self.native.host.cardinality_uses.clone();Ok(())
    }
    fn xml_load_copying_reference(&mut self,program:&crate::xml_load::Program)->Result<()>{
        self.graph.xml=Some(XmlRuntimeFacts{gaps:program.gaps.clone(),..Default::default()});
        let mut occurrence=0;
        for (action_id,action) in program.actions().iter().enumerate(){
            self.budget()?;
            match action{
                crate::xml_load::Action::Lua{file}=>{
                    self.load_file(file,occurrence)?;occurrence+=1;
                    if !self.native.facts.control.complete{return Ok(());}
                },
                crate::xml_load::Action::Object{instance,origin,tag,name,parent}=>{
                    let site=xml_site(origin);let ctx=self.xml_context(action_id,&site);let object=self.allocate_object(&site)?;
                    // Native members remain open; subsequently copied or written Lua
                    // fields have their own normal write/observation versions.
                    self.unknown_object(object,"xml_native_members_unavailable".into(),site.clone());
                    let value=self.graph.values.len();self.graph.values.push(AbstractValue{id:value,origin:ValueOrigin::Xml{xml:origin.clone()},kind:ValueKind::Object(object)});
                    self.xml_objects.insert(instance.clone(),(object,value));
                    if let Some(name)=name{let cell=format!("global:{name}");self.native.typed_cells.borrow_mut().insert(cell.clone(),StorageOwner::Global{name:name.clone()});self.write(cell,site,&ctx,Decision::Bound(value),vec![]);}
                    self.graph.xml.as_mut().expect("XML facts").objects.push(XmlObjectFact{instance:instance.clone(),object,value,origin:origin.clone(),tag:tag.clone(),name:name.clone(),parent:parent.clone()});
                },
                crate::xml_load::Action::Mixin{instance,reference}=>{let ctx=self.xml_context(action_id,&xml_site(&reference.origin));self.xml_mixin(instance,reference,&ctx)?;},
                crate::xml_load::Action::ParentField{parent,parent_reference,child,reference}=>{
                    let site=xml_site(&reference.origin);let ctx=self.xml_context(action_id,&site);
                    let mut inputs=vec![];
                    let parent=if let Some(reference)=parent_reference{let fact=self.xml_reference(reference,None,&ctx,"parent")?;inputs.push(fact);self.object(&self.graph.uses[fact].decision)}else{parent.as_ref().and_then(|parent|self.xml_objects.get(parent).map(|(object,_)|*object))};
                    if let Some(parent)=parent{self.put_field(parent,reference.name.clone(),site,&ctx,Decision::Bound(self.xml_objects[child].1),inputs);}
                    else{self.graph.xml.as_mut().expect("XML facts").gaps.push(json!({"code":"xml_parent_key_receiver_unavailable","reference":reference}));}
                },
                crate::xml_load::Action::Handler{instance,handler,target,origin}=>{
                    let (object,self_value)=self.xml_objects[instance];
                    let selected=if let crate::xml_load::HandlerTarget::Inline{file}=target{
                        self.native.facts.closures.iter().rev().find(|closure|closure.load_occurrence==occurrence.saturating_sub(1)&&self.graph.functions[closure.function].closure.file==*file)
                            .map(|closure|Decision::Bound(closure.value)).unwrap_or_else(||Decision::Unavailable("xml_inline_closure_unavailable".into()))
                    }else{Decision::Unavailable("xml_handler_not_observed".into())};
                    let facts=self.graph.xml.as_mut().expect("XML facts");let id=facts.handlers.len();
                    facts.handlers.push(XmlHandlerFact{id,action:action_id,instance:instance.clone(),object,self_value,handler:handler.clone(),target:target.clone(),origin:origin.clone(),selected,selected_use:None,closure:None,resolution_phase:"hypothetical_after_load"});
                },
                crate::xml_load::Action::Definition{..}=>{},
            }
        }
        let handlers=self.graph.xml.as_ref().expect("XML facts").handlers.clone();
        for handler in handlers{
            let ctx=self.xml_context(handler.action,&xml_site(&handler.origin));
            let fact=match &handler.target{
                crate::xml_load::HandlerTarget::Method{reference}=>Some(self.xml_reference(reference,Some(handler.object),&ctx,"handler_method")?),
                crate::xml_load::HandlerTarget::Function{reference}=>Some(self.xml_reference(reference,None,&ctx,"handler_function")?),
                _=>None,
            };
            let selected=fact.map(|fact|self.graph.uses[fact].decision.clone()).unwrap_or(handler.selected);
            let closure=match selected{Decision::Bound(value)=>self.native.closure_values.get(&value).copied(),_=>None};
            let destination=&mut self.graph.xml.as_mut().expect("XML facts").handlers[handler.id];destination.selected=selected;destination.selected_use=fact;destination.closure=closure;
        }
        Ok(())
    }
}
