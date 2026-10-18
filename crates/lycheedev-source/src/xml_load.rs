//! Ordered TOC/XML construction facts. Native UI types and lifecycle timing are
//! deliberately separate from the observed Lua fields and hypothetical entries.
use crate::{xml_inline::source_map::Span, xml_program::{AttributeFact, Occurrence, XmlProgram}};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

#[path = "xml_load/children.rs"]
mod children;
// Independent bounded preparation scratch; an exhausted index budget uses the
// original scan, so it cannot reject or truncate a valid XML construction.
const CHILD_INDEX_BYTES:usize=4*1024*1024;

pub const RULES: &str = "source-xml-ordered-object-program-v2-load-context";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum LoadFamily { Mainline, Classic }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all="snake_case")]
pub enum GameMode { Standard, Plunderstorm, Wowhack }
/// A manifest's hypothetical loading context, not an observation of a running
/// client's game mode. Explicit alternative manifests keep independent contexts.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all="camelCase")]
pub struct LoadContext {
    pub family:Option<LoadFamily>,
    pub game:Option<String>,
    pub mode:GameMode,
    pub phase:&'static str,
    pub assumption:&'static str,
    pub basis:Vec<String>,
    pub uncertainties:Vec<String>,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LoadDecision { Include(String), Exclude, Unavailable(String) }
impl LoadContext {
    pub(crate) fn for_manifest(manifest:&str,raw:&[u8],source:Option<&Value>)->Self {
        let headers=crate::facts::toc(raw).headers;
        Self::for_manifest_headers(manifest,headers.iter().map(|header|(header.key.as_str(),header.value.as_str())),source)
    }
    /// The authenticated index preserves these parser-observed fields, including
    /// duplicates and order. Borrowing them does not require original-byte I/O.
    pub(crate) fn for_manifest_headers<'a>(manifest:&str,headers:impl IntoIterator<Item=(&'a str,&'a str)>,source:Option<&Value>)->Self {
        let mut context=Self{family:None,game:None,mode:GameMode::Standard,phase:"game",assumption:"hypothetical_source_load_not_observed_runtime",basis:vec!["default_source_research_mode_is_standard_unless_manifest_selects_a_mode".into()],uncertainties:vec![]};
        if let Some(source)=source.filter(|source|source["repository"]=="wow-ui-source") {
            // Product/layout associations are from the pinned source baselines,
            // including Forever's Mainline layout; version-number guesses are invalid.
            context.family=match source["product"].as_str(){Some("retail"|"forever")=>Some(LoadFamily::Mainline),Some("classic"|"titan")=>Some(LoadFamily::Classic),_=>None};
            if context.family.is_some(){context.basis.push(format!("pinned_source_product:{}",source["product"].as_str().unwrap()));}
        }
        let suffix=manifest.rsplit('/').next().unwrap_or(manifest).strip_suffix(".toc").unwrap_or(manifest).rsplit('_').next().unwrap_or("").to_ascii_lowercase();
        context.select_manifest_tags(&[suffix.as_str()],"manifest_suffix");
        for (key,value) in headers {
            if key.eq_ignore_ascii_case("AllowLoadGameType") {
                let tags=condition_tags(value);
                context.select_manifest_tags(&tags,"manifest_AllowLoadGameType");
            }
        }
        if context.mode!=GameMode::Standard {
            // Special-mode [Game] directories are independent of the family.
            // The current source hypothesis does not establish their expansion.
            context.game=None;
        }else if context.family==Some(LoadFamily::Mainline){
            context.game=Some("Standard".into());
            context.basis.push("pinned_Mainline_standard_mode_uses_Standard_game_layout".into());
        }
        context
    }
    fn select_manifest_tags(&mut self,tags:&[&str],basis:&str) {
        let families=tags.iter().filter_map(|tag|match *tag{"mainline"=>Some(LoadFamily::Mainline),"classic"|"vanilla"|"tbc"|"wrath"|"cata"|"mists"=>Some(LoadFamily::Classic),_=>None}).collect::<Vec<_>>();
        // A mixed OR such as "classic, standard" does not require Classic:
        // a Mainline standard-mode scope already satisfies its second term.
        if let Some(family)=families.first().copied().filter(|family|families.len()==tags.len()&&families.iter().all(|other|other==family)) {
            if self.family.is_some_and(|previous|previous!=family)&&self.basis.iter().any(|basis|basis.starts_with("manifest_")) {
                self.uncertainties.push(format!("conflicting manifest families: {basis}:{}",tags.join(",")));return;
            }
            if self.family!=Some(family){self.game=None;}
            self.family=Some(family);self.basis.push(format!("{basis}:{}",tags.join(",")));
        }
        if tags.len()==1 {
            self.game=match tags[0]{"vanilla"=>Some("Vanilla".into()),"tbc"=>Some("TBC".into()),"wrath"=>Some("Wrath".into()),"cata"=>Some("Cata".into()),"mists"=>Some("Mists".into()),_=>self.game.take()};
            let mode=match tags[0]{"standard"=>Some(GameMode::Standard),"plunderstorm"=>Some(GameMode::Plunderstorm),"wowhack"=>Some(GameMode::Wowhack),_=>None};
            if let Some(mode)=mode{self.mode=mode;self.basis.push(format!("{basis}:{}",tags[0]));}
        }
    }
    fn tag(&self,tag:&str)->Option<bool>{
        match tag {
            "mainline"=>self.family.map(|family|family==LoadFamily::Mainline),
            "classic"=>self.family.map(|family|family==LoadFamily::Classic),
            "standard"=>Some(self.mode==GameMode::Standard),
            "plunderstorm"=>Some(self.mode==GameMode::Plunderstorm),
            "wowhack"=>Some(self.mode==GameMode::Wowhack),
            "vanilla"|"tbc"|"wrath"|"cata"|"mists"=>if self.family==Some(LoadFamily::Mainline){Some(false)}else{self.game.as_ref().map(|game|game.eq_ignore_ascii_case(tag))},
            _=>None,
        }
    }
    fn condition(&self,key:&str,value:&str)->Option<bool>{
        match key.to_ascii_lowercase().as_str() {
            "allowloadgametype"|"excludeloadgametype"=>{
                let tags=condition_tags(value);if tags.is_empty(){return None;}
                let values=tags.iter().map(|tag|self.tag(tag)).collect::<Vec<_>>();
                let matched=if values.contains(&Some(true)){Some(true)}else if values.contains(&None){None}else{Some(false)};
                if key.eq_ignore_ascii_case("ExcludeLoadGameType"){matched.map(|value|!value)}else{matched}
            },
            "allowload"=>match value.trim().to_ascii_lowercase().as_str(){"game"=>Some(true),"glue"=>Some(false),_=>None},
            _=>None,
        }
    }
    pub(crate) fn manifest_admission(&self,raw:&[u8])->LoadDecision{
        if !self.uncertainties.is_empty(){return LoadDecision::Unavailable(self.uncertainties.join("; "));}
        let headers=crate::facts::toc(raw).headers;
        self.manifest_admission_headers(headers.iter().map(|header|(header.key.as_str(),header.value.as_str())))
    }
    pub(crate) fn manifest_admission_headers<'a>(&self,headers:impl IntoIterator<Item=(&'a str,&'a str)>)->LoadDecision{
        if !self.uncertainties.is_empty(){return LoadDecision::Unavailable(self.uncertainties.join("; "));}
        let mut unknown=Vec::new();
        for (key,value) in headers {
            let lower=key.to_ascii_lowercase();
            if !lower.starts_with("allowload")&&!lower.starts_with("excludeload"){continue;}
            match self.condition(key,value){Some(true)=>{},Some(false)=>return LoadDecision::Exclude,None=>unknown.push(format!("{key}: {value}"))}
        }
        if unknown.is_empty(){LoadDecision::Include(String::new())}else{LoadDecision::Unavailable(format!("unresolved manifest conditions: {}",unknown.join("; ")))}
    }
    /// Decode only the derived load selection. Original LoadFact targets and
    /// byte evidence retain the complete unmodified TOC line.
    pub(crate) fn target(&self,raw:&str)->LoadDecision{
        let mut target=raw.trim();let mut unknown=Vec::new();let mut excluded=false;
        while target.ends_with(']') {
            let Some(start)=target.rfind('[')else{break};
            let before=&target[..start];
            if !before.ends_with(char::is_whitespace){break;}
            let condition=&target[start+1..target.len()-1];
            let (key,value)=condition.split_once(char::is_whitespace).unwrap_or((condition,""));
            match self.condition(key.trim_end_matches(':'),value){Some(true)=>{},Some(false)=>excluded=true,None=>unknown.push(condition.to_owned())}
            target=before.trim_end();
        }
        if excluded{return LoadDecision::Exclude;}
        if !unknown.is_empty(){return LoadDecision::Unavailable(format!("unresolved load conditions: {}",unknown.join("; ")));}
        let mut target=target.replace('\\',"/");
        if target.contains("[Family]") {
            let Some(family)=self.family else{return LoadDecision::Unavailable("load family is not established".into())};
            target=target.replace("[Family]",match family{LoadFamily::Mainline=>"Mainline",LoadFamily::Classic=>"Classic"});
        }
        if target.contains("[Game]") {
            let Some(game)=&self.game else{return LoadDecision::Unavailable("load game subtype is not established".into())};
            target=target.replace("[Game]",game);
        }
        if target.contains(['[',']']){return LoadDecision::Unavailable("unresolved load path token".into());}
        LoadDecision::Include(target)
    }
}
fn condition_tags(value:&str)->Vec<&str>{value.split(|character:char|character==','||character.is_whitespace()).filter(|tag|!tag.is_empty()).collect()}
/// One bounded case-fold index per admitted source set. Exact lookup remains on
/// the caller's existing map; collision members cannot bypass ambiguity checks.
pub(crate) struct LoadPaths<'a>{folded:BTreeMap<String,Option<&'a str>>,ambiguous:BTreeSet<&'a str>}
impl<'a> LoadPaths<'a>{
    pub(crate) fn new(paths:impl Iterator<Item=&'a str>,control:&OperationControl)->Result<Self>{
        let mut result=Self{folded:BTreeMap::new(),ambiguous:BTreeSet::new()};
        for(index,path)in paths.enumerate(){
            control.check()?;
            if index>=crate::native::SourceLimits::research().files{return Err(ToolkitError::new("source.native_budget","Load path index exceeds source file admission budget",3));}
            match result.folded.entry(path.to_ascii_lowercase()){
                std::collections::btree_map::Entry::Vacant(entry)=>{entry.insert(Some(path));},
                std::collections::btree_map::Entry::Occupied(mut entry)=>{
                    if let Some(previous)=*entry.get(){if previous==path{continue;}result.ambiguous.insert(previous);}
                    result.ambiguous.insert(path);entry.insert(None);
                },
            }
        }
        Ok(result)
    }
    pub(crate) fn resolve(&self,from:&str,target:&str,exact:impl FnOnce(&str)->bool)->Option<String>{
    let target=target.replace('\\',"/");
    if target.starts_with('/')||target.contains([':', '\0']){return None;}
    let mut parts=from.rsplit_once('/').map(|(parent,_)|parent.split('/').map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default();
    for part in target.split('/'){match part{""|"."=>{},".."=>{parts.pop()?;},_=>parts.push(part.into())}}
    let path=parts.join("/");
    if exact(&path){if self.ambiguous.contains(path.as_str()){None}else{Some(path)}}
    else{self.folded.get(&path.to_ascii_lowercase()).copied().flatten().map(str::to_owned)}
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct XmlOrigin { pub path:String, pub sha256:String, pub bytes:Span }
#[derive(Clone, Debug, Serialize)]
pub struct XmlReference { pub name:String, pub origin:XmlOrigin }
#[derive(Clone, Debug, Serialize)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum HandlerTarget { Inline{file:String}, Method{reference:XmlReference}, Function{reference:XmlReference}, Empty, Unavailable{reason:String} }
#[derive(Clone, Debug, Serialize)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum Action {
    Lua{file:String},
    Object{instance:String,origin:XmlOrigin,tag:String,name:Option<String>,parent:Option<String>},
    Mixin{instance:String,reference:XmlReference},
    ParentField{parent:Option<String>,parent_reference:Option<XmlReference>,child:String,reference:XmlReference},
    Handler{instance:String,handler:String,target:HandlerTarget,origin:XmlOrigin},
    Definition{name:String,intrinsic:bool,origin:XmlOrigin},
}
#[derive(Clone,Debug,Serialize)]
pub struct Program {
    pub rules:&'static str,
    pub manifest:String,
    pub context:LoadContext,
    actions:Vec<Action>,
    pub documents:BTreeMap<String,String>,
    pub gaps:Vec<Value>,
    #[serde(skip)] originals:BTreeMap<String,Arc<str>>,
}
impl Program {
    pub fn actions(&self)->&[Action]{&self.actions}
    pub fn lua_files(&self)->Vec<String>{self.actions.iter().filter_map(|action|match action{Action::Lua{file}=>Some(file.clone()),_=>None}).collect()}
    pub fn original(&self,path:&str)->Option<&str>{self.originals.get(path).map(AsRef::as_ref)}
    pub fn from_closure(closure:&crate::closure::Closure,control:&OperationControl)->Result<Self>{
        Self::from_sources(&closure.manifest,closure.documents.iter().map(|document|(document.path.as_str(),document.bytes.as_slice())),control)
    }
    pub fn from_sources<'a>(manifest:&str,sources:impl IntoIterator<Item=(&'a str,&'a [u8])>,control:&OperationControl)->Result<Self>{
        Self::from_sources_for_pin(manifest,sources,None,control)
    }
    pub(crate) fn from_sources_for_pin<'a>(manifest:&str,sources:impl IntoIterator<Item=(&'a str,&'a [u8])>,source:Option<&Value>,control:&OperationControl)->Result<Self>{
        let sources=sources.into_iter().collect::<BTreeMap<_,_>>();
        let paths=LoadPaths::new(sources.keys().copied(),control)?;
        Self::from_indexed_sources(manifest,&sources,&paths,source,control)
    }
    pub(crate) fn from_indexed_sources<'a>(manifest:&str,sources:&BTreeMap<&'a str,&'a[u8]>,paths:&LoadPaths<'a>,source:Option<&Value>,control:&OperationControl)->Result<Self>{
        let catalog=crate::xml_program::catalog().map_err(|error|ToolkitError::new("codebase.xml_invalid",error,4))?;
        Self::from_indexed_sources_with_catalog(manifest,sources,paths,source,control,&catalog)
    }
    pub(crate) fn from_indexed_sources_with_catalog<'a>(manifest:&str,sources:&BTreeMap<&'a str,&'a[u8]>,paths:&LoadPaths<'a>,source:Option<&Value>,control:&OperationControl,catalog:&crate::xml_program::Catalog)->Result<Self>{
        Self::from_indexed_sources_with_originals(manifest,sources,paths,source,control,catalog,None)
    }
    /// Immutable source text may be shared across manifests from this same preparation.
    /// Parsed programs, templates, actions and load contexts remain independent.
    pub(crate) fn from_indexed_sources_with_originals<'a>(manifest:&str,sources:&BTreeMap<&'a str,&'a[u8]>,paths:&LoadPaths<'a>,source:Option<&Value>,control:&OperationControl,catalog:&crate::xml_program::Catalog,originals:Option<&mut BTreeMap<String,Arc<str>>>)->Result<Self>{
        control.check()?;
        let context=LoadContext::for_manifest(manifest,sources.get(manifest).copied().unwrap_or_default(),source);
        let mut walker=Walker{sources,paths,catalog,shared_originals:originals,parsed:BTreeMap::new(),templates:BTreeMap::new(),intrinsics:BTreeMap::new(),active:BTreeSet::new(),
            result:Self{rules:RULES,manifest:manifest.into(),context,actions:vec![],documents:BTreeMap::new(),gaps:vec![],originals:BTreeMap::new()},control,objects:0,file_loads:0,include_depth:0,construction_steps:0,limits:crate::native::SourceLimits::research(),
            object_elements:catalog.object_elements(),child_index_remaining:CHILD_INDEX_BYTES,
            #[cfg(test)] scan_children:false};
        walker.load(manifest,0)?;
        Ok(walker.result)
    }
}
// Parsed membership is immutable and private to one manifest walk. Indexing it
// changes neither expansion order nor the per-occurrence traversal budget.
struct ParsedXml {
    program:XmlProgram, children:Option<children::ChildIndex>,
    #[cfg(test)] scan_children:bool,
}
impl std::ops::Deref for ParsedXml {
    type Target=XmlProgram;
    fn deref(&self)->&XmlProgram{&self.program}
}
impl ParsedXml {
    fn children(&self,parent:Option<usize>)->std::borrow::Cow<'_,[usize]>{
        #[cfg(test)]
        if self.scan_children {return std::borrow::Cow::Owned(self.program.occurrences.iter().filter(|node|node.parent==parent).map(|node|node.ordinal).collect());}
        match &self.children{
            Some(children)=>std::borrow::Cow::Borrowed(children.get(parent)),
            None=>std::borrow::Cow::Owned(self.program.occurrences.iter().filter(|node|node.parent==parent).map(|node|node.ordinal).collect()),
        }
    }
}
struct Walker<'a,'c,'s>{
    catalog:&'s crate::xml_program::Catalog,
    shared_originals:Option<&'s mut BTreeMap<String,Arc<str>>>,
    sources:&'s BTreeMap<&'a str,&'a [u8]>, paths:&'s LoadPaths<'a>,parsed:BTreeMap<String,Arc<ParsedXml>>,
    templates:BTreeMap<String,(String,usize)>,intrinsics:BTreeMap<String,(String,usize)>,active:BTreeSet<String>,
    result:Program,control:&'c OperationControl,objects:usize,object_elements:BTreeSet<String>,
    file_loads:usize,include_depth:usize,construction_steps:usize,limits:crate::native::SourceLimits,
    child_index_remaining:usize,
    #[cfg(test)] scan_children:bool,
}
impl Walker<'_,'_,'_>{
    fn step(&mut self)->Result<()>{
        self.control.check()?;
        if self.construction_steps>=self.limits.construction_steps{return Err(ToolkitError::new("source.native_budget","XML program construction exceeds the source construction budget",3));}
        self.construction_steps+=1;
        Ok(())
    }
    fn action(&mut self,action:Action)->Result<()>{self.step()?;self.result.actions.push(action);Ok(())}
    fn xml_step(&mut self,depth:usize)->Result<()>{
        self.step()?;
        if depth>=crate::xml_program::MAX_DEPTH{return Err(ToolkitError::new("source.native_budget","XML expansion exceeds the XML parser depth budget",3));}
        Ok(())
    }
    fn gap(&mut self,code:&str,origin:&XmlOrigin,detail:impl Serialize){self.result.gaps.push(json!({"code":code,"origin":origin,"detail":detail}));}
    fn origin(program:&XmlProgram,bytes:Span)->XmlOrigin{XmlOrigin{path:program.path.clone(),sha256:program.raw_sha256.clone(),bytes}}
    fn attr<'a>(node:&'a Occurrence,key:&str)->Option<&'a AttributeFact>{node.attributes.get(key)}
    fn truth(node:&Occurrence,key:&str)->bool{Self::attr(node,key).is_some_and(|attribute|matches!(attribute.value.as_str(),"true"|"1"))}
    fn resolve(&self,from:&str,target:&str)->Option<String>{
        self.paths.resolve(from,target,|path|self.sources.contains_key(path))
    }
    fn load_reference(&mut self,program:&XmlProgram,attribute:&AttributeFact)->Result<()>{
        match self.result.context.target(&attribute.value){
            LoadDecision::Include(target)=>if let Some(path)=self.resolve(&program.path,&target){self.load(&path,self.include_depth+1)}else{
                self.gap("xml_load_target_unavailable",&Self::origin(program,attribute.raw),&attribute.value);Ok(())
            },
            LoadDecision::Exclude=>Ok(()),
            LoadDecision::Unavailable(reason)=>{self.gap("xml_load_condition_unavailable",&Self::origin(program,attribute.raw),json!({"target":attribute.value,"reason":reason}));Ok(())}
        }
    }
    fn load(&mut self,path:&str,depth:usize)->Result<()>{
        self.step()?;
        if depth>64||self.file_loads>=20000{return Err(ToolkitError::new("source.native_budget","XML program exceeds the addon file-load count or include depth budget",3));}
        self.file_loads+=1;
        let Some(raw)=self.sources.get(path).copied()else{return Err(ToolkitError::new("source.native_invalid_input",format!("load source unavailable: {path}"),4));};
        let sha=storage::sha256(raw);
        self.result.documents.insert(path.into(),sha.clone());
        if !self.active.insert(path.into()){
            self.gap("xml_load_cycle",&XmlOrigin{path:path.into(),sha256:sha,bytes:Span::new(0,raw.len())},path);return Ok(());
        }
        let previous_depth=self.include_depth;self.include_depth=depth;
        if path.to_ascii_lowercase().ends_with(".lua"){
            self.action(Action::Lua{file:path.into()})?;
        }else if path.to_ascii_lowercase().ends_with(".toc"){
            let origin=XmlOrigin{path:path.into(),sha256:sha.clone(),bytes:Span::new(0,raw.len())};
            match self.result.context.manifest_admission(raw){
                LoadDecision::Include(_)=>for load in crate::facts::toc(raw).loads{
                    match self.result.context.target(&load.path){
                        LoadDecision::Include(target)=>if let Some(target)=self.resolve(path,&target){self.load(&target,depth+1)?;}else{self.gap("toc_load_target_unavailable",&origin,load);},
                        LoadDecision::Exclude=>{},
                        LoadDecision::Unavailable(reason)=>self.gap("toc_load_condition_unavailable",&origin,json!({"load":load,"reason":reason})),
                    }
                },
                LoadDecision::Exclude=>{},
                LoadDecision::Unavailable(reason)=>self.gap("toc_manifest_condition_unavailable",&origin,reason),
                }
        }else if path.to_ascii_lowercase().ends_with(".xml"){
            if !self.parsed.contains_key(path){
                let program=crate::xml_program::extract_production_with_catalog(path,raw,self.catalog).map_err(|error|ToolkitError::new("codebase.xml_invalid",error,4))?;
                let children=children::ChildIndex::build(&program,self.child_index_remaining,self.control)?;
                if let Some(children)=&children{self.child_index_remaining-=children.retained_bytes();}
                self.parsed.insert(path.into(),Arc::new(ParsedXml{program,children,
                    #[cfg(test)] scan_children:self.scan_children}));
                let original=if let Some(originals)=self.shared_originals.as_deref_mut(){
                    if let Some(shared)=originals.get(path){
                        if shared.as_bytes()!=raw{return Err(ToolkitError::new("source.native_invalid_input","XML original changed within one preparation",4));}
                        Arc::clone(shared)
                    }else{
                        let shared:Arc<str>=std::str::from_utf8(raw).expect("extractor admitted UTF8").into();
                        originals.insert(path.into(),Arc::clone(&shared));shared
                    }
                }else{std::str::from_utf8(raw).expect("extractor admitted UTF8").into()};
                self.result.originals.insert(path.into(),original);
            }
            let program=self.parsed[path].clone();
            for root in program.children(None).iter().copied(){self.node(&program,root,None,None,0)?;}
        }
        self.include_depth=previous_depth;self.active.remove(path);Ok(())
    }
    fn object_node(&self,program:&ParsedXml,node:&Occurrence)->bool{
        if self.object_elements.contains(&node.tag){return true;}
        if node.kind=="object_declaration"||node.attributes.contains_key("intrinsic"){return true;}
        let parent=node.parent.map(|id|&program.occurrences[id]);
        if parent.is_some_and(|parent|parent.tag=="Frames"){return true;}
        program.children(Some(node.ordinal)).iter().any(|id|program.occurrences[*id].tag=="Scripts")
    }
    fn node(&mut self,program:&Arc<ParsedXml>,id:usize,owner:Option<&str>,parent_name:Option<&str>,depth:usize)->Result<()>{
        self.xml_step(depth)?;let node=&program.occurrences[id];
        match node.kind.as_str(){
            "include"|"script_file"=>{
                self.load_reference(program,node.attributes.get("file").expect("extractor file attribute"))?;
                if node.script.is_some(){self.gap("xml_file_and_inline_body",&Self::origin(program,node.raw),"both sources were declared; inline runtime selection is unknown");}
                return Ok(());
            },
            "inline_script"|"binding_declaration"=>{
                if let Some(script)=node.script{self.action(Action::Lua{file:format!("{}/__inline_{script}.lua",program.path)})?;}
                return Ok(());
            },
            "handler_registration"=>{
                if let Some(instance)=owner{self.handler(program,node,instance)?;}else{self.gap("xml_handler_owner_unavailable",&Self::origin(program,node.raw),&node.tag);}
                return Ok(());
            },
            "handler_method_reference"|"handler_function_reference"=>return Ok(()),
            "mixin_reference"=>{
                if let Some(instance)=owner{if let Some(attribute)=node.attributes.get("name"){self.mixin(program,instance,attribute)?;}else{self.gap("xml_mixin_name_unavailable",&Self::origin(program,node.raw),&node.attributes);}}
                return Ok(());
            },
            _=>{},
        }
        if self.object_node(program,node)||self.intrinsics.contains_key(&node.tag){
            let intrinsic=Self::truth(node,"intrinsic");
            if Self::truth(node,"virtual")||intrinsic{
                if let Some(name)=Self::attr(node,"name"){
                    self.templates.insert(name.value.clone(),(program.path.clone(),id));
                    if intrinsic{self.intrinsics.insert(name.value.clone(),(program.path.clone(),id));}
                    self.action(Action::Definition{name:name.value.clone(),intrinsic,origin:Self::origin(program,node.raw)})?;
                }else{self.gap("xml_definition_name_unavailable",&Self::origin(program,node.raw),&node.tag);}
                return Ok(());
            }
            let name=Self::attr(node,"name").and_then(|name|{
                if name.value.contains("$parent"){parent_name.map(|parent|name.value.replace("$parent",parent))}else{Some(name.value.clone())}
            });
            if Self::attr(node,"name").is_some_and(|name|name.value.contains("$parent"))&&name.is_none(){self.gap("xml_parent_name_unavailable",&Self::origin(program,node.raw),"$parent has no established parent name");}
            if self.objects>=self.limits.object_limit(){return Err(ToolkitError::new("source.native_budget","XML object count exceeds the source object identity budget",3));}
            let instance=format!("xml-instance:{}",self.objects);self.objects+=1;
            self.action(Action::Object{instance:instance.clone(),origin:Self::origin(program,node.raw),tag:node.tag.clone(),name:name.clone(),parent:owner.map(str::to_owned)})?;
            if let Some(attribute)=Self::attr(node,"parentKey"){
                self.action(Action::ParentField{parent:owner.map(str::to_owned),parent_reference:Self::attr(node,"parent").map(|parent|XmlReference{name:parent.value.clone(),origin:Self::origin(program,parent.raw)}),child:instance.clone(),reference:XmlReference{name:attribute.value.clone(),origin:Self::origin(program,attribute.raw)}})?;
            }
            let mut active=BTreeSet::new();
            if let Some((path,template))=self.intrinsics.get(&node.tag).cloned(){let source=self.parsed[&path].clone();self.apply(&source,template,&instance,name.as_deref(),depth,&mut active)?;}
            self.apply(program,id,&instance,name.as_deref(),depth,&mut active)?;
            return Ok(());
        }
        for child in program.children(Some(id)).iter().copied(){self.node(program,child,owner,parent_name,depth+1)?;}
        Ok(())
    }
    fn apply(&mut self,program:&Arc<ParsedXml>,id:usize,instance:&str,name:Option<&str>,depth:usize,active:&mut BTreeSet<(String,usize)>)->Result<()>{
        self.xml_step(depth)?;let node=&program.occurrences[id];let key=(program.path.clone(),id);
        if !active.insert(key.clone()){self.gap("xml_template_cycle",&Self::origin(program,node.raw),&node.tag);return Ok(());}
        if let Some(attribute)=Self::attr(node,"inherits"){
            for template in names(&attribute.value){
                if let Some((path,index))=self.templates.get(template).cloned(){let source=self.parsed[&path].clone();self.apply(&source,index,instance,name,depth+1,active)?;}
                else{self.gap("xml_template_unavailable",&Self::origin(program,attribute.raw),template);}
            }
        }
        if let Some(attribute)=Self::attr(node,"mixin"){self.mixin(program,instance,attribute)?;}
        for key in ["nativeMixin","parentArray"]{if let Some(attribute)=Self::attr(node,key){self.gap("xml_native_or_parent_binding_unavailable",&Self::origin(program,attribute.raw),json!({"attribute":key,"value":attribute.value}));}}
        for child in program.children(Some(id)).iter().copied(){self.node(program,child,Some(instance),name,depth+1)?;}
        active.remove(&key);Ok(())
    }
    fn mixin(&mut self,program:&XmlProgram,instance:&str,attribute:&AttributeFact)->Result<()>{
        for name in names(&attribute.value){self.action(Action::Mixin{instance:instance.into(),reference:XmlReference{name:name.into(),origin:Self::origin(program,attribute.raw)}})?;}
        Ok(())
    }
    fn handler(&mut self,program:&XmlProgram,node:&Occurrence,instance:&str)->Result<()>{
        if node.handler_catalog.as_ref().and_then(|catalog|catalog["handlerDeclarations"].as_array()).is_some_and(Vec::is_empty){self.gap("xml_handler_catalog_unavailable",&Self::origin(program,node.raw),&node.tag);}
        let method=Self::attr(node,"method");let function=Self::attr(node,"function");
        let body=node.script.filter(|id|!program.scripts[*id].decoded.text.trim().is_empty());
        let target=match (method,function,body){
            (Some(attribute),None,None)=>HandlerTarget::Method{reference:XmlReference{name:attribute.value.clone(),origin:Self::origin(program,attribute.raw)}},
            (None,Some(attribute),None)=>HandlerTarget::Function{reference:XmlReference{name:attribute.value.clone(),origin:Self::origin(program,attribute.raw)}},
            (None,None,Some(script))=>{let file=format!("{}/__inline_{script}.lua",program.path);self.action(Action::Lua{file:file.clone()})?;HandlerTarget::Inline{file}},
            (None,None,None)=>HandlerTarget::Empty,
            _=>{self.gap("xml_handler_binding_conflict",&Self::origin(program,node.raw),"multiple handler sources declared");HandlerTarget::Unavailable{reason:"multiple handler sources declared".into()}},
        };
        self.action(Action::Handler{instance:instance.into(),handler:node.tag.clone(),target,origin:Self::origin(program,node.raw)})?;Ok(())
    }
}
fn names(value:&str)->impl Iterator<Item=&str>{value.split(',').map(str::trim).filter(|name|!name.is_empty())}

#[cfg(test)]
mod tests{
    use super::*;
    fn walk_with_child_mode(sources:&BTreeMap<&str,&[u8]>,manifest:&str,scan_children:bool,construction_steps:usize)->(Result<Program>,(usize,usize,usize,usize)){
        walk_with_child_budget(sources,manifest,scan_children,construction_steps,CHILD_INDEX_BYTES)
    }
    fn walk_with_child_budget(sources:&BTreeMap<&str,&[u8]>,manifest:&str,scan_children:bool,construction_steps:usize,index_budget:usize)->(Result<Program>,(usize,usize,usize,usize)){
        let control=OperationControl::default();
        let catalog=crate::xml_program::catalog().unwrap();
        let paths=LoadPaths::new(sources.keys().copied(),&control).unwrap();
        let source=json!({"repository":"wow-ui-source","product":"retail"});
        let context=LoadContext::for_manifest(manifest,sources.get(manifest).copied().unwrap_or_default(),Some(&source));
        let mut limits=crate::native::SourceLimits::research();limits.construction_steps=construction_steps;
        let mut walker=Walker{sources,paths:&paths,catalog:&catalog,shared_originals:None,parsed:BTreeMap::new(),templates:BTreeMap::new(),intrinsics:BTreeMap::new(),active:BTreeSet::new(),
            result:Program{rules:RULES,manifest:manifest.into(),context,actions:vec![],documents:BTreeMap::new(),gaps:vec![],originals:BTreeMap::new()},
            control:&control,objects:0,file_loads:0,include_depth:0,construction_steps:0,limits,object_elements:catalog.object_elements(),child_index_remaining:index_budget,scan_children};
        let loaded=walker.load(manifest,0);
        let counters=(walker.construction_steps,walker.file_loads,walker.objects,walker.include_depth);
        (loaded.map(|()|walker.result),counters)
    }
    #[test]
    fn manifest_originals_share_script_storage_without_changing_program_or_mapping(){
        let control=OperationControl::default();
        let raw="<Ui><Frame name=\"Frame界\"><Scripts><OnLoad>local value = 1 &lt; 2</OnLoad></Scripts></Frame></Ui>";
        let sources=BTreeMap::from([("Main.toc",b"Frame.xml\n".as_slice()),("Other.toc",b"Frame.xml\n".as_slice()),("Frame.xml",raw.as_bytes())]);
        let paths=LoadPaths::new(sources.keys().copied(),&control).unwrap();
        let catalog=crate::xml_program::catalog().unwrap();
        let source=json!({"repository":"wow-ui-source","product":"retail"});
        let script=crate::xml_inline::extract("Frame.xml",raw.as_bytes()).unwrap().remove(0);
        let original=script.original_shared();let map_before=script.mapping_manifest();
        let mut shared=BTreeMap::from([("Frame.xml".into(),Arc::clone(&original))]);
        for manifest in ["Main.toc","Other.toc"]{
            let baseline=Program::from_indexed_sources_with_catalog(manifest,&sources,&paths,Some(&source),&control,&catalog).unwrap();
            let program=Program::from_indexed_sources_with_originals(manifest,&sources,&paths,Some(&source),&control,&catalog,Some(&mut shared)).unwrap();
            assert!(Arc::ptr_eq(&program.originals["Frame.xml"],&original));
            assert_eq!(program.original("Frame.xml"),Some(raw));
            assert_eq!(serde_json::to_value(&program).unwrap(),serde_json::to_value(&baseline).unwrap());
            assert_eq!(script.mapping_manifest(),map_before);
        }
        let mut lazy=BTreeMap::new();
        let first=Program::from_indexed_sources_with_originals("Main.toc",&sources,&paths,Some(&source),&control,&catalog,Some(&mut lazy)).unwrap();
        let second=Program::from_indexed_sources_with_originals("Other.toc",&sources,&paths,Some(&source),&control,&catalog,Some(&mut lazy)).unwrap();
        assert_eq!(lazy.len(),1);
        assert!(Arc::ptr_eq(&first.originals["Frame.xml"],&second.originals["Frame.xml"]));
        let mut mismatch=BTreeMap::from([("Frame.xml".into(),Arc::<str>::from("changed"))]);
        assert_eq!(Program::from_indexed_sources_with_originals("Main.toc",&sources,&paths,Some(&source),&control,&catalog,Some(&mut mismatch)).unwrap_err().code,"source.native_invalid_input");
        control.cancel_handle().store(true,std::sync::atomic::Ordering::Release);
        assert_eq!(Program::from_indexed_sources_with_originals("Main.toc",&sources,&paths,Some(&source),&control,&catalog,Some(&mut shared)).unwrap_err().code,"command.cancelled");
    }
    fn assert_child_walk_equivalent(sources:&BTreeMap<&str,&[u8]>,manifest:&str,steps:usize){
        let (indexed,indexed_counts)=walk_with_child_mode(sources,manifest,false,steps);
        let (scanned,scanned_counts)=walk_with_child_mode(sources,manifest,true,steps);
        assert_eq!(indexed_counts,scanned_counts,"all original traversal charges must remain");
        match(indexed,scanned){
            (Ok(indexed),Ok(scanned))=>{
                assert_eq!(serde_json::to_vec(&indexed).unwrap(),serde_json::to_vec(&scanned).unwrap());
                assert_eq!(indexed.originals,scanned.originals,"unserialized original bytes also remain identical");
            },
            (Err(indexed),Err(scanned))=>{assert_eq!(indexed.code,scanned.code);assert_eq!(indexed.message,scanned.message);},
            _=>panic!("indexed traversal changed success or failure"),
        }
    }
    #[test]
    fn indexed_walk_matches_scan_for_templates_occurrences_and_alternative_manifests(){
        let sources=BTreeMap::from([
            ("Main.toc",b"templates.xml\nroot.xml\nroot.xml\nignored.lua [AllowLoadGameType classic]\n".as_slice()),
            ("Other.toc",b"templates.xml\nroot.xml\n".as_slice()),
            ("templates.xml",b"<Ui><Frame name=\"Base\" virtual=\"true\" mixin=\"M,N\"><Scripts><OnLoad>Target(self)</OnLoad><OnShow method=\"Run\"/></Scripts></Frame><Frame name=\"Special\" intrinsic=\"true\" inherits=\"Base\"/><Frame name=\"Cycle\" virtual=\"true\" inherits=\"Cycle\"/></Ui>".as_slice()),
            ("root.xml",b"<Ui><Include file=\"cycle.xml\"/><Script file=\"defs.lua\"/><Frame name=\"A\" inherits=\"Base\"><Frames><Special name=\"$parentChild\" parentKey=\"Child\"/></Frames><Scripts><OnFuture function=\"Future\"/><OnHide method=\"Run\">Target(self)</OnHide></Scripts></Frame><Frame inherits=\"Missing,Cycle\"/><Unknown><Scripts><OnEvent method=\"Event\"/></Scripts></Unknown></Ui>".as_slice()),
            ("cycle.xml",b"<Ui><Include file=\"root.xml\"/></Ui>".as_slice()),
            ("defs.lua",b"M={}\nN={}\n".as_slice()),
            ("ignored.lua",b"Ignored()\n".as_slice()),
        ]);
        for manifest in ["Main.toc","Other.toc"]{assert_child_walk_equivalent(&sources,manifest,50000000);}
        let (program,_)=walk_with_child_mode(&sources,"Main.toc",false,50000000);
        let program=program.unwrap();
        assert_eq!(program.actions().iter().filter(|action|matches!(action,Action::Object{name:Some(name),..}if name=="A")).count(),2);
        assert!(!program.lua_files().contains(&"ignored.lua".into()));
        for code in ["xml_load_cycle","xml_template_cycle","xml_template_unavailable","xml_handler_binding_conflict","xml_handler_catalog_unavailable"]{
            assert!(program.gaps.iter().any(|gap|gap["code"]==code),"{code}");
        }
    }
    #[test]
    fn indexed_walk_preserves_every_construction_budget_boundary(){
        let sources=BTreeMap::from([
            ("Main.toc",b"root.xml\nroot.xml\n".as_slice()),
            ("root.xml",b"<Ui><Frame name=\"T\" virtual=\"true\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame><Frame inherits=\"T\" mixin=\"M,N\"><Frames><Frame parentKey=\"Child\"/></Frames></Frame></Ui>".as_slice()),
        ]);
        let (_,counts)=walk_with_child_mode(&sources,"Main.toc",true,50000000);
        for steps in 0..=counts.0+1{assert_child_walk_equivalent(&sources,"Main.toc",steps);}
    }
    #[test]
    fn exhausted_child_index_scratch_preserves_complete_walk_and_charges(){
        let sources=BTreeMap::from([
            ("Main.toc",b"one.xml\ntwo.xml\none.xml\n".as_slice()),
            ("one.xml",b"<Ui><Frame name=\"Template\" virtual=\"true\"><Scripts><OnLoad method=\"Run\"/></Scripts></Frame><Frame inherits=\"Template\"/></Ui>".as_slice()),
            ("two.xml",b"<Ui><Frame inherits=\"Template\"><Frames><Frame parentKey=\"Child\"/></Frames></Frame></Ui>".as_slice()),
        ]);
        let (expected,counts)=walk_with_child_mode(&sources,"Main.toc",true,50000000);
        let expected=expected.unwrap();
        for budget in [0,1,64,128,256,512,CHILD_INDEX_BYTES]{
            let (actual,actual_counts)=walk_with_child_budget(&sources,"Main.toc",false,50000000,budget);
            let actual=actual.unwrap();
            assert_eq!(serde_json::to_vec(&actual).unwrap(),serde_json::to_vec(&expected).unwrap());
            assert_eq!(actual.originals,expected.originals);
            assert_eq!(actual_counts,counts,"index budget {budget}");
        }
    }
    #[test]
    fn indexed_walk_preserves_malformed_document_failure(){
        let sources=BTreeMap::from([
            ("Main.toc",b"good.xml\nbad.xml\n".as_slice()),
            ("good.xml",b"<Ui><Frame/></Ui>".as_slice()),
            ("bad.xml",b"<Ui><Frame></Ui>".as_slice()),
        ]);
        assert_child_walk_equivalent(&sources,"Main.toc",50000000);
        let (result,_)=walk_with_child_mode(&sources,"Main.toc",false,50000000);
        assert_eq!(result.unwrap_err().code,"codebase.xml_invalid");
    }
    #[test]
    fn repeated_xml_loads_create_instances_and_keep_empty_references(){
        let sources=[("Main.toc",b"root.xml\nroot.xml\n".as_slice()),("root.xml",b"<Ui><Script file=\"defs.lua\"/><Frame><Scripts><OnLoad method=\"Run\"/><OnShow function=\"Callback\"/><OnHide/></Scripts></Frame></Ui>".as_slice()),("defs.lua",b"M={}\n".as_slice())];
        let program=Program::from_sources("Main.toc",sources,&OperationControl::default()).unwrap();
        assert_eq!(program.lua_files(),vec!["defs.lua","defs.lua"]);
        let objects=program.actions().iter().filter_map(|action|if let Action::Object{instance,..}=action{Some(instance)}else{None}).collect::<Vec<_>>();
        assert_eq!(objects.len(),2);assert_ne!(objects[0],objects[1]);
        assert_eq!(program.actions().iter().filter(|action|matches!(action,Action::Handler{target:HandlerTarget::Method{..},..})).count(),2);
        assert_eq!(program.actions().iter().filter(|action|matches!(action,Action::Handler{target:HandlerTarget::Function{..},..})).count(),2);
        assert_eq!(program.actions().iter().filter(|action|matches!(action,Action::Handler{target:HandlerTarget::Empty,..})).count(),2);
    }
    #[test]
    fn unknown_handler_is_preserved_with_catalog_gap(){
        let program=Program::from_sources("Main.toc",[("Main.toc",b"root.xml\n".as_slice()),("root.xml",b"<Ui><Frame><Scripts><OnFuture method=\"Run\"/></Scripts></Frame></Ui>".as_slice())],&OperationControl::default()).unwrap();
        assert!(program.actions().iter().any(|action|matches!(action,Action::Handler{handler,target:HandlerTarget::Method{..},..}if handler=="OnFuture")));
        assert!(program.gaps.iter().any(|gap|gap["code"]=="xml_handler_catalog_unavailable"));
    }
    #[test]
    fn construction_actions_do_not_consume_file_load_budget(){
        let mixins=vec!["M";100].join(",");
        let xml=format!("<Ui>{}</Ui>",format!("<Frame mixin=\"{mixins}\"/>").repeat(201));
        let program=Program::from_sources("Main.toc",[("Main.toc",b"root.xml\n".as_slice()),("root.xml",xml.as_bytes())],&OperationControl::default()).unwrap();
        assert!(program.actions().len()>20000);
        assert_eq!(program.actions().iter().filter(|action|matches!(action,Action::Object{..})).count(),201);
    }
    #[test]
    fn xml_nesting_does_not_consume_include_depth(){
        let xml=format!("<Ui>{}<Script file=\"defs.lua\"/>{}</Ui>","<Layer>".repeat(70),"</Layer>".repeat(70));
        let program=Program::from_sources("Main.toc",[("Main.toc",b"root.xml\n".as_slice()),("root.xml",xml.as_bytes()),("defs.lua",b"M={}\n".as_slice())],&OperationControl::default()).unwrap();
        assert_eq!(program.lua_files(),vec!["defs.lua"]);
    }
}
