// Flow is a projection of the common execution graph, never another Lua evaluator.
pub const FLOW_RULES: &str = "source-common-flow-candidate-paths-v1";

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(default, rename_all="camelCase")]
pub struct FlowBudget {
    pub max_files:usize, pub max_functions:usize, pub max_depth:usize,
    pub max_paths:usize, pub max_millis:u64, pub max_output_bytes:usize,
}
impl Default for FlowBudget {
    fn default()->Self{Self{max_files:32,max_functions:512,max_depth:4,max_paths:128,max_millis:2000,max_output_bytes:1<<20}}
}
impl FlowBudget {
    fn validate(&self)->lycheedev_core::Result<()> {
        if self.max_files==0||self.max_functions==0||self.max_depth==0||self.max_paths==0||self.max_millis==0||self.max_output_bytes==0 {
            return Err(input_error("flow budgets must be positive"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ExecutionObservation {
    pub occurrence:Option<usize>, pub paths:Vec<Vec<NativePredicate>>,
    pub loops:Vec<NativeLoopIteration>, pub view:Option<NativeQueryView>,
}
#[derive(Clone, Debug, Serialize)]
pub struct CommonExpressionFact {
    pub id:usize, pub site:Site, pub context:String, pub operator:String,
    pub operands:Vec<usize>, pub result:usize, pub proof_id:String,
}
#[derive(Clone, Debug, Serialize)]
pub struct ConditionalWriteCandidate {
    pub id:String, pub side:String, pub checkpoint:usize,
    pub paths:Vec<Vec<NativePredicate>>, pub write:Option<usize>,
    pub selected:Decision, pub presence:String, pub mutation_observation:Option<usize>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ConditionalWriteObservation {
    pub id:String, pub join:usize, pub branch:usize, pub destination:usize,
    pub cell:String, pub object:Option<usize>, pub field:Option<String>,
    pub mutation_observation:Option<usize>, pub candidates:Vec<ConditionalWriteCandidate>,
}

impl Builder {
    fn flow_expression(&mut self,site:Site,ctx:&Context,operator:String,operands:Vec<usize>,result:usize)->Result<()> {
        if self.graph.host_runtime.is_none(){return Ok(())}
        self.budget()?;
        let facts=self.graph.host_runtime.as_mut().expect("host facts");
        let id=facts.expressions.len();
        let proof_id=digest(&(self.graph.snapshot_key.as_str(),id,&site,&ctx.name,&operator,&operands,result));
        facts.expressions.push(CommonExpressionFact{id,site,context:ctx.name.clone(),operator,operands,result,proof_id});
        Ok(())
    }
    fn flow_write_join(&mut self,join:usize,branch:usize,destination:usize,sides:&[NativeTerminalPath],writes:[Option<usize>;2],field:Option<(usize,String)>)->Result<()> {
        self.budget()?;
        let mut candidates=Vec::new();
        for (index,(side,write)) in sides.iter().zip(writes).enumerate(){
            let checkpoint=side.checkpoint.ok_or("conditional write candidate lacks checkpoint")?;
            let selected=write.map(|id|self.graph.writes[id].decision.clone()).unwrap_or_else(||Decision::KnownAbsent("absent_on_join_path".into()));
            let id=digest(&(self.graph.snapshot_key.as_str(),"write_candidate",join,destination,index,checkpoint,write,&side.paths));
            candidates.push(ConditionalWriteCandidate{id,side:if index==0{"left"}else{"right"}.into(),checkpoint,paths:side.paths.clone(),write,selected,presence:if write.is_some(){"present"}else{"absent"}.into(),mutation_observation:write.and_then(|write|self.field_write_observations.get(&write).copied())});
        }
        let id=digest(&(self.graph.snapshot_key.as_str(),"write_observation",join,destination,&candidates));
        self.native.facts.control.write_joins.push(ConditionalWriteObservation{id,join,branch,destination,cell:self.graph.writes[destination].cell.clone(),object:field.as_ref().map(|item|item.0),field:field.map(|item|item.1),mutation_observation:self.field_write_observations.get(&destination).copied(),candidates});
        Ok(())
    }
    fn flow_publish_contexts(&mut self){
        for (name,state) in &self.native.contexts {
            self.native.facts.control.context_observations.insert(name.clone(),ExecutionObservation{occurrence:state.occurrence,paths:state.paths.clone(),loops:state.loops.clone(),view:self.native.facts.query_views.get(name).cloned()});
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct FlowOptions { pub target:QueryTarget }
impl Default for FlowOptions { fn default()->Self{Self{target:QueryTarget::All}} }
#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum FlowNode { Use{use_fact:usize}, Write{write:usize}, Argument{call:usize,slot:usize}, Return{call:usize,slot:usize} }
#[derive(Clone, Debug, Serialize)]
pub struct FlowProofStep {
    pub node:FlowNode, pub site:Site, pub context:String,
    pub occurrence:Option<usize>, pub selected:Decision,
    pub writes:Vec<usize>, pub candidate:Option<String>,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct FlowSeed {
    pub binding:usize, pub call:usize, pub slot:usize, pub value:usize,
    pub site:Site, pub rule:EnvironmentEvidence, pub rule_pointer:String,
}
#[derive(Clone, Debug, Serialize)]
pub struct FlowCandidate {
    pub seed:FlowSeed, pub conditions:Vec<NativePredicate>, pub steps:Vec<FlowProofStep>,
}
#[derive(Clone, Debug, Serialize)]
pub struct FlowFinding {
    pub id:String, pub call:usize, pub argument:usize, pub site:Site,
    pub occurrence:Option<usize>, pub context:String, pub rule:HostRuleEvidence,
    pub candidates:Vec<FlowCandidate>, pub multiplicity:String,
}
#[derive(Clone, Debug, Serialize)]
pub struct FlowGuardProof {
    pub call:usize, pub argument:usize, pub guard_call:usize, pub condition:usize,
    pub subject_use:usize, pub candidate:FlowCandidate, pub subject:FlowCandidate,
    pub conditions:Vec<NativePredicate>,
}
#[derive(Clone, Debug, Serialize)]
pub struct FlowGap {
    pub code:String, pub site:Option<Site>, pub context:Option<String>,
    pub detail:Value, pub dependencies:Vec<FlowCandidate>,
}
#[derive(Clone, Debug, Serialize)]
pub struct FlowResult {
    pub rules_version:String, pub snapshot:String, pub selector:QuerySelector,
    pub findings:QueryPage<FlowFinding>, pub guarded:Vec<FlowGuardProof>, pub gaps:Vec<FlowGap>,
    pub complete:bool, pub input_complete:bool, pub analysis_complete:bool,
    pub output_complete:bool, pub truncated:bool, pub statements:usize,
    pub lifecycle_acceptance:String,
}

struct FlowLink { node:FlowNode, paths:Vec<Vec<NativePredicate>>, candidate:Option<String> }
#[derive(Serialize)]
struct FlowUnknownDependency {
    reason:String, conditions:Vec<NativePredicate>, steps:Vec<FlowProofStep>,
}
struct FlowProjector<'a,'b> {
    query:&'a SourceQueries<'b>, graph:&'a Graph, host:&'a HostRuntimeFacts,
    expressions:BTreeMap<usize,&'a CommonExpressionFact>,
    returns:BTreeMap<usize,&'a CommonTransferFact>,
    scalars:BTreeMap<usize,&'a CommonCallSlots>,
    calls:BTreeMap<usize,&'a CommonCallSlots>,
    writes:BTreeMap<usize,&'a ConditionalWriteObservation>,
}

fn flow_paths(paths:&[Vec<NativePredicate>])->Vec<Vec<NativePredicate>> {
    if paths.is_empty(){vec![vec![]]}else{paths.to_vec()}
}
fn flow_intersection(left:&[NativePredicate],right:&[NativePredicate])->Option<Vec<NativePredicate>> {
    let mut result=left.to_vec();
    for predicate in right {
        if result.iter().any(|old|old.condition==predicate.condition&&old.outcome!=predicate.outcome){return None}
        if !result.contains(predicate){result.push(predicate.clone());}
    }
    result.sort_by_key(|predicate|(predicate.condition,predicate.outcome));Some(result)
}
// Difference between two conjunctions, retaining disjoint feasible alternatives.
fn flow_difference(path:&[NativePredicate],removed:&[NativePredicate])->Vec<Vec<NativePredicate>> {
    if flow_intersection(path,removed).is_none(){return vec![path.to_vec()]}
    let mut prefix=path.to_vec();let mut output=Vec::new();
    for predicate in removed {
        if prefix.contains(predicate){continue}
        let mut other=prefix.clone();other.push(NativePredicate{condition:predicate.condition,outcome:!predicate.outcome});
        other.sort_by_key(|item|(item.condition,item.outcome));output.push(other);prefix.push(predicate.clone());
    }
    output
}

impl<'a,'b> FlowProjector<'a,'b> {
    fn new(query:&'a SourceQueries<'b>,graph:&'a Graph,host:&'a HostRuntimeFacts)->lycheedev_core::Result<Self>{
        let mut result=Self{query,graph,host,expressions:BTreeMap::new(),returns:BTreeMap::new(),scalars:BTreeMap::new(),calls:BTreeMap::new(),writes:BTreeMap::new()};
        for expression in &host.expressions{query.step()?;result.expressions.insert(expression.result,expression);}
        for transfer in &host.transfers{query.step()?;if transfer.return_candidates.is_some()&&let TransferDestination::Use{use_fact}=transfer.destination{result.returns.insert(use_fact,transfer);}}
        for call in &host.calls{query.step()?;result.scalars.insert(call.scalar_result,call);result.calls.insert(call.call,call);}
        for write in &graph.native_invocations.as_ref().expect("native flow").control.write_joins{query.step()?;result.writes.insert(write.destination,write);}
        Ok(result)
    }
    fn observation(&self,context:&str)->Option<&ExecutionObservation>{self.graph.native_invocations.as_ref()?.control.context_observations.get(context)}
    fn paths(&self,context:&str)->Vec<Vec<NativePredicate>>{self.observation(context).map(|row|flow_paths(&row.paths)).unwrap_or_else(||vec![vec![]])}
    fn executed(&self,context:&str)->bool{
        !matches!(self.query.occurrence(context),QuerySelector::SyntaxOnly|QuerySelector::SourceTemplate{..})&&self.query.accepts(context)
    }
    fn step_fact(&self,node:&FlowNode,candidate:Option<String>)->FlowProofStep {
        let (site,context,selected,writes)=match *node {
            FlowNode::Use{use_fact}=>{let fact=&self.graph.uses[use_fact];(fact.site.clone(),fact.context.clone(),fact.decision.clone(),fact.writes.clone())},
            FlowNode::Write{write}=>{let fact=&self.graph.writes[write];(fact.site.clone(),fact.context.clone(),fact.decision.clone(),vec![write])},
            FlowNode::Argument{call,slot}|FlowNode::Return{call,slot}=>{
                let call_fact=&self.graph.calls[call];let slots=self.calls[&call];
                let value=if matches!(node,FlowNode::Argument{..}){slots.arguments.get(slot-1)}else{slots.returns.get(slot-1)};
                (call_fact.site.clone(),call_fact.context.clone(),value.map(|value|value.selected.clone()).unwrap_or_else(||Decision::KnownAbsent("absent_slot".into())),value.map(|value|value.writes.clone()).unwrap_or_default())
            }
        };
        let occurrence=self.observation(&context).and_then(|row|row.occurrence);
        FlowProofStep{node:node.clone(),site,context,occurrence,selected,writes,candidate}
    }
    fn seed(&self,node:&FlowNode)->Option<FlowSeed>{
        let FlowNode::Use{use_fact}=node else{return None};
        let fact=&self.graph.uses[*use_fact];let identity=fact.host_return_slot.as_ref()?;
        let binding=identity.binding?;let Decision::Bound(value)=fact.decision else{return None};
        let rules=self.host.bindings.get(binding)?.declaration.rules()?;
        if !matches!(rules.record().get("returns")?.get(identity.slot.to_string())?.get("state")?.as_str(),Some("secret"|"possible")){return None}
        Some(FlowSeed{binding,call:identity.call,slot:identity.slot,value,site:fact.site.clone(),rule:rules.evidence().clone(),rule_pointer:format!("{}/returns/{}",rules.evidence().pointer(),identity.slot)})
    }
    fn links(&self,node:&FlowNode)->Vec<FlowLink>{
        let plain=|node|FlowLink{node,paths:vec![vec![]],candidate:None};
        match *node {
            FlowNode::Argument{call,slot}=>self.calls[&call].arguments.get(slot-1).map(|slot|plain(FlowNode::Use{use_fact:slot.use_fact})).into_iter().collect(),
            FlowNode::Return{call,slot}=>self.calls[&call].returns.get(slot-1).map(|slot|plain(FlowNode::Use{use_fact:slot.use_fact})).into_iter().collect(),
            FlowNode::Write{write}=>{
                if let Some(join)=self.writes.get(&write){return join.candidates.iter().filter_map(|candidate|candidate.write.map(|write|FlowLink{node:FlowNode::Write{write},paths:flow_paths(&candidate.paths),candidate:Some(candidate.id.clone())})).collect()}
                self.graph.writes[write].inputs.iter().map(|use_fact|plain(FlowNode::Use{use_fact:*use_fact})).collect()
            },
            FlowNode::Use{use_fact}=>{
                if let Some(transfer)=self.returns.get(&use_fact){
                    return transfer.return_candidates.as_ref().expect("indexed candidates").iter().flat_map(|candidate|candidate.source_uses.iter().map(|use_fact|FlowLink{node:FlowNode::Use{use_fact:*use_fact},paths:flow_paths(&candidate.paths),candidate:Some(candidate.id.clone())})).collect()
                }
                if let Some(call)=self.scalars.get(&use_fact){return if call.returns.is_empty(){vec![]}else{vec![plain(FlowNode::Return{call:call.call,slot:1})]}}
                if let Some(expression)=self.expressions.get(&use_fact){
                    return match expression.operator.as_str(){
                        "parenthesis"|"selected_operand"=>expression.operands.iter().map(|use_fact|plain(FlowNode::Use{use_fact:*use_fact})).collect(),
                        "and"|"or" if expression.operands.len()==2=>{
                            let left=expression.operands[0];let on=expression.operator=="and";
                            vec![FlowLink{node:FlowNode::Use{use_fact:left},paths:vec![vec![NativePredicate{condition:left,outcome:!on}]],candidate:None},FlowLink{node:FlowNode::Use{use_fact:expression.operands[1]},paths:vec![vec![NativePredicate{condition:left,outcome:on}]],candidate:None}]
                        },
                        _=>vec![],
                    }
                }
                self.graph.uses[use_fact].writes.iter().map(|write|plain(FlowNode::Write{write:*write})).collect()
            },
        }
    }
    fn unknown_leaf(&self,step:&FlowProofStep)->Option<String>{
        match &step.selected {
            Decision::Unavailable(reason)=>Some(reason.clone()),
            Decision::NotManaged=>Some("dependency_not_managed".into()),
            Decision::Bound(value)=>match &self.graph.values[*value].kind {
                ValueKind::NativeParameter(_)=>Some("unknown_parameter_value".into()),
                ValueKind::CallResult(_)=>Some("unknown_call_return_value".into()),
                ValueKind::HostReturn{binding,slot,..}=>{
                    let declaration=&self.host.bindings[*binding].declaration;
                    if declaration.predicate_argument().is_some(){return None}
                    let state=declaration.rules().and_then(|rules|rules.record().get("returns")).and_then(|returns|returns.get(slot.to_string())).and_then(|rule|rule.get("state")).and_then(Value::as_str);
                    if matches!(state,Some("never"|"secret"|"possible")){None}else{Some("host_return_risk_rule_unavailable".into())}
                },
                _=>None,
            },
            Decision::KnownAbsent(_)=>None,
        }
    }
    fn trace(&self,node:FlowNode,conditions:&[NativePredicate])->lycheedev_core::Result<Vec<FlowCandidate>>{
        self.trace_dependencies(node,conditions,&mut Vec::new())
    }
    fn trace_dependencies(&self,node:FlowNode,conditions:&[NativePredicate],unknown:&mut Vec<FlowUnknownDependency>)->lycheedev_core::Result<Vec<FlowCandidate>>{
        let mut work=vec![(node,conditions.to_vec(),Vec::<FlowProofStep>::new(),None)];
        let mut seen=std::collections::BTreeSet::new();let mut output=Vec::new();
        while let Some((node,path,steps,candidate))=work.pop(){
            self.query.step()?;
            let key=(node.clone(),path.iter().map(|item|(item.condition,item.outcome)).collect::<Vec<_>>());
            if !seen.insert(key){continue}
            let step=self.step_fact(&node,candidate);let context_paths=self.paths(&step.context);
            for context in context_paths {
                let Some(path)=flow_intersection(&path,&context)else{continue};
                let mut steps=steps.clone();steps.push(step.clone());
                if let Some(seed)=self.seed(&node){steps.reverse();output.push(FlowCandidate{seed,conditions:path,steps});continue}
                let links=self.links(&node);
                if links.is_empty()&&let Some(reason)=self.unknown_leaf(&step){
                    let mut steps=steps.clone();steps.reverse();
                    unknown.push(FlowUnknownDependency{reason,conditions:path.clone(),steps});
                }
                for link in links{
                    for link_path in link.paths{
                        self.query.step()?;
                        if let Some(path)=flow_intersection(&path,&link_path){work.push((link.node.clone(),path,steps.clone(),link.candidate.clone()));}
                    }
                }
            }
        }
        Ok(output)
    }
    fn predicate(&self,condition:usize,truth:bool)->Option<(&CommonCallSlots,bool)>{
        let mut current=condition;let mut truth=truth;let mut seen=std::collections::BTreeSet::new();
        while seen.insert(current){
            if let Some(call)=self.scalars.get(&current)&&call.predicate.is_some(){return Some((call,truth))}
            let expression=self.expressions.get(&current)?;
            match expression.operator.as_str(){"not"=>truth = !truth,"parenthesis"|"selected_operand"=>{},_=>return None}
            current = *expression.operands.first()?;
        }
        None
    }
    fn guard(&self,call:usize,argument:usize,candidate:FlowCandidate,proofs:&mut Vec<FlowGuardProof>)->lycheedev_core::Result<Vec<FlowCandidate>>{
        let mut remaining=vec![candidate.clone()];
        for condition in &candidate.conditions {
            self.query.step()?;
            let Some((guard,false))=self.predicate(condition.condition,condition.outcome)else{continue};
            let Some(subject)=guard.predicate.as_ref().and_then(|predicate|predicate.subject.as_ref())else{continue};
            let subjects=self.trace(FlowNode::Use{use_fact:subject.use_fact},&[])?;
            for subject_candidate in subjects {
                if subject_candidate.seed!=candidate.seed{continue}
                let mut next=Vec::new();
                for candidate in remaining{
                    self.query.step()?;
                    if let Some(overlap)=flow_intersection(&candidate.conditions,&subject_candidate.conditions){
                        proofs.push(FlowGuardProof{call,argument,guard_call:guard.call,condition:condition.condition,subject_use:subject.use_fact,candidate:candidate.clone(),subject:subject_candidate.clone(),conditions:overlap});
                    }
                    for conditions in flow_difference(&candidate.conditions,&subject_candidate.conditions){let mut copy=candidate.clone();copy.conditions=conditions;next.push(copy);}
                }
                remaining=next;
            }
        }
        Ok(remaining)
    }
}

impl SourceQueries<'_> {
    pub fn flow(&self,options:FlowOptions,mut page:PageRequest)->lycheedev_core::Result<FlowResult>{
        let declaration_target=self.validate_target(&options.target)?;
        let graph=self.graph.ok_or_else(||input_error("flow requires an execution graph"))?;
        let budget=graph.inputs.corpus.limits.flow.as_ref().ok_or_else(||input_error("flow budget must be admitted before graph construction"))?;
        let native=graph.native_invocations.as_ref().ok_or_else(||input_error("flow requires native facts"))?;
        let host=graph.host_runtime.as_ref().ok_or_else(||input_error("flow requires admitted environment bindings"))?;
        let projector=FlowProjector::new(self,graph,host)?;
        let mut findings=Vec::new();let mut guarded=Vec::new();let mut gaps=Vec::new();
        for call in &host.calls{
            self.step()?;
            if !projector.executed(&call.context){continue}
            let fact=&graph.calls[call.call];
            let callee=&graph.uses[fact.callee];
            if !self.target_matches(&options.target,declaration_target,&fact.site,self.lexical_id_at(&callee.site),self.call_cell(callee),Some(&callee.decision)){continue}
            let rules=call.host_binding.and_then(|binding|host.bindings[binding].declaration.rules());
            if let Some(rules)=rules{
                for slot in &call.arguments{
                    self.step()?;
                    if !matches!(rules.record().get("arguments").and_then(|rules|rules.get(slot.slot.to_string())).and_then(|rule|rule.get("secret")).and_then(Value::as_str),Some("forbidden"|"conditional"|"unknown")){continue}
                    let mut candidates=Vec::new();let mut unknown=Vec::new();
                    for path in projector.paths(&call.context){
                        for candidate in projector.trace_dependencies(FlowNode::Argument{call:call.call,slot:slot.slot},&path,&mut unknown)?{
                            for candidate in projector.guard(call.call,slot.slot,candidate,&mut guarded)?{
                                candidates.push(candidate);
                            }
                        }
                    }
                    for dependency in unknown{
                        gaps.push(FlowGap{code:"dependency_value_unavailable".into(),site:Some(fact.site.clone()),context:Some(call.context.clone()),detail:json!({"call":call.call,"argument":slot.slot,"dependency":dependency}),dependencies:vec![]});
                    }
                    if !candidates.is_empty(){
                        let mut seen=std::collections::BTreeSet::new();candidates.retain(|candidate|seen.insert(digest(candidate)));
                        let id=digest(&(FLOW_RULES,&graph.snapshot_key,call.call,slot.slot));
                        findings.push(FlowFinding{id,call:call.call,argument:slot.slot,site:fact.site.clone(),occurrence:projector.observation(&call.context).and_then(|row|row.occurrence),context:call.context.clone(),rule:rules.clone(),candidates,multiplicity:if native.control.complete{"exact_abstract_occurrence"}else{"verified_prefix_occurrence"}.into()});
                    }
                }
            }
            if matches!(fact.invocation_coverage,CallCoverage::Unavailable)||matches!(fact.effect_coverage,CallCoverage::Unavailable){
                let mut dependencies=Vec::new();for slot in &call.arguments{dependencies.extend(projector.trace(FlowNode::Use{use_fact:slot.use_fact},&[])?);}
                gaps.push(FlowGap{code:"unresolved_call".into(),site:Some(fact.site.clone()),context:Some(call.context.clone()),detail:json!({"call":call.call,"callee":callee.decision,"invocation":fact.invocation_coverage,"effect":fact.effect_coverage}),dependencies});
            }
        }
        for expression in &host.expressions {
            self.step()?;
            if !projector.executed(&expression.context)||matches!(expression.operator.as_str(),"not"|"parenthesis"|"selected_operand"|"and"|"or"){continue}
            let mut dependencies=Vec::new();for operand in &expression.operands{dependencies.extend(projector.trace(FlowNode::Use{use_fact:*operand},&[])?);}
            if !dependencies.is_empty(){gaps.push(FlowGap{code:"operation_rule_unavailable".into(),site:Some(expression.site.clone()),context:Some(expression.context.clone()),detail:json!({"expression":expression.id,"operator":expression.operator,"operands":expression.operands,"result":expression.result}),dependencies});}
        }
        for boundary in &native.control.boundaries{
            if matches!(self.occurrence(&boundary.context),QuerySelector::SourceTemplate{..}){continue}
            gaps.push(FlowGap{code:boundary.reason.clone(),site:Some(boundary.site.clone()),context:Some(boundary.context.clone()),detail:json!(boundary),dependencies:vec![]});
        }
        for boundary in &native.execution_boundaries{gaps.push(FlowGap{code:boundary.reason.clone(),site:None,context:None,detail:json!(boundary),dependencies:vec![]});}
        findings.sort_by(|left,right|(left.call,left.argument,&left.id).cmp(&(right.call,right.argument,&right.id)));findings.dedup_by(|left,right|left.id==right.id);
        page.bytes=page.bytes.min(budget.max_output_bytes);
        let findings=self.page(findings,&json!({"kind":"flow","rules":FLOW_RULES,"options":options}),page,native.control.truncated.then(||"execution_budget".into()))?;
        let output_complete=!findings.has_more&&!native.control.truncated;
        let complete=native.control.complete&&gaps.is_empty()&&output_complete;
        let result=FlowResult{rules_version:FLOW_RULES.into(),snapshot:graph.snapshot_key.clone(),selector:self.selector.clone(),findings,guarded,gaps,complete,input_complete:true,analysis_complete:native.control.complete,output_complete,truncated:native.control.truncated,statements:native.control.flow_statements,lifecycle_acceptance:"external_owner_measurement_required".into()};
        self.checked_serialize(&result,budget.max_output_bytes)?;Ok(result)
    }
}
