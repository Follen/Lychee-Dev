// Host material joins the existing evaluator. It is never executed or projected
// as a synthetic Lua declaration. All indices below refer to the common graph.
const HOST_CALL_FACT_LIMIT:usize=256;
const HOST_TRANSFER_LIMIT:usize=4096;
pub const HOST_RUNTIME_RULES:&str="source-authenticated-host-common-projection-accounting-v3";
fn host_runtime_profile()->Value {json!({"rulesVersion":HOST_RUNTIME_RULES,"origins":"source_or_authenticated_environment","binding":"initial_environment_lookup_overridden_by_source_writes","callEffects":"authenticated_no_lua_heap_write_or_explicit_unknown","cardinality":"exact_bounded_path_alternatives_or_unknown_open_tail","returnCandidates":"source_uses_and_path_predicates_to_joined_return","captures":"structural_cell_references_not_value_snapshots","typing":"unavailable_without_native_source_descriptor","views":["ordered_load","source_template","source_invocation"],"explicitEntryCapability":ENTRY_RULES,"entryRequest":false,"guardFlow":true,"sourceExecution":false,"limits":{"calls":HOST_CALL_FACT_LIMIT,"transfers":HOST_TRANSFER_LIMIT,"captureReferences":INVOCATION_CAPTURE_EDGE_LIMIT,"slotsPerCall":INVOCATION_RETURN_LIMIT,"bindings":HOST_BINDING_LIMIT,"sharedOperationSteps":10000}})}

#[derive(Clone,Debug,Serialize)]
pub struct HostInitialValue { pub binding:usize, pub value:usize, pub declaration:HostBinding, pub proof_id:String }
#[derive(Clone,Debug,Serialize)]
pub struct HostNamespaceField {pub object:usize,pub field:String,pub value:usize,pub binding:usize}
#[derive(Clone,Debug,Serialize)]
pub struct HostNominalObject {pub value:usize,pub object:usize,pub binding:usize,pub slot:usize,pub type_name:String,pub proof_id:String}
#[derive(Clone,Debug,Serialize)]
pub struct HostReturnSlotIdentity { pub binding:Option<usize>, pub call:usize, pub slot:usize, pub presence:String }
#[derive(Clone,Debug,Serialize)]
pub struct CommonSlotFact { pub slot:usize, pub use_fact:usize, pub selected:Decision, pub writes:Vec<usize>, pub presence:String }
#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
pub struct OpenReturnTail {
    pub calls:Vec<usize>, pub first_unknown_slot:usize,
    // Analysis representation width only; never a Lua return-count assertion.
    pub tracked_slots:usize,
}
#[derive(Clone,Debug,Serialize)]
pub struct FiniteSlotCardinality { pub arities:Vec<usize>, pub tracked_slots:usize }
#[derive(Clone,Debug,Serialize)]
pub struct HostPredicateSubject { pub argument_slot:usize, pub subject:Option<CommonSlotFact>, pub truth:String }
#[derive(Clone,Debug,Serialize)]
pub struct CommonCallSlots {
    pub call:usize, pub context:String, pub view:NativeQueryView,
    pub host_binding:Option<usize>, pub callee_value:Option<usize>,
    pub receiver:Option<usize>, pub syntactic_arguments:Vec<Site>,
    pub arguments:Vec<CommonSlotFact>, pub argument_arities:Option<Vec<usize>>, pub argument_open_tail:Option<OpenReturnTail>,
    pub returns:Vec<CommonSlotFact>, pub return_arity:Option<usize>, pub return_arities:Option<Vec<usize>>, pub return_open_tail:Option<OpenReturnTail>,
    pub scalar_result:usize, pub predicate:Option<HostPredicateSubject>, pub proof_id:String,
}
#[derive(Clone,Debug,Serialize)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum TransferDestination { Use{use_fact:usize}, Write{write:usize}, Argument{call:usize,slot:usize}, Return{call:usize,slot:usize}, ScalarResult{call:usize} }
#[derive(Clone,Debug,Serialize)]
pub struct CommonTransferFact {
    pub id:usize, pub kind:String, pub source_uses:Vec<usize>, pub source_writes:Vec<usize>,
    pub destination:TransferDestination, pub selected:Decision, pub condition_facts:Vec<usize>, pub proof_id:String,
    #[serde(skip_serializing_if="Option::is_none")]
    pub return_candidates:Option<Vec<ConditionalReturnCandidate>>,
}
#[derive(Clone,Debug,Serialize)]
pub struct ConditionalReturnCandidate {
    pub id:String, pub observation:String, pub slot:usize,
    pub context:String, pub paths:Vec<Vec<NativePredicate>>, pub use_fact:Option<usize>,
    pub source_uses:Vec<usize>, pub selected:Decision, pub presence:String,
}
#[derive(Clone,Debug,Serialize)]
pub struct CommonCaptureReference {
    pub closure:usize, pub cell:String, pub declaration:Site, pub creation_context:String,
    // Identifies the closure owning the reference, never the cell's selected value.
    pub owner_value:usize, pub proof_id:String,
}
#[derive(Clone,Debug,Serialize)]
pub struct HostRuntimeFacts {
    pub namespace_fields:Vec<HostNamespaceField>,
    pub nominal_objects:Vec<HostNominalObject>,
    pub method_fields:Vec<HostNamespaceField>,
    pub expressions:Vec<CommonExpressionFact>,
    pub schema:String, pub profile:Value,
    pub bindings:Vec<HostInitialValue>, pub calls:Vec<CommonCallSlots>, pub transfers:Vec<CommonTransferFact>,
    pub capture_references:Vec<CommonCaptureReference>,
    pub open_tail_uses:BTreeMap<usize,OpenReturnTail>,
    pub cardinality_uses:BTreeMap<usize,FiniteSlotCardinality>,
}
#[derive(Default)]
struct HostRuntimeState { initial_globals:BTreeMap<String,usize>, initial_fields:BTreeMap<(usize,String),usize>, nominal_objects:BTreeMap<usize,usize>, methods:BTreeMap<String,Vec<(String,usize,usize)>>, open_tail_uses:BTreeMap<usize,OpenReturnTail>, cardinality_uses:BTreeMap<usize,FiniteSlotCardinality>, invocation_calls:usize, invocation_transfers:usize }

impl Builder {
    fn host_initialize(&mut self)->Result<()> {
        if self.inputs.host.data.bindings.is_empty()&&!self.inputs.host.data.bindings_admitted{return Ok(())}
        let mut facts=HostRuntimeFacts{namespace_fields:vec![],nominal_objects:vec![],method_fields:vec![],expressions:vec![],schema:"source-host-common-facts-v1".into(),profile:host_runtime_profile_for(self.inputs.entry.is_some(),self.inputs.corpus.limits.research),bindings:vec![],calls:vec![],transfers:vec![],capture_references:vec![],open_tail_uses:BTreeMap::new(),cardinality_uses:BTreeMap::new()};
        let mut namespaces=BTreeMap::<String,usize>::new();
        for (binding,declaration) in self.inputs.host.data.bindings.clone().into_iter().enumerate(){
            self.budget()?;
            let kind=if declaration.kind()=="empty_object" {
                if self.objects.len()>=self.object_limit(){return Err("host object identity budget exceeded".into())}
                // Initial environment state has no source mutation or source range.
                let object=self.objects.len();self.objects.push(ObjectState::default());ValueKind::Object(object)
            }else{ValueKind::HostCallable(binding)};
            let value=self.graph.values.len();
            self.graph.values.push(AbstractValue{id:value,origin:ValueOrigin::Environment{environment:declaration.declaration().clone()},kind});
            if let Some(receiver)=declaration.receiver_type(){
                let member=declaration.native_descriptor().expect("admitted method descriptor").declaration.name.clone();
                self.native.host.methods.entry(receiver.into()).or_default().push((member,value,binding));
                let proof_id=digest(&(self.graph.snapshot_key.as_str(),binding,value,&declaration));
                facts.bindings.push(HostInitialValue{binding,value,declaration,proof_id});
                continue;
            }
            let parts=declaration.name().split('.').collect::<Vec<_>>();
            let mut parent=None;let mut path=String::new();
            for part in &parts[..parts.len()-1]{
                if !path.is_empty(){path.push('.');}path.push_str(part);
                let object=if let Some(object)=namespaces.get(&path){*object}else{
                    if self.objects.len()>=self.object_limit(){return Err("host namespace object budget exceeded".into())}
                    let object=self.objects.len();self.objects.push(ObjectState{unknown:Some("environment_namespace_enumeration_unavailable".into()),..Default::default()});
                    let namespace_value=self.graph.values.len();
                    self.graph.values.push(AbstractValue{id:namespace_value,origin:ValueOrigin::Environment{environment:declaration.declaration().clone()},kind:ValueKind::Object(object)});
                    if let Some(parent)=parent{self.native.host.initial_fields.insert((parent,(*part).into()),namespace_value);facts.namespace_fields.push(HostNamespaceField{object:parent,field:(*part).into(),value:namespace_value,binding});}
                    else{self.native.host.initial_globals.insert(format!("global:{part}"),namespace_value);}
                    namespaces.insert(path.clone(),object);object
                };parent=Some(object);
            }
            if let Some(object)=parent{let field=parts.last().expect("host name").to_string();self.native.host.initial_fields.insert((object,field.clone()),value);facts.namespace_fields.push(HostNamespaceField{object,field,value,binding});}
            else{self.native.host.initial_globals.insert(format!("global:{}",declaration.name()),value);}
            let proof_id=digest(&(self.graph.snapshot_key.as_str(),binding,value,&declaration));
            facts.bindings.push(HostInitialValue{binding,value,declaration,proof_id});
        }
        // Unknown ambient names still use inputs.host.data.closed in read(). This flag
        // now marks an external call that may have rebound an admitted initial name.
        self.native.facts.global_environment_open=false;
        self.graph.host_runtime=Some(facts);Ok(())
    }
    fn host_binding(&self,selected:&Decision)->Option<usize> {
        match selected{Decision::Bound(value)=>match self.graph.values[*value].kind{ValueKind::HostCallable(binding)=>Some(binding),_=>None},_=>None}
    }
    fn host_tail(&self,values:&[usize])->Option<OpenReturnTail> {
        let last=values.last()?;let mut tail=self.native.host.open_tail_uses.get(last)?.clone();
        tail.first_unknown_slot+=values.len().saturating_sub(tail.tracked_slots);tail.tracked_slots=values.len();Some(tail)
    }
    fn host_list_slot(&self,values:&[usize],index:usize,missing:&str)->(Decision,Vec<usize>) {
        if let Some(fact)=values.get(index){return(self.graph.uses[*fact].decision.clone(),vec![*fact])}
        if let Some(tail)=self.host_tail(values){return(Decision::Unavailable(format!("open_return_tail_slot_{}_from_calls_{:?}",index+1,tail.calls)),values.last().copied().into_iter().collect())}
        (Decision::KnownAbsent(missing.into()),vec![])
    }
    fn host_arities(&self,values:&[usize])->Option<Vec<usize>> {
        if self.host_tail(values).is_some(){return None}
        if let Some(cardinality)=values.last().and_then(|fact|self.native.host.cardinality_uses.get(fact)){
            let offset=values.len().saturating_sub(cardinality.tracked_slots);
            return Some(cardinality.arities.iter().map(|arity|arity+offset).collect())
        }
        Some(vec![values.len()])
    }
    fn host_slot_presence(&self,values:&[usize],index:usize)->&'static str {
        if let Some(tail)=self.host_tail(values) && index>=tail.first_unknown_slot.saturating_sub(1){return "unknown"}
        if let Some(arities)=self.host_arities(values){
            if arities.iter().all(|arity|index>=*arity){return "absent"}
            if arities.iter().any(|arity|index>=*arity){return "unknown"}
        }
        "present"
    }
    fn host_join_return_facts(&mut self,paths:&[NativePath],output:&[usize])->Result<()> {
        if self.graph.host_runtime.is_none(){return Ok(())}
        let mut arities=std::collections::BTreeSet::new();let mut open_calls=std::collections::BTreeSet::new();let mut first_unknown_slot=output.len();
        for path in paths {
            self.budget()?;
            let values=match &path.outcome{NativeControl::Return(values)=>values.as_slice(),_=>&[]};
            if let Some(tail)=self.host_tail(values){first_unknown_slot=first_unknown_slot.min(tail.first_unknown_slot);open_calls.extend(tail.calls);}
            if let Some(counts)=self.host_arities(values){arities.extend(counts);}
        }
        if let Some(minimum)=arities.first(){first_unknown_slot=first_unknown_slot.min(minimum+1);}
        if let Some(fact)=output.last(){
            if !open_calls.is_empty(){self.native.host.open_tail_uses.insert(*fact,OpenReturnTail{calls:open_calls.into_iter().collect(),first_unknown_slot,tracked_slots:output.len()});}
            else if arities.len()>1{self.native.host.cardinality_uses.insert(*fact,FiniteSlotCardinality{arities:arities.into_iter().collect(),tracked_slots:output.len()});}
        }
        for (index,fact) in output.iter().enumerate(){
            self.budget()?;let mut candidates=Vec::new();let mut inputs=Vec::new();let mut conditions=std::collections::BTreeSet::new();
            let observation=digest(&(self.graph.snapshot_key.as_str(),"conditional_return",fact,index+1));
            for (candidate_index,path) in paths.iter().enumerate(){
                self.budget()?;
                let values=match &path.outcome{NativeControl::Return(values)=>values.as_slice(),_=>&[]};
                let (selected,source_uses)=self.host_list_slot(values,index,"missing_return_slot_is_nil");inputs.extend(source_uses.iter().copied());
                let terminal=self.native_terminal(path);for condition in terminal.paths.iter().flatten(){conditions.insert(condition.condition);}
                let id=digest(&(&observation,candidate_index,&terminal.context,&terminal.paths,values.get(index)));
                candidates.push(ConditionalReturnCandidate{id,observation:observation.clone(),slot:index+1,context:terminal.context,paths:terminal.paths,use_fact:values.get(index).copied(),source_uses,selected,presence:self.host_slot_presence(values,index).into()});
            }
            inputs.sort_unstable();inputs.dedup();
            self.host_transfer("conditional_return_candidates",inputs,vec![],TransferDestination::Use{use_fact:*fact},self.graph.uses[*fact].decision.clone(),conditions.into_iter().collect())?;
            let edge=self.graph.host_runtime.as_mut().expect("host facts").transfers.last_mut().expect("return candidate edge");
            edge.proof_id=digest(&(&edge.proof_id,&candidates));edge.return_candidates=Some(candidates);
        }
        Ok(())
    }
    fn host_invoke(&mut self,binding:usize,call:usize,site:&Site,ctx:&Context)->Result<Vec<usize>> {
        let arity=self.inputs.host.data.bindings[binding].return_arity();
        // With no cardinality declaration even slot one may be absent. A single
        // possible slot anchors an open sequence; it is not an inferred arity.
        let count=arity.unwrap_or(1);let mut returns=Vec::new();
        for slot in 1..=count {
            self.budget()?;
            let value=self.value(site,ValueKind::HostReturn{binding,call,slot});
            if let Some(type_name)=self.inputs.host.data.bindings[binding].native_descriptor().and_then(|descriptor|descriptor.nominal_return_name(slot)).map(str::to_owned){
                if self.objects.len()>=self.object_limit(){return Err("host nominal object identity budget exceeded".into())}
                let Decision::Bound(value_id)=value else{return Err("host return identity missing".into())};
                let object=self.objects.len();
                self.objects.push(ObjectState{unknown:Some("environment_nominal_member_enumeration_unavailable".into()),..Default::default()});
                self.native.host.nominal_objects.insert(value_id,object);
                let methods=self.native.host.methods.get(&type_name).cloned().unwrap_or_default();
                for(field,method_value,method_binding)in methods{
                    self.budget()?;
                    self.native.host.initial_fields.insert((object,field.clone()),method_value);
                    self.graph.host_runtime.as_mut().expect("host facts").method_fields.push(HostNamespaceField{object,field,value:method_value,binding:method_binding});
                }
                let proof_id=digest(&(self.graph.snapshot_key.as_str(),value_id,object,binding,call,slot,&type_name));
                self.graph.host_runtime.as_mut().expect("host facts").nominal_objects.push(HostNominalObject{value:value_id,object,binding,slot,type_name,proof_id});
            }
            let fact=self.usage(site.clone(),ctx,value,vec![]);
            let identity=HostReturnSlotIdentity{binding:Some(binding),call,slot,presence:if arity.is_some(){"present"}else{"unknown"}.into()};
            self.graph.uses[fact].proof_id=digest(&(&self.graph.uses[fact].proof_id,&identity));
            self.graph.uses[fact].host_return_slot=Some(identity);returns.push(fact);
        }
        if arity.is_none() && let Some(fact)=returns.last(){self.native.host.open_tail_uses.insert(*fact,OpenReturnTail{calls:vec![call],first_unknown_slot:1,tracked_slots:returns.len()});}
        Ok(returns)
    }
    fn host_unknown_returns(&mut self,call:usize,site:&Site,ctx:&Context,selected:&Decision)->Result<Vec<usize>> {
        self.budget()?;
        let fact=self.usage(site.clone(),ctx,selected.clone(),vec![]);
        let identity=HostReturnSlotIdentity{binding:None,call,slot:1,presence:"unknown".into()};
        self.graph.uses[fact].proof_id=digest(&(&self.graph.uses[fact].proof_id,&identity));self.graph.uses[fact].host_return_slot=Some(identity);
        self.native.host.open_tail_uses.insert(fact,OpenReturnTail{calls:vec![call],first_unknown_slot:1,tracked_slots:1});
        Ok(vec![fact])
    }
    fn host_slot(&self,fact:usize,slot:usize,presence:&str)->CommonSlotFact {
        let value=&self.graph.uses[fact];CommonSlotFact{slot,use_fact:fact,selected:value.decision.clone(),writes:value.writes.clone(),presence:presence.into()}
    }
    fn host_record_call(&mut self,call:usize,ctx:&Context,receiver:Option<usize>,syntactic_arguments:Vec<Site>,returned:Option<&[usize]>,scalar_result:usize)->Result<()> {
        if self.graph.host_runtime.is_none(){return Ok(())}
        self.budget()?;
        if !self.inputs.corpus.limits.research||!ctx.deferred{
            if self.native.host.invocation_calls>=HOST_CALL_FACT_LIMIT{return Err("host call fact budget exceeded".into())}
            self.native.host.invocation_calls+=1;
        }
        let fact=&self.graph.calls[call];let selected=&self.graph.uses[fact.callee].decision;
        let binding=self.host_binding(selected);
        let argument_open_tail=self.host_tail(&fact.arguments);
        let return_open_tail=returned.and_then(|values|self.host_tail(values));
        let argument_arities=self.host_arities(&fact.arguments);let return_arities=returned.and_then(|values|self.host_arities(values));
        let arguments=fact.arguments.iter().enumerate().map(|(index,use_fact)|self.host_slot(*use_fact,index+1,self.host_slot_presence(&fact.arguments,index))).collect::<Vec<_>>();
        let returns=returned.unwrap_or_default().iter().enumerate().map(|(index,fact)|self.host_slot(*fact,index+1,self.host_slot_presence(returned.unwrap_or_default(),index))).collect::<Vec<_>>();
        let predicate=binding.and_then(|binding|self.inputs.host.data.bindings[binding].predicate_argument()).map(|slot|HostPredicateSubject{argument_slot:slot,subject:arguments.get(slot-1).cloned(),truth:"unknown_not_derived_from_risk_label".into()});
        let return_arity=return_arities.as_ref().filter(|arities|arities.len()==1).map(|arities|arities[0]);
        let view=self.native.facts.query_views.get(&ctx.name).cloned().ok_or("host call view missing")?;
        let callee_value=match selected{Decision::Bound(value)=>Some(*value),_=>None};
        let proof_id=digest(&(self.graph.snapshot_key.as_str(),call,&view,binding,callee_value,receiver,&syntactic_arguments,&arguments,&returns,&argument_open_tail,&return_open_tail,&argument_arities,&return_arities,scalar_result));
        self.graph.host_runtime.as_mut().expect("host facts initialized").calls.push(CommonCallSlots{call,context:ctx.name.clone(),view,host_binding:binding,callee_value,receiver,syntactic_arguments,arguments,argument_arities,argument_open_tail,returns,return_arity,return_arities,return_open_tail,scalar_result,predicate,proof_id});
        Ok(())
    }
    fn host_transfer(&mut self,kind:&str,source_uses:Vec<usize>,source_writes:Vec<usize>,destination:TransferDestination,selected:Decision,condition_facts:Vec<usize>)->Result<()> {
        self.budget()?;
        let facts=self.graph.host_runtime.as_mut().ok_or("host transfers without host facts")?;
        // host_finish_facts projects already bounded uses, writes and call slots;
        // those source facts do not create another dynamic invocation. The
        // conditional-return join is the one transfer created during execution.
        if !self.inputs.corpus.limits.research||kind=="conditional_return_candidates"{
            if self.native.host.invocation_transfers>=HOST_TRANSFER_LIMIT{return Err("host transfer fact budget exceeded".into())}
            self.native.host.invocation_transfers+=1;
        }
        let id=facts.transfers.len();let proof_id=digest(&(self.graph.snapshot_key.as_str(),id,kind,&source_uses,&source_writes,&destination,&selected,&condition_facts));
        facts.transfers.push(CommonTransferFact{id,kind:kind.into(),source_uses,source_writes,destination,selected,condition_facts,proof_id,return_candidates:None});Ok(())
    }
    fn host_finish_facts(&mut self)->Result<()> {
        if self.graph.host_runtime.is_none(){return Ok(())}
        // Projection only appends transfers/capture references. Keep the original
        // source lengths and order without retaining a second whole fact vector.
        for index in 0..self.graph.uses.len(){
            let fact=self.graph.uses[index].clone();
            if !fact.writes.is_empty(){self.host_transfer("storage_read",vec![],fact.writes,TransferDestination::Use{use_fact:fact.id},fact.decision,vec![])?;}
        }
        for index in 0..self.graph.writes.len(){
            let write=self.graph.writes[index].clone();
            let conditions=write.conditional_branch.map(|branch|self.graph.branches[branch].condition_facts.clone()).unwrap_or_default();
            self.host_transfer("storage_write",write.inputs,vec![],TransferDestination::Write{write:write.id},write.decision,conditions)?;
        }
        for index in 0..self.native.facts.control.write_joins.len(){
            let join=self.native.facts.control.write_joins[index].clone();
            let sources=join.candidates.iter().filter_map(|candidate|candidate.write).collect();
            self.host_transfer("conditional_write_candidates",vec![],sources,TransferDestination::Write{write:join.destination},self.graph.writes[join.destination].decision.clone(),self.graph.branches[join.branch].condition_facts.clone())?;
        }
        for index in 0..self.graph.host_runtime.as_ref().expect("host facts").calls.len(){
            let call=self.graph.host_runtime.as_ref().expect("host facts").calls[index].clone();
            for slot in call.arguments{self.host_transfer("argument_slot",vec![slot.use_fact],slot.writes,TransferDestination::Argument{call:call.call,slot:slot.slot},slot.selected,vec![])?;}
            for slot in &call.returns{self.host_transfer("return_slot",vec![slot.use_fact],slot.writes.clone(),TransferDestination::Return{call:call.call,slot:slot.slot},slot.selected.clone(),vec![])?;}
            self.host_transfer("scalar_adjustment",call.returns.first().map(|slot|slot.use_fact).into_iter().collect(),vec![],TransferDestination::ScalarResult{call:call.call},self.graph.uses[call.scalar_result].decision.clone(),vec![])?;
            self.host_transfer("scalar_result_use",call.returns.first().map(|slot|slot.use_fact).into_iter().collect(),vec![],TransferDestination::Use{use_fact:call.scalar_result},self.graph.uses[call.scalar_result].decision.clone(),vec![])?;
        }
        for index in 0..self.native.facts.capture_facts.len(){
            let capture=self.native.facts.capture_facts[index].clone();
            self.budget()?;
            let owner_value=self.native.facts.closures[capture.closure].value;
            let proof_id=digest(&(self.graph.snapshot_key.as_str(),"structural_capture_reference",capture.closure,&capture.cell,&capture.declaration,&capture.creation_context,owner_value));
            let facts=self.graph.host_runtime.as_mut().expect("host facts");
            if !self.inputs.corpus.limits.research&&facts.capture_references.len()>=INVOCATION_CAPTURE_EDGE_LIMIT{return Err("host capture reference budget exceeded".into())}
            facts.capture_references.push(CommonCaptureReference{closure:capture.closure,cell:capture.cell,declaration:capture.declaration,creation_context:capture.creation_context,owner_value,proof_id});
        }
        let facts=self.graph.host_runtime.as_mut().expect("host facts");facts.calls.sort_by_key(|call|call.call);facts.open_tail_uses=self.native.host.open_tail_uses.clone();facts.cardinality_uses=self.native.host.cardinality_uses.clone();Ok(())
    }
}

fn host_runtime_profile_for(entry:bool,research:bool)->Value{
    let mut profile=host_runtime_profile();
    profile["factAccounting"]=if research{json!({"profile":"whole_source_research","calls":"actual_invocations","transfers":"execution_constructed_return_joins","sourceTemplatesAndProjections":"existing_source_graph_and_shared_operation_budget"})}else{json!({"profile":"bounded_prototype_or_flow","calls":"all_call_facts","transfers":"all_transfer_facts"})};
    if entry{profile["entryRequest"]=json!(true);profile["views"].as_array_mut().expect("fixed views").push(json!("explicit_entry"));profile["origins"]=json!("source_or_authenticated_environment_or_explicit_entry");}profile
}
