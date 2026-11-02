// Structured control belongs to the same native evaluator and heap as all value facts.
const CONTROL_NODE_LIMIT:usize=4096;
const CONTROL_EDGE_LIMIT:usize=8192;
const CONTROL_ACTIVE_LIMIT:usize=32;
const CONTROL_TERMINAL_LIMIT:usize=128;
const LOOP_ITERATION_LIMIT:usize=64;
const LOOP_TOTAL_LIMIT:usize=256;

#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
pub struct NativePredicate { pub condition:usize, pub outcome:bool }
#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
pub struct NativeLoopIteration { pub site:Site, pub iteration:usize, pub activation:usize }
#[derive(Clone,Debug,Serialize)]
pub struct NativeOccurrence {
    pub id:usize, pub site:Site, pub context:String, pub load:usize, pub frame:Option<usize>,
    pub parent:Option<usize>, pub paths:Vec<Vec<NativePredicate>>, pub loops:Vec<NativeLoopIteration>,
}
#[derive(Clone,Debug,Serialize)]
pub struct NativeControlEdge { pub from:Option<usize>, pub to:usize, pub kind:String, pub condition:Option<NativePredicate> }
#[derive(Clone,Debug,Serialize)]
pub struct NativeScopeActivation { pub id:usize, pub scope:Site, pub context:String, pub cells:BTreeMap<String,String> }
#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="snake_case")]
pub enum NativeBoundaryKind { Knowledge, Unsupported, Termination, Budget }
#[derive(Clone,Debug,Serialize)]
pub struct NativeControlBoundary { pub id:usize, pub kind:NativeBoundaryKind, pub reason:String, pub site:Site, pub occurrence:Option<usize>, pub context:String, pub truncated:bool }
#[derive(Clone,Debug,Serialize)]
#[serde(tag="kind",content="facts",rename_all="snake_case")]
pub enum NativeControl { Continue, Return(Vec<usize>), Break, Unresolved(usize) }
#[derive(Clone,Debug,Serialize)]
pub struct NativeTerminalPath { pub context:String, pub occurrence:Option<usize>, pub paths:Vec<Vec<NativePredicate>>, pub outcome:NativeControl, pub checkpoint:Option<usize> }
#[derive(Clone,Debug,Serialize)]
pub struct NativeControlJoin { pub id:usize, pub occurrence:Option<usize>, pub branch:usize, pub paths:Vec<NativeTerminalPath>, pub left_checkpoint:usize, pub right_checkpoint:usize }
/// A value produced by an actual loop activation, not an expression at its declaration.
#[derive(Clone,Debug,Serialize)]
pub struct NativeDerivedNumber {
    pub value:usize, pub number:f64, pub number_bits:String, pub origin:Site, pub statement:Site,
    pub header_facts:Vec<usize>, pub iteration:usize, pub predecessor:Option<usize>,
    pub occurrence:usize, pub rule:String, pub proof_id:String,
}
#[derive(Clone,Debug,Serialize)]
pub struct NativeControlFacts {
    pub context_observations:BTreeMap<String,ExecutionObservation>,
    pub write_joins:Vec<ConditionalWriteObservation>,
    pub flow_statements:usize,
    pub complete:bool, pub truncated:bool, pub coverage:String,
    pub occurrences:Vec<NativeOccurrence>, pub edges:Vec<NativeControlEdge>,
    pub activations:Vec<NativeScopeActivation>, pub exits:Vec<NativeTerminalPath>, pub boundaries:Vec<NativeControlBoundary>, pub terminals:Vec<NativeTerminalPath>, pub joins:Vec<NativeControlJoin>,
    pub context_occurrences:BTreeMap<String,usize>, pub numbers:BTreeMap<usize,f64>, pub derived_numbers:BTreeMap<usize,NativeDerivedNumber>,
}
impl Default for NativeControlFacts {fn default()->Self{Self{context_observations:BTreeMap::new(),write_joins:vec![],flow_statements:0,complete:true,truncated:false,coverage:"complete_bounded_execution".into(),occurrences:vec![],edges:vec![],activations:vec![],exits:vec![],boundaries:vec![],terminals:vec![],joins:vec![],context_occurrences:BTreeMap::new(),numbers:BTreeMap::new(),derived_numbers:BTreeMap::new()}}}
#[derive(Clone,Default)]
struct NativeContextState {
    occurrence:Option<usize>, paths:Vec<Vec<NativePredicate>>, loops:Vec<NativeLoopIteration>, activations:BTreeMap<String,usize>,
}
#[derive(Clone)]
struct NativePath { ctx:Context, outcome:NativeControl, heap:Option<NativeHeapCheckpoint> }
struct NativeLoopPosition { iteration:usize, predecessor:Option<usize> }
impl NativePath {fn continuing(ctx:&Context)->Self{Self{ctx:ctx.clone(),outcome:NativeControl::Continue,heap:None}}}

// Exact Boolean union: (P and C) or (P and not C) equals P. Branch edges
// and join inputs retain the original predicates; only continuation reachability
// is reduced. Distinct condition facts are never treated as the same predicate.
fn reduce_predicate_alternatives(alternatives:&mut Vec<Vec<NativePredicate>>) {
    loop {
        let mut merge=None;
        'pairs: for left in 0..alternatives.len(){for right in left+1..alternatives.len(){
            let a=&alternatives[left];let b=&alternatives[right];
            if a.len()!=b.len(){continue}
            let mut different=None;
            for(index,(a,b))in a.iter().zip(b).enumerate(){
                if a.condition!=b.condition{different=None;break}
                if a.outcome!=b.outcome{if different.is_some(){different=None;break}different=Some(index);}
            }
            if let Some(index)=different{merge=Some((left,right,index));break 'pairs}
        }}
        let Some((left,right,index))=merge else{break};
        let mut common=alternatives.remove(right);common.remove(index);alternatives.remove(left);
        if !alternatives.contains(&common){alternatives.push(common)}
    }
}

impl Builder {
    fn native_number(&self,fact:usize)->Option<f64>{match self.graph.uses.get(fact)?.decision{Decision::Bound(value)=>self.native.numeric_values.get(&value).copied(),_=>None}}
    fn native_number_value(&mut self,site:&Site,number:f64)->Decision {
        let value=self.value(site,ValueKind::Primitive("number".into()));
        if let Decision::Bound(id)=value{self.native.numeric_values.insert(id,number);self.native.facts.control.numbers.insert(id,number);}
        value
    }
    fn native_loop_number(&mut self,origin:&Site,statement:&Site,number:f64,header_facts:&[usize],position:NativeLoopPosition,ctx:&Context)->Result<Decision> {
        let NativeLoopPosition{iteration,predecessor}=position;
        self.budget()?;
        let occurrence=self.native.contexts.get(&ctx.name).and_then(|state|state.occurrence).ok_or("numeric loop value lacks occurrence")?;
        let decision=self.native_number_value(origin,number);
        let Decision::Bound(value)=decision else{return Err("numeric loop value allocation failed".into())};
        let rule="lua51-numeric-for-iteration-value-v1".to_string();
        let number_bits=format!("{:016x}",number.to_bits());
        let proof_id=digest(&(self.graph.snapshot_key.as_str(),value,&number_bits,origin,statement,header_facts,iteration,predecessor,occurrence,&rule));
        self.native.facts.control.derived_numbers.insert(value,NativeDerivedNumber{value,number,number_bits,origin:origin.clone(),statement:statement.clone(),header_facts:header_facts.to_vec(),iteration,predecessor,occurrence,rule,proof_id});
        Ok(decision)
    }
    fn native_terminal(&self,path:&NativePath)->NativeTerminalPath {
        let state=self.native.contexts.get(&path.ctx.name).cloned().unwrap_or_default();
        NativeTerminalPath{context:path.ctx.name.clone(),occurrence:state.occurrence,paths:state.paths,outcome:path.outcome.clone(),checkpoint:path.heap.as_ref().map(|heap|heap.id)}
    }
    fn native_boundary(&mut self,site:&Site,ctx:&Context,kind:NativeBoundaryKind,reason:&str)->NativePath {
        let truncated=matches!(kind,NativeBoundaryKind::Budget);let id=self.native.facts.control.boundaries.len();
        let occurrence=self.native.contexts.get(&ctx.name).and_then(|state|state.occurrence);
        self.native.facts.control.complete=false;self.native.facts.control.truncated|=truncated;
        self.native.facts.control.coverage="verified_prefix_with_explicit_boundaries".into();
        self.native.facts.control.boundaries.push(NativeControlBoundary{id,kind,reason:reason.into(),site:site.clone(),occurrence,context:ctx.name.clone(),truncated});
        self.native_mark_uncertain_effect();
        NativePath{ctx:ctx.clone(),outcome:NativeControl::Unresolved(id),heap:None}
    }
    fn native_occurrence(&mut self,site:&Site,ctx:&Context,kind:&str)->Result<Context> {
        self.budget()?;
        if self.native.facts.control.occurrences.len()>=CONTROL_NODE_LIMIT||self.native.facts.control.edges.len()>=CONTROL_EDGE_LIMIT{return Err("native control node or edge budget exceeded".into())}
        let id=self.native.facts.control.occurrences.len();
        let child=self.native_child_context(ctx,format!("{}:visit:{id}",ctx.name.split(":visit:").next().unwrap_or(&ctx.name)),ctx.conditional_branch);
        let mut state=self.native.contexts.get(&child.name).cloned().unwrap_or_default();
        if state.paths.is_empty(){state.paths.push(vec![])}
        self.native.facts.control.occurrences.push(NativeOccurrence{id,site:site.clone(),context:child.name.clone(),load:ctx.occurrence,frame:self.native.context_frames.get(&ctx.name).copied(),parent:state.occurrence,paths:state.paths.clone(),loops:state.loops.clone()});
        self.native.facts.control.edges.push(NativeControlEdge{from:state.occurrence,to:id,kind:kind.into(),condition:None});
        state.occurrence=Some(id);self.native.contexts.insert(child.name.clone(),state);
        self.native.facts.control.context_occurrences.insert(child.name.clone(),id);Ok(child)
    }
    fn native_predicate_context(&mut self,ctx:&Context,fact:usize,outcome:bool,branch:Option<usize>)->Result<Context> {
        if self.native.facts.control.edges.len()>=CONTROL_EDGE_LIMIT{return Err("native control edge budget exceeded".into())}
        let child=self.native_child_context(ctx,format!("{}:condition:{fact}:{outcome}",ctx.name),branch);
        let state=self.native.contexts.entry(child.name.clone()).or_default();if state.paths.is_empty(){state.paths.push(vec![])}
        let predicate=NativePredicate{condition:fact,outcome};for path in &mut state.paths{path.push(predicate.clone());}
        if let Some(id)=state.occurrence{self.native.facts.control.edges.push(NativeControlEdge{from:Some(id),to:id,kind:"condition".into(),condition:Some(predicate)});}
        Ok(child)
    }
    fn native_activate(&mut self,scope:&Site,block:Option<&LuaBlock>,ctx:&Context,extra:Option<&Site>)->Result<Context> {
        self.budget()?;if self.native.facts.control.activations.len()>=CONTROL_NODE_LIMIT{return Err("native scope activation budget exceeded".into())}
        let id=self.native.facts.control.activations.len();let child=self.native_child_context(ctx,format!("{}:activation:{id}",ctx.name),ctx.conditional_branch);
        let mut declarations=Vec::new();if let Some(site)=extra{declarations.push(site.clone())}
        for stat in block.into_iter().flat_map(LuaBlock::get_stats){self.budget()?;match stat{
            LuaStat::LocalStat(stat)=>for local in stat.get_local_name_list(){if let Some(name)=local.get_name_token(){declarations.push(self.inputs.site(&ctx.file,name.get_range()));}},
            LuaStat::LocalFuncStat(stat)=>if let Some(name)=stat.get_local_name().and_then(|name|name.get_name_token()){declarations.push(self.inputs.site(&ctx.file,name.get_range()));},_=>{},
        }}
        for declaration in &declarations{self.native.contexts.entry(child.name.clone()).or_default().activations.insert(declaration.id(),id);}
        let cells=declarations.iter().map(|site|(site.id(),self.native_local_key(&site.id(),&child))).collect();
        self.native.facts.control.activations.push(NativeScopeActivation{id,scope:scope.clone(),context:child.name.clone(),cells});Ok(child)
    }
    fn native_path_snapshot(&mut self,mut path:NativePath,site:&Site)->Result<NativePath> {
        if path.heap.is_none(){path.heap=Some(self.native_checkpoint(site,&path.ctx)?)}Ok(path)
    }
    fn native_restore_path(&mut self,path:&NativePath,site:&Site)->Result<()> {if let Some(heap)=&path.heap{self.native_restore(heap,site,&path.ctx)?;}Ok(())}
    fn native_join_continuations(&mut self,mut paths:Vec<NativePath>,site:&Site,ctx:&Context)->Result<Vec<NativePath>> {
        // The unsaved path owns the current heap. Save it before restoring any
        // previously terminated sibling; otherwise a late return loses its writes.
        if paths.len()>1 {for path in &mut paths{if path.heap.is_none(){*path=self.native_path_snapshot(path.clone(),site)?;}}}
        let (mut active,mut stopped):(Vec<_>,Vec<_>)=paths.into_iter().partition(|path|matches!(path.outcome,NativeControl::Continue));
        if active.len()>CONTROL_ACTIVE_LIMIT||stopped.len()>CONTROL_TERMINAL_LIMIT{return Err("native active or terminal path budget exceeded".into())}
        if active.len()<2{stopped.append(&mut active);return Ok(stopped)}
        let mut first=active.remove(0);self.native_restore_path(&first,site)?;first=self.native_path_snapshot(first,site)?;
        for next in active {
            self.native_restore_path(&next,site)?;let next=self.native_path_snapshot(next,site)?;
            let left=first.heap.as_ref().ok_or("left continuation checkpoint")?;let right=next.heap.as_ref().ok_or("right continuation checkpoint")?;
            let branch=first.ctx.conditional_branch.or(next.ctx.conditional_branch).unwrap_or_else(||self.native_branch(site,ctx,vec![]));
            let incoming=vec![self.native_terminal(&first),self.native_terminal(&next)];
            let join=self.native.facts.control.joins.len();
            self.native_restore(left,site,ctx)?;self.native_join(left,right,site,ctx,(branch,join),&incoming)?;
            let mut merged=self.native_child_context(ctx,format!("{}:join:{}",ctx.name,self.native.facts.control.joins.len()),ctx.conditional_branch);
            let mut alternatives=Vec::new();for path in &incoming{for predicates in &path.paths{if !alternatives.contains(predicates){alternatives.push(predicates.clone())}}}
            reduce_predicate_alternatives(&mut alternatives);
            if alternatives.len()>CONTROL_ACTIVE_LIMIT{return Err("native predicate alternative budget exceeded".into())}
            self.native.contexts.entry(merged.name.clone()).or_default().paths=alternatives;
            // Branches return to the same lexical continuation, not to either branch scope.
            merged.conditional_branch=ctx.conditional_branch;
            self.native.facts.control.joins.push(NativeControlJoin{id:join,occurrence:self.native.contexts.get(&ctx.name).and_then(|state|state.occurrence),branch,paths:incoming,left_checkpoint:left.id,right_checkpoint:right.id});
            first=self.native_path_snapshot(NativePath::continuing(&merged),site)?;
        }
        stopped.push(first);Ok(stopped)
    }
    fn native_block(&mut self,block:LuaBlock,ctx:&Context,depth:usize)->Result<Vec<NativePath>> {
        let range=block.get_range();self.native_block_body(Some(block),range,ctx,depth)
    }
    fn native_block_body(&mut self,block:Option<LuaBlock>,range:TextRange,ctx:&Context,depth:usize)->Result<Vec<NativePath>> {
        self.native_register_view(ctx);self.budget()?;if depth>64{return Err("native block depth budget exceeded".into())}
        let scope=self.inputs.site(&ctx.file,range);
        let scope_ctx=self.native_activate(&scope,block.as_ref(),ctx,None)?;
        self.native_stats(block.iter().flat_map(LuaBlock::get_stats).collect(),&scope_ctx,depth)
    }
    fn native_load_terminals(&mut self,paths:Vec<NativePath>)->Result<()> {
        if self.native.facts.control.terminals.len()+paths.len()>CONTROL_TERMINAL_LIMIT{return Err("native terminal path budget exceeded".into())}
        let terminals=paths.iter().map(|path|self.native_terminal(path)).collect::<Vec<_>>();self.native.facts.control.terminals.extend(terminals);Ok(())
    }
    fn native_stats(&mut self,stats:Vec<LuaStat>,ctx:&Context,depth:usize)->Result<Vec<NativePath>> {
        let mut paths=vec![NativePath::continuing(ctx)];
        for stat in stats {
            let site=self.inputs.site(&ctx.file,stat.get_range());let mut next=Vec::new();
            for mut path in paths {
                if !matches!(path.outcome,NativeControl::Continue){next.push(path);continue}
                if !path.ctx.deferred && let Some(flow)=&self.inputs.corpus.limits.flow {
                    if self.native.facts.control.flow_statements>=flow.max_paths {
                        next.push(self.native_boundary(&site,&path.ctx,NativeBoundaryKind::Budget,"flow_path_budget"));
                        continue;
                    }
                    self.native.facts.control.flow_statements+=1;
                }
                self.native_restore_path(&path,&site)?;path.heap=None;
                let stmt_ctx=self.native_occurrence(&site,&path.ctx,"statement")?;
                let results=self.native_statement(stat.clone(),&stmt_ctx,depth)?;
                next.extend(results);
                if next.len()>CONTROL_ACTIVE_LIMIT+CONTROL_TERMINAL_LIMIT{return Err("native pending path budget exceeded".into())}
            }
            paths=self.native_join_continuations(next,&site,ctx)?;
            if paths.iter().all(|path|!matches!(path.outcome,NativeControl::Continue)){break}
        }
        Ok(paths)
    }
    fn native_record_exit(&mut self,ctx:&Context,outcome:NativeControl)->Result<Vec<NativePath>> {
        if self.native.facts.control.exits.len()>=CONTROL_TERMINAL_LIMIT{return Err("native terminal exit budget exceeded".into())}
        let path=NativePath{ctx:ctx.clone(),outcome,heap:None};let fact=self.native_terminal(&path);self.native.facts.control.exits.push(fact);Ok(vec![path])
    }
    fn native_statement(&mut self,stat:LuaStat,ctx:&Context,depth:usize)->Result<Vec<NativePath>> {
        self.budget()?;let site=self.inputs.site(&ctx.file,stat.get_range());
        match stat {
            LuaStat::IfStat(stat)=>{let mut alternatives=vec![(stat.get_condition_expr(),stat.get_block())];for clause in stat.get_all_clause(){alternatives.push((clause.get_condition_expr(),clause.get_block()));}self.native_alternatives(&alternatives,0,&site,ctx,depth+1)},
            LuaStat::DoStat(stat)=>if let Some(block)=stat.get_block(){self.native_block(block,ctx,depth+1)}else{Ok(vec![NativePath::continuing(ctx)])},
            LuaStat::BreakStat(_)=>self.native_record_exit(ctx,NativeControl::Break),
            LuaStat::ForStat(stat)=>self.native_numeric_loop(stat,&site,ctx,depth+1),
            LuaStat::RepeatStat(stat)=>self.native_repeat_loop(stat,&site,ctx,depth+1),
            LuaStat::WhileStat(stat)=>self.native_while_loop(stat,&site,ctx,depth+1),
            LuaStat::ForRangeStat(stat)=>{self.native_eval_list(stat.get_expr_list().collect(),ctx,0)?;Ok(vec![self.native_boundary(&site,ctx,NativeBoundaryKind::Knowledge,"generic_iterator_identity_or_iteration_sequence_unavailable")])},
            other=>{let outcome=self.native_simple_stat(other,ctx)?;if matches!(outcome,NativeControl::Continue){Ok(vec![NativePath::continuing(ctx)])}else{self.native_record_exit(ctx,outcome)}},
        }
    }
    fn native_alternatives(&mut self,alternatives:&[(Option<LuaExpr>,Option<LuaBlock>)],index:usize,site:&Site,ctx:&Context,depth:usize)->Result<Vec<NativePath>> {
        self.budget()?;if depth>64{return Err("native conditional depth budget exceeded".into())}
        let Some((condition,block))=alternatives.get(index)else{return Ok(vec![NativePath::continuing(ctx)])};
        let Some(condition)=condition else{return block.clone().map(|block|self.native_block(block,ctx,depth+1)).unwrap_or_else(||Ok(vec![NativePath::continuing(ctx)]))};
        let fact=self.eval(condition.clone(),ctx,0)?;let truth=self.native_truthiness(&self.graph.uses[fact].decision);
        if let Some(outcome)=truth {
            let child=self.native_predicate_context(ctx,fact,outcome,ctx.conditional_branch)?;
            return if outcome{block.clone().map(|block|self.native_block(block,&child,depth+1)).unwrap_or_else(||Ok(vec![NativePath::continuing(&child)]))}else{self.native_alternatives(alternatives,index+1,site,&child,depth+1)}
        }
        let branch=self.native_branch(site,ctx,vec![fact]);let before=self.native_checkpoint(site,ctx)?;
        let yes_ctx=self.native_predicate_context(ctx,fact,true,Some(branch))?;
        let mut yes=block.clone().map(|block|self.native_block(block,&yes_ctx,depth+1)).unwrap_or_else(||Ok(vec![NativePath::continuing(&yes_ctx)]))?;
        for path in &mut yes {self.native_restore_path(path,site)?;*path=self.native_path_snapshot(path.clone(),site)?;}
        self.native_restore(&before,site,ctx)?;
        let no_ctx=self.native_predicate_context(ctx,fact,false,Some(branch))?;
        let mut no=self.native_alternatives(alternatives,index+1,site,&no_ctx,depth+1)?;
        for path in &mut no {self.native_restore_path(path,site)?;*path=self.native_path_snapshot(path.clone(),site)?;}
        yes.extend(no);self.native_join_continuations(yes,site,ctx)
    }
    fn native_loop_context(&mut self,site:&Site,block:&LuaBlock,ctx:&Context,iteration:usize,variable:Option<&Site>)->Result<Context> {
        self.budget()?;self.native.loop_iterations+=1;
        let child=self.native_activate(site,Some(block),ctx,variable)?;
        let activation=self.native.facts.control.activations.last().ok_or("loop activation")?.id;
        let state=self.native.contexts.entry(child.name.clone()).or_default();state.loops.retain(|item|item.site!=*site);state.loops.push(NativeLoopIteration{site:site.clone(),iteration,activation});
        self.native_occurrence(site,&child,"loop_iteration")
    }
    fn native_loop_limit(&self,iteration:usize)->bool{iteration>=LOOP_ITERATION_LIMIT||self.native.loop_iterations>=LOOP_TOTAL_LIMIT}
    fn native_numeric_loop(&mut self,stat:emmylua_parser::LuaForStat,site:&Site,ctx:&Context,depth:usize)->Result<Vec<NativePath>> {
        let exprs:Vec<_>=stat.get_iter_expr().collect();let mut facts=Vec::new();for expr in exprs{facts.push(self.eval(expr,ctx,0)?)}
        let Some(mut value)=facts.first().and_then(|fact|self.native_number(*fact))else{return Ok(vec![self.native_boundary(site,ctx,NativeBoundaryKind::Knowledge,"numeric_loop_initial_value_unavailable")])};
        let Some(end)=facts.get(1).and_then(|fact|self.native_number(*fact))else{return Ok(vec![self.native_boundary(site,ctx,NativeBoundaryKind::Knowledge,"numeric_loop_limit_unavailable")])};
        let step=if let Some(fact)=facts.get(2){let Some(step)=self.native_number(*fact)else{return Ok(vec![self.native_boundary(site,ctx,NativeBoundaryKind::Knowledge,"numeric_loop_step_unavailable")])};step}else{1.0};
        if step==0.0{return Ok(vec![self.native_boundary(site,ctx,NativeBoundaryKind::Unsupported,"numeric_zero_step_not_in_supported_termination_subset")])}
        let block=stat.get_block().ok_or("numeric loop body missing")?;let variable=self.inputs.site(&ctx.file,stat.get_var_name().ok_or("numeric loop declaration missing")?.get_range());
        let mut iteration=0;let mut predecessor=None;let mut stopped=Vec::new();let mut active=vec![NativePath::continuing(ctx)];
        while if step>0.0{value<=end}else{value>=end} {
            if self.native_loop_limit(iteration){for path in active{stopped.push(self.native_boundary(site,&path.ctx,NativeBoundaryKind::Budget,"numeric_loop_iteration_budget_exhausted"));}return Ok(stopped)}
            let mut next=Vec::new();for path in active {
                self.native_restore_path(&path,site)?;let loop_ctx=self.native_loop_context(site,&block,&path.ctx,iteration,Some(&variable))?;
                let decision=self.native_loop_number(&variable,site,value,&facts,NativeLoopPosition{iteration,predecessor},&loop_ctx)?;
                if let Decision::Bound(id)=decision{predecessor=Some(id);}
                self.write(self.native_local_key(&variable.id(),&loop_ctx),variable.clone(),&loop_ctx,decision,facts.clone());
                for mut result in self.native_stats(block.get_stats().collect(),&loop_ctx,depth)? {match result.outcome{
                    NativeControl::Continue=>next.push(result),NativeControl::Break=>{result.outcome=NativeControl::Continue;stopped.push(result)},_=>stopped.push(result),
                }}
            }
            active=next;if active.is_empty(){return Ok(stopped)}
            iteration+=1;let new_value=value+step;if !new_value.is_finite()||new_value==value{for path in active{stopped.push(self.native_boundary(site,&path.ctx,NativeBoundaryKind::Knowledge,"numeric_progress_not_representable"));}return Ok(stopped)}value=new_value;
        }
        stopped.extend(active);self.native_join_continuations(stopped,site,ctx)
    }
    fn native_repeat_loop(&mut self,stat:emmylua_parser::LuaRepeatStat,site:&Site,ctx:&Context,depth:usize)->Result<Vec<NativePath>> {
        let block=stat.get_block().ok_or("repeat body missing")?;let condition=stat.get_condition_expr().ok_or("repeat condition missing")?;
        let mut active=vec![NativePath::continuing(ctx)];let mut stopped=Vec::new();let mut iteration=0;
        loop {
            if self.native_loop_limit(iteration){for path in active{stopped.push(self.native_boundary(site,&path.ctx,NativeBoundaryKind::Budget,"repeat_loop_iteration_budget_exhausted"));}return Ok(stopped)}
            let mut next=Vec::new();for path in active {
                self.native_restore_path(&path,site)?;let loop_ctx=self.native_loop_context(site,&block,&path.ctx,iteration,None)?;
                for mut result in self.native_stats(block.get_stats().collect(),&loop_ctx,depth)? {
                    self.native_restore_path(&result,site)?;
                    match result.outcome {
                        NativeControl::Break=>{result.outcome=NativeControl::Continue;stopped.push(result)},
                        NativeControl::Continue=>{result.heap=None;let fact=self.eval(condition.clone(),&result.ctx,0)?;match self.native_truthiness(&self.graph.uses[fact].decision){
                            Some(outcome)=>{result.ctx=self.native_predicate_context(&result.ctx,fact,outcome,result.ctx.conditional_branch)?;result=self.native_path_snapshot(result,site)?;if outcome{stopped.push(result)}else{next.push(result)}},
                            None=>stopped.push(self.native_boundary(site,&result.ctx,NativeBoundaryKind::Knowledge,"repeat_condition_unavailable_after_verified_body")),
                        }},_=>stopped.push(result),
                    }
                }
            }
            if next.is_empty(){return self.native_join_continuations(stopped,site,ctx)}active=next;iteration+=1;
        }
    }
    fn native_while_loop(&mut self,stat:emmylua_parser::LuaWhileStat,site:&Site,ctx:&Context,depth:usize)->Result<Vec<NativePath>> {
        let fact=self.eval(stat.get_condition_expr().ok_or("while condition missing")?,ctx,0)?;
        match self.native_truthiness(&self.graph.uses[fact].decision) {
            Some(false)=>{let child=self.native_predicate_context(ctx,fact,false,ctx.conditional_branch)?;Ok(vec![NativePath::continuing(&child)])},
            None=>Ok(vec![self.native_boundary(site,ctx,NativeBoundaryKind::Knowledge,"while_condition_unavailable")]),
            Some(true)=>{
                if self.native_loop_limit(0){return Ok(vec![self.native_boundary(site,ctx,NativeBoundaryKind::Budget,"total_loop_iteration_budget_exhausted")])}
                let block=stat.get_block().ok_or("while body missing")?;let child=self.native_predicate_context(ctx,fact,true,ctx.conditional_branch)?;
                let loop_ctx=self.native_loop_context(site,&block,&child,0,None)?;let mut output=Vec::new();
                for mut path in self.native_stats(block.get_stats().collect(),&loop_ctx,depth)?{match path.outcome{
                    NativeControl::Break=>{path.outcome=NativeControl::Continue;output.push(path)},
                    NativeControl::Continue=>output.push(self.native_boundary(site,&path.ctx,NativeBoundaryKind::Termination,"while_prefix_verified_further_termination_unresolved")),_=>output.push(path),
                }}Ok(output)
            }
        }
    }
    fn native_finish_invocation(&mut self,paths:&[NativePath],ctx:&Context,frame:usize)->Result<Vec<usize>> {
        let site=self.graph.functions[self.native.facts.frames[frame].function].closure.clone();
        self.native.facts.frames[frame].return_paths=paths.iter().map(|path|self.native_terminal(path)).collect();
        if paths.iter().any(|path|matches!(path.outcome,NativeControl::Unresolved(_)|NativeControl::Break)){return Err("native invocation stopped at explicit control boundary".into())}
        if paths.len()==1 {self.native_restore_path(&paths[0],&site)?;return Ok(match &paths[0].outcome{NativeControl::Return(values)=>values.clone(),_=>vec![]})}
        let count=paths.iter().map(|path|match &path.outcome{NativeControl::Return(values)=>values.len(),_=>0}).max().unwrap_or(0);
        let mut output=Vec::new();for index in 0..count {self.budget()?;
            let mut selected=None;let mut equal=true;
            for path in paths{self.budget()?;let decision=match &path.outcome{NativeControl::Return(values)=>self.host_list_slot(values,index,"missing_return_slot_is_nil").0,_=>Decision::KnownAbsent("missing_return_slot_is_nil".into())};if let Some(previous)=&selected{equal&=*previous==decision}else{selected=Some(decision)}}
            let decision=if equal{selected.unwrap_or_else(||Decision::KnownAbsent("implicit_nil_return".into()))}else{Decision::Unavailable("conditional_return_value_candidates".into())};
            // Writes contains only write IDs. Candidate uses are separate edges.
            output.push(self.usage(site.clone(),ctx,decision,vec![]));
        }
        self.host_join_return_facts(paths,&output)?;
        let continuations=paths.iter().cloned().map(|mut path|{path.outcome=NativeControl::Continue;path}).collect();
        self.native_join_continuations(continuations,&site,ctx)?;Ok(output)
    }
}

// Only enumerated evaluator resource failures become truncation. Control and
// internal/protocol errors keep their own failure path.
fn native_budget_failure(reason:&str)->bool {matches!(reason,
        "conditional depth budget exceeded"
        |"flow call depth budget exceeded"
        |"flow function budget exceeded"
        |"expression depth budget exceeded"
        |"graph construction budget exceeded"
        |"host call fact budget exceeded"
        |"host capture reference budget exceeded"
        |"host object identity budget exceeded"
        |"host transfer fact budget exceeded"
        |"native active or terminal path budget exceeded"
        |"native block depth budget exceeded"
        |"native call depth budget exceeded"
        |"native captured cell edge budget exceeded"
        |"native checkpoint count budget exceeded"
        |"native checkpoint slot budget exceeded"
        |"native closure capture budget exceeded"
        |"native closure instance budget exceeded"
        |"native conditional depth budget exceeded"
        |"native control edge budget exceeded"
        |"native control node or edge budget exceeded"
        |"native escape event budget exceeded"
        |"native expression list arity budget exceeded"
        |"native frame budget exceeded"
        |"native normalized argument budget exceeded"
        |"native parameter count budget exceeded"
        |"native pending path budget exceeded"
        |"native predicate alternative budget exceeded"
        |"native reachable state footprint budget exceeded"
        |"native restore count budget exceeded"
        |"native retained external root budget exceeded"
        |"native scope activation budget exceeded"
        |"native terminal exit budget exceeded"
        |"native terminal path budget exceeded"
        |"native uncertain effect budget exceeded"
        |"object copy edge budget exceeded"
        |"object field count budget exceeded"
        |"object identity budget exceeded"
        |"object mutation budget exceeded"
        |"object observation count budget exceeded"
        |"object observation depth budget exceeded"
        |"object observation field budget exceeded"
        |"source.native_budget: shared operation step budget exceeded"
)}
impl Builder {
    fn native_recover_budget(&mut self,reason:&str)->Result<bool> {
        self.control.check().map_err(|error|error.to_string())?;
        if !native_budget_failure(reason){return Ok(false)}
        // A real completed occurrence anchors the retained prefix; never invent a site.
        let occurrence=self.native.facts.control.occurrences.last().cloned().ok_or_else(||reason.to_string())?;
        let branch=self.native.facts.context_branches.get(&occurrence.context).copied().flatten();
        let ctx=Context{file:occurrence.site.file.clone(),occurrence:occurrence.load,name:occurrence.context.clone(),deferred:false,parameters:BTreeMap::new(),conditional_branch:branch};
        self.native_boundary(&occurrence.site,&ctx,NativeBoundaryKind::Budget,reason);
        // The boundary now owns this failure. Finalize the retained prefix without
        // replaying its heap-observation latch; normal control and fact limits remain.
        if self.observation_failure.as_deref()==Some(reason){self.observation_failure=None;}
        Ok(true)
    }
}

#[cfg(test)]
mod native_budget_recovery_tests {
    use super::*;
    use std::{time::{Duration, Instant}};

    #[test]
    fn continuation_predicates_preserve_truth_table_and_distinct_facts() {
        let term=|items:&[(usize,bool)]|items.iter().map(|&(condition,outcome)|NativePredicate{condition,outcome}).collect::<Vec<_>>();
        for mut alternatives in [
            vec![term(&[(0,true),(1,true)]),term(&[(0,true),(1,false)])],
            vec![term(&[(0,true),(1,true)]),term(&[(0,false),(1,false)])],
            vec![term(&[(0,true),(1,true)]),term(&[(0,false),(2,false)])],
            (0..8).map(|bits|term(&(0..3).map(|i|(i,bits&(1<<i)!=0)).collect::<Vec<_>>())).collect(),
        ]{
            let original=alternatives.clone();reduce_predicate_alternatives(&mut alternatives);
            for assignment in 0..8 {
                let holds=|terms:&Vec<Vec<NativePredicate>>|terms.iter().any(|path|path.iter().all(|p|(assignment&(1<<p.condition)!=0)==p.outcome));
                assert_eq!(holds(&alternatives),holds(&original),"{original:?} -> {alternatives:?}");
            }
            if original.len()==8{assert_eq!(alternatives,vec![vec![]]);}
        }
    }

    fn prepared_source(source:String,namespace_count:usize)->(PreparedSnapshot,OperationControl) {
        let control=OperationControl::uncancelable(Instant::now()+Duration::from_secs(30));
        let callables=(0..namespace_count).map(|index|{
            let name=format!("Namespace{index:03}.Noop");
            json!({"name":name,"identity":format!("namespace-callable-{index}"),"effect":"unknown","returnArity":0,"sourceExecution":false,
                "descriptor":{"kind":"function","name":name}})
        }).collect::<Vec<_>>();
        let material=serde_json::to_vec(&json!({"sourceExecution":false,"configExecution":false,"callables":callables})).unwrap();
        let host=TrustedHostEnvironment::from_host_material("namespace-budget-test".into(),false,
            vec![HostMaterialInput{name:"api".into(),expected_sha256:hash(&material),bytes:material}],vec![],&control).unwrap()
            .with_bindings((0..namespace_count).map(|index|HostBindingInput::Callable{
                declaration:HostMaterialRef{material:"api".into(),pointer:format!("/callables/{index}")},rules:None,
            }).collect(),&control).unwrap();
        let files=[("load.lua",source),("admitted.lua","function NeverLoaded() end\n".into())].into_iter().map(|(path,text)|SourceFileInput{
            path:path.into(),content_sha256:hash(text.as_bytes()),original_utf8:text.clone(),analysis_utf8:text,map:SourceMap::Identity,
        }).collect();
        let mut config=Emmyrc::default();config.runtime.version=emmylua_code_analysis::EmmyrcLuaVersion::Lua51;
        let inputs=SourceInputs::admit(SourceInputSpec{
            files,workspace_root:std::env::current_dir().unwrap().join("logical-namespace-budget-test"),
            source_identity:json!({"kind":"namespace-budget-regression"}),phase:"after_load".into(),ordered_occurrences:vec!["load.lua".into()],config,host,
            luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},limits:SourceLimits::research(),
        },&control).unwrap();
        (prepare(inputs,&control).unwrap(),control)
    }

    #[test]
    fn namespace_unknown_calls_share_versions_but_keep_every_effect_and_entry() {
        let namespace_count=246;
        let source=format!("Marker=0\n{}Marker=1\nlocal function After() return 7 end\n","UnknownCall()\n".repeat(20));
        let (prepared,control)=prepared_source(source,namespace_count);
        assert_eq!(prepared.inputs().file_names().count(),2);
        assert_eq!(prepared.files().len(),2);

        // Seeding the complete host environment creates no source mutation facts.
        let seed=prepared.build_plan("host-seed", "after_load", LoadPlan::Known(vec![]), None, None, &control).unwrap();
        assert!(seed.object_mutations.is_empty());
        assert!(seed.native_invocations.as_ref().unwrap().control.complete);
        assert_eq!(seed.host_runtime.as_ref().unwrap().bindings.len(),namespace_count);

        let EntryResolution::Unique{candidate}=prepared.entry_candidates("load.lua",prepared.inputs().original("load.lua").unwrap().lines().position(|line|line.contains("function After(")).unwrap()+1,&control).unwrap()else{panic!("unique explicit entry")};
        let graph=prepared.build_plan("namespace-entry","after_load",LoadPlan::Known(vec!["load.lua".into()]),Some(EntryRequest{
            candidate:candidate.id.clone(),load_occurrence:Some(0),creation_context:None,arguments:EntryArguments::Exact(vec![]),
        }),None,&control).unwrap();
        let facts=graph.native_invocations.as_ref().unwrap();
        assert!(facts.control.complete);
        assert!(!facts.control.truncated);
        assert!(facts.control.boundaries.is_empty());
        assert_eq!(graph.object_mutations.len(),namespace_count);
        for (index,mutation) in graph.object_mutations.iter().enumerate(){
            assert_eq!(mutation.observation,index+1);
            assert_eq!(mutation.kind,"unknown");
            assert!(mutation.object_id<namespace_count);
            assert_eq!(mutation.site.text(prepared.inputs()).unwrap(),"UnknownCall()");
        }
        assert_eq!(graph.calls.len(),20);
        assert!(graph.calls.iter().all(|call|call.site.text(prepared.inputs()).unwrap()=="UnknownCall()"));
        let suffix=prepared.inputs().original("load.lua").unwrap().find("Marker=1").unwrap() as u32;
        assert!(graph.writes.iter().any(|write|write.site.start==suffix));
        let host=graph.host_runtime.as_ref().unwrap();
        assert_eq!(host.bindings.len(),namespace_count);
        assert_eq!(host.namespace_fields.len(),namespace_count);
        assert_eq!(host.calls.len(),20);
        assert!(!host.transfers.is_empty());
        assert!(host.transfers.iter().any(|transfer|transfer.kind=="storage_write"));
        assert_eq!(facts.escape_events.len(),20);
        assert_eq!(facts.unknown_call_footprints.len(),1);
        let footprint=&facts.unknown_call_footprints[0];
        assert_eq!(footprint.objects,(0..namespace_count).collect::<Vec<_>>());
        assert_eq!(footprint.cells,vec!["global:Marker"]);
        assert!(facts.escape_events.iter().all(|event|event.footprint==0&&event.application_complete));
        assert_eq!(facts.escape_events[0].observation_before,0);
        assert!(facts.escape_events[1..].iter().all(|event|event.observation_before==namespace_count&&event.observation_after==namespace_count));
        assert_eq!(facts.entries.len(),1);
        let entry=&facts.entries[0];
        assert_eq!(entry.request.candidate,candidate.id);
        assert!(entry.effects_complete);
        assert_eq!(entry.return_facts.len(),1);
        assert!(matches!(graph.uses[entry.return_facts[0]].decision,Decision::Bound(_)));
        prepared.freeze(&graph,NativeViewSelection::Entry(entry.frame.unwrap()),&control).unwrap();
        replay_effects_and_views(&graph);
    }

    #[test]
    fn changing_objects_retain_bounded_prefix_and_host_facts() {
        // Each fresh table has 108 actual writes, then escapes through the call.
        // Their unavailable values add no reachable objects or storage-read
        // transfers. The 35th call reaches 4096 real object mutations.
        let fields=(0..108).map(|index|format!("f{index}=UnknownValue")).collect::<Vec<_>>().join(",");
        let source=format!("local target\n{}Suffix()\n",format!("target={{{fields}}}\nUnknownCall(target)\n").repeat(40));
        let (prepared,control)=prepared_source(source,247);
        let graph=prepared.build_plan("changing-prefix", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &control).unwrap();
        let facts=graph.native_invocations.as_ref().unwrap();
        assert!(!facts.control.complete);
        assert!(facts.control.truncated);
        assert_eq!(facts.control.coverage,"verified_prefix_with_explicit_boundaries");
        let boundary=facts.control.boundaries.iter().find(|boundary|boundary.reason=="object mutation budget exceeded").unwrap();
        assert!(matches!(boundary.kind,NativeBoundaryKind::Budget));
        assert!(boundary.truncated);
        assert_eq!(boundary.site,facts.control.occurrences[boundary.occurrence.unwrap()].site);
        assert_eq!(boundary.site.text(prepared.inputs()).unwrap(),"UnknownCall(target)");
        assert_eq!(graph.object_mutations.len(),OBJECT_MUTATION_LIMIT);
        assert_eq!(graph.object_mutations.iter().filter(|mutation|mutation.kind=="field_write").count(),35*108);
        assert_eq!(graph.calls.len(),35);
        assert!(graph.calls.iter().all(|call|call.site.text(prepared.inputs()).unwrap()=="UnknownCall(target)"));
        let host=graph.host_runtime.as_ref().unwrap();
        assert_eq!(host.bindings.len(),247);
        assert_eq!(host.calls.len(),34);
        assert!(!host.transfers.is_empty());
        assert_eq!(facts.escape_events.len(),35);
        assert!(facts.escape_events[..34].iter().all(|event|event.application_complete));
        assert!(!facts.escape_events[34].application_complete);
        assert!(prepared.freeze(&graph,NativeViewSelection::SourceTemplates,&control).err().unwrap().to_string().contains("prefix graph requires boundary review"));
        let queries=prepared.source_queries(&control).unwrap().with_execution(&graph,QuerySelector::AllOccurrences,&control).unwrap();
        let page=queries.calls(QueryTarget::All,PageRequest::default()).unwrap();
        assert_eq!(page.rows.len(),35);
        assert_eq!(page.coverage,"verified_prefix_with_explicit_boundaries");
        replay_effects_and_views(&graph);
    }

    fn use_at<'a>(graph:&'a Graph,inputs:&SourceInputs,marker:&str,token:&str)->&'a UseFact {
        let source=inputs.original("load.lua").unwrap();
        let start=(source.find(marker).unwrap()+marker.rfind(token).unwrap()) as u32;
        let matches=graph.uses.iter().filter(|fact|fact.site.file=="load.lua"&&fact.site.start==start&&fact.site.end==start+token.len() as u32).collect::<Vec<_>>();
        assert_eq!(matches.len(),1,"{marker}");matches[0]
    }

    #[test]
    fn intervening_write_invalidates_again_without_changing_frozen_old_views() {
        let source="local box={x=1}\nlocal prior=box\nUnknownCall(box)\nlocal lost=box.x\nbox.x=2\nlocal restored=box\nlocal restored_value=box.x\nUnknownCall()\nlocal lost_again=box.x\nlocal later=box\nUnknownCall()\nlocal same=box\n";
        let (prepared,control)=prepared_source(source.into(),0);
        let graph=prepared.build_plan("intervening-write", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &control).unwrap();
        assert!(graph.native_invocations.as_ref().unwrap().control.complete);
        let prior=use_at(&graph,prepared.inputs(),"local prior=box","box");
        let restored=use_at(&graph,prepared.inputs(),"local restored=box","box");
        let later=use_at(&graph,prepared.inputs(),"local later=box","box");
        let same=use_at(&graph,prepared.inputs(),"local same=box","box");
        let old=&graph.object_views[prior.object_view.unwrap()];
        let repaired=&graph.object_views[restored.object_view.unwrap()];
        let lost=&graph.object_views[later.object_view.unwrap()];
        assert!(matches!(old.fields[0].selected_value,Decision::Bound(_)));
        assert!(matches!(repaired.fields[0].selected_value,Decision::Bound(_)));
        assert_ne!(old.fields[0].selected_value,repaired.fields[0].selected_value);
        assert_eq!(old.fields[0].destination_write,graph.writes.iter().find(|write|write.site.text(prepared.inputs()).unwrap()=="x=1").unwrap().id);
        assert_eq!(repaired.fields[0].destination_write,graph.writes.iter().find(|write|write.site.text(prepared.inputs()).unwrap()=="box.x").unwrap().id);
        assert!(matches!(lost.fields[0].selected_value,Decision::Unavailable(_)));
        assert_eq!(later.object_view,same.object_view);
        for marker in ["local lost=box.x","local lost_again=box.x"]{
            assert_eq!(use_at(&graph,prepared.inputs(),marker,"box.x").decision,Decision::Unavailable("ordinary_unknown_call_reachable_effect".into()));
        }
        assert!(matches!(use_at(&graph,prepared.inputs(),"local restored_value=box.x","box.x").decision,Decision::Bound(_)));
        let facts=graph.native_invocations.as_ref().unwrap();
        assert_eq!(facts.escape_events.len(),3);
        assert_eq!(facts.unknown_call_footprints.len(),1);
        assert_eq!(graph.object_mutations.iter().filter(|mutation|mutation.kind=="unknown").count(),2);
        assert_eq!(facts.escape_events[2].observation_before,facts.escape_events[2].observation_after);
        let frozen=prepared.freeze(&graph,NativeViewSelection::SourceTemplates,&control).unwrap();
        assert_eq!(frozen.shapes[&PreparedSnapshot::native_view_key(&old.key)].proof,old.proof_id);
        assert_eq!(frozen.shapes[&PreparedSnapshot::native_view_key(&repaired.key)].proof,repaired.proof_id);
        assert_eq!(frozen.shapes[&PreparedSnapshot::native_view_key(&lost.key)].proof,lost.proof_id);
        replay_effects_and_views(&graph);
    }

    #[test]
    fn captured_cell_rebind_extends_the_full_footprint_and_preserves_other_locals() {
        let source="local box={x=1}\nlocal untouched={x=9}\nlocal function Read() return box end\nUnknownCall(Read)\nbox={x=2}\nlocal current=box\nUnknownCall()\nlocal lost=current.x\nUnknownCall()\nlocal still_lost=current.x\nlocal safe=untouched.x\n";
        let (prepared,control)=prepared_source(source.into(),0);
        let graph=prepared.build_plan("escaped-cell-rebind", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &control).unwrap();
        let facts=graph.native_invocations.as_ref().unwrap();
        assert!(facts.control.complete);
        assert_eq!(facts.escape_events.len(),3);
        let first=&facts.unknown_call_footprints[facts.escape_events[0].footprint];
        let second=&facts.unknown_call_footprints[facts.escape_events[1].footprint];
        let captured=facts.closures.iter().find(|closure|!closure.captures.is_empty()).unwrap().captures.values().next().unwrap();
        assert_eq!(first.cells,vec![captured.clone()]);
        assert_eq!(second.cells,first.cells);
        assert_eq!(first.objects,vec![0]);
        assert_eq!(second.objects,vec![0,2]);
        assert_eq!(facts.escape_events[1].footprint,facts.escape_events[2].footprint);
        assert_eq!(facts.escape_events[2].observation_before,facts.escape_events[2].observation_after);
        assert!(facts.escaped_roots.contains(&NativeEscapeRoot::Cell(captured.clone())));
        for marker in ["local lost=current.x","local still_lost=current.x"]{
            assert_eq!(use_at(&graph,prepared.inputs(),marker,"current.x").decision,Decision::Unavailable("ordinary_unknown_call_reachable_effect".into()));
        }
        assert!(matches!(use_at(&graph,prepared.inputs(),"local safe=untouched.x","untouched.x").decision,Decision::Bound(_)));
        replay_effects_and_views(&graph);
    }

    #[test]
    fn branch_checkpoints_restore_real_changes_and_reuse_unchanged_versions() {
        for (name,branch,changed) in [
            ("same-branches","if Flag then UnknownCall() else UnknownCall() end",false),
            ("written-branch","if Flag then box.x=2; UnknownCall() else UnknownCall() end",true),
        ]{
            let source=format!("local box={{x=1}}\nUnknownCall(box)\n{branch}\nlocal after=box.x\n");
            let (prepared,control)=prepared_source(source,0);
            let graph=prepared.build_plan(name, &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &control).unwrap();
            let facts=graph.native_invocations.as_ref().unwrap();
            assert!(facts.control.complete);
            assert_eq!(facts.escape_events.len(),3);
            assert_eq!(facts.unknown_call_footprints.len(),1);
            assert!(!facts.checkpoints.is_empty());
            assert!(!facts.restores.is_empty());
            assert_eq!(graph.object_mutations.iter().any(|mutation|mutation.kind=="checkpoint_restore"),changed);
            if !changed{assert_eq!(graph.object_mutations.len(),3);}
            assert!(matches!(use_at(&graph,prepared.inputs(),"local after=box.x","box.x").decision,Decision::Unavailable(_)));
            assert!(facts.escape_events.iter().all(|event|event.application_complete));
            replay_effects_and_views(&graph);
        }
    }

    #[test]
    fn checkpoint_storage_shares_namespace_states_and_preserves_explicit_entry() {
        let source=format!("local box={{x=0}}\n{}\nlocal function After() return box.x end\n",(0..12).map(|n|format!("if Flag then box.x={} else box.x={} end\n",n+1,n+2)).collect::<String>());
        let(prepared,control)=prepared_source(source,246);
        let EntryResolution::Unique{candidate}=prepared.entry_candidates("load.lua",prepared.inputs().original("load.lua").unwrap().lines().position(|line|line.contains("function After(")).unwrap()+1,&control).unwrap()else{panic!("unique entry")};
        let graph=prepared.build_plan("shared-checkpoints","after_load",LoadPlan::Known(vec!["load.lua".into()]),Some(EntryRequest{candidate:candidate.id,load_occurrence:Some(0),creation_context:None,arguments:EntryArguments::Exact(vec![])}),None,&control).unwrap();
        let facts=graph.native_invocations.as_ref().unwrap();
        assert!(facts.control.complete,"{:?}",facts.control.boundaries);
        assert_eq!(facts.entries.len(),1);assert!(facts.checkpoints.len()>12);
        assert!(facts.checkpoints.windows(2).all(|pair|Arc::ptr_eq(&pair[0].objects[0],&pair[1].objects[0])));
        for checkpoint in &facts.checkpoints{assert_eq!(serde_json::to_value(&checkpoint.cells).unwrap(),json!(checkpoint.cells.materialize()));}
        replay_effects_and_views(&graph);
    }

    #[test]
    fn checkpoint_storage_still_stops_at_real_changed_heap_budget() {
        // Each changed object version owns its complete field map. Keep the
        // number of writes small so the fixture isolates checkpoint storage.
        let mut source=format!("local box={{{}}}\n",(0..100).map(|i|format!("v{i}=0")).collect::<Vec<_>>().join(","));
        for round in 1..=32{source.push_str(&format!("if Flag then box.v0={round} end\n"));}
        source.push_str("AfterBudget=1\n");
        let(prepared,control)=prepared_source(source,0);
        let graph=prepared.build_plan("changed-checkpoint-boundary", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &control).unwrap();
        let facts=graph.native_invocations.as_ref().unwrap();
        assert!(!facts.control.complete);assert!(facts.control.truncated);
        assert!(facts.control.boundaries.iter().any(|boundary|boundary.reason=="native checkpoint slot budget exceeded"),"{:?}",facts.control.boundaries);
        assert!(facts.checkpoints.len()<INVOCATION_CHECKPOINT_LIMIT);
    }

    #[test]
    fn research_declarations_and_templates_do_not_consume_invocation_instances() {
        let source=(0..320).map(|index|format!("function F{index}() return {index} end\n")).collect::<String>();
        let(prepared,control)=prepared_source(source,0);
        let EntryResolution::Unique{candidate}=prepared.entry_candidates("load.lua",320,&control).unwrap()else{panic!("unique source entry")};
        let graph=prepared.build_plan("many-module-declarations","after_load",LoadPlan::Known(vec!["load.lua".into()]),Some(EntryRequest{candidate:candidate.id,load_occurrence:Some(0),creation_context:None,arguments:EntryArguments::Exact(vec![])}),None,&control).unwrap();
        let facts=graph.native_invocations.as_ref().unwrap();
        assert!(facts.control.complete,"{:?}",facts.control.boundaries);
        assert_eq!(facts.closures.len(),320);assert_eq!(facts.entries.len(),1);
        assert_eq!(facts.frames.iter().filter(|frame|frame.closure.is_none()).count(),320);
        assert_eq!(facts.frames.iter().filter(|frame|frame.closure.is_some()).count(),1);
        assert!(facts.entries[0].effects_complete);
    }

    #[test]
    fn research_factory_instances_still_share_original_closure_budget() {
        let source=format!("local function Make() return function() return 1 end, function() return 2 end end\n{}", "local a,b=Make()\n".repeat(129));
        let(prepared,control)=prepared_source(source,0);
        let graph=prepared.build_plan("dynamic-instance-budget", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &control).unwrap();
        let facts=graph.native_invocations.as_ref().unwrap();
        assert!(!facts.control.complete);assert!(facts.control.truncated);
        assert!(facts.control.boundaries.iter().any(|boundary|boundary.reason=="native closure instance budget exceeded"),"{:?}",facts.control.boundaries);
        assert_eq!(facts.closures.len(),1+INVOCATION_CLOSURE_LIMIT);
    }

    #[test]
    fn research_projects_all_admitted_storage_facts() {
        let source=format!("local original=7\n{}",(0..2200).map(|index|format!("local copy{index}=original\n")).collect::<String>());
        let(prepared,control)=prepared_source(source,0);
        let graph=prepared.build_plan("common-storage-projection", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &control).unwrap();
        let native=graph.native_invocations.as_ref().unwrap();
        assert!(native.control.complete,"{:?}",native.control.boundaries);
        let facts=graph.host_runtime.as_ref().unwrap();
        assert_eq!(facts.transfers.iter().filter(|fact|fact.kind=="storage_read").count(),2200);
        assert_eq!(facts.transfers.iter().filter(|fact|fact.kind=="storage_write").count(),2201);
        assert!(facts.transfers.iter().all(|fact|matches!(fact.selected,Decision::Bound(_))));
    }

    #[test]
    fn research_views_share_unchanged_shapes_and_track_nested_alias_writes() {
        let mut source="local stable={child={x=1}}\nstable.child.back=stable\nlocal changing={y=0}\nlocal original=stable.child.x\n".to_string();
        for index in 0..180{source.push_str(&format!("changing.y={index}\nlocal copy{index}=stable.child.x\n"));}
        source.push_str("local before=stable.child.x\nlocal alias=stable.child\nalias.x=2\nlocal after=stable.child.x\n");
        let(prepared,control)=prepared_source(source,0);
        let graph=prepared.build_plan("reachable-shape-versions", &prepared.inputs().phase, LoadPlan::Known(prepared.inputs().ordered.clone()), None, None, &control).unwrap();
        assert!(graph.native_invocations.as_ref().unwrap().control.complete,"{:?}",graph.native_invocations.as_ref().unwrap().control.boundaries);
        let original=use_at(&graph,prepared.inputs(),"local original=stable.child.x","stable");
        let before=use_at(&graph,prepared.inputs(),"local before=stable.child.x","stable");
        let after=use_at(&graph,prepared.inputs(),"local after=stable.child.x","stable");
        assert_eq!(original.object_view,before.object_view);
        assert_ne!(before.object_view,after.object_view);
        assert_ne!(use_at(&graph,prepared.inputs(),"local before=stable.child.x","stable.child.x").decision,use_at(&graph,prepared.inputs(),"local after=stable.child.x","stable.child.x").decision);
        replay_effects_and_views(&graph);
    }

    #[derive(Clone,Default,Debug,PartialEq,Eq)]
    struct ReplayObject {
        fields:BTreeMap<String,usize>,
        unknown:Option<String>,
        unknown_observation:usize,
    }

    // Reconstruct object states from persisted evidence, not Builder or its
    // idempotence predicate. Checkpoint restoration follows its explicit record.
    fn replay_at(graph:&Graph,observation:usize)->BTreeMap<usize,ReplayObject> {
        let mut states=BTreeMap::<usize,ReplayObject>::new();
        if let Some(host)=&graph.host_runtime{
            for field in &host.namespace_fields{
                states.entry(field.object).or_insert_with(||ReplayObject{unknown:Some("environment_namespace_enumeration_unavailable".into()),..Default::default()});
            }
        }
        for mutation in graph.object_mutations.iter().take_while(|mutation|mutation.observation<=observation){
            match mutation.kind{
                "create"=>{assert!(states.insert(mutation.object_id,ReplayObject::default()).is_none());},
                "field_write"=>{
                    let write=&graph.writes[mutation.write.unwrap()];
                    let prefix=format!("object:{}:field:",mutation.object_id);
                    let key=write.cell.strip_prefix(&prefix).unwrap();
                    assert_eq!(mutation.site,write.site);
                    states.get_mut(&mutation.object_id).unwrap().fields.insert(key.into(),write.id);
                },
                "unknown"=>{
                    let state=states.get_mut(&mutation.object_id).unwrap();
                    state.unknown=mutation.reason.clone();state.unknown_observation=mutation.observation;
                },
                "checkpoint_restore"=>{
                    let id:usize=mutation.reason.as_ref().unwrap().strip_prefix("checkpoint:").unwrap().parse().unwrap();
                    let checkpoint=&graph.native_invocations.as_ref().unwrap().checkpoints[id];
                    let state=&checkpoint.objects[mutation.object_id];
                    states.insert(mutation.object_id,ReplayObject{fields:state.fields.clone(),unknown:state.unknown.clone(),unknown_observation:state.unknown_observation});
                },
                kind=>panic!("unrecognized mutation {kind}"),
            }
        }
        states
    }

    fn replay_effects_and_views(graph:&Graph) {
        let facts=graph.native_invocations.as_ref().unwrap();
        let writes=graph.object_mutations.iter().filter_map(|mutation|mutation.write.map(|write|(write,mutation.observation))).collect::<BTreeMap<_,_>>();
        assert!(graph.object_mutations.len()<=OBJECT_MUTATION_LIMIT);
        for (index,mutation) in graph.object_mutations.iter().enumerate(){assert_eq!(mutation.observation,index+1);}
        for (id,footprint) in facts.unknown_call_footprints.iter().enumerate(){
            assert_eq!(footprint.id,id);
            assert!(footprint.objects.windows(2).all(|pair|pair[0]<pair[1]));
            assert!(footprint.cells.windows(2).all(|pair|pair[0]<pair[1]));
            assert!(footprint.objects.len()+footprint.cells.len()<=INVOCATION_FOOTPRINT_LIMIT);
            assert_eq!(footprint.proof_id,digest(&(graph.snapshot_key.as_str(),"unknown_call_footprint_v1",&footprint.objects,&footprint.cells)));
        }
        let mut proofs=std::collections::BTreeSet::new();
        for event in &facts.escape_events{
            let call=&graph.calls[event.call];
            assert_eq!(event.site,call.site);assert_eq!(event.context,call.context);
            assert!(event.observation_before<=event.observation_after);
            let footprint=&facts.unknown_call_footprints[event.footprint];
            assert_eq!(event.proof_id,digest(&(graph.snapshot_key.as_str(),"unknown_call_effect_v1",event.call,&event.site,&event.context,event.roots_before,&event.retained,event.roots_after,&footprint.proof_id,event.observation_before,event.observation_after,event.application_complete)));
            assert!(proofs.insert(&event.proof_id));
            let before=replay_at(graph,event.observation_before);
            let after=replay_at(graph,event.observation_after);
            let mutations=graph.object_mutations.iter().filter(|mutation|event.observation_before<mutation.observation&&mutation.observation<=event.observation_after).collect::<Vec<_>>();
            for mutation in &mutations{
                assert_eq!(mutation.kind,"unknown");
                assert_eq!(mutation.site,event.site);
                assert!(footprint.objects.contains(&mutation.object_id));
            }
            if event.application_complete{
                for object in &footprint.objects{
                    let old=&before[object];let new=&after[object];
                    assert_eq!(old.fields,new.fields);
                    assert_eq!(new.unknown.as_deref(),Some("ordinary_unknown_call_reachable_effect"));
                    assert!(new.unknown_observation>0);
                    // Every field is unavailable after this effect. Any state
                    // change is present in the mutation log at this call site.
                    assert!(new.fields.values().all(|write|writes[write]<=new.unknown_observation));
                    if old!=new{assert_eq!(mutations.iter().filter(|mutation|mutation.object_id==*object).count(),1);}
                    else{assert!(mutations.iter().all(|mutation|mutation.object_id!=*object));}
                }
            }
            for cell in &footprint.cells{
                assert!(graph.writes.iter().any(|write|&write.cell==cell&&write.site==event.site&&write.context==event.context&&write.decision==Decision::Unavailable("ordinary_unknown_call_reachable_cell_rebind".into())));
            }
        }
        for view in &graph.object_views{
            let states=replay_at(graph,view.key.observation);let state=&states[&view.key.object_id];
            assert_eq!(state.fields.keys().collect::<Vec<_>>(),view.fields.iter().map(|field|&field.key).collect::<Vec<_>>());
            for field in &view.fields{
                let write=state.fields[&field.key];
                assert_eq!(field.destination_write,write);
                let selected=if let Some(reason)=&state.unknown{if writes[&write]<=state.unknown_observation{Decision::Unavailable(reason.clone())}else{graph.writes[write].decision.clone()}}else{graph.writes[write].decision.clone()};
                assert_eq!(field.selected_value,selected);
            }
            assert_eq!(view.proof_id,digest(&(&view.key,&view.fields,&view.enumeration)));
        }
    }
}
