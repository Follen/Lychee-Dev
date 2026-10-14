use super::*;
use crate::{xml_inline::{self,source_map::Span},xml_load};
use std::sync::Arc;

/// Original XML/TOC bytes are the cache material. Derived Lua and maps belong to
/// the prepared snapshot and are rebuilt by the same production extractor.
pub(crate) struct XmlResearch {
    pub documents:BTreeMap<String,String>,
    pub scripts:BTreeMap<String,xml_inline::Script>,
    pub programs:Vec<Arc<xml_load::Program>>,
    pub gaps:Vec<Value>,
    owners:BTreeMap<String,Vec<(String,xml_load::XmlOrigin)>>,
    declaration_owners:BTreeMap<String,xml_load::XmlOrigin>,
}
impl XmlResearch {
    pub fn prepare(files:&mut BTreeMap<String,String>,documents:BTreeMap<String,String>,plans:&[LoadEvidence],source:&Value,control:&OperationControl)->Result<Self>{
        let mut scripts=BTreeMap::new();let mut gaps=Vec::new();let mut declaration_owners=BTreeMap::new();
        let mut objects=None;
        for(path,text)in &documents{
            control.check()?;
            if !path.to_ascii_lowercase().ends_with(".xml"){continue;}
            // Keep the exact occurrence -> script -> nearest object relation
            // from the same production parse used to generate admitted Lua.
            // Query planning must not reparse XML with a fresh local timeout.
            let parsed=crate::xml_program::extract_production(path,text.as_bytes())
                .map_err(|error|ToolkitError::new("codebase.xml_invalid",error,4));
            control.check()?;
            match parsed{
                Ok(program)=>match xml_inline::scripts(&program,text.as_bytes()){
                    Ok(extracted)=>{
                        if objects.is_none(){objects=Some(crate::xml_program::catalog().map_err(|error|ToolkitError::new("codebase.xml_invalid",error,4))?.object_elements());}
                        declaration_owners.extend(Self::declaration_owners(&program,objects.as_ref().expect("object catalog"),control)?);
                        for script in extracted{files.insert(script.key.clone(),String::from_utf8(script.source.clone()).expect("extracted UTF8"));scripts.insert(script.key.clone(),script);}
                    },
                    Err(error)=>gaps.push(json!({"code":error.code,"detail":error.message,"path":path})),
                },
                Err(error)=>gaps.push(json!({"code":error.code,"detail":error.message,"path":path})),
            }
        }
        let mut programs=Vec::new();
        let sources=files.iter().filter(|(path,_)|!scripts.contains_key(*path)).chain(documents.iter()).map(|(path,text)|(path.as_str(),text.as_bytes())).collect::<BTreeMap<_,_>>();
        let paths=xml_load::LoadPaths::new(sources.keys().copied(),control)?;
        for plan in plans {
            control.check()?;
            match xml_load::Program::from_indexed_sources(&plan.manifest,&sources,&paths,Some(source),control){
                Ok(program)=>programs.push(Arc::new(program)),
                Err(error)=>{control.check()?;gaps.push(json!({"code":error.code,"detail":error.message,"manifest":plan.manifest}));}
            }
        }
        drop(paths);drop(sources);
        let mut owners:BTreeMap<String,Vec<(String,xml_load::XmlOrigin)>>=BTreeMap::new();
        for program in &programs{
            let objects=program.actions().iter().filter_map(|action|if let xml_load::Action::Object{instance,origin,name:Some(name),..}=action{Some((instance,(name,origin)))}else{None}).collect::<BTreeMap<_,_>>();
            for action in program.actions(){if let xml_load::Action::Handler{instance,target:xml_load::HandlerTarget::Inline{file},..}=action&&let Some((name,origin))=objects.get(instance){owners.entry(file.clone()).or_default().push(((*name).clone(),(*origin).clone()));}}
        }
        Ok(Self{documents,scripts,programs,gaps,owners,declaration_owners})
    }
    /// Ui occurrence spans authenticate opening tags, not enclosing elements.
    /// The extractor's ordinal links establish ancestry for named and unnamed
    /// objects without comparing those spans to complete script elements.
    fn declaration_owners(program:&crate::xml_program::XmlProgram,objects:&std::collections::BTreeSet<String>,control:&OperationControl)->Result<BTreeMap<String,xml_load::XmlOrigin>>{
        let mut script_parents=std::collections::BTreeSet::new();
        for node in &program.occurrences{control.check()?;if node.tag=="Scripts"&&let Some(parent)=node.parent{script_parents.insert(parent);}}
        let mut nearest:Vec<Option<Span>>=Vec::with_capacity(program.occurrences.len());
        let mut owners=BTreeMap::new();
        for node in &program.occurrences{
            control.check()?;
            let parent=node.parent.map(|id|&program.occurrences[id]);
            let object=objects.contains(&node.tag)||node.kind=="object_declaration"||node.attributes.contains_key("intrinsic")
                ||parent.is_some_and(|parent|parent.tag=="Frames")||script_parents.contains(&node.ordinal);
            let owner=if object{Some(node.raw)}else{node.parent.and_then(|id|nearest[id])};
            nearest.push(owner);
            if let Some(id)=node.script&&program.scripts[id].kind=="handler"&&let Some(bytes)=owner{
                owners.insert(format!("{}/__inline_{id}.lua",program.path),xml_load::XmlOrigin{path:program.path.clone(),sha256:program.raw_sha256.clone(),bytes});
            }
        }
        Ok(owners)
    }
    pub fn mapping_manifests(&self)->Vec<Value>{self.scripts.values().map(xml_inline::Script::mapping_manifest).collect()}
    pub fn identity(&self,control:&OperationControl)->Result<Value>{self.identity_with_rules(xml_load::RULES,xml_inline::SOURCE_MAP_RULES,control)}
    fn identity_with_rules(&self,program_rules:&str,map_rules:&str,control:&OperationControl)->Result<Value>{
        let documents=hash_sequence(&json!(["xml-original-documents-v1",program_rules,map_rules]),self.documents.iter().map(|(path,text)|(path,storage::sha256(text.as_bytes()))),control)?;
        let maps=hash_sequence(&json!(["xml-source-maps-v1",program_rules,map_rules]),self.scripts.iter().map(|(key,script)|(key,script.mapping_manifest())),control)?;
        let contexts=hash_sequence(&json!(["xml-load-contexts-v1",program_rules]),self.programs.iter().map(|program|(&program.manifest,&program.context)),control)?;
        Ok(json!({"schema":"lycheedev.source-xml-input-identity.v1","programRules":program_rules,"mapRules":map_rules,
            "documentsSHA256":documents,"documentCount":self.documents.len(),"mapsSHA256":maps,"mapCount":self.scripts.len(),
            "loadContextsSHA256":contexts,"loadContextCount":self.programs.len(),
            "encoding":"canonical JSON sequence: rules header then entries in BTreeMap key order",
            "materials":{"documents":"SnapshotRequest.load_documents: original XML/TOC UTF8 bytes","maps":"nativeResearch.sourceMaps: complete generated-script mapping manifests"}}))
    }
    pub fn position(&self,prepared:&PreparedSnapshot,site:&Site)->Result<Value>{
        let span=Span::new(site.start as usize,site.end as usize);
        if let Some(script)=self.scripts.get(&site.file){let mut value=script.position(span)?;value["generatedSite"]=json!(site);return Ok(value);}
        let text=if let Some(text)=self.documents.get(&site.file){text.as_str()}else{prepared.inputs().original(&site.file)?};
        let range=xml_inline::source_map::utf16_range(text,span).map_err(|error|ToolkitError::new("source.native_source_range",error,4))?;
        Ok(json!({"path":site.file,"range":range,"coveringRange":range,"bytes":span,"positionAccuracy":"utf16-character"}))
    }
    pub fn line(&self,prepared:&PreparedSnapshot,site:&Site)->Result<(String,usize)>{
        let mapped=self.position(prepared,site)?;
        Ok((mapped["path"].as_str().expect("mapped path").into(),mapped["coveringRange"]["start"]["line"].as_u64().unwrap_or(0)as usize+1))
    }
    fn original_path<'a>(&'a self,file:&'a str)->&'a str{self.scripts.get(file).map_or(file,|script|script.path.as_str())}
    fn owns_file(&self,symbol:&index::Symbol,file:&str)->bool{
        self.owners.get(file).is_some_and(|owners|owners.iter().any(|(name,origin)|name==&symbol.name&&self.matches_origin(symbol,origin)))
    }
    /// An inherited handler can belong to an object declared in another XML file.
    /// Keep that ownership route alongside the script's original source path.
    pub fn may_contain_outgoing(&self,symbol:&index::Symbol,file:&str)->bool{
        self.original_path(file)==symbol.path||self.owns_file(symbol,file)
    }
    /// Decide membership before scanning or charging an outgoing operation.
    /// A template uses the exact nearest-object origin retained at preparation;
    /// an instance may also own handlers inherited from another XML document.
    pub(crate) fn planner_script_eligibility(&self,symbol:&index::Symbol,file:&str,mut step:impl FnMut()->Result<()>)->Result<Option<bool>> {
        if !self.scripts.contains_key(file){return Ok(None);}
        let mut known=false;let mut definition=false;let mut definition_owned=false;
        for program in &self.programs {for action in program.actions(){
            step()?;
            match action {
                xml_load::Action::Object{name:Some(name),origin,..} if name==&symbol.name&&self.matches_origin(symbol,origin)=>{known=true;},
                xml_load::Action::Definition{name,origin,..} if name==&symbol.name&&self.matches_origin(symbol,origin)=>{
                    known=true;definition=true;
                    // Exact path/hash/opening-tag identity excludes nested and
                    // sibling objects even when their names and lines coincide.
                    definition_owned|=self.declaration_owners.get(file)==Some(origin);
                },
                _=>{},
            }
        }}
        if definition{return Ok(Some(definition_owned));}
        if let Some(owners)=self.owners.get(file){for(name,origin)in owners{
            step()?;
            if name==&symbol.name&&self.matches_origin(symbol,origin){return Ok(Some(true));}
        }}
        Ok(known.then_some(false))
    }
    pub fn matches_location(&self,prepared:&PreparedSnapshot,symbol:&index::Symbol,site:&Site)->Result<bool>{
        if self.original_path(&site.file)!=symbol.path{return Ok(false);}
        let(_,line)=self.line(prepared,site)?;Ok(line==symbol.line)
    }
    pub fn matches_function(&self,prepared:&PreparedSnapshot,symbol:&index::Symbol,site:&Site)->Result<bool>{
        if self.original_path(&site.file)!=symbol.path{return Ok(false);}
        let(_,line)=self.line(prepared,site)?;
        Ok(line==symbol.line&&site.text(prepared.inputs())?==symbol.name)
    }
    pub fn within(&self,prepared:&PreparedSnapshot,symbol:&index::Symbol,site:&Site)->Result<bool>{
        if self.owns_file(symbol,&site.file){return Ok(true);}
        if self.original_path(&site.file)!=symbol.path{return Ok(false);}
        let(_,line)=self.line(prepared,site)?;
        Ok(line>=symbol.line&&line<=symbol.end_line.max(symbol.line))
    }
    pub fn matches_origin(&self,symbol:&index::Symbol,origin:&xml_load::XmlOrigin)->bool{
        origin.path==symbol.path&&self.documents.get(&origin.path).is_some_and(|text|1+text.as_bytes()[..origin.bytes.start].iter().filter(|byte|**byte==b'\n').count()==symbol.line)
    }
    pub fn owns_handler(&self,symbol:&index::Symbol,handler:&XmlHandlerFact,program:&xml_load::Program)->Result<bool>{
        Ok(program.actions().iter().any(|action|matches!(action,xml_load::Action::Object{instance,origin,name,..} if *instance==handler.instance&&name.as_deref()==Some(symbol.name.as_str())&&self.matches_origin(symbol,origin))))
    }
    pub fn project_row(&self,prepared:&PreparedSnapshot,row:&mut Value,site:&Site)->Result<()>{
        let position=self.position(prepared,site)?;
        row["path"]=position["path"].clone();row["line"]=json!(position["coveringRange"]["start"]["line"].as_u64().unwrap_or(0)+1);
        let span=if self.scripts.contains_key(&site.file){position["sourceMap"]["covering"].clone()}else{position["bytes"].clone()};
        row["site"]=json!({"file":position["path"],"start":span["start"],"end":span["end"]});
        row["position"]=position;Ok(())
    }

    // Prior eager predicates remain a test oracle for the path-only shortcut.
    #[cfg(test)]
    pub(crate) fn eager_matches_location(&self,prepared:&PreparedSnapshot,symbol:&index::Symbol,site:&Site)->Result<bool>{let(path,line)=self.line(prepared,site)?;Ok(path==symbol.path&&line==symbol.line)}
    #[cfg(test)]
    pub(crate) fn eager_matches_function(&self,prepared:&PreparedSnapshot,symbol:&index::Symbol,site:&Site)->Result<bool>{
        let(path,line)=self.line(prepared,site)?;Ok(path==symbol.path&&line==symbol.line&&site.text(prepared.inputs())?==symbol.name)
    }
    #[cfg(test)]
    pub(crate) fn eager_within(&self,prepared:&PreparedSnapshot,symbol:&index::Symbol,site:&Site)->Result<bool>{
        if self.owners.get(&site.file).is_some_and(|owners|owners.iter().any(|(name,origin)|name==&symbol.name&&self.matches_origin(symbol,origin))){return Ok(true);}
        let(path,line)=self.line(prepared,site)?;Ok(path==symbol.path&&line>=symbol.line&&line<=symbol.end_line.max(symbol.line))
    }
}

/// Stream a canonical sequence into SHA256. Only one item is materialized at a
/// time; neither the whole manifest Value nor its serialized bytes are retained.
fn hash_sequence<T:Serialize>(header:&Value,items:impl IntoIterator<Item=T>,control:&OperationControl)->Result<String>{
    use serde::ser::{SerializeSeq,Serializer};
    struct HashWriter<'a>{hash:Sha256,control:&'a OperationControl}
    impl std::io::Write for HashWriter<'_>{
        fn write(&mut self,bytes:&[u8])->std::io::Result<usize>{self.control.check().map_err(std::io::Error::other)?;self.hash.update(bytes);Ok(bytes.len())}
        fn flush(&mut self)->std::io::Result<()>{Ok(())}
    }
    let mut writer=HashWriter{hash:Sha256::new(),control};
    let result=(||->std::result::Result<(),serde_json::Error>{
        let mut serializer=serde_json::Serializer::new(&mut writer);
        let mut sequence=serializer.serialize_seq(None)?;
        sequence.serialize_element(header)?;
        for item in items{sequence.serialize_element(&item)?;}
        sequence.end()
    })();
    control.check()?;result?;Ok(format!("{:x}",writer.hash.finalize()))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn declaration_owner_walk_keeps_original_cancellation(){
        let program=crate::xml_program::extract_production("Frame.xml",b"<Ui><Frame name=\"A\" virtual=\"true\"><Scripts><OnLoad>Own()</OnLoad></Scripts></Frame></Ui>").unwrap();
        let objects=crate::xml_program::catalog().unwrap().object_elements();
        let control=OperationControl::default();
        control.cancel_handle().store(true,std::sync::atomic::Ordering::Release);
        assert_eq!(XmlResearch::declaration_owners(&program,&objects,&control).unwrap_err().code,"command.cancelled");
    }
    #[test]
    fn full_scale_xml_identity_is_compact_and_invalidates_each_material(){
        let control=OperationControl::default();
        let catalog=crate::xml_program::catalog().unwrap();
        let mut documents=BTreeMap::new();let mut scripts=BTreeMap::new();
        // Match the complete fixed Retail cardinalities with real parsed maps,
        // including entity segments, rather than replacing maps with fake IDs.
        for ordinal in 0..1091{
            let count=if ordinal<745{4}else{3};
            let path=format!("Interface/AddOns/Blizzard_Identity/Frames{ordinal}.xml");
            let raw=format!("<Ui>{}</Ui>","<Script>local value = 1 &lt; 2</Script>".repeat(count));
            let program=crate::xml_program::extract_with_deadline(&path,raw.as_bytes(),&catalog,None,&Default::default()).unwrap();
            for script in xml_inline::scripts(&program,raw.as_bytes()).unwrap(){scripts.insert(script.key.clone(),script);}
            documents.insert(path,raw);
        }
        for ordinal in 0..394{documents.insert(format!("Interface/AddOns/Blizzard_Identity/Manifest{ordinal}.toc"),format!("Frames{}.xml\n",ordinal%1091));}
        let mut xml=XmlResearch{documents,scripts,programs:vec![],gaps:vec![],owners:BTreeMap::new(),declaration_owners:BTreeMap::new()};
        assert_eq!(xml.documents.len(),1485);assert_eq!(xml.scripts.len(),4018);
        let old_documents=xml.documents.iter().map(|(path,text)|(path,storage::sha256(text.as_bytes()))).collect::<BTreeMap<_,_>>();
        assert!(serde_json::to_vec(&old_documents).unwrap().len()>65536,"fixture must reproduce the original identity cap failure");
        let identity=xml.identity(&control).unwrap();
        let source_identity=json!({"source":{"commit":"fixed-retail"},"researchRules":RESEARCH_RULES,"xml":identity});
        let compact=serialize_json(&source_identity,&control,65536).unwrap();assert!(compact.len()<2048);
        assert_eq!(identity["documentCount"],1485);assert_eq!(identity["mapCount"],4018);
        assert_eq!(xml.identity(&control).unwrap(),identity);
        let path=xml.documents.keys().find(|path|path.ends_with(".xml")).unwrap().clone();
        xml.documents.get_mut(&path).unwrap().push(' ');
        assert_ne!(xml.identity(&control).unwrap()["documentsSHA256"],identity["documentsSHA256"]);
        xml.documents.get_mut(&path).unwrap().pop();
        let raw=xml.documents.remove(&path).unwrap();let renamed=format!("{path}.renamed");xml.documents.insert(renamed.clone(),raw);
        assert_ne!(xml.identity(&control).unwrap()["documentsSHA256"],identity["documentsSHA256"]);
        let raw=xml.documents.remove(&renamed).unwrap();xml.documents.insert(path,raw);
        let key=xml.scripts.keys().next().unwrap().clone();
        let original=xml.scripts[&key].xml_sha256.clone();xml.scripts.get_mut(&key).unwrap().xml_sha256="f".repeat(64);
        assert_ne!(xml.identity(&control).unwrap()["mapsSHA256"],identity["mapsSHA256"]);
        xml.scripts.get_mut(&key).unwrap().xml_sha256=original;
        let origin=xml.scripts[&key].compiler.segments[0].origin;
        xml.scripts.get_mut(&key).unwrap().compiler.segments[0].origin=xml_inline::source_map::Origin::Entity;
        assert_ne!(xml.identity(&control).unwrap()["mapsSHA256"],identity["mapsSHA256"]);
        xml.scripts.get_mut(&key).unwrap().compiler.segments[0].origin=origin;
        let rules=xml.identity_with_rules("changed-program-rules",xml_inline::SOURCE_MAP_RULES,&control).unwrap();
        assert_ne!(rules["documentsSHA256"],identity["documentsSHA256"]);assert_ne!(rules["mapsSHA256"],identity["mapsSHA256"]);
        let rules=xml.identity_with_rules(xml_load::RULES,"changed-map-rules",&control).unwrap();
        assert_ne!(rules["documentsSHA256"],identity["documentsSHA256"]);assert_ne!(rules["mapsSHA256"],identity["mapsSHA256"]);
        assert_eq!(xml.identity(&control).unwrap(),identity);
    }
    #[test]
    fn streamed_xml_digest_matches_canonical_json_sequence(){
        let control=OperationControl::default();let header=json!(["rules","version"]);
        let entries=[("b",3), ("a",4)];
        let streamed=hash_sequence(&header,entries,&control).unwrap();
        assert_eq!(streamed,storage::sha256(&serde_json::to_vec(&json!([header,["b",3],["a",4]])).unwrap()));
    }
}
