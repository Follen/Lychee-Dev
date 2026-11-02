// Owned, data-only inputs. Investigated files are never an authority for host policy.
use lycheedev_core::{ToolkitError, control::OperationControl};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceMap { Identity, Utf8BomPrefix }

#[derive(Clone, Debug)]
pub struct SourceFileInput {
    pub path: String,
    pub original_utf8: String,
    pub content_sha256: String,
    pub analysis_utf8: String,
    pub map: SourceMap,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceLimits {
    #[serde(default)]
    pub research: bool,
    #[serde(skip_serializing_if="Option::is_none")]
    pub flow: Option<FlowBudget>,
    pub files: usize,
    pub input_bytes: usize,
    pub ast_nodes: usize,
    pub construction_steps: usize,
    pub output_bytes: usize,
    pub total_milliseconds: u64,
}
impl Default for SourceLimits {
    fn default() -> Self { Self { research:false, flow:None, files:16, input_bytes:65536, ast_nodes:6000, construction_steps:10000, output_bytes:2097152, total_milliseconds:30000 } }
}
impl SourceLimits {
    pub(crate) fn object_limit(&self)->usize{if self.research{20000}else{OBJECT_LIMIT}}
    /// Whole pinned-source admission is separate from bounded flow execution.
    pub fn research() -> Self { Self { research:true, files:20000, input_bytes:512<<20, ast_nodes:20000000, construction_steps:50000000, total_milliseconds:180000, ..Self::default() } }
    fn validate(&self)->lycheedev_core::Result<()> {
        if let Some(flow)=&self.flow { flow.validate()?; }
        let mut ceiling=Self::default();
        if self.research { ceiling=Self::research(); }
        if self.flow.is_some(){ceiling.files=32;ceiling.input_bytes=32<<20;ceiling.ast_nodes=2000000;ceiling.construction_steps=100000;}
        if self.files==0 || self.files>ceiling.files || self.input_bytes==0 || self.input_bytes>ceiling.input_bytes
            || self.ast_nodes==0 || self.ast_nodes>ceiling.ast_nodes || self.construction_steps==0 || self.construction_steps>ceiling.construction_steps
            || self.output_bytes==0 || self.output_bytes>ceiling.output_bytes || self.total_milliseconds==0 || self.total_milliseconds>ceiling.total_milliseconds {
            return Err(input_error("limits exceed the admitted source resource ceiling"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct LuacIdentity { pub version:String, pub platform:String, pub executable_sha256:String, pub executable_bytes:u64 }

pub struct SourceInputSpec {
    pub files:Vec<SourceFileInput>,
    pub workspace_root:PathBuf,
    pub source_identity:Value,
    pub phase:String,
    pub ordered_occurrences:Vec<String>,
    pub config:Emmyrc,
    pub host:TrustedHostEnvironment,
    pub luac:LuacIdentity,
    pub limits:SourceLimits,
}

#[derive(Clone, Debug)]
struct OwnedSourceFile { path:String, original:Arc<str>, analysis:Arc<str>, prefix:u32, identity:Value }

/// All fields are private: callers cannot change verified bytes, maps or policy after admission.
#[derive(Clone, Debug)]
pub struct SourceInputs {
    admission_control:OperationControl, shared_steps:Arc<std::sync::atomic::AtomicUsize>, corpus:Arc<SourceCorpus>, host:TrustedHostEnvironment,
    phase:String, ordered:Vec<String>, load_plan:Option<LoadPlan>, entry:Option<EntryRequest>, identity:Arc<SourceIdentityEvidence>, identity_sha256:String,
    graph_common:Arc<serde_json::Map<String,Value>>,
    xml:Option<Arc<crate::xml_load::Program>>,
}

#[derive(Debug)]
struct SourceCorpus {
    files:Vec<OwnedSourceFile>, index:BTreeMap<String,usize>, root:PathBuf,
    config:Emmyrc, luac:LuacIdentity, limits:SourceLimits, source_identity:Value,
}

fn input_error(message:&str)->ToolkitError { ToolkitError::new("source.native_invalid_input",message,4) }
fn controlled_hash(bytes:&[u8],control:&OperationControl)->lycheedev_core::Result<String> {
    let mut hasher=Sha256::new();for chunk in bytes.chunks(16384){control.check()?;hasher.update(chunk);}control.check()?;Ok(format!("{:x}",hasher.finalize()))
}
fn valid_sha(value:&str)->bool { value.len()==64 && value.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }
fn public_result<T>(result:Result<T>,control:&OperationControl)->lycheedev_core::Result<T> {
    control.check()?;
    result.map_err(|message|ToolkitError::new("source.native_unavailable",message,3))
}

impl SourceInputs {
    fn charge(&self,steps:usize)->lycheedev_core::Result<()> {
        self.admission_control.check()?;
        let mut used=self.shared_steps.load(std::sync::atomic::Ordering::Relaxed);
        loop {
            self.admission_control.check()?;
            let Some(next)=used.checked_add(steps).filter(|next|*next<=self.corpus.limits.construction_steps) else {
                return Err(ToolkitError::new("source.native_budget","shared operation step budget exceeded",3));
            };
            match self.shared_steps.compare_exchange_weak(used,next,std::sync::atomic::Ordering::Relaxed,std::sync::atomic::Ordering::Relaxed) {
                Ok(_)=>return Ok(()),
                Err(observed)=>used=observed,
            }
        }
    }
    pub fn admit(spec:SourceInputSpec,control:&OperationControl)->lycheedev_core::Result<Arc<Self>> {
        control.check()?;spec.limits.validate()?;
        if let Some(flow)=&spec.limits.flow {
            if spec.files.len()>flow.max_files { return Err(input_error("flow file budget exceeded")); }
            if control.remaining()?.as_millis()>u128::from(flow.max_millis) { return Err(input_error("flow requires the original total operation deadline")); }
        }
        require_same_control(&spec.host.admission_control,control)?;
        if !crate::engine_config::explicit_authenticated_inputs(&spec.config) {
            return Err(input_error("only explicit authenticated Lua 5.1 UTF8 inputs are admitted; external workspaces/libraries or filters are unsupported"));
        }
        if control.remaining()?.as_millis()>u128::from(spec.limits.total_milliseconds) {return Err(input_error("caller control exceeds the declared total operation budget"));}
        if spec.files.is_empty() || spec.files.len()>spec.limits.files {return Err(input_error("source file count budget exceeded"));}
        if !spec.workspace_root.is_absolute() || spec.workspace_root.to_string_lossy().len()>4096 {return Err(input_error("explicit absolute logical workspace root required"));}
        if !valid_sha(&spec.luac.executable_sha256) || spec.luac.executable_bytes==0 || spec.luac.version!="5.1.5" || spec.luac.platform!="windows-amd64" {return Err(input_error("explicit supported luac identity required"));}
        let mut metadata_nodes=0usize;let mut metadata_work=vec![(&spec.source_identity,0usize)];
        while let Some((value,depth))=metadata_work.pop(){
            control.check()?;metadata_nodes+=1;if depth>32 || metadata_nodes>8192{return Err(input_error("source identity depth or node budget"));}
            match value {Value::Array(items)=>{if items.len()>8192-metadata_work.len(){return Err(input_error("source identity pending node budget"))}metadata_work.extend(items.iter().map(|item|(item,depth+1)))},Value::Object(items)=>{if items.len()>8192-metadata_work.len(){return Err(input_error("source identity pending node budget"))}metadata_work.extend(items.values().map(|item|(item,depth+1)))},_=>{}}
            if metadata_work.len()>8192{return Err(input_error("source identity pending node budget"));}
        }
        let source_identity_bytes=match serialize_json(&spec.source_identity,control,65536){Ok(bytes)=>bytes,Err(_)=>{control.check()?;return Err(input_error("source identity serialization byte budget"))}};
        if source_identity_bytes.len()>65536 {return Err(input_error("source identity byte budget"));}
        let mut files=Vec::new();let mut index=BTreeMap::new();let mut folded=std::collections::BTreeSet::new();let mut total=0usize;
        for file in spec.files {
            control.check()?;
            if !crate::git::source_path(&file.path) || !folded.insert(file.path.to_lowercase()) {return Err(input_error("invalid or colliding source path"));}
            total=total.checked_add(file.original_utf8.len()).ok_or_else(||input_error("input byte count overflow"))?;
            if total>spec.limits.input_bytes || file.original_utf8.len()>u32::MAX as usize {return Err(input_error("source byte budget exceeded"));}
            let actual=controlled_hash(file.original_utf8.as_bytes(),control)?;
            if actual!=file.content_sha256 {return Err(input_error("source content hash mismatch"));}
            let prefix=match file.map {SourceMap::Identity=>0,SourceMap::Utf8BomPrefix=>{
                if !file.original_utf8.starts_with('\u{feff}') {return Err(input_error("BOM map requires the exact UTF8 BOM prefix"));}3
            }};
            if file.original_utf8.get(prefix as usize..)!=Some(file.analysis_utf8.as_str()) {return Err(input_error("analysis bytes do not equal the verified source-map transform"));}
            let analysis_sha=controlled_hash(file.analysis_utf8.as_bytes(),control)?;
            let identity=json!({"file":file.path,"bytes":file.original_utf8.len(),"sha256":actual,"analysisBytes":file.analysis_utf8.len(),"analysisSHA256":analysis_sha,"map":file.map});
            let original:Arc<str>=Arc::from(file.original_utf8);
            let analysis=if prefix==0{original.clone()}else{Arc::from(file.analysis_utf8)};
            index.insert(file.path.clone(),files.len());files.push(OwnedSourceFile{path:file.path,original,analysis,prefix,identity});
        }
        let corpus=Arc::new(SourceCorpus{files,index,root:spec.workspace_root,config:spec.config,luac:spec.luac,limits:spec.limits,source_identity:spec.source_identity});
        let mut inputs=Self{admission_control:control.clone(),shared_steps:Arc::new(std::sync::atomic::AtomicUsize::new(0)),corpus,host:spec.host,phase:spec.phase,ordered:spec.ordered_occurrences,load_plan:None,entry:None,xml:None,identity:Arc::new(SourceIdentityEvidence::empty()),identity_sha256:String::new(),graph_common:Arc::new(serde_json::Map::new())};
        inputs.validate_execution()?;
        for intrinsic in &inputs.host.data.intrinsics {
            let file=inputs.file(&intrinsic.file).map_err(|_|input_error("intrinsic declaration file is not admitted"))?;
            if file.identity["sha256"]!=intrinsic.source_sha256 || file.original.get(intrinsic.start as usize..intrinsic.end as usize)!=Some(intrinsic.name.as_str()) {return Err(input_error("trusted intrinsic declaration does not match exact admitted source bytes"));}
        }
        inputs.initialize_evidence(control)?;inputs.refresh_identity(control)?;Ok(Arc::new(inputs))
    }
    fn validate_execution(&self)->lycheedev_core::Result<()> {
        if !matches!(self.phase.as_str(),"unknown"|"after_load") || self.ordered.len()>if self.corpus.limits.research{20000}else{256} || self.ordered.iter().any(|name|!self.corpus.index.contains_key(name)) {return Err(input_error("unsupported phase or invalid ordered load occurrence"));}Ok(())
    }
    fn initialize_evidence(&mut self,control:&OperationControl)->lycheedev_core::Result<()> {
        control.check()?;
        let config=serde_json::to_value(&self.corpus.config).map_err(|_|input_error("effective Emmy config serialization"))?;
        require_same_control(&self.host.admission_control,control)?;
        let common=evidence_fields(json!({"schema":"lycheedev.native-source-inputs.v1","source":self.corpus.source_identity,"workspaceRoot":self.corpus.root,
            "files":self.corpus.files.iter().map(|file|&file.identity).collect::<Vec<_>>(),
            "effectiveEmmyrc":config,"engine":{"baseCommit":ENGINE_COMMIT,"baseArchiveSHA256":ENGINE_ARCHIVE_SHA256},
            "luac":self.corpus.luac,"syntaxCheck":{"state":"pending","requires":"trusted checker result bound to exact original source and executable bytes"},
            "limits":self.corpus.limits,"fixedGraphLimits":fixed_graph_limits(),"sourceMapProfile":self.source_map_profile(),"sourceExecution":false,"configExecution":false,
            "control":{"deadline":"caller-owned-total-operation","stages":"same-admission-deadline-and-token-required","constructionSteps":"shared-by-all-prepares-and-execution-views-of-one-admission","synchronousEngine":"external-owned-process-hard-deadline-required"}}));
        self.identity=Arc::new(SourceIdentityEvidence{common:Arc::new(common),execution:serde_json::Map::new(),environment:Some(self.host.data.clone()),environment_value:std::sync::OnceLock::new(),xml:None,xml_value:std::sync::OnceLock::new()});
        let effective_json=serde_json::to_string(&config).map_err(|_|input_error("effective Emmy config serialization"))?;
        self.graph_common=Arc::new(evidence_fields(json!({"schema":"lycheedev.native-context-snapshot-inputs.v2",
            "engine":{"baseCommit":ENGINE_COMMIT,"baseArchiveSHA256":ENGINE_ARCHIVE_SHA256},
            "effectiveEmmyrc":config,"effectiveEmmyrcJson":effective_json,"effectiveEmmyrcSHA256":hash(effective_json.as_bytes()),
            "files":self.corpus.files.iter().map(|file|json!({"file":file.path,"bytes":file.original.len(),"sha256":file.identity["sha256"]})).collect::<Vec<_>>(),
            "sourceMapProfile":self.source_map_profile(),"intrinsicProfile":PROFILE,
            "initialEnvironmentProfile":self.host.data.profile,"workspaceRoot":self.corpus.root,"stdlibInputs":[],"environmentInputs":self.host.data.materials,"configExecution":false})));
        Ok(())
    }
    fn refresh_identity(&mut self,control:&OperationControl)->lycheedev_core::Result<()> {
        control.check()?;
        let mut execution=json!({"orderedScriptLoadOccurrences":self.ordered.iter().enumerate().map(|(index,file)|json!({"index":index,"file":file})).collect::<Vec<_>>(),"executionPhase":self.phase,
            "rules":{"native":NATIVE_RULES,"capabilityProfile":NATIVE_PROFILE,"dependencyLockSHA256":hash(DEPENDENCY_LOCK)}});
        if let Some(plan)=&self.load_plan{execution["loadPlan"]=json!(plan);execution["rules"]["explicitExecution"]=json!(ENTRY_RULES);}
        if let Some(entry)=&self.entry{execution["explicitEntry"]=json!(entry);}
        if !self.host.data.bindings.is_empty(){execution["rules"]["hostRuntime"]=host_runtime_profile_for(self.entry.is_some(),self.corpus.limits.research);}
        if self.corpus.limits.flow.is_some(){execution["rules"]["flow"]=json!(FLOW_RULES);}
        self.identity=Arc::new(SourceIdentityEvidence{common:self.identity.common.clone(),execution:evidence_fields(execution),environment:self.identity.environment.clone(),environment_value:std::sync::OnceLock::new(),xml:self.xml.clone(),xml_value:std::sync::OnceLock::new()});
        let (sha256,_)=serialize_json_hash(self.identity.as_ref(),control,if self.corpus.limits.research{32<<20}else{2097152})?;
        self.identity_sha256=sha256;Ok(())
    }
    pub fn source_map_profile(&self)->&'static str {if self.corpus.files.iter().all(|file|file.prefix==0){SOURCE_MAP_PROFILE}else{"verified-per-file-identity-or-utf8-bom-prefix-v1"}}
    pub fn identity(&self)->&SourceIdentityEvidence{&self.identity}
    pub fn identity_sha256(&self)->&str{&self.identity_sha256}
    pub fn limits(&self)->&SourceLimits{&self.corpus.limits}
    pub fn file_names(&self)->impl Iterator<Item=&str>{self.corpus.files.iter().map(|file|file.path.as_str())}
    pub fn original(&self,name:&str)->lycheedev_core::Result<&str>{Ok(&self.file(name).map_err(|_|input_error("source file not admitted"))?.original)}
    pub fn analysis_text(&self,name:&str)->lycheedev_core::Result<&str>{Ok(&self.file(name).map_err(|_|input_error("source file not admitted"))?.analysis)}
    fn file(&self,name:&str)->Result<&OwnedSourceFile>{self.corpus.index.get(name).map(|id|&self.corpus.files[*id]).ok_or_else(||format!("source file not admitted: {name}"))}
    pub fn original_range(&self,name:&str,range:TextRange)->lycheedev_core::Result<TextRange>{
        let file=self.file(name).map_err(|_|input_error("source file not admitted"))?;
        let start:u32=range.start().into();let end:u32=range.end().into();
        file.analysis.get(start as usize..end as usize).ok_or_else(||input_error("invalid analysis UTF8 range"))?;
        let start=start.checked_add(file.prefix).ok_or_else(||input_error("mapped range overflow"))?;let end=end.checked_add(file.prefix).ok_or_else(||input_error("mapped range overflow"))?;
        file.original.get(start as usize..end as usize).ok_or_else(||input_error("invalid original UTF8 range"))?;
        Ok(TextRange::new(start.into(),end.into()))
    }
    pub fn analysis_range(&self,name:&str,range:TextRange)->lycheedev_core::Result<TextRange>{
        let file=self.file(name).map_err(|_|input_error("source file not admitted"))?;
        let start:u32=range.start().into();let end:u32=range.end().into();
        file.original.get(start as usize..end as usize).ok_or_else(||input_error("invalid original UTF8 range"))?;
        let start=start.checked_sub(file.prefix).ok_or_else(||input_error("position is inside removed BOM"))?;let end=end.checked_sub(file.prefix).ok_or_else(||input_error("position is inside removed BOM"))?;
        file.analysis.get(start as usize..end as usize).ok_or_else(||input_error("invalid mapped analysis UTF8 range"))?;
        Ok(TextRange::new(start.into(),end.into()))
    }
    fn site(&self,file:&str,range:TextRange)->Site {
        // Only parser-owned ranges reach this internal helper; admission checks the complete transform.
        Site::at(file,self.original_range(file,range).expect("admitted parser range"))
    }
}
