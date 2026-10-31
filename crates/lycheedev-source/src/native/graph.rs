#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Site { pub file: String, pub start: u32, pub end: u32 }
impl Site {
    pub fn at(file: &str, range: TextRange) -> Self { Self { file:file.into(), start:range.start().into(), end:range.end().into() } }
    pub fn id(&self) -> String { format!("{}:{}:{}", self.file,self.start,self.end) }
    pub fn text<'a>(&self,inputs:&'a SourceInputs) -> lycheedev_core::Result<&'a str> { inputs.original(&self.file)?.get(self.start as usize..self.end as usize).ok_or_else(||input_error("invalid original Site UTF8 range")) }
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag="state", content="detail", rename_all="snake_case")]
pub enum Decision {
    Bound(usize), Unavailable(String), KnownAbsent(String),
    NotManaged,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag="kind", content="detail", rename_all="snake_case")]
pub enum ValueKind { Primitive(String), Function(usize), Object(usize), CallResult(usize), NativeParameter(Site), HostCallable(usize), HostReturn{binding:usize,call:usize,slot:usize} }
#[derive(Clone, Debug, Serialize)]
pub struct AbstractValue { pub id: usize, pub origin: ValueOrigin, pub kind: ValueKind }
#[derive(Clone, Debug, Serialize)]
pub struct Signature { pub parameters: Vec<(String,String)>, pub returns: String, pub evidence: String }
#[derive(Clone, Debug, Serialize)]
pub struct ParameterIdentity { pub index:usize, pub declaration:Site }
#[derive(Clone, Debug, Serialize)]
#[serde(tag="kind", rename_all="snake_case")]
pub enum EffectExpression { Name {site:Site}, Literal {site:Site, primitive:String} }
#[derive(Clone, Debug, Serialize)]
pub struct ParameterFieldWrite { pub parameter:ParameterIdentity, pub field:String, pub receiver_site:Site, pub write_site:Site, pub value:EffectExpression }
#[derive(Clone, Debug, Serialize)]
pub struct FunctionEffectSummary { pub id:String, pub parameters:Vec<ParameterIdentity>, pub writes:Vec<ParameterFieldWrite>, pub return_rule:String }
#[derive(Clone, Debug, Serialize)]
pub struct ParameterBinding { pub parameter:ParameterIdentity, pub argument_fact:Option<usize>, pub selected_value:Decision, pub object_id:Option<usize> }
#[derive(Clone, Debug, Serialize)]
pub struct Function { pub id: usize, pub declaration: Site, pub closure: Site, pub name: String, pub signature: Option<Signature>, pub signature_reason: Option<String>, pub intrinsic: Option<String>, pub pure_return_body: bool, pub body_check_coverage:String, pub load_occurrence: usize, pub effect_summary:Option<FunctionEffectSummary>, pub effect_summary_unavailable_reason:Option<String> }
#[derive(Clone, Debug, Serialize)]
pub struct WriteVersion { pub id: usize, pub cell: String, pub site: Site, pub decision: Decision, pub previous: Option<usize>, pub inputs: Vec<usize>, pub context: String, pub conditional_branch:Option<usize> }
#[derive(Clone, Debug, Serialize)]
pub struct UseFact {
    pub id: usize, pub site: Site, pub context: String, pub decision: Decision, pub writes: Vec<usize>, pub proof_id: String,
    #[serde(skip_serializing_if="Option::is_none")]
    pub object_view: Option<usize>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub host_return_slot: Option<HostReturnSlotIdentity>,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectViewKey { pub context: String, pub observation: usize, pub object_id: usize }
#[derive(Clone, Debug, Serialize)]
#[serde(tag="state", content="reason", rename_all="snake_case")]
pub enum ObjectEnumeration { Complete, Unavailable(String) }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all="snake_case")]
pub enum SlotPresence { Present, Absent, Unavailable }
#[derive(Clone, Debug, Serialize)]
pub struct MemberSlotFact {
    pub key: String,
    pub presence: SlotPresence,
    pub selected_value: Decision,
    pub destination_write: usize,
    pub source_write: Option<usize>,
    pub copy_origin: Option<usize>,
    pub nested_object_view: Option<usize>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub xml_copy_origin:Option<usize>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ObjectShapeFacts {
    pub id: usize, pub key: ObjectViewKey, pub fields: Vec<MemberSlotFact>,
    pub enumeration: ObjectEnumeration, pub proof_id: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct ObjectMutationFact {
    pub observation: usize, pub object_id: usize, pub kind: &'static str, pub site: Site,
    pub write: Option<usize>, pub reason: Option<String>,
}
const OBJECT_LIMIT: usize = 64;
const OBJECT_VIEW_LIMIT: usize = 512;
const OBJECT_SLOT_LIMIT: usize = 4096;
const OBJECT_FIELD_LIMIT: usize = 128;
const OBJECT_DEPTH_LIMIT: usize = 64;
const OBJECT_MUTATION_LIMIT: usize = 4096;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all="snake_case")]
pub enum CallCoverage { ProfileContract, BoundedProvedPureSubset, BoundedParameterFieldWrites, BoundedInvocation, Unavailable }
#[derive(Clone, Debug, Serialize)]
pub struct CallFact {
    pub id: usize, pub site: Site, pub context: String, pub callee: usize, pub arguments: Vec<usize>, pub intrinsic: Option<String>,
    #[serde(rename="resultFactId")]
    pub result_fact: Option<usize>,
    pub argument_mapping_unavailable_reason: Option<String>,
    pub invocation_coverage: CallCoverage,
    pub effect_coverage: CallCoverage,
    pub parameter_bindings: Vec<ParameterBinding>,
    pub effect_write_versions: Vec<usize>,
    pub conditional_branch:Option<usize>,
}
#[derive(Clone, Debug, Serialize)]
pub struct CopyEdge { pub id: usize, pub call: usize, pub source_object: usize, pub source_field_write: usize, pub destination_object: usize, pub destination_field_write: usize, pub field: String, pub selected_value: Decision, pub profile_rule: String }
#[derive(Clone, Debug, Serialize)]
pub struct ReturnCheck { pub function: usize, pub expression: usize, pub context: String }
#[derive(Clone, Debug, Serialize)]
pub struct BranchFact { pub id:usize, pub site:Site, pub parent:Option<usize>, pub condition_sites:Vec<Site>, pub condition_facts:Vec<usize>, pub call_facts:Vec<usize>, pub write_versions:Vec<usize>, pub footprints:Vec<Value>, pub coverage_gaps:Vec<Value> }
#[derive(Clone, Debug, Serialize)]
pub struct GraphFacts { pub snapshot_key: String, #[serde(serialize_with="serialize_graph_snapshot")] pub snapshot_inputs: Arc<GraphSnapshotEvidence>, pub snapshot_inputs_json: SnapshotInputsJson, pub scenario: String, pub phase: String, pub script_load_occurrences: Vec<String>, pub values: Vec<AbstractValue>, pub functions: Vec<Function>, pub writes: Vec<WriteVersion>, pub uses: Vec<UseFact>, pub calls: Vec<CallFact>, pub copies: Vec<CopyEdge>, pub returns: Vec<ReturnCheck>, pub branches:Vec<BranchFact>, pub raw_emmy: Vec<RawEmmyEvidence>,
    #[serde(skip_serializing_if="Vec::is_empty")]
    pub object_views: Vec<ObjectShapeFacts>,
    #[serde(skip_serializing_if="Vec::is_empty")]
    pub object_mutations: Vec<ObjectMutationFact>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub native_invocations: Option<NativeInvocationFacts>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub host_runtime: Option<HostRuntimeFacts>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub xml:Option<XmlRuntimeFacts>,
}

fn serialize_graph_snapshot<S:serde::Serializer>(snapshot:&Arc<GraphSnapshotEvidence>,serializer:S)->std::result::Result<S::Ok,S::Error> {
    snapshot.as_ref().serialize(serializer)
}

#[derive(Clone)]
enum ScriptBody { Block(LuaBlock), Empty(TextRange) }
impl ScriptBody {
    fn from_chunk(chunk:&LuaChunk,analysis_text:&str)->Result<Self> {
        match chunk.get_block() {
            Some(block)=>Ok(Self::Block(block)),
            // The pinned parser deliberately removes the Block of a zero-byte
            // chunk. Do not turn a missing block for any other input into a no-op.
            None if analysis_text.is_empty() && chunk.get_range().is_empty()=>Ok(Self::Empty(chunk.get_range())),
            None=>Err("block".into()),
        }
    }
}
struct Harvest { declaration_sites:BTreeMap<String,SyntaxSite>, blocks: BTreeMap<String,ScriptBody>, local_reads: BTreeMap<SyntaxSite,SyntaxSite>, signatures: BTreeMap<String,Signature>, intrinsic_declarations:BTreeMap<String,String>, raw: Vec<RawEmmyEvidence> }
fn harvest(analysis: &EmmyLuaAnalysis, names: &[&str],inputs:&SourceInputs,control:&OperationControl) -> Result<Harvest> {
    let mut result=Harvest { declaration_sites:BTreeMap::new(), blocks:BTreeMap::new(), local_reads:BTreeMap::new(), signatures:BTreeMap::new(), intrinsic_declarations:BTreeMap::new(), raw:vec![] };
    let mut node_count=0;
    for name in names {
        control.check().map_err(|e|e.to_string())?;
        let path=inputs.corpus.root.join(name);
        let id=analysis.get_file_id(&file_path_to_uri(&path).ok_or("URI")?).ok_or("file")?;
        let model=analysis.semantic_model(id);
        let tree=model.syntax_tree().ok_or("tree")?;
        if !model.parse_errors().unwrap_or_default().is_empty() { return Err(format!("fixture parse error: {name}")); }
        let root=tree.get_red_root();
        let chunk=LuaChunk::cast(root.clone()).ok_or("chunk")?;
        let body=ScriptBody::from_chunk(&chunk,inputs.analysis_text(name).map_err(|error|error.to_string())?)?;
        result.blocks.insert((*name).into(),body.clone());
        if let ScriptBody::Block(block)=body {for stat in block.get_stats(){if let LuaStat::FuncStat(stat)=stat{
            let expr=stat.get_func_name().ok_or("host declaration")?.to_expr();let site=inputs.site(name,expr.get_range());
            if let Some(policy)=inputs.host.data.intrinsics.iter().find(|policy|policy.file==*name&&policy.start==site.start&&policy.end==site.end){result.intrinsic_declarations.insert(site.id(),policy.name.clone());}
        }}}
        let facts=model.file_facts().ok_or("file facts")?;
        for decl in &facts.decls {if decl.kind.is_local(){let site=inputs.site(name,decl.name_range);result.declaration_sites.insert(site.id(),SyntaxSite::from_site(inputs,&site).expect("admitted declaration file"));}}
        let evidence_file:Arc<str>=Arc::from(*name);
        for node in root.descendants() {
            node_count+=1;
            control.check().map_err(|e|e.to_string())?;
            if node_count>inputs.corpus.limits.ast_nodes { return Err("AST node budget exceeded".into()); }
            if let Some(expr)=LuaNameExpr::cast(node) {
                let site=inputs.site(name,expr.get_range());
                let raw=model.resolve_name(expr.get_position());
                let local=raw.as_ref().and_then(|id|facts.decl_by_id(id)).filter(|decl|decl.kind.is_local());
                if let Some(decl)=local { result.local_reads.insert(SyntaxSite::from_site(inputs,&site).expect("admitted reference file"),SyntaxSite::from_site(inputs,&inputs.site(name,decl.name_range)).expect("admitted declaration file")); }
                result.raw.push(RawEmmyEvidence::new(Arc::clone(&evidence_file),&site,raw));
            }
        }
        for sig in &facts.signatures {
            let Some(docs)=&sig.docs else {continue};
            if sig.is_method || sig.is_variadic || !docs.generic_params.is_empty() || !docs.nullable_params.is_empty() || !docs.overloads.is_empty() || !docs.return_overloads.is_empty() || docs.return_cast.is_some() || !docs.constructor_params.is_empty() || docs.returns.len()!=1 || docs.param_types.len()!=sig.param_names.len() {continue;}
            let primitive=|syntax: emmylua_parser::LuaSyntaxId| -> Option<String> {
                let node=syntax.to_node_from_root(&root)?;
                let text=node.text().to_string();
                match text.trim() {"number"|"string"|"boolean"|"nil"=>Some(text.trim().into()),_=>None}
            };
            let Some(ret)=primitive(docs.returns[0]) else {continue};
            let params:Option<Vec<_>>=sig.param_names.iter().map(|name| {
                let (_,syntax)=docs.param_types.iter().find(|(n,_)|n==name)?;
                Some((name.to_string(),primitive(*syntax)?))
            }).collect();
            let Some(params)=params else {continue};
            let Some(node)=sig.closure_syntax.to_node_from_root(&root) else {continue};
            result.signatures.insert(inputs.site(name,node.text_range()).id(),Signature {parameters:params,returns:ret,evidence:"Emmy signature-owned explicit primitive doc AST; no body inference".into()});
        }
    }
    Ok(result)
}

#[derive(Clone)]
struct Context { file: String, occurrence: usize, name: String, deferred: bool, parameters: BTreeMap<String,Decision>, conditional_branch:Option<usize> }
#[derive(Clone,Default,Debug,Serialize,PartialEq,Eq)]
pub struct ObjectState { pub fields:BTreeMap<String,usize>, pub unknown:Option<String>, pub unknown_observation:usize }
struct AssignmentTarget { site:Site, destination:AssignmentDestination }
enum AssignmentDestination {
    Cell(String),
    Field { object:usize, key:Option<String>, receiver_fact:usize },
    UncertainField { receiver_fact:usize },
}
struct Builder { inputs:Arc<SourceInputs>, control:OperationControl, graph:GraphFacts, harvest:Harvest, cells:BTreeMap<String,usize>, objects:Vec<ObjectState>, bodies:Vec<(usize,LuaClosureExpr)>,
    heap_observation: usize, object_versions:Vec<usize>, object_view_ids: BTreeMap<ObjectViewKey,usize>, object_slots: usize,
    field_write_observations: BTreeMap<usize,usize>,
    observation_failure: Option<String>,
    native: NativeInvocationState,
    xml_objects:BTreeMap<String,(usize,usize)>,
}
impl Builder {
    fn budget(&mut self) -> Result<()> {
        self.control.check().map_err(|e|e.to_string())?;
        if let Some(reason)=&self.observation_failure{return Err(reason.clone())}
        self.inputs.charge(1).map_err(|error|error.to_string())?;
        let (uses,values,writes)=if self.inputs.corpus.limits.research{(200000,100000,100000)}else{(5000,2000,2000)};
        if self.graph.uses.len()>uses || self.graph.values.len()>values || self.graph.writes.len()>writes {Err("graph construction budget exceeded".into())}else{Ok(())}
    }
    fn value(&mut self, site:&Site,kind:ValueKind)->Decision {
        let id=self.graph.values.len(); self.graph.values.push(AbstractValue{id,origin:ValueOrigin::Source(site.clone()),kind});Decision::Bound(id)
    }
    fn usage(&mut self,site:Site,ctx:&Context,decision:Decision,writes:Vec<usize>)->usize {
        let id=self.graph.uses.len();
        let object_view={
            match self.object(&decision).map(|object|self.capture_object_view(object,0)) {
                Some(Ok(view))=>Some(view),
                Some(Err(reason))=>{self.observation_failure.get_or_insert(reason);None},
                None=>None,
            }
        };
        let proof_id=digest(&(self.graph.snapshot_key.as_str(),id,&site,&ctx.name,&decision,&writes,object_view));
        self.graph.uses.push(UseFact{id,site,context:ctx.name.clone(),decision,writes,proof_id,object_view,host_return_slot:None});id
    }
    fn allocate_object(&mut self,site:&Site)->Result<usize> {
        if self.objects.len()>=self.object_limit(){return Err("object identity budget exceeded".into())}
        let object=self.objects.len();self.objects.push(ObjectState::default());self.observe_heap(object,"create",site.clone(),None,None);Ok(object)
    }
    fn object_limit(&self)->usize{self.inputs.corpus.limits.object_limit()}
    fn observe_heap(&mut self,object:usize,kind:&'static str,site:Site,write:Option<usize>,reason:Option<String>) {
        if self.graph.object_mutations.len()>=OBJECT_MUTATION_LIMIT{self.observation_failure.get_or_insert("object mutation budget exceeded".into());return}
        self.heap_observation+=1;
        self.object_versions.resize(self.objects.len(),0);self.object_versions[object]=self.heap_observation;
        if kind=="field_write"&&let Some(write)=write{self.field_write_observations.insert(write,self.heap_observation);}
        self.graph.object_mutations.push(ObjectMutationFact{observation:self.heap_observation,object_id:object,kind,site,write,reason});
    }
    fn unknown_object(&mut self,object:usize,reason:String,site:Site) {
        self.objects[object].unknown=Some(reason.clone());self.observe_heap(object,"unknown",site,None,Some(reason));
        self.objects[object].unknown_observation=self.heap_observation;
    }
    fn unknown_call_object(&mut self,object:usize,site:Site) {
        const REASON:&str="ordinary_unknown_call_reachable_effect";
        let state=&self.objects[object];
        // The caller records this call's full effect footprint even when the
        // abstract object state is unchanged. A later field write requires a new
        // invalidation; its observation must never alias the former version.
        let unchanged=state.unknown.as_deref()==Some(REASON)&&state.unknown_observation>0
            &&state.fields.values().all(|write|self.field_write_observations.get(write).copied().unwrap_or(0)<=state.unknown_observation);
        if !unchanged{self.unknown_object(object,REASON.into(),site);}
    }
    fn object_observation(&self,object:usize)->Result<usize> {
        let mut pending=vec![(object,0)];let mut seen=std::collections::BTreeSet::new();let mut observation=0;
        while let Some((object,depth))=pending.pop(){
            if !seen.insert(object){continue}
            if depth>OBJECT_DEPTH_LIMIT{return Err("object observation depth budget exceeded".into())}
            if seen.len()>OBJECT_VIEW_LIMIT{return Err("object observation count budget exceeded".into())}
            let state=&self.objects[object];
            self.inputs.charge(1+state.fields.len()).map_err(|error|error.to_string())?;
            observation=observation.max(self.object_versions.get(object).copied().unwrap_or(0));
            for write in state.fields.values(){
                if state.unknown.is_some()&&self.field_write_observations.get(write).copied().unwrap_or(0)<=state.unknown_observation{continue}
                if let Some(child)=self.object(&self.graph.writes[*write].decision){pending.push((child,depth+1));}
            }
        }
        Ok(observation)
    }
    fn capture_object_view(&mut self,object:usize,depth:usize)->Result<usize> {
        if let Some(reason)=&self.observation_failure{return Err(reason.clone())}
        // An unrelated heap mutation does not create a new shape. Include every
        // reachable child version so alias writes and cycles remain observable.
        let observation=if self.inputs.corpus.limits.research{self.object_observation(object)?}else{self.heap_observation};
        let key=ObjectViewKey{context:self.graph.snapshot_key.clone(),observation,object_id:object};
        if let Some(id)=self.object_view_ids.get(&key){return Ok(*id)}
        if depth>OBJECT_DEPTH_LIMIT{return Err("object observation depth budget exceeded".into())}
        if self.graph.object_views.len()>=OBJECT_VIEW_LIMIT{return Err("object observation count budget exceeded".into())}
        let state=self.objects.get(object).ok_or("object observation identity missing")?;
        if state.fields.len()>OBJECT_FIELD_LIMIT||self.object_slots+state.fields.len()>OBJECT_SLOT_LIMIT{return Err("object observation field budget exceeded".into())}
        self.object_slots+=state.fields.len();
        let fields:Vec<_>=state.fields.iter().map(|(field,write)|(field.clone(),*write)).collect();
        let unknown=state.unknown.clone();
        let unknown_observation=state.unknown_observation;
        let mut enumeration=unknown.as_ref().map(|reason|ObjectEnumeration::Unavailable(reason.clone())).unwrap_or(ObjectEnumeration::Complete);
        let id=self.graph.object_views.len();self.object_view_ids.insert(key.clone(),id);
        // Reserve identity before following child references: a cyclic graph
        // uses view IDs, never recursive copies or a fictitious deep-copy value.
        self.graph.object_views.push(ObjectShapeFacts{id,key:key.clone(),fields:vec![],enumeration:enumeration.clone(),proof_id:String::new()});
        let mut slots=Vec::with_capacity(fields.len());
        for(field,write)in fields{
            let selected=self.graph.writes[write].decision.clone();
            let copy=self.graph.copies.iter().rev().find(|edge|edge.destination_object==object&&edge.destination_field_write==write).map(|edge|(edge.id,edge.source_field_write));
            let field_invalidated=self.field_write_observations.get(&write).copied().unwrap_or(0)<=unknown_observation;
            let selected_value=unknown.as_ref().filter(|_|field_invalidated).map(|reason|Decision::Unavailable(reason.clone())).unwrap_or(selected.clone());
            // Lua nil assignments remove a key. Keep the tombstone/write
            // provenance without reporting it as a present member. An unknown
            // value can also be nil, so exact enumeration is then unavailable.
            let presence=match &selected_value {
                Decision::KnownAbsent(_)=>SlotPresence::Absent,
                Decision::Bound(value)=>match &self.graph.values[*value].kind{
                    ValueKind::Primitive(kind) if kind=="nil"=>SlotPresence::Absent,
                    ValueKind::Primitive(_)|ValueKind::Function(_)|ValueKind::Object(_)|ValueKind::HostCallable(_)=>SlotPresence::Present,
                    ValueKind::CallResult(_)|ValueKind::NativeParameter(_)|ValueKind::HostReturn{..}=>SlotPresence::Unavailable,
                },
                Decision::Unavailable(_)|Decision::NotManaged=>SlotPresence::Unavailable,
            };
            if matches!(presence,SlotPresence::Unavailable)&&matches!(enumeration,ObjectEnumeration::Complete){enumeration=ObjectEnumeration::Unavailable(format!("field_presence_unavailable:{field}"));}
            let nested_object_view=match self.object(&selected_value){Some(child)=>Some(self.capture_object_view(child,depth+1)?),None=>None};
            let xml_copy=self.graph.xml.as_ref().and_then(|facts|facts.copies.iter().rev().find(|copy|copy.destination_field_write==write));
            slots.push(MemberSlotFact{key:field,presence,selected_value,destination_write:write,source_write:copy.map(|(_,source)|source).or_else(||xml_copy.map(|copy|copy.source_field_write)),copy_origin:copy.map(|(copy,_)|copy),nested_object_view,xml_copy_origin:xml_copy.map(|copy|copy.id)});
        }
        let proof_id=digest(&(&key,&slots,&enumeration));
        self.graph.object_views[id]=ObjectShapeFacts{id,key,fields:slots,enumeration,proof_id};Ok(id)
    }
    fn write(&mut self,cell:String,site:Site,ctx:&Context,decision:Decision,inputs:Vec<usize>)->usize {
        let id=self.graph.writes.len();let previous=self.cells.insert(cell.clone(),id);
        self.graph.writes.push(WriteVersion{id,cell,site,decision,previous,inputs,context:ctx.name.clone(),conditional_branch:ctx.conditional_branch});
        if let Some(branch)=ctx.conditional_branch{self.graph.branches[branch].write_versions.push(id);}
        id
    }
    fn read(&self,key:&str,ctx:&Context)->(Decision,Vec<usize>) {
        if let Some(value)=ctx.parameters.get(key) {return(value.clone(),vec![])}
        if ctx.deferred && self.graph.phase=="unknown" && key.starts_with("global:") {return(Decision::Unavailable("unknown_execution_phase".into()),vec![])}
        match self.cells.get(key) {
            Some(id) if ctx.deferred && self.graph.phase=="unknown" && self.graph.writes[*id].previous.is_some()=>(Decision::Unavailable("captured_mutable_local_execution_phase_unknown".into()),vec![*id]),
            Some(id)=>(self.graph.writes[*id].decision.clone(),vec![*id]),
            None if key.starts_with("global:")&&!self.inputs.host.data.closed=>(Decision::Unavailable("open_initial_host_environment".into()),vec![]),
            None if key.starts_with("global:")=>(Decision::KnownAbsent("closed_fixture_initial_environment".into()),vec![]),
            None=>(Decision::Unavailable("lexical_local_value_not_available".into()),vec![]),
        }
    }
    fn object(&self,decision:&Decision)->Option<usize> {
        let Decision::Bound(id)=decision else {return None};
        match self.graph.values[*id].kind {ValueKind::Object(object)=>Some(object),ValueKind::HostReturn{..}=>self.native.host.nominal_objects.get(id).copied(),_=>None}
    }
    fn function(&self,decision:&Decision)->Option<usize> {
        let Decision::Bound(id)=decision else {return None};
        match self.graph.values[*id].kind {ValueKind::Function(function)=>Some(function),_=>None}
    }
    fn static_key(key:Option<LuaIndexKey>)->Option<String> {
        match key? {LuaIndexKey::Name(token)=>Some(token.get_name_text().into()),LuaIndexKey::String(token)=>Some(token.get_value()),_=>None}
    }
    fn field(&self,object:usize,key:&str)->(Decision,Vec<usize>) {
        let state=&self.objects[object];
        if state.unknown_observation==0&&!state.fields.contains_key(key)&&let Some(value)=self.native.host.initial_fields.get(&(object,key.to_owned())){return(Decision::Bound(*value),vec![])}
        if let Some(reason)=&state.unknown {
            let restored=state.fields.get(key).is_some_and(|write|self.field_write_observations.get(write).is_some_and(|observation|*observation>state.unknown_observation));
            if !restored{return(Decision::Unavailable(reason.clone()),vec![])}
        }
        match state.fields.get(key) {Some(id)=>(self.graph.writes[*id].decision.clone(),vec![*id]),None=>(Decision::KnownAbsent("closed_empty_table_and_complete_field_write_history".into()),vec![])}
    }
    fn put_field(&mut self,object:usize,key:String,site:Site,ctx:&Context,decision:Decision,inputs:Vec<usize>)->usize {
        let cell=format!("object:{object}:field:{key}");
        {self.native.typed_cells.borrow_mut().insert(cell.clone(),StorageOwner::ObjectField{object,field:key.clone()});}
        let id=self.write(cell,site,ctx,decision,inputs);
        self.objects[object].fields.insert(key,id);self.observe_heap(object,"field_write",self.graph.writes[id].site.clone(),Some(id),None);
        if self.objects[object].fields.len()>OBJECT_FIELD_LIMIT{self.observation_failure.get_or_insert("object field count budget exceeded".into());}
        id
    }
    fn summarize_parameter_writes(&self,closure:&LuaClosureExpr,file:&str)->std::result::Result<FunctionEffectSummary,&'static str> {
        let mut parameters=vec![];
        if let Some(list)=closure.get_params_list(){for (index,param) in list.get_params().enumerate(){
            if index>=32 || param.is_dots(){return Err("effect_summary_parameter_shape_or_limit")}
            let token=param.get_name_token().ok_or("effect_summary_parameter_identity_missing")?;
            parameters.push(ParameterIdentity{index,declaration:self.inputs.site(file,token.get_range())});
        }}
        let block=closure.get_block().ok_or("effect_summary_body_missing")?;
        let mut writes=vec![];
        for stat in block.get_stats(){
            if writes.len()>=32{return Err("effect_summary_write_limit")}
            let LuaStat::AssignStat(stat)=stat else{return Err("effect_summary_statement_not_parameter_assignment")};
            let (targets,values)=stat.get_var_and_expr_list();
            if targets.len()!=1 || values.len()!=1{return Err("effect_summary_assignment_arity_not_supported")}
            let LuaExpr::IndexExpr(index)=targets[0].to_expr() else{return Err("effect_summary_target_not_static_parameter_field")};
            let field=Self::static_key(index.get_index_key()).ok_or("effect_summary_dynamic_field")?;
            let receiver=index.get_prefix_expr().ok_or("effect_summary_receiver_missing")?;
            if !matches!(receiver,LuaExpr::NameExpr(_)){return Err("effect_summary_receiver_not_parameter_name")}
            let receiver_site=self.inputs.site(file,receiver.get_range());
            let lexical_id=self.harvest.local_read(&self.inputs,&receiver_site).ok_or("effect_summary_receiver_lexical_identity_missing")?;
            let parameter=parameters.iter().find(|param|param.declaration.id()==lexical_id).ok_or("effect_summary_receiver_is_not_this_function_parameter")?.clone();
            let value_site=self.inputs.site(file,values[0].get_range());
            let value=match &values[0] {
                LuaExpr::NameExpr(_)=>EffectExpression::Name{site:value_site},
                LuaExpr::LiteralExpr(literal)=> {
                    let primitive=match literal.get_literal(){Some(LuaLiteralToken::Number(_))=>"number",Some(LuaLiteralToken::String(_))=>"string",Some(LuaLiteralToken::Bool(_))=>"boolean",Some(LuaLiteralToken::Nil(_))=>"nil",_=>return Err("effect_summary_literal_not_supported")};
                    EffectExpression::Literal{site:value_site,primitive:primitive.into()}
                }
                _=>return Err("effect_summary_rhs_not_name_or_primitive_literal"),
            };
            writes.push(ParameterFieldWrite{parameter,field,receiver_site,write_site:self.inputs.site(file,index.get_range()),value});
        }
        if writes.is_empty(){return Err("effect_summary_has_no_parameter_field_writes")}
        let id=digest(&(NATIVE_RULES,self.inputs.site(file,closure.get_range()),&parameters,&writes));
        Ok(FunctionEffectSummary{id,parameters,writes,return_rule:"implicit_nil_after_straight_line_writes".into()})
    }
    fn eval(&mut self,expr:LuaExpr,ctx:&Context,depth:usize)->Result<usize> { self.native_eval(expr,ctx,depth) }
    fn intrinsic(&mut self,name:&str,call:usize,site:&Site,ctx:&Context,args:&[usize])->Result<Decision> {
        if ctx.deferred{return Ok(Decision::Unavailable("deferred_intrinsic_effect_not_analyzed".into()))}
        let decisions:Vec<_>=args.iter().map(|id|self.graph.uses[*id].decision.clone()).collect();
        if name=="setmetatable" {
            if let Some(object)=decisions.first().and_then(|d|self.object(d)){self.unknown_object(object,"metatable_behavior_unavailable".into(),site.clone());}
            return Ok(Decision::Unavailable("metatable_behavior_unavailable".into()));
        }
        let (target,sources)=if name=="CreateFromMixins" {let target=self.allocate_object(site)?;(target,decisions.as_slice())}
        else {let Some(target)=decisions.first().and_then(|d|self.object(d))else{return Ok(Decision::Unavailable("mixin_target_not_known_table".into()))};(target,&decisions[1..])};
        for decision in sources {
            let Some(source)=self.object(decision)else{self.unknown_object(target,"mixin_source_not_known_table".into(),site.clone());return Ok(Decision::Unavailable("mixin_source_not_known_table".into()))};
            let state=self.objects[source].clone();
            if let Some(reason)=state.unknown {self.unknown_object(target,reason.clone(),site.clone());return Ok(Decision::Unavailable(reason));}
            for (field,source_write) in state.fields {
                let selected=self.graph.writes[source_write].decision.clone();
                {
                    let absent=matches!(&selected,Decision::KnownAbsent(_))||matches!(&selected,Decision::Bound(value) if matches!(&self.graph.values[*value].kind,ValueKind::Primitive(kind) if kind=="nil"));
                    if absent{continue}
                    let uncertain=matches!(&selected,Decision::Unavailable(_)|Decision::NotManaged)||matches!(&selected,Decision::Bound(value) if matches!(&self.graph.values[*value].kind,ValueKind::CallResult(_)|ValueKind::NativeParameter(_)|ValueKind::HostReturn{..}));
                    if uncertain{self.unknown_object(target,"mixin_source_field_presence_unavailable".into(),site.clone());return Ok(Decision::Unavailable("mixin_source_field_presence_unavailable".into()))}
                }
                let destination_write=self.put_field(target,field.clone(),site.clone(),ctx,selected.clone(),vec![self.graph.calls[call].callee]);
                if self.graph.copies.len()>=OBJECT_SLOT_LIMIT{return Err("object copy edge budget exceeded".into())}
                let id=self.graph.copies.len();self.graph.copies.push(CopyEdge{id,call,source_object:source,source_field_write:source_write,destination_object:target,destination_field_write:destination_write,field,selected_value:selected,profile_rule:format!("{PROFILE}:{name}:left-to-right-shallow-copy-assumption")});
            }
        }
        Ok(self.value(site,ValueKind::Object(target)))
    }
    fn apply_assignment(&mut self,target:AssignmentTarget,decision:Decision,mut input:Vec<usize>,ctx:&Context) {
        match target.destination {
            AssignmentDestination::Cell(cell)=>{self.write(cell,target.site,ctx,decision,input);}
            AssignmentDestination::Field{object,key,receiver_fact}=> {
                // The receiver use proves which immutable object was captured
                // before any sibling assignment changed its source cell.
                input.push(receiver_fact);
                if let Some(key)=key{self.put_field(object,key,target.site,ctx,decision,input);}
                else{self.unknown_object(object,"dynamic_field_write".into(),target.site);}
            }
            AssignmentDestination::UncertainField{receiver_fact}=>self.native_uncertain_field_write(target.site,ctx,receiver_fact,decision,input),
        }
    }
    fn block(&mut self,block:LuaBlock,ctx:&Context)->Result<()> { let paths=self.native_block(block,ctx,0)?; self.native_load_terminals(paths) }
    fn deferred_bodies(&mut self)->Result<()> { self.native_static_bodies() }
}

#[cfg(test)]
#[path = "empty_script_tests.rs"]
mod empty_script_tests;

// All public semantic views below consume only the frozen graph. No Emmy model,
// AST, name lookup, or fallback resolver is reachable from this facade.
pub struct Facade<'a> { pub graph:&'a Graph }
impl Facade<'_> {
    pub fn selected_function(&self,decision:&Decision)->Option<&Function> {
        let Decision::Bound(id)=decision else{return None};
        if let ValueKind::Function(function)=self.graph.values[*id].kind{Some(&self.graph.functions[function])}else{None}
    }
    pub fn value_type(&self,decision:&Decision,depth:usize)->std::result::Result<String,String> {
        if depth>64{return Err("type_depth_budget".into())}
        match decision {
            Decision::KnownAbsent(_)=>Ok("nil".into()),
            Decision::Unavailable(reason)=>Err(reason.clone()),
            Decision::NotManaged=>Err("not_managed_no_fallback_provider_installed".into()),
            Decision::Bound(id) if self.graph.values[*id].origin.environment().is_some()=>Err("environment_value_type_unavailable".into()),
            Decision::Bound(id) if self.graph.values[*id].origin.source().is_none()=>Err("entry_value_type_unavailable".into()),
            Decision::Bound(id)=>match &self.graph.values[*id].kind {
                ValueKind::NativeParameter(_)=>Err("native_parameter_requires_native_projection".into()),
                ValueKind::HostCallable(_)|ValueKind::HostReturn{..}=>Err("host_value_type_unavailable".into()),
                ValueKind::Primitive(kind)=>Ok(kind.clone()),ValueKind::Object(_)=>Ok("table".into()),ValueKind::Function(_)=>Ok("function".into()),
                ValueKind::CallResult(call)=> {
                    let callee=&self.graph.uses[self.graph.calls[*call].callee];
                    self.selected_function(&callee.decision).and_then(|f|f.signature.as_ref()).map(|sig|sig.returns.clone()).ok_or("selected_function_signature_unavailable".into())
                }
            },
        }
    }
    pub fn binding(&self,fact:&UseFact)->Value {
        let selected=self.selected_function(&fact.decision);
        json!({"factId":fact.id,"proofId":fact.proof_id,"site":fact.site,"context":fact.context,"decision":fact.decision,
            "selectedValueId":if let Decision::Bound(id)=fact.decision{Some(id)}else{None},
            "selectedDeclaration":selected.map(|f|&f.declaration),"selectedFunctionName":selected.map(|f|&f.name),
            "writeVersions":fact.writes,"signature":selected.and_then(|f|f.signature.as_ref())})
    }
    pub fn type_view(&self,fact:&UseFact)->Value {
        match self.value_type(&fact.decision,0){Ok(kind)=>json!({"factId":fact.id,"proofId":fact.proof_id,"state":"known","type":kind}),Err(reason)=>json!({"factId":fact.id,"proofId":fact.proof_id,"state":"unavailable","reason":reason})}
    }
    pub fn call_view(&self,call:&CallFact)->Value {
        let callee=&self.graph.uses[call.callee];let function=self.selected_function(&callee.decision);
        let signature=function.and_then(|f|f.signature.as_ref());
        let Some(result_id)=call.result_fact else{return json!({"callId":call.id,"site":call.site,"context":call.context,"invocationCoverage":"unavailable","resultCoverage":"unavailable","unavailableReason":"call_stopped_at_explicit_control_boundary","resultFactId":null})};
        let result=&self.graph.uses[result_id];
        let result_view=self.type_view(result);
        let argument_types:Vec<_>=call.arguments.iter().map(|id|self.value_type(&self.graph.uses[*id].decision,0)).collect();
        let mut diagnostics=vec![];
        let coverage=if call.argument_mapping_unavailable_reason.is_some(){"unavailable"}
        else if call.intrinsic.is_some(){if matches!(call.invocation_coverage,CallCoverage::ProfileContract){"profile_contract"}else{"unavailable"}}
        else if let Some(signature)=signature {
            if call.arguments.len()!=signature.parameters.len(){diagnostics.push(json!({"code":"argument_count_mismatch","expected":signature.parameters.len(),"actual":call.arguments.len(),"factId":callee.id,"proofId":callee.proof_id}));}
            for (index,((_,expected),actual)) in signature.parameters.iter().zip(&argument_types).enumerate(){if let Ok(actual)=actual && actual!=expected{let argument=&self.graph.uses[call.arguments[index]];diagnostics.push(json!({"code":"argument_type_mismatch","index":index,"expected":expected,"actual":actual,"site":argument.site,"factId":callee.id,"proofId":callee.proof_id,"argumentFactId":argument.id,"argumentProofId":argument.proof_id}));}}
            if argument_types.iter().all(|r|r.is_ok()){"checked"}else{"unavailable"}
        }else{"unavailable"};
        json!({"callId":call.id,"site":call.site,"context":call.context,"factId":callee.id,"proofId":callee.proof_id,
            "binding":self.binding(callee),"signature":signature,"argumentFactIds":call.arguments,
            "argumentTypes":argument_types.iter().map(|r|r.as_ref().ok()).collect::<Vec<_>>(),
            "argumentTypeEvidence":argument_types,"declaredReturnType":signature.map(|s|&s.returns),"argumentCheckCoverage":coverage,
            "invocationCoverage":call.invocation_coverage,"effectCoverage":call.effect_coverage,
            "conditionalBranchId":call.conditional_branch,"executionState":if call.conditional_branch.is_some(){"conditional_possible"}else if call.context.starts_with("function:"){"deferred_expression_query"}else{"load_call"},
            "effectSummary":function.and_then(|f|f.effect_summary.as_ref()),"effectSummaryUnavailableReason":function.and_then(|f|f.effect_summary_unavailable_reason.as_ref()),
            "parameterBindings":call.parameter_bindings,"effectWriteVersions":call.effect_write_versions,
            "resultFactId":result.id,"resultProofId":result.proof_id,"resultType":result_view.get("type"),"resultCoverage":result_view["state"],"resultTypeView":result_view,
            "returnType":signature.map(|s|&s.returns),"checkCoverage":coverage,"diagnostics":diagnostics,
            "unavailableReason":if coverage=="unavailable" {Some(call.argument_mapping_unavailable_reason.as_deref().unwrap_or(match &callee.decision{Decision::Unavailable(reason)=>reason.as_str(),_=>"selected_signature_or_argument_type_unavailable"}))}else{None},
            "invocationUnavailableReason":if matches!(call.invocation_coverage,CallCoverage::Unavailable){result_view.get("reason")}else{None},"intrinsic":call.intrinsic})
    }
    pub fn return_view(&self,check:&ReturnCheck)->Value {
        let fact=&self.graph.uses[check.expression];let function=&self.graph.functions[check.function];
        let actual=self.value_type(&fact.decision,0);let declared=function.signature.as_ref().map(|s|s.returns.as_str());
        let coverage=if actual.is_ok()&&declared.is_some(){"checked"}else{"unavailable"};
        let mismatch=actual.as_ref().ok().zip(declared).map(|(a,d)|a!=d);
        json!({"functionId":function.id,"functionName":function.name,"functionDeclaration":function.declaration,"expressionSite":fact.site,
            "factId":fact.id,"proofId":fact.proof_id,"context":check.context,"declaredReturnType":declared,"actualReturnType":actual.as_ref().ok(),
            "checkCoverage":coverage,"mismatch":mismatch,"diagnostic":if mismatch==Some(true){Some("return_type_mismatch")}else{None},"typeEvidence":actual})
    }
    pub fn views(&self)->Value {
        let uses:Vec<_>=self.graph.uses.iter().map(|fact|json!({"binding":self.binding(fact),"type":self.type_view(fact),"definitionFactId":fact.id,"referencesFactId":fact.id,"checkFactId":fact.id})).collect();
        let mut inverse:BTreeMap<usize,Vec<Value>>=BTreeMap::new();
        for fact in &self.graph.uses {if self.selected_function(&fact.decision).is_some() && let Decision::Bound(value)=fact.decision{inverse.entry(value).or_default().push(json!({"factId":fact.id,"proofId":fact.proof_id,"site":fact.site,"context":fact.context}));}}
        let mut inverse_callees:BTreeMap<usize,Vec<Value>>=BTreeMap::new();
        for call in &self.graph.calls{let fact=&self.graph.uses[call.callee];if self.selected_function(&fact.decision).is_some() && let Decision::Bound(value)=fact.decision{inverse_callees.entry(value).or_default().push(json!({"factId":fact.id,"proofId":fact.proof_id,"site":fact.site,"context":fact.context,"callId":call.id}));}}
        json!({"uses":uses,"calls":self.graph.calls.iter().map(|c|self.call_view(c)).collect::<Vec<_>>(),"returnChecks":self.graph.returns.iter().map(|r|self.return_view(r)).collect::<Vec<_>>(),"inverseReferences":inverse,"inverseCalleeReferences":inverse_callees,
            "checkScope":"return-expression and call-site checks only; no whole-function or whole-program checking claim",
            "managedFallbackCalls":0,"ownership":"Bound, Unavailable and KnownAbsent are authoritative; only NotManaged permits a future fallback provider"})
    }
    pub fn site_fact(&self,file:&str,marker:&str,token:&str,context_prefix:&str)->Result<&UseFact> {
        let text=self.graph.inputs.original(file).map_err(|e|e.to_string())?;let marker_at=text.find(marker).ok_or("missing assertion marker")?;
        if text[marker_at+marker.len()..].contains(marker){return Err("non-unique assertion marker".into())}
        let start=(marker_at+marker.find(token).ok_or("missing assertion token")?) as u32;
        let found:Vec<_>=self.graph.uses.iter().filter(|f|f.site.file==file&&f.site.start==start&&f.site.end==start+token.len()as u32&&f.context.starts_with(context_prefix)).collect();
        if found.len()!=1{return Err(format!("expected one fact: {file} {marker} {token}, got {}",found.len()))}Ok(found[0])
    }
}

fn build_graph(prepared:&PreparedSnapshot,name:&str,snapshot_inputs:GraphSnapshotEvidence,inputs:Arc<SourceInputs>)->Result<Graph> {
    let analysis=&prepared.analysis;let control=&prepared.control;let owner=prepared.owner.clone();
    let phase=inputs.phase.clone();let occurrence_files=inputs.ordered.clone();let ordered:Vec<&str>=occurrence_files.iter().map(String::as_str).collect();
    control.check().map_err(|e|e.to_string())?;
    let mut unique=match &inputs.load_plan{Some(LoadPlan::Unknown(files))=>files.iter().map(String::as_str).collect(),_=>ordered.to_vec()};unique.sort();unique.dedup();
    let snapshot_inputs=Arc::new(snapshot_inputs);
    let (snapshot_inputs_json,snapshot_key)=SnapshotInputsJson::new(Arc::clone(&snapshot_inputs),control,if inputs.corpus.limits.research{32<<20}else{2097152}).map_err(|e|e.to_string())?;
    let mut harvest=harvest(analysis,&unique,&inputs,control)?;
    let graph=GraphFacts{snapshot_key,snapshot_inputs,snapshot_inputs_json,scenario:name.into(),phase,script_load_occurrences:ordered.iter().enumerate().map(|(i,n)|format!("{i}:{n}")).collect(),values:vec![],functions:vec![],writes:vec![],uses:vec![],calls:vec![],copies:vec![],returns:vec![],branches:vec![],raw_emmy:std::mem::take(&mut harvest.raw),object_views:vec![],object_mutations:vec![],native_invocations:None,host_runtime:None,xml:None};
    let mut builder=Builder{inputs:inputs.clone(),control:control.clone(),graph,harvest,cells:BTreeMap::new(),objects:vec![],bodies:vec![],heap_observation:0,object_versions:vec![],object_view_ids:BTreeMap::new(),object_slots:0,field_write_observations:BTreeMap::new(),observation_failure:None,native:NativeInvocationState::default(),xml_objects:BTreeMap::new()};
    builder.native.facts.global_environment_open = !inputs.host.data.closed;
    builder.host_initialize()?;
    if let Some(plan@LoadPlan::Unknown(_))=&inputs.load_plan{
        builder.native.facts.control.complete=false;builder.native.facts.control.coverage="unavailable_execution_plan".into();
        builder.native.facts.execution_boundaries.push(ExecutionBoundary{kind:NativeBoundaryKind::Knowledge,reason:"load_order_unknown".into(),load_plan:plan.clone()});
    }
    let load_result=if let Some(program)=inputs.xml.as_ref(){builder.xml_load(program)}else{
        let mut result=Ok(());
        for (occurrence,file) in ordered.iter().enumerate(){
            result=builder.load_file(file,occurrence);
            if result.is_err()||!builder.native.facts.control.complete{break;}
        }
        result
    };
    if let Err(reason)=load_result{
        control.check().map_err(|e|e.to_string())?;
        if (builder.native.facts.control.complete||reason!="native invocation stopped at explicit control boundary")&&!builder.native_recover_budget(&reason)?{return Err(reason)}
    }    if builder.native.facts.control.complete && let Some(request)=inputs.entry.clone()
        && let Err(reason)=builder.native_explicit_entry(prepared,request){
        control.check().map_err(|error|error.to_string())?;
        if (reason!="native invocation stopped at explicit control boundary" || builder.native.facts.control.complete)
            && !builder.native_recover_budget(&reason)?{return Err(reason)}
    }
    if builder.native.facts.control.complete
        && let Err(reason)=builder.deferred_bodies(){control.check().map_err(|e|e.to_string())?;
            if !builder.native_recover_budget(&reason)?{return Err(reason)}
    }
    {
        builder.flow_publish_contexts();
        builder.host_finish_facts()?;
        if builder.native.typed_cells.borrow().len()>8192{return Err("typed storage identity budget exceeded".into())}
        builder.native.facts.storages=builder.native.typed_cells.borrow().iter().map(|(cell,owner)|StorageFact{cell:cell.clone(),owner:owner.clone()}).collect();
    }
    let complete=builder.native.facts.control.complete;
    if complete {if let Some(reason)=builder.observation_failure{return Err(reason)}
        if builder.graph.object_views.iter().any(|view|view.proof_id.is_empty()){return Err("cannot freeze an incomplete object observation".into())}
        if builder.graph.calls.iter().any(|call|call.result_fact.is_none()){return Err("cannot freeze a call without its result fact".into())}
    }
    control.check().map_err(|e|e.to_string())?;
    {builder.graph.native_invocations=Some(builder.native.facts);}
    Ok(Graph{facts:builder.graph,inputs,control:control.clone(),owner,research_control:Default::default()})
}
