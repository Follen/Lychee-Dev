use super::*;

fn owned_capacity(documents:&BTreeMap<String,ResearchDocument>)->usize{
    documents.values().map(|text|match text{ResearchDocument::Owned(text)=>text.capacity(),ResearchDocument::Shared(_)=>0}).sum()
}

#[test]
fn production_xml_documents_share_script_and_program_originals_without_changing_evidence(){
    let control=OperationControl::default();
    let mut documents=BTreeMap::<String,String>::from([
        ("One.toc".into(),"Frames.xml\nLoaded.xml\n".into()),
        ("Two.toc".into(),"Frames.xml\n".into()),
        ("Frames.xml".into(),"\u{feff}<Ui>\r\n<Frame name=\"Frame\"><Scripts><OnLoad>Target(self, &quot;😀&quot;)</OnLoad></Scripts></Frame></Ui>".into()),
        ("Loaded.xml".into(),"<Ui><Frame name=\"Loaded\"/></Ui>".into()),
        ("Unused.xml".into(),"<Ui><Frame name=\"Unused\" virtual=\"true\"/></Ui>".into()),
        ("Broken.xml".into(),"<Ui><Frame></Ui>".into()),
    ]);
    // Capacity rather than text length is the allocation actually eliminated.
    for text in documents.values_mut(){text.reserve(2048);}
    let original_allocations=documents.iter().map(|(path,text)|(path.clone(),(text.as_ptr(),text.capacity()))).collect::<BTreeMap<_,_>>();
    let expected=documents.clone();
    let plans=["One.toc","Two.toc"].map(|manifest|LoadEvidence{manifest:manifest.into(),ordered:vec![],issues:vec![]});
    let source=json!({"repository":"xml-document-sharing","product":"retail"});
    let mut files=BTreeMap::new();
    let mut xml=XmlResearch::prepare(&mut files,documents,&plans,&source,&control).unwrap();
    assert_eq!(xml.programs.len(),2);
    assert_eq!(xml.documents.keys().collect::<Vec<_>>(),expected.keys().collect::<Vec<_>>());
    for (path,text) in &xml.documents{assert_eq!(text.as_str(),expected[path],"{path}");}

    let ResearchDocument::Shared(frame)=&xml.documents["Frames.xml"]else{panic!("script original must be shared")};
    for script in xml.scripts.values(){
        assert!(Arc::ptr_eq(frame,&script.original_shared()));
        assert_eq!(files[&script.key].as_bytes(),script.source_bytes());
    }
    assert!(!xml.scripts.is_empty());
    for program in &xml.programs{assert_eq!(program.original("Frames.xml").unwrap().as_ptr(),frame.as_ptr());}
    let ResearchDocument::Shared(loaded)=&xml.documents["Loaded.xml"]else{panic!("load-only original must be shared")};
    assert_eq!(xml.programs[0].original("Loaded.xml").unwrap().as_ptr(),loaded.as_ptr());
    for path in ["One.toc","Two.toc","Unused.xml","Broken.xml"]{
        let ResearchDocument::Owned(text)=&xml.documents[path]else{panic!("{path} has no independently retained original")};
        assert_eq!((text.as_ptr(),text.capacity()),original_allocations[path],"unshared allocation must move unchanged");
    }
    let before=original_allocations.values().map(|(_,capacity)|*capacity).sum::<usize>();
    let saved=original_allocations["Frames.xml"].1+original_allocations["Loaded.xml"].1;
    assert_eq!(before-owned_capacity(&xml.documents),saved);
    assert!(saved>frame.len()+loaded.len(),"spare String capacity must also disappear");

    let identity=xml.identity(&control).unwrap();
    let shared_documents=std::mem::replace(&mut xml.documents,retain_documents(expected.clone(),BTreeMap::new()));
    assert_eq!(serde_json::to_vec(&xml.identity(&control).unwrap()).unwrap(),serde_json::to_vec(&identity).unwrap(),"the full identity, including maps and load contexts, must retain its bytes");
    xml.documents=shared_documents;
    let expected_digest=hash_sequence(&json!(["xml-original-documents-v1",xml_load::RULES,xml_inline::SOURCE_MAP_RULES]),expected.iter().map(|(path,text)|(path,storage::sha256(text.as_bytes()))),&control).unwrap();
    assert_eq!(xml.identity(&control).unwrap()["documentsSHA256"],expected_digest);
    let sources=expected.iter().map(|(path,text)|(path.as_str(),text.as_bytes())).collect::<BTreeMap<_,_>>();
    let paths=xml_load::LoadPaths::new(sources.keys().copied(),&control).unwrap();
    for (plan,program) in plans.iter().zip(&xml.programs){
        let original=xml_load::Program::from_indexed_sources(&plan.manifest,&sources,&paths,Some(&source),&control).unwrap();
        assert_eq!(serde_json::to_vec(program.as_ref()).unwrap(),serde_json::to_vec(&original).unwrap());
    }
    for (path,text) in &expected{
        if path=="Frames.xml"{
            let original=xml_inline::extract(path,text.as_bytes()).unwrap();
            for script in original{
                assert_eq!(xml.scripts[&script.key].mapping_manifest(),script.mapping_manifest());
                let range=Span::new(0,script.source_bytes().len());
                assert_eq!(xml.scripts[&script.key].position(range).unwrap(),script.position(range).unwrap());
            }
        }
    }
    assert_eq!(xml.gaps.len(),1);
    let failure=crate::xml_program::extract_production("Broken.xml",expected["Broken.xml"].as_bytes()).unwrap_err();
    assert_eq!(xml.gaps[0],json!({"code":"codebase.xml_invalid","detail":failure,"path":"Broken.xml"}));
}

#[test]
fn document_pool_fallback_preserves_unshared_and_mismatching_original_allocations(){
    let retained:Arc<str>=Arc::from("same");
    let foreign:Arc<str>=Arc::from("different");
    let lone:Arc<str>=Arc::from("lone");
    let mut documents=BTreeMap::<String,String>::from([
        ("shared.xml".into(),"same".into()),("mismatch.xml".into(),"original".into()),
        ("unused.toc".into(),"contents".into()),("lone.xml".into(),"lone".into()),
    ]);
    for text in documents.values_mut(){text.reserve(1024);}
    let pointers=documents.iter().map(|(path,text)|(path.clone(),(text.as_ptr(),text.capacity()))).collect::<BTreeMap<_,_>>();
    let pooled=BTreeMap::from([
        ("shared.xml".into(),Arc::clone(&retained)),("mismatch.xml".into(),Arc::clone(&foreign)),
        ("lone.xml".into(),lone),("not-admitted.xml".into(),Arc::from("unused")),
    ]);
    let actual=retain_documents(documents,pooled);
    assert_eq!(actual.len(),4);
    let ResearchDocument::Shared(text)=&actual["shared.xml"]else{panic!("existing owner must be reused")};
    assert!(Arc::ptr_eq(text,&retained));
    for path in ["mismatch.xml","unused.toc","lone.xml"]{
        let ResearchDocument::Owned(text)=&actual[path]else{panic!("{path} must keep its owned original")};
        assert_eq!((text.as_ptr(),text.capacity()),pointers[path]);
    }
    assert_eq!(actual["mismatch.xml"].as_str(),"original");
    assert_eq!(Arc::strong_count(&foreign),1,"unused pool owner must drop");
}

#[test]
fn shared_document_identity_preserves_original_cancelled_control(){
    let control=OperationControl::default();
    let xml=XmlResearch::prepare(&mut BTreeMap::new(),BTreeMap::from([("Frame.xml".into(),"<Ui><Script>Target()</Script></Ui>".into())]),&[],&json!({}),&control).unwrap();
    assert!(matches!(xml.documents["Frame.xml"],ResearchDocument::Shared(_)));
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(xml.identity(&control).unwrap_err().code,"command.cancelled");
}
