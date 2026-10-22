use super::*;
use crate::{xml_inline::{self,source_map::Span},xml_load};
use std::sync::Arc;

/// A document without a retained parsed owner keeps its original allocation.
/// Parsed XML reuses the exact immutable original already held by scripts or
/// load programs; converting unrelated TOC/invalid XML into Arcs would add a
/// new copy and allocation overlap without eliminating any retained text.
pub(crate) enum ResearchDocument {
    Owned(String),
    Shared(Arc<str>),
}
impl ResearchDocument {
    fn as_str(&self)->&str{match self{Self::Owned(text)=>text.as_str(),Self::Shared(text)=>text.as_ref()}}
    fn as_bytes(&self)->&[u8]{self.as_str().as_bytes()}
}
fn retain_documents(documents:BTreeMap<String,String>,mut originals:BTreeMap<String,Arc<str>>)->BTreeMap<String,ResearchDocument>{
    documents.into_iter().map(|(path,text)|{
        // The pool was populated only by the same production extraction/load
        // above. Exact equality also makes a future unrelated pool entry a
        // no-sharing fallback, never a new trust source or public error.
        // A failed load can leave only the pool's Arc; that has no surviving
        // duplicate to share, so preserve the moved String in that case too.
        let retained=match originals.remove(&path){
            Some(shared) if Arc::strong_count(&shared)>1&&shared.as_ref()==text.as_str()=>ResearchDocument::Shared(shared),
            _=>ResearchDocument::Owned(text),
        };
        (path,retained)
    }).collect()
}

/// Original XML/TOC bytes are the cache material. Derived Lua and maps belong to
/// the prepared snapshot and are rebuilt by the same production extractor.
pub(crate) struct XmlResearch {
    pub documents:BTreeMap<String,ResearchDocument>,
    pub scripts:BTreeMap<String,xml_inline::Script>,
    pub programs:Vec<Arc<xml_load::Program>>,
    pub gaps:Vec<Value>,
    owners:BTreeMap<String,Vec<(String,xml_load::XmlOrigin)>>,
    declaration_owners:BTreeMap<String,xml_load::XmlOrigin>,
}
impl XmlResearch {
    pub fn prepare(files:&mut BTreeMap<String,String>,documents:BTreeMap<String,String>,plans:&[LoadEvidence],source:&Value,control:&OperationControl)->Result<Self>{
        let mut scripts=BTreeMap::new();let mut gaps=Vec::new();let mut declaration_owners=BTreeMap::new();
        let mut originals=BTreeMap::new();
        // Local to this preparation: the immutable built-in catalog is verified
        // lazily once. Retain a failed verification too, preserving each original
        // document/manifest gap without reparsing or silently accepting it.
        let mut catalog=None;
        let mut objects=None;
        for(path,text)in &documents{
            control.check()?;
            if !path.to_ascii_lowercase().ends_with(".xml"){continue;}
            // Keep the exact occurrence -> script -> nearest object relation
            // from the same production parse used to generate admitted Lua.
            // Query planning must not reparse XML with a fresh local timeout.
            let parsed=catalog.get_or_insert_with(crate::xml_program::catalog).as_ref().map_err(Clone::clone)
                .and_then(|catalog|crate::xml_program::extract_production_with_catalog(path,text.as_bytes(),catalog))
                .map_err(|error|ToolkitError::new("codebase.xml_invalid",error,4));
            control.check()?;
            match parsed{
                Ok(program)=>match xml_inline::scripts(&program,text.as_bytes()){
                    Ok(extracted)=>{
                        if objects.is_none(){objects=Some(catalog.as_ref().and_then(|catalog|catalog.as_ref().ok()).expect("successful production catalog").object_elements());}
                        declaration_owners.extend(Self::declaration_owners(&program,objects.as_ref().expect("object catalog"),control)?);
                        for script in extracted{originals.entry(script.path.clone()).or_insert_with(||script.original_shared());files.insert(script.key.clone(),script.compiler.text.clone());scripts.insert(script.key.clone(),script);}
                    },
                    Err(error)=>gaps.push(json!({"code":error.code,"detail":error.message,"path":path})),
                },
                Err(error)=>gaps.push(json!({"code":error.code,"detail":error.message,"path":path})),
            }
        }
        // Script maps seed this request-local pool. Other XML originals enter
        // lazily when a manifest loads them, avoiding new retention for unused files.
        let mut programs=Vec::new();
        let sources=files.iter().filter(|(path,_)|!scripts.contains_key(*path)).chain(documents.iter()).map(|(path,text)|(path.as_str(),text.as_bytes())).collect::<BTreeMap<_,_>>();
        let paths=xml_load::LoadPaths::new(sources.keys().copied(),control)?;
        for plan in plans {
            control.check()?;
            let program=catalog.get_or_insert_with(crate::xml_program::catalog).as_ref().map_err(|error|ToolkitError::new("codebase.xml_invalid",error.clone(),4))
                .and_then(|catalog|xml_load::Program::from_indexed_sources_with_originals(&plan.manifest,&sources,&paths,Some(source),control,catalog,Some(&mut originals)));
            match program{
                Ok(program)=>programs.push(Arc::new(program)),
                Err(error)=>{control.check()?;gaps.push(json!({"code":error.code,"detail":error.message,"manifest":plan.manifest}));}
            }
        }
        drop(paths);drop(sources);
        let documents=retain_documents(documents,originals);
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
    #[cfg(test)]
    pub fn mapping_manifests(&self)->Vec<Value>{self.scripts.values().map(xml_inline::Script::mapping_manifest).collect()}
    pub fn identity(&self,control:&OperationControl)->Result<Value>{self.identity_with_rules(xml_load::RULES,xml_inline::SOURCE_MAP_RULES,control)}
    fn identity_with_rules(&self,program_rules:&str,map_rules:&str,control:&OperationControl)->Result<Value>{
        let documents=hash_sequence(&json!(["xml-original-documents-v1",program_rules,map_rules]),self.documents.iter().map(|(path,text)|(path,storage::sha256(text.as_bytes()))),control)?;
        let maps=hash_sequence(&json!(["xml-source-maps-v1",program_rules,map_rules]),self.scripts.iter().map(|(key,script)|(key,script.mapping_manifest_view())),control)?;
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
    fn request_catalog_is_verified_once_without_changing_production_xml_evidence(){
        let catalog=crate::xml_program::catalog().unwrap();
        for (path,text) in [
            ("Frames.xml","\u{feff}<Ui>\r\n<Frame name=\"A\" virtual=\"true\"><Scripts><OnLoad>local s=&quot;😀&quot;&#10;Target(self)</OnLoad><OnFuture method=\"Run\"/></Scripts></Frame></Ui>"),
            ("Bindings.xml","<Bindings><Binding name=\"CLICK_界\" header=\"H\">Target(keystate)</Binding></Bindings>"),
            ("Bad.xml","<Ui><Frame></Ui>"),
        ] {
            let expected=crate::xml_program::extract_production(path,text.as_bytes());
            let before=crate::xml_program::catalog_verifications();
            let actual=crate::xml_program::extract_production_with_catalog(path,text.as_bytes(),&catalog);
            assert_eq!(crate::xml_program::catalog_verifications(),before);
            match (expected,actual) {
                (Ok(expected),Ok(actual))=>assert_eq!(serde_json::to_vec(&expected).unwrap(),serde_json::to_vec(&actual).unwrap()),
                (Err(expected),Err(actual))=>assert_eq!(expected,actual),
                _=>panic!("production extraction result changed"),
            }
        }
    }
    #[test]
    fn request_catalog_is_shared_across_all_documents_and_manifest_alternatives(){
        let control=OperationControl::default();
        let documents=BTreeMap::from([
            ("One.toc".into(),"Frames.xml\nFrames.xml\n".into()),
            ("Two.toc".into(),"Frames.xml\nObjects.xml\n".into()),
            ("Frames.xml".into(),"<Ui><Frame name=\"Template\" virtual=\"true\"><Scripts><OnLoad>Target(self)</OnLoad><OnFuture method=\"Run\"/></Scripts></Frame></Ui>".into()),
            ("Objects.xml".into(),"<Ui><Frame name=\"Object\" inherits=\"Template\"/></Ui>".into()),
            ("Broken.xml".into(),"<Ui><Frame></Ui>".into()),
        ]);
        let plans=["One.toc","Two.toc"].map(|manifest|LoadEvidence{manifest:manifest.into(),ordered:vec![],issues:vec![]});
        let source=json!({"repository":"catalog-test","product":"retail"});
        let before=crate::xml_program::catalog_verifications();
        let mut files=BTreeMap::new();
        let xml=XmlResearch::prepare(&mut files,documents.clone(),&plans,&source,&control).unwrap();
        assert_eq!(crate::xml_program::catalog_verifications()-before,1,"one request verifies the complete immutable catalog once");
        assert_eq!(xml.programs.len(),2);assert!(!xml.scripts.is_empty());
        assert!(xml.gaps.iter().any(|gap|gap["path"]=="Broken.xml"&&gap["code"]=="codebase.xml_invalid"));
        let sources=documents.iter().map(|(path,text)|(path.as_str(),text.as_bytes())).collect::<BTreeMap<_,_>>();
        let paths=xml_load::LoadPaths::new(sources.keys().copied(),&control).unwrap();
        for (plan,actual) in plans.iter().zip(&xml.programs) {
            let expected=xml_load::Program::from_indexed_sources(&plan.manifest,&sources,&paths,Some(&source),&control).unwrap();
            assert_eq!(serde_json::to_vec(&expected).unwrap(),serde_json::to_vec(actual.as_ref()).unwrap());
        }
        let before=crate::xml_program::catalog_verifications();
        XmlResearch::prepare(&mut BTreeMap::new(),documents,&plans,&source,&control).unwrap();
        assert_eq!(crate::xml_program::catalog_verifications()-before,1,"a later request verifies its own catalog");
    }
    #[test]
    fn request_catalog_remains_lazy_and_preserves_cancellation(){
        let control=OperationControl::default();
        let before=crate::xml_program::catalog_verifications();
        XmlResearch::prepare(&mut BTreeMap::new(),BTreeMap::new(),&[],&json!({}),&control).unwrap();
        assert_eq!(crate::xml_program::catalog_verifications(),before);
        control.tracked_cancel_handle().unwrap().cancel();
        let error=XmlResearch::prepare(&mut BTreeMap::new(),BTreeMap::from([("x.xml".into(),"<Ui/>".into())]),&[],&json!({}),&control).err().unwrap();
        assert_eq!(error.code,"command.cancelled");
        assert_eq!(crate::xml_program::catalog_verifications(),before);
    }
    #[test]
    fn declaration_owner_walk_keeps_original_cancellation(){
        let program=crate::xml_program::extract_production("Frame.xml",b"<Ui><Frame name=\"A\" virtual=\"true\"><Scripts><OnLoad>Own()</OnLoad></Scripts></Frame></Ui>").unwrap();
        let objects=crate::xml_program::catalog().unwrap().object_elements();
        let control=OperationControl::default();
        control.tracked_cancel_handle().unwrap().cancel();
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
        let documents=retain_documents(documents,BTreeMap::new());
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
        let ResearchDocument::Owned(text)=xml.documents.get_mut(&path).unwrap()else{panic!("fixture has no shared pool")};text.push(' ');
        assert_ne!(xml.identity(&control).unwrap()["documentsSHA256"],identity["documentsSHA256"]);
        let ResearchDocument::Owned(text)=xml.documents.get_mut(&path).unwrap()else{panic!("fixture has no shared pool")};text.pop();
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
    fn streamed_mapping_identity_matches_original_manifest_sequence(){
        let control=OperationControl::default();
        let documents=BTreeMap::from([
            ("z.xml".into(),"<Ui><Script>local z=1</Script></Ui>".into()),
            ("a.xml".into(),"\u{feff}<Ui>\r\n<Frame><Scripts><OnLoad>local text=&quot;界&quot;\r\n</OnLoad></Scripts></Frame></Ui>".into()),
        ]);
        let xml=XmlResearch::prepare(&mut BTreeMap::new(),documents,&[],&json!({}),&control).unwrap();
        let header=json!(["xml-source-maps-v1",xml_load::RULES,xml_inline::SOURCE_MAP_RULES]);
        let old=hash_sequence(&header,xml.scripts.iter().map(|(key,script)|(key,script.mapping_manifest())),&control).unwrap();
        assert_eq!(xml.identity(&control).unwrap()["mapsSHA256"],old);
        let old=serde_json::to_vec(&xml.mapping_manifests()).unwrap();
        assert_eq!(serde_json::to_vec(&ResearchSourceMaps(&xml)).unwrap(),old);
        let empty=XmlResearch::prepare(&mut BTreeMap::new(),BTreeMap::new(),&[],&json!({}),&control).unwrap();
        let old=hash_sequence::<Value>(&header,[],&control).unwrap();
        assert_eq!(empty.identity(&control).unwrap()["mapsSHA256"],old);
        assert_eq!(serde_json::to_vec(&ResearchSourceMaps(&empty)).unwrap(),b"[]");
    }
    #[test]
    fn streamed_xml_digest_matches_canonical_json_sequence(){
        let control=OperationControl::default();let header=json!(["rules","version"]);
        let entries=[("b",3), ("a",4)];
        let streamed=hash_sequence(&header,entries,&control).unwrap();
        assert_eq!(streamed,storage::sha256(&serde_json::to_vec(&json!([header,["b",3],["a",4]])).unwrap()));
    }
}

#[cfg(test)]
#[path="xml_document_sharing_tests.rs"]
mod document_sharing_tests;
