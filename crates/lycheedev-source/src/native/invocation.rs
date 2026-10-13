// Native-only abstract invocation evaluation. Source Lua is never executed.
// The legacy V7 graph retains its original evaluator and serialized shape.
const INVOCATION_DEPTH_LIMIT: usize = 32;
const INVOCATION_FRAME_LIMIT: usize = 256;
const INVOCATION_CLOSURE_LIMIT: usize = 256;
const INVOCATION_CAPTURE_LIMIT: usize = 128;
const INVOCATION_CAPTURE_EDGE_LIMIT: usize = 2048;
const INVOCATION_PARAMETER_LIMIT: usize = 32;
const INVOCATION_RETURN_LIMIT: usize = 32;
const INVOCATION_FOOTPRINT_LIMIT: usize = 4096;
const INVOCATION_ESCAPE_ROOT_LIMIT: usize = 4096;
const INVOCATION_UNCERTAIN_EFFECT_LIMIT: usize = 4096;
const INVOCATION_CHECKPOINT_LIMIT: usize = 128;
const INVOCATION_CHECKPOINT_SLOT_LIMIT: usize = 4096;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag="kind", rename_all="snake_case")]
pub enum NativeQueryView { Load {occurrence:usize}, Xml {action:usize}, Static {function:usize}, Invocation {frame:usize}, Entry {frame:usize} }
#[derive(Clone, Debug, Serialize)]
pub struct NativeClosureInstance {
    pub id:usize, pub function:usize, pub value:usize, pub creation_context:String, pub load_occurrence:usize,
    pub captures:BTreeMap<String,String>, pub method:bool, pub self_parameter:Option<Site>,
}
#[derive(Clone, Debug, Serialize)]
pub struct NativeFrame {
    pub id:usize, pub function:usize, pub closure:Option<usize>, pub call:Option<usize>,
    #[serde(skip_serializing_if="omit_source_invocation_origin")]
    pub origin:Option<NativeInvocationOrigin>,
    pub context:String, pub captures:BTreeMap<String,String>, pub return_facts:Vec<usize>,
    pub implicit_self:Option<String>, pub effects_complete:bool, pub return_paths:Vec<NativeTerminalPath>,
}
#[derive(Clone,Debug,Serialize,PartialEq,Eq,PartialOrd,Ord)]
#[serde(tag="kind",content="identity",rename_all="snake_case")]
pub enum NativeEscapeRoot { Object(usize), Closure(usize), Cell(String) }
#[derive(Clone,Debug,Serialize)]
pub struct NativeEscapeEvent {
    pub call:usize, pub context:String, pub roots_before:usize,
    pub retained:Vec<NativeEscapeRoot>, pub roots_after:usize,
    pub site:Site, pub footprint:usize, pub observation_before:usize, pub observation_after:usize,
    pub application_complete:bool, pub proof_id:String,
}
#[derive(Clone,Debug,Serialize)]
pub struct NativeUnknownCallFootprint {
    pub id:usize, pub objects:Vec<usize>, pub cells:Vec<String>, pub proof_id:String,
}
#[derive(Clone,Debug,Serialize)]
pub struct NativeUncertainEffect {
    pub id:usize, pub site:Site, pub context:String, pub receiver_fact:usize,
    pub value:Decision, pub inputs:Vec<usize>, pub reason:String, pub proof_id:String,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct NativeInvocationFacts {
    #[serde(skip_serializing_if="Vec::is_empty")]
    pub entries:Vec<NativeEntryFact>,
    #[serde(skip_serializing_if="Vec::is_empty")]
    pub execution_boundaries:Vec<ExecutionBoundary>,
    pub control:NativeControlFacts,
    pub storages:Vec<StorageFact>, pub capture_facts:Vec<CaptureFact>, pub use_storages:Vec<UseStorageFact>,
    pub closures:Vec<NativeClosureInstance>, pub frames:Vec<NativeFrame>,
    pub query_views:BTreeMap<String,NativeQueryView>,
    pub context_branches:BTreeMap<String,Option<usize>>,
    pub global_environment_open:bool,
    pub checkpoints:Vec<NativeHeapCheckpoint>, pub restores:Vec<NativeRestoreFact>,
    pub escaped_roots:std::collections::BTreeSet<NativeEscapeRoot>, pub escape_events:Vec<NativeEscapeEvent>, pub uncertain_effects:Vec<NativeUncertainEffect>,
    #[serde(skip_serializing_if="Vec::is_empty")]
    pub unknown_call_footprints:Vec<NativeUnknownCallFootprint>,
}
#[derive(Default)]
struct NativeInvocationState {
    host:HostRuntimeState,
    typed_cells:std::cell::RefCell<BTreeMap<String,StorageOwner>>,
    facts:NativeInvocationFacts,
    context_frames:BTreeMap<String,usize>,
    closure_values:BTreeMap<usize,usize>,
    definitions:BTreeMap<String,usize>,
    expanded_results:BTreeMap<usize,Vec<usize>>,
    boolean_values:BTreeMap<usize,bool>,
    numeric_values:BTreeMap<usize,f64>,
    unknown_footprint_ids:BTreeMap<String,usize>,
    contexts:BTreeMap<String,NativeContextState>,
    loop_iterations:usize,
    capture_edges:usize, checkpoint_slots:usize,
    module_closures:std::collections::BTreeSet<(usize,usize)>, dynamic_closures:usize, invocation_frames:usize,
    stack:Vec<usize>,
}

#[derive(Clone,Debug,Serialize)]
pub struct NativeHeapCheckpoint {
    pub id:usize, pub site:Site, pub context:String, pub observation:usize,
    pub cells:NativeCheckpointCells,
    #[serde(serialize_with="serialize_checkpoint_objects")]
    pub objects:Vec<Arc<ObjectState>>, pub global_open:bool,
    #[serde(serialize_with="serialize_checkpoint_roots")]
    pub escaped_roots:Arc<std::collections::BTreeSet<NativeEscapeRoot>>,
}
fn serialize_checkpoint_objects<S:serde::Serializer>(objects:&[Arc<ObjectState>],serializer:S)->std::result::Result<S::Ok,S::Error>{
    serializer.collect_seq(objects.iter().map(Arc::as_ref))
}
fn serialize_checkpoint_roots<S:serde::Serializer>(roots:&Arc<std::collections::BTreeSet<NativeEscapeRoot>>,serializer:S)->std::result::Result<S::Ok,S::Error>{
    roots.as_ref().serialize(serializer)
}
// Checkpoints retain changed cell versions rather than another complete copy of
// the same heap. The bounded checkpoint chain still serializes as the full map.
#[derive(Clone,Debug,Default)]
pub struct NativeCheckpointCells(Option<Arc<NativeCellDelta>>);
#[derive(Debug)]
struct NativeCellDelta { previous:NativeCheckpointCells, changes:BTreeMap<String,Option<usize>> }
impl NativeCheckpointCells {
    fn materialize(&self)->BTreeMap<String,usize>{
        let mut chain=Vec::new();let mut next=self;
        while let Some(delta)=&next.0{chain.push(delta);next=&delta.previous;}
        let mut result=BTreeMap::new();
        for delta in chain.into_iter().rev(){for(key,value)in &delta.changes{match value{Some(value)=>{result.insert(key.clone(),*value);},None=>{result.remove(key);}}}}
        result
    }
    fn changed_slots(before:&BTreeMap<String,usize>,after:&BTreeMap<String,usize>)->usize{
        after.iter().filter(|(key,value)|before.get(*key)!=Some(*value)).count()+before.keys().filter(|key|!after.contains_key(*key)).count()
    }
    fn capture(&self,before:&BTreeMap<String,usize>,after:&BTreeMap<String,usize>)->Self{
        let changes=after.iter().filter(|(key,value)|before.get(*key)!=Some(*value)).map(|(key,value)|(key.clone(),Some(*value)))
            .chain(before.keys().filter(|key|!after.contains_key(*key)).map(|key|(key.clone(),None))).collect::<BTreeMap<_,_>>();
        if changes.is_empty(){self.clone()}else{Self(Some(Arc::new(NativeCellDelta{previous:self.clone(),changes})))}
    }
}
impl Serialize for NativeCheckpointCells {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->std::result::Result<S::Ok,S::Error>{self.materialize().serialize(serializer)}
}
#[derive(Clone,Debug,Serialize)]
pub struct NativeRestoreFact {
    pub checkpoint:usize, pub site:Site, pub context:String, pub observation:usize,
    pub restored_objects:Vec<usize>, pub retained_escape_roots:std::collections::BTreeSet<NativeEscapeRoot>,
}

impl Builder {
    fn native_register_view(&mut self,ctx:&Context) {
        self.native.facts.context_branches.insert(ctx.name.clone(),ctx.conditional_branch);
        self.native.facts.query_views.entry(ctx.name.clone()).or_insert(NativeQueryView::Load{occurrence:ctx.occurrence});
    }
    fn native_child_context(&mut self,ctx:&Context,name:String,branch:Option<usize>)->Context {
        let mut child=ctx.clone();child.name=name;child.conditional_branch=branch;
        let state=self.native.contexts.get(&ctx.name).cloned().unwrap_or_default();
        if let Some(id)=state.occurrence{self.native.facts.control.context_occurrences.insert(child.name.clone(),id);}
        self.native.contexts.insert(child.name.clone(),state);
        let view=self.native.facts.query_views.get(&ctx.name).cloned().unwrap_or(NativeQueryView::Load{occurrence:ctx.occurrence});
        self.native.facts.query_views.insert(child.name.clone(),view);
        self.native.facts.context_branches.insert(child.name.clone(),child.conditional_branch);
        if let Some(frame)=self.native.context_frames.get(&ctx.name).copied(){self.native.context_frames.insert(child.name.clone(),frame);}
        child
    }
    fn native_local_key(&self,declaration:&str,ctx:&Context)->String {
        let frame=self.native.context_frames.get(&ctx.name).copied();
        let mut cell=match frame{Some(frame)=>format!("frame:{frame}:local:{declaration}"),None=>format!("local:{declaration}:load:{}",ctx.occurrence)};
        if let Some(activation)=self.native.contexts.get(&ctx.name).and_then(|state|state.activations.get(declaration)){cell.push_str(&format!(":activation:{activation}"));}
        if let Some(site)=self.harvest.declaration_sites.get(declaration){self.native.typed_cells.borrow_mut().entry(cell.clone()).or_insert_with(||StorageOwner::Local{declaration:site.clone(),load_occurrence:ctx.occurrence,frame});}
        cell
    }
    fn native_cell_for_decl(&self,declaration:&str,ctx:&Context)->String {
        if let Some(frame)=self.native.context_frames.get(&ctx.name)
            && let Some(cell)=self.native.facts.frames[*frame].captures.get(declaration){return cell.clone()}
        self.native_local_key(declaration,ctx)
    }
    fn native_key(&self,site:&Site,ctx:&Context)->String {
        if let Some(declaration)=self.harvest.local_reads.get(&site.id()){return self.native_cell_for_decl(declaration,ctx)}
        if site.text(&self.inputs).expect("admitted parser Site")=="self" && let Some(frame)=self.native.context_frames.get(&ctx.name)
            && let Some(cell)=&self.native.facts.frames[*frame].implicit_self{return cell.clone()}
        let name=site.text(&self.inputs).expect("admitted parser Site").to_string();let cell=format!("global:{name}");
        self.native.typed_cells.borrow_mut().entry(cell.clone()).or_insert(StorageOwner::Global{name});cell
    }
    fn native_read(&self,key:&str,ctx:&Context)->(Decision,Vec<usize>) {
        if key.starts_with("global:")&&!self.cells.contains_key(key)&&self.native.facts.global_environment_open {
            return (Decision::Unavailable(if self.inputs.host.data.closed||self.graph.host_runtime.is_some(){"unknown_call_open_global_environment"}else{"open_initial_host_environment"}.into()),vec![])
        }
        if !self.cells.contains_key(key) && !(ctx.deferred&&self.graph.phase=="unknown")
            && let Some(value)=self.native.host.initial_globals.get(key){return (Decision::Bound(*value),vec![])}
        self.read(key,ctx)
    }
    fn native_definition_contains(site:&Site,declaration:&str)->bool {
        let Some(rest)=declaration.strip_prefix(&format!("{}:",site.file))else{return false};
        let Some((start,end))=rest.split_once(':')else{return false};
        match(start.parse::<u32>(),end.parse::<u32>()){(Ok(start),Ok(end))=>start>=site.start&&end<=site.end,_=>false}
    }
    fn native_register_function(&mut self,name:String,declaration:Site,closure:LuaClosureExpr,ctx:&Context)->Decision {
        match self.native_register_function_checked(name,declaration,closure,ctx){
            Ok(value)=>value,
            Err(reason)=>{self.observation_failure.get_or_insert(reason.clone());Decision::Unavailable(reason)},
        }
    }
    fn native_register_function_checked(&mut self,name:String,declaration:Site,closure:LuaClosureExpr,ctx:&Context)->Result<Decision> {
        self.budget()?;
        let closure_site=self.inputs.site(&ctx.file,closure.get_range());
        let definition_key=closure_site.id();
        let self_parameter=closure.get_parent::<emmylua_parser::LuaFuncStat>()
            .and_then(|stat|stat.get_func_name()).and_then(|name|match name.to_expr(){
                LuaExpr::IndexExpr(index)=>index.get_index_token().filter(|token|token.is_colon())
                    .map(|token|self.inputs.site(&ctx.file,token.syntax().text_range())),
                _=>None,
            });
        let method=self_parameter.is_some();
        let function=if let Some(id)=self.native.definitions.get(&definition_key){*id}else{
            if self.inputs.corpus.limits.flow.as_ref().is_some_and(|flow|self.graph.functions.len()>=flow.max_functions){return Err("flow function budget exceeded".into())}
            let signature=self.harvest.signatures.get(&definition_key).cloned();
            let pure_return_body=closure.get_block().is_some_and(|block|{
                let stats:Vec<_>=block.get_stats().collect();
                stats.len()==1&&matches!(&stats[0],LuaStat::ReturnStat(stat) if stat.get_expr_list().all(|expr|matches!(expr,LuaExpr::NameExpr(_)|LuaExpr::LiteralExpr(_))))
            });
            let intrinsic=self.harvest.intrinsic_declarations.get(&declaration.id()).cloned();
            let body_check_coverage=if closure.get_block().is_none_or(|block|block.get_stats().all(|stat|matches!(stat,LuaStat::ReturnStat(_)))){"return_expressions_only"}else{"unavailable_non_return_statements"}.to_string();
            let(effect_summary,effect_summary_unavailable_reason)=match self.summarize_parameter_writes(&closure,&ctx.file){Ok(summary)=>(Some(summary),None),Err(reason)=>(None,Some(reason.into()))};
            let id=self.graph.functions.len();
            self.graph.functions.push(Function{id,declaration:declaration.clone(),closure:closure_site.clone(),name,signature_reason:signature.is_none().then(||"missing_or_unsupported_explicit_primitive_signature".into()),signature,intrinsic,pure_return_body,body_check_coverage,load_occurrence:ctx.occurrence,effect_summary,effect_summary_unavailable_reason});
            self.bodies.push((id,closure.clone()));self.native.definitions.insert(definition_key,id);id
        };
        // Whole-source declarations are already bounded by admitted syntax and
        // load occurrences. Count only additional runtime instances against the
        // invocation allowance; a factory or repeated loop still consumes it.
        let module_binding=self.inputs.corpus.limits.research
            &&!self.native.context_frames.contains_key(&ctx.name)
            &&self.native.module_closures.insert((function,ctx.occurrence));
        if !module_binding{
            if self.native.dynamic_closures>=INVOCATION_CLOSURE_LIMIT{return Err("native closure instance budget exceeded".into())}
            self.native.dynamic_closures+=1;
        }
        let mut captures=BTreeMap::new();
        // Include free names used by nested closures: the outer closure must
        // retain their cells so that creating the inner closure later is valid.
        for node in closure.syntax().descendants(){
            self.budget()?;
            let Some(expr)=LuaNameExpr::cast(node)else{continue};
            let site=self.inputs.site(&ctx.file,expr.get_range());
            let Some(lexical)=self.harvest.local_reads.get(&site.id())else{continue};
            if Self::native_definition_contains(&closure_site,lexical)||self_parameter.as_ref().is_some_and(|site|site.id()==*lexical){continue}
            captures.insert(lexical.clone(),self.native_cell_for_decl(lexical,ctx));
            if captures.len()>INVOCATION_CAPTURE_LIMIT{return Err("native closure capture budget exceeded".into())}
        }
        if !module_binding{self.native.capture_edges+=captures.len();}
        if self.native.capture_edges>INVOCATION_CAPTURE_EDGE_LIMIT{return Err("native captured cell edge budget exceeded".into())}
        let decision=self.value(&declaration,ValueKind::Function(function));
        let Decision::Bound(value)=decision else{unreachable!()};
        let id=self.native.facts.closures.len();
        for (declaration,cell) in &captures {let site=self.harvest.declaration_sites.get(declaration).ok_or("capture declaration source identity missing")?;self.native.facts.capture_facts.push(CaptureFact{closure:id,declaration:site.clone(),cell:cell.clone(),creation_context:ctx.name.clone()});}
        self.native.facts.closures.push(NativeClosureInstance{id,function,value,creation_context:ctx.name.clone(),load_occurrence:ctx.occurrence,captures,method,self_parameter});
        self.native.closure_values.insert(value,id);
        Ok(Decision::Bound(value))
    }
    fn native_frame(&mut self,function:usize,closure:Option<usize>,call:Option<usize>,context:String,captures:BTreeMap<String,String>)->Result<usize> {
        self.budget()?;
        // One static template per admitted definition is a source fact, not a
        // call. Concrete source calls and explicit entries share this counter.
        if !self.inputs.corpus.limits.research||closure.is_some(){
            if self.native.invocation_frames>=INVOCATION_FRAME_LIMIT{return Err("native frame budget exceeded".into())}
            self.native.invocation_frames+=1;
        }
        let id=self.native.facts.frames.len();
        self.native.facts.frames.push(NativeFrame{id,function,closure,call,origin:call.map(|call|NativeInvocationOrigin::SourceCall{call}),context:context.clone(),captures,return_facts:vec![],implicit_self:None,effects_complete:true,return_paths:vec![]});
        self.native.context_frames.insert(context.clone(),id);
        self.native.facts.query_views.insert(context,if call.is_some(){NativeQueryView::Invocation{frame:id}}else{NativeQueryView::Static{function}});
        Ok(id)
    }
    fn native_invoke(&mut self,call:usize,caller:&Context,args:&[usize])->Result<(Vec<usize>,bool)> {
        let callee=self.graph.uses[self.graph.calls[call].callee].decision.clone();
        let Decision::Bound(value)=callee else{return Err("invocation without selected closure value".into())};
        let closure_id=*self.native.closure_values.get(&value).ok_or("selected function lacks closure instance")?;
        self.native_invoke_closure(NativeInvocationOrigin::SourceCall{call},closure_id,Some(caller),args)
    }
    fn native_invoke_closure(&mut self,origin:NativeInvocationOrigin,closure_id:usize,caller:Option<&Context>,args:&[usize])->Result<(Vec<usize>,bool)> {
        if let Some(flow)=&self.inputs.corpus.limits.flow {
            let depth=self.native.stack.iter().filter(|frame|self.native.facts.frames[**frame].call.is_some()).count()+usize::from(matches!(origin,NativeInvocationOrigin::SourceCall{..}));
            if depth>flow.max_depth {return Err("flow call depth budget exceeded".into())}
        }
        if self.native.stack.len()>=INVOCATION_DEPTH_LIMIT{return Err("native call depth budget exceeded".into())}
        if self.native.stack.iter().any(|frame|self.native.facts.frames[*frame].closure==Some(closure_id)){
            if let (NativeInvocationOrigin::SourceCall{call},Some(caller))=(&origin,caller){let site=self.graph.calls[*call].site.clone();self.native_boundary(&site,caller,NativeBoundaryKind::Knowledge,"recursive_invocation_requires_summary");}
            return Err("native invocation stopped at explicit control boundary".into())
        }
        let instance=self.native.facts.closures[closure_id].clone();
        let function=self.graph.functions[instance.function].clone();
        let closure=self.bodies.iter().find(|(id,_)|*id==instance.function).map(|(_,closure)|closure.clone()).ok_or("invocation body missing")?;
        let frame=self.native.facts.frames.len();
        let (call,name)=match origin{NativeInvocationOrigin::SourceCall{call}=>(Some(call),format!("invocation:{frame}:call:{call}")),NativeInvocationOrigin::ExplicitEntry{entry}=>(None,format!("entry:{entry}:frame:{frame}"))};
        self.native_frame(instance.function,Some(closure_id),call,name.clone(),instance.captures)?;
        self.native.facts.frames[frame].effects_complete=false;
        // Source frames retain their existing serialized shape; entry provenance is explicit.
        if let NativeInvocationOrigin::ExplicitEntry{entry}=origin{
            self.native.facts.frames[frame].origin=Some(origin);
            self.native.facts.entries[entry].frame=Some(frame);
            self.native.facts.query_views.insert(name.clone(),NativeQueryView::Entry{frame});
        }
        let ctx=Context{file:function.closure.file.clone(),occurrence:instance.load_occurrence,name,deferred:false,parameters:BTreeMap::new(),conditional_branch:match origin{NativeInvocationOrigin::ExplicitEntry{entry}=>self.native.facts.entries[entry].assumption.conditional_branch,_=>caller.and_then(|caller|caller.conditional_branch)}};
        let mut state=match origin{
            NativeInvocationOrigin::ExplicitEntry{entry}=>NativeContextState{paths:self.native.facts.entries[entry].assumption.paths.clone(),..NativeContextState::default()},
            NativeInvocationOrigin::SourceCall{..}=>caller.and_then(|caller|self.native.contexts.get(&caller.name)).cloned().unwrap_or_default(),
        };state.activations.clear();
        self.native.contexts.insert(ctx.name.clone(),state);
        let ctx=self.native_occurrence(&function.closure,&ctx,"invocation_entry")?;
        let mut offset=0;
        let mut bindings=Vec::new();
        if instance.method {
            let self_site=instance.self_parameter.as_ref().ok_or("method implicit self identity missing")?;
            let key=self.native_local_key(&self_site.id(),&ctx);
            let (decision,inputs)=self.native_argument_slot(&origin,args,0,"missing_self_argument_is_nil");
            bindings.push(ParameterBinding{parameter:ParameterIdentity{index:0,declaration:self_site.clone()},argument_fact:args.first().copied(),object_id:self.object(&decision),selected_value:decision.clone()});
            self.write(key.clone(),self_site.clone(),&ctx,decision,inputs);
            self.native.facts.frames[frame].implicit_self=Some(key);offset=1;
        }
        if let Some(list)=closure.get_params_list(){for(index,param)in list.get_params().enumerate(){
            self.budget()?;
            // A variadic declaration does not read its argument sequence. XML
            // handler wrappers may declare ... while using only fixed inputs.
            // Keep unknown variadic values behind the expression-use boundary.
            if param.is_dots(){continue}
            if index+offset>=INVOCATION_PARAMETER_LIMIT{return Err("native parameter count budget exceeded".into())}
            let token=param.get_name_token().ok_or("native parameter identity missing")?;
            let site=self.inputs.site(&ctx.file,token.get_range());
            let argument=args.get(index+offset).copied();
            let (selected,inputs)=self.native_argument_slot(&origin,args,index+offset,"missing_argument_is_nil");
            bindings.push(ParameterBinding{parameter:ParameterIdentity{index:index+offset,declaration:site.clone()},argument_fact:argument,object_id:self.object(&selected),selected_value:selected.clone()});
            self.write(self.native_local_key(&site.id(),&ctx),site,&ctx,selected,inputs);
        }}
        match origin{NativeInvocationOrigin::SourceCall{call}=>self.graph.calls[call].parameter_bindings=bindings,NativeInvocationOrigin::ExplicitEntry{entry}=>self.native.facts.entries[entry].parameters=bindings};
        let first_write=self.graph.writes.len();
        self.native.facts.frames[frame].effects_complete=true;
        self.native.stack.push(frame);
        let body=self.native_closure_body(&closure,&ctx);
        self.native.stack.pop();
        let result=body.and_then(|paths|self.native_finish_invocation(&paths,&ctx,frame));
        // A stopped invocation retains its actual prefix writes, never a successful return.
        match origin{
            NativeInvocationOrigin::SourceCall{call}=>self.graph.calls[call].effect_write_versions=(first_write..self.graph.writes.len()).collect(),
            NativeInvocationOrigin::ExplicitEntry{entry}=>self.native.facts.entries[entry].effect_write_versions=(first_write..self.graph.writes.len()).collect(),
        }
        let returns=match result{Ok(returns)=>returns,Err(reason)=>{
            self.native.facts.frames[frame].effects_complete=false;
            self.control.check().map_err(|error|error.to_string())?;
            return Err(reason);
        }};
        self.native.facts.frames[frame].return_facts=returns.clone();
        if let NativeInvocationOrigin::ExplicitEntry{entry}=origin{
            self.native.facts.entries[entry].return_facts=returns.clone();
            self.native.facts.entries[entry].effects_complete=self.native.facts.frames[frame].effects_complete;
        }
        Ok((returns,self.native.facts.frames[frame].effects_complete))
    }
    fn native_closure_body(&mut self,closure:&LuaClosureExpr,ctx:&Context)->Result<Vec<NativePath>> {
        match closure.get_block(){
            Some(block)=>self.native_block(block,ctx,0),
            // The admitted parser omits Block when the parameter list is
            // followed by end. Falling through returns zero values; scalar
            // and assignment adjustment add nil at their existing call sites.
            None=>{self.native_register_view(ctx);self.budget()?;Ok(vec![NativePath::continuing(ctx)])},
        }
    }
    fn native_truthiness(&self,decision:&Decision)->Option<bool> {
        match decision {
            Decision::KnownAbsent(_)=>Some(false),
            Decision::Bound(value)=>match &self.graph.values[*value].kind {
                ValueKind::Primitive(kind) if kind=="nil"=>Some(false),
                ValueKind::Primitive(kind) if kind=="boolean"=>self.native.boolean_values.get(value).copied(),
                ValueKind::Primitive(_)|ValueKind::Function(_)|ValueKind::Object(_)|ValueKind::HostCallable(_)=>Some(true),
                _=>None,
            },_=>None,
        }
    }
    fn native_eval_list(&mut self,expressions:Vec<LuaExpr>,ctx:&Context,depth:usize)->Result<Vec<usize>> {
        self.native_eval_adjusted_list(expressions,ctx,depth,None)
    }
    fn native_eval_adjusted_list(&mut self,expressions:Vec<LuaExpr>,ctx:&Context,depth:usize,assignment_slots:Option<usize>)->Result<Vec<usize>> {
        let count=expressions.len();let mut facts=Vec::new();
        for(index,expr)in expressions.into_iter().enumerate(){
            if index+1==count && let Some(slots)=assignment_slots
                && matches!(&expr,LuaExpr::LiteralExpr(literal) if matches!(literal.get_literal(),Some(LuaLiteralToken::Dots(_)))) {
                // Assignment fixes the result arity. Every remaining slot can
                // be an unknown variadic value or nil; no sequence length is
                // inferred. Calls and returns still require the open sequence.
                if slots>INVOCATION_RETURN_LIMIT{return Err("native expression list arity budget exceeded".into())}
                self.native_register_view(ctx);self.budget()?;
                if depth>64{return Err("expression depth budget exceeded".into())}
                let site=self.inputs.site(&ctx.file,expr.get_range());
                for _ in facts.len()..slots {
                    self.budget()?;
                    facts.push(self.usage(site.clone(),ctx,Decision::Unavailable("variadic_argument_slot_unavailable".into()),vec![]));
                }
                continue;
            }
            let expandable=index+1==count&&matches!(expr,LuaExpr::CallExpr(_));
            let fact=self.eval(expr,ctx,depth)?;
            if expandable&&let Some(values)=self.native.expanded_results.get(&fact){facts.extend(values.iter().copied())}else{facts.push(fact)}
            if facts.len()>INVOCATION_RETURN_LIMIT{return Err("native expression list arity budget exceeded".into())}
        }
        Ok(facts)
    }
    fn native_index_key(&mut self,key:Option<LuaIndexKey>,ctx:&Context,depth:usize)->Result<Option<String>> {
        self.native_index_key_observation(key,ctx,depth).map(|(key,_)|key)
    }
    fn native_index_key_observation(&mut self,key:Option<LuaIndexKey>,ctx:&Context,depth:usize)->Result<(Option<String>,Option<usize>)> {
        match key {
            Some(LuaIndexKey::Expr(expr))=>{let fact=self.eval(expr,ctx,depth+1)?;Ok((None,Some(fact)))},
            other=>Ok((Self::static_key(other),None)),
        }
    }
    fn native_index(&mut self,index:emmylua_parser::LuaIndexExpr,ctx:&Context,depth:usize)->Result<(usize,usize)> {
        let site=self.inputs.site(&ctx.file,index.get_range());
        let base=self.eval(index.get_prefix_expr().ok_or("native index base")?,ctx,depth+1)?;
        let (key,key_fact)=self.native_index_key_observation(index.get_index_key(),ctx,depth)?;
        let selected=self.graph.uses[base].decision.clone();
        let read_cell=match(self.object(&selected),key.as_ref()){(Some(object),Some(field))=>{let cell=format!("object:{object}:field:{field}");self.native.typed_cells.borrow_mut().entry(cell.clone()).or_insert_with(||StorageOwner::ObjectField{object,field:field.clone()});Some(cell)},_=>None};
        let(decision,writes)=if ctx.deferred&&self.graph.phase=="unknown"{(Decision::Unavailable("unknown_execution_phase".into()),vec![])}else{
            match(self.object(&selected),key){
                (_,None)=>(Decision::Unavailable("dynamic_field_key".into()),vec![]),
                (Some(object),Some(key))=>self.field(object,&key),
                _=>(match selected{Decision::Unavailable(reason)=>Decision::Unavailable(reason),_=>Decision::Unavailable("index_receiver_not_known_table".into())},vec![]),
            }
        };
        let fact=self.usage(site.clone(),ctx,decision,writes);if let Some(cell)=read_cell{self.native.facts.use_storages.push(UseStorageFact{use_fact:fact,cell});}
        if let Some(key)=key_fact {self.flow_expression(site,ctx,"dynamic_index".into(),vec![base,key],fact)?;}
        Ok((fact,base))
    }
    fn native_call(&mut self,call:emmylua_parser::LuaCallExpr,ctx:&Context,depth:usize)->Result<usize> {
        let site=self.inputs.site(&ctx.file,call.get_range());
        let prefix=call.get_prefix_expr().ok_or("native callee missing")?;
        let(callee,receiver)=if call.is_colon_call(){
            let LuaExpr::IndexExpr(index)=prefix else{return Err("colon call lacks index callee".into())};
            let(callee,receiver)=self.native_index(index,ctx,depth+1)?;(callee,Some(receiver))
        }else{(self.eval(prefix,ctx,depth+1)?,None)};
        let mut args=receiver.into_iter().collect::<Vec<_>>();
        let explicit:Vec<LuaExpr>=call.get_args_list().map(|list|list.get_args().collect()).unwrap_or_default();
        let syntactic_arguments=explicit.iter().map(|expr|self.inputs.site(&ctx.file,expr.get_range())).collect();
        args.extend(self.native_eval_list(explicit,ctx,depth+1)?);
        if args.len()>INVOCATION_PARAMETER_LIMIT{return Err("native normalized argument budget exceeded".into())}
        let selected=self.graph.uses[callee].decision.clone();
        let function=self.function(&selected);
        let intrinsic=function.and_then(|id|self.graph.functions[id].intrinsic.clone());
        let id=self.graph.calls.len();
        self.graph.calls.push(CallFact{id,site:site.clone(),context:ctx.name.clone(),callee,arguments:args.clone(),intrinsic:intrinsic.clone(),result_fact:None,argument_mapping_unavailable_reason:None,invocation_coverage:CallCoverage::Unavailable,effect_coverage:CallCoverage::Unavailable,parameter_bindings:vec![],effect_write_versions:vec![],conditional_branch:ctx.conditional_branch});
        if self.graph.host_runtime.is_some() {
            if self.host_tail(&args).is_some(){self.graph.calls[id].argument_mapping_unavailable_reason=Some("open_argument_cardinality".into());}
            else if self.host_arities(&args).is_some_and(|arities|arities.len()>1){self.graph.calls[id].argument_mapping_unavailable_reason=Some("conditional_argument_cardinality".into());}
        }
        if let Some(branch)=ctx.conditional_branch{self.graph.branches[branch].call_facts.push(id);}
        let mut expanded=None;
        let mut effects_complete=true;
        let(result,coverage)=if let Some(binding)=self.host_binding(&selected) {
            let returned=self.host_invoke(binding,id,&site,ctx)?;
            if self.inputs.host.data.bindings[binding].effect()==HostEffect::Unknown {
                if !ctx.deferred{self.native_unknown_call(id,ctx)?;}effects_complete=false;
            }
            let result=returned.first().map(|fact|self.graph.uses[*fact].decision.clone()).unwrap_or_else(||Decision::KnownAbsent("host_zero_return_scalar_nil".into()));
            expanded=Some(returned);(result,CallCoverage::ProfileContract)
        }else if ctx.deferred {
            if function.is_some_and(|id|self.graph.functions[id].pure_return_body){(self.value(&site,ValueKind::CallResult(id)),CallCoverage::BoundedProvedPureSubset)}
            else{(Decision::Unavailable("static_body_call_effects_not_executed".into()),CallCoverage::Unavailable)}
        }else if let Some(intrinsic)=intrinsic {
            let result=self.intrinsic(&intrinsic,id,&site,ctx,&args)?;
            let coverage=if matches!(result,Decision::Bound(_)){CallCoverage::ProfileContract}else{CallCoverage::Unavailable};
            (result,coverage)
        }else if let Some(function)=function {
            let(returned,complete)=self.native_invoke(id,ctx,&args)?;effects_complete=complete;
            let result=returned.first().map(|fact|self.graph.uses[*fact].decision.clone()).unwrap_or_else(||Decision::KnownAbsent("implicit_nil_return".into()));
            expanded=Some(returned);
            let function=&self.graph.functions[function];
            let coverage=if function.pure_return_body{CallCoverage::BoundedProvedPureSubset}else if function.effect_summary.is_some(){CallCoverage::BoundedParameterFieldWrites}else{CallCoverage::BoundedInvocation};
            (result,coverage)
        }else{
            self.native_unknown_call(id,ctx)?;
            let result=match selected{Decision::Unavailable(reason)=>Decision::Unavailable(reason),_=>Decision::Unavailable("callee_not_known_function".into())};
            if self.graph.host_runtime.is_some(){expanded=Some(self.host_unknown_returns(id,&site,ctx,&result)?);}
            (result,CallCoverage::Unavailable)
        };
        let fact=self.usage(site,ctx,result,vec![]);
        self.host_record_call(id,ctx,receiver,syntactic_arguments,expanded.as_deref(),fact)?;
        if let Some(values)=expanded{self.native.expanded_results.insert(fact,values);}
        self.graph.calls[id].result_fact=Some(fact);
        self.graph.calls[id].invocation_coverage=coverage.clone();
        self.graph.calls[id].effect_coverage=if effects_complete{coverage}else{CallCoverage::Unavailable};
        Ok(fact)
    }
    fn native_eval(&mut self,expr:LuaExpr,ctx:&Context,depth:usize)->Result<usize> {
        self.native_register_view(ctx);self.budget()?;
        if depth>64{return Err("expression depth budget exceeded".into())}
        let site=self.inputs.site(&ctx.file,expr.get_range());
        let read_cell=matches!(&expr,LuaExpr::NameExpr(_)).then(||self.native_key(&site,ctx));
        let mut expression:Option<(String,Vec<usize>)>=None;
        let(decision,writes)=match expr {
            LuaExpr::NameExpr(_)=>self.native_read(&self.native_key(&site,ctx),ctx),
            LuaExpr::LiteralExpr(literal)=>{
                if !ctx.deferred && matches!(literal.get_literal(),Some(LuaLiteralToken::Dots(_))){
                    self.native_boundary(&site,ctx,NativeBoundaryKind::Knowledge,"variadic_argument_sequence_unavailable");
                    return Err("native invocation stopped at variadic argument sequence boundary".into());
                }
                let mut truth=None;
                let primitive=match literal.get_literal(){Some(LuaLiteralToken::Number(_))=>"number",Some(LuaLiteralToken::String(_))=>"string",Some(LuaLiteralToken::Bool(token))=>{truth=Some(token.is_true());"boolean"},Some(LuaLiteralToken::Nil(_))=>"nil",_=>"unsupported"};
                if primitive=="unsupported"{(Decision::Unavailable("unsupported_literal".into()),vec![])}else{
                    let value=self.value(&site,ValueKind::Primitive(primitive.into()));
                    if let(Decision::Bound(id),Some(truth))=(&value,truth){self.native.boolean_values.insert(*id,truth);}
                    if primitive=="number" && let Decision::Bound(id)=&value && let Ok(number)=site.text(&self.inputs).map_err(|e|e.to_string())?.parse::<f64>() && number.is_finite(){self.native.numeric_values.insert(*id,number);self.native.facts.control.numbers.insert(*id,number);}
                    (value,vec![])
                }
            }
            LuaExpr::TableExpr(table)=>{
                if ctx.deferred{return Ok(self.usage(site,ctx,Decision::Unavailable("static_table_allocation_not_executed".into()),vec![]))}
                let object=self.allocate_object(&site)?;
                for field in table.get_fields(){
                    let key=self.native_index_key(field.get_field_key(),ctx,depth)?;
                    let fact=field.get_value_expr().map(|value|self.eval(value,ctx,depth+1)).transpose()?;
                    match(key,fact){(Some(key),Some(fact))=>{self.put_field(object,key,self.inputs.site(&ctx.file,field.get_range()),ctx,self.graph.uses[fact].decision.clone(),vec![fact]);},_=>self.unknown_object(object,"dynamic_table_key".into(),self.inputs.site(&ctx.file,field.get_range()))}
                }
                (self.value(&site,ValueKind::Object(object)),vec![])
            }
            LuaExpr::IndexExpr(index)=>return self.native_index(index,ctx,depth).map(|(fact,_)|fact),
            LuaExpr::CallExpr(call)=>return self.native_call(call,ctx,depth),
            LuaExpr::ClosureExpr(closure)=>{
                if ctx.deferred{(Decision::Unavailable("static_closure_creation_not_executed".into()),vec![])}else{(self.native_register_function("<anonymous>".into(),site.clone(),closure,ctx),vec![])}
            }
            LuaExpr::ParenExpr(paren)=>{let fact=self.eval(paren.get_expr().ok_or("paren")?,ctx,depth+1)?;expression=Some(("parenthesis".into(),vec![fact]));(self.graph.uses[fact].decision.clone(),self.graph.uses[fact].writes.clone())}
            LuaExpr::BinaryExpr(binary)=>{
                let(left,right)=binary.get_exprs().ok_or("binary operands")?;
                let left_fact=self.eval(left,ctx,depth+1)?;
                let selected=self.graph.uses[left_fact].decision.clone();
                let op=binary.get_op_token().ok_or("binary operator")?.get_op();
                if matches!(op,emmylua_parser::BinaryOperator::OpAnd|emmylua_parser::BinaryOperator::OpOr){
                    let rhs_on=op==emmylua_parser::BinaryOperator::OpAnd;
                    match self.native_truthiness(&selected){
                        Some(truth) if truth!=rhs_on=>{expression=Some(("selected_operand".into(),vec![left_fact]));(selected,self.graph.uses[left_fact].writes.clone())},
                        Some(_)=>{let right_fact=self.eval(right,ctx,depth+1)?;expression=Some(("selected_operand".into(),vec![right_fact]));(self.graph.uses[right_fact].decision.clone(),self.graph.uses[right_fact].writes.clone())},
                        None=>{
                            let branch=self.native_branch(&site,ctx,vec![left_fact]);
                            let child=self.native_predicate_context(ctx,left_fact,rhs_on,Some(branch))?;
                            let before=self.native_checkpoint(&site,ctx)?;
                            let right_fact=self.eval(right,&child,depth+1)?;
                            expression=Some((if rhs_on{"and"}else{"or"}.into(),vec![left_fact,right_fact]));
                            let after=self.native_checkpoint(&site,&child)?;self.native_restore(&before,&site,ctx)?;
                            let skip_ctx=self.native_predicate_context(ctx,left_fact,!rhs_on,Some(branch))?;
                            let alternatives=vec![NativePath{ctx:skip_ctx,outcome:NativeControl::Continue,heap:Some(before)},NativePath{ctx:child,outcome:NativeControl::Continue,heap:Some(after)}];
                            self.native_join_continuations(alternatives,&site,ctx)?;
                            let result=if self.graph.uses[right_fact].decision==selected{selected}else{Decision::Unavailable("conditional_expression_value".into())};
                            (result,vec![])
                        }
                    }
                }else{
                    let right_fact=self.eval(right,ctx,depth+1)?;
                    let operator=binary.get_op_token().ok_or("binary operator")?.get_text().to_string();
                    expression=Some((operator,vec![left_fact,right_fact]));
                    (Decision::Unavailable("unsupported_binary_result_operands_evaluated".into()),vec![])
                }
            }
            LuaExpr::UnaryExpr(unary)=>{
                let fact=self.eval(unary.get_expr().ok_or("unary operand")?,ctx,depth+1)?;
                expression=Some((unary.get_op_token().ok_or("unary operator")?.get_text().to_string(),vec![fact]));
                if unary.get_op_token().is_some_and(|op|op.get_op()==emmylua_parser::UnaryOperator::OpNot){
                    let truth=self.native_truthiness(&self.graph.uses[fact].decision);
                    let value=self.value(&site,ValueKind::Primitive("boolean".into()));
                    if let(Decision::Bound(id),Some(truth))=(&value,truth){self.native.boolean_values.insert(*id,!truth);}
                    (value,vec![])
                }else if unary.get_op_token().is_some_and(|op|op.get_op()==emmylua_parser::UnaryOperator::OpUnm) {
                    if let Some(number)=self.native_number(fact){let value=self.native_number_value(&site,-number);(value,vec![])}else{(Decision::Unavailable("unsupported_unary_result_operand_evaluated".into()),vec![])}
                }else{(Decision::Unavailable("unsupported_unary_result_operand_evaluated".into()),vec![])}
            }
            _=>{
                for child in expr.syntax().children().filter_map(LuaExpr::cast){self.eval(child,ctx,depth+1)?;}
                (Decision::Unavailable("unsupported_expression_children_evaluated".into()),vec![])
            }
        };
        let fact=self.usage(site.clone(),ctx,decision,writes);if let Some(cell)=read_cell{self.native.facts.use_storages.push(UseStorageFact{use_fact:fact,cell});}
        if let Some((operator,operands))=expression {self.flow_expression(site,ctx,operator,operands,fact)?;}
        Ok(fact)
    }
    fn native_assignment_target(&mut self,expr:LuaExpr,ctx:&Context,depth:usize)->Result<AssignmentTarget> {
        let site=self.inputs.site(&ctx.file,expr.get_range());
        let destination=match expr {
            LuaExpr::NameExpr(_)=>AssignmentDestination::Cell(self.native_key(&site,ctx)),
            LuaExpr::IndexExpr(index)=>{
                let base=self.eval(index.get_prefix_expr().ok_or("native write base")?,ctx,depth+1)?;
                let key=self.native_index_key(index.get_index_key(),ctx,depth)?;
                match self.object(&self.graph.uses[base].decision){
                    Some(object)=>AssignmentDestination::Field{object,key,receiver_fact:base},
                    None=>AssignmentDestination::UncertainField{receiver_fact:base},
                }
            },
            _=>return Err("native assignment target unsupported".into()),
        };
        Ok(AssignmentTarget{site,destination})
    }
    fn native_simple_stat(&mut self,stat:LuaStat,ctx:&Context)->Result<NativeControl> {
            match stat {
                LuaStat::LocalStat(stat)=>{
                    let names:Vec<_>=stat.get_local_name_list().collect();
                    let values=self.native_eval_adjusted_list(stat.get_value_exprs().collect(),ctx,0,Some(names.len()))?;
                    for(index,name)in names.into_iter().enumerate(){
                        let site=self.inputs.site(&ctx.file,name.get_name_token().ok_or("native local declaration")?.get_range());
                        let(decision,inputs)=self.host_list_slot(&values,index,"missing_assignment_slot_is_nil");
                        self.write(self.native_local_key(&site.id(),ctx),site,ctx,decision,inputs);
                    }
                }
                LuaStat::AssignStat(stat)=>{
                    let(vars,exprs)=stat.get_var_and_expr_list();
                    let mut targets=Vec::new();for var in vars{targets.push(self.native_assignment_target(var.to_expr(),ctx,0)?);}
                    let values=self.native_eval_adjusted_list(exprs,ctx,0,Some(targets.len()))?;
                    for(index,target)in targets.into_iter().enumerate(){
                        let(decision,inputs)=self.host_list_slot(&values,index,"missing_assignment_slot_is_nil");
                        self.apply_assignment(target,decision,inputs,ctx);
                    }
                }
                LuaStat::FuncStat(stat)=>{
                    let name=stat.get_func_name().ok_or("native function name")?.to_expr();
                    let site=self.inputs.site(&ctx.file,name.get_range());let target=self.native_assignment_target(name,ctx,0)?;
                    let value=self.native_register_function(site.text(&self.inputs).expect("admitted parser Site").into(),site,stat.get_closure().ok_or("native closure")?,ctx);
                    self.apply_assignment(target,value,vec![],ctx);
                }
                LuaStat::LocalFuncStat(stat)=>{
                    let token=stat.get_local_name().and_then(|name|name.get_name_token()).ok_or("native local function name")?;
                    let site=self.inputs.site(&ctx.file,token.get_range());
                    let value=self.native_register_function(site.text(&self.inputs).expect("admitted parser Site").into(),site.clone(),stat.get_closure().ok_or("native local closure")?,ctx);
                    self.write(self.native_local_key(&site.id(),ctx),site,ctx,value,vec![]);
                }
                LuaStat::CallExprStat(stat)=>{self.eval(LuaExpr::CallExpr(stat.get_call_expr().ok_or("native call statement")?),ctx,0)?;}
                LuaStat::ReturnStat(stat)=>return Ok(NativeControl::Return(self.native_eval_list(stat.get_expr_list().collect(),ctx,0)?)),
                LuaStat::EmptyStat(_)=>{},
                _=>return Err("native statement outside bounded invocation subset".into()),
            }
        Ok(NativeControl::Continue)
    }
    fn native_branch(&mut self,site:&Site,ctx:&Context,conditions:Vec<usize>)->usize {
        let id=self.graph.branches.len();
        let condition_sites=conditions.iter().map(|id|self.graph.uses[*id].site.clone()).collect();
        self.graph.branches.push(BranchFact{id,site:site.clone(),parent:ctx.conditional_branch,condition_sites,condition_facts:conditions,call_facts:vec![],write_versions:vec![],footprints:vec![],coverage_gaps:vec![]});id
    }
    fn native_checkpoint(&mut self,site:&Site,ctx:&Context)->Result<NativeHeapCheckpoint> {
        self.budget()?;
        if self.native.facts.checkpoints.len()>=INVOCATION_CHECKPOINT_LIMIT{return Err("native checkpoint count budget exceeded".into())}
        self.inputs.charge(self.cells.len()+self.objects.len()+self.native.facts.escaped_roots.len()).map_err(|error|error.to_string())?;
        let previous=self.native.facts.checkpoints.last();
        let before=previous.map(|checkpoint|checkpoint.cells.materialize()).unwrap_or_default();
        let same_roots=previous.is_some_and(|checkpoint|*checkpoint.escaped_roots==self.native.facts.escaped_roots);
        let slots=NativeCheckpointCells::changed_slots(&before,&self.cells)
            +self.objects.iter().enumerate().filter(|(index,object)|!previous.and_then(|checkpoint|checkpoint.objects.get(*index)).is_some_and(|old|old.as_ref()==*object)).map(|(_,object)|1+object.fields.len()).sum::<usize>()
            +if same_roots{0}else{self.native.facts.escaped_roots.len()};
        if slots>INVOCATION_CHECKPOINT_SLOT_LIMIT-self.native.checkpoint_slots{return Err("native checkpoint slot budget exceeded".into())}
        self.inputs.charge(slots).map_err(|error|error.to_string())?;
        let cells=previous.map(|checkpoint|checkpoint.cells.clone()).unwrap_or_default().capture(&before,&self.cells);
        let objects=self.objects.iter().enumerate().map(|(index,object)|match previous.and_then(|checkpoint|checkpoint.objects.get(index)){
            Some(old) if old.as_ref()==object=>old.clone(),_=>Arc::new(object.clone()),
        }).collect();
        let escaped_roots=if same_roots{previous.unwrap().escaped_roots.clone()}else{Arc::new(self.native.facts.escaped_roots.clone())};
        self.native.checkpoint_slots+=slots;
        let checkpoint=NativeHeapCheckpoint{id:self.native.facts.checkpoints.len(),site:site.clone(),context:ctx.name.clone(),observation:self.heap_observation,cells,objects,global_open:self.native.facts.global_environment_open,escaped_roots};
        self.native.facts.checkpoints.push(checkpoint.clone());Ok(checkpoint)
    }
    fn native_restore(&mut self,checkpoint:&NativeHeapCheckpoint,site:&Site,ctx:&Context)->Result<()> {
        self.budget()?;
        if self.native.facts.restores.len()>=INVOCATION_CHECKPOINT_LIMIT{return Err("native restore count budget exceeded".into())}
        let cells=checkpoint.cells.materialize();
        self.inputs.charge(cells.len()).map_err(|error|error.to_string())?;
        self.cells=cells;
        let mut restored_objects=Vec::new();
        // Object IDs are append-only. Keep unreachable branch allocations so a
        // later branch cannot reuse an identity already present in frozen facts.
        // Each actual rollback gets a source-backed mutation and immutable state
        // checkpoint; an observation number alone is not restoration evidence.
        for(index,object)in checkpoint.objects.iter().enumerate(){
            self.budget()?;
            if self.objects[index]!=**object{
                self.objects[index]=object.as_ref().clone();restored_objects.push(index);
                self.observe_heap(index,"checkpoint_restore",site.clone(),None,Some(format!("checkpoint:{}",checkpoint.id)));
            }
        }
        self.native.facts.global_environment_open=checkpoint.global_open;
        // An external party may have retained anything reachable on either
        // speculative branch; potential escapes are monotone across joins.
        self.native.facts.escaped_roots.extend(checkpoint.escaped_roots.iter().cloned());
        self.native.facts.restores.push(NativeRestoreFact{checkpoint:checkpoint.id,site:site.clone(),context:ctx.name.clone(),observation:self.heap_observation,restored_objects,retained_escape_roots:self.native.facts.escaped_roots.clone()});
        self.budget()
    }
    fn native_join(&mut self,left:&NativeHeapCheckpoint,right:&NativeHeapCheckpoint,site:&Site,ctx:&Context,(branch,join):(usize,usize),sides:&[NativeTerminalPath])->Result<()> {
        self.budget()?;
        let left_cells=left.cells.materialize();let right_cells=right.cells.materialize();
        let keys:std::collections::BTreeSet<_>=left_cells.keys().chain(right_cells.keys()).cloned().collect();
        let mut changed=Vec::new();
        for key in keys {
            if key.starts_with("object:"){continue}
            let a=left_cells.get(&key).copied();let b=right_cells.get(&key).copied();
            let same=match(a,b){(Some(a),Some(b))=>self.graph.writes[a].decision==self.graph.writes[b].decision,(None,None)=>true,_=>false};
            if a==b{continue}
            self.budget()?;
            let selected=if same{a.map(|id|self.graph.writes[id].decision.clone()).unwrap_or_else(||Decision::KnownAbsent("absent_join_cell".into()))}else{Decision::Unavailable("conditional_cell_write".into())};
            let write=self.write(key.clone(),site.clone(),ctx,selected,vec![]);
            self.flow_write_join(join,branch,write,sides,[a,b],None)?;
            changed.push(json!({"cell":key,"leftWrite":a,"rightWrite":b,"joinedWrite":write}));
        }
        for object in 0..left.objects.len().min(right.objects.len()){
            self.budget()?;
            let a=&left.objects[object];let b=&right.objects[object];
            if a.unknown!=b.unknown||a.unknown_observation!=b.unknown_observation{
                self.unknown_object(object,"conditional_object_behavior".into(),site.clone());
                changed.push(json!({"object":object,"behavior":"conditional_unknown"}));
            }
            let fields:std::collections::BTreeSet<_>=a.fields.keys().chain(b.fields.keys()).cloned().collect();
            for field in fields {
                let av=a.fields.get(&field).copied();let bv=b.fields.get(&field).copied();
                let same=match(av,bv){(Some(a),Some(b))=>self.graph.writes[a].decision==self.graph.writes[b].decision,(None,None)=>true,_=>false};
                if av==bv{continue}
                self.budget()?;
                let selected=if same{av.map(|id|self.graph.writes[id].decision.clone()).unwrap_or_else(||Decision::KnownAbsent("absent_join_field".into()))}else{Decision::Unavailable("conditional_field_write".into())};
                let write=self.put_field(object,field.clone(),site.clone(),ctx,selected,vec![]);
                self.flow_write_join(join,branch,write,sides,[av,bv],Some((object,field.clone())))?;
                changed.push(json!({"object":object,"field":field,"leftWrite":av,"rightWrite":bv,"joinedWrite":write}));
            }
        }
        self.native.facts.global_environment_open=left.global_open||right.global_open;
        self.graph.branches[branch].footprints.push(json!({"execution":"conditional_join","changes":changed}));
        Ok(())
    }
    fn native_mark_uncertain_effect(&mut self) {
        for frame in &self.native.stack{self.native.facts.frames[*frame].effects_complete=false;}
    }
    fn native_uncertain_field_write(&mut self,site:Site,ctx:&Context,receiver_fact:usize,value:Decision,inputs:Vec<usize>) {
        self.native_mark_uncertain_effect();
        if self.native.facts.uncertain_effects.len()>=INVOCATION_UNCERTAIN_EFFECT_LIMIT{
            self.observation_failure.get_or_insert("native uncertain effect budget exceeded".into());return
        }
        let id=self.native.facts.uncertain_effects.len();let reason="field_write_receiver_identity_unavailable".to_string();
        let proof_id=digest(&(self.graph.snapshot_key.as_str(),id,&site,&ctx.name,receiver_fact,&value,&inputs,&reason));
        self.native.facts.uncertain_effects.push(NativeUncertainEffect{id,site,context:ctx.name.clone(),receiver_fact,value,inputs,reason,proof_id});
        // Do not recover a former object through the receiver's previous write.
        // Its escaped state stays unavailable; unrelated local objects survive.
    }
    fn native_unknown_call(&mut self,call:usize,ctx:&Context)->Result<()> {
        if ctx.deferred{return Ok(())}
        self.native_mark_uncertain_effect();
        #[derive(Clone,PartialEq,Eq,PartialOrd,Ord)]
        enum Node { Value(usize), Object(usize), Closure(usize), Cell(String) }
        let fact=self.graph.calls[call].clone();let mut work=Vec::new();
        let roots_before=self.native.facts.escaped_roots.len();let mut retained_roots=Vec::new();
        // External references outlive a single call. Captured cells are roots,
        // not snapshots of their former selected object, so revisit their
        // current values after local rebinding on every subsequent unknown call.
        for root in &self.native.facts.escaped_roots{work.push(match root{
            NativeEscapeRoot::Object(object)=>Node::Object(*object),
            NativeEscapeRoot::Closure(closure)=>Node::Closure(*closure),
            NativeEscapeRoot::Cell(cell)=>Node::Cell(cell.clone()),
        });}
        for argument in &fact.arguments{if let Decision::Bound(value)=self.graph.uses[*argument].decision{work.push(Node::Value(value));}}
        if let Decision::Bound(value)=self.graph.uses[fact.callee].decision{work.push(Node::Value(value));}
        // Unknown code shares globals, but not arbitrary caller lexical cells.
        for key in self.cells.keys().filter(|key|key.starts_with("global:")){work.push(Node::Cell(key.clone()));}
        for (key,value) in &self.native.host.initial_globals {if !self.cells.contains_key(key){work.push(Node::Value(*value));}}
        let mut seen=std::collections::BTreeSet::new();let mut cells=Vec::new();let mut objects=Vec::new();
        while let Some(node)=work.pop(){
            self.budget()?;
            if !seen.insert(node.clone()){continue}
            if seen.len()>INVOCATION_FOOTPRINT_LIMIT{return Err("native reachable state footprint budget exceeded".into())}
            let retained=match &node{
                Node::Object(object)=>Some(NativeEscapeRoot::Object(*object)),
                Node::Closure(closure)=>Some(NativeEscapeRoot::Closure(*closure)),
                Node::Cell(cell)=>Some(NativeEscapeRoot::Cell(cell.clone())),_=>None,
            };
            if let Some(root)=retained{
                if self.native.facts.escaped_roots.insert(root.clone()){retained_roots.push(root);}
                if self.native.facts.escaped_roots.len()>INVOCATION_ESCAPE_ROOT_LIMIT{return Err("native retained external root budget exceeded".into())}
            }
            match node {
                Node::Value(value)=>match &self.graph.values[value].kind {
                    ValueKind::Object(object)=>work.push(Node::Object(*object)),
                    ValueKind::HostReturn{..}=>if let Some(object)=self.native.host.nominal_objects.get(&value){work.push(Node::Object(*object));},
                    ValueKind::Function(_)=>if let Some(closure)=self.native.closure_values.get(&value){work.push(Node::Closure(*closure));},
                    _=>{},
                },
                Node::Object(object)=>{
                    objects.push(object);
                    for write in self.objects[object].fields.values(){if let Decision::Bound(value)=self.graph.writes[*write].decision{work.push(Node::Value(value));}}
                }
                Node::Closure(closure)=>{
                    for cell in self.native.facts.closures[closure].captures.values(){work.push(Node::Cell(cell.clone()));}
                }
                Node::Cell(cell)=>{
                    if let Some(write)=self.cells.get(&cell)&&let Decision::Bound(value)=self.graph.writes[*write].decision{work.push(Node::Value(value));}
                    cells.push(cell);
                }
            }
        }
        if self.native.facts.escape_events.len()>=INVOCATION_FOOTPRINT_LIMIT{return Err("native escape event budget exceeded".into())}
        let roots_after=self.native.facts.escaped_roots.len();
        // Every call retains its full reachability result, including roots that
        // escaped earlier. Equal footprints share one immutable evidence record.
        let mut footprint_objects=objects.clone();footprint_objects.sort_unstable();
        let mut footprint_cells=cells.clone();footprint_cells.sort();
        let footprint_proof=digest(&(self.graph.snapshot_key.as_str(),"unknown_call_footprint_v1",&footprint_objects,&footprint_cells));
        let footprint=if let Some(id)=self.native.unknown_footprint_ids.get(&footprint_proof){*id}else{
            let id=self.native.facts.unknown_call_footprints.len();
            self.native.facts.unknown_call_footprints.push(NativeUnknownCallFootprint{id,objects:footprint_objects,cells:footprint_cells,proof_id:footprint_proof.clone()});
            self.native.unknown_footprint_ids.insert(footprint_proof.clone(),id);id
        };
        let observation_before=self.heap_observation;
        let mut inputs=vec![fact.callee];inputs.extend(fact.arguments);
        for object in objects{self.unknown_call_object(object,fact.site.clone());}
        for cell in cells{self.write(cell,fact.site.clone(),ctx,Decision::Unavailable("ordinary_unknown_call_reachable_cell_rebind".into()),inputs.clone());}
        self.native.facts.global_environment_open=true;
        let observation_after=self.heap_observation;
        let application_complete=self.observation_failure.is_none();
        let proof_id=digest(&(self.graph.snapshot_key.as_str(),"unknown_call_effect_v1",call,&fact.site,&ctx.name,roots_before,&retained_roots,roots_after,&footprint_proof,observation_before,observation_after,application_complete));
        self.native.facts.escape_events.push(NativeEscapeEvent{call,context:ctx.name.clone(),roots_before,retained:retained_roots,roots_after,site:fact.site,footprint,observation_before,observation_after,application_complete,proof_id});
        Ok(())
    }
    fn native_static_bodies(&mut self)->Result<()> {
        // Exactly one symbolic source template per definition. Concrete calls
        // remain separate query views; none of their heap writes are replayed.
        for(function_id,closure)in self.bodies.clone(){
            self.budget()?;let function=self.graph.functions[function_id].clone();if function.intrinsic.is_some(){continue}
            let instances:Vec<_>=self.native.facts.closures.iter().filter(|instance|instance.function==function_id).cloned().collect();
            let mut captures=BTreeMap::new();let mut ambiguous=Vec::new();
            let names:std::collections::BTreeSet<_>=instances.iter().flat_map(|instance|instance.captures.keys().cloned()).collect();
            for lexical in names{
                let cells:std::collections::BTreeSet<_>=instances.iter().filter_map(|instance|instance.captures.get(&lexical).cloned()).collect();
                if cells.len()==1{captures.insert(lexical,cells.into_iter().next().ok_or("static capture cell")?);}else{ambiguous.push(lexical);}
            }
            let name=format!("function:{function_id}:phase:{}",self.graph.phase);
            let frame=self.native_frame(function_id,None,None,name.clone(),captures)?;
            let mut ctx=Context{file:function.closure.file.clone(),occurrence:function.load_occurrence,name,deferred:true,parameters:BTreeMap::new(),conditional_branch:None};
            for lexical in ambiguous{ctx.parameters.insert(self.native_local_key(&lexical,&ctx),Decision::Unavailable("captured_runtime_cell_requires_invocation_selector".into()));}
            if let Some(params)=closure.get_params_list(){for param in params.get_params(){if let Some(token)=param.get_name_token(){
                let site=self.inputs.site(&ctx.file,token.get_range());let value=self.value(&site,ValueKind::NativeParameter(site.clone()));
                ctx.parameters.insert(self.native_local_key(&site.id(),&ctx),value);
            }}}
            if let Some(self_site)=instances.first().and_then(|instance|instance.self_parameter.clone()){
                let key=self.native_local_key(&self_site.id(),&ctx);
                let value=self.value(&self_site,ValueKind::NativeParameter(self_site.clone()));
                self.native.facts.frames[frame].implicit_self=Some(key.clone());ctx.parameters.insert(key,value);
            }
            let Some(block)=closure.get_block()else{continue};
            for stat in block.get_stats(){if let LuaStat::ReturnStat(stat)=stat{for expr in stat.get_expr_list(){
                let fact=if function.body_check_coverage=="return_expressions_only"{self.eval(expr,&ctx,0)?}else{self.usage(self.inputs.site(&ctx.file,expr.get_range()),&ctx,Decision::Unavailable("function_body_statement_effects_not_analyzed".into()),vec![])};
                self.graph.returns.push(ReturnCheck{function:function_id,expression:fact,context:ctx.name.clone()});
                self.native.facts.frames[frame].return_facts.push(fact);
            }}}
        }
        Ok(())
    }
}
