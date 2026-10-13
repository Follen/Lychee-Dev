// Explicit abstract entry is input evidence, never a fabricated source call.
pub const ENTRY_RULES:&str="source-explicit-entry-load-plan-v2";
#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
#[serde(tag="kind",content="files",rename_all="snake_case")]
pub enum LoadPlan { Known(Vec<String>), Unknown(Vec<String>) }
#[derive(Clone,Debug,Serialize,PartialEq)]
#[serde(tag="kind",content="value",rename_all="snake_case")]
pub enum EntryArgument { Unknown, Nil, Boolean(bool), Number(f64), String(String) }
#[derive(Clone,Debug,Serialize,PartialEq)]
#[serde(tag="kind",content="values",rename_all="snake_case")]
pub enum EntryArguments { Unspecified, Exact(Vec<EntryArgument>), XmlHandler(usize) }
#[derive(Clone,Debug,Serialize,PartialEq)]
pub struct EntryRequest {
    pub candidate:String, pub load_occurrence:Option<usize>, pub creation_context:Option<String>,
    pub arguments:EntryArguments,
}
#[derive(Clone,Debug,Serialize,PartialEq,Eq)]
pub struct EntryCandidate { pub id:String, pub closure:Site, pub start_line:usize, pub end_line:usize }
#[derive(Clone,Debug,Serialize)]
#[serde(tag="state",rename_all="snake_case")]
pub enum EntryResolution { Unique{candidate:EntryCandidate}, Absent, SelectorRequired{candidates:Vec<EntryCandidate>} }
#[derive(Clone,Copy,Debug,Serialize)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum NativeInvocationOrigin { SourceCall{call:usize}, ExplicitEntry{entry:usize} }
#[derive(Clone,Debug,Serialize)]
pub struct NativeEntryFact {
    pub id:usize, pub request:EntryRequest, pub closure:usize, pub frame:Option<usize>,
    pub parameters:Vec<ParameterBinding>, pub effect_write_versions:Vec<usize>, pub return_facts:Vec<usize>,
    pub argument_values:Vec<Decision>, pub assumption:EntryExistenceAssumption, pub effects_complete:bool, pub proof_id:String,
}
#[derive(Clone,Debug,Serialize)]
pub struct EntryExistenceAssumption {
    pub rule:String, pub creation_context:String, pub creation_occurrence:Option<usize>,
    pub paths:Vec<Vec<NativePredicate>>, pub conditional_branch:Option<usize>,
}
#[derive(Clone,Debug,Serialize)]
pub struct ExecutionBoundary { pub kind:NativeBoundaryKind, pub reason:String, pub load_plan:LoadPlan }

fn original_line_at(text:&str,offset:usize)->usize {
    let bytes=text.as_bytes();let mut line=1;let mut i=0;
    while i<offset && i<bytes.len(){let ch=bytes[i];i+=1;if ch==b'\r'||ch==b'\n'{
        if i<bytes.len() && bytes[i]!=ch && (bytes[i]==b'\r'||bytes[i]==b'\n'){i+=1;}line+=1;
    }}line
}
impl PreparedSnapshot {
    pub fn entry_catalog(&self,control:&OperationControl)->lycheedev_core::Result<&[EntryCandidate]>{
        require_same_control(&self.control,control)?;Ok(&self.entry_catalog)
    }
    /// Resolves a declared function name to source closures; it never chooses a loaded instance.
    pub fn entry_symbol_candidates(&self,file:&str,name:&str,control:&OperationControl)->lycheedev_core::Result<EntryResolution>{
        require_same_control(&self.control,control)?;
        let file_id=*self.files.get(file).ok_or_else(||input_error("entry file is not admitted"))?;
        let model=self.analysis.semantic_model(file_id);
        let root=model.syntax_tree().ok_or_else(||input_error("entry syntax missing"))?.get_red_root();
        let mut candidates=Vec::new();
        for node in root.descendants(){
            control.check()?;self.inputs.charge(1)?;
            let closure=if let Some(stat)=emmylua_parser::LuaFuncStat::cast(node.clone()){
                stat.get_func_name().filter(|item|item.get_text()==name).and_then(|_|stat.get_closure())
            }else if let Some(stat)=emmylua_parser::LuaLocalFuncStat::cast(node){
                stat.get_local_name().and_then(|item|item.get_name_token()).filter(|item|item.get_text()==name).and_then(|_|stat.get_closure())
            }else{None};
            if let Some(closure)=closure{candidates.push(self.entry_candidate(file,closure.get_syntax_id()).map_err(|message|input_error(&message))?);}
        }
        Ok(match candidates.len(){0=>EntryResolution::Absent,1=>EntryResolution::Unique{candidate:candidates.remove(0)},_=>EntryResolution::SelectorRequired{candidates}})
    }
    fn entry_candidate(&self,file:&str,syntax:LuaSyntaxId)->Result<EntryCandidate>{
        let range=syntax.get_range();let mapped=self.inputs.original_range(file,range).map_err(|error|error.to_string())?;let site=Site::at(file,mapped);
        let original=self.inputs.original(file).map_err(|error|error.to_string())?;
        let input=self.inputs.file(file)?;
        Ok(EntryCandidate{id:digest(&(ENTRY_RULES,&input.identity,&site)),start_line:original_line_at(original,site.start as usize),end_line:original_line_at(original,site.end.saturating_sub(1) as usize),closure:site})
    }
    /// All closures intersecting this original-source line. Never picks by spelling or first match.
    pub fn entry_candidates(&self,file:&str,line:usize,control:&OperationControl)->lycheedev_core::Result<EntryResolution>{
        require_same_control(&self.control,control)?;
        let original=self.inputs.original(file)?;
        if line==0 || line>original_line_at(original,original.len()){return Err(input_error("entry line outside original source"));}
        let mut candidates=Vec::new();
        for candidate in &self.entry_catalog{
            control.check()?;self.inputs.charge(1)?;
            if candidate.closure.file==file && candidate.start_line<=line && candidate.end_line>=line{candidates.push(candidate.clone());}
            if candidates.len()>INVOCATION_CLOSURE_LIMIT{return Err(input_error("entry candidate count budget exceeded"));}
        }
        candidates.sort_by_key(|candidate|(candidate.closure.start,candidate.closure.end));
        Ok(match candidates.len(){0=>EntryResolution::Absent,1=>EntryResolution::Unique{candidate:candidates.remove(0)},_=>EntryResolution::SelectorRequired{candidates}})
    }
    pub fn build_plan(&self,name:&str,phase:&str,plan:LoadPlan,entry:Option<EntryRequest>,scope:Option<&str>,control:&OperationControl)->lycheedev_core::Result<Graph>{
        self.build_program(name,phase,plan,entry,scope,None,control)
    }
    pub fn build_xml_plan(&self,name:&str,program:Arc<crate::xml_load::Program>,entry:Option<EntryRequest>,control:&OperationControl)->lycheedev_core::Result<Graph>{
        self.build_program(name,"after_load",LoadPlan::Known(program.lua_files()),entry,Some(crate::xml_load::RULES),Some(program),control)
    }
    #[allow(clippy::too_many_arguments)]
    fn build_program(&self,name:&str,phase:&str,plan:LoadPlan,entry:Option<EntryRequest>,scope:Option<&str>,xml:Option<Arc<crate::xml_load::Program>>,control:&OperationControl)->lycheedev_core::Result<Graph>{
        require_same_control(&self.control,control)?;
        if name.len()>256{return Err(input_error("graph name byte budget exceeded"));}
        let files=match &plan{LoadPlan::Known(files)|LoadPlan::Unknown(files)=>files};
        if files.len()>if self.inputs.corpus.limits.research{20000}else{256} || files.iter().any(|file|!self.inputs.corpus.index.contains_key(file)){return Err(input_error("invalid explicit load plan"));}
        if matches!(plan,LoadPlan::Unknown(_)) && (files.is_empty()||files.windows(2).any(|pair|pair[0]>=pair[1])){return Err(input_error("unknown load plan requires a sorted unique nonempty file set"));}
        if let Some(request)=&entry{
            if !valid_sha(&request.candidate)||request.creation_context.as_ref().is_some_and(|value|value.len()>1024){return Err(input_error("invalid entry selector"));}
            if let EntryArguments::Exact(values)=&request.arguments
                && (values.len()>INVOCATION_PARAMETER_LIMIT || values.iter().any(|value|matches!(value,EntryArgument::Number(number) if !number.is_finite())||matches!(value,EntryArgument::String(text) if text.len()>4096))){return Err(input_error("entry argument budget or numeric value invalid"));}
            self.inputs.charge(self.entry_catalog.len())?;
            let found=self.entry_catalog.iter().any(|candidate|candidate.id==request.candidate);
            if !found{return Err(input_error("entry candidate does not belong to admitted source"));}
        }
        let mut owned=(*self.inputs).clone();owned.phase=phase.into();owned.ordered=match &plan{LoadPlan::Known(files)=>files.clone(),LoadPlan::Unknown(_)=>vec![]};owned.load_plan=Some(plan);owned.entry=entry;owned.xml=xml;
        owned.validate_execution()?;owned.refresh_identity(control)?;
        let result=public_result(self.graph_owned(name,scope,true,Arc::new(owned)),control);control.check()?;result
    }
}
impl Builder {
    fn native_explicit_entry(&mut self,prepared:&PreparedSnapshot,request:EntryRequest)->Result<()> {
        let mut candidates=Vec::new();
        for instance in &self.native.facts.closures {
            self.inputs.charge(1).map_err(|error|error.to_string())?;
            let function=&self.graph.functions[instance.function];
            let syntax=*prepared.closures.get(&function.closure.id()).ok_or("entry closure source identity missing")?;
            let candidate=prepared.entry_candidate(&function.closure.file,syntax)?;
            if candidate.id==request.candidate && request.load_occurrence.is_none_or(|load|load==instance.load_occurrence)
                && request.creation_context.as_ref().is_none_or(|context|context==&instance.creation_context){candidates.push(instance.id);}
        }
        if candidates.len()!=1{return Err(format!("explicit entry requires one loaded closure instance; candidates={candidates:?}"));}
        let closure=candidates[0];let id=self.native.facts.entries.len();
        let mut argument_values=Vec::new();
        if let EntryArguments::XmlHandler(registration)=&request.arguments{
            let handler=self.graph.xml.as_ref().and_then(|facts|facts.handlers.get(*registration)).ok_or("XML handler registration missing")?;
            if handler.closure!=Some(closure){return Err("XML handler entry does not select its observed closure".into());}
            argument_values.push(Decision::Bound(handler.self_value));
        }
        if let EntryArguments::Exact(values)=&request.arguments {for (slot,argument) in values.iter().enumerate(){
            self.budget()?;
            let selected=match argument {
                EntryArgument::Unknown=>Decision::Unavailable(format!("explicit_entry_{id}_argument_{}_unknown",slot+1)),
                EntryArgument::Nil=>Decision::KnownAbsent("explicit_entry_nil_argument".into()),
                EntryArgument::Boolean(_) | EntryArgument::Number(_) | EntryArgument::String(_)=>{
                    let value=self.graph.values.len();let kind=match argument{EntryArgument::Boolean(_)=>"boolean",EntryArgument::Number(_)=>"number",_=>"string"};
                    self.graph.values.push(AbstractValue{id:value,origin:ValueOrigin::Entry{entry:id,slot:slot+1},kind:ValueKind::Primitive(kind.into())});
                    if let EntryArgument::Boolean(boolean)=argument{self.native.boolean_values.insert(value,*boolean);}
                    if let EntryArgument::Number(number)=argument{self.native.numeric_values.insert(value,*number);self.native.facts.control.numbers.insert(value,*number);}
                    Decision::Bound(value)
                }
            };argument_values.push(selected);
        }}
        let creation_context=self.native.facts.closures[closure].creation_context.clone();
        let creation=self.native.contexts.get(&creation_context).ok_or("entry closure creation context missing")?;
        let assumption=EntryExistenceAssumption{rule:"selected_closure_exists".into(),creation_context:creation_context.clone(),creation_occurrence:creation.occurrence,paths:creation.paths.clone(),conditional_branch:self.native.facts.context_branches.get(&creation_context).copied().flatten()};
        let proof_id=digest(&(self.graph.snapshot_key.as_str(),"explicit_entry",id,&request,closure,&argument_values,&assumption));
        self.native.facts.entries.push(NativeEntryFact{id,request,closure,frame:None,parameters:vec![],effect_write_versions:vec![],return_facts:vec![],argument_values,assumption,effects_complete:false,proof_id});
        self.native_invoke_closure(NativeInvocationOrigin::ExplicitEntry{entry:id},closure,None,&[])?;Ok(())
    }
    fn native_argument_slot(&self,origin:&NativeInvocationOrigin,args:&[usize],index:usize,missing:&str)->(Decision,Vec<usize>){
        match origin{
            NativeInvocationOrigin::SourceCall{..}=>self.host_list_slot(args,index,missing),
            NativeInvocationOrigin::ExplicitEntry{entry}=>{
                let fact=&self.native.facts.entries[*entry];
                let decision=match &fact.request.arguments{
                    EntryArguments::Unspecified=>Decision::Unavailable(format!("explicit_entry_{entry}_argument_{}_unspecified",index+1)),
                    EntryArguments::Exact(_)=>fact.argument_values.get(index).cloned().unwrap_or_else(||Decision::KnownAbsent(missing.into())),
                    EntryArguments::XmlHandler(_)=>fact.argument_values.get(index).cloned().unwrap_or_else(||Decision::Unavailable(format!("xml_handler_argument_{}_unspecified",index+1))),
                };(decision,vec![])
            }
        }
    }
}

fn omit_source_invocation_origin(origin:&Option<NativeInvocationOrigin>)->bool{!matches!(origin,Some(NativeInvocationOrigin::ExplicitEntry{..}))}
